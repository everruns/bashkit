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
}

impl ExecResult {
    /// Create a successful result with the given stdout.
    pub fn ok(stdout: impl Into<String>) -> Self {
        Self {
            stdout: stdout.into(),
            stderr: String::new(),
            exit_code: 0,
            control_flow: ControlFlow::None,
        }
    }

    /// Create a failed result with the given stderr.
    pub fn err(stderr: impl Into<String>, exit_code: i32) -> Self {
        Self {
            stdout: String::new(),
            stderr: stderr.into(),
            exit_code,
            control_flow: ControlFlow::None,
        }
    }

    /// Create a result with stdout and custom exit code.
    pub fn with_code(stdout: impl Into<String>, exit_code: i32) -> Self {
        Self {
            stdout: stdout.into(),
            stderr: String::new(),
            exit_code,
            control_flow: ControlFlow::None,
        }
    }

    /// Create a result with a control flow signal
    pub fn with_control_flow(control_flow: ControlFlow) -> Self {
        Self {
            stdout: String::new(),
            stderr: String::new(),
            exit_code: 0,
            control_flow,
        }
    }

    /// Check if the result indicates success.
    pub fn is_success(&self) -> bool {
        self.exit_code == 0
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
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
}
