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

mod alias;
mod archive;
pub(crate) mod arg_parser;
mod assert;
mod awk;
mod base64;
mod bc;
mod caller;
mod cat;
mod checksum;
mod clear;
mod column;
mod comm;
mod compgen;
mod csv;
mod curl;
mod cuttr;
mod date;
mod diff;
mod dirstack;
mod disk;
mod dotenv;
mod echo;
mod environ;
mod envsubst;
mod expand;
mod export;
mod expr;
mod fc;
mod fileops;
mod flow;
mod fold;
mod glob_cmd;
mod grep;
mod headtail;
mod help;
mod hextools;
mod http;
mod iconv;
mod inspect;
mod introspect;
mod join;
#[cfg(feature = "jq")]
mod jq;
mod json;
mod log;
mod ls;
mod mapfile;
mod mkfifo;
mod navigation;
mod nl;
mod numfmt;
mod parallel;
mod paste;
mod patch;
mod path;
mod pipeline;
mod printf;
mod read;
mod retry;
mod rg;
pub(crate) mod search_common;
mod sed;
mod semver;
mod seq;
mod sleep;
mod sortuniq;
mod source;
mod split;
mod strings;
mod system;
mod template;
mod test;
mod textrev;
pub(crate) mod timeout;
mod tomlq;
mod trap;
mod tree;
mod vars;
mod verify;
mod wait;
mod wc;
mod yaml;
mod yes;
mod zip_cmd;

/// Max width/precision for printf-style format specifiers to prevent memory exhaustion.
/// Shared by printf and AWK builtins.
pub(crate) const MAX_FORMAT_WIDTH: usize = 10_000;

#[cfg(feature = "git")]
mod git;

#[cfg(feature = "ssh")]
mod ssh;

#[cfg(feature = "python")]
mod python;

#[cfg(feature = "typescript")]
mod typescript;

pub use alias::{Alias, Unalias};
pub use archive::{Gunzip, Gzip, Tar};
pub use assert::Assert;
pub use awk::Awk;
pub use base64::Base64;
pub use bc::Bc;
pub use caller::Caller;
pub use cat::Cat;
pub use checksum::{Md5sum, Sha1sum, Sha256sum};
pub use clear::Clear;
pub use column::Column;
pub use comm::Comm;
pub use compgen::Compgen;
pub use csv::Csv;
pub use curl::{Curl, Wget};
pub use cuttr::{Cut, Tr};
pub use date::Date;
pub use diff::Diff;
pub use dirstack::{Dirs, Popd, Pushd};
pub use disk::{Df, Du};
pub use dotenv::Dotenv;
pub use echo::Echo;
pub use environ::{Env, History, Printenv};
pub use envsubst::Envsubst;
pub use expand::{Expand, Unexpand};
pub use export::Export;
pub use expr::Expr;
pub use fc::Fc;
pub use fileops::{Chmod, Chown, Cp, Kill, Ln, Mkdir, Mktemp, Mv, Rm, Touch};
pub use flow::{Break, Colon, Continue, Exit, False, Return, True};
pub use fold::Fold;
pub use glob_cmd::GlobCmd;
pub use grep::Grep;
pub use headtail::{Head, Tail};
pub use help::Help;
pub use hextools::{Hexdump, Od, Xxd};
pub use http::Http;
pub use iconv::Iconv;
pub use inspect::{File, Less, Stat};
pub use introspect::{Hash, Type, Which};
pub use join::Join;
#[cfg(feature = "jq")]
pub use jq::Jq;
pub use json::Json;
pub use log::Log;
pub(crate) use ls::glob_match;
pub use ls::{Find, Ls, Rmdir};
pub use mapfile::Mapfile;
pub use mkfifo::Mkfifo;
pub use navigation::{Cd, Pwd};
pub use nl::Nl;
pub use numfmt::Numfmt;
pub use parallel::Parallel;
pub use paste::Paste;
pub use patch::Patch;
pub use path::{Basename, Dirname, Readlink, Realpath};
pub use pipeline::{Tee, Watch, Xargs};
pub use printf::Printf;
pub use read::Read;
pub use retry::Retry;
pub use rg::Rg;
pub use sed::Sed;
pub use semver::Semver;
pub use seq::Seq;
pub use sleep::Sleep;
pub use sortuniq::{Sort, Uniq};
pub use source::Source;
pub use split::Split;
pub use strings::Strings;
pub use system::{DEFAULT_HOSTNAME, DEFAULT_USERNAME, Hostname, Id, Uname, Whoami};
pub use template::Template;
pub use test::{Bracket, Test};
pub use textrev::{Rev, Tac};
pub use timeout::Timeout;
pub use tomlq::Tomlq;
pub use trap::Trap;
pub use tree::Tree;
pub use vars::{Eval, Local, Readonly, Set, Shift, Shopt, Times, Unset};
pub use verify::Verify;
pub use wait::Wait;
pub use wc::Wc;
pub use yaml::Yaml;
pub use yes::Yes;
pub use zip_cmd::{Unzip, Zip};

