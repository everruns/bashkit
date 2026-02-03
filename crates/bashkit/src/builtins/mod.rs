//! Built-in shell commands
//!
//! This module provides the [`Builtin`] trait for implementing custom commands
//! and the [`Context`] struct for execution context.
//!
//! # Custom Builtins
//!
//! Implement the [`Builtin`] trait to create custom commands:
//!
//! ```rust
//! use bashkit::{Builtin, BuiltinContext, ExecResult, async_trait};
//!
//! struct MyCommand;
//!
//! #[async_trait]
//! impl Builtin for MyCommand {
//!     async fn execute(&self, ctx: BuiltinContext<'_>) -> bashkit::Result<ExecResult> {
//!         Ok(ExecResult::ok("Hello!\n".to_string()))
//!     }
//! }
//! ```
//!
//! Register via [`BashBuilder::builtin`](crate::BashBuilder::builtin).

mod archive;
mod awk;
mod cat;
mod curl;
mod cuttr;
mod date;
mod echo;
mod environ;
mod export;
mod fileops;
mod flow;
mod grep;
mod headtail;
mod inspect;
mod jq;
mod ls;
mod navigation;
mod path;
mod pipeline;
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

pub use archive::{Gunzip, Gzip, Tar};
pub use awk::Awk;
pub use cat::Cat;
pub use curl::{Curl, Wget};
pub use cuttr::{Cut, Tr};
pub use date::Date;
pub use echo::Echo;
pub use environ::{Env, History, Printenv};
pub use export::Export;
pub use fileops::{Chmod, Cp, Mkdir, Mv, Rm, Touch};
pub use flow::{Break, Colon, Continue, Exit, False, Return, True};
pub use grep::Grep;
pub use headtail::{Head, Tail};
pub use inspect::{File, Less, Stat};
pub use jq::Jq;
pub use ls::{Find, Ls, Rmdir};
pub use navigation::{Cd, Pwd};
pub use path::{Basename, Dirname};
pub use pipeline::{Tee, Watch, Xargs};
pub use printf::Printf;
pub use read::Read;
pub use sed::Sed;
pub use sleep::Sleep;
pub use sortuniq::{Sort, Uniq};
pub use source::Source;
pub use system::{Hostname, Id, Uname, Whoami, DEFAULT_HOSTNAME, DEFAULT_USERNAME};
pub use test::{Bracket, Test};
pub use timeout::Timeout;
pub use vars::{Eval, Local, Readonly, Set, Shift, Times, Unset};
pub use wait::Wait;
pub use wc::Wc;

use async_trait::async_trait;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::error::Result;
use crate::fs::FileSystem;
use crate::interpreter::ExecResult;

/// Resolve a path relative to the current working directory.
///
/// If the path is absolute, returns it unchanged.
/// If relative, joins it with the cwd.
///
/// # Example
///
/// ```ignore
/// let abs = resolve_path(Path::new("/home"), "/etc/passwd");
/// assert_eq!(abs, PathBuf::from("/etc/passwd"));
///
/// let rel = resolve_path(Path::new("/home"), "file.txt");
/// assert_eq!(rel, PathBuf::from("/home/file.txt"));
/// ```
pub fn resolve_path(cwd: &Path, path_str: &str) -> PathBuf {
    let path = Path::new(path_str);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(path)
    }
}

