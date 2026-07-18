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
mod clap_env;
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
mod generated;
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
mod shuf;
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
mod truncate;
mod vars;
mod verify;
mod wait;
mod wc;
mod yaml;
mod yes;
mod zip_cmd;

mod helpers;
pub(crate) use helpers::{invalid_option, BuiltinHelper};

pub(crate) mod limits;
pub(crate) use limits::MAX_FORMAT_WIDTH;

pub(crate) mod git;

pub(crate) mod ssh;

#[cfg(any(feature = "python", feature = "typescript"))]
mod runtime_limits;

#[cfg(feature = "python")]
mod python;

#[cfg(feature = "typescript")]
mod typescript;

#[cfg(feature = "sqlite")]
mod sqlite;

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
pub use shuf::Shuf;

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
pub use truncate::Truncate;
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

#[cfg(any(feature = "python", feature = "typescript"))]
pub use runtime_limits::RuntimeLimits;

#[cfg(feature = "python")]
pub(crate) use python::PythonInprocessOptIn;
#[cfg(feature = "python")]
pub use python::{Python, PythonExternalFnHandler, PythonExternalFns, PythonLimits};

#[cfg(feature = "typescript")]
pub use typescript::{
    TypeScriptConfig, TypeScriptExtension, TypeScriptExternalFnHandler, TypeScriptExternalFns,
    TypeScriptLimits,
};

#[cfg(feature = "sqlite")]
pub(crate) use sqlite::SqliteInprocessOptIn;
#[cfg(feature = "sqlite")]
pub use sqlite::{Sqlite, SqliteBackend, SqliteLimits};

use async_trait::async_trait;
use clap::{CommandFactory, FromArgMatches};
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
            "--" => break,
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

/// A bundle of shell capabilities registered together.
///
/// Extensions are intended for embedders that want to contribute a family of
/// builtins as one unit. Builders expand extensions into normal builtin
/// registrations so command dispatch remains unchanged.
pub trait Extension: Send + Sync {
    /// Return builtin commands contributed by this extension.
    ///
    /// Later registrations with the same name replace earlier registrations,
    /// matching [`BashBuilder::builtin`](crate::BashBuilder::builtin).
    fn builtins(&self) -> Vec<(String, Box<dyn Builtin>)>;
}

/// Host-owned, mutable registry of builtin commands.
///
/// Unlike [`BashBuilder::builtin`](crate::BashBuilder::builtin), entries can be
/// inserted and removed at any point during the lifetime of the `Bash`
/// instance — the interpreter consults the registry at command-dispatch time.
///
/// Cloning the registry produces a new handle that shares the same underlying
/// storage; mutations made via any handle are visible to all others. This
/// lets embedders (FFI bindings, REPLs, plugin systems) hand a handle to the
/// builder and retain another for runtime registration.
///
/// In the command-resolution order, host-registered builtins are checked
/// after shell functions and POSIX special builtins, but before baked-in
/// builtins — so embedders can override baked-in commands.
#[derive(Clone, Default)]
pub struct BuiltinRegistry {
    inner: Arc<std::sync::RwLock<HashMap<String, Arc<dyn Builtin>>>>,
}

impl BuiltinRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register or replace a builtin under `name`.
    pub fn insert(&self, name: impl Into<String>, builtin: Arc<dyn Builtin>) {
        if let Ok(mut guard) = self.inner.write() {
            guard.insert(name.into(), builtin);
        }
    }

    /// Remove the entry for `name`, returning the previously registered
    /// builtin if any.
    pub fn remove(&self, name: &str) -> Option<Arc<dyn Builtin>> {
        self.inner.write().ok().and_then(|mut g| g.remove(name))
    }

    /// Look up the builtin registered under `name`, returning a cloned handle.
    pub fn lookup(&self, name: &str) -> Option<Arc<dyn Builtin>> {
        self.inner.read().ok().and_then(|g| g.get(name).cloned())
    }

    /// Return the set of currently registered builtin names.
    pub fn names(&self) -> Vec<String> {
        self.inner
            .read()
            .map(|g| g.keys().cloned().collect())
            .unwrap_or_default()
    }

    /// True if no builtins are registered.
    pub fn is_empty(&self) -> bool {
        self.inner.read().map(|g| g.is_empty()).unwrap_or(true)
    }
}

