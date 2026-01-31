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
