//! Built-in shell commands

mod echo;
mod flow;
mod navigation;

pub use echo::Echo;
pub use flow::{Exit, False, True};
pub use navigation::{Cd, Pwd};

use async_trait::async_trait;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use crate::error::Result;
use crate::fs::FileSystem;
use crate::interpreter::ExecResult;

/// Context for builtin command execution.
pub struct Context<'a> {
    /// Command arguments (not including the command name)
    pub args: &'a [String],
    /// Environment variables
    pub env: &'a HashMap<String, String>,
    /// Shell variables
    #[allow(dead_code)] // Will be used by set, export, declare builtins
    pub variables: &'a mut HashMap<String, String>,
    /// Current working directory
    pub cwd: &'a mut PathBuf,
    /// Filesystem
    pub fs: Arc<dyn FileSystem>,
}

/// Trait for builtin commands.
#[async_trait]
pub trait Builtin: Send + Sync {
    /// Execute the builtin command.
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult>;
}