/// Typed, per-execution data exposed to builtin implementations.
///
/// This is intentionally separate from shell state: extensions live for one
/// `Bash::exec*()` call, while the shell/interpreter may persist across many
/// executions.
#[derive(Default)]
pub struct ExecutionExtensions {
    values: HashMap<TypeId, Box<dyn Any + Send + Sync>>,
}

/// Per-exec wall-clock deadline for builtins with synchronous VM sections.
#[derive(Debug, Clone)]
pub(crate) struct ExecutionDeadline {
    #[allow(dead_code)]
    started_at: crate::time_compat::Instant,
    timeout: std::time::Duration,
}

impl ExecutionDeadline {
    /// Create a deadline anchored at the start of `Bash::exec*`.
    pub(crate) fn new(timeout: std::time::Duration) -> Self {
        Self {
            started_at: crate::time_compat::Instant::now(),
            timeout,
        }
    }

    /// Remaining budget; never returns zero so downstream VM timers stay active.
    #[allow(dead_code)]
    pub(crate) fn remaining(&self) -> std::time::Duration {
        let remaining = self.timeout.saturating_sub(self.started_at.elapsed());
        if remaining.is_zero() {
            std::time::Duration::from_millis(1)
        } else {
            remaining
        }
    }

    /// Whether the wall-clock budget is exhausted. Builtins with synchronous
    /// loops the async timeout cannot preempt poll this to abort.
    #[allow(dead_code)]
    pub(crate) fn is_expired(&self) -> bool {
        self.started_at.elapsed() >= self.timeout
    }
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
    /// Command-scoped environment assignments (`VAR=value cmd ...`), applied
    /// only to this command's environment. Used by `xargs --process-slot-var`
    /// to expose a per-invocation parallel-slot index.
    pub assignments: Vec<(String, String)>,
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
    /// Run a sequence of commands, then merge builtin-generated stderr/exit semantics.
    BatchWithStatus {
        /// Commands to execute in order.
        commands: Vec<SubCommand>,
        /// Builtin-generated stderr to prepend to command stderr.
        stderr_prefix: String,
        /// Force non-zero exit (1) when command sequence would otherwise succeed.
        force_error_exit: bool,
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
    pub git_client: Option<&'a crate::builtins::git::GitClient>,

    /// SSH client for ssh/scp/sftp operations.
    ///
    /// Only available when the `ssh` feature is enabled and
    /// an [`SshConfig`](crate::SshConfig) is configured via
    /// [`BashBuilder::ssh`](crate::BashBuilder::ssh).
    #[cfg(feature = "ssh")]
    pub ssh_client: Option<&'a crate::builtins::ssh::SshClient>,

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

    /// Clear hidden per-instance state that must not survive snapshot restore.
    ///
    /// Most builtins are stateless and keep the default no-op. Builtins with
    /// session caches must override this so [`Bash::restore_snapshot`](crate::Bash::restore_snapshot)
    /// restores the full observable boundary, not just shell variables and VFS bytes.
    fn reset_session_state(&self) {}
}

/// Trait for custom builtins that parse arguments with [`clap`].
///
/// Implement this trait when a builtin has enough flags/options that deriving a
/// `clap::Parser` struct is clearer than manually inspecting [`Context::args`].
///
/// # Example
///
/// ```rust
/// use bashkit::{Bash, BashkitContext, ClapBuiltin, async_trait};
/// use bashkit::clap::Parser;
///
/// #[derive(Parser)]
/// #[command(name = "greet")]
/// struct GreetArgs {
///     #[arg(short, long, default_value = "World")]
///     name: String,
/// }
///
/// struct Greet;
///
/// #[async_trait]
/// impl ClapBuiltin for Greet {
///     type Args = GreetArgs;
///
///     async fn execute_clap(
///         &self,
///         args: Self::Args,
///         ctx: &mut BashkitContext<'_>,
///     ) -> bashkit::Result<()> {
///         ctx.write_stdout(format!("Hello, {}!\n", args.name));
///         Ok(())
///     }
/// }
///
/// # #[tokio::main]
/// # async fn main() -> bashkit::Result<()> {
/// let mut bash = Bash::builder()
///     .builtin("greet", Box::new(Greet))
///     .build();
/// let result = bash.exec("greet --name Alice").await?;
/// assert_eq!(result.stdout, "Hello, Alice!\n");
/// # Ok(())
/// # }
/// ```
#[async_trait]
pub trait ClapBuiltin: Send + Sync {
    /// Clap parser type for this builtin's arguments.
    type Args: clap::Parser + Send + 'static;