#[cfg(feature = "git")]
pub use git::Git;

#[cfg(feature = "ssh")]
pub use ssh::{Scp, Sftp, Ssh};

#[cfg(feature = "python")]
pub(crate) use python::PythonInprocessOptIn;
#[cfg(feature = "python")]
pub use python::{Python, PythonExternalFnHandler, PythonExternalFns, PythonLimits};

#[cfg(feature = "typescript")]
pub use typescript::{
    TypeScript, TypeScriptConfig, TypeScriptExternalFnHandler, TypeScriptExternalFns,
    TypeScriptLimits,
};

use async_trait::async_trait;
use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::error::Result;
use crate::fs::FileSystem;
use crate::interpreter::ExecResult;

pub(crate) async fn read_text_file(
    fs: &dyn FileSystem,
    path: &Path,
    cmd_name: &str,
) -> std::result::Result<String, ExecResult> {
    let content = fs
        .read_file(path)
        .await
        .map_err(|e| ExecResult::err(format!("{cmd_name}: {}: {e}\n", path.display()), 1))?;

    // Binary device files (/dev/urandom, /dev/random): preserve raw bytes as
    // Latin-1 (ISO 8859-1) so each byte 0x00-0xFF maps 1:1 to a char.
    // This lets `tr -dc 'a-z0-9' < /dev/urandom | head -c N` work correctly.
    if path == Path::new("/dev/urandom") || path == Path::new("/dev/random") {
        return Ok(content.iter().map(|&b| b as char).collect());
    }

    Ok(String::from_utf8_lossy(&content).into_owned())
}

/// Check args for `--help` and optionally `--version`.
///
/// Returns `Some(ExecResult)` when the flag is found, `None` otherwise.
/// Call at the top of `execute()` to add standard help/version support.
///
/// Only matches long flags (`--help`, `--version`) because short flags
/// `-h` and `-V` have different meanings in many tools (e.g. `sort -V`
/// for version sort, `ls -h` for human-readable, `grep -h` to suppress
/// filenames).  Tools that want `-h`/`-V` as aliases should handle them
/// in their own `execute()` method.
pub(crate) fn check_help_version(
    args: &[String],
    help_text: &str,
    version: Option<&str>,
) -> Option<ExecResult> {
    for arg in args {
        match arg.as_str() {
            "--help" => return Some(ExecResult::ok(help_text.to_string())),
            "--version" => {
                if let Some(ver) = version {
                    return Some(ExecResult::ok(format!("{ver}\n")));
                }
            }
            // Stop checking after first non-flag argument
            s if !s.starts_with('-') => break,
            _ => {}
        }
    }
    None
}

// Re-export ShellRef for internal builtins
pub(crate) use crate::interpreter::ShellRef;

// Re-export for use by builtins
pub use crate::interpreter::BuiltinSideEffect;

/// Typed, per-execution data exposed to builtin implementations.
///
/// This is intentionally separate from shell state: extensions live for one
/// `Bash::exec*()` call, while the shell/interpreter may persist across many
/// executions.
#[derive(Default)]
pub struct ExecutionExtensions {
    values: HashMap<TypeId, Box<dyn Any + Send + Sync>>,
}

impl ExecutionExtensions {
    /// Create an empty execution extension bag.
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert a typed value, replacing any previous value of the same type.
    pub fn insert<T>(&mut self, value: T) -> Option<T>
    where
        T: Send + Sync + 'static,
    {
        self.values
            .insert(TypeId::of::<T>(), Box::new(value))
            .and_then(|prev| prev.downcast::<T>().ok().map(|prev| *prev))
    }

    /// Builder-style insert.
    pub fn with<T>(mut self, value: T) -> Self
    where
        T: Send + Sync + 'static,
    {
        let _ = self.insert(value);
        self
    }

    /// Look up a typed value by exact type.
    pub fn get<T>(&self) -> Option<&T>
    where
        T: Send + Sync + 'static,
    {
        self.values
            .get(&TypeId::of::<T>())
            .and_then(|value| value.downcast_ref::<T>())
    }

    /// Return whether the bag is empty.
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }
}

/// A sub-command that a builtin wants the interpreter to execute.
///
/// Builtins like `timeout`, `xargs`, and `find -exec` need to execute
/// other commands. They return an [`ExecutionPlan`] describing what to
/// run, and the interpreter handles actual execution.
#[derive(Debug, Clone)]
pub struct SubCommand {
    /// Command name (e.g. "echo", "rm").
    pub name: String,
    /// Command arguments.
    pub args: Vec<String>,
    /// Optional stdin to pipe into the command.
    pub stdin: Option<String>,
}

