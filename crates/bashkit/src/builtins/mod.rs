//! Built-in shell commands

mod awk;
mod cat;
mod echo;
mod export;
mod fileops;
mod flow;
mod grep;
mod headtail;
mod jq;
mod navigation;
mod path;
mod printf;
mod read;
mod sed;
mod sleep;
mod source;
mod test;
mod vars;
mod wc;

pub use awk::Awk;
pub use cat::Cat;
pub use echo::Echo;
pub use export::Export;
pub use fileops::{Chmod, Cp, Mkdir, Mv, Rm, Touch};
pub use flow::{Break, Continue, Exit, False, Return, True};
pub use grep::Grep;
pub use headtail::{Head, Tail};
pub use jq::Jq;
pub use navigation::{Cd, Pwd};
pub use path::{Basename, Dirname};
pub use printf::Printf;
pub use read::Read;
pub use sed::Sed;
pub use sleep::Sleep;
pub use source::Source;
pub use test::{Bracket, Test};
pub use vars::{Local, Set, Shift, Unset};
pub use wc::Wc;

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
    /// Standard input (from pipeline)
    pub stdin: Option<&'a str>,
}

/// Trait for builtin commands.
#[async_trait]
pub trait Builtin: Send + Sync {
    /// Execute the builtin command.
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult>;
}