    /// Execute the builtin with already-parsed clap arguments.
    async fn execute_clap(&self, args: Self::Args, ctx: &mut BashkitContext<'_>) -> Result<()>;

    /// Optional short hint for LLM system prompts.
    fn llm_hint(&self) -> Option<&'static str> {
        None
    }

    /// Clear hidden per-instance state that must not survive snapshot restore.
    ///
    /// Most builtins are stateless and keep the default no-op. Builtins with
    /// session caches must override this so [`Bash::restore_snapshot`](crate::Bash::restore_snapshot)
    /// restores the full observable boundary, not just shell variables and VFS bytes.
    fn reset_session_state(&self) {}
}

/// Mutable execution context for [`ClapBuiltin`] implementations.
///
/// This context keeps clap-backed builtins close to normal CLI code: handlers
/// write to stdout/stderr, set an exit code when needed, and return
/// `bashkit::Result<()>` for fatal host-side errors.
pub struct BashkitContext<'a> {
    inner: Context<'a>,

    /// Captured stdout for this builtin invocation.
    pub stdout: String,

    /// Captured stderr for this builtin invocation.
    pub stderr: String,

    /// Exit code for this builtin invocation.
    pub exit_code: i32,
}

impl<'a> BashkitContext<'a> {
    fn new(inner: Context<'a>) -> Self {
        Self {
            inner,
            stdout: String::new(),
            stderr: String::new(),
            exit_code: 0,
        }
    }

    fn into_exec_result(self) -> ExecResult {
        ExecResult {
            stdout: self.stdout,
            stdout_bytes: None,
            stderr: self.stderr,
            exit_code: self.exit_code,
            ..Default::default()
        }
    }

    /// Original shell words passed to the builtin, after shell expansion.
    pub fn raw_args(&self) -> &[String] {
        self.inner.args
    }

    /// Environment variables visible to this builtin.
    pub fn env(&self) -> &HashMap<String, String> {
        self.inner.env
    }

    /// Mutable shell variables.
    pub fn variables(&mut self) -> &mut HashMap<String, String> {
        self.inner.variables
    }

    /// Current working directory.
    pub fn cwd(&self) -> &Path {
        self.inner.cwd
    }

    /// Mutable current working directory.
    pub fn cwd_mut(&mut self) -> &mut PathBuf {
        self.inner.cwd
    }

    /// Virtual filesystem for this shell.
    pub fn fs(&self) -> Arc<dyn FileSystem> {
        Arc::clone(&self.inner.fs)
    }

    /// Pipeline stdin, if the builtin is invoked after a pipe.
    pub fn stdin(&self) -> Option<&str> {
        self.inner.stdin
    }

    /// Look up a typed per-execution extension, if present.
    pub fn execution_extension<T>(&self) -> Option<&T>
    where
        T: Send + Sync + 'static,
    {
        self.inner.execution_extension::<T>()
    }

    /// Append text to stdout.
    pub fn write_stdout(&mut self, output: impl AsRef<str>) {
        self.stdout.push_str(output.as_ref());
    }

    /// Append text to stderr.
    pub fn write_stderr(&mut self, output: impl AsRef<str>) {
        self.stderr.push_str(output.as_ref());
    }

    /// Set the command exit code.
    pub fn set_exit_code(&mut self, exit_code: i32) {
        self.exit_code = exit_code;
    }

    /// Append stderr text and set a failing exit code.
    pub fn fail(&mut self, stderr: impl AsRef<str>, exit_code: i32) {
        self.write_stderr(stderr);
        self.set_exit_code(exit_code);
    }
}

#[async_trait]
impl<T> Builtin for T
where
    T: ClapBuiltin,
{
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let mut command = <T::Args as CommandFactory>::command().color(clap::ColorChoice::Never);
        let command_name = command.get_name().to_string();
        let argv = std::iter::once(command_name).chain(ctx.args.iter().cloned());

        let mut matches = match command.try_get_matches_from_mut(argv) {
            Ok(matches) => matches,
            Err(error) => return Ok(clap_error_to_exec_result(error)),
        };
        let args = match <T::Args as FromArgMatches>::from_arg_matches_mut(&mut matches) {
            Ok(args) => args,
            Err(error) => return Ok(clap_error_to_exec_result(error)),
        };

        let mut ctx = BashkitContext::new(ctx);
        self.execute_clap(args, &mut ctx).await?;
        Ok(ctx.into_exec_result())
    }

    fn llm_hint(&self) -> Option<&'static str> {
        ClapBuiltin::llm_hint(self)
    }

    fn reset_session_state(&self) {
        ClapBuiltin::reset_session_state(self);
    }
}

