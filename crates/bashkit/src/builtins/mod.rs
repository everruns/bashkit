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
mod column;
mod comm;
mod curl;
mod cuttr;
mod date;
mod diff;
mod disk;
mod echo;
mod environ;
mod export;
mod expr;
mod fileops;
mod flow;
mod grep;
mod headtail;
mod hextools;
mod inspect;
mod jq;
mod ls;
mod navigation;
mod nl;
mod paste;
mod path;
mod pipeline;
mod printf;
mod read;
mod sed;
mod seq;
mod sleep;
mod sortuniq;
mod source;
mod strings;
mod system;
mod test;
mod textrev;
mod timeout;
mod vars;
mod wait;
mod wc;
mod yes;

#[cfg(feature = "git")]
mod git;

#[cfg(feature = "python")]
mod python;

pub use archive::{Gunzip, Gzip, Tar};
pub use awk::Awk;
pub use cat::Cat;
pub use column::Column;
pub use comm::Comm;
pub use curl::{Curl, Wget};
pub use cuttr::{Cut, Tr};
pub use date::Date;
pub use diff::Diff;
pub use disk::{Df, Du};
pub use echo::Echo;
pub use environ::{Env, History, Printenv};
pub use export::Export;
pub use expr::Expr;
pub use fileops::{Chmod, Chown, Cp, Kill, Ln, Mkdir, Mktemp, Mv, Rm, Touch};
pub use flow::{Break, Colon, Continue, Exit, False, Return, True};
pub use grep::Grep;
pub use headtail::{Head, Tail};
pub use hextools::{Hexdump, Od, Xxd};
pub use inspect::{File, Less, Stat};
pub use jq::Jq;
pub use ls::{Find, Ls, Rmdir};
pub use navigation::{Cd, Pwd};
pub use nl::Nl;
pub use paste::Paste;
pub use path::{Basename, Dirname, Realpath};
pub use pipeline::{Tee, Watch, Xargs};
pub use printf::Printf;
pub use read::Read;
pub use sed::Sed;
pub use seq::Seq;
pub use sleep::Sleep;
pub use sortuniq::{Sort, Uniq};
pub use source::Source;
pub use strings::Strings;
pub use system::{Hostname, Id, Uname, Whoami, DEFAULT_HOSTNAME, DEFAULT_USERNAME};
pub use test::{Bracket, Test};
pub use textrev::{Rev, Tac};
pub use timeout::Timeout;
pub use vars::{Eval, Local, Readonly, Set, Shift, Shopt, Times, Unset};
pub use wait::Wait;
pub use wc::Wc;
pub use yes::Yes;

#[cfg(feature = "git")]
pub use git::Git;

#[cfg(feature = "python")]
pub use python::{Python, PythonLimits};

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
///
/// // Paths are normalized (. and .. resolved)
/// let dot = resolve_path(Path::new("/"), ".");
/// assert_eq!(dot, PathBuf::from("/"));
/// ```
pub fn resolve_path(cwd: &Path, path_str: &str) -> PathBuf {
    let path = Path::new(path_str);
    let joined = if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(path)
    };
    // Normalize the path to handle . and .. components
    normalize_path(&joined)
}

/// Normalize a path by resolving `.` and `..` components.
///
/// This ensures paths like `/.` become `/` and `/tmp/../home` becomes `/home`.
/// Used internally to ensure filesystem implementations receive clean paths.
fn normalize_path(path: &Path) -> PathBuf {
    use std::path::Component;

    let mut result = PathBuf::new();

    for component in path.components() {
        match component {
            Component::RootDir => {
                result.push("/");
            }
            Component::Normal(name) => {
                result.push(name);
            }
            Component::ParentDir => {
                result.pop();
            }
            Component::CurDir => {
                // Skip . components
            }
            Component::Prefix(_) => {
                // Windows prefix, ignore
            }
        }
    }

    // Ensure we return "/" for empty result (e.g., from "/..")
    if result.as_os_str().is_empty() {
        result.push("/");
    }

    result
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

    /// Git client for git operations.
    ///
    /// Only available when the `git` feature is enabled and
    /// a [`GitConfig`](crate::GitConfig) is configured via
    /// [`BashBuilder::git`](crate::BashBuilder::git).
    #[cfg(feature = "git")]
    pub git_client: Option<&'a crate::git::GitClient>,
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
            #[cfg(feature = "git")]
            git_client: None,
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
/// # LLM Hints
///
/// Builtins can provide short hints for LLM system prompts via [`llm_hint`](Builtin::llm_hint).
/// These appear in the tool's `help()` and `system_prompt()` output so LLMs know
/// about capabilities and limitations.
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

    /// Optional short hint for LLM system prompts.
    ///
    /// Return a concise one-line description of capabilities and limitations.
    /// These hints are included in `help()` and `system_prompt()` output
    /// when the builtin is registered.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// fn llm_hint(&self) -> Option<&'static str> {
    ///     Some("mycommand: Processes data files. Max 10MB input. No network access.")
    /// }
    /// ```
    fn llm_hint(&self) -> Option<&'static str> {
        None
    }
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

    #[test]
    fn test_resolve_path_dot_from_root() {
        // "." from root should normalize to "/"
        let cwd = PathBuf::from("/");
        let result = resolve_path(&cwd, ".");
        assert_eq!(result, PathBuf::from("/"));
    }

    #[test]
    fn test_resolve_path_dot_from_normal_dir() {
        // "." should be stripped, returning the cwd itself
        let cwd = PathBuf::from("/home/user");
        let result = resolve_path(&cwd, ".");
        assert_eq!(result, PathBuf::from("/home/user"));
    }

    #[test]
    fn test_resolve_path_dotdot() {
        // ".." should go up one directory
        let cwd = PathBuf::from("/home/user");
        let result = resolve_path(&cwd, "..");
        assert_eq!(result, PathBuf::from("/home"));
    }

    #[test]
    fn test_resolve_path_dotdot_from_root() {
        // ".." from root stays at root
        let cwd = PathBuf::from("/");
        let result = resolve_path(&cwd, "..");
        assert_eq!(result, PathBuf::from("/"));
    }

    #[test]
    fn test_resolve_path_complex() {
        // Complex path with . and ..
        let cwd = PathBuf::from("/home/user");
        let result = resolve_path(&cwd, "./downloads/../documents/./file.txt");
        assert_eq!(result, PathBuf::from("/home/user/documents/file.txt"));
    }
}