/// Execution context for builtin commands.
///
/// Provides access to the shell execution environment including arguments,
/// variables, filesystem, and pipeline input.
///
/// # Example
///
/// ```rust
/// use bashkit::{Builtin, BuiltinContext, ExecResult, async_trait};
///
/// struct Echo;
///
/// #[async_trait]
/// impl Builtin for Echo {
///     async fn execute(&self, ctx: BuiltinContext<'_>) -> bashkit::Result<ExecResult> {
///         // Access command arguments
///         let output = ctx.args.join(" ");
///
///         // Access environment variables
///         let _home = ctx.env.get("HOME");
///
///         // Access pipeline input
///         if let Some(stdin) = ctx.stdin {
///             return Ok(ExecResult::ok(stdin.to_string()));
///         }
///
///         Ok(ExecResult::ok(format!("{}\n", output)))
///     }
/// }
/// ```
pub struct Context<'a> {
    /// Command arguments (not including the command name).
    ///
    /// For `mycommand arg1 arg2`, this contains `["arg1", "arg2"]`.
    pub args: &'a [String],

    /// Environment variables.
    ///
    /// Read-only access to variables set via [`BashBuilder::env`](crate::BashBuilder::env)
    /// or the `export` builtin.
    pub env: &'a HashMap<String, String>,

    /// Shell variables (mutable).
    ///
    /// Allows builtins to set or modify shell variables.
    #[allow(dead_code)] // Will be used by set, export, declare builtins
    pub variables: &'a mut HashMap<String, String>,

    /// Current working directory (mutable).
    ///
    /// Used by `cd` and path resolution.
    pub cwd: &'a mut PathBuf,

    /// Virtual filesystem.
    ///
    /// Provides async file operations (read, write, mkdir, etc.).
    pub fs: Arc<dyn FileSystem>,

    /// Standard input from pipeline.
    ///
    /// Contains output from the previous command in a pipeline.
    /// For `echo hello | mycommand`, stdin will be `Some("hello\n")`.
    pub stdin: Option<&'a str>,

    /// HTTP client for network operations (curl, wget).
    ///
    /// Only available when the `network` feature is enabled and
    /// a [`NetworkAllowlist`](crate::NetworkAllowlist) is configured via
    /// [`BashBuilder::network`](crate::BashBuilder::network).
    #[cfg(feature = "http_client")]
    pub http_client: Option<&'a crate::network::HttpClient>,
}

impl<'a> Context<'a> {
    /// Create a new Context for testing purposes.
    ///
    /// This helper handles the conditional `http_client` field automatically.
    #[cfg(test)]
    pub fn new_for_test(
        args: &'a [String],
        env: &'a std::collections::HashMap<String, String>,
        variables: &'a mut std::collections::HashMap<String, String>,
        cwd: &'a mut std::path::PathBuf,
        fs: std::sync::Arc<dyn crate::fs::FileSystem>,
        stdin: Option<&'a str>,
    ) -> Self {
        Self {
            args,
            env,
            variables,
            cwd,
            fs,
            stdin,
            #[cfg(feature = "http_client")]
            http_client: None,
        }
    }
}

/// Trait for implementing builtin commands.
///
/// All custom builtins must implement this trait. The trait requires `Send + Sync`
/// for thread safety in async contexts.
///
/// # Example
///
/// ```rust
/// use bashkit::{Bash, Builtin, BuiltinContext, ExecResult, async_trait};
///
/// struct Greet {
///     default_name: String,
/// }
///
/// #[async_trait]
/// impl Builtin for Greet {
///     async fn execute(&self, ctx: BuiltinContext<'_>) -> bashkit::Result<ExecResult> {
///         let name = ctx.args.first()
///             .map(|s| s.as_str())
///             .unwrap_or(&self.default_name);
///         Ok(ExecResult::ok(format!("Hello, {}!\n", name)))
///     }
/// }
///
/// // Register the builtin
/// let bash = Bash::builder()
///     .builtin("greet", Box::new(Greet { default_name: "World".into() }))
///     .build();
/// ```
///
/// # Return Values
///
/// Return [`ExecResult::ok`](crate::ExecResult::ok) for success with output,
/// or [`ExecResult::err`](crate::ExecResult::err) for errors with exit code.
#[async_trait]
pub trait Builtin: Send + Sync {
    /// Execute the builtin command.
    ///
    /// # Arguments
    ///
    /// * `ctx` - The execution context containing arguments, environment, and filesystem
    ///
    /// # Returns
    ///
    /// * `Ok(ExecResult)` - Execution result with stdout, stderr, and exit code
    /// * `Err(Error)` - Fatal error that should abort execution
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_path_absolute() {
        let cwd = PathBuf::from("/home/user");
        let result = resolve_path(&cwd, "/tmp/file.txt");
        assert_eq!(result, PathBuf::from("/tmp/file.txt"));
    }

    #[test]
    fn test_resolve_path_relative() {
        let cwd = PathBuf::from("/home/user");
        let result = resolve_path(&cwd, "downloads/file.txt");
        assert_eq!(result, PathBuf::from("/home/user/downloads/file.txt"));
    }
}