/// Execution plan returned by builtins that need to run sub-commands.
///
/// Instead of executing commands directly (which would require interpreter
/// access), builtins return a plan that the interpreter fulfills.
#[derive(Debug)]
pub enum ExecutionPlan {
    /// Run a single command with a timeout.
    Timeout {
        /// Maximum duration before killing the command.
        duration: std::time::Duration,
        /// Whether to preserve the command's exit status on timeout.
        preserve_status: bool,
        /// The command to execute.
        command: SubCommand,
    },
    /// Run a sequence of commands, collecting their output.
    Batch {
        /// Commands to execute in order.
        commands: Vec<SubCommand>,
    },
}

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

// Re-export shared normalize_path for use by builtins
use crate::fs::normalize_path;

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

    /// SSH client for ssh/scp/sftp operations.
    ///
    /// Only available when the `ssh` feature is enabled and
    /// an [`SshConfig`](crate::SshConfig) is configured via
    /// [`BashBuilder::ssh`](crate::BashBuilder::ssh).
    #[cfg(feature = "ssh")]
    pub ssh_client: Option<&'a crate::ssh::SshClient>,

    /// Direct access to interpreter shell state.
    ///
    /// Provides internal builtins with:
    /// - **Mutable access** to aliases and traps (simple HashMap state)
    /// - **Read-only access** to functions, builtins, call stack, history, jobs
    ///
    /// `None` for custom/external builtins; `Some(...)` for internal builtins
    /// that need interpreter state (e.g. `type`, `alias`, `trap`).
    ///
    /// Design: aliases/traps are directly mutable because they're simple HashMaps
    /// with no invariants. Arrays use [`BuiltinSideEffect`] because they need
    /// budget checking. History uses side effects for VFS persistence.
    pub(crate) shell: Option<ShellRef<'a>>,
}

impl<'a> Context<'a> {
    /// Look up a typed per-execution extension, if present.
    pub fn execution_extension<T>(&self) -> Option<&T>
    where
        T: Send + Sync + 'static,
    {
        self.shell
            .as_ref()
            .and_then(|shell| shell.execution_extensions.get::<T>())
    }

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
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
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

    /// Return an execution plan for sub-command execution.
    ///
    /// Builtins that need to execute other commands (e.g. `timeout`, `xargs`,
    /// `find -exec`) override this to return an `ExecutionPlan`. The interpreter
    /// fulfills the plan by executing the sub-commands and returning results.
    ///
    /// When this returns `Some(plan)`, the interpreter ignores the `execute()`
    /// result and instead runs the plan. When `None`, normal `execute()` is used.
    ///
    /// The default implementation returns `Ok(None)`.
    async fn execution_plan(&self, _ctx: &Context<'_>) -> Result<Option<ExecutionPlan>> {
        Ok(None)
    }

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

#[async_trait]
impl Builtin for std::sync::Arc<dyn Builtin> {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        (**self).execute(ctx).await
    }

    async fn execution_plan(&self, ctx: &Context<'_>) -> Result<Option<ExecutionPlan>> {
        (**self).execution_plan(ctx).await
    }

    fn llm_hint(&self) -> Option<&'static str> {
        (**self).llm_hint()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::{FileSystem, InMemoryFs};

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

    #[tokio::test]
    async fn read_text_file_returns_lossy_utf8() {
        let fs = InMemoryFs::new();
        fs.write_file(Path::new("/tmp/data.bin"), b"hi\xffthere")
            .await
            .unwrap();

        let text = read_text_file(&fs, Path::new("/tmp/data.bin"), "cat")
            .await
            .unwrap();

        assert_eq!(text, "hi\u{fffd}there");
    }

    #[tokio::test]
    async fn read_text_file_formats_missing_file_errors() {
        let fs = InMemoryFs::new();
        let err = read_text_file(&fs, Path::new("/tmp/missing.txt"), "cat")
            .await
            .unwrap_err();

        assert_eq!(err.exit_code, 1);
        assert!(err.stderr.contains("cat: /tmp/missing.txt:"));
    }

    #[test]
    fn check_help_version_returns_help() {
        let args = vec!["--help".to_string()];
        let r = check_help_version(&args, "usage text\n", Some("v1.0"));
        assert!(r.is_some());
        assert_eq!(r.unwrap().stdout, "usage text\n");
    }

    #[test]
    fn check_help_version_returns_version() {
        let args = vec!["--version".to_string()];
        let r = check_help_version(&args, "usage\n", Some("tool 1.0"));
        assert!(r.is_some());
        assert_eq!(r.unwrap().stdout, "tool 1.0\n");
    }

    #[test]
    fn check_help_version_no_version_configured() {
        let args = vec!["--version".to_string()];
        let r = check_help_version(&args, "usage\n", None);
        assert!(r.is_none());
    }

    #[test]
    fn check_help_version_stops_at_non_flag() {
        let args = vec!["file.txt".to_string(), "--help".to_string()];
        let r = check_help_version(&args, "usage\n", None);
        assert!(r.is_none());
    }

    #[test]
    fn check_help_version_no_match() {
        let args = vec!["-c".to_string(), "filter".to_string()];
        let r = check_help_version(&args, "usage\n", Some("v1"));
        assert!(r.is_none());
    }
}
