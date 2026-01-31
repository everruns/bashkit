//! BashKit - Sandboxed bash interpreter for multi-tenant environments
//!
//! Part of the Everruns ecosystem.
//!
//! # Example
//!
//! ```rust
//! use bashkit::Bash;
//!
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     let mut bash = Bash::new();
//!     let result = bash.exec("echo hello").await?;
//!     assert_eq!(result.stdout, "hello\n");
//!     assert_eq!(result.exit_code, 0);
//!     Ok(())
//! }
//! ```

mod builtins;
mod error;
mod fs;
mod interpreter;
mod parser;

pub use error::{Error, Result};
pub use interpreter::ExecResult;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use fs::{FileSystem, InMemoryFs};
use interpreter::Interpreter;
use parser::Parser;

/// Main entry point for BashKit.
///
/// Provides a sandboxed bash interpreter with an in-memory virtual filesystem.
pub struct Bash {
    #[allow(dead_code)] // Will be used for filesystem access methods
    fs: Arc<dyn FileSystem>,
    interpreter: Interpreter,
}

impl Default for Bash {
    fn default() -> Self {
        Self::new()
    }
}

impl Bash {
    /// Create a new Bash instance with default settings.
    pub fn new() -> Self {
        let fs: Arc<dyn FileSystem> = Arc::new(InMemoryFs::new());
        let interpreter = Interpreter::new(Arc::clone(&fs));
        Self { fs, interpreter }
    }

    /// Create a new BashBuilder for customized configuration.
    pub fn builder() -> BashBuilder {
        BashBuilder::default()
    }

    /// Execute a bash script and return the result.
    pub async fn exec(&mut self, script: &str) -> Result<ExecResult> {
        let parser = Parser::new(script);
        let ast = parser.parse()?;
        self.interpreter.execute(&ast).await
    }
}

/// Builder for customized Bash configuration.
#[derive(Default)]
pub struct BashBuilder {
    fs: Option<Arc<dyn FileSystem>>,
    env: HashMap<String, String>,
    cwd: Option<PathBuf>,
}

impl BashBuilder {
    /// Set a custom filesystem.
    pub fn fs(mut self, fs: Arc<dyn FileSystem>) -> Self {
        self.fs = Some(fs);
        self
    }

    /// Set an environment variable.
    pub fn env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.insert(key.into(), value.into());
        self
    }

    /// Set the current working directory.
    pub fn cwd(mut self, cwd: impl Into<PathBuf>) -> Self {
        self.cwd = Some(cwd.into());
        self
    }

    /// Build the Bash instance.
    pub fn build(self) -> Bash {
        let fs = self.fs.unwrap_or_else(|| Arc::new(InMemoryFs::new()));
        let mut interpreter = Interpreter::new(Arc::clone(&fs));

        for (key, value) in self.env {
            interpreter.set_env(&key, &value);
        }

        if let Some(cwd) = self.cwd {
            interpreter.set_cwd(cwd);
        }

        Bash { fs, interpreter }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_echo_hello() {
        let mut bash = Bash::new();
        let result = bash.exec("echo hello").await.unwrap();
        assert_eq!(result.stdout, "hello\n");
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn test_echo_multiple_args() {
        let mut bash = Bash::new();
        let result = bash.exec("echo hello world").await.unwrap();
        assert_eq!(result.stdout, "hello world\n");
        assert_eq!(result.exit_code, 0);
    }
}
