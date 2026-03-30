//! Interpreter state types

/// Control flow signals from commands like break, continue, return
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ControlFlow {
    #[default]
    None,
    /// Break out of a loop (with optional level count)
    Break(u32),
    /// Continue to next iteration (with optional level count)
    Continue(u32),
    /// Return from a function (with exit code)
    Return(i32),
}

/// Structured side-effect channel for builtins that need to communicate
/// state changes back to the interpreter.
///
/// Used only for state with invariants that builtins can't enforce directly:
/// - Arrays: need memory budget checking via `insert_array_checked`
/// - Positional params: stored on the call stack, not in Context
/// - History: needs VFS persistence via `save_history`
/// - Exit code: interpreter tracks `last_exit_code` separately
///
/// Simple state (aliases, traps) is mutated directly via [`ShellRef`].
#[derive(Debug, Clone)]
pub enum BuiltinSideEffect {
    /// Shift N positional parameters (replaces `_SHIFT_COUNT`).
    ShiftPositional(usize),
    /// Replace all positional parameters (replaces `_SET_POSITIONAL`).
    SetPositional(Vec<String>),
    /// Populate an indexed array variable (replaces `_ARRAY_READ_*`).
    SetArray { name: String, elements: Vec<String> },
    /// Populate an indexed array with index->value pairs (for mapfile).
    SetIndexedArray {
        name: String,
        entries: Vec<(usize, String)>,
    },
    /// Remove an indexed array.
    RemoveArray(String),
    /// Clear command history (interpreter persists to VFS).
    ClearHistory,
    /// Set the last exit code (for wait builtin).
    SetLastExitCode(i32),
    /// Set a shell variable (respects local scoping via `set_variable`).
    SetVariable { name: String, value: String },
}

/// Result of executing a bash script.
#[derive(Debug, Clone, Default)]
pub struct ExecResult {
    /// Standard output
    pub stdout: String,
    /// Standard error
    pub stderr: String,
    /// Exit code
    pub exit_code: i32,
    /// Control flow signal (break, continue, return)
    pub control_flow: ControlFlow,
    /// Whether stdout was truncated due to output size limits
    pub stdout_truncated: bool,
    /// Whether stderr was truncated due to output size limits
    pub stderr_truncated: bool,
    /// Final environment state after execution (opt-in via `capture_final_env`)
    pub final_env: Option<std::collections::HashMap<String, String>>,
    /// Structured trace events (empty when `TraceMode::Off`).
    pub events: Vec<crate::trace::TraceEvent>,
    /// Structured side effects from builtin execution.
    /// The interpreter processes these after the builtin returns.
    pub side_effects: Vec<BuiltinSideEffect>,
    /// When true, the non-zero exit code came from an AND-OR list (e.g. `false && true`)
    /// and should NOT trigger `set -e` / errexit at the caller level.
    /// Propagated through compound commands so nested loops don't re-trigger errexit.
    pub errexit_suppressed: bool,
}

impl ExecResult {
    /// Create a successful result with the given stdout.
    pub fn ok(stdout: impl Into<String>) -> Self {
        Self {
            stdout: stdout.into(),
            stderr: String::new(),
            exit_code: 0,
            ..Default::default()
        }
    }

    /// Create a failed result with the given stderr.
    pub fn err(stderr: impl Into<String>, exit_code: i32) -> Self {
        Self {
            stdout: String::new(),
            stderr: stderr.into(),
            exit_code,
            ..Default::default()
        }
    }

    /// Create a result with stdout and custom exit code.
    pub fn with_code(stdout: impl Into<String>, exit_code: i32) -> Self {
        Self {
            stdout: stdout.into(),
            stderr: String::new(),
            exit_code,
            ..Default::default()
        }
    }

    /// Create a result with a control flow signal
    pub fn with_control_flow(control_flow: ControlFlow) -> Self {
        Self {
            control_flow,
            ..Default::default()
        }
    }

