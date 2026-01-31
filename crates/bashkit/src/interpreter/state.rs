//! Interpreter state types

/// Result of executing a bash script.
#[derive(Debug, Clone, Default)]
pub struct ExecResult {
    /// Standard output
    pub stdout: String,
    /// Standard error
    pub stderr: String,
    /// Exit code
    pub exit_code: i32,
}

impl ExecResult {
    /// Create a successful result with the given stdout.
    pub fn ok(stdout: impl Into<String>) -> Self {
        Self {
            stdout: stdout.into(),
            stderr: String::new(),
            exit_code: 0,
        }
    }

    /// Create a failed result with the given stderr.
    pub fn err(stderr: impl Into<String>, exit_code: i32) -> Self {
        Self {
            stdout: String::new(),
            stderr: stderr.into(),
            exit_code,
        }
    }

    /// Check if the result indicates success.
    pub fn is_success(&self) -> bool {
        self.exit_code == 0
    }
}
