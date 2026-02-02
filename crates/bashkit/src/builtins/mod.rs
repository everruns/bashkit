//! Built-in shell commands

mod awk;
mod cat;
mod curl;
mod cuttr;
mod date;
mod echo;
mod export;
mod fileops;
mod flow;
mod grep;
mod headtail;
mod jq;
mod ls;
mod navigation;
mod path;
mod printf;
mod read;
mod sed;
mod sleep;
mod sortuniq;
mod source;
mod system;
mod test;
mod timeout;
mod vars;
mod wait;
mod wc;

pub use awk::Awk;
pub use cat::Cat;
pub use curl::{Curl, Wget};
pub use cuttr::{Cut, Tr};
pub use date::Date;
pub use echo::Echo;
pub use export::Export;
pub use fileops::{Chmod, Cp, Mkdir, Mv, Rm, Touch};
pub use flow::{Break, Continue, Exit, False, Return, True};
pub use grep::Grep;
pub use headtail::{Head, Tail};
pub use jq::Jq;
pub use ls::{Find, Ls, Rmdir};
pub use navigation::{Cd, Pwd};
pub use path::{Basename, Dirname};
pub use printf::Printf;
pub use read::Read;
pub use sed::Sed;
pub use sleep::Sleep;
pub use sortuniq::{Sort, Uniq};
pub use source::Source;
pub use system::{Hostname, Id, Uname, Whoami, DEFAULT_HOSTNAME, DEFAULT_USERNAME};
pub use test::{Bracket, Test};
pub use timeout::Timeout;
pub use vars::{Local, Set, Shift, Unset};
pub use wait::Wait;
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