    /// Check if the result indicates success.
    pub fn is_success(&self) -> bool {
        self.exit_code == 0
    }
}

/// Action for the caller's loop after processing a loop body iteration.
pub(crate) enum LoopAction {
    /// Continue normally (no control flow signal).
    None,
    /// Break out of the current loop.
    Break,
    /// Continue to next iteration of the current loop.
    Continue,
    /// Exit the loop immediately and return this result to the caller.
    /// Used for multi-level break/continue propagation and return.
    Exit(ExecResult),
}

/// Accumulates stdout/stderr/exit_code/errexit_suppressed across loop
/// iterations and handles break/continue/return control flow propagation.
///
/// Eliminates duplicated tracking in for, arithmetic-for, and while/until loops.
pub(crate) struct LoopAccumulator {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    pub errexit_suppressed: bool,
}

impl LoopAccumulator {
    pub fn new() -> Self {
        Self {
            stdout: String::new(),
            stderr: String::new(),
            exit_code: 0,
            errexit_suppressed: false,
        }
    }

    /// Accumulate a loop body result and classify the control flow action.
    ///
    /// Appends stdout/stderr, updates exit_code and errexit_suppressed.
    /// For multi-level break/continue or return, builds a propagation
    /// `ExecResult` and returns `LoopAction::Exit`.
    pub fn accumulate(&mut self, result: ExecResult) -> LoopAction {
        self.stdout.push_str(&result.stdout);
        self.stderr.push_str(&result.stderr);
        self.exit_code = result.exit_code;
        self.errexit_suppressed = result.errexit_suppressed;

        match result.control_flow {
            ControlFlow::Break(n) if n <= 1 => LoopAction::Break,
            ControlFlow::Break(n) => LoopAction::Exit(self.build_exit(ControlFlow::Break(n - 1))),
            ControlFlow::Continue(n) if n <= 1 => LoopAction::Continue,
            ControlFlow::Continue(n) => {
                LoopAction::Exit(self.build_exit(ControlFlow::Continue(n - 1)))
            }
            ControlFlow::Return(code) => {
                LoopAction::Exit(self.build_exit(ControlFlow::Return(code)))
            }
            ControlFlow::None => LoopAction::None,
        }
    }

    /// Consume into a final `ExecResult` with `ControlFlow::None`.
    pub fn finish(self) -> ExecResult {
        ExecResult {
            stdout: self.stdout,
            stderr: self.stderr,
            exit_code: self.exit_code,
            control_flow: ControlFlow::None,
            errexit_suppressed: self.errexit_suppressed,
            ..Default::default()
        }
    }