fn clap_error_to_exec_result(error: clap::Error) -> ExecResult {
    let text = error.to_string();
    if matches!(
        error.kind(),
        clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion
    ) {
        return ExecResult::ok(text);
    }

    ExecResult::err(cap_diagnostic(text, 1024), error.exit_code())
}

fn cap_diagnostic(mut text: String, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text;
    }

    let cut = text
        .char_indices()
        .map(|(idx, _)| idx)
        .take_while(|idx| *idx <= max_bytes)
        .last()
        .unwrap_or(0);
    text.truncate(cut);
    text
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

/// Internal alias for `crate::testing` so per-tool `#[cfg(test)]`
/// modules can keep their existing `crate::builtins::debug_leak_check::*`
/// imports. The source of truth is `crate::testing` (which is also
/// reachable from integration tests and cargo-fuzz targets).
#[cfg(test)]
pub(crate) use crate::testing as debug_leak_check;

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
    fn check_help_version_stops_at_option_delimiter() {
        let args = vec!["--".to_string(), "--help".to_string()];
        let r = check_help_version(&args, "usage\n", Some("v1"));
        assert!(r.is_none());
    }

    #[test]
    fn check_help_version_no_match() {
        let args = vec!["-c".to_string(), "filter".to_string()];
        let r = check_help_version(&args, "usage\n", Some("v1"));
        assert!(r.is_none());
    }

    // -------------------------------------------------------------------------
    // TM-INF-022: stderr from builtins must not leak Rust Debug shapes.
    //
    // Static guard — walks every `crates/bashkit/src/builtins/*.rs` file
    // and asserts no Rust Debug format directives appear, modulo
    // `// debug-ok: <reason>` per-line opt-outs.
    //
    // Dynamic counterpart: each tool's own `mod tests` exercises its
    // error paths through `super::debug_leak_check::assert_no_leak`.
    // -------------------------------------------------------------------------

    #[test]
    fn no_debug_fmt_in_builtin_source() {
        // Match `{:?}`, `{:#?}`, `{name:?}`, `{name:#?}`. // debug-ok: pattern doc
        let pat = regex::Regex::new(r"\{[A-Za-z0-9_]*:#?\?\}").unwrap();
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/builtins");
        let mut violations = Vec::new();

        // Recursive walk so submodule directories like `builtins/jq/` are
        // covered too. The TM-INF-022 invariant must hold for every builtin
        // source file regardless of layout.
        fn walk(dir: &std::path::Path, violations: &mut Vec<String>, pat: &regex::Regex) {
            for entry in std::fs::read_dir(dir).expect("read builtins dir") {
                let entry = entry.unwrap();
                let path = entry.path();
                if path.is_dir() {
                    walk(&path, violations, pat);
                    continue;
                }
                if path.extension().is_none_or(|e| e != "rs") {
                    continue;
                }
                let src = std::fs::read_to_string(&path).expect("read source");
                for (i, line) in src.lines().enumerate() {
                    if line.contains("// debug-ok:") {
                        continue;
                    }
                    if line.trim_start().starts_with("#[derive(") {
                        continue;
                    }
                    if pat.is_match(line) {
                        // Show parent dir + filename so jq submodules are
                        // distinguishable from the top-level files.
                        let rel = path
                            .strip_prefix(std::path::Path::new(env!("CARGO_MANIFEST_DIR")))
                            .unwrap_or(&path);
                        violations.push(format!(
                            "{}:{}: {}",
                            rel.display(),
                            i + 1,
                            line.trim_end()
                        ));
                    }
                }
            }
        }
        walk(&dir, &mut violations, &pat);

        assert!(
            violations.is_empty(),
            "Rust Debug formatting found in builtin source. This leaks \
             internal struct shapes into stderr where LLM agents see them. \
             Use Display ({{}}) or a domain-specific formatter. Add \
             `// debug-ok: <reason>` to the line for legitimate test \
             asserts.\n\nViolations:\n{}",
            violations.join("\n")
        );
    }

    /// TM-INF-024: clap `Arg::env(...)` reads defaults from the real
    /// process environment. uutils ships `.env("TABSIZE")` /
    /// `.env("TIME_STYLE")` on `ls`, but bashkit isolates scripts inside
    /// `ctx.env`; if the host parser were allowed to consult `std::env`
    /// the sandbox boundary would leak (host can probe presence, host can
    /// inject values that propagate into bashkit's option-validation
    /// path). Codegen strips `.env(...)` from generated Arg chains and
    /// re-emits the metadata as a `<UTIL>_ENV_DEFAULTS` table that the
    /// `clap_env::apply_env_defaults` shim feeds from `ctx.env`. This
    /// static guard makes sure no future regen (or hand-edit) re-adds
    /// a runtime `.env(...)` call. Defence-in-depth: the workspace
    /// `clap` dep also drops the `env` cargo feature, so a slipped
    /// `.env(...)` won't compile.
    #[test]
    fn no_clap_env_in_generated_parsers() {
        let pat = regex::Regex::new(r"\.env\s*\(").unwrap();
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/builtins/generated");
        let mut violations = Vec::new();
        for entry in std::fs::read_dir(&dir).expect("read generated dir") {
            let entry = entry.unwrap();
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("rs") {
                continue;
            }
            let src = std::fs::read_to_string(&path).expect("read generated file");
            for (i, line) in src.lines().enumerate() {
                // Skip doc/line comments — they reference `.env(...)`
                // when describing the harvest rule. We only care about
                // real call expressions.
                if line.trim_start().starts_with("//") {
                    continue;
                }
                if pat.is_match(line) {
                    let rel = path
                        .strip_prefix(std::path::Path::new(env!("CARGO_MANIFEST_DIR")))
                        .unwrap_or(&path);
                    violations.push(format!("{}:{}: {}", rel.display(), i + 1, line.trim_end()));
                }
            }
        }
        assert!(
            violations.is_empty(),
            "clap `Arg::env(...)` found in a generated parser. This pulls \
             defaults from the host process environment and breaks bashkit's \
             sandbox boundary (TM-INF-024). Re-run `just regen-coreutils-args` \
             — the codegen harvests these into `<UTIL>_ENV_DEFAULTS` instead — \
             or remove the call by hand.\n\n{}",
            violations.join("\n")
        );
    }

    /// Every `<util>_args.rs` MUST expose a `<UTIL>_ENV_DEFAULTS` static.
    /// The codegen always emits one (possibly empty) so the bashkit-side
    /// surface is uniform — every clap-based builtin can wire through
    /// `apply_env_defaults` without per-util conditional code. Catches
    /// the regression where a regen drops the table because the codegen
    /// branch that emits it was removed or skipped.
    #[test]
    fn every_generated_parser_emits_env_defaults_table() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/builtins/generated");
        let mut missing = Vec::new();
        for entry in std::fs::read_dir(&dir).expect("read generated dir") {
            let entry = entry.unwrap();
            let path = entry.path();
            let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
            if !name.ends_with("_args.rs") {
                continue;
            }
            // util name is everything before `_args.rs`
            let util = name.trim_end_matches("_args.rs");
            let const_name = format!("{}_ENV_DEFAULTS", util.to_uppercase());
            let needle = format!("pub static {const_name}");
            let src = std::fs::read_to_string(&path).expect("read generated file");
            if !src.contains(&needle) {
                let rel = path
                    .strip_prefix(std::path::Path::new(env!("CARGO_MANIFEST_DIR")))
                    .unwrap_or(&path);
                missing.push(format!("{}: missing `{needle}: ...`", rel.display()));
            }
        }
        assert!(
            missing.is_empty(),
            "Generated parser is missing its `<UTIL>_ENV_DEFAULTS` sidecar. \
             The codegen always emits this (possibly empty) so the bashkit \
             builtin can route argv through `apply_env_defaults` without \
             per-util conditionals. Re-run `just regen-coreutils-args` to \
             regenerate.\n\n{}",
            missing.join("\n")
        );
    }

    /// Pin LS's env-default surface explicitly. uutils' upstream `ls`
    /// uses `.env("TABSIZE")` and `.env("TIME_STYLE")` as of the pinned
    /// rev — both must appear in `LS_ENV_DEFAULTS`, with matching long
    /// flags, or the virtual-env shim silently drops them. Updating
    /// uutils may legitimately add or remove rows; bump this list in
    /// the same PR as the codegen regen.
    #[test]
    fn ls_env_defaults_surface_matches_uutils() {
        use crate::builtins::generated::ls_args::LS_ENV_DEFAULTS;
        let mut got: Vec<(&'static str, &'static str)> = LS_ENV_DEFAULTS
            .iter()
            .map(|d| (d.env_var, d.long))
            .collect();
        got.sort();
        let mut expected = vec![("TABSIZE", "tabsize"), ("TIME_STYLE", "time-style")];
        expected.sort();
        assert_eq!(
            got, expected,
            "LS_ENV_DEFAULTS surface drifted from upstream uutils. Either \
             the codegen harvest dropped a row, or uutils added/removed an \
             `.env(...)` annotation on `ls` — bump this fixture together \
             with the regen."
        );
    }

    /// Every `<util>_args.rs` header MUST reference the same uutils
    /// revision as `generated::UUTILS_REVISION`. The drift workflow
    /// bumps both atomically; this test catches the case where someone
    /// regenerates a single util at a different rev and forgets to
    /// update the pin (or vice versa).
    #[test]
    fn generated_args_headers_match_pinned_uutils_revision() {
        let pin = generated::UUTILS_REVISION;
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/builtins/generated");

        let mut mismatches = Vec::new();
        for entry in std::fs::read_dir(&dir).expect("read generated dir") {
            let entry = entry.unwrap();
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("rs") {
                continue;
            }
            let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
            // Only `*_args.rs` carry per-util headers; `mod.rs` holds
            // the pin itself.
            if !name.ends_with("_args.rs") {
                continue;
            }
            let body = std::fs::read_to_string(&path).expect("read generated file");
            let header_rev = body
                .lines()
                .find_map(|l| {
                    l.strip_prefix("// Source: uutils/coreutils@")
                        .and_then(|rest| rest.split_whitespace().next())
                })
                .unwrap_or("");
            if header_rev != pin {
                mismatches.push(format!(
                    "{}: header references uutils@{header_rev}, pin is uutils@{pin}",
                    path.display()
                ));
            }
        }
        assert!(
            mismatches.is_empty(),
            "Generated argument files drift from `generated::UUTILS_REVISION` \
             (`{pin}`). Regenerate every util at the pinned rev or bump the \
             pin to match. The drift workflow does both atomically; manual \
             bumps must too.\n\n{}",
            mismatches.join("\n")
        );
    }

    /// Coarse sweep: every common flag-accepting builtin called with a
    /// bogus flag must produce a clean error. Tools without flag parsing
    /// (`true`, `false`, `:`) and tools that take a path/filter as their
    /// first arg (`cd`, `source`) are excluded.
    #[tokio::test]
    async fn every_builtin_handles_bogus_flag_cleanly() {
        const TOOLS: &[&str] = &[
            "cat",
            "ls",
            "wc",
            "head",
            "tail",
            "sort",
            "uniq",
            "cut",
            "tr",
            "grep",
            "sed",
            "awk",
            "find",
            "tree",
            "diff",
            "comm",
            "paste",
            "column",
            "join",
            "split",
            "fold",
            "expand",
            "unexpand",
            "nl",
            "tac",
            "truncate",
            "shuf",
            "rev",
            "strings",
            "od",
            "xxd",
            "hexdump",
            "base64",
            "md5sum",
            "sha1sum",
            "sha256sum",
            "tar",
            "gzip",
            "gunzip",
            "zip",
            "unzip",
            "seq",
            "expr",
            "bc",
            "numfmt",
            "test",
            "printf",
            "echo",
            "env",
            "printenv",
            "stat",
            "file",
            "basename",
            "dirname",
            "realpath",
            "readlink",
            "mktemp",
            "tee",
            "csv",
            "json",
            "yaml",
            "tomlq",
            "jq",
            "semver",
            "envsubst",
            "template",
            "patch",
        ];
        for tool in TOOLS {
            let r =
                super::debug_leak_check::run(&format!("{tool} --xyzzy-not-a-real-flag </dev/null"))
                    .await;
            super::debug_leak_check::assert_no_leak(&r, &format!("{tool}_bogus_flag"), &[]);
        }
    }
}