    /// Build an exit result, draining accumulated stdout/stderr.
    fn build_exit(&mut self, control_flow: ControlFlow) -> ExecResult {
        let exit_code = match control_flow {
            ControlFlow::Return(code) => code,
            _ => self.exit_code,
        };
        ExecResult {
            stdout: std::mem::take(&mut self.stdout),
            stderr: std::mem::take(&mut self.stderr),
            exit_code,
            control_flow,
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- ControlFlow ---

    #[test]
    fn control_flow_default_is_none() {
        assert_eq!(ControlFlow::default(), ControlFlow::None);
    }

    #[test]
    fn control_flow_break_stores_level() {
        let cf = ControlFlow::Break(2);
        assert_eq!(cf, ControlFlow::Break(2));
        assert_ne!(cf, ControlFlow::Break(1));
    }

    #[test]
    fn control_flow_continue_stores_level() {
        let cf = ControlFlow::Continue(3);
        assert_eq!(cf, ControlFlow::Continue(3));
    }

    #[test]
    fn control_flow_return_stores_code() {
        let cf = ControlFlow::Return(42);
        assert_eq!(cf, ControlFlow::Return(42));
    }

    #[test]
    fn control_flow_variants_not_equal() {
        assert_ne!(ControlFlow::None, ControlFlow::Break(0));
        assert_ne!(ControlFlow::Break(1), ControlFlow::Continue(1));
        assert_ne!(ControlFlow::Continue(1), ControlFlow::Return(1));
    }

    #[test]
    fn control_flow_clone() {
        let cf = ControlFlow::Return(5);
        let cloned = cf;
        assert_eq!(cf, cloned);
    }

    // --- ExecResult::ok ---

    #[test]
    fn exec_result_ok_sets_stdout() {
        let r = ExecResult::ok("hello");
        assert_eq!(r.stdout, "hello");
        assert_eq!(r.stderr, "");
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.control_flow, ControlFlow::None);
        assert!(!r.stdout_truncated);
        assert!(!r.stderr_truncated);
    }

    #[test]
    fn exec_result_ok_empty_string() {
        let r = ExecResult::ok("");
        assert_eq!(r.stdout, "");
        assert!(r.is_success());
    }

    #[test]
    fn exec_result_ok_accepts_string() {
        let s = String::from("owned");
        let r = ExecResult::ok(s);
        assert_eq!(r.stdout, "owned");
    }

    // --- ExecResult::err ---

    #[test]
    fn exec_result_err_sets_stderr_and_code() {
        let r = ExecResult::err("bad command", 127);
        assert_eq!(r.stdout, "");
        assert_eq!(r.stderr, "bad command");
        assert_eq!(r.exit_code, 127);
        assert_eq!(r.control_flow, ControlFlow::None);
    }

    #[test]
    fn exec_result_err_is_not_success() {
        let r = ExecResult::err("fail", 1);
        assert!(!r.is_success());
    }

    #[test]
    fn exec_result_err_with_code_zero_is_success() {
        // Edge case: err constructor with exit_code 0
        let r = ExecResult::err("warning", 0);
        assert!(r.is_success());
    }

    // --- ExecResult::with_code ---

    #[test]
    fn exec_result_with_code_sets_stdout_and_code() {
        let r = ExecResult::with_code("partial", 2);
        assert_eq!(r.stdout, "partial");
        assert_eq!(r.stderr, "");
        assert_eq!(r.exit_code, 2);
        assert_eq!(r.control_flow, ControlFlow::None);
    }

    #[test]
    fn exec_result_with_code_zero() {
        let r = ExecResult::with_code("ok", 0);
        assert!(r.is_success());
    }

    #[test]
    fn exec_result_with_code_negative() {
        let r = ExecResult::with_code("", -1);
        assert!(!r.is_success());
        assert_eq!(r.exit_code, -1);
    }

    // --- ExecResult::with_control_flow ---

    #[test]
    fn exec_result_with_control_flow_break() {
        let r = ExecResult::with_control_flow(ControlFlow::Break(1));
        assert_eq!(r.stdout, "");
        assert_eq!(r.stderr, "");
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.control_flow, ControlFlow::Break(1));
    }

    #[test]
    fn exec_result_with_control_flow_continue() {
        let r = ExecResult::with_control_flow(ControlFlow::Continue(1));
        assert_eq!(r.control_flow, ControlFlow::Continue(1));
    }

    #[test]
    fn exec_result_with_control_flow_return() {
        let r = ExecResult::with_control_flow(ControlFlow::Return(0));
        assert_eq!(r.control_flow, ControlFlow::Return(0));
    }

    #[test]
    fn exec_result_with_control_flow_none() {
        let r = ExecResult::with_control_flow(ControlFlow::None);
        assert_eq!(r.control_flow, ControlFlow::None);
        assert!(r.is_success());
    }

    // --- ExecResult::is_success ---

    #[test]
    fn exec_result_is_success_true_for_zero() {
        let r = ExecResult::ok("x");
        assert!(r.is_success());
    }

    #[test]
    fn exec_result_is_success_false_for_nonzero() {
        let r = ExecResult::err("x", 1);
        assert!(!r.is_success());
        let r2 = ExecResult::with_code("", 255);
        assert!(!r2.is_success());
    }

    // --- ExecResult::default ---

    #[test]
    fn exec_result_default() {
        let r = ExecResult::default();
        assert_eq!(r.stdout, "");
        assert_eq!(r.stderr, "");
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.control_flow, ControlFlow::None);
        assert!(!r.stdout_truncated);
        assert!(!r.stderr_truncated);
        assert!(r.final_env.is_none());
        assert!(r.is_success());
    }

    // --- Debug ---

    #[test]
    fn exec_result_debug_format() {
        let r = ExecResult::ok("test");
        let dbg = format!("{:?}", r);
        assert!(dbg.contains("ExecResult"));
        assert!(dbg.contains("test"));
    }

    #[test]
    fn control_flow_debug_format() {
        let cf = ControlFlow::Break(3);
        let dbg = format!("{:?}", cf);
        assert!(dbg.contains("Break"));
        assert!(dbg.contains("3"));
    }

    // --- LoopAccumulator ---

    #[test]
    fn loop_acc_accumulate_none() {
        let mut acc = LoopAccumulator::new();
        let r = ExecResult {
            stdout: "out".into(),
            stderr: "err".into(),
            exit_code: 2,
            errexit_suppressed: true,
            ..Default::default()
        };
        assert!(matches!(acc.accumulate(r), LoopAction::None));
        assert_eq!(acc.stdout, "out");
        assert_eq!(acc.stderr, "err");
        assert_eq!(acc.exit_code, 2);
        assert!(acc.errexit_suppressed);
    }

    #[test]
    fn loop_acc_accumulate_break_level_1() {
        let mut acc = LoopAccumulator::new();
        let r = ExecResult {
            control_flow: ControlFlow::Break(1),
            ..Default::default()
        };
        assert!(matches!(acc.accumulate(r), LoopAction::Break));
    }

    #[test]
    fn loop_acc_accumulate_break_level_3() {
        let mut acc = LoopAccumulator::new();
        acc.stdout.push_str("prev ");
        let r = ExecResult {
            stdout: "body".into(),
            control_flow: ControlFlow::Break(3),
            ..Default::default()
        };
        match acc.accumulate(r) {
            LoopAction::Exit(result) => {
                assert_eq!(result.control_flow, ControlFlow::Break(2));
                assert_eq!(result.stdout, "prev body");
            }
            other => panic!("expected Exit, got {:?}", std::mem::discriminant(&other)),
        }
    }

    #[test]
    fn loop_acc_accumulate_continue_level_1() {
        let mut acc = LoopAccumulator::new();
        let r = ExecResult {
            control_flow: ControlFlow::Continue(1),
            ..Default::default()
        };
        assert!(matches!(acc.accumulate(r), LoopAction::Continue));
    }

    #[test]
    fn loop_acc_accumulate_return() {
        let mut acc = LoopAccumulator::new();
        let r = ExecResult {
            control_flow: ControlFlow::Return(42),
            ..Default::default()
        };
        match acc.accumulate(r) {
            LoopAction::Exit(result) => {
                assert_eq!(result.control_flow, ControlFlow::Return(42));
                assert_eq!(result.exit_code, 42);
            }
            other => panic!("expected Exit, got {:?}", std::mem::discriminant(&other)),
        }
    }

    #[test]
    fn loop_acc_finish() {
        let mut acc = LoopAccumulator::new();
        let r1 = ExecResult {
            stdout: "a".into(),
            exit_code: 1,
            errexit_suppressed: true,
            ..Default::default()
        };
        acc.accumulate(r1);
        let r2 = ExecResult {
            stdout: "b".into(),
            exit_code: 0,
            errexit_suppressed: false,
            ..Default::default()
        };
        acc.accumulate(r2);
        let final_result = acc.finish();
        assert_eq!(final_result.stdout, "ab");
        assert_eq!(final_result.exit_code, 0);
        assert!(!final_result.errexit_suppressed);
        assert_eq!(final_result.control_flow, ControlFlow::None);
    }
}
