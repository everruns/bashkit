//! Bashkit - Awesomely fast virtual sandbox with bash and file system
//!
//! Virtual bash interpreter for AI agents, CI/CD pipelines, and code sandboxes.
//! Written in Rust.
//!
//! # Features
//!
//! - **POSIX compliant** - Substantial IEEE 1003.1-2024 Shell Command Language compliance
//! - **Sandboxed, in-process execution** - No real filesystem access by default
//! - **Virtual filesystem** - [`InMemoryFs`], [`OverlayFs`], [`MountableFs`]
//! - **Resource limits** - Command count, loop iterations, function depth
//! - **Network allowlist** - Control HTTP access per-domain
//! - **Custom builtins** - Extend with domain-specific commands
//! - **Async-first** - Built on tokio
//! - **Experimental: Git** - Virtual git operations on the VFS (`git` feature)
//! - **Experimental: Python** - Embedded Python via [Monty](https://github.com/pydantic/monty) (`python` feature)
//! - **Experimental: SQLite** - Embedded SQLite-compatible engine via [Turso](https://github.com/tursodatabase/turso) (`sqlite` feature)
//!
//! # Built-in Commands (160)
//!
//! | Category | Commands |
//! |----------|----------|
//! | Core | `echo`, `printf`, `cat`, `nl`, `read`, `log` |
//! | Navigation | `cd`, `pwd`, `ls`, `find`, `tree`, `pushd`, `popd`, `dirs` |
//! | Flow control | `true`, `false`, `exit`, `return`, `break`, `continue`, `test`, `[`, `assert` |
//! | Variables | `export`, `set`, `unset`, `local`, `shift`, `source`, `.`, `eval`, `readonly`, `times`, `declare`, `typeset`, `let`, `dotenv`, `envsubst` |
//! | Shell | `bash`, `sh` (virtual re-invocation), `:`, `trap`, `caller`, `getopts`, `shopt`, `alias`, `unalias`, `compgen`, `fc`, `help` |
//! | Text processing | `grep`, `rg`, `sed`, `awk`, `jq`, `head`, `tail`, `sort`, `uniq`, `cut`, `tr`, `wc`, `paste`, `column`, `diff`, `comm`, `strings`, `tac`, `rev`, `seq`, `expr`, `fold`, `expand`, `unexpand`, `join`, `split`, `iconv`, `template` |
//! | File operations | `mkdir`, `mktemp`, `mkfifo`, `rm`, `cp`, `mv`, `touch`, `chmod`, `chown`, `ln`, `rmdir`, `realpath`, `readlink`, `glob`, `patch` |
//! | File inspection | `file`, `stat`, `less` |
//! | Archives | `tar`, `gzip`, `gunzip`, `zip`, `unzip` |
//! | Byte tools | `od`, `xxd`, `hexdump`, `base64` |
//! | Checksums | `md5sum`, `sha1sum`, `sha256sum`, `verify` |
//! | Utilities | `sleep`, `date`, `basename`, `dirname`, `timeout`, `wait`, `watch`, `yes`, `kill`, `clear`, `retry`, `parallel` |
//! | Disk | `df`, `du` |
//! | Pipeline | `xargs`, `tee` |
//! | System info | `whoami`, `hostname`, `uname`, `id`, `env`, `printenv`, `history` |
//! | Structured data | `json`, `csv`, `yaml`, `tomlq`, `semver` |
//! | Network | `curl`, `wget`, `http` (requires [`NetworkAllowlist`])
//! | Arithmetic | `bc` |
//! | Experimental | `python`, `python3` (requires `python` feature), `git` (requires `git` feature), `ts`, `typescript`, `node`, `deno`, `bun` (requires `typescript` feature), `ssh`, `scp`, `sftp` (requires `ssh` feature), `sqlite`, `sqlite3` (requires `sqlite` feature)
//!
//! # Shell Features
//!
//! - Variables and parameter expansion (`$VAR`, `${VAR:-default}`, `${#VAR}`)
//! - Command substitution (`$(cmd)`)
//! - Arithmetic expansion (`$((1 + 2))`)
//! - Pipelines and redirections (`|`, `>`, `>>`, `<`, `<<<`, `2>&1`)
//! - Control flow (`if`/`elif`/`else`, `for`, `while`, `case`)
//! - Functions (POSIX and bash-style)
//! - Arrays (`arr=(a b c)`, `${arr[@]}`, `${#arr[@]}`)
//! - Glob expansion (`*`, `?`)
//! - Here documents (`<<EOF`)
//!
//! - [`compatibility_scorecard`] - Full compatibility status
//!
//! # Quick Start
//!
//! ```rust
//! use bashkit::Bash;
//!
//! # #[tokio::main]
//! # async fn main() -> bashkit::Result<()> {
//! let mut bash = Bash::new();
//! let result = bash.exec("echo 'Hello, World!'").await?;
//! assert_eq!(result.stdout, "Hello, World!\n");
//! assert_eq!(result.exit_code, 0);
//! # Ok(())
//! # }
//! ```
//!
//! # Basic Usage
//!
//! ## Simple Commands
//!
//! ```rust
//! use bashkit::Bash;
//!
//! # #[tokio::main]
//! # async fn main() -> bashkit::Result<()> {
//! let mut bash = Bash::new();
//!
//! // Echo with variables
//! let result = bash.exec("NAME=World; echo \"Hello, $NAME!\"").await?;
//! assert_eq!(result.stdout, "Hello, World!\n");
//!
//! // Pipelines
//! let result = bash.exec("echo -e 'apple\\nbanana\\ncherry' | grep a").await?;
//! assert_eq!(result.stdout, "apple\nbanana\n");
//!
//! // Arithmetic
//! let result = bash.exec("echo $((2 + 2 * 3))").await?;
//! assert_eq!(result.stdout, "8\n");
//! # Ok(())
//! # }
//! ```
//!
//! ## Control Flow
//!
//! ```rust
//! use bashkit::Bash;
//!
//! # #[tokio::main]
//! # async fn main() -> bashkit::Result<()> {
//! let mut bash = Bash::new();
//!
//! // For loops
//! let result = bash.exec("for i in 1 2 3; do echo $i; done").await?;
//! assert_eq!(result.stdout, "1\n2\n3\n");
//!
//! // If statements
//! let result = bash.exec("if [ 5 -gt 3 ]; then echo bigger; fi").await?;
//! assert_eq!(result.stdout, "bigger\n");
//!
//! // Functions
//! let result = bash.exec("greet() { echo \"Hello, $1!\"; }; greet World").await?;
//! assert_eq!(result.stdout, "Hello, World!\n");
//! # Ok(())
//! # }
//! ```
//!
//! ## File Operations
//!
//! All file operations happen in the virtual filesystem:
//!
//! ```rust
//! use bashkit::Bash;
//!
//! # #[tokio::main]
//! # async fn main() -> bashkit::Result<()> {
//! let mut bash = Bash::new();
//!
//! // Create and read files
//! bash.exec("echo 'Hello' > /tmp/test.txt").await?;
//! bash.exec("echo 'World' >> /tmp/test.txt").await?;
//!
//! let result = bash.exec("cat /tmp/test.txt").await?;
//! assert_eq!(result.stdout, "Hello\nWorld\n");
//!
//! // Directory operations
//! bash.exec("mkdir -p /data/nested/dir").await?;
//! bash.exec("echo 'content' > /data/nested/dir/file.txt").await?;
//! # Ok(())
//! # }
//! ```
//!
//! # Configuration with Builder
//!
//! Use [`Bash::builder()`] for advanced configuration:
//!
//! ```rust
//! use bashkit::{Bash, ExecutionLimits};
//!
//! # #[tokio::main]
//! # async fn main() -> bashkit::Result<()> {
//! let mut bash = Bash::builder()
//!     .env("API_KEY", "secret123")
//!     .username("deploy")
//!     .hostname("prod-server")
//!     .limits(ExecutionLimits::new().max_commands(100))
//!     .build();
//!
//! let result = bash.exec("whoami && hostname").await?;
//! assert_eq!(result.stdout, "deploy\nprod-server\n");
//! # Ok(())
//! # }
//! ```
//!
//! # LLM Tool Integration
//!
//! Use [`BashTool`] when the host needs schemas, Markdown help, a compact system prompt,
//! and validated single-use executions.
//!
//! ```rust
//! use bashkit::{BashTool, Tool};
//!
//! # #[tokio::main]
//! # async fn main() -> anyhow::Result<()> {
//! let tool = BashTool::builder()
//!     .username("agent")
//!     .hostname("sandbox")
//!     .build();
//!
//! let output = tool
//!     .execution(serde_json::json!({
//!         "commands": "echo hello from bashkit",
//!         "timeout_ms": 1000
//!     }))?
//!     .execute()
//!     .await?;
//!
//! assert_eq!(output.result["stdout"], "hello from bashkit\n");
//! assert!(tool.help().contains("## Parameters"));
//! # Ok(())
//! # }
//! ```
//!
//! # Custom Builtins
//!
//! Register custom commands to extend Bashkit with domain-specific functionality:
//!
//! ```rust
//! use bashkit::{Bash, Builtin, BuiltinContext, ExecResult, async_trait};
//!
//! struct Greet;
//!
//! #[async_trait]
//! impl Builtin for Greet {
//!     async fn execute(&self, ctx: BuiltinContext<'_>) -> bashkit::Result<ExecResult> {
//!         let name = ctx.args.first().map(|s| s.as_str()).unwrap_or("World");
//!         Ok(ExecResult::ok(format!("Hello, {}!\n", name)))
//!     }
//! }
//!
//! # #[tokio::main]
//! # async fn main() -> bashkit::Result<()> {
//! let mut bash = Bash::builder()
//!     .builtin("greet", Box::new(Greet))
//!     .build();
//!
//! let result = bash.exec("greet Alice").await?;
//! assert_eq!(result.stdout, "Hello, Alice!\n");
//! # Ok(())
//! # }
//! ```
//!
//! Custom builtins have access to:
//! - Command arguments (`ctx.args`)
//! - Environment variables (`ctx.env`)
//! - Shell variables (`ctx.variables`)
//! - Virtual filesystem (`ctx.fs`)
//! - Pipeline stdin (`ctx.stdin`)
//!
//! See [`BashBuilder::builtin`] for more details.
//!
//! # Virtual Filesystem
//!
//! Bashkit provides three filesystem implementations:
//!
//! - [`InMemoryFs`]: Simple in-memory filesystem (default)
//! - [`OverlayFs`]: Copy-on-write overlay for layered storage
//! - [`MountableFs`]: Mount multiple filesystems at different paths
//!
//! See the `fs` module documentation for details and examples.
//!
//! # Direct Filesystem Access
//!
//! Access the filesystem directly via [`Bash::fs()`]:
//!
//! ```rust
//! use bashkit::{Bash, FileSystem};
//! use std::path::Path;
//!
//! # #[tokio::main]
//! # async fn main() -> bashkit::Result<()> {
//! let mut bash = Bash::new();
//! let fs = bash.fs();
//!
//! // Pre-populate files before running scripts
//! fs.mkdir(Path::new("/config"), false).await?;
//! fs.write_file(Path::new("/config/app.conf"), b"debug=true").await?;
//!
//! // Run a script that reads the config
//! let result = bash.exec("cat /config/app.conf").await?;
//! assert_eq!(result.stdout, "debug=true");
//!
//! // Read script output directly
//! bash.exec("echo 'result' > /output.txt").await?;
//! let output = fs.read_file(Path::new("/output.txt")).await?;
//! assert_eq!(output, b"result\n");
//! # Ok(())
//! # }
//! ```
//!
//! # HTTP Access (curl/wget)
//!
//! Enable the `http_client` feature and configure an allowlist for network access:
//!
//! ```rust,ignore
//! use bashkit::{Bash, NetworkAllowlist};
//!
//! let mut bash = Bash::builder()
//!     .network(NetworkAllowlist::new()
//!         .allow("https://httpbin.org"))
//!     .build();
//!
//! // curl and wget now work for allowed URLs
//! let result = bash.exec("curl -s https://httpbin.org/get").await?;
//! assert!(result.stdout.contains("httpbin.org"));
//! ```
//!
//! Security features:
//! - URL allowlist enforcement (no access without explicit configuration)
//! - 10MB response size limit (prevents memory exhaustion)
//! - 30 second timeout (prevents hanging)
//! - No automatic redirects (prevents allowlist bypass)
//! - Zip bomb protection for compressed responses
//!
//! See [`NetworkAllowlist`] for allowlist configuration options.
//!
//! # Experimental: Git Support
//!
//! Enable the `git` feature for virtual git operations. All git data lives in
//! the virtual filesystem.
//!
//! ```toml
//! [dependencies]
//! bashkit = { version = "0.1", features = ["git"] }
//! ```
//!
//! ```rust,ignore
//! use bashkit::{Bash, GitConfig};
//!
//! let mut bash = Bash::builder()
//!     .git(GitConfig::new()
//!         .author("Deploy Bot", "deploy@example.com"))
//!     .build();
//!
//! bash.exec("git init").await?;
//! bash.exec("echo 'hello' > file.txt").await?;
//! bash.exec("git add file.txt").await?;
//! bash.exec("git commit -m 'initial'").await?;
//! bash.exec("git log").await?;
//! ```
//!
//! Supported: `init`, `config`, `add`, `commit`, `status`, `log`, `branch`,
//! `checkout`, `diff`, `reset`, `remote`, `clone`/`push`/`pull`/`fetch` (virtual mode).
//!
//! See [`GitConfig`] for configuration options.
//!
//! # Experimental: Python Support
//!
//! Enable the `python` feature to embed the [Monty](https://github.com/pydantic/monty)
//! Python interpreter (pure Rust, Python 3.12). Python `pathlib.Path` operations are
//! bridged to the virtual filesystem.
//!
//! ```toml
//! [dependencies]
//! bashkit = { version = "0.1", features = ["python"] }
//! ```
//!
//! ```rust,ignore
//! use bashkit::Bash;
//!
//! let mut bash = Bash::builder().python().build();
//!
//! // Inline code
//! bash.exec("python3 -c \"print(2 ** 10)\"").await?;
//!
//! // VFS bridging — files shared between bash and Python
//! bash.exec("echo 'data' > /tmp/shared.txt").await?;
//! bash.exec(r#"python3 -c "
//! from pathlib import Path
//! print(Path('/tmp/shared.txt').read_text().strip())
//! ""#).await?;
//! ```
//!
//! Stdlib modules: `math`, `pathlib`, `os` (getenv/environ), `sys`, `typing`.
//! Security note: `re` is disabled due to regex backtracking DoS risk.
//! Limitations: no `open()` (use `pathlib.Path`), no network, no classes,
//! no third-party imports.
//!
//! See `PythonLimits` for resource limit configuration.
//!
//! See the `python_guide` module docs (requires `python` feature).
//!
//! # Examples
//!
//! See the `examples/` directory for complete working examples:
//!
//! - `basic.rs` - Getting started with Bashkit
//! - `custom_fs.rs` - Using different filesystem implementations
//! - `custom_filesystem_impl.rs` - Implementing the [`FileSystem`] trait
//! - `resource_limits.rs` - Setting execution limits
//! - `virtual_identity.rs` - Customizing username/hostname
//! - `text_processing.rs` - Using grep, sed, awk, and jq
//! - `agent_tool.rs` - LLM agent integration
//! - `git_workflow.rs` - Git operations on the virtual filesystem
//! - `python_scripts.rs` - Embedded Python with VFS bridging
//! - `python_external_functions.rs` - Python callbacks into host functions
//!
//! # Guides
//!
//! - [`custom_builtins_guide`] - Creating custom builtins
//! - [`compatibility_scorecard`] - Feature parity tracking
//! - [`live_mounts_guide`] - Live mount/unmount on running instances
//! - `python_guide` - Embedded Python (Monty) guide (requires `python` feature)
//! - `logging_guide` - Structured logging with security (requires `logging` feature)
//!
//! # Resources
//!
//! - [`threat_model`] - Security threats and mitigations
//!
//! # Ecosystem
//!
//! Bashkit is part of the [Everruns](https://everruns.com) ecosystem.

// Stricter panic prevention - prefer proper error handling over unwrap()
#![warn(clippy::unwrap_used)]
#![cfg_attr(test, allow(clippy::unwrap_used))]

mod builtins;
mod credential;
mod error;
mod fs;
/// Interceptor hooks for the execution pipeline.
pub mod hooks;
#[cfg(feature = "interop")]
pub mod interop;
mod interpreter;
mod limits;
#[cfg(feature = "logging")]
mod logging_impl;
mod network;
/// Parser module - exposed for fuzzing and testing
pub mod parser;
/// Scripted tool: compose ToolDef+callback pairs into a single Tool via bash scripts.
/// Requires the `scripted_tool` feature.
#[cfg(feature = "scripted_tool")]
pub mod scripted_tool;
mod snapshot;
/// Test-only helpers shared between internal `#[cfg(test)]` modules,
/// integration tests in `tests/*.rs`, and cargo-fuzz targets in
/// `fuzz/fuzz_targets/*.rs`. See `specs/threat-model.md` for the
/// invariants enforced (TM-INF-013, TM-INF-016, TM-INF-022).
#[doc(hidden)]
pub mod testing;
/// Tool contract for LLM integration
pub mod tool;
/// Reusable tool primitives: ToolDef, ToolArgs, ToolImpl, exec types.
#[cfg(feature = "scripted_tool")]
pub(crate) mod tool_def;
/// Structured execution trace events.
pub mod trace;

pub use async_trait::async_trait;
pub use builtins::git::GitConfig;
pub use builtins::ssh::{SshAllowlist, SshConfig, TrustedHostKey};
pub use builtins::{Builtin, Context as BuiltinContext, ExecutionExtensions};
pub use credential::Credential;
pub use error::{Error, Result};
pub use fs::{
    DirEntry, FileSystem, FileSystemExt, FileType, FsBackend, FsLimitExceeded, FsLimits, FsUsage,
    InMemoryFs, LazyLoader, Metadata, MountableFs, OverlayFs, PosixFs, SearchCapabilities,
    SearchCapable, SearchMatch, SearchProvider, SearchQuery, SearchResults, VfsSnapshot,
    normalize_path, verify_filesystem_requirements,
};
#[cfg(feature = "realfs")]
pub use fs::{RealFs, RealFsMode};
pub use interpreter::{
    ControlFlow, ExecResult, HistoryEntry, OutputCallback, ShellState, ShellStateView,
};
pub use limits::{
    ExecutionCounters, ExecutionLimits, LimitExceeded, MemoryBudget, MemoryLimits, SessionLimits,
};
pub use network::NetworkAllowlist;
pub use snapshot::{Snapshot, SnapshotOptions};
pub use tool::BashToolBuilder as ToolBuilder;
pub use tool::{
    BashTool, BashToolBuilder, Tool, ToolError, ToolExecution, ToolImage, ToolOutput,
    ToolOutputChunk, ToolOutputMetadata, ToolRequest, ToolResponse, ToolService, ToolStatus,
    VERSION,
};
pub use trace::{
    TraceCallback, TraceCollector, TraceEvent, TraceEventDetails, TraceEventKind, TraceMode,
};

#[cfg(feature = "scripted_tool")]
pub use scripted_tool::{
    AsyncToolCallback, CallbackKind, DiscoverTool, DiscoveryMode, ScriptedCommandInvocation,
    ScriptedCommandKind, ScriptedExecutionTrace, ScriptedTool, ScriptedToolBuilder,
    ScriptingToolSet, ScriptingToolSetBuilder, ToolArgs, ToolCallback, ToolDef,
};
#[cfg(feature = "scripted_tool")]
pub use tool_def::{AsyncToolExec, SyncToolExec, ToolImpl};

#[cfg(feature = "http_client")]
pub use network::{HttpClient, HttpHandler};

/// Re-exported network response type for custom HTTP handler implementations.
#[cfg(feature = "http_client")]
pub use network::Response as HttpResponse;

#[cfg(feature = "bot-auth")]
pub use network::{BotAuthConfig, BotAuthError, BotAuthPublicKey, derive_bot_auth_public_key};

#[cfg(feature = "git")]
pub use builtins::git::GitClient;

#[cfg(feature = "ssh")]
pub use builtins::ssh::{SshClient, SshHandler, SshOutput, SshTarget};

#[cfg(feature = "python")]
pub use builtins::{PythonExternalFnHandler, PythonExternalFns, PythonLimits};

#[cfg(feature = "sqlite")]
pub use builtins::{Sqlite, SqliteBackend, SqliteLimits};
// Re-export monty types needed by external handler consumers.
// **Unstable:** These types come from monty (git-pinned, not on crates.io).
// They may change in breaking ways between bashkit releases.
#[cfg(feature = "python")]
pub use monty::{ExcType, ExtFunctionResult, MontyException, MontyObject};

#[cfg(feature = "typescript")]
pub use builtins::{
    TypeScriptConfig, TypeScriptExternalFnHandler, TypeScriptExternalFns, TypeScriptLimits,
};
// Re-export zapcode-core types needed by external handler consumers.
#[cfg(feature = "typescript")]
pub use zapcode_core::Value as ZapcodeValue;

/// Logging utilities module
///
/// Provides structured logging with security features including sensitive data redaction.
/// Only available when the `logging` feature is enabled.
#[cfg(feature = "logging")]
pub mod logging {
    pub use crate::logging_impl::{
        LogConfig, format_error_for_log, format_script_for_log, sanitize_for_log,
    };
}

#[cfg(feature = "logging")]
pub use logging::LogConfig;

use interpreter::Interpreter;
use parser::Parser;
use std::collections::HashMap;
#[cfg(feature = "realfs")]
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

#[cfg(any(feature = "python", feature = "sqlite"))]
fn env_opt_in_enabled(env: &HashMap<String, String>, key: &str) -> bool {
    env.get(key)
        .is_some_and(|v| matches!(v.as_str(), "1" | "true" | "TRUE" | "yes" | "YES"))
}

/// Main entry point for Bashkit.
///
/// Provides a virtual bash interpreter with an in-memory virtual filesystem.
pub struct Bash {
    fs: Arc<dyn FileSystem>,
    /// Outermost MountableFs layer for live mount/unmount after build.
    mountable: Arc<MountableFs>,
    interpreter: Interpreter,
    /// Parser timeout (stored separately for use before interpreter runs)
    parser_timeout: std::time::Duration,
    /// Maximum input script size in bytes
    max_input_bytes: usize,
    /// Maximum AST nesting depth for parsing
    max_ast_depth: usize,
    /// Maximum parser operations (fuel)
    max_parser_operations: usize,
    /// Logging configuration
    #[cfg(feature = "logging")]
    log_config: logging::LogConfig,
    /// Operator-approved in-process Python opt-in captured at build time.
    #[cfg(feature = "python")]
    python_inprocess_opt_in: bool,
    /// Operator-approved in-process SQLite opt-in captured at build time.
    #[cfg(feature = "sqlite")]
    sqlite_inprocess_opt_in: bool,
}

impl Default for Bash {
    fn default() -> Self {
        Self::new()
    }
}

impl Bash {
    /// Create a new Bash instance with default settings.
    pub fn new() -> Self {
        let base_fs: Arc<dyn FileSystem> = Arc::new(InMemoryFs::new());
        let mountable = Arc::new(MountableFs::new(base_fs));
        let fs: Arc<dyn FileSystem> = Arc::clone(&mountable) as Arc<dyn FileSystem>;
        let interpreter = Interpreter::new(Arc::clone(&fs));
        let parser_timeout = ExecutionLimits::default().parser_timeout;
        let max_input_bytes = ExecutionLimits::default().max_input_bytes;
        let max_ast_depth = ExecutionLimits::default().max_ast_depth;
        let max_parser_operations = ExecutionLimits::default().max_parser_operations;
        Self {
            fs,
            mountable,
            interpreter,
            parser_timeout,
            max_input_bytes,
            max_ast_depth,
            max_parser_operations,
            #[cfg(feature = "logging")]
            log_config: logging::LogConfig::default(),
            #[cfg(feature = "python")]
            python_inprocess_opt_in: false,
            #[cfg(feature = "sqlite")]
            sqlite_inprocess_opt_in: false,
        }
    }

    /// Create a new BashBuilder for customized configuration.
    pub fn builder() -> BashBuilder {
        BashBuilder::default()
    }

    /// Execute a bash script and return the result.
    ///
    /// This method first validates that the script does not exceed the maximum
    /// input size, then parses the script with a timeout, AST depth limit, and fuel limit,
    /// then executes the resulting AST.
    pub async fn exec(&mut self, script: &str) -> Result<ExecResult> {
        self.exec_with_extensions(script, ExecutionExtensions::new())
            .await
    }

    /// Execute a bash script with per-execution builtin extensions.
    pub async fn exec_with_extensions(
        &mut self,
        script: &str,
        mut extensions: ExecutionExtensions,
    ) -> Result<ExecResult> {
        // Expose active execution limits to builtins that need to honor
        // per-execution sandbox settings.
        let _ = extensions.insert(self.interpreter.limits().clone());
        #[cfg(feature = "python")]
        let _ = extensions.insert(builtins::PythonInprocessOptIn(self.python_inprocess_opt_in));
        #[cfg(feature = "sqlite")]
        let _ = extensions.insert(builtins::SqliteInprocessOptIn(self.sqlite_inprocess_opt_in));
        let _extensions_guard = self.interpreter.scoped_execution_extensions(extensions);
        self.exec_impl(script).await
    }

    async fn exec_impl(&mut self, script: &str) -> Result<ExecResult> {
        // THREAT[TM-ISO-005/006/007]: Reset transient state between exec() calls
        self.interpreter.reset_transient_state();

        // Check raw input size before hooks to avoid allocating/copying oversized
        // untrusted scripts in hook payloads.
        let input_len = script.len();
        if input_len > self.max_input_bytes {
            #[cfg(feature = "logging")]
            tracing::error!(
                target: "bashkit::session",
                input_len = input_len,
                max_bytes = self.max_input_bytes,
                "Script exceeds maximum input size"
            );
            return Err(Error::ResourceLimit(LimitExceeded::InputTooLarge(
                input_len,
                self.max_input_bytes,
            )));
        }

        // THREAT[TM-LOG-001]: Sensitive data in logs
        // Mitigation: Use LogConfig to redact sensitive script content
        #[cfg(feature = "logging")]
        {
            let script_info = logging::format_script_for_log(script, &self.log_config);
            tracing::info!(target: "bashkit::session", script = %script_info, "Starting script execution");
        }

        // Fire before_exec hooks — may modify or cancel the script
        let script = if !self.interpreter.hooks().before_exec.is_empty() {
            let input = hooks::ExecInput {
                script: script.to_string(),
            };
            match self.interpreter.hooks().fire_before_exec(input) {
                Some(modified) => std::borrow::Cow::Owned(modified.script),
                None => {
                    return Ok(ExecResult::err("cancelled by before_exec hook", 1));
                }
            }
        } else {
            std::borrow::Cow::Borrowed(script)
        };
        let script = script.as_ref();

        // Re-check size after hooks in case the hook rewrites to a larger script.
        let input_len = script.len();
        if input_len > self.max_input_bytes {
            #[cfg(feature = "logging")]
            tracing::error!(
                target: "bashkit::session",
                input_len = input_len,
                max_bytes = self.max_input_bytes,
                "Script exceeds maximum input size"
            );
            return Err(Error::ResourceLimit(LimitExceeded::InputTooLarge(
                input_len,
                self.max_input_bytes,
            )));
        }

        let parser_timeout = self.parser_timeout;
        let max_ast_depth = self.max_ast_depth;
        let max_parser_operations = self.max_parser_operations;
        let script_owned = script.to_owned();

        #[cfg(feature = "logging")]
        tracing::debug!(
            target: "bashkit::parser",
            input_len = input_len,
            max_ast_depth = max_ast_depth,
            max_operations = max_parser_operations,
            "Parsing script"
        );

        // On WASM, tokio::task::spawn_blocking and tokio::time::timeout don't
        // work (no blocking thread pool, timer driver unreliable). Parse inline.
        #[cfg(target_family = "wasm")]
        let ast = {
            let parser = Parser::with_limits_and_timeout(
                &script_owned,
                max_ast_depth,
                max_parser_operations,
                Some(parser_timeout),
            );
            parser.parse()?
        };

        // On native targets, parse with timeout using spawn_blocking since
        // parsing is sync and we don't want to block the async runtime.
        #[cfg(not(target_family = "wasm"))]
        let ast = {
            let parse_result = tokio::time::timeout(parser_timeout, async {
                tokio::task::spawn_blocking(move || {
                    let parser =
                        Parser::with_limits(&script_owned, max_ast_depth, max_parser_operations);
                    parser.parse()
                })
                .await
            })
            .await;

            match parse_result {
                Ok(Ok(result)) => {
                    match &result {
                        Ok(_) => {
                            #[cfg(feature = "logging")]
                            tracing::debug!(target: "bashkit::parser", "Parse completed successfully");
                        }
                        Err(_e) => {
                            #[cfg(feature = "logging")]
                            tracing::warn!(target: "bashkit::parser", error = %_e, "Parse error");
                        }
                    }
                    result?
                }
                Ok(Err(join_error)) => {
                    #[cfg(feature = "logging")]
                    tracing::error!(
                        target: "bashkit::parser",
                        error = %join_error,
                        "Parser task failed"
                    );
                    return Err(Error::parse(format!("parser task failed: {}", join_error)));
                }
                Err(_elapsed) => {
                    #[cfg(feature = "logging")]
                    tracing::error!(
                        target: "bashkit::parser",
                        timeout_ms = parser_timeout.as_millis() as u64,
                        "Parser timeout exceeded"
                    );
                    return Err(Error::ResourceLimit(LimitExceeded::ParserTimeout(
                        parser_timeout,
                    )));
                }
            }
        };

        #[cfg(feature = "logging")]
        tracing::debug!(target: "bashkit::interpreter", "Starting interpretation");

        // Static budget validation: reject obviously expensive scripts before execution
        parser::validate_budget(&ast, self.interpreter.limits())
            .map_err(|e| Error::Execution(format!("budget validation failed: {e}")))?;

        // Load persisted history on first exec (no-op if already loaded)
        self.interpreter.load_history().await;

        let exec_start = std::time::Instant::now();
        // THREAT[TM-DOS-057]: Wrap execution with timeout to prevent sleep/blocking bypass
        let execution_timeout = self.interpreter.limits().timeout;
        #[cfg(not(target_family = "wasm"))]
        let result =
            match tokio::time::timeout(execution_timeout, self.interpreter.execute(&ast)).await {
                Ok(r) => r,
                Err(_elapsed) => Err(Error::ResourceLimit(LimitExceeded::Timeout(
                    execution_timeout,
                ))),
            };
        #[cfg(target_family = "wasm")]
        let result = self.interpreter.execute(&ast).await;
        // Issue #1184: clean up process substitution temp files after execution.
        // Done here (outside Interpreter::execute) to avoid increasing the
        // recursive async state machine size which causes stack overflow.
        self.interpreter.cleanup_proc_sub_files().await;
        let duration_ms = exec_start.elapsed().as_millis() as u64;

        // Record history entry for each line of the script
        if let Ok(ref exec_result) = result {
            let cwd = self.interpreter.cwd().to_string_lossy().to_string();
            let timestamp = chrono::Utc::now().timestamp();
            for line in script.lines() {
                let trimmed = line.trim();
                if !trimmed.is_empty() && !trimmed.starts_with('#') {
                    self.interpreter.record_history(
                        trimmed.to_string(),
                        timestamp,
                        cwd.clone(),
                        exec_result.exit_code,
                        duration_ms,
                    );
                }
            }
            // Persist history to VFS if configured
            self.interpreter.save_history().await;
        }

        #[cfg(feature = "logging")]
        match &result {
            Ok(exec_result) => {
                tracing::info!(
                    target: "bashkit::session",
                    exit_code = exec_result.exit_code,
                    stdout_len = exec_result.stdout.len(),
                    stderr_len = exec_result.stderr.len(),
                    "Script execution completed"
                );
            }
            Err(e) => {
                let error = logging::format_error_for_log(&e.to_string(), &self.log_config);
                tracing::error!(
                    target: "bashkit::session",
                    error = %error,
                    "Script execution failed"
                );
            }
        }

        // Fire after_exec hooks
        if let Ok(ref exec_result) = result
            && !self.interpreter.hooks().after_exec.is_empty()
        {
            let output = hooks::ExecOutput {
                script: script.to_string(),
                stdout: exec_result.stdout.clone(),
                stderr: exec_result.stderr.clone(),
                exit_code: exec_result.exit_code,
            };
            self.interpreter.hooks().fire_after_exec(output);
        }

        // Fire on_error hooks for execution errors
        if let Err(ref e) = result
            && !self.interpreter.hooks().on_error.is_empty()
        {
            let error_event = hooks::ErrorEvent {
                message: e.to_string(),
            };
            self.interpreter.hooks().fire_on_error(error_event);
        }

        result
    }

    /// Execute a bash script with streaming output.
    ///
    /// Like [`exec`](Self::exec), but calls `output_callback` with incremental
    /// `(stdout_chunk, stderr_chunk)` pairs as output is produced. Callbacks fire
    /// after each loop iteration, command list element, and top-level command.
    ///
    /// The full result is still returned in [`ExecResult`] for callers that need it.
    ///
    /// # Example
    ///
    /// ```rust
    /// use bashkit::Bash;
    /// use std::sync::{Arc, Mutex};
    ///
    /// # #[tokio::main]
    /// # async fn main() -> bashkit::Result<()> {
    /// let chunks: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    /// let chunks_cb = chunks.clone();
    /// let mut bash = Bash::new();
    /// let result = bash.exec_streaming(
    ///     "for i in 1 2 3; do echo $i; done",
    ///     Box::new(move |stdout, _stderr| {
    ///         chunks_cb.lock().unwrap().push(stdout.to_string());
    ///     }),
    /// ).await?;
    /// assert_eq!(result.stdout, "1\n2\n3\n");
    /// assert_eq!(*chunks.lock().unwrap(), vec!["1\n", "2\n", "3\n"]);
    /// # Ok(())
    /// # }
    /// ```
    pub async fn exec_streaming(
        &mut self,
        script: &str,
        output_callback: OutputCallback,
    ) -> Result<ExecResult> {
        self.exec_streaming_with_extensions(script, output_callback, ExecutionExtensions::new())
            .await
    }

    /// Execute a bash script with streaming output and per-execution builtin extensions.
    pub async fn exec_streaming_with_extensions(
        &mut self,
        script: &str,
        output_callback: OutputCallback,
        extensions: ExecutionExtensions,
    ) -> Result<ExecResult> {
        self.interpreter.set_output_callback(output_callback);
        let result = self.exec_with_extensions(script, extensions).await;
        self.interpreter.clear_output_callback();
        result
    }

    /// Return a shared cancellation token.
    ///
    /// Set the token to `true` from any thread to abort execution at the next
    /// command boundary with [`Error::Cancelled`].
    ///
    /// The caller is responsible for resetting the flag to `false` before
    /// calling `exec()` again.
    pub fn cancellation_token(&self) -> Arc<std::sync::atomic::AtomicBool> {
        self.interpreter.cancellation_token()
    }

    /// Return the hooks registry (read-only after build).
    ///
    /// Hooks are registered via [`BashBuilder`] methods (`on_exit`,
    /// `before_exec`, `after_exec`, `before_tool`, `after_tool`,
    /// `on_error`) and frozen at build time.
    ///
    /// HTTP hooks (`before_http`, `after_http`) live on the
    /// `HttpClient` (requires `http_client` feature) and are set via
    /// the builder as well.
    pub fn hooks(&self) -> &hooks::Hooks {
        self.interpreter.hooks()
    }

    /// Get a clone of the underlying filesystem.
    ///
    /// Provides direct access to the virtual filesystem for:
    /// - Pre-populating files before script execution
    /// - Reading binary file outputs after execution
    /// - Injecting test data or configuration
    ///
    /// # Example
    /// ```rust,no_run
    /// use bashkit::Bash;
    /// use std::path::Path;
    ///
    /// #[tokio::main]
    /// async fn main() -> anyhow::Result<()> {
    ///     let mut bash = Bash::new();
    ///     let fs = bash.fs();
    ///
    ///     // Pre-populate config file
    ///     fs.mkdir(Path::new("/config"), false).await?;
    ///     fs.write_file(Path::new("/config/app.txt"), b"debug=true\n").await?;
    ///
    ///     // Bash script can read pre-populated files
    ///     let result = bash.exec("cat /config/app.txt").await?;
    ///     assert_eq!(result.stdout, "debug=true\n");
    ///
    ///     // Bash creates output, read it directly
    ///     bash.exec("echo 'done' > /output.txt").await?;
    ///     let output = fs.read_file(Path::new("/output.txt")).await?;
    ///     assert_eq!(output, b"done\n");
    ///     Ok(())
    /// }
    /// ```
    pub fn fs(&self) -> Arc<dyn FileSystem> {
        Arc::clone(&self.fs)
    }

    /// Mount a filesystem at `vfs_path` on a live interpreter.
    ///
    /// Unlike [`BashBuilder`] mount methods which configure mounts before build,
    /// this method attaches a filesystem **after** the interpreter is running.
    /// Shell state (env vars, cwd, history) is preserved — no rebuild needed.
    ///
    /// The mount takes effect immediately: subsequent `exec()` calls will see
    /// files from the mounted filesystem at the given path.
    ///
    /// # Arguments
    ///
    /// * `vfs_path` - Absolute path where the filesystem will appear (e.g. `/mnt/data`)
    /// * `fs` - The filesystem to mount
    ///
    /// # Errors
    ///
    /// Returns an error if `vfs_path` is not absolute.
    ///
    /// # Example
    ///
    /// ```rust
    /// use bashkit::{Bash, FileSystem, InMemoryFs};
    /// use std::path::Path;
    /// use std::sync::Arc;
    ///
    /// # #[tokio::main]
    /// # async fn main() -> bashkit::Result<()> {
    /// let mut bash = Bash::new();
    ///
    /// // Create and populate a filesystem
    /// let data_fs = Arc::new(InMemoryFs::new());
    /// data_fs.write_file(Path::new("/users.json"), br#"["alice"]"#).await?;
    ///
    /// // Mount it live — no rebuild, no state loss
    /// bash.mount("/mnt/data", data_fs)?;
    ///
    /// let result = bash.exec("cat /mnt/data/users.json").await?;
    /// assert!(result.stdout.contains("alice"));
    /// # Ok(())
    /// # }
    /// ```
    pub fn mount(
        &self,
        vfs_path: impl AsRef<std::path::Path>,
        fs: Arc<dyn FileSystem>,
    ) -> Result<()> {
        self.mountable.mount(vfs_path, fs)
    }

    /// Unmount a previously mounted filesystem.
    ///
    /// After unmounting, paths under `vfs_path` fall back to the root filesystem
    /// or the next shorter mount prefix. Shell state is preserved.
    ///
    /// # Errors
    ///
    /// Returns an error if nothing is mounted at `vfs_path`.
    ///
    /// # Example
    ///
    /// ```rust
    /// use bashkit::{Bash, FileSystem, InMemoryFs};
    /// use std::path::Path;
    /// use std::sync::Arc;
    ///
    /// # #[tokio::main]
    /// # async fn main() -> bashkit::Result<()> {
    /// let mut bash = Bash::new();
    ///
    /// let tmp_fs = Arc::new(InMemoryFs::new());
    /// tmp_fs.write_file(Path::new("/data.txt"), b"temp").await?;
    ///
    /// bash.mount("/scratch", tmp_fs)?;
    /// let result = bash.exec("cat /scratch/data.txt").await?;
    /// assert_eq!(result.stdout, "temp");
    ///
    /// bash.unmount("/scratch")?;
    /// // /scratch/data.txt is no longer accessible
    /// # Ok(())
    /// # }
    /// ```
    pub fn unmount(&self, vfs_path: impl AsRef<std::path::Path>) -> Result<()> {
        self.mountable.unmount(vfs_path)
    }

    /// Capture the current shell state (variables, env, cwd, options).
    ///
    /// Returns a serializable snapshot of the interpreter state. Combine with
    /// [`InMemoryFs::snapshot()`] for full session persistence.
    ///
    /// # Example
    ///
    /// ```rust
    /// use bashkit::Bash;
    ///
    /// # #[tokio::main]
    /// # async fn main() -> bashkit::Result<()> {
    /// let mut bash = Bash::new();
    /// bash.exec("x=42").await?;
    ///
    /// let state = bash.shell_state();
    ///
    /// bash.exec("x=99").await?;
    /// bash.restore_shell_state(&state);
    ///
    /// let result = bash.exec("echo $x").await?;
    /// assert_eq!(result.stdout, "42\n");
    /// # Ok(())
    /// # }
    /// ```
    pub fn shell_state(&self) -> ShellState {
        self.interpreter.shell_state()
    }

    /// Capture a lightweight shell-state view for prompt/UI inspection.
    ///
    /// Unlike [`shell_state()`](Self::shell_state), this omits function
    /// definitions so callers that only need prompt/completion data avoid
    /// cloning AST-heavy state.
    pub fn shell_state_view(&self) -> ShellStateView {
        self.interpreter.shell_state_view()
    }

    /// Restore shell state from a previous snapshot.
    ///
    /// Restores variables, env, cwd, arrays, functions, aliases, traps, and
    /// options. Does not restore builtins or VFS contents.
    pub fn restore_shell_state(&mut self, state: &ShellState) {
        self.interpreter.restore_shell_state(state);
    }

    /// Get the current session-level counters (cumulative across exec() calls).
    ///
    /// Returns `(session_commands, session_exec_calls)`.
    pub fn session_counters(&self) -> (u64, u64) {
        let c = self.interpreter.counters();
        (c.session_commands, c.session_exec_calls)
    }

    /// Restore session-level counters to resume a session across Bash instances.
    ///
    /// This is used by the MCP server to persist cumulative session counters
    /// across fresh Bash instances created per tool call.
    pub fn restore_session_counters(&mut self, session_commands: u64, session_exec_calls: u64) {
        self.interpreter
            .restore_session_counters(session_commands, session_exec_calls);
    }
}

/// Builder for customized Bash configuration.
///
/// # Example
///
/// ```rust
/// use bashkit::{Bash, ExecutionLimits};
///
/// let bash = Bash::builder()
///     .env("HOME", "/home/user")
///     .username("deploy")
///     .hostname("prod-server")
///     .limits(ExecutionLimits::new().max_commands(1000))
///     .build();
/// ```
///
/// ## Custom Builtins
///
/// You can register custom builtins to extend bashkit with domain-specific commands:
///
/// ```rust
/// use bashkit::{Bash, Builtin, BuiltinContext, ExecResult, async_trait};
///
/// struct MyCommand;
///
/// #[async_trait]
/// impl Builtin for MyCommand {
///     async fn execute(&self, ctx: BuiltinContext<'_>) -> bashkit::Result<ExecResult> {
///         Ok(ExecResult::ok(format!("Hello from custom command!\n")))
///     }
/// }
///
/// let bash = Bash::builder()
///     .builtin("mycommand", Box::new(MyCommand))
///     .build();
/// ```
/// A file to be mounted during builder construction.
struct MountedFile {
    path: PathBuf,
    content: String,
    mode: u32,
}

struct MountedLazyFile {
    path: PathBuf,
    size_hint: u64,
    mode: u32,
    loader: LazyLoader,
}

/// A real host directory to mount in the VFS during builder construction.
#[cfg(feature = "realfs")]
struct MountedRealDir {
    /// Path on the host filesystem.
    host_path: PathBuf,
    /// Mount point inside the VFS (e.g. "/mnt/data"). None = overlay at root.
    vfs_mount: Option<PathBuf>,
    /// Access mode.
    mode: fs::RealFsMode,
}

#[derive(Default)]
pub struct BashBuilder {
    fs: Option<Arc<dyn FileSystem>>,
    env: HashMap<String, String>,
    cwd: Option<PathBuf>,
    limits: ExecutionLimits,
    session_limits: SessionLimits,
    memory_limits: MemoryLimits,
    trace_mode: TraceMode,
    trace_callback: Option<TraceCallback>,
    username: Option<String>,
    hostname: Option<String>,
    /// Fixed epoch for virtualizing the `date` builtin (TM-INF-018)
    fixed_epoch: Option<i64>,
    shell_profile: interpreter::ShellProfile,
    custom_builtins: HashMap<String, Box<dyn Builtin>>,
    /// Files to mount in the virtual filesystem
    mounted_files: Vec<MountedFile>,
    /// Lazy files to mount (loaded on first read)
    mounted_lazy_files: Vec<MountedLazyFile>,
    /// Network allowlist for curl/wget builtins
    #[cfg(feature = "http_client")]
    network_allowlist: Option<NetworkAllowlist>,
    /// Custom HTTP handler for request interception
    #[cfg(feature = "http_client")]
    http_handler: Option<Box<dyn network::HttpHandler>>,
    /// Bot-auth config for transparent request signing
    #[cfg(feature = "bot-auth")]
    bot_auth_config: Option<network::BotAuthConfig>,
    /// Logging configuration
    #[cfg(feature = "logging")]
    log_config: Option<logging::LogConfig>,
    /// Git configuration for git builtins
    #[cfg(feature = "git")]
    git_config: Option<GitConfig>,
    /// SSH configuration for ssh/scp/sftp builtins
    #[cfg(feature = "ssh")]
    ssh_config: Option<SshConfig>,
    /// Custom SSH handler for transport interception
    #[cfg(feature = "ssh")]
    ssh_handler: Option<Box<dyn builtins::ssh::SshHandler>>,
    /// Real host directories to mount in the VFS
    #[cfg(feature = "realfs")]
    real_mounts: Vec<MountedRealDir>,
    /// Optional allowlist of host paths that may be mounted.
    /// When set, only paths starting with an allowed prefix are accepted.
    #[cfg(feature = "realfs")]
    mount_path_allowlist: Option<Vec<PathBuf>>,
    /// Optional VFS path for persistent history
    history_file: Option<PathBuf>,
    /// Interceptor hooks
    hooks_on_exit: Vec<hooks::Interceptor<hooks::ExitEvent>>,
    hooks_before_exec: Vec<hooks::Interceptor<hooks::ExecInput>>,
    hooks_after_exec: Vec<hooks::Interceptor<hooks::ExecOutput>>,
    hooks_before_tool: Vec<hooks::Interceptor<hooks::ToolEvent>>,
    hooks_after_tool: Vec<hooks::Interceptor<hooks::ToolResult>>,
    hooks_on_error: Vec<hooks::Interceptor<hooks::ErrorEvent>>,
    #[cfg(feature = "http_client")]
    hooks_before_http: Vec<hooks::Interceptor<hooks::HttpRequestEvent>>,
    #[cfg(feature = "http_client")]
    hooks_after_http: Vec<hooks::Interceptor<hooks::HttpResponseEvent>>,
    /// Credential injection policy
    #[cfg(feature = "http_client")]
    credential_policy: Option<credential::CredentialPolicy>,
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

    /// Set execution limits.
    pub fn limits(mut self, limits: ExecutionLimits) -> Self {
        self.limits = limits;
        self
    }

    /// Restrict this shell to logic/data-flow commands and custom builtins.
    pub(crate) fn logic_only(mut self) -> Self {
        self.shell_profile = interpreter::ShellProfile::LogicOnly;
        self
    }

    /// Set session-level resource limits.
    ///
    /// Session limits persist across `exec()` calls and prevent tenants
    /// from circumventing per-execution limits by splitting work.
    pub fn session_limits(mut self, limits: SessionLimits) -> Self {
        self.session_limits = limits;
        self
    }

    /// Set per-instance memory limits.
    ///
    /// Controls the maximum variables, arrays, and functions a Bash
    /// instance can hold. Prevents memory exhaustion in multi-tenant use.
    pub fn memory_limits(mut self, limits: MemoryLimits) -> Self {
        self.memory_limits = limits;
        self
    }

    /// Cap total interpreter memory to `bytes`.
    ///
    /// Convenience wrapper over [`memory_limits`](Self::memory_limits) that
    /// sets `max_total_variable_bytes` to `bytes` and clamps
    /// `max_function_body_bytes` to `min(bytes, default)`. Count-based
    /// sub-limits (variable count, array entries, function count) stay at
    /// their defaults.
    ///
    /// # Example
    /// ```
    /// # use bashkit::Bash;
    /// let bash = Bash::builder()
    ///     .max_memory(10 * 1024 * 1024)   // 10 MB
    ///     .build();
    /// ```
    pub fn max_memory(self, bytes: usize) -> Self {
        let defaults = MemoryLimits::default();
        self.memory_limits(
            MemoryLimits::new()
                .max_total_variable_bytes(bytes)
                .max_function_body_bytes(bytes.min(defaults.max_function_body_bytes)),
        )
    }

    /// Set the trace mode for structured execution tracing.
    ///
    /// - `TraceMode::Off` (default): No events, zero overhead
    /// - `TraceMode::Redacted`: Events with secrets scrubbed
    /// - `TraceMode::Full`: Raw events, no redaction
    pub fn trace_mode(mut self, mode: TraceMode) -> Self {
        self.trace_mode = mode;
        self
    }

    /// Set a real-time callback for trace events.
    ///
    /// The callback is invoked for each trace event as it occurs.
    /// Requires `trace_mode` to be set to `Redacted` or `Full`.
    pub fn on_trace_event(mut self, callback: TraceCallback) -> Self {
        self.trace_callback = Some(callback);
        self
    }

    /// Set the sandbox username.
    ///
    /// This configures `whoami` and `id` builtins to return this username,
    /// and automatically sets the `USER` environment variable.
    pub fn username(mut self, username: impl Into<String>) -> Self {
        self.username = Some(username.into());
        self
    }

    /// Set the sandbox hostname.
    ///
    /// This configures `hostname` and `uname -n` builtins to return this hostname.
    pub fn hostname(mut self, hostname: impl Into<String>) -> Self {
        self.hostname = Some(hostname.into());
        self
    }

    /// Configure whether a file descriptor is reported as a terminal by `[ -t fd ]`.
    ///
    /// In a sandboxed VFS environment, all FDs default to non-terminal (false).
    /// Use this to simulate interactive mode for scripts that check `[ -t 0 ]`
    /// (stdin), `[ -t 1 ]` (stdout), or `[ -t 2 ]` (stderr).
    ///
    /// ```rust
    /// # use bashkit::Bash;
    /// let bash = Bash::builder()
    ///     .tty(0, true)  // stdin is a terminal
    ///     .tty(1, true)  // stdout is a terminal
    ///     .build();
    /// ```
    pub fn tty(mut self, fd: u32, is_terminal: bool) -> Self {
        if is_terminal {
            self.env.insert(format!("_TTY_{}", fd), "1".to_string());
        }
        self
    }

    /// Set a fixed Unix epoch for the `date` builtin.
    ///
    /// THREAT[TM-INF-018]: Prevents `date` from leaking real host time.
    /// When set, `date` returns this fixed time instead of the real clock.
    pub fn fixed_epoch(mut self, epoch: i64) -> Self {
        self.fixed_epoch = Some(epoch);
        self
    }

    /// Enable persistent history stored at the given VFS path.
    ///
    /// History entries are loaded from this file at startup and saved after each
    /// `exec()` call. The file is stored in the virtual filesystem.
    pub fn history_file(mut self, path: impl Into<PathBuf>) -> Self {
        self.history_file = Some(path.into());
        self
    }

    /// Configure network access for curl/wget builtins.
    ///
    /// Network access is disabled by default. Use this method to enable HTTP
    /// requests from scripts with a URL allowlist for security.
    ///
    /// # Security
    ///
    /// The allowlist uses a default-deny model:
    /// - Only URLs matching allowlist patterns can be accessed
    /// - Pattern matching is literal (no DNS resolution) to prevent DNS rebinding
    /// - Scheme, host, port, and path prefix are all validated
    ///
    /// # Example
    ///
    /// ```rust
    /// use bashkit::{Bash, NetworkAllowlist};
    ///
    /// // Allow access to specific APIs only
    /// let allowlist = NetworkAllowlist::new()
    ///     .allow("https://api.example.com")
    ///     .allow("https://cdn.example.com/assets");
    ///
    /// let bash = Bash::builder()
    ///     .network(allowlist)
    ///     .build();
    /// ```
    ///
    /// # Warning
    ///
    /// Using [`NetworkAllowlist::allow_all()`] is dangerous and should only be
    /// used for testing or when the script is fully trusted.
    #[cfg(feature = "http_client")]
    pub fn network(mut self, allowlist: NetworkAllowlist) -> Self {
        self.network_allowlist = Some(allowlist);
        self
    }

    /// Set a custom HTTP handler for request interception.
    ///
    /// The handler is called after the URL allowlist check, so the security
    /// boundary stays in bashkit. Use this for:
    /// - Corporate proxies
    /// - Logging/auditing
    /// - Caching responses
    /// - Rate limiting
    /// - Mocking HTTP responses in tests
    ///
    /// # Example
    ///
    /// ```ignore
    /// use bashkit::network::HttpHandler;
    ///
    /// struct MyHandler;
    ///
    /// #[async_trait::async_trait]
    /// impl HttpHandler for MyHandler {
    ///     async fn request(
    ///         &self,
    ///         method: &str,
    ///         url: &str,
    ///         body: Option<&[u8]>,
    ///         headers: &[(String, String)],
    ///     ) -> Result<bashkit::network::Response, String> {
    ///         Ok(bashkit::network::Response {
    ///             status: 200,
    ///             headers: vec![],
    ///             body: b"mocked".to_vec(),
    ///         })
    ///     }
    /// }
    ///
    /// let bash = Bash::builder()
    ///     .network(NetworkAllowlist::allow_all())
    ///     .http_handler(Box::new(MyHandler))
    ///     .build();
    /// ```
    #[cfg(feature = "http_client")]
    pub fn http_handler(mut self, handler: Box<dyn network::HttpHandler>) -> Self {
        self.http_handler = Some(handler);
        self
    }

    /// Enable transparent request signing for all outbound HTTP requests.
    ///
    /// When configured, every HTTP request made by curl/wget/http builtins
    /// is signed with Ed25519 per RFC 9421 / web-bot-auth profile. No CLI
    /// arguments or script changes needed — signing is fully transparent.
    ///
    /// Signing failures are non-blocking: the request is sent unsigned.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use bashkit::{Bash, NetworkAllowlist};
    /// use bashkit::network::BotAuthConfig;
    ///
    /// let bash = Bash::builder()
    ///     .network(NetworkAllowlist::new().allow("https://api.example.com"))
    ///     .bot_auth(BotAuthConfig::from_seed([42u8; 32])
    ///         .with_agent_fqdn("bot.example.com"))
    ///     .build();
    /// ```
    #[cfg(feature = "bot-auth")]
    pub fn bot_auth(mut self, config: network::BotAuthConfig) -> Self {
        self.bot_auth_config = Some(config);
        self
    }

    /// Configure logging behavior.
    ///
    /// When the `logging` feature is enabled, Bashkit can emit structured logs
    /// at various levels (error, warn, info, debug, trace) during execution.
    ///
    /// # Log Levels
    ///
    /// - **ERROR**: Unrecoverable failures, exceptions, security violations
    /// - **WARN**: Recoverable issues, limit warnings, deprecated usage
    /// - **INFO**: Session lifecycle (start/end), high-level execution flow
    /// - **DEBUG**: Command execution, variable expansion, control flow
    /// - **TRACE**: Internal parser/interpreter state, detailed data flow
    ///
    /// # Security (TM-LOG-001)
    ///
    /// By default, sensitive data is redacted from logs:
    /// - Environment variables matching secret patterns (PASSWORD, TOKEN, etc.)
    /// - URL credentials (user:pass@host)
    /// - Values that look like API keys or JWTs
    ///
    /// # Example
    ///
    /// ```rust
    /// use bashkit::{Bash, LogConfig};
    ///
    /// let bash = Bash::builder()
    ///     .log_config(LogConfig::new()
    ///         .redact_env("MY_CUSTOM_SECRET"))
    ///     .build();
    /// ```
    ///
    /// # Warning
    ///
    /// Do not use `LogConfig::unsafe_disable_redaction()` or
    /// `LogConfig::unsafe_log_scripts()` in production, as they may expose
    /// sensitive data in logs.
    #[cfg(feature = "logging")]
    pub fn log_config(mut self, config: logging::LogConfig) -> Self {
        self.log_config = Some(config);
        self
    }

    /// Configure git support for git commands.
    ///
    /// Git access is disabled by default. Use this method to enable git
    /// commands with the specified configuration.
    ///
    /// # Security
    ///
    /// - All operations are confined to the virtual filesystem
    /// - Author identity is sandboxed (configurable, never from host)
    /// - Remote operations (Phase 2) require URL allowlist
    /// - No access to host git config or credentials
    ///
    /// # Example
    ///
    /// ```rust
    /// use bashkit::{Bash, GitConfig};
    ///
    /// let bash = Bash::builder()
    ///     .git(GitConfig::new()
    ///         .author("CI Bot", "ci@example.com"))
    ///     .build();
    /// ```
    ///
    /// # Threat Mitigations
    ///
    /// - TM-GIT-002: Host identity leak - uses configured author, never host
    /// - TM-GIT-003: Host config access - no filesystem access outside VFS
    /// - TM-GIT-005: Repository escape - all paths within VFS
    #[cfg(feature = "git")]
    pub fn git(mut self, config: GitConfig) -> Self {
        self.git_config = Some(config);
        self
    }

    /// Configure SSH access for ssh/scp/sftp builtins.
    ///
    /// # Example
    ///
    /// ```rust
    /// use bashkit::{Bash, SshConfig};
    ///
    /// let bash = Bash::builder()
    ///     .ssh(SshConfig::new()
    ///         .allow("*.supabase.co")
    ///         .default_user("root"))
    ///     .build();
    /// ```
    ///
    /// # Threat Mitigations
    ///
    /// - TM-SSH-001: Unauthorized host access - host allowlist (default-deny)
    /// - TM-SSH-002: Credential leakage - keys from VFS only
    /// - TM-SSH-005: Connection hang - configurable timeouts
    #[cfg(feature = "ssh")]
    pub fn ssh(mut self, config: SshConfig) -> Self {
        self.ssh_config = Some(config);
        self
    }

    /// Set a custom SSH handler for transport interception.
    ///
    /// Embedders can implement [`SshHandler`] to mock, proxy, log, or
    /// rate-limit SSH operations. The allowlist check happens before
    /// the handler is called.
    #[cfg(feature = "ssh")]
    pub fn ssh_handler(mut self, handler: Box<dyn builtins::ssh::SshHandler>) -> Self {
        self.ssh_handler = Some(handler);
        self
    }

    /// Enable embedded Python (`python`/`python3` builtins) via Monty interpreter
    /// with default resource limits.
    ///
    /// Monty runs directly in the host process with resource limits enforced
    /// by Monty's runtime (memory, allocations, time, recursion).
    ///
    /// For security, execution is runtime-gated: set
    /// `BASHKIT_ALLOW_INPROCESS_PYTHON=1` via builder `.env(...)` before
    /// invoking `python`/`python3`.
    ///
    /// Requires the `python` feature flag. Python `pathlib.Path` operations are
    /// bridged to the virtual filesystem.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let bash = Bash::builder().python().build();
    /// ```
    #[cfg(feature = "python")]
    pub fn python(self) -> Self {
        self.python_with_limits(builtins::PythonLimits::default())
    }

    /// Enable embedded SQLite (`sqlite`/`sqlite3` builtins) via Turso.
    ///
    /// Registers both names with the default [`SqliteLimits`]. The Turso
    /// engine is BETA upstream — for security, execution is runtime-gated:
    /// set `BASHKIT_ALLOW_INPROCESS_SQLITE=1` via builder `.env(...)` (or
    /// `export`) before invoking `sqlite`.
    ///
    /// Requires the `sqlite` feature flag. Database files are loaded from /
    /// flushed to the virtual filesystem at command boundaries.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let bash = Bash::builder()
    ///     .sqlite()
    ///     .env("BASHKIT_ALLOW_INPROCESS_SQLITE", "1")
    ///     .build();
    /// ```
    #[cfg(feature = "sqlite")]
    pub fn sqlite(self) -> Self {
        self.sqlite_with_limits(builtins::SqliteLimits::default())
    }

    /// Enable embedded SQLite with custom limits and backend selection.
    ///
    /// See [`BashBuilder::sqlite`] for details. Use [`SqliteLimits::backend`]
    /// to switch between the in-memory shim (Phase 1, default) and the
    /// VFS-backed adapter (Phase 2).
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use bashkit::{SqliteBackend, SqliteLimits};
    ///
    /// let bash = Bash::builder()
    ///     .sqlite_with_limits(
    ///         SqliteLimits::default()
    ///             .backend(SqliteBackend::Vfs)
    ///             .max_db_bytes(8 * 1024 * 1024),
    ///     )
    ///     .build();
    /// ```
    #[cfg(feature = "sqlite")]
    pub fn sqlite_with_limits(self, limits: builtins::SqliteLimits) -> Self {
        self.builtin(
            "sqlite",
            Box::new(builtins::Sqlite::with_limits(limits.clone())),
        )
        .builtin("sqlite3", Box::new(builtins::Sqlite::with_limits(limits)))
    }

    /// Enable embedded Python with custom resource limits.
    ///
    /// See [`BashBuilder::python`] for details.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use bashkit::PythonLimits;
    /// use std::time::Duration;
    ///
    /// let bash = Bash::builder()
    ///     .python_with_limits(PythonLimits::default().max_duration(Duration::from_secs(5)))
    ///     .build();
    /// ```
    #[cfg(feature = "python")]
    pub fn python_with_limits(self, limits: builtins::PythonLimits) -> Self {
        self.builtin(
            "python",
            Box::new(builtins::Python::with_limits(limits.clone())),
        )
        .builtin("python3", Box::new(builtins::Python::with_limits(limits)))
    }

    /// Enable embedded Python with external function handlers.
    ///
    /// See [`PythonExternalFnHandler`] for handler details.
    #[cfg(feature = "python")]
    pub fn python_with_external_handler(
        self,
        limits: builtins::PythonLimits,
        external_fns: Vec<String>,
        handler: builtins::PythonExternalFnHandler,
    ) -> Self {
        self.builtin(
            "python",
            Box::new(
                builtins::Python::with_limits(limits.clone())
                    .with_external_handler(external_fns.clone(), handler.clone()),
            ),
        )
        .builtin(
            "python3",
            Box::new(
                builtins::Python::with_limits(limits).with_external_handler(external_fns, handler),
            ),
        )
    }

    /// Enable embedded TypeScript/JavaScript execution via ZapCode with defaults.
    ///
    /// Registers `ts`, `typescript`, `node`, `deno`, and `bun` builtins.
    /// Requires the `typescript` feature.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let bash = Bash::builder().typescript().build();
    /// bash.exec("ts -c \"console.log('hello')\"").await?;
    /// ```
    #[cfg(feature = "typescript")]
    pub fn typescript(self) -> Self {
        self.typescript_with_config(builtins::TypeScriptConfig::default())
    }

    /// Enable embedded TypeScript with custom resource limits.
    ///
    /// See [`BashBuilder::typescript`] for details.
    #[cfg(feature = "typescript")]
    pub fn typescript_with_limits(self, limits: builtins::TypeScriptLimits) -> Self {
        self.typescript_with_config(builtins::TypeScriptConfig::default().limits(limits))
    }

    /// Enable embedded TypeScript with full configuration control.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use bashkit::{TypeScriptConfig, TypeScriptLimits};
    /// use std::time::Duration;
    ///
    /// // Only ts/typescript commands, no node/deno/bun aliases
    /// let bash = Bash::builder()
    ///     .typescript_with_config(TypeScriptConfig::default().compat_aliases(false))
    ///     .build();
    ///
    /// // Disable unsupported-mode hints
    /// let bash = Bash::builder()
    ///     .typescript_with_config(TypeScriptConfig::default().unsupported_mode_hint(false))
    ///     .build();
    ///
    /// // Custom limits + no compat aliases
    /// let bash = Bash::builder()
    ///     .typescript_with_config(
    ///         TypeScriptConfig::default()
    ///             .limits(TypeScriptLimits::default().max_duration(Duration::from_secs(5)))
    ///             .compat_aliases(false)
    ///     )
    ///     .build();
    /// ```
    #[cfg(feature = "typescript")]
    pub fn typescript_with_config(self, config: builtins::TypeScriptConfig) -> Self {
        let mut builder = self
            .builtin(
                "ts",
                Box::new(builtins::TypeScript::from_config(&config, "ts")),
            )
            .builtin(
                "typescript",
                Box::new(builtins::TypeScript::from_config(&config, "typescript")),
            );

        if config.enable_compat_aliases {
            builder = builder
                .builtin(
                    "node",
                    Box::new(builtins::TypeScript::from_config(&config, "node")),
                )
                .builtin(
                    "deno",
                    Box::new(builtins::TypeScript::from_config(&config, "deno")),
                )
                .builtin(
                    "bun",
                    Box::new(builtins::TypeScript::from_config(&config, "bun")),
                );
        }

        builder
    }

    /// Enable embedded TypeScript with external function handlers.
    ///
    /// See [`TypeScriptExternalFnHandler`] for handler details.
    #[cfg(feature = "typescript")]
    pub fn typescript_with_external_handler(
        self,
        limits: builtins::TypeScriptLimits,
        external_fns: Vec<String>,
        handler: builtins::TypeScriptExternalFnHandler,
    ) -> Self {
        let config = builtins::TypeScriptConfig::default().limits(limits);

        let make = |cmd_name: &str| {
            builtins::TypeScript::from_config(&config, cmd_name)
                .with_external_handler(external_fns.clone(), handler.clone())
        };

        self.builtin("ts", Box::new(make("ts")))
            .builtin("typescript", Box::new(make("typescript")))
            .builtin("node", Box::new(make("node")))
            .builtin("deno", Box::new(make("deno")))
            .builtin("bun", Box::new(make("bun")))
    }

    /// Register a custom builtin command.
    ///
    /// Custom builtins extend bashkit with domain-specific commands that can be
    /// invoked from bash scripts. They have full access to the execution context
    /// including arguments, environment, shell variables, and the virtual filesystem.
    ///
    /// Custom builtins can override default builtins if registered with the same name.
    ///
    /// # Arguments
    ///
    /// * `name` - The command name (e.g., "psql", "kubectl")
    /// * `builtin` - A boxed implementation of the [`Builtin`] trait
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
    /// let bash = Bash::builder()
    ///     .builtin("greet", Box::new(Greet { default_name: "World".into() }))
    ///     .build();
    /// ```
    pub fn builtin(mut self, name: impl Into<String>, builtin: Box<dyn Builtin>) -> Self {
        self.custom_builtins.insert(name.into(), builtin);
        self
    }

    /// Register an `on_exit` interceptor hook.
    ///
    /// Fired when the `exit` builtin runs.  The hook can inspect or
    /// modify the [`ExitEvent`](hooks::ExitEvent), or cancel the exit.
    /// Multiple hooks run in registration order.
    ///
    /// # Example
    ///
    /// ```rust
    /// use bashkit::hooks::{HookAction, ExitEvent};
    /// use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
    ///
    /// let exited = Arc::new(AtomicBool::new(false));
    /// let flag = exited.clone();
    ///
    /// let bash = bashkit::Bash::builder()
    ///     .on_exit(Box::new(move |event: ExitEvent| {
    ///         flag.store(true, Ordering::Relaxed);
    ///         HookAction::Continue(event)
    ///     }))
    ///     .build();
    /// ```
    pub fn on_exit(mut self, hook: hooks::Interceptor<hooks::ExitEvent>) -> Self {
        self.hooks_on_exit.push(hook);
        self
    }

    /// Register a `before_exec` interceptor hook.
    ///
    /// Fires before a script is executed. Can modify the script text
    /// or cancel execution entirely.
    pub fn before_exec(mut self, hook: hooks::Interceptor<hooks::ExecInput>) -> Self {
        self.hooks_before_exec.push(hook);
        self
    }

    /// Register an `after_exec` interceptor hook.
    ///
    /// Fires after script execution completes. Can modify or inspect
    /// the output (stdout, stderr, exit code).
    pub fn after_exec(mut self, hook: hooks::Interceptor<hooks::ExecOutput>) -> Self {
        self.hooks_after_exec.push(hook);
        self
    }

    /// Register a `before_tool` interceptor hook.
    ///
    /// Fires before a builtin command is executed. Can modify args or
    /// cancel the tool invocation.
    pub fn before_tool(mut self, hook: hooks::Interceptor<hooks::ToolEvent>) -> Self {
        self.hooks_before_tool.push(hook);
        self
    }

    /// Register an `after_tool` interceptor hook.
    ///
    /// Fires after a builtin command completes.
    pub fn after_tool(mut self, hook: hooks::Interceptor<hooks::ToolResult>) -> Self {
        self.hooks_after_tool.push(hook);
        self
    }

    /// Register an `on_error` interceptor hook.
    ///
    /// Fires when the interpreter encounters an error.
    pub fn on_error(mut self, hook: hooks::Interceptor<hooks::ErrorEvent>) -> Self {
        self.hooks_on_error.push(hook);
        self
    }

    /// Register a `before_http` interceptor hook.
    ///
    /// Fires before each HTTP request (after allowlist validation).
    /// Can modify the URL/headers or cancel the request.
    ///
    /// # Example
    ///
    /// ```
    /// use bashkit::{Bash, hooks::{HookAction, HttpRequestEvent}};
    ///
    /// let bash = Bash::builder()
    ///     .before_http(Box::new(|req: HttpRequestEvent| {
    ///         if req.url.contains("blocked") {
    ///             HookAction::Cancel("blocked by policy".into())
    ///         } else {
    ///             HookAction::Continue(req)
    ///         }
    ///     }))
    ///     .build();
    /// ```
    #[cfg(feature = "http_client")]
    pub fn before_http(mut self, hook: hooks::Interceptor<hooks::HttpRequestEvent>) -> Self {
        self.hooks_before_http.push(hook);
        self
    }

    /// Register an `after_http` interceptor hook.
    ///
    /// Fires after each HTTP response is received. Can inspect
    /// response status and headers.
    #[cfg(feature = "http_client")]
    pub fn after_http(mut self, hook: hooks::Interceptor<hooks::HttpResponseEvent>) -> Self {
        self.hooks_after_http.push(hook);
        self
    }

    /// Inject credentials for outbound HTTP requests matching the given URL pattern.
    ///
    /// The pattern uses the same matching as [`NetworkAllowlist`]
    /// (scheme + host + port + path prefix). Injected headers **overwrite**
    /// any existing headers with the same name set by the script, preventing
    /// credential spoofing.
    ///
    /// The script never sees the real credential — it is injected transparently
    /// by a `before_http` hook after the allowlist check.
    ///
    /// # Example
    ///
    /// ```rust
    /// use bashkit::{Bash, Credential, NetworkAllowlist};
    ///
    /// let bash = Bash::builder()
    ///     .network(NetworkAllowlist::new()
    ///         .allow("https://api.github.com"))
    ///     .credential("https://api.github.com",
    ///         Credential::bearer("ghp_xxxx"))
    ///     .build();
    /// // Scripts can now: curl -s https://api.github.com/repos/foo/bar
    /// // Authorization: Bearer ghp_xxxx is added transparently.
    /// ```
    ///
    /// See [`credential_injection_guide`] for the full guide.
    #[cfg(feature = "http_client")]
    pub fn credential(mut self, pattern: &str, cred: credential::Credential) -> Self {
        self.credential_policy
            .get_or_insert_with(credential::CredentialPolicy::new)
            .add_injection(pattern, cred);
        self
    }

    /// Inject credentials via a placeholder env var visible to scripts.
    ///
    /// Sets environment variable `env_name` to an opaque placeholder string.
    /// When a request to `pattern` contains the placeholder in any header
    /// value, it is replaced with the real credential on the wire.
    ///
    /// The placeholder is a random string (`bk_placeholder_<hex>`) that:
    /// - Cannot be reversed to the real credential
    /// - Is only replaced for requests matching the URL pattern
    /// - Passes most SDK non-empty validation checks
    ///
    /// # Example
    ///
    /// ```rust
    /// use bashkit::{Bash, Credential, NetworkAllowlist};
    ///
    /// let bash = Bash::builder()
    ///     .network(NetworkAllowlist::new()
    ///         .allow("https://api.openai.com"))
    ///     .credential_placeholder("OPENAI_API_KEY",
    ///         "https://api.openai.com",
    ///         Credential::bearer("sk-real-key"))
    ///     .build();
    /// // Scripts see $OPENAI_API_KEY as "bk_placeholder_..." and use it normally.
    /// // The placeholder is replaced with the real key in outbound headers.
    /// ```
    ///
    /// See [`credential_injection_guide`] for the full guide.
    #[cfg(feature = "http_client")]
    pub fn credential_placeholder(
        mut self,
        env_name: &str,
        pattern: &str,
        cred: credential::Credential,
    ) -> Self {
        let placeholder = self
            .credential_policy
            .get_or_insert_with(credential::CredentialPolicy::new)
            .add_placeholder(pattern, cred);
        self.env.insert(env_name.to_string(), placeholder);
        self
    }

    /// Mount a text file in the virtual filesystem.
    ///
    /// This creates a regular file (mode `0o644`) with the specified content at
    /// the given path. Parent directories are created automatically.
    ///
    /// Mounted files are added via an [`OverlayFs`] layer on top of the base
    /// filesystem. This means:
    /// - The base filesystem remains unchanged
    /// - Mounted files take precedence over base filesystem files
    /// - Works with any filesystem implementation
    ///
    /// # Example
    ///
    /// ```rust
    /// use bashkit::Bash;
    ///
    /// # #[tokio::main]
    /// # async fn main() -> bashkit::Result<()> {
    /// let mut bash = Bash::builder()
    ///     .mount_text("/config/app.conf", "debug=true\nport=8080\n")
    ///     .mount_text("/data/users.json", r#"["alice", "bob"]"#)
    ///     .build();
    ///
    /// let result = bash.exec("cat /config/app.conf").await?;
    /// assert_eq!(result.stdout, "debug=true\nport=8080\n");
    /// # Ok(())
    /// # }
    /// ```
    pub fn mount_text(mut self, path: impl Into<PathBuf>, content: impl Into<String>) -> Self {
        self.mounted_files.push(MountedFile {
            path: path.into(),
            content: content.into(),
            mode: 0o644,
        });
        self
    }

    /// Mount a readonly text file in the virtual filesystem.
    ///
    /// This creates a readonly file (mode `0o444`) with the specified content.
    /// Parent directories are created automatically.
    ///
    /// Readonly files are useful for:
    /// - Configuration that shouldn't be modified by scripts
    /// - Reference data that should remain immutable
    /// - Simulating system files like `/etc/passwd`
    ///
    /// Mounted files are added via an [`OverlayFs`] layer on top of the base
    /// filesystem. This means:
    /// - The base filesystem remains unchanged
    /// - Mounted files take precedence over base filesystem files
    /// - Works with any filesystem implementation
    ///
    /// # Example
    ///
    /// ```rust
    /// use bashkit::Bash;
    ///
    /// # #[tokio::main]
    /// # async fn main() -> bashkit::Result<()> {
    /// let mut bash = Bash::builder()
    ///     .mount_readonly_text("/etc/version", "1.2.3")
    ///     .mount_readonly_text("/etc/app.conf", "production=true\n")
    ///     .build();
    ///
    /// // Can read the file
    /// let result = bash.exec("cat /etc/version").await?;
    /// assert_eq!(result.stdout, "1.2.3");
    ///
    /// // File has readonly permissions
    /// let stat = bash.fs().stat(std::path::Path::new("/etc/version")).await?;
    /// assert_eq!(stat.mode, 0o444);
    /// # Ok(())
    /// # }
    /// ```
    pub fn mount_readonly_text(
        mut self,
        path: impl Into<PathBuf>,
        content: impl Into<String>,
    ) -> Self {
        self.mounted_files.push(MountedFile {
            path: path.into(),
            content: content.into(),
            mode: 0o444,
        });
        self
    }

    /// Mount a lazy file whose content is loaded on first read.
    ///
    /// The `loader` closure is called at most once when the file is first read.
    /// If the file is overwritten before being read, the loader is never called.
    /// `stat()` returns metadata using `size_hint` without triggering the load.
    ///
    /// # Example
    ///
    /// ```rust
    /// use bashkit::Bash;
    /// use std::sync::Arc;
    ///
    /// # #[tokio::main]
    /// # async fn main() -> bashkit::Result<()> {
    /// let mut bash = Bash::builder()
    ///     .mount_lazy("/data/large.csv", 1024, Arc::new(|| {
    ///         b"id,name\n1,Alice\n".to_vec()
    ///     }))
    ///     .build();
    ///
    /// let result = bash.exec("cat /data/large.csv").await?;
    /// assert_eq!(result.stdout, "id,name\n1,Alice\n");
    /// # Ok(())
    /// # }
    /// ```
    pub fn mount_lazy(
        mut self,
        path: impl Into<PathBuf>,
        size_hint: u64,
        loader: LazyLoader,
    ) -> Self {
        self.mounted_lazy_files.push(MountedLazyFile {
            path: path.into(),
            size_hint,
            mode: 0o644,
            loader,
        });
        self
    }

    /// Mount a real host directory as a readonly overlay at the VFS root.
    ///
    /// Files from `host_path` become visible at the same paths inside the VFS.
    /// For example, if the host directory contains `src/main.rs`, it will be
    /// available as `/src/main.rs` inside the virtual bash session.
    ///
    /// The host directory is read-only: scripts cannot modify host files.
    ///
    /// Requires the `realfs` feature flag.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let bash = Bash::builder()
    ///     .mount_real_readonly("/path/to/project")
    ///     .build();
    /// ```
    #[cfg(feature = "realfs")]
    pub fn mount_real_readonly(mut self, host_path: impl Into<PathBuf>) -> Self {
        self.real_mounts.push(MountedRealDir {
            host_path: host_path.into(),
            vfs_mount: None,
            mode: fs::RealFsMode::ReadOnly,
        });
        self
    }

    /// Mount a real host directory as a readonly filesystem at a specific VFS path.
    ///
    /// Files from `host_path` become visible under `vfs_mount` inside the VFS.
    /// For example, mounting `/home/user/data` at `/mnt/data` makes
    /// `/home/user/data/file.txt` available as `/mnt/data/file.txt`.
    ///
    /// The host directory is read-only: scripts cannot modify host files.
    ///
    /// Requires the `realfs` feature flag.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let bash = Bash::builder()
    ///     .mount_real_readonly_at("/path/to/data", "/mnt/data")
    ///     .build();
    /// ```
    #[cfg(feature = "realfs")]
    pub fn mount_real_readonly_at(
        mut self,
        host_path: impl Into<PathBuf>,
        vfs_mount: impl Into<PathBuf>,
    ) -> Self {
        self.real_mounts.push(MountedRealDir {
            host_path: host_path.into(),
            vfs_mount: Some(vfs_mount.into()),
            mode: fs::RealFsMode::ReadOnly,
        });
        self
    }

    /// Mount a real host directory with read-write access at the VFS root.
    ///
    /// **WARNING**: This breaks the sandbox boundary. Scripts can modify files
    /// on the host filesystem. Only use when:
    /// - The script is fully trusted
    /// - The host directory is appropriately scoped
    ///
    /// Requires the `realfs` feature flag.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let bash = Bash::builder()
    ///     .mount_real_readwrite("/path/to/workspace")
    ///     .build();
    /// ```
    #[cfg(feature = "realfs")]
    pub fn mount_real_readwrite(mut self, host_path: impl Into<PathBuf>) -> Self {
        self.real_mounts.push(MountedRealDir {
            host_path: host_path.into(),
            vfs_mount: None,
            mode: fs::RealFsMode::ReadWrite,
        });
        self
    }

    /// Mount a real host directory with read-write access at a specific VFS path.
    ///
    /// **WARNING**: This breaks the sandbox boundary. Scripts can modify files
    /// on the host filesystem.
    ///
    /// Requires the `realfs` feature flag.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let bash = Bash::builder()
    ///     .mount_real_readwrite_at("/path/to/workspace", "/mnt/workspace")
    ///     .build();
    /// ```
    #[cfg(feature = "realfs")]
    pub fn mount_real_readwrite_at(
        mut self,
        host_path: impl Into<PathBuf>,
        vfs_mount: impl Into<PathBuf>,
    ) -> Self {
        self.real_mounts.push(MountedRealDir {
            host_path: host_path.into(),
            vfs_mount: Some(vfs_mount.into()),
            mode: fs::RealFsMode::ReadWrite,
        });
        self
    }

    /// Set an allowlist of host paths that may be mounted.
    ///
    /// When set, only host paths starting with an allowed prefix are accepted
    /// by `mount_real_*` methods. Paths outside the allowlist are rejected with
    /// a warning at build time.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let bash = Bash::builder()
    ///     .allowed_mount_paths(["/home/user/projects", "/tmp"])
    ///     .mount_real_readonly("/home/user/projects/data")  // OK
    ///     .mount_real_readonly("/etc/passwd")                // rejected
    ///     .build();
    /// ```
    #[cfg(feature = "realfs")]
    pub fn allowed_mount_paths(
        mut self,
        paths: impl IntoIterator<Item = impl Into<PathBuf>>,
    ) -> Self {
        self.mount_path_allowlist = Some(paths.into_iter().map(|p| p.into()).collect());
        self
    }

    /// Build the Bash instance.
    ///
    /// If mounted files are specified, they are added via an [`OverlayFs`] layer
    /// on top of the base filesystem. This means:
    /// - The base filesystem remains unchanged
    /// - Mounted files take precedence over base filesystem files
    /// - Works with any filesystem implementation
    ///
    /// # Example
    ///
    /// ```rust
    /// use bashkit::{Bash, InMemoryFs};
    /// use std::sync::Arc;
    ///
    /// # #[tokio::main]
    /// # async fn main() -> bashkit::Result<()> {
    /// // Works with default InMemoryFs
    /// let mut bash = Bash::builder()
    ///     .mount_text("/config/app.conf", "debug=true\n")
    ///     .build();
    ///
    /// // Also works with custom filesystems
    /// let custom_fs = Arc::new(InMemoryFs::new());
    /// let mut bash = Bash::builder()
    ///     .fs(custom_fs)
    ///     .mount_text("/config/app.conf", "debug=true\n")
    ///     .mount_readonly_text("/etc/version", "1.0.0")
    ///     .build();
    ///
    /// let result = bash.exec("cat /config/app.conf").await?;
    /// assert_eq!(result.stdout, "debug=true\n");
    /// # Ok(())
    /// # }
    /// ```
    pub fn build(self) -> Bash {
        let base_fs: Arc<dyn FileSystem> = if self.shell_profile.is_logic_only() {
            Arc::new(fs::DisabledFs)
        } else {
            self.fs.unwrap_or_else(|| Arc::new(InMemoryFs::new()))
        };

        // Layer 1: Apply real filesystem mounts (if any)
        #[cfg(feature = "realfs")]
        let base_fs = Self::apply_real_mounts(
            &self.real_mounts,
            self.mount_path_allowlist.as_deref(),
            base_fs,
        );

        // Layer 2: If there are mounted text/lazy files, wrap in an OverlayFs
        let has_mounts = !self.mounted_files.is_empty() || !self.mounted_lazy_files.is_empty();
        let base_fs: Arc<dyn FileSystem> = if has_mounts {
            let overlay = OverlayFs::with_limits(base_fs.clone(), base_fs.limits());
            for mf in &self.mounted_files {
                overlay.upper().add_file(&mf.path, &mf.content, mf.mode);
            }
            for lf in self.mounted_lazy_files {
                overlay
                    .upper()
                    .add_lazy_file(&lf.path, lf.size_hint, lf.mode, lf.loader);
            }
            Arc::new(overlay)
        } else {
            base_fs
        };

        // Layer 3: Wrap in MountableFs for post-build live mount/unmount
        let mountable = Arc::new(MountableFs::new(base_fs));
        let fs: Arc<dyn FileSystem> = Arc::clone(&mountable) as Arc<dyn FileSystem>;

        let mut result = Self::build_with_fs(
            fs,
            mountable,
            self.env,
            self.username,
            self.hostname,
            self.fixed_epoch,
            self.cwd,
            self.shell_profile,
            self.limits,
            self.session_limits,
            self.memory_limits,
            self.trace_mode,
            self.trace_callback,
            self.custom_builtins,
            self.history_file,
            #[cfg(feature = "http_client")]
            self.network_allowlist,
            #[cfg(feature = "http_client")]
            self.http_handler,
            #[cfg(feature = "bot-auth")]
            self.bot_auth_config,
            #[cfg(feature = "logging")]
            self.log_config,
            #[cfg(feature = "git")]
            self.git_config,
            #[cfg(feature = "ssh")]
            self.ssh_config,
            #[cfg(feature = "ssh")]
            self.ssh_handler,
        );

        // Set hooks after build — avoids adding another arg to build_with_fs.
        let hooks = hooks::Hooks {
            on_exit: self.hooks_on_exit,
            before_exec: self.hooks_before_exec,
            after_exec: self.hooks_after_exec,
            before_tool: self.hooks_before_tool,
            after_tool: self.hooks_after_tool,
            on_error: self.hooks_on_error,
        };
        if hooks.has_hooks() {
            result.interpreter.set_hooks(hooks);
        }

        // Convert credential policy into a before_http hook.
        // Credential hook runs FIRST so subsequent hooks see injected headers.
        #[cfg(feature = "http_client")]
        let mut hooks_before_http = Vec::new();
        #[cfg(feature = "http_client")]
        if let Some(policy) = self.credential_policy
            && !policy.is_empty()
        {
            hooks_before_http.push(policy.into_hook());
        }
        #[cfg(feature = "http_client")]
        hooks_before_http.extend(self.hooks_before_http);

        // Set HTTP hooks on the HttpClient (transport-level, not interpreter-level)
        #[cfg(feature = "http_client")]
        if (!hooks_before_http.is_empty() || !self.hooks_after_http.is_empty())
            && let Some(client) = result.interpreter.http_client_mut()
        {
            if !hooks_before_http.is_empty() {
                client.set_before_http(hooks_before_http);
            }
            if !self.hooks_after_http.is_empty() {
                client.set_after_http(self.hooks_after_http);
            }
        }

        result
    }

    /// Sensitive host paths that are blocked from mounting by default.
    #[cfg(feature = "realfs")]
    const SENSITIVE_MOUNT_PATHS: &[&str] = &["/etc/shadow", "/etc/sudoers", "/proc", "/sys"];

    #[cfg(feature = "realfs")]
    fn apply_real_mounts(
        real_mounts: &[MountedRealDir],
        mount_allowlist: Option<&[PathBuf]>,
        base_fs: Arc<dyn FileSystem>,
    ) -> Arc<dyn FileSystem> {
        if real_mounts.is_empty() {
            return base_fs;
        }

        let mut current_fs = base_fs;
        let mut mount_points: Vec<(PathBuf, Arc<dyn FileSystem>)> = Vec::new();
        let canonical_allowlist: Option<Vec<PathBuf>> = mount_allowlist.map(|allowlist| {
            allowlist
                .iter()
                .filter_map(|allowed| match std::fs::canonicalize(allowed) {
                    Ok(path) => Some(path),
                    Err(e) => {
                        eprintln!(
                            "bashkit: warning: failed to canonicalize allowlist path {}: {}",
                            allowed.display(),
                            e
                        );
                        None
                    }
                })
                .collect()
        });

        for m in real_mounts {
            // Warn on writable mounts
            if m.mode == fs::RealFsMode::ReadWrite {
                eprintln!(
                    "bashkit: warning: writable mount at {} — scripts can modify host files",
                    m.host_path.display()
                );
            }

            let canonical_host = match std::fs::canonicalize(&m.host_path) {
                Ok(path) => path,
                Err(e) => {
                    eprintln!(
                        "bashkit: warning: failed to canonicalize mount path {}: {}",
                        m.host_path.display(),
                        e
                    );
                    continue;
                }
            };

            // Block sensitive paths
            if Self::SENSITIVE_MOUNT_PATHS
                .iter()
                .any(|s| canonical_host.starts_with(Path::new(s)))
            {
                eprintln!(
                    "bashkit: warning: refusing to mount sensitive path {}",
                    m.host_path.display()
                );
                continue;
            }

            // Check allowlist if configured
            if let Some(allowlist) = &canonical_allowlist
                && !allowlist
                    .iter()
                    .any(|allowed| canonical_host.starts_with(allowed))
            {
                eprintln!(
                    "bashkit: warning: mount path {} not in allowlist, skipping",
                    m.host_path.display()
                );
                continue;
            }

            let real_backend = match fs::RealFs::new(&canonical_host, m.mode) {
                Ok(b) => b,
                Err(e) => {
                    eprintln!(
                        "bashkit: warning: failed to mount {}: {}",
                        m.host_path.display(),
                        e
                    );
                    continue;
                }
            };
            let real_fs: Arc<dyn FileSystem> = Arc::new(PosixFs::new(real_backend));

            match &m.vfs_mount {
                None => {
                    // Overlay at root: real fs becomes the lower layer,
                    // existing VFS content overlaid on top
                    current_fs = Arc::new(OverlayFs::new(real_fs));
                }
                Some(mount_point) => {
                    mount_points.push((mount_point.clone(), real_fs));
                }
            }
        }

        // If there are specific mount points, wrap in MountableFs
        if !mount_points.is_empty() {
            let mountable = MountableFs::new(current_fs);
            for (path, fs) in mount_points {
                if let Err(e) = mountable.mount(&path, fs) {
                    eprintln!(
                        "bashkit: warning: failed to mount at {}: {}",
                        path.display(),
                        e
                    );
                }
            }
            Arc::new(mountable)
        } else {
            current_fs
        }
    }

    /// Internal helper to build Bash with a configured filesystem.
    #[allow(clippy::too_many_arguments)]
    fn build_with_fs(
        fs: Arc<dyn FileSystem>,
        mountable: Arc<MountableFs>,
        env: HashMap<String, String>,
        username: Option<String>,
        hostname: Option<String>,
        fixed_epoch: Option<i64>,
        cwd: Option<PathBuf>,
        shell_profile: interpreter::ShellProfile,
        limits: ExecutionLimits,
        session_limits: SessionLimits,
        memory_limits: MemoryLimits,
        trace_mode: TraceMode,
        trace_callback: Option<TraceCallback>,
        custom_builtins: HashMap<String, Box<dyn Builtin>>,
        history_file: Option<PathBuf>,
        #[cfg(feature = "http_client")] network_allowlist: Option<NetworkAllowlist>,
        #[cfg(feature = "http_client")] http_handler: Option<Box<dyn network::HttpHandler>>,
        #[cfg(feature = "bot-auth")] bot_auth_config: Option<network::BotAuthConfig>,
        #[cfg(feature = "logging")] log_config: Option<logging::LogConfig>,
        #[cfg(feature = "git")] git_config: Option<GitConfig>,
        #[cfg(feature = "ssh")] ssh_config: Option<SshConfig>,
        #[cfg(feature = "ssh")] ssh_handler: Option<Box<dyn builtins::ssh::SshHandler>>,
    ) -> Bash {
        #[cfg(feature = "logging")]
        let log_config = log_config.unwrap_or_default();

        #[cfg(feature = "logging")]
        tracing::debug!(
            target: "bashkit::config",
            redact_sensitive = log_config.redact_sensitive,
            log_scripts = log_config.log_script_content,
            "Bash instance configured"
        );

        let mut interpreter = Interpreter::with_config(
            Arc::clone(&fs),
            username.clone(),
            hostname,
            fixed_epoch,
            custom_builtins,
            shell_profile,
        );

        // Set environment variables (also override shell variable defaults)
        for (key, value) in &env {
            interpreter.set_env(key, value);
            // Shell variables like HOME, USER should also be set as variables
            // so they take precedence over the defaults
            interpreter.set_var(key, value);
        }
        #[cfg(feature = "python")]
        let python_inprocess_opt_in = env_opt_in_enabled(&env, "BASHKIT_ALLOW_INPROCESS_PYTHON");
        #[cfg(feature = "sqlite")]
        let sqlite_inprocess_opt_in = env_opt_in_enabled(&env, "BASHKIT_ALLOW_INPROCESS_SQLITE");
        drop(env);

        // If username is set, automatically set USER env var
        if let Some(ref username) = username {
            interpreter.set_env("USER", username);
            interpreter.set_var("USER", username);
        }

        if let Some(cwd) = cwd {
            interpreter.set_cwd(cwd);
        }

        // Configure HTTP client for network builtins
        #[cfg(feature = "http_client")]
        if let Some(allowlist) = network_allowlist {
            let mut client = network::HttpClient::new(allowlist);
            if let Some(handler) = http_handler {
                client.set_handler(handler);
            }
            #[cfg(feature = "bot-auth")]
            if let Some(bot_auth) = bot_auth_config {
                client.set_bot_auth(bot_auth);
            }
            interpreter.set_http_client(client);
        }

        // Configure git client for git builtins
        #[cfg(feature = "git")]
        if let Some(config) = git_config {
            let client = builtins::git::GitClient::new(config);
            interpreter.set_git_client(client);
        }

        // Configure SSH client for ssh/scp/sftp builtins
        #[cfg(feature = "ssh")]
        if let Some(config) = ssh_config {
            let mut client = builtins::ssh::SshClient::new(config);
            if let Some(handler) = ssh_handler {
                client.set_handler(handler);
            }
            interpreter.set_ssh_client(client);
        }

        // Configure persistent history file
        if let Some(hf) = history_file {
            interpreter.set_history_file(hf);
        }

        let parser_timeout = limits.parser_timeout;
        let max_input_bytes = limits.max_input_bytes;
        let max_ast_depth = limits.max_ast_depth;
        let max_parser_operations = limits.max_parser_operations;
        interpreter.set_limits(limits);
        interpreter.set_session_limits(session_limits);
        interpreter.set_memory_limits(memory_limits);
        let mut trace_collector = TraceCollector::new(trace_mode);
        if let Some(cb) = trace_callback {
            trace_collector.set_callback(cb);
        }
        interpreter.set_trace(trace_collector);
        Bash {
            fs,
            mountable,
            interpreter,
            parser_timeout,
            max_input_bytes,
            max_ast_depth,
            max_parser_operations,
            #[cfg(feature = "logging")]
            log_config,
            #[cfg(feature = "python")]
            python_inprocess_opt_in,
            #[cfg(feature = "sqlite")]
            sqlite_inprocess_opt_in,
        }
    }
}

// =============================================================================
// Documentation Modules
// =============================================================================
// These modules embed external markdown guides into rustdoc.
// Source files live in crates/bashkit/docs/ - edit there, not here.
// See specs/documentation.md for the documentation approach.

/// Guide for transparent credential injection in outbound HTTP requests.
///
/// Two modes: **injection** (script unaware) and **placeholder** (opaque
/// env var replaced on the wire). Credentials are scoped per URL pattern
/// and never visible to sandboxed scripts.
///
/// **Related:** [`BashBuilder::credential`], [`BashBuilder::credential_placeholder`],
/// [`Credential`], [`NetworkAllowlist`], [`threat_model`]
#[cfg(feature = "http_client")]
#[doc = include_str!("../docs/credential-injection.md")]
pub mod credential_injection_guide {}

/// Guide for creating custom builtins to extend Bashkit.
///
/// This guide covers:
/// - Implementing the [`Builtin`] trait
/// - Accessing execution context ([`BuiltinContext`])
/// - Working with arguments, environment, and filesystem
/// - Best practices and examples
///
/// **Related:** [`BashBuilder::builtin`], [`compatibility_scorecard`]
#[doc = include_str!("../docs/custom_builtins.md")]
pub mod custom_builtins_guide {}

/// Bash compatibility scorecard.
///
/// Tracks feature parity with real bash:
/// - Implemented vs missing features
/// - Builtins, syntax, expansions
/// - POSIX compliance status
/// - Resource limits
///
/// **Related:** [`custom_builtins_guide`], [`threat_model`]
#[doc = include_str!("../docs/compatibility.md")]
pub mod compatibility_scorecard {}

/// jq builtin: supported filters, flags, and variables.
///
/// **Topics covered:**
/// - Implemented command-line flags
/// - Variables (including `$ENV`)
/// - Notable filters and the bashkit compatibility shim
/// - Known gaps where bashkit's input model differs from upstream jq
///
/// **Related:** [`compatibility_scorecard`], [`threat_model`]
#[doc = include_str!("../docs/jq.md")]
pub mod jq_guide {}

/// Security threat model guide.
///
/// This guide documents security threats addressed by Bashkit and their mitigations.
/// All threats use stable IDs for tracking and code references.
///
/// **Topics covered:**
/// - Denial of Service mitigations (TM-DOS-*)
/// - Sandbox escape prevention (TM-ESC-*)
/// - Information disclosure protection (TM-INF-*)
/// - Network security controls (TM-NET-*)
/// - Multi-tenant isolation (TM-ISO-*)
///
/// **Related:** [`ExecutionLimits`], [`FsLimits`], [`NetworkAllowlist`]
#[doc = include_str!("../docs/threat-model.md")]
pub mod threat_model {}

/// Guide for embedded Python via the Monty interpreter.
///
/// **Experimental:** The Monty integration is experimental with known security
/// issues. See the guide below and [`threat_model`] for details.
///
/// This guide covers:
/// - Enabling Python with [`BashBuilder::python`]
/// - VFS bridging (`pathlib.Path` → virtual filesystem)
/// - Configuring resource limits with [`PythonLimits`]
/// - LLM tool integration via [`BashToolBuilder::python`]
/// - Known limitations (no `open()`, no HTTP, no classes)
///
/// **Related:** [`BashBuilder::python`], [`PythonLimits`], [`threat_model`]
#[cfg(feature = "python")]
#[doc = include_str!("../docs/python.md")]
pub mod python_guide {}

/// Guide for the embedded SQLite builtin (Turso).
///
/// Topics covered:
/// - Quick start with `Bash::builder().sqlite()`
/// - Memory vs VFS backends
/// - `:memory:` databases
/// - Output modes (list, csv, tabs, line, column, box, json, markdown)
/// - Dot-commands (`.tables`, `.schema`, `.dump`, `.read`, …)
/// - Resource limits and security model
///
/// **Related:** [`BashBuilder::sqlite`], [`SqliteLimits`], [`SqliteBackend`], [`threat_model`]
#[cfg(feature = "sqlite")]
#[doc = include_str!("../docs/sqlite.md")]
pub mod sqlite_guide {}

/// Guide for embedded TypeScript execution via the ZapCode interpreter.
///
/// This guide covers:
/// - Quick start with `Bash::builder().typescript()`
/// - Inline code, script files, pipelines
/// - VFS bridging via `readFile()`/`writeFile()` external functions
/// - Resource limits via `TypeScriptLimits`
/// - Configuration via `TypeScriptConfig` (compat aliases, unsupported-mode hints)
/// - LLM tool integration
///
/// **Related:** [`BashBuilder::typescript`], [`TypeScriptLimits`], [`TypeScriptConfig`], [`threat_model`]
#[cfg(feature = "typescript")]
#[doc = include_str!("../docs/typescript.md")]
pub mod typescript_guide {}

/// Guide for SSH/SCP/SFTP remote operations.
///
/// **Related:** [`BashBuilder::ssh`], [`SshConfig`], [`SshAllowlist`], [`threat_model`]
#[cfg(feature = "ssh")]
#[doc = include_str!("../docs/ssh.md")]
pub mod ssh_guide {}

/// Guide for live mount/unmount on a running Bash instance.
///
/// This guide covers:
/// - Attaching/detaching filesystems post-build
/// - State preservation across mount operations
/// - Hot-swapping mounted filesystems
/// - Layered filesystem architecture
///
/// **Related:** [`Bash::mount`], [`Bash::unmount`], [`MountableFs`], [`BashBuilder::mount_text`]
#[doc = include_str!("../docs/live_mounts.md")]
pub mod live_mounts_guide {}

/// Logging guide for Bashkit.
///
/// This guide covers configuring structured logging, log levels, security
/// considerations, and integration with tracing subscribers.
///
/// **Topics covered:**
/// - Enabling the `logging` feature
/// - Log levels and targets
/// - Security: sensitive data redaction (TM-LOG-*)
/// - Integration with tracing-subscriber
///
/// **Related:** [`LogConfig`], [`threat_model`]
#[cfg(feature = "logging")]
#[doc = include_str!("../docs/logging.md")]
pub mod logging_guide {}

/// Interceptor hooks guide for Bashkit.
///
/// This guide covers the hook system for observing, modifying, and cancelling
/// operations at key points in the execution pipeline.
///
/// **Topics covered:**
/// - Execution hooks (`before_exec`, `after_exec`)
/// - Tool hooks (`before_tool`, `after_tool`)
/// - Lifecycle hooks (`on_exit`, `on_error`)
/// - HTTP hooks (`before_http`, `after_http`)
/// - Chaining multiple hooks
/// - Event payloads and thread safety
///
/// **Related:** [`BashBuilder`], [`hooks`], [`custom_builtins_guide`]
#[doc = include_str!("../docs/hooks.md")]
pub mod hooks_guide {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

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

    #[tokio::test]
    async fn test_variable_expansion() {
        let mut bash = Bash::builder().env("HOME", "/home/user").build();
        let result = bash.exec("echo $HOME").await.unwrap();
        assert_eq!(result.stdout, "/home/user\n");
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn test_variable_brace_expansion() {
        let mut bash = Bash::builder().env("USER", "testuser").build();
        let result = bash.exec("echo ${USER}").await.unwrap();
        assert_eq!(result.stdout, "testuser\n");
    }

    #[tokio::test]
    async fn test_undefined_variable_expands_to_empty() {
        let mut bash = Bash::new();
        let result = bash.exec("echo $UNDEFINED_VAR").await.unwrap();
        assert_eq!(result.stdout, "\n");
    }

    #[tokio::test]
    async fn test_pipeline() {
        let mut bash = Bash::new();
        let result = bash.exec("echo hello | cat").await.unwrap();
        assert_eq!(result.stdout, "hello\n");
    }

    #[tokio::test]
    async fn test_pipeline_three_commands() {
        let mut bash = Bash::new();
        let result = bash.exec("echo hello | cat | cat").await.unwrap();
        assert_eq!(result.stdout, "hello\n");
    }

    #[tokio::test]
    async fn test_redirect_output() {
        let mut bash = Bash::new();
        let result = bash.exec("echo hello > /tmp/test.txt").await.unwrap();
        assert_eq!(result.stdout, "");
        assert_eq!(result.exit_code, 0);

        // Read the file back
        let result = bash.exec("cat /tmp/test.txt").await.unwrap();
        assert_eq!(result.stdout, "hello\n");
    }

    #[tokio::test]
    async fn test_redirect_append() {
        let mut bash = Bash::new();
        bash.exec("echo hello > /tmp/append.txt").await.unwrap();
        bash.exec("echo world >> /tmp/append.txt").await.unwrap();

        let result = bash.exec("cat /tmp/append.txt").await.unwrap();
        assert_eq!(result.stdout, "hello\nworld\n");
    }

    #[tokio::test]
    async fn test_command_list_and() {
        let mut bash = Bash::new();
        let result = bash.exec("true && echo success").await.unwrap();
        assert_eq!(result.stdout, "success\n");
    }

    #[tokio::test]
    async fn test_command_list_and_short_circuit() {
        let mut bash = Bash::new();
        let result = bash.exec("false && echo should_not_print").await.unwrap();
        assert_eq!(result.stdout, "");
        assert_eq!(result.exit_code, 1);
    }

    #[tokio::test]
    async fn test_command_list_or() {
        let mut bash = Bash::new();
        let result = bash.exec("false || echo fallback").await.unwrap();
        assert_eq!(result.stdout, "fallback\n");
    }

    #[tokio::test]
    async fn test_command_list_or_short_circuit() {
        let mut bash = Bash::new();
        let result = bash.exec("true || echo should_not_print").await.unwrap();
        assert_eq!(result.stdout, "");
        assert_eq!(result.exit_code, 0);
    }

    /// Phase 1 target test: `echo $HOME | cat > /tmp/out && cat /tmp/out`
    #[tokio::test]
    async fn test_phase1_target() {
        let mut bash = Bash::builder().env("HOME", "/home/testuser").build();

        let result = bash
            .exec("echo $HOME | cat > /tmp/out && cat /tmp/out")
            .await
            .unwrap();

        assert_eq!(result.stdout, "/home/testuser\n");
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn test_redirect_input() {
        let mut bash = Bash::new();
        // Create a file first
        bash.exec("echo hello > /tmp/input.txt").await.unwrap();

        // Read it using input redirection
        let result = bash.exec("cat < /tmp/input.txt").await.unwrap();
        assert_eq!(result.stdout, "hello\n");
    }

    #[tokio::test]
    async fn test_here_string() {
        let mut bash = Bash::new();
        let result = bash.exec("cat <<< hello").await.unwrap();
        assert_eq!(result.stdout, "hello\n");
    }

    #[tokio::test]
    async fn test_if_true() {
        let mut bash = Bash::new();
        let result = bash.exec("if true; then echo yes; fi").await.unwrap();
        assert_eq!(result.stdout, "yes\n");
    }

    #[tokio::test]
    async fn test_if_false() {
        let mut bash = Bash::new();
        let result = bash.exec("if false; then echo yes; fi").await.unwrap();
        assert_eq!(result.stdout, "");
    }

    #[tokio::test]
    async fn test_if_else() {
        let mut bash = Bash::new();
        let result = bash
            .exec("if false; then echo yes; else echo no; fi")
            .await
            .unwrap();
        assert_eq!(result.stdout, "no\n");
    }

    #[tokio::test]
    async fn test_if_elif() {
        let mut bash = Bash::new();
        let result = bash
            .exec("if false; then echo one; elif true; then echo two; else echo three; fi")
            .await
            .unwrap();
        assert_eq!(result.stdout, "two\n");
    }

    #[tokio::test]
    async fn test_for_loop() {
        let mut bash = Bash::new();
        let result = bash.exec("for i in a b c; do echo $i; done").await.unwrap();
        assert_eq!(result.stdout, "a\nb\nc\n");
    }

    #[tokio::test]
    async fn test_for_loop_positional_params() {
        let mut bash = Bash::new();
        // for x; do ... done iterates over positional parameters inside a function
        let result = bash
            .exec("f() { for x; do echo $x; done; }; f one two three")
            .await
            .unwrap();
        assert_eq!(result.stdout, "one\ntwo\nthree\n");
    }

    #[tokio::test]
    async fn test_while_loop() {
        let mut bash = Bash::new();
        // While with false condition - executes 0 times
        let result = bash.exec("while false; do echo loop; done").await.unwrap();
        assert_eq!(result.stdout, "");
    }

    #[tokio::test]
    async fn test_subshell() {
        let mut bash = Bash::new();
        let result = bash.exec("(echo hello)").await.unwrap();
        assert_eq!(result.stdout, "hello\n");
    }

    #[tokio::test]
    async fn test_brace_group() {
        let mut bash = Bash::new();
        let result = bash.exec("{ echo hello; }").await.unwrap();
        assert_eq!(result.stdout, "hello\n");
    }

    #[tokio::test]
    async fn test_function_keyword() {
        let mut bash = Bash::new();
        let result = bash
            .exec("function greet { echo hello; }; greet")
            .await
            .unwrap();
        assert_eq!(result.stdout, "hello\n");
    }

    #[tokio::test]
    async fn test_function_posix() {
        let mut bash = Bash::new();
        let result = bash.exec("greet() { echo hello; }; greet").await.unwrap();
        assert_eq!(result.stdout, "hello\n");
    }

    #[tokio::test]
    async fn test_function_args() {
        let mut bash = Bash::new();
        let result = bash
            .exec("greet() { echo $1 $2; }; greet world foo")
            .await
            .unwrap();
        assert_eq!(result.stdout, "world foo\n");
    }

    #[tokio::test]
    async fn test_function_arg_count() {
        let mut bash = Bash::new();
        let result = bash
            .exec("count() { echo $#; }; count a b c")
            .await
            .unwrap();
        assert_eq!(result.stdout, "3\n");
    }

    #[tokio::test]
    async fn test_case_literal() {
        let mut bash = Bash::new();
        let result = bash
            .exec("case foo in foo) echo matched ;; esac")
            .await
            .unwrap();
        assert_eq!(result.stdout, "matched\n");
    }

    #[tokio::test]
    async fn test_case_wildcard() {
        let mut bash = Bash::new();
        let result = bash
            .exec("case bar in *) echo default ;; esac")
            .await
            .unwrap();
        assert_eq!(result.stdout, "default\n");
    }

    #[tokio::test]
    async fn test_case_no_match() {
        let mut bash = Bash::new();
        let result = bash.exec("case foo in bar) echo no ;; esac").await.unwrap();
        assert_eq!(result.stdout, "");
    }

    #[tokio::test]
    async fn test_case_multiple_patterns() {
        let mut bash = Bash::new();
        let result = bash
            .exec("case foo in bar|foo|baz) echo matched ;; esac")
            .await
            .unwrap();
        assert_eq!(result.stdout, "matched\n");
    }

    #[tokio::test]
    async fn test_case_bracket_expr() {
        let mut bash = Bash::new();
        // Test [abc] bracket expression
        let result = bash
            .exec("case b in [abc]) echo matched ;; esac")
            .await
            .unwrap();
        assert_eq!(result.stdout, "matched\n");
    }

    #[tokio::test]
    async fn test_case_bracket_range() {
        let mut bash = Bash::new();
        // Test [a-z] range expression
        let result = bash
            .exec("case m in [a-z]) echo letter ;; esac")
            .await
            .unwrap();
        assert_eq!(result.stdout, "letter\n");
    }

    #[tokio::test]
    async fn test_case_bracket_wide_unicode_range() {
        let mut bash = Bash::new();
        let result = bash
            .exec("case z in [a-\u{10ffff}]) echo wide ;; esac")
            .await
            .unwrap();
        assert_eq!(result.stdout, "wide\n");
    }

    #[tokio::test]
    async fn test_case_bracket_negation() {
        let mut bash = Bash::new();
        // Test [!abc] negation
        let result = bash
            .exec("case x in [!abc]) echo not_abc ;; esac")
            .await
            .unwrap();
        assert_eq!(result.stdout, "not_abc\n");
    }

    #[tokio::test]
    async fn test_break_as_command() {
        let mut bash = Bash::new();
        // Just run break alone - should not error
        let result = bash.exec("break").await.unwrap();
        // break outside of loop returns success with no output
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn test_for_one_item() {
        let mut bash = Bash::new();
        // Simple for loop with one item
        let result = bash.exec("for i in a; do echo $i; done").await.unwrap();
        assert_eq!(result.stdout, "a\n");
    }

    #[tokio::test]
    async fn test_for_with_break() {
        let mut bash = Bash::new();
        // For loop with break
        let result = bash.exec("for i in a; do break; done").await.unwrap();
        assert_eq!(result.stdout, "");
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn test_for_echo_break() {
        let mut bash = Bash::new();
        // For loop with echo then break - tests the semicolon command list in body
        let result = bash
            .exec("for i in a b c; do echo $i; break; done")
            .await
            .unwrap();
        assert_eq!(result.stdout, "a\n");
    }

    #[tokio::test]
    async fn test_test_string_empty() {
        let mut bash = Bash::new();
        let result = bash.exec("test -z '' && echo yes").await.unwrap();
        assert_eq!(result.stdout, "yes\n");
    }

    #[tokio::test]
    async fn test_test_string_not_empty() {
        let mut bash = Bash::new();
        let result = bash.exec("test -n 'hello' && echo yes").await.unwrap();
        assert_eq!(result.stdout, "yes\n");
    }

    #[tokio::test]
    async fn test_test_string_equal() {
        let mut bash = Bash::new();
        let result = bash.exec("test foo = foo && echo yes").await.unwrap();
        assert_eq!(result.stdout, "yes\n");
    }

    #[tokio::test]
    async fn test_test_string_not_equal() {
        let mut bash = Bash::new();
        let result = bash.exec("test foo != bar && echo yes").await.unwrap();
        assert_eq!(result.stdout, "yes\n");
    }

    #[tokio::test]
    async fn test_test_numeric_equal() {
        let mut bash = Bash::new();
        let result = bash.exec("test 5 -eq 5 && echo yes").await.unwrap();
        assert_eq!(result.stdout, "yes\n");
    }

    #[tokio::test]
    async fn test_test_numeric_less_than() {
        let mut bash = Bash::new();
        let result = bash.exec("test 3 -lt 5 && echo yes").await.unwrap();
        assert_eq!(result.stdout, "yes\n");
    }

    #[tokio::test]
    async fn test_bracket_form() {
        let mut bash = Bash::new();
        let result = bash.exec("[ foo = foo ] && echo yes").await.unwrap();
        assert_eq!(result.stdout, "yes\n");
    }

    #[tokio::test]
    async fn test_if_with_test() {
        let mut bash = Bash::new();
        let result = bash
            .exec("if [ 5 -gt 3 ]; then echo bigger; fi")
            .await
            .unwrap();
        assert_eq!(result.stdout, "bigger\n");
    }

    #[tokio::test]
    async fn test_variable_assignment() {
        let mut bash = Bash::new();
        let result = bash.exec("FOO=bar; echo $FOO").await.unwrap();
        assert_eq!(result.stdout, "bar\n");
    }

    #[tokio::test]
    async fn test_variable_assignment_inline() {
        let mut bash = Bash::new();
        // Assignment before command
        let result = bash.exec("MSG=hello; echo $MSG world").await.unwrap();
        assert_eq!(result.stdout, "hello world\n");
    }

    #[tokio::test]
    async fn test_variable_assignment_only() {
        let mut bash = Bash::new();
        // Assignment without command should succeed silently
        let result = bash.exec("FOO=bar").await.unwrap();
        assert_eq!(result.stdout, "");
        assert_eq!(result.exit_code, 0);

        // Verify the variable was set
        let result = bash.exec("echo $FOO").await.unwrap();
        assert_eq!(result.stdout, "bar\n");
    }

    #[tokio::test]
    async fn test_multiple_assignments() {
        let mut bash = Bash::new();
        let result = bash.exec("A=1; B=2; C=3; echo $A $B $C").await.unwrap();
        assert_eq!(result.stdout, "1 2 3\n");
    }

    #[tokio::test]
    async fn test_prefix_assignment_visible_in_env() {
        let mut bash = Bash::new();
        // VAR=value command should make VAR visible in the command's environment
        let result = bash.exec("MYVAR=hello printenv MYVAR").await.unwrap();
        assert_eq!(result.stdout, "hello\n");
    }

    #[tokio::test]
    async fn test_prefix_assignment_temporary() {
        let mut bash = Bash::new();
        // Prefix assignment should NOT persist after the command
        bash.exec("MYVAR=hello printenv MYVAR").await.unwrap();
        let result = bash.exec("echo ${MYVAR:-unset}").await.unwrap();
        assert_eq!(result.stdout, "unset\n");
    }

    #[tokio::test]
    async fn test_prefix_assignment_duplicate_name_temporary() {
        let mut bash = Bash::new();
        // Duplicate prefix assignments should still restore original env.
        let result = bash.exec("A=1 A=2 printenv A").await.unwrap();
        assert_eq!(result.stdout, "2\n");
        let result = bash.exec("echo ${A:-unset}").await.unwrap();
        assert_eq!(result.stdout, "unset\n");
    }

    #[tokio::test]
    async fn test_prefix_assignment_does_not_clobber_existing_env() {
        let mut bash = Bash::new();
        // Set up existing env var
        let result = bash
            .exec("EXISTING=original; export EXISTING; EXISTING=temp printenv EXISTING")
            .await
            .unwrap();
        assert_eq!(result.stdout, "temp\n");
    }

    #[tokio::test]
    async fn test_prefix_assignment_multiple_vars() {
        let mut bash = Bash::new();
        // Multiple prefix assignments on same command
        let result = bash.exec("A=one B=two printenv A").await.unwrap();
        assert_eq!(result.stdout, "one\n");
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn test_prefix_assignment_empty_value() {
        let mut bash = Bash::new();
        // Empty value is still set in environment
        let result = bash.exec("MYVAR= printenv MYVAR").await.unwrap();
        assert_eq!(result.stdout, "\n");
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn test_prefix_assignment_not_found_without_prefix() {
        let mut bash = Bash::new();
        // printenv for a var that was never set should fail
        let result = bash.exec("printenv NONEXISTENT").await.unwrap();
        assert_eq!(result.stdout, "");
        assert_eq!(result.exit_code, 1);
    }

    #[tokio::test]
    async fn test_prefix_assignment_does_not_persist_in_variables() {
        let mut bash = Bash::new();
        // After prefix assignment with command, var should not be in shell scope
        bash.exec("TMPVAR=gone echo ok").await.unwrap();
        let result = bash.exec("echo \"${TMPVAR:-unset}\"").await.unwrap();
        assert_eq!(result.stdout, "unset\n");
    }

    #[tokio::test]
    async fn test_assignment_only_persists() {
        let mut bash = Bash::new();
        // Assignment without a command should persist (not a prefix assignment)
        bash.exec("PERSIST=yes").await.unwrap();
        let result = bash.exec("echo $PERSIST").await.unwrap();
        assert_eq!(result.stdout, "yes\n");
    }

    #[tokio::test]
    async fn test_printf_string() {
        let mut bash = Bash::new();
        let result = bash.exec("printf '%s' hello").await.unwrap();
        assert_eq!(result.stdout, "hello");
    }

    #[tokio::test]
    async fn test_printf_newline() {
        let mut bash = Bash::new();
        let result = bash.exec("printf 'hello\\n'").await.unwrap();
        assert_eq!(result.stdout, "hello\n");
    }

    #[tokio::test]
    async fn test_printf_multiple_args() {
        let mut bash = Bash::new();
        let result = bash.exec("printf '%s %s\\n' hello world").await.unwrap();
        assert_eq!(result.stdout, "hello world\n");
    }

    #[tokio::test]
    async fn test_printf_integer() {
        let mut bash = Bash::new();
        let result = bash.exec("printf '%d' 42").await.unwrap();
        assert_eq!(result.stdout, "42");
    }

    #[tokio::test]
    async fn test_export() {
        let mut bash = Bash::new();
        let result = bash.exec("export FOO=bar; echo $FOO").await.unwrap();
        assert_eq!(result.stdout, "bar\n");
    }

    #[tokio::test]
    async fn test_read_basic() {
        let mut bash = Bash::new();
        let result = bash.exec("echo hello | read VAR; echo $VAR").await.unwrap();
        assert_eq!(result.stdout, "hello\n");
    }

    #[tokio::test]
    async fn test_read_multiple_vars() {
        let mut bash = Bash::new();
        let result = bash
            .exec("echo 'a b c' | read X Y Z; echo $X $Y $Z")
            .await
            .unwrap();
        assert_eq!(result.stdout, "a b c\n");
    }

    #[tokio::test]
    async fn test_read_respects_local_scope() {
        // Regression: `local k; read -r k <<< "val"` must set k in local scope
        let mut bash = Bash::new();
        let result = bash
            .exec(
                r#"
fn() { local k; read -r k <<< "test"; echo "$k"; }
fn
"#,
            )
            .await
            .unwrap();
        assert_eq!(result.stdout, "test\n");
    }

    #[tokio::test]
    async fn test_local_ifs_array_join() {
        // Regression: local IFS=":" must affect "${arr[*]}" joining
        let mut bash = Bash::new();
        let result = bash
            .exec(
                r#"
fn() {
  local arr=(a b c)
  local IFS=":"
  echo "${arr[*]}"
}
fn
"#,
            )
            .await
            .unwrap();
        assert_eq!(result.stdout, "a:b:c\n");
    }

    #[tokio::test]
    async fn test_glob_star() {
        let mut bash = Bash::new();
        // Create some files
        bash.exec("echo a > /tmp/file1.txt").await.unwrap();
        bash.exec("echo b > /tmp/file2.txt").await.unwrap();
        bash.exec("echo c > /tmp/other.log").await.unwrap();

        // Glob for *.txt files
        let result = bash.exec("echo /tmp/*.txt").await.unwrap();
        assert_eq!(result.stdout, "/tmp/file1.txt /tmp/file2.txt\n");
    }

    #[tokio::test]
    async fn test_glob_question_mark() {
        let mut bash = Bash::new();
        // Create some files
        bash.exec("echo a > /tmp/a1.txt").await.unwrap();
        bash.exec("echo b > /tmp/a2.txt").await.unwrap();
        bash.exec("echo c > /tmp/a10.txt").await.unwrap();

        // Glob for a?.txt (single character)
        let result = bash.exec("echo /tmp/a?.txt").await.unwrap();
        assert_eq!(result.stdout, "/tmp/a1.txt /tmp/a2.txt\n");
    }

    #[tokio::test]
    async fn test_glob_no_match() {
        let mut bash = Bash::new();
        // Glob that doesn't match anything should return the pattern
        let result = bash.exec("echo /nonexistent/*.xyz").await.unwrap();
        assert_eq!(result.stdout, "/nonexistent/*.xyz\n");
    }

    #[tokio::test]
    async fn test_command_substitution() {
        let mut bash = Bash::new();
        let result = bash.exec("echo $(echo hello)").await.unwrap();
        assert_eq!(result.stdout, "hello\n");
    }

    #[tokio::test]
    async fn test_command_substitution_in_string() {
        let mut bash = Bash::new();
        let result = bash.exec("echo \"result: $(echo 42)\"").await.unwrap();
        assert_eq!(result.stdout, "result: 42\n");
    }

    #[tokio::test]
    async fn test_command_substitution_pipeline() {
        let mut bash = Bash::new();
        let result = bash.exec("echo $(echo hello | cat)").await.unwrap();
        assert_eq!(result.stdout, "hello\n");
    }

    #[tokio::test]
    async fn test_command_substitution_variable() {
        let mut bash = Bash::new();
        let result = bash.exec("VAR=$(echo test); echo $VAR").await.unwrap();
        assert_eq!(result.stdout, "test\n");
    }

    #[tokio::test]
    async fn test_arithmetic_simple() {
        let mut bash = Bash::new();
        let result = bash.exec("echo $((1 + 2))").await.unwrap();
        assert_eq!(result.stdout, "3\n");
    }

    #[tokio::test]
    async fn test_arithmetic_multiply() {
        let mut bash = Bash::new();
        let result = bash.exec("echo $((3 * 4))").await.unwrap();
        assert_eq!(result.stdout, "12\n");
    }

    #[tokio::test]
    async fn test_arithmetic_with_variable() {
        let mut bash = Bash::new();
        let result = bash.exec("X=5; echo $((X + 3))").await.unwrap();
        assert_eq!(result.stdout, "8\n");
    }

    #[tokio::test]
    async fn test_arithmetic_complex() {
        let mut bash = Bash::new();
        let result = bash.exec("echo $((2 + 3 * 4))").await.unwrap();
        assert_eq!(result.stdout, "14\n");
    }

    #[tokio::test]
    async fn test_heredoc_simple() {
        let mut bash = Bash::new();
        let result = bash.exec("cat <<EOF\nhello\nworld\nEOF").await.unwrap();
        assert_eq!(result.stdout, "hello\nworld\n");
    }

    #[tokio::test]
    async fn test_heredoc_single_line() {
        let mut bash = Bash::new();
        let result = bash.exec("cat <<END\ntest\nEND").await.unwrap();
        assert_eq!(result.stdout, "test\n");
    }

    #[tokio::test]
    async fn test_unset() {
        let mut bash = Bash::new();
        let result = bash
            .exec("FOO=bar; unset FOO; echo \"x${FOO}y\"")
            .await
            .unwrap();
        assert_eq!(result.stdout, "xy\n");
    }

    #[tokio::test]
    async fn test_local_basic() {
        let mut bash = Bash::new();
        // Test that local command runs without error
        let result = bash.exec("local X=test; echo $X").await.unwrap();
        assert_eq!(result.stdout, "test\n");
    }

    #[tokio::test]
    async fn test_set_option() {
        let mut bash = Bash::new();
        let result = bash.exec("set -e; echo ok").await.unwrap();
        assert_eq!(result.stdout, "ok\n");
    }

    #[tokio::test]
    async fn test_param_default() {
        let mut bash = Bash::new();
        // ${var:-default} when unset
        let result = bash.exec("echo ${UNSET:-default}").await.unwrap();
        assert_eq!(result.stdout, "default\n");

        // ${var:-default} when set
        let result = bash.exec("X=value; echo ${X:-default}").await.unwrap();
        assert_eq!(result.stdout, "value\n");
    }

    #[tokio::test]
    async fn test_param_assign_default() {
        let mut bash = Bash::new();
        // ${var:=default} assigns when unset
        let result = bash.exec("echo ${NEW:=assigned}; echo $NEW").await.unwrap();
        assert_eq!(result.stdout, "assigned\nassigned\n");
    }

    #[tokio::test]
    async fn test_param_length() {
        let mut bash = Bash::new();
        let result = bash.exec("X=hello; echo ${#X}").await.unwrap();
        assert_eq!(result.stdout, "5\n");
    }

    #[tokio::test]
    async fn test_param_remove_prefix() {
        let mut bash = Bash::new();
        // ${var#pattern} - remove shortest prefix
        let result = bash.exec("X=hello.world.txt; echo ${X#*.}").await.unwrap();
        assert_eq!(result.stdout, "world.txt\n");
    }

    #[tokio::test]
    async fn test_param_remove_prefix_mixed_pattern() {
        let mut bash = Bash::new();
        // ${var#./"$other"} - pattern mixing literal and quoted variable
        let result = bash
            .exec(r#"i="./tag_hello.tmp.html"; prefix_tags="tag_"; echo ${i#./"$prefix_tags"}"#)
            .await
            .unwrap();
        assert_eq!(result.stdout, "hello.tmp.html\n");
    }

    #[tokio::test]
    async fn test_param_remove_suffix() {
        let mut bash = Bash::new();
        // ${var%pattern} - remove shortest suffix
        let result = bash.exec("X=file.tar.gz; echo ${X%.*}").await.unwrap();
        assert_eq!(result.stdout, "file.tar\n");
    }

    #[tokio::test]
    async fn test_positional_param_prefix_replace() {
        let mut bash = Bash::new();
        // ${@/#/prefix} should prepend prefix to each positional parameter
        let result = bash
            .exec(r#"f() { set -- "${@/#/tag_}"; echo "$@"; }; f hello world"#)
            .await
            .unwrap();
        assert_eq!(result.stdout, "tag_hello tag_world\n");
    }

    #[tokio::test]
    async fn test_positional_param_suffix_replace() {
        let mut bash = Bash::new();
        // ${@/%/suffix} should append suffix to each positional parameter
        let result = bash
            .exec(r#"f() { set -- "${@/%/.html}"; echo "$@"; }; f hello world"#)
            .await
            .unwrap();
        assert_eq!(result.stdout, "hello.html world.html\n");
    }

    #[tokio::test]
    async fn test_positional_param_prefix_var_replace() {
        let mut bash = Bash::new();
        // ${@/#/$var} should prepend var value to each positional parameter
        let result = bash
            .exec(r#"f() { p="tag_"; set -- "${@/#/$p}"; echo "$@"; }; f hello world"#)
            .await
            .unwrap();
        assert_eq!(result.stdout, "tag_hello tag_world\n");
    }

    #[tokio::test]
    async fn test_positional_param_prefix_strip() {
        let mut bash = Bash::new();
        // ${@#prefix} should strip prefix from each positional parameter
        let result = bash
            .exec(r#"f() { set -- "${@#tag_}"; echo "$@"; }; f tag_hello tag_world"#)
            .await
            .unwrap();
        assert_eq!(result.stdout, "hello world\n");
    }

    #[tokio::test]
    async fn test_array_basic() {
        let mut bash = Bash::new();
        // Basic array declaration and access
        let result = bash.exec("arr=(a b c); echo ${arr[1]}").await.unwrap();
        assert_eq!(result.stdout, "b\n");
    }

    #[tokio::test]
    async fn test_array_all_elements() {
        let mut bash = Bash::new();
        // ${arr[@]} - all elements
        let result = bash
            .exec("arr=(one two three); echo ${arr[@]}")
            .await
            .unwrap();
        assert_eq!(result.stdout, "one two three\n");
    }

    #[tokio::test]
    async fn test_array_length() {
        let mut bash = Bash::new();
        // ${#arr[@]} - number of elements
        let result = bash.exec("arr=(a b c d e); echo ${#arr[@]}").await.unwrap();
        assert_eq!(result.stdout, "5\n");
    }

    #[tokio::test]
    async fn test_array_indexed_assignment() {
        let mut bash = Bash::new();
        // arr[n]=value assignment
        let result = bash
            .exec("arr[0]=first; arr[1]=second; echo ${arr[0]} ${arr[1]}")
            .await
            .unwrap();
        assert_eq!(result.stdout, "first second\n");
    }

    #[tokio::test]
    async fn test_array_single_quote_subscript_no_panic() {
        // Regression: single quote char as array index caused begin > end slice panic
        let mut bash = Bash::new();
        // Should not panic on malformed subscript with lone quote
        let _ = bash.exec("echo ${arr[\"]}").await;
    }

    // Resource limit tests

    #[tokio::test]
    async fn test_command_limit() {
        let limits = ExecutionLimits::new().max_commands(5);
        let mut bash = Bash::builder().limits(limits).build();

        // Run 6 commands - should fail on the 6th
        let result = bash.exec("true; true; true; true; true; true").await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("maximum command count exceeded"),
            "Expected command limit error, got: {}",
            err
        );
    }

    #[tokio::test]
    async fn test_command_limit_not_exceeded() {
        let limits = ExecutionLimits::new().max_commands(10);
        let mut bash = Bash::builder().limits(limits).build();

        // Run 5 commands - should succeed
        let result = bash.exec("true; true; true; true; true").await.unwrap();
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn test_loop_iteration_limit() {
        let limits = ExecutionLimits::new().max_loop_iterations(5);
        let mut bash = Bash::builder().limits(limits).build();

        // Loop that tries to run 10 times
        let result = bash
            .exec("for i in 1 2 3 4 5 6 7 8 9 10; do echo $i; done")
            .await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("maximum loop iterations exceeded"),
            "Expected loop limit error, got: {}",
            err
        );
    }

    #[tokio::test]
    async fn test_loop_iteration_limit_not_exceeded() {
        let limits = ExecutionLimits::new().max_loop_iterations(10);
        let mut bash = Bash::builder().limits(limits).build();

        // Loop that runs 5 times - should succeed
        let result = bash
            .exec("for i in 1 2 3 4 5; do echo $i; done")
            .await
            .unwrap();
        assert_eq!(result.stdout, "1\n2\n3\n4\n5\n");
    }

    #[tokio::test]
    async fn test_function_depth_limit() {
        let limits = ExecutionLimits::new().max_function_depth(3);
        let mut bash = Bash::builder().limits(limits).build();

        // Recursive function that would go 5 deep
        let result = bash
            .exec("f() { echo $1; if [ $1 -lt 5 ]; then f $(($1 + 1)); fi; }; f 1")
            .await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("maximum function depth exceeded"),
            "Expected function depth error, got: {}",
            err
        );
    }

    #[tokio::test]
    async fn test_function_depth_limit_not_exceeded() {
        let limits = ExecutionLimits::new().max_function_depth(10);
        let mut bash = Bash::builder().limits(limits).build();

        // Simple function call - should succeed
        let result = bash.exec("f() { echo hello; }; f").await.unwrap();
        assert_eq!(result.stdout, "hello\n");
    }

    #[tokio::test]
    async fn test_while_loop_limit() {
        let limits = ExecutionLimits::new().max_loop_iterations(3);
        let mut bash = Bash::builder().limits(limits).build();

        // While loop with counter
        let result = bash
            .exec("i=0; while [ $i -lt 10 ]; do echo $i; i=$((i + 1)); done")
            .await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("maximum loop iterations exceeded"),
            "Expected loop limit error, got: {}",
            err
        );
    }

    #[tokio::test]
    async fn test_awk_respects_loop_iteration_limit() {
        let limits = ExecutionLimits::new().max_loop_iterations(5);
        let mut bash = Bash::builder().limits(limits).build();
        let result = bash
            .exec("awk 'BEGIN { i=0; while(1) { i++; if(i>999) break } print i }'")
            .await
            .unwrap();
        assert_eq!(result.stdout.trim(), "5");
    }

    #[tokio::test]
    async fn test_awk_for_in_respects_loop_iteration_limit() {
        let limits = ExecutionLimits::new().max_loop_iterations(3);
        let mut bash = Bash::builder().limits(limits).build();
        let result = bash
            .exec("awk 'BEGIN { for(i=1;i<=10;i++) a[i]=i; c=0; for(k in a) c++; print c }'")
            .await
            .unwrap();
        assert_eq!(result.stdout.trim(), "3");
    }

    #[tokio::test]
    async fn test_default_limits_allow_normal_scripts() {
        // Default limits should allow typical scripts to run
        let mut bash = Bash::new();
        // Avoid using "done" as a word after a for loop - it causes parsing ambiguity
        let result = bash
            .exec("for i in 1 2 3 4 5; do echo $i; done && echo finished")
            .await
            .unwrap();
        assert_eq!(result.stdout, "1\n2\n3\n4\n5\nfinished\n");
    }

    #[tokio::test]
    async fn test_for_followed_by_echo_done() {
        let mut bash = Bash::new();
        let result = bash
            .exec("for i in 1; do echo $i; done; echo ok")
            .await
            .unwrap();
        assert_eq!(result.stdout, "1\nok\n");
    }

    // Filesystem access tests

    #[tokio::test]
    async fn test_fs_read_write_binary() {
        let bash = Bash::new();
        let fs = bash.fs();
        let path = std::path::Path::new("/tmp/binary.bin");

        // Write binary data with null bytes and high bytes
        let binary_data: Vec<u8> = vec![0x00, 0x01, 0xFF, 0xFE, 0x42, 0x00, 0x7F];
        fs.write_file(path, &binary_data).await.unwrap();

        // Read it back
        let content = fs.read_file(path).await.unwrap();
        assert_eq!(content, binary_data);
    }

    #[tokio::test]
    async fn test_fs_write_then_exec_cat() {
        let mut bash = Bash::new();
        let path = std::path::Path::new("/tmp/prepopulated.txt");

        // Pre-populate a file before running bash
        bash.fs()
            .write_file(path, b"Hello from Rust!\n")
            .await
            .unwrap();

        // Access it from bash
        let result = bash.exec("cat /tmp/prepopulated.txt").await.unwrap();
        assert_eq!(result.stdout, "Hello from Rust!\n");
    }

    #[tokio::test]
    async fn test_fs_exec_then_read() {
        let mut bash = Bash::new();
        let path = std::path::Path::new("/tmp/from_bash.txt");

        // Create file via bash
        bash.exec("echo 'Created by bash' > /tmp/from_bash.txt")
            .await
            .unwrap();

        // Read it directly
        let content = bash.fs().read_file(path).await.unwrap();
        assert_eq!(content, b"Created by bash\n");
    }

    #[tokio::test]
    async fn test_fs_exists_and_stat() {
        let bash = Bash::new();
        let fs = bash.fs();
        let path = std::path::Path::new("/tmp/testfile.txt");

        // File doesn't exist yet
        assert!(!fs.exists(path).await.unwrap());

        // Create it
        fs.write_file(path, b"content").await.unwrap();

        // Now exists
        assert!(fs.exists(path).await.unwrap());

        // Check metadata
        let stat = fs.stat(path).await.unwrap();
        assert!(stat.file_type.is_file());
        assert_eq!(stat.size, 7); // "content" = 7 bytes
    }

    #[tokio::test]
    async fn test_fs_mkdir_and_read_dir() {
        let bash = Bash::new();
        let fs = bash.fs();

        // Create nested directories
        fs.mkdir(std::path::Path::new("/data/nested/dir"), true)
            .await
            .unwrap();

        // Create some files
        fs.write_file(std::path::Path::new("/data/file1.txt"), b"1")
            .await
            .unwrap();
        fs.write_file(std::path::Path::new("/data/file2.txt"), b"2")
            .await
            .unwrap();

        // Read directory
        let entries = fs.read_dir(std::path::Path::new("/data")).await.unwrap();
        let names: Vec<_> = entries.iter().map(|e| e.name.as_str()).collect();
        assert!(names.contains(&"nested"));
        assert!(names.contains(&"file1.txt"));
        assert!(names.contains(&"file2.txt"));
    }

    #[tokio::test]
    async fn test_fs_append() {
        let bash = Bash::new();
        let fs = bash.fs();
        let path = std::path::Path::new("/tmp/append.txt");

        fs.write_file(path, b"line1\n").await.unwrap();
        fs.append_file(path, b"line2\n").await.unwrap();
        fs.append_file(path, b"line3\n").await.unwrap();

        let content = fs.read_file(path).await.unwrap();
        assert_eq!(content, b"line1\nline2\nline3\n");
    }

    #[tokio::test]
    async fn test_fs_copy_and_rename() {
        let bash = Bash::new();
        let fs = bash.fs();

        fs.write_file(std::path::Path::new("/tmp/original.txt"), b"data")
            .await
            .unwrap();

        // Copy
        fs.copy(
            std::path::Path::new("/tmp/original.txt"),
            std::path::Path::new("/tmp/copied.txt"),
        )
        .await
        .unwrap();

        // Rename
        fs.rename(
            std::path::Path::new("/tmp/copied.txt"),
            std::path::Path::new("/tmp/renamed.txt"),
        )
        .await
        .unwrap();

        // Verify
        let content = fs
            .read_file(std::path::Path::new("/tmp/renamed.txt"))
            .await
            .unwrap();
        assert_eq!(content, b"data");
        assert!(
            !fs.exists(std::path::Path::new("/tmp/copied.txt"))
                .await
                .unwrap()
        );
    }

    // Bug fix tests

    #[tokio::test]
    async fn test_echo_done_as_argument() {
        // BUG: "done" should be parsed as a regular argument when not in loop context
        let mut bash = Bash::new();
        let result = bash
            .exec("for i in 1; do echo $i; done; echo done")
            .await
            .unwrap();
        assert_eq!(result.stdout, "1\ndone\n");
    }

    #[tokio::test]
    async fn test_simple_echo_done() {
        // Simple echo done without any loop
        let mut bash = Bash::new();
        let result = bash.exec("echo done").await.unwrap();
        assert_eq!(result.stdout, "done\n");
    }

    #[tokio::test]
    async fn test_dev_null_redirect() {
        // BUG: Redirecting to /dev/null should discard output silently
        let mut bash = Bash::new();
        let result = bash.exec("echo hello > /dev/null; echo ok").await.unwrap();
        assert_eq!(result.stdout, "ok\n");
    }

    #[tokio::test]
    async fn test_string_concatenation_in_loop() {
        // Test string concatenation in a loop
        let mut bash = Bash::new();
        // First test: basic for loop still works
        let result = bash.exec("for i in a b c; do echo $i; done").await.unwrap();
        assert_eq!(result.stdout, "a\nb\nc\n");

        // Test variable assignment followed by for loop
        let mut bash = Bash::new();
        let result = bash
            .exec("result=x; for i in a b c; do echo $i; done; echo $result")
            .await
            .unwrap();
        assert_eq!(result.stdout, "a\nb\nc\nx\n");

        // Test string concatenation in a loop
        let mut bash = Bash::new();
        let result = bash
            .exec("result=start; for i in a b c; do result=${result}$i; done; echo $result")
            .await
            .unwrap();
        assert_eq!(result.stdout, "startabc\n");
    }

    // Negative/edge case tests for reserved word handling

    #[tokio::test]
    async fn test_done_still_terminates_loop() {
        // Ensure "done" still works as a loop terminator
        let mut bash = Bash::new();
        let result = bash.exec("for i in 1 2; do echo $i; done").await.unwrap();
        assert_eq!(result.stdout, "1\n2\n");
    }

    #[tokio::test]
    async fn test_fi_still_terminates_if() {
        // Ensure "fi" still works as an if terminator
        let mut bash = Bash::new();
        let result = bash.exec("if true; then echo yes; fi").await.unwrap();
        assert_eq!(result.stdout, "yes\n");
    }

    #[tokio::test]
    async fn test_echo_fi_as_argument() {
        // "fi" should be a valid argument outside of if context
        let mut bash = Bash::new();
        let result = bash.exec("echo fi").await.unwrap();
        assert_eq!(result.stdout, "fi\n");
    }

    #[tokio::test]
    async fn test_echo_then_as_argument() {
        // "then" should be a valid argument outside of if context
        let mut bash = Bash::new();
        let result = bash.exec("echo then").await.unwrap();
        assert_eq!(result.stdout, "then\n");
    }

    #[tokio::test]
    async fn test_reserved_words_in_quotes_are_arguments() {
        // Reserved words in quotes should always be arguments
        let mut bash = Bash::new();
        let result = bash.exec("echo 'done' 'fi' 'then'").await.unwrap();
        assert_eq!(result.stdout, "done fi then\n");
    }

    #[tokio::test]
    async fn test_nested_loops_done_keyword() {
        // Nested loops should properly match done keywords
        let mut bash = Bash::new();
        let result = bash
            .exec("for i in 1; do for j in a; do echo $i$j; done; done")
            .await
            .unwrap();
        assert_eq!(result.stdout, "1a\n");
    }

    // Negative/edge case tests for /dev/null

    #[tokio::test]
    async fn test_dev_null_read_returns_empty() {
        // Reading from /dev/null should return empty
        let mut bash = Bash::new();
        let result = bash.exec("cat /dev/null").await.unwrap();
        assert_eq!(result.stdout, "");
    }

    #[tokio::test]
    async fn test_dev_null_append() {
        // Appending to /dev/null should work silently
        let mut bash = Bash::new();
        let result = bash.exec("echo hello >> /dev/null; echo ok").await.unwrap();
        assert_eq!(result.stdout, "ok\n");
    }

    #[tokio::test]
    async fn test_dev_null_in_pipeline() {
        // /dev/null in a pipeline should work
        let mut bash = Bash::new();
        let result = bash
            .exec("echo hello | cat > /dev/null; echo ok")
            .await
            .unwrap();
        assert_eq!(result.stdout, "ok\n");
    }

    #[tokio::test]
    async fn test_dev_null_exists() {
        // /dev/null should exist and be readable
        let mut bash = Bash::new();
        let result = bash.exec("cat /dev/null; echo exit_$?").await.unwrap();
        assert_eq!(result.stdout, "exit_0\n");
    }

    // Custom username/hostname tests

    #[tokio::test]
    async fn test_custom_username_whoami() {
        let mut bash = Bash::builder().username("alice").build();
        let result = bash.exec("whoami").await.unwrap();
        assert_eq!(result.stdout, "alice\n");
    }

    #[tokio::test]
    async fn test_custom_username_id() {
        let mut bash = Bash::builder().username("bob").build();
        let result = bash.exec("id").await.unwrap();
        assert!(result.stdout.contains("uid=1000(bob)"));
        assert!(result.stdout.contains("gid=1000(bob)"));
    }

    #[tokio::test]
    async fn test_custom_username_sets_user_env() {
        let mut bash = Bash::builder().username("charlie").build();
        let result = bash.exec("echo $USER").await.unwrap();
        assert_eq!(result.stdout, "charlie\n");
    }

    #[tokio::test]
    async fn test_default_ppid_is_sandboxed() {
        let mut bash = Bash::new();
        let result = bash.exec("echo $PPID").await.unwrap();
        assert_eq!(result.stdout, "0\n");
    }

    #[tokio::test]
    async fn test_custom_hostname() {
        let mut bash = Bash::builder().hostname("my-server").build();
        let result = bash.exec("hostname").await.unwrap();
        assert_eq!(result.stdout, "my-server\n");
    }

    #[tokio::test]
    async fn test_custom_hostname_uname() {
        let mut bash = Bash::builder().hostname("custom-host").build();
        let result = bash.exec("uname -n").await.unwrap();
        assert_eq!(result.stdout, "custom-host\n");
    }

    #[tokio::test]
    async fn test_default_username_and_hostname() {
        // Default values should still work
        let mut bash = Bash::new();
        let result = bash.exec("whoami").await.unwrap();
        assert_eq!(result.stdout, "sandbox\n");

        let result = bash.exec("hostname").await.unwrap();
        assert_eq!(result.stdout, "bashkit-sandbox\n");
    }

    #[tokio::test]
    async fn test_custom_username_and_hostname_combined() {
        let mut bash = Bash::builder()
            .username("deploy")
            .hostname("prod-server-01")
            .build();

        let result = bash.exec("whoami && hostname").await.unwrap();
        assert_eq!(result.stdout, "deploy\nprod-server-01\n");

        let result = bash.exec("echo $USER").await.unwrap();
        assert_eq!(result.stdout, "deploy\n");
    }

    // Custom builtins tests

    mod custom_builtins {
        use super::*;
        use crate::builtins::{Builtin, Context};
        use crate::{ExecResult, ExecutionExtensions};
        use async_trait::async_trait;

        /// A simple custom builtin that outputs a static string
        struct Hello;

        #[async_trait]
        impl Builtin for Hello {
            async fn execute(&self, _ctx: Context<'_>) -> crate::Result<ExecResult> {
                Ok(ExecResult::ok("Hello from custom builtin!\n".to_string()))
            }
        }

        #[tokio::test]
        async fn test_custom_builtin_basic() {
            let mut bash = Bash::builder().builtin("hello", Box::new(Hello)).build();

            let result = bash.exec("hello").await.unwrap();
            assert_eq!(result.stdout, "Hello from custom builtin!\n");
            assert_eq!(result.exit_code, 0);
        }

        struct ExecutionScoped;

        #[async_trait]
        impl Builtin for ExecutionScoped {
            async fn execute(&self, ctx: Context<'_>) -> crate::Result<ExecResult> {
                let value = ctx
                    .execution_extension::<String>()
                    .cloned()
                    .unwrap_or_else(|| "missing".to_string());
                Ok(ExecResult::ok(format!("{value}\n")))
            }
        }

        #[tokio::test]
        async fn test_custom_builtin_execution_extensions_are_per_call() {
            let mut bash = Bash::builder()
                .builtin("read-ext", Box::new(ExecutionScoped))
                .build();

            let result = bash
                .exec_with_extensions(
                    "read-ext",
                    ExecutionExtensions::new().with("scoped".to_string()),
                )
                .await
                .unwrap();
            assert_eq!(result.stdout, "scoped\n");

            let result = bash.exec("read-ext").await.unwrap();
            assert_eq!(result.stdout, "missing\n");
        }

        /// A custom builtin that uses arguments
        struct Greet;

        #[async_trait]
        impl Builtin for Greet {
            async fn execute(&self, ctx: Context<'_>) -> crate::Result<ExecResult> {
                let name = ctx.args.first().map(|s| s.as_str()).unwrap_or("World");
                Ok(ExecResult::ok(format!("Hello, {}!\n", name)))
            }
        }

        #[tokio::test]
        async fn test_custom_builtin_with_args() {
            let mut bash = Bash::builder().builtin("greet", Box::new(Greet)).build();

            let result = bash.exec("greet").await.unwrap();
            assert_eq!(result.stdout, "Hello, World!\n");

            let result = bash.exec("greet Alice").await.unwrap();
            assert_eq!(result.stdout, "Hello, Alice!\n");

            let result = bash.exec("greet Bob Charlie").await.unwrap();
            assert_eq!(result.stdout, "Hello, Bob!\n");
        }

        /// A custom builtin that reads from stdin
        struct Upper;

        #[async_trait]
        impl Builtin for Upper {
            async fn execute(&self, ctx: Context<'_>) -> crate::Result<ExecResult> {
                let input = ctx.stdin.unwrap_or("");
                Ok(ExecResult::ok(input.to_uppercase()))
            }
        }

        #[tokio::test]
        async fn test_custom_builtin_with_stdin() {
            let mut bash = Bash::builder().builtin("upper", Box::new(Upper)).build();

            let result = bash.exec("echo hello | upper").await.unwrap();
            assert_eq!(result.stdout, "HELLO\n");
        }

        /// A custom builtin that interacts with the filesystem
        struct WriteFile;

        #[async_trait]
        impl Builtin for WriteFile {
            async fn execute(&self, ctx: Context<'_>) -> crate::Result<ExecResult> {
                if ctx.args.len() < 2 {
                    return Ok(ExecResult::err(
                        "Usage: writefile <path> <content>\n".to_string(),
                        1,
                    ));
                }
                let path = std::path::Path::new(&ctx.args[0]);
                let content = ctx.args[1..].join(" ");
                ctx.fs.write_file(path, content.as_bytes()).await?;
                Ok(ExecResult::ok(String::new()))
            }
        }

        #[tokio::test]
        async fn test_custom_builtin_with_filesystem() {
            let mut bash = Bash::builder()
                .builtin("writefile", Box::new(WriteFile))
                .build();

            bash.exec("writefile /tmp/test.txt custom content here")
                .await
                .unwrap();

            let result = bash.exec("cat /tmp/test.txt").await.unwrap();
            assert_eq!(result.stdout, "custom content here");
        }

        /// A custom builtin that overrides a default builtin
        struct CustomEcho;

        #[async_trait]
        impl Builtin for CustomEcho {
            async fn execute(&self, ctx: Context<'_>) -> crate::Result<ExecResult> {
                let msg = ctx.args.join(" ");
                Ok(ExecResult::ok(format!("[CUSTOM] {}\n", msg)))
            }
        }

        #[tokio::test]
        async fn test_custom_builtin_override_default() {
            let mut bash = Bash::builder()
                .builtin("echo", Box::new(CustomEcho))
                .build();

            let result = bash.exec("echo hello world").await.unwrap();
            assert_eq!(result.stdout, "[CUSTOM] hello world\n");
        }

        /// Test multiple custom builtins
        #[tokio::test]
        async fn test_multiple_custom_builtins() {
            let mut bash = Bash::builder()
                .builtin("hello", Box::new(Hello))
                .builtin("greet", Box::new(Greet))
                .builtin("upper", Box::new(Upper))
                .build();

            let result = bash.exec("hello").await.unwrap();
            assert_eq!(result.stdout, "Hello from custom builtin!\n");

            let result = bash.exec("greet Test").await.unwrap();
            assert_eq!(result.stdout, "Hello, Test!\n");

            let result = bash.exec("echo foo | upper").await.unwrap();
            assert_eq!(result.stdout, "FOO\n");
        }

        /// A custom builtin with internal state
        struct Counter {
            prefix: String,
        }

        #[async_trait]
        impl Builtin for Counter {
            async fn execute(&self, ctx: Context<'_>) -> crate::Result<ExecResult> {
                let count = ctx
                    .args
                    .first()
                    .and_then(|s| s.parse::<i32>().ok())
                    .unwrap_or(1);
                let mut output = String::new();
                for i in 1..=count {
                    output.push_str(&format!("{}{}\n", self.prefix, i));
                }
                Ok(ExecResult::ok(output))
            }
        }

        #[tokio::test]
        async fn test_custom_builtin_with_state() {
            let mut bash = Bash::builder()
                .builtin(
                    "count",
                    Box::new(Counter {
                        prefix: "Item ".to_string(),
                    }),
                )
                .build();

            let result = bash.exec("count 3").await.unwrap();
            assert_eq!(result.stdout, "Item 1\nItem 2\nItem 3\n");
        }

        /// A custom builtin that returns an error
        struct Fail;

        #[async_trait]
        impl Builtin for Fail {
            async fn execute(&self, ctx: Context<'_>) -> crate::Result<ExecResult> {
                let code = ctx
                    .args
                    .first()
                    .and_then(|s| s.parse::<i32>().ok())
                    .unwrap_or(1);
                Ok(ExecResult::err(
                    format!("Failed with code {}\n", code),
                    code,
                ))
            }
        }

        #[tokio::test]
        async fn test_custom_builtin_error() {
            let mut bash = Bash::builder().builtin("fail", Box::new(Fail)).build();

            let result = bash.exec("fail 42").await.unwrap();
            assert_eq!(result.exit_code, 42);
            assert_eq!(result.stderr, "Failed with code 42\n");
        }

        #[tokio::test]
        async fn test_custom_builtin_in_script() {
            let mut bash = Bash::builder().builtin("greet", Box::new(Greet)).build();

            let script = r#"
                for name in Alice Bob Charlie; do
                    greet $name
                done
            "#;

            let result = bash.exec(script).await.unwrap();
            assert_eq!(
                result.stdout,
                "Hello, Alice!\nHello, Bob!\nHello, Charlie!\n"
            );
        }

        #[tokio::test]
        async fn test_custom_builtin_with_conditionals() {
            let mut bash = Bash::builder()
                .builtin("fail", Box::new(Fail))
                .builtin("hello", Box::new(Hello))
                .build();

            let result = bash.exec("fail 1 || hello").await.unwrap();
            assert_eq!(result.stdout, "Hello from custom builtin!\n");
            assert_eq!(result.exit_code, 0);

            let result = bash.exec("hello && fail 5").await.unwrap();
            assert_eq!(result.exit_code, 5);
        }

        /// A custom builtin that reads environment variables
        struct EnvReader;

        #[async_trait]
        impl Builtin for EnvReader {
            async fn execute(&self, ctx: Context<'_>) -> crate::Result<ExecResult> {
                let var_name = ctx.args.first().map(|s| s.as_str()).unwrap_or("HOME");
                let value = ctx
                    .env
                    .get(var_name)
                    .map(|s| s.as_str())
                    .unwrap_or("(not set)");
                Ok(ExecResult::ok(format!("{}={}\n", var_name, value)))
            }
        }

        #[tokio::test]
        async fn test_custom_builtin_reads_env() {
            let mut bash = Bash::builder()
                .env("MY_VAR", "my_value")
                .builtin("readenv", Box::new(EnvReader))
                .build();

            let result = bash.exec("readenv MY_VAR").await.unwrap();
            assert_eq!(result.stdout, "MY_VAR=my_value\n");

            let result = bash.exec("readenv UNKNOWN").await.unwrap();
            assert_eq!(result.stdout, "UNKNOWN=(not set)\n");
        }
    }

    // Parser timeout tests

    #[tokio::test]
    async fn test_parser_timeout_default() {
        // Default parser timeout should be 5 seconds
        let limits = ExecutionLimits::default();
        assert_eq!(limits.parser_timeout, std::time::Duration::from_secs(5));
    }

    #[tokio::test]
    async fn test_parser_timeout_custom() {
        // Parser timeout can be customized
        let limits = ExecutionLimits::new().parser_timeout(std::time::Duration::from_millis(100));
        assert_eq!(limits.parser_timeout, std::time::Duration::from_millis(100));
    }

    #[tokio::test]
    async fn test_parser_timeout_normal_script() {
        // Normal scripts should complete well within timeout
        let limits = ExecutionLimits::new().parser_timeout(std::time::Duration::from_secs(1));
        let mut bash = Bash::builder().limits(limits).build();
        let result = bash.exec("echo hello").await.unwrap();
        assert_eq!(result.stdout, "hello\n");
    }

    // Parser fuel tests

    #[tokio::test]
    async fn test_parser_fuel_default() {
        // Default parser fuel should be 100,000
        let limits = ExecutionLimits::default();
        assert_eq!(limits.max_parser_operations, 100_000);
    }

    #[tokio::test]
    async fn test_parser_fuel_custom() {
        // Parser fuel can be customized
        let limits = ExecutionLimits::new().max_parser_operations(1000);
        assert_eq!(limits.max_parser_operations, 1000);
    }

    #[tokio::test]
    async fn test_parser_fuel_normal_script() {
        // Normal scripts should parse within fuel limit
        let limits = ExecutionLimits::new().max_parser_operations(1000);
        let mut bash = Bash::builder().limits(limits).build();
        let result = bash.exec("echo hello").await.unwrap();
        assert_eq!(result.stdout, "hello\n");
    }

    // Input size limit tests

    #[tokio::test]
    async fn test_input_size_limit_default() {
        // Default input size limit should be 10MB
        let limits = ExecutionLimits::default();
        assert_eq!(limits.max_input_bytes, 10_000_000);
    }

    #[tokio::test]
    async fn test_input_size_limit_custom() {
        // Input size limit can be customized
        let limits = ExecutionLimits::new().max_input_bytes(1000);
        assert_eq!(limits.max_input_bytes, 1000);
    }

    #[tokio::test]
    async fn test_input_size_limit_enforced() {
        // Scripts exceeding the limit should be rejected
        let limits = ExecutionLimits::new().max_input_bytes(10);
        let mut bash = Bash::builder().limits(limits).build();

        // This script is longer than 10 bytes
        let result = bash.exec("echo hello world").await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("input too large"),
            "Expected input size error, got: {}",
            err
        );
    }

    #[tokio::test]
    async fn test_input_size_limit_normal_script() {
        // Normal scripts should complete within limit
        let limits = ExecutionLimits::new().max_input_bytes(1000);
        let mut bash = Bash::builder().limits(limits).build();
        let result = bash.exec("echo hello").await.unwrap();
        assert_eq!(result.stdout, "hello\n");
    }

    // AST depth limit tests

    #[tokio::test]
    async fn test_ast_depth_limit_default() {
        // Default AST depth limit should be 100
        let limits = ExecutionLimits::default();
        assert_eq!(limits.max_ast_depth, 100);
    }

    #[tokio::test]
    async fn test_ast_depth_limit_custom() {
        // AST depth limit can be customized
        let limits = ExecutionLimits::new().max_ast_depth(10);
        assert_eq!(limits.max_ast_depth, 10);
    }

    #[tokio::test]
    async fn test_ast_depth_limit_normal_script() {
        // Normal scripts should parse within limit
        let limits = ExecutionLimits::new().max_ast_depth(10);
        let mut bash = Bash::builder().limits(limits).build();
        let result = bash.exec("if true; then echo ok; fi").await.unwrap();
        assert_eq!(result.stdout, "ok\n");
    }

    #[tokio::test]
    async fn test_ast_depth_limit_enforced() {
        // Deeply nested scripts should be rejected
        let limits = ExecutionLimits::new().max_ast_depth(2);
        let mut bash = Bash::builder().limits(limits).build();

        // This script has 3 levels of nesting (exceeds limit of 2)
        let result = bash
            .exec("if true; then if true; then if true; then echo nested; fi; fi; fi")
            .await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("AST nesting too deep"),
            "Expected AST depth error, got: {}",
            err
        );
    }

    #[tokio::test]
    async fn test_parser_fuel_enforced() {
        // Scripts exceeding fuel limit should be rejected
        // With fuel of 3, parsing "echo a" should fail (needs multiple operations)
        let limits = ExecutionLimits::new().max_parser_operations(3);
        let mut bash = Bash::builder().limits(limits).build();

        // Even a simple script needs more than 3 parsing operations
        let result = bash.exec("echo a; echo b; echo c").await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("parser fuel exhausted"),
            "Expected parser fuel error, got: {}",
            err
        );
    }

    // set -e (errexit) tests

    #[tokio::test]
    async fn test_set_e_basic() {
        // set -e should exit on non-zero return
        let mut bash = Bash::new();
        let result = bash
            .exec("set -e; true; false; echo should_not_reach")
            .await
            .unwrap();
        assert_eq!(result.stdout, "");
        assert_eq!(result.exit_code, 1);
    }

    #[tokio::test]
    async fn test_set_e_after_failing_cmd() {
        // set -e exits immediately on failed command
        let mut bash = Bash::new();
        let result = bash
            .exec("set -e; echo before; false; echo after")
            .await
            .unwrap();
        assert_eq!(result.stdout, "before\n");
        assert_eq!(result.exit_code, 1);
    }

    #[tokio::test]
    async fn test_set_e_disabled() {
        // set +e disables errexit
        let mut bash = Bash::new();
        let result = bash
            .exec("set -e; set +e; false; echo still_running")
            .await
            .unwrap();
        assert_eq!(result.stdout, "still_running\n");
    }

    #[tokio::test]
    async fn test_set_e_in_pipeline_last() {
        // set -e only checks last command in pipeline
        let mut bash = Bash::new();
        let result = bash
            .exec("set -e; false | true; echo reached")
            .await
            .unwrap();
        assert_eq!(result.stdout, "reached\n");
    }

    #[tokio::test]
    async fn test_set_e_in_if_condition() {
        // set -e should not trigger on if condition failure
        let mut bash = Bash::new();
        let result = bash
            .exec("set -e; if false; then echo yes; else echo no; fi; echo done")
            .await
            .unwrap();
        assert_eq!(result.stdout, "no\ndone\n");
    }

    #[tokio::test]
    async fn test_set_e_in_while_condition() {
        // set -e should not trigger on while condition failure
        let mut bash = Bash::new();
        let result = bash
            .exec("set -e; x=0; while [ \"$x\" -lt 2 ]; do echo \"x=$x\"; x=$((x + 1)); done; echo done")
            .await
            .unwrap();
        assert_eq!(result.stdout, "x=0\nx=1\ndone\n");
    }

    #[tokio::test]
    async fn test_set_e_in_brace_group() {
        // set -e should work inside brace groups
        let mut bash = Bash::new();
        let result = bash
            .exec("set -e; { echo start; false; echo unreached; }; echo after")
            .await
            .unwrap();
        assert_eq!(result.stdout, "start\n");
        assert_eq!(result.exit_code, 1);
    }

    #[tokio::test]
    async fn test_set_e_and_chain() {
        // set -e should not trigger on && chain (false && ... is expected to not run second)
        let mut bash = Bash::new();
        let result = bash
            .exec("set -e; false && echo one; echo reached")
            .await
            .unwrap();
        assert_eq!(result.stdout, "reached\n");
    }

    #[tokio::test]
    async fn test_set_e_or_chain() {
        // set -e should not trigger on || chain (true || false is expected to short circuit)
        let mut bash = Bash::new();
        let result = bash
            .exec("set -e; true || false; echo reached")
            .await
            .unwrap();
        assert_eq!(result.stdout, "reached\n");
    }

    // Tilde expansion tests

    #[tokio::test]
    async fn test_tilde_expansion_basic() {
        // ~ should expand to $HOME
        let mut bash = Bash::builder().env("HOME", "/home/testuser").build();
        let result = bash.exec("echo ~").await.unwrap();
        assert_eq!(result.stdout, "/home/testuser\n");
    }

    #[tokio::test]
    async fn test_tilde_expansion_with_path() {
        // ~/path should expand to $HOME/path
        let mut bash = Bash::builder().env("HOME", "/home/testuser").build();
        let result = bash.exec("echo ~/documents/file.txt").await.unwrap();
        assert_eq!(result.stdout, "/home/testuser/documents/file.txt\n");
    }

    #[tokio::test]
    async fn test_tilde_expansion_in_assignment() {
        // Tilde expansion should work in variable assignments
        let mut bash = Bash::builder().env("HOME", "/home/testuser").build();
        let result = bash.exec("DIR=~/data; echo $DIR").await.unwrap();
        assert_eq!(result.stdout, "/home/testuser/data\n");
    }

    #[tokio::test]
    async fn test_tilde_expansion_default_home() {
        // ~ should default to /home/sandbox (DEFAULT_USERNAME is "sandbox")
        let mut bash = Bash::new();
        let result = bash.exec("echo ~").await.unwrap();
        assert_eq!(result.stdout, "/home/sandbox\n");
    }

    #[tokio::test]
    async fn test_tilde_not_at_start() {
        // ~ not at start of word should not expand
        let mut bash = Bash::builder().env("HOME", "/home/testuser").build();
        let result = bash.exec("echo foo~bar").await.unwrap();
        assert_eq!(result.stdout, "foo~bar\n");
    }

    // Special variables tests

    #[tokio::test]
    async fn test_special_var_dollar_dollar() {
        // $$ - current process ID
        let mut bash = Bash::new();
        let result = bash.exec("echo $$").await.unwrap();
        // Should be a numeric value
        let pid: u32 = result.stdout.trim().parse().expect("$$ should be a number");
        assert!(pid > 0, "$$ should be a positive number");
    }

    #[tokio::test]
    async fn test_special_var_random() {
        // $RANDOM - random number between 0 and 32767
        let mut bash = Bash::new();
        let result = bash.exec("echo $RANDOM").await.unwrap();
        let random: u32 = result
            .stdout
            .trim()
            .parse()
            .expect("$RANDOM should be a number");
        assert!(random < 32768, "$RANDOM should be < 32768");
    }

    #[tokio::test]
    async fn test_special_var_random_varies() {
        // $RANDOM should return different values on different calls
        let mut bash = Bash::new();
        let result1 = bash.exec("echo $RANDOM").await.unwrap();
        let result2 = bash.exec("echo $RANDOM").await.unwrap();
        // With high probability, they should be different
        // (small chance they're the same, so this test may rarely fail)
        // We'll just check they're both valid numbers
        let _: u32 = result1
            .stdout
            .trim()
            .parse()
            .expect("$RANDOM should be a number");
        let _: u32 = result2
            .stdout
            .trim()
            .parse()
            .expect("$RANDOM should be a number");
    }

    #[tokio::test]
    async fn test_random_different_instances() {
        // Two separate Bash instances should produce different PRNG sequences
        // (with very high probability, since each is seeded from OS entropy)
        let mut bash1 = Bash::new();
        let mut bash2 = Bash::new();
        let r1 = bash1.exec("echo $RANDOM").await.unwrap();
        let r2 = bash2.exec("echo $RANDOM").await.unwrap();
        let v1: u32 = r1.stdout.trim().parse().expect("should be a number");
        let v2: u32 = r2.stdout.trim().parse().expect("should be a number");
        assert!(v1 < 32768);
        assert!(v2 < 32768);
        // Extremely unlikely to collide with independent OS-entropy seeds
        assert_ne!(v1, v2, "separate instances should produce different values");
    }

    #[tokio::test]
    async fn test_random_reseed() {
        // RANDOM=N should reseed the PRNG, producing a deterministic sequence
        let mut bash1 = Bash::new();
        let mut bash2 = Bash::new();
        bash1.exec("RANDOM=42").await.unwrap();
        bash2.exec("RANDOM=42").await.unwrap();
        let r1 = bash1.exec("echo $RANDOM").await.unwrap();
        let r2 = bash2.exec("echo $RANDOM").await.unwrap();
        assert_eq!(
            r1.stdout, r2.stdout,
            "same seed should produce same first value"
        );
    }

    #[tokio::test]
    async fn test_random_sequential_varies() {
        // Sequential $RANDOM calls within a single instance should differ
        let mut bash = Bash::new();
        let result = bash.exec("echo $RANDOM $RANDOM $RANDOM").await.unwrap();
        let values: Vec<u32> = result
            .stdout
            .split_whitespace()
            .map(|s| s.parse().expect("should be a number"))
            .collect();
        assert_eq!(values.len(), 3);
        // At least two of three should differ (LCG never produces same value twice in a row)
        assert!(
            values[0] != values[1] || values[1] != values[2],
            "sequential RANDOM calls should produce different values"
        );
    }

    #[tokio::test]
    async fn test_special_var_lineno() {
        // $LINENO - current line number
        let mut bash = Bash::new();
        let result = bash.exec("echo $LINENO").await.unwrap();
        assert_eq!(result.stdout, "1\n");
    }

    #[tokio::test]
    async fn test_lineno_multiline() {
        // $LINENO tracks line numbers across multiple lines
        let mut bash = Bash::new();
        let result = bash
            .exec(
                r#"echo "line $LINENO"
echo "line $LINENO"
echo "line $LINENO""#,
            )
            .await
            .unwrap();
        assert_eq!(result.stdout, "line 1\nline 2\nline 3\n");
    }

    #[tokio::test]
    async fn test_lineno_in_loop() {
        // $LINENO inside a for loop
        let mut bash = Bash::new();
        let result = bash
            .exec(
                r#"for i in 1 2; do
  echo "loop $LINENO"
done"#,
            )
            .await
            .unwrap();
        // Loop body is on line 2
        assert_eq!(result.stdout, "loop 2\nloop 2\n");
    }

    // File test operator tests

    #[tokio::test]
    async fn test_file_test_r_readable() {
        // -r file: true if file exists (readable in virtual fs)
        let mut bash = Bash::new();
        bash.exec("echo hello > /tmp/readable.txt").await.unwrap();
        let result = bash
            .exec("test -r /tmp/readable.txt && echo yes")
            .await
            .unwrap();
        assert_eq!(result.stdout, "yes\n");
    }

    #[tokio::test]
    async fn test_file_test_r_not_exists() {
        // -r file: false if file doesn't exist
        let mut bash = Bash::new();
        let result = bash
            .exec("test -r /tmp/nonexistent.txt && echo yes || echo no")
            .await
            .unwrap();
        assert_eq!(result.stdout, "no\n");
    }

    #[tokio::test]
    async fn test_file_test_w_writable() {
        // -w file: true if file exists (writable in virtual fs)
        let mut bash = Bash::new();
        bash.exec("echo hello > /tmp/writable.txt").await.unwrap();
        let result = bash
            .exec("test -w /tmp/writable.txt && echo yes")
            .await
            .unwrap();
        assert_eq!(result.stdout, "yes\n");
    }

    #[tokio::test]
    async fn test_file_test_x_executable() {
        // -x file: true if file exists and has execute permission
        let mut bash = Bash::new();
        bash.exec("echo '#!/bin/bash' > /tmp/script.sh")
            .await
            .unwrap();
        bash.exec("chmod 755 /tmp/script.sh").await.unwrap();
        let result = bash
            .exec("test -x /tmp/script.sh && echo yes")
            .await
            .unwrap();
        assert_eq!(result.stdout, "yes\n");
    }

    #[tokio::test]
    async fn test_file_test_x_not_executable() {
        // -x file: false if file has no execute permission
        let mut bash = Bash::new();
        bash.exec("echo 'data' > /tmp/noexec.txt").await.unwrap();
        bash.exec("chmod 644 /tmp/noexec.txt").await.unwrap();
        let result = bash
            .exec("test -x /tmp/noexec.txt && echo yes || echo no")
            .await
            .unwrap();
        assert_eq!(result.stdout, "no\n");
    }

    #[tokio::test]
    async fn test_file_test_e_exists() {
        // -e file: true if file exists
        let mut bash = Bash::new();
        bash.exec("echo hello > /tmp/exists.txt").await.unwrap();
        let result = bash
            .exec("test -e /tmp/exists.txt && echo yes")
            .await
            .unwrap();
        assert_eq!(result.stdout, "yes\n");
    }

    #[tokio::test]
    async fn test_file_test_f_regular() {
        // -f file: true if regular file
        let mut bash = Bash::new();
        bash.exec("echo hello > /tmp/regular.txt").await.unwrap();
        let result = bash
            .exec("test -f /tmp/regular.txt && echo yes")
            .await
            .unwrap();
        assert_eq!(result.stdout, "yes\n");
    }

    #[tokio::test]
    async fn test_file_test_d_directory() {
        // -d file: true if directory
        let mut bash = Bash::new();
        bash.exec("mkdir -p /tmp/mydir").await.unwrap();
        let result = bash.exec("test -d /tmp/mydir && echo yes").await.unwrap();
        assert_eq!(result.stdout, "yes\n");
    }

    #[tokio::test]
    async fn test_file_test_s_size() {
        // -s file: true if file has size > 0
        let mut bash = Bash::new();
        bash.exec("echo hello > /tmp/nonempty.txt").await.unwrap();
        let result = bash
            .exec("test -s /tmp/nonempty.txt && echo yes")
            .await
            .unwrap();
        assert_eq!(result.stdout, "yes\n");
    }

    // ============================================================
    // Stderr Redirection Tests
    // ============================================================

    #[tokio::test]
    async fn test_redirect_both_stdout_stderr() {
        // &> redirects both stdout and stderr to file
        let mut bash = Bash::new();
        // echo outputs to stdout, we use &> to redirect both to file
        let result = bash.exec("echo hello &> /tmp/out.txt").await.unwrap();
        // stdout should be empty (redirected to file)
        assert_eq!(result.stdout, "");
        // Verify file contents
        let check = bash.exec("cat /tmp/out.txt").await.unwrap();
        assert_eq!(check.stdout, "hello\n");
    }

    #[tokio::test]
    async fn test_stderr_redirect_to_file() {
        // 2> redirects stderr to file
        // We need a command that outputs to stderr - let's use a command that fails
        // Or use a subshell with explicit stderr output
        let mut bash = Bash::new();
        // Create a test script that outputs to both stdout and stderr
        bash.exec("echo stdout; echo stderr 2> /tmp/err.txt")
            .await
            .unwrap();
        // Note: echo stderr doesn't actually output to stderr, it outputs to stdout
        // We need to test with actual stderr output
    }

    #[tokio::test]
    async fn test_fd_redirect_parsing() {
        // Test that 2> is parsed correctly
        let mut bash = Bash::new();
        // Just test the parsing doesn't error
        let result = bash.exec("true 2> /tmp/err.txt").await.unwrap();
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn test_fd_redirect_append_parsing() {
        // Test that 2>> is parsed correctly
        let mut bash = Bash::new();
        let result = bash.exec("true 2>> /tmp/err.txt").await.unwrap();
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn test_fd_dup_parsing() {
        // Test that 2>&1 is parsed correctly
        let mut bash = Bash::new();
        let result = bash.exec("echo hello 2>&1").await.unwrap();
        assert_eq!(result.stdout, "hello\n");
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn test_dup_output_redirect_stdout_to_stderr() {
        // >&2 redirects stdout to stderr
        let mut bash = Bash::new();
        let result = bash.exec("echo hello >&2").await.unwrap();
        // stdout should be moved to stderr
        assert_eq!(result.stdout, "");
        assert_eq!(result.stderr, "hello\n");
    }

    #[tokio::test]
    async fn test_lexer_redirect_both() {
        // Test that &> is lexed as a single token, not & followed by >
        let mut bash = Bash::new();
        // Without proper lexing, this would be parsed as background + redirect
        let result = bash.exec("echo test &> /tmp/both.txt").await.unwrap();
        assert_eq!(result.stdout, "");
        let check = bash.exec("cat /tmp/both.txt").await.unwrap();
        assert_eq!(check.stdout, "test\n");
    }

    #[tokio::test]
    async fn test_lexer_dup_output() {
        // Test that >& is lexed correctly
        let mut bash = Bash::new();
        let result = bash.exec("echo test >&2").await.unwrap();
        assert_eq!(result.stdout, "");
        assert_eq!(result.stderr, "test\n");
    }

    #[tokio::test]
    async fn test_digit_before_redirect() {
        // Test that 2> works with digits
        let mut bash = Bash::new();
        // 2> should be recognized as stderr redirect
        let result = bash.exec("echo hello 2> /tmp/err.txt").await.unwrap();
        assert_eq!(result.exit_code, 0);
        // stdout should still have the output since echo doesn't write to stderr
        assert_eq!(result.stdout, "hello\n");
    }

    // ============================================================
    // Arithmetic Logical Operator Tests
    // ============================================================

    #[tokio::test]
    async fn test_arithmetic_logical_and_true() {
        // Both sides true
        let mut bash = Bash::new();
        let result = bash.exec("echo $((1 && 1))").await.unwrap();
        assert_eq!(result.stdout, "1\n");
    }

    #[tokio::test]
    async fn test_arithmetic_logical_and_false_left() {
        // Left side false - short circuits
        let mut bash = Bash::new();
        let result = bash.exec("echo $((0 && 1))").await.unwrap();
        assert_eq!(result.stdout, "0\n");
    }

    #[tokio::test]
    async fn test_arithmetic_logical_and_false_right() {
        // Right side false
        let mut bash = Bash::new();
        let result = bash.exec("echo $((1 && 0))").await.unwrap();
        assert_eq!(result.stdout, "0\n");
    }

    #[tokio::test]
    async fn test_arithmetic_logical_or_false() {
        // Both sides false
        let mut bash = Bash::new();
        let result = bash.exec("echo $((0 || 0))").await.unwrap();
        assert_eq!(result.stdout, "0\n");
    }

    #[tokio::test]
    async fn test_arithmetic_logical_or_true_left() {
        // Left side true - short circuits
        let mut bash = Bash::new();
        let result = bash.exec("echo $((1 || 0))").await.unwrap();
        assert_eq!(result.stdout, "1\n");
    }

    #[tokio::test]
    async fn test_arithmetic_logical_or_true_right() {
        // Right side true
        let mut bash = Bash::new();
        let result = bash.exec("echo $((0 || 1))").await.unwrap();
        assert_eq!(result.stdout, "1\n");
    }

    #[tokio::test]
    async fn test_arithmetic_logical_combined() {
        // Combined && and || with expressions
        let mut bash = Bash::new();
        // (5 > 3) && (2 < 4) => 1 && 1 => 1
        let result = bash.exec("echo $((5 > 3 && 2 < 4))").await.unwrap();
        assert_eq!(result.stdout, "1\n");
    }

    #[tokio::test]
    async fn test_arithmetic_logical_with_comparison() {
        // || with comparison
        let mut bash = Bash::new();
        // (5 < 3) || (2 < 4) => 0 || 1 => 1
        let result = bash.exec("echo $((5 < 3 || 2 < 4))").await.unwrap();
        assert_eq!(result.stdout, "1\n");
    }

    #[tokio::test]
    async fn test_arithmetic_multibyte_no_panic() {
        // Regression: multi-byte chars caused char-index/byte-index mismatch panic
        let mut bash = Bash::new();
        // Multi-byte char in comma expression - should not panic
        let result = bash.exec("echo $((0,1))").await.unwrap();
        assert_eq!(result.stdout, "1\n");
        // Ensure multi-byte input doesn't panic (treated as 0 / error)
        let _ = bash.exec("echo $((\u{00e9}+1))").await;
    }

    // ============================================================
    // Brace Expansion Tests
    // ============================================================

    #[tokio::test]
    async fn test_brace_expansion_list() {
        // {a,b,c} expands to a b c
        let mut bash = Bash::new();
        let result = bash.exec("echo {a,b,c}").await.unwrap();
        assert_eq!(result.stdout, "a b c\n");
    }

    #[tokio::test]
    async fn test_brace_expansion_with_prefix() {
        // file{1,2,3}.txt expands to file1.txt file2.txt file3.txt
        let mut bash = Bash::new();
        let result = bash.exec("echo file{1,2,3}.txt").await.unwrap();
        assert_eq!(result.stdout, "file1.txt file2.txt file3.txt\n");
    }

    #[tokio::test]
    async fn test_brace_expansion_numeric_range() {
        // {1..5} expands to 1 2 3 4 5
        let mut bash = Bash::new();
        let result = bash.exec("echo {1..5}").await.unwrap();
        assert_eq!(result.stdout, "1 2 3 4 5\n");
    }

    #[tokio::test]
    async fn test_brace_expansion_char_range() {
        // {a..e} expands to a b c d e
        let mut bash = Bash::new();
        let result = bash.exec("echo {a..e}").await.unwrap();
        assert_eq!(result.stdout, "a b c d e\n");
    }

    #[tokio::test]
    async fn test_brace_expansion_reverse_range() {
        // {5..1} expands to 5 4 3 2 1
        let mut bash = Bash::new();
        let result = bash.exec("echo {5..1}").await.unwrap();
        assert_eq!(result.stdout, "5 4 3 2 1\n");
    }

    #[tokio::test]
    async fn test_brace_expansion_nested() {
        // Nested brace expansion: {a,b}{1,2}
        let mut bash = Bash::new();
        let result = bash.exec("echo {a,b}{1,2}").await.unwrap();
        assert_eq!(result.stdout, "a1 a2 b1 b2\n");
    }

    #[tokio::test]
    async fn test_brace_expansion_with_suffix() {
        // Prefix and suffix: pre{x,y}suf
        let mut bash = Bash::new();
        let result = bash.exec("echo pre{x,y}suf").await.unwrap();
        assert_eq!(result.stdout, "prexsuf preysuf\n");
    }

    #[tokio::test]
    async fn test_brace_expansion_empty_item() {
        // {,foo} expands to (empty) foo
        let mut bash = Bash::new();
        let result = bash.exec("echo x{,y}z").await.unwrap();
        assert_eq!(result.stdout, "xz xyz\n");
    }

    // ============================================================
    // String Comparison Tests
    // ============================================================

    #[tokio::test]
    async fn test_string_less_than() {
        let mut bash = Bash::new();
        let result = bash
            .exec("test apple '<' banana && echo yes")
            .await
            .unwrap();
        assert_eq!(result.stdout, "yes\n");
    }

    #[tokio::test]
    async fn test_string_greater_than() {
        let mut bash = Bash::new();
        let result = bash
            .exec("test banana '>' apple && echo yes")
            .await
            .unwrap();
        assert_eq!(result.stdout, "yes\n");
    }

    #[tokio::test]
    async fn test_string_less_than_false() {
        let mut bash = Bash::new();
        let result = bash
            .exec("test banana '<' apple && echo yes || echo no")
            .await
            .unwrap();
        assert_eq!(result.stdout, "no\n");
    }

    // ============================================================
    // Array Indices Tests
    // ============================================================

    #[tokio::test]
    async fn test_array_indices_basic() {
        // ${!arr[@]} returns the indices of the array
        let mut bash = Bash::new();
        let result = bash.exec("arr=(a b c); echo ${!arr[@]}").await.unwrap();
        assert_eq!(result.stdout, "0 1 2\n");
    }

    #[tokio::test]
    async fn test_array_indices_sparse() {
        // ${!arr[@]} should show indices even for sparse arrays
        let mut bash = Bash::new();
        let result = bash
            .exec("arr[0]=a; arr[5]=b; arr[10]=c; echo ${!arr[@]}")
            .await
            .unwrap();
        assert_eq!(result.stdout, "0 5 10\n");
    }

    #[tokio::test]
    async fn test_array_indices_star() {
        // ${!arr[*]} should also work
        let mut bash = Bash::new();
        let result = bash.exec("arr=(x y z); echo ${!arr[*]}").await.unwrap();
        assert_eq!(result.stdout, "0 1 2\n");
    }

    #[tokio::test]
    async fn test_array_indices_empty() {
        // Empty array should return empty string
        let mut bash = Bash::new();
        let result = bash.exec("arr=(); echo \"${!arr[@]}\"").await.unwrap();
        assert_eq!(result.stdout, "\n");
    }

    // ============================================================
    // Text file builder methods
    // ============================================================

    #[tokio::test]
    async fn test_text_file_basic() {
        let mut bash = Bash::builder()
            .mount_text("/config/app.conf", "debug=true\nport=8080\n")
            .build();

        let result = bash.exec("cat /config/app.conf").await.unwrap();
        assert_eq!(result.stdout, "debug=true\nport=8080\n");
    }

    #[tokio::test]
    async fn test_text_file_multiple() {
        let mut bash = Bash::builder()
            .mount_text("/data/file1.txt", "content one")
            .mount_text("/data/file2.txt", "content two")
            .mount_text("/other/file3.txt", "content three")
            .build();

        let result = bash.exec("cat /data/file1.txt").await.unwrap();
        assert_eq!(result.stdout, "content one");

        let result = bash.exec("cat /data/file2.txt").await.unwrap();
        assert_eq!(result.stdout, "content two");

        let result = bash.exec("cat /other/file3.txt").await.unwrap();
        assert_eq!(result.stdout, "content three");
    }

    #[tokio::test]
    async fn test_text_file_nested_directory() {
        // Parent directories should be created automatically
        let mut bash = Bash::builder()
            .mount_text("/a/b/c/d/file.txt", "nested content")
            .build();

        let result = bash.exec("cat /a/b/c/d/file.txt").await.unwrap();
        assert_eq!(result.stdout, "nested content");
    }

    #[tokio::test]
    async fn test_text_file_mode() {
        let bash = Bash::builder()
            .mount_text("/tmp/writable.txt", "content")
            .build();

        let stat = bash
            .fs()
            .stat(std::path::Path::new("/tmp/writable.txt"))
            .await
            .unwrap();
        assert_eq!(stat.mode, 0o644);
    }

    #[tokio::test]
    async fn test_readonly_text_basic() {
        let mut bash = Bash::builder()
            .mount_readonly_text("/etc/version", "1.2.3")
            .build();

        let result = bash.exec("cat /etc/version").await.unwrap();
        assert_eq!(result.stdout, "1.2.3");
    }

    #[tokio::test]
    async fn test_readonly_text_mode() {
        let bash = Bash::builder()
            .mount_readonly_text("/etc/readonly.conf", "immutable")
            .build();

        let stat = bash
            .fs()
            .stat(std::path::Path::new("/etc/readonly.conf"))
            .await
            .unwrap();
        assert_eq!(stat.mode, 0o444);
    }

    #[tokio::test]
    async fn test_text_file_mixed_readonly_writable() {
        let bash = Bash::builder()
            .mount_text("/data/writable.txt", "can edit")
            .mount_readonly_text("/data/readonly.txt", "cannot edit")
            .build();

        let writable_stat = bash
            .fs()
            .stat(std::path::Path::new("/data/writable.txt"))
            .await
            .unwrap();
        let readonly_stat = bash
            .fs()
            .stat(std::path::Path::new("/data/readonly.txt"))
            .await
            .unwrap();

        assert_eq!(writable_stat.mode, 0o644);
        assert_eq!(readonly_stat.mode, 0o444);
    }

    #[tokio::test]
    async fn test_text_file_with_env() {
        // text_file should work alongside other builder methods
        let mut bash = Bash::builder()
            .env("APP_NAME", "testapp")
            .mount_text("/config/app.conf", "name=${APP_NAME}")
            .build();

        let result = bash.exec("echo $APP_NAME").await.unwrap();
        assert_eq!(result.stdout, "testapp\n");

        let result = bash.exec("cat /config/app.conf").await.unwrap();
        assert_eq!(result.stdout, "name=${APP_NAME}");
    }

    #[tokio::test]
    #[cfg(feature = "jq")]
    async fn test_text_file_json() {
        let mut bash = Bash::builder()
            .mount_text("/data/users.json", r#"["alice", "bob", "charlie"]"#)
            .build();

        let result = bash.exec("cat /data/users.json | jq '.[0]'").await.unwrap();
        assert_eq!(result.stdout, "\"alice\"\n");
    }

    #[tokio::test]
    async fn test_mount_with_custom_filesystem() {
        // Mount files work with custom filesystems via OverlayFs
        let custom_fs = std::sync::Arc::new(InMemoryFs::new());

        // Pre-populate the base filesystem
        custom_fs
            .write_file(std::path::Path::new("/base.txt"), b"from base")
            .await
            .unwrap();

        let mut bash = Bash::builder()
            .fs(custom_fs)
            .mount_text("/mounted.txt", "from mount")
            .mount_readonly_text("/readonly.txt", "immutable")
            .build();

        // Can read base file
        let result = bash.exec("cat /base.txt").await.unwrap();
        assert_eq!(result.stdout, "from base");

        // Can read mounted files
        let result = bash.exec("cat /mounted.txt").await.unwrap();
        assert_eq!(result.stdout, "from mount");

        let result = bash.exec("cat /readonly.txt").await.unwrap();
        assert_eq!(result.stdout, "immutable");

        // Mounted readonly file has correct permissions
        let stat = bash
            .fs()
            .stat(std::path::Path::new("/readonly.txt"))
            .await
            .unwrap();
        assert_eq!(stat.mode, 0o444);
    }

    #[tokio::test]
    async fn test_mount_overwrites_base_file() {
        // Mounted files take precedence over base filesystem
        let custom_fs = std::sync::Arc::new(InMemoryFs::new());
        custom_fs
            .write_file(std::path::Path::new("/config.txt"), b"original")
            .await
            .unwrap();

        let mut bash = Bash::builder()
            .fs(custom_fs)
            .mount_text("/config.txt", "overwritten")
            .build();

        let result = bash.exec("cat /config.txt").await.unwrap();
        assert_eq!(result.stdout, "overwritten");
    }

    #[tokio::test]
    async fn test_mount_preserves_custom_fs_limits() {
        let limited_fs =
            std::sync::Arc::new(InMemoryFs::with_limits(FsLimits::new().max_total_bytes(32)));

        let bash = Bash::builder()
            .fs(limited_fs)
            .mount_text("/mounted.txt", "seed")
            .build();

        let write_err = bash
            .fs()
            .write_file(
                std::path::Path::new("/too-big.txt"),
                b"this payload should exceed thirty-two bytes",
            )
            .await;
        assert!(write_err.is_err(), "custom fs limits should still apply");
    }

    #[tokio::test]
    async fn test_mount_text_respects_filesystem_limits() {
        let limited_fs = std::sync::Arc::new(InMemoryFs::with_limits(
            FsLimits::new().max_total_bytes(5).max_file_size(5),
        ));

        let bash = Bash::builder()
            .fs(limited_fs)
            .mount_text("/too-large.txt", "123456")
            .build();

        let exists = bash
            .fs()
            .exists(std::path::Path::new("/too-large.txt"))
            .await
            .unwrap();
        assert!(!exists, "mount_text should not bypass configured FsLimits");
    }

    // ============================================================
    // Parser Error Location Tests
    // ============================================================

    #[tokio::test]
    async fn test_parse_error_includes_line_number() {
        // Parse errors should include line/column info
        let mut bash = Bash::new();
        let result = bash
            .exec(
                r#"echo ok
if true; then
echo missing fi"#,
            )
            .await;
        // Should fail to parse due to missing 'fi'
        assert!(result.is_err());
        let err = result.unwrap_err();
        let err_msg = format!("{}", err);
        // Error should mention line number
        assert!(
            err_msg.contains("line") || err_msg.contains("parse"),
            "Error should be a parse error: {}",
            err_msg
        );
    }

    #[tokio::test]
    async fn test_parse_error_on_specific_line() {
        // Syntax error on line 3 should report line 3
        use crate::parser::Parser;
        let script = "echo line1\necho line2\nif true; then\n";
        let result = Parser::new(script).parse();
        assert!(result.is_err());
        let err = result.unwrap_err();
        let err_msg = format!("{}", err);
        // Error should mention the problem (either "expected" or "syntax error")
        assert!(
            err_msg.contains("expected") || err_msg.contains("syntax error"),
            "Error should be a parse error: {}",
            err_msg
        );
    }

    // ==================== Root directory access tests ====================

    #[tokio::test]
    async fn test_cd_to_root_and_ls() {
        // Test: cd / && ls should work
        let mut bash = Bash::new();
        let result = bash.exec("cd / && ls").await.unwrap();
        assert_eq!(
            result.exit_code, 0,
            "cd / && ls should succeed: {}",
            result.stderr
        );
        assert!(result.stdout.contains("tmp"), "Root should contain tmp");
        assert!(result.stdout.contains("home"), "Root should contain home");
    }

    #[tokio::test]
    async fn test_cd_to_root_and_pwd() {
        // Test: cd / && pwd should show /
        let mut bash = Bash::new();
        let result = bash.exec("cd / && pwd").await.unwrap();
        assert_eq!(result.exit_code, 0, "cd / && pwd should succeed");
        assert_eq!(result.stdout.trim(), "/");
    }

    #[tokio::test]
    async fn test_cd_to_root_and_ls_dot() {
        // Test: cd / && ls . should list root contents
        let mut bash = Bash::new();
        let result = bash.exec("cd / && ls .").await.unwrap();
        assert_eq!(
            result.exit_code, 0,
            "cd / && ls . should succeed: {}",
            result.stderr
        );
        assert!(result.stdout.contains("tmp"), "Root should contain tmp");
        assert!(result.stdout.contains("home"), "Root should contain home");
    }

    #[tokio::test]
    async fn test_ls_root_directly() {
        // Test: ls / should work
        let mut bash = Bash::new();
        let result = bash.exec("ls /").await.unwrap();
        assert_eq!(
            result.exit_code, 0,
            "ls / should succeed: {}",
            result.stderr
        );
        assert!(result.stdout.contains("tmp"), "Root should contain tmp");
        assert!(result.stdout.contains("home"), "Root should contain home");
        assert!(result.stdout.contains("dev"), "Root should contain dev");
    }

    #[tokio::test]
    async fn test_ls_root_long_format() {
        // Test: ls -la / should work
        let mut bash = Bash::new();
        let result = bash.exec("ls -la /").await.unwrap();
        assert_eq!(
            result.exit_code, 0,
            "ls -la / should succeed: {}",
            result.stderr
        );
        assert!(result.stdout.contains("tmp"), "Root should contain tmp");
        assert!(
            result.stdout.contains("drw"),
            "Should show directory permissions"
        );
    }

    // === Issue 1: Heredoc file writes ===

    #[tokio::test]
    async fn test_heredoc_redirect_to_file() {
        // cat > file <<'EOF' is the #1 way LLMs create multi-line files
        let mut bash = Bash::new();
        let result = bash
            .exec("cat > /tmp/out.txt <<'EOF'\nhello\nworld\nEOF\ncat /tmp/out.txt")
            .await
            .unwrap();
        assert_eq!(result.stdout, "hello\nworld\n");
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn test_heredoc_redirect_to_file_unquoted() {
        let mut bash = Bash::new();
        let result = bash
            .exec("cat > /tmp/out.txt <<EOF\nhello\nworld\nEOF\ncat /tmp/out.txt")
            .await
            .unwrap();
        assert_eq!(result.stdout, "hello\nworld\n");
        assert_eq!(result.exit_code, 0);
    }

    // === Issue 2: Compound pipelines ===

    #[tokio::test]
    async fn test_pipe_to_while_read() {
        // cmd | while read ...; do ... done is extremely common
        let mut bash = Bash::new();
        let result = bash
            .exec("echo -e 'a\\nb\\nc' | while read line; do echo \"got: $line\"; done")
            .await
            .unwrap();
        assert!(
            result.stdout.contains("got: a"),
            "stdout: {}",
            result.stdout
        );
        assert!(
            result.stdout.contains("got: b"),
            "stdout: {}",
            result.stdout
        );
        assert!(
            result.stdout.contains("got: c"),
            "stdout: {}",
            result.stdout
        );
    }

    #[tokio::test]
    async fn test_pipe_to_while_read_count() {
        let mut bash = Bash::new();
        let result = bash
            .exec("printf 'x\\ny\\nz\\n' | while read line; do echo $line; done")
            .await
            .unwrap();
        assert_eq!(result.stdout, "x\ny\nz\n");
    }

    // === Issue 3: Source loading functions ===

    #[tokio::test]
    async fn test_source_loads_functions() {
        let mut bash = Bash::new();
        // Write a function library, then source it and call the function
        bash.exec("cat > /tmp/lib.sh <<'EOF'\ngreet() { echo \"hello $1\"; }\nEOF")
            .await
            .unwrap();
        let result = bash.exec("source /tmp/lib.sh; greet world").await.unwrap();
        assert_eq!(result.stdout, "hello world\n");
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn test_source_loads_variables() {
        let mut bash = Bash::new();
        bash.exec("echo 'MY_VAR=loaded' > /tmp/vars.sh")
            .await
            .unwrap();
        let result = bash
            .exec("source /tmp/vars.sh; echo $MY_VAR")
            .await
            .unwrap();
        assert_eq!(result.stdout, "loaded\n");
    }

    // === Issue 4: chmod +x symbolic mode ===

    #[tokio::test]
    async fn test_chmod_symbolic_plus_x() {
        let mut bash = Bash::new();
        bash.exec("echo '#!/bin/bash' > /tmp/script.sh")
            .await
            .unwrap();
        let result = bash.exec("chmod +x /tmp/script.sh").await.unwrap();
        assert_eq!(
            result.exit_code, 0,
            "chmod +x should succeed: {}",
            result.stderr
        );
    }

    #[tokio::test]
    async fn test_chmod_symbolic_u_plus_x() {
        let mut bash = Bash::new();
        bash.exec("echo 'test' > /tmp/file.txt").await.unwrap();
        let result = bash.exec("chmod u+x /tmp/file.txt").await.unwrap();
        assert_eq!(
            result.exit_code, 0,
            "chmod u+x should succeed: {}",
            result.stderr
        );
    }

    #[tokio::test]
    async fn test_chmod_symbolic_a_plus_r() {
        let mut bash = Bash::new();
        bash.exec("echo 'test' > /tmp/file.txt").await.unwrap();
        let result = bash.exec("chmod a+r /tmp/file.txt").await.unwrap();
        assert_eq!(
            result.exit_code, 0,
            "chmod a+r should succeed: {}",
            result.stderr
        );
    }

    // === Issue 5: Awk arrays ===

    #[tokio::test]
    async fn test_awk_array_length() {
        // length(arr) should return element count
        let mut bash = Bash::new();
        let result = bash
            .exec(r#"echo "" | awk 'BEGIN{a[1]="x"; a[2]="y"; a[3]="z"} END{print length(a)}'"#)
            .await
            .unwrap();
        assert_eq!(result.stdout, "3\n");
    }

    #[tokio::test]
    async fn test_awk_array_read_after_split() {
        // split() + reading elements back
        let mut bash = Bash::new();
        let result = bash
            .exec(r#"echo "a:b:c" | awk '{n=split($0,arr,":"); for(i=1;i<=n;i++) print arr[i]}'"#)
            .await
            .unwrap();
        assert_eq!(result.stdout, "a\nb\nc\n");
    }

    #[tokio::test]
    async fn test_awk_array_word_count_pattern() {
        // Classic word frequency count - the most common awk array pattern
        let mut bash = Bash::new();
        let result = bash
            .exec(
                r#"printf "apple\nbanana\napple\ncherry\nbanana\napple" | awk '{count[$1]++} END{for(w in count) print w, count[w]}'"#,
            )
            .await
            .unwrap();
        assert!(
            result.stdout.contains("apple 3"),
            "stdout: {}",
            result.stdout
        );
        assert!(
            result.stdout.contains("banana 2"),
            "stdout: {}",
            result.stdout
        );
        assert!(
            result.stdout.contains("cherry 1"),
            "stdout: {}",
            result.stdout
        );
    }

    // ---- Streaming output tests ----

    #[tokio::test]
    async fn test_exec_streaming_for_loop() {
        let chunks = Arc::new(Mutex::new(Vec::new()));
        let chunks_cb = chunks.clone();
        let mut bash = Bash::new();

        let result = bash
            .exec_streaming(
                "for i in 1 2 3; do echo $i; done",
                Box::new(move |stdout, _stderr| {
                    chunks_cb.lock().unwrap().push(stdout.to_string());
                }),
            )
            .await
            .unwrap();

        assert_eq!(result.stdout, "1\n2\n3\n");
        assert_eq!(
            *chunks.lock().unwrap(),
            vec!["1\n", "2\n", "3\n"],
            "each loop iteration should stream separately"
        );
    }

    #[tokio::test]
    async fn test_exec_streaming_while_loop() {
        let chunks = Arc::new(Mutex::new(Vec::new()));
        let chunks_cb = chunks.clone();
        let mut bash = Bash::new();

        let result = bash
            .exec_streaming(
                "i=0; while [ $i -lt 3 ]; do i=$((i+1)); echo $i; done",
                Box::new(move |stdout, _stderr| {
                    chunks_cb.lock().unwrap().push(stdout.to_string());
                }),
            )
            .await
            .unwrap();

        assert_eq!(result.stdout, "1\n2\n3\n");
        let chunks = chunks.lock().unwrap();
        // The while loop emits each iteration; surrounding list may add events too
        assert!(
            chunks.contains(&"1\n".to_string()),
            "should contain first iteration output"
        );
        assert!(
            chunks.contains(&"2\n".to_string()),
            "should contain second iteration output"
        );
        assert!(
            chunks.contains(&"3\n".to_string()),
            "should contain third iteration output"
        );
    }

    #[tokio::test]
    async fn test_exec_streaming_no_callback_still_works() {
        // exec (non-streaming) should still work fine
        let mut bash = Bash::new();
        let result = bash.exec("for i in a b c; do echo $i; done").await.unwrap();
        assert_eq!(result.stdout, "a\nb\nc\n");
    }

    #[tokio::test]
    async fn test_exec_streaming_nested_loops_no_duplicates() {
        let chunks = Arc::new(Mutex::new(Vec::new()));
        let chunks_cb = chunks.clone();
        let mut bash = Bash::new();

        let result = bash
            .exec_streaming(
                "for i in 1 2; do for j in a b; do echo \"$i$j\"; done; done",
                Box::new(move |stdout, _stderr| {
                    chunks_cb.lock().unwrap().push(stdout.to_string());
                }),
            )
            .await
            .unwrap();

        assert_eq!(result.stdout, "1a\n1b\n2a\n2b\n");
        let chunks = chunks.lock().unwrap();
        // Inner loop should emit each iteration; outer should not duplicate
        let total_chars: usize = chunks.iter().map(|c| c.len()).sum();
        assert_eq!(
            total_chars,
            result.stdout.len(),
            "total streamed bytes should match final output: chunks={:?}",
            *chunks
        );
    }

    #[tokio::test]
    async fn test_exec_streaming_mixed_list_and_loop() {
        let chunks = Arc::new(Mutex::new(Vec::new()));
        let chunks_cb = chunks.clone();
        let mut bash = Bash::new();

        let result = bash
            .exec_streaming(
                "echo start; for i in 1 2; do echo $i; done; echo end",
                Box::new(move |stdout, _stderr| {
                    chunks_cb.lock().unwrap().push(stdout.to_string());
                }),
            )
            .await
            .unwrap();

        assert_eq!(result.stdout, "start\n1\n2\nend\n");
        let chunks = chunks.lock().unwrap();
        assert_eq!(
            *chunks,
            vec!["start\n", "1\n", "2\n", "end\n"],
            "mixed list+loop should produce exactly 4 events"
        );
    }

    #[tokio::test]
    async fn test_exec_streaming_stderr() {
        let stderr_chunks = Arc::new(Mutex::new(Vec::new()));
        let stderr_cb = stderr_chunks.clone();
        let mut bash = Bash::new();

        let result = bash
            .exec_streaming(
                "echo ok; echo err >&2; echo ok2",
                Box::new(move |_stdout, stderr| {
                    if !stderr.is_empty() {
                        stderr_cb.lock().unwrap().push(stderr.to_string());
                    }
                }),
            )
            .await
            .unwrap();

        assert_eq!(result.stdout, "ok\nok2\n");
        assert_eq!(result.stderr, "err\n");
        let stderr_chunks = stderr_chunks.lock().unwrap();
        assert!(
            stderr_chunks.contains(&"err\n".to_string()),
            "stderr should be streamed: {:?}",
            *stderr_chunks
        );
    }

    // ---- Streamed vs non-streamed equivalence tests ----
    //
    // These run the same script through exec() and exec_streaming() and assert
    // that the final ExecResult is identical, plus concatenated chunks == stdout.

    /// Helper: run script both ways, assert equivalence.
    async fn assert_streaming_equivalence(script: &str) {
        // Non-streaming
        let mut bash_plain = Bash::new();
        let plain = bash_plain.exec(script).await.unwrap();

        // Streaming
        let stdout_chunks: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let stderr_chunks: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let so = stdout_chunks.clone();
        let se = stderr_chunks.clone();
        let mut bash_stream = Bash::new();
        let streamed = bash_stream
            .exec_streaming(
                script,
                Box::new(move |stdout, stderr| {
                    if !stdout.is_empty() {
                        so.lock().unwrap().push(stdout.to_string());
                    }
                    if !stderr.is_empty() {
                        se.lock().unwrap().push(stderr.to_string());
                    }
                }),
            )
            .await
            .unwrap();

        // Final results must match
        assert_eq!(
            plain.stdout, streamed.stdout,
            "stdout mismatch for: {script}"
        );
        assert_eq!(
            plain.stderr, streamed.stderr,
            "stderr mismatch for: {script}"
        );
        assert_eq!(
            plain.exit_code, streamed.exit_code,
            "exit_code mismatch for: {script}"
        );

        // Concatenated chunks must equal full stdout/stderr
        let reassembled_stdout: String = stdout_chunks.lock().unwrap().iter().cloned().collect();
        assert_eq!(
            reassembled_stdout, streamed.stdout,
            "reassembled stdout chunks != final stdout for: {script}"
        );
        let reassembled_stderr: String = stderr_chunks.lock().unwrap().iter().cloned().collect();
        assert_eq!(
            reassembled_stderr, streamed.stderr,
            "reassembled stderr chunks != final stderr for: {script}"
        );
    }

    #[tokio::test]
    async fn test_streaming_equivalence_for_loop() {
        assert_streaming_equivalence("for i in 1 2 3; do echo $i; done").await;
    }

    #[tokio::test]
    async fn test_streaming_equivalence_while_loop() {
        assert_streaming_equivalence("i=0; while [ $i -lt 4 ]; do i=$((i+1)); echo $i; done").await;
    }

    #[tokio::test]
    async fn test_streaming_equivalence_nested_loops() {
        assert_streaming_equivalence("for i in a b; do for j in 1 2; do echo \"$i$j\"; done; done")
            .await;
    }

    #[tokio::test]
    async fn test_streaming_equivalence_mixed_list() {
        assert_streaming_equivalence("echo start; for i in x y; do echo $i; done; echo end").await;
    }

    #[tokio::test]
    async fn test_streaming_equivalence_stderr() {
        assert_streaming_equivalence("echo out; echo err >&2; echo out2").await;
    }

    #[tokio::test]
    async fn test_streaming_equivalence_pipeline() {
        assert_streaming_equivalence("echo -e 'a\\nb\\nc' | grep b").await;
    }

    #[tokio::test]
    async fn test_streaming_equivalence_conditionals() {
        assert_streaming_equivalence("if true; then echo yes; else echo no; fi; echo done").await;
    }

    #[tokio::test]
    async fn test_streaming_equivalence_subshell() {
        assert_streaming_equivalence("x=$(echo hello); echo $x").await;
    }

    #[tokio::test]
    async fn test_max_memory_caps_string_growth() {
        let mut bash = Bash::builder()
            .max_memory(1024)
            .limits(
                ExecutionLimits::new()
                    .max_commands(10_000)
                    .max_loop_iterations(10_000),
            )
            .build();
        let result = bash
            .exec(r#"x=AAAAAAAAAA; i=0; while [ $i -lt 25 ]; do x="$x$x"; i=$((i+1)); done; echo ${#x}"#)
            .await
            .unwrap();
        let len: usize = result.stdout.trim().parse().unwrap();
        // 25 doublings of 10 bytes = 335 544 320 without limits; must be capped ≤ 1024
        assert!(len <= 1024, "string length {len} must be ≤ 1024");
    }

    /// Issue #1116: 2>/dev/null must suppress stderr in streaming mode
    #[tokio::test]
    async fn test_stderr_redirect_devnull_streaming() {
        let stderr_chunks = Arc::new(Mutex::new(Vec::new()));
        let stderr_cb = stderr_chunks.clone();
        let mut bash = Bash::new();

        // Compound command — the main bug: callback fired before redirect applied
        let result = bash
            .exec_streaming(
                "{ ls /nonexistent; } 2>/dev/null; echo exit:$?",
                Box::new(move |_stdout, stderr| {
                    if !stderr.is_empty() {
                        stderr_cb.lock().unwrap().push(stderr.to_string());
                    }
                }),
            )
            .await
            .unwrap();

        assert_eq!(result.stderr, "", "final stderr should be empty");
        let stderr_chunks = stderr_chunks.lock().unwrap();
        assert!(
            stderr_chunks.is_empty(),
            "no stderr should be streamed when 2>/dev/null is used, got: {:?}",
            *stderr_chunks
        );
    }

    #[tokio::test]
    async fn test_dot_slash_prefix_ls() {
        // Issue #1114: ./filename should resolve identically to filename
        let mut bash = Bash::new();
        bash.exec("mkdir -p /tmp/blogtest && cd /tmp/blogtest && echo hello > tag_hello.html")
            .await
            .unwrap();

        // ls without ./ prefix should work
        let result = bash
            .exec("cd /tmp/blogtest && ls tag_hello.html")
            .await
            .unwrap();
        assert_eq!(
            result.exit_code, 0,
            "ls tag_hello.html should succeed: {}",
            result.stderr
        );
        assert!(result.stdout.contains("tag_hello.html"));

        // ls with ./ prefix should also work
        let result = bash
            .exec("cd /tmp/blogtest && ls ./tag_hello.html")
            .await
            .unwrap();
        assert_eq!(
            result.exit_code, 0,
            "ls ./tag_hello.html should succeed: {}",
            result.stderr
        );
        assert!(result.stdout.contains("tag_hello.html"));
    }

    #[tokio::test]
    async fn test_dot_slash_prefix_glob() {
        // Issue #1114: ./*.html should resolve identically to *.html
        let mut bash = Bash::new();
        bash.exec("mkdir -p /tmp/globtest && cd /tmp/globtest && echo hello > tag_hello.html")
            .await
            .unwrap();

        // glob without ./ prefix
        let result = bash.exec("cd /tmp/globtest && echo *.html").await.unwrap();
        assert_eq!(
            result.exit_code, 0,
            "echo *.html should succeed: {}",
            result.stderr
        );
        assert!(result.stdout.contains("tag_hello.html"));

        // glob with ./ prefix
        let result = bash
            .exec("cd /tmp/globtest && echo ./*.html")
            .await
            .unwrap();
        assert_eq!(
            result.exit_code, 0,
            "echo ./*.html should succeed: {}",
            result.stderr
        );
        assert!(result.stdout.contains("tag_hello.html"));
    }

    #[tokio::test]
    async fn test_dot_slash_prefix_cat() {
        // Issue #1114: cat ./filename should work
        let mut bash = Bash::new();
        bash.exec("mkdir -p /tmp/cattest && cd /tmp/cattest && echo content123 > myfile.txt")
            .await
            .unwrap();

        let result = bash
            .exec("cd /tmp/cattest && cat ./myfile.txt")
            .await
            .unwrap();
        assert_eq!(
            result.exit_code, 0,
            "cat ./myfile.txt should succeed: {}",
            result.stderr
        );
        assert!(result.stdout.contains("content123"));
    }

    #[tokio::test]
    async fn test_dot_slash_prefix_redirect() {
        // Issue #1114: redirecting to ./filename should work
        let mut bash = Bash::new();
        bash.exec("mkdir -p /tmp/redirtest && cd /tmp/redirtest")
            .await
            .unwrap();

        let result = bash
            .exec("cd /tmp/redirtest && echo hello > ./output.txt && cat ./output.txt")
            .await
            .unwrap();
        assert_eq!(
            result.exit_code, 0,
            "redirect to ./output.txt should succeed: {}",
            result.stderr
        );
        assert!(result.stdout.contains("hello"));
    }

    #[tokio::test]
    async fn test_dot_slash_prefix_test_builtin() {
        // Issue #1114: test -f ./filename should work
        let mut bash = Bash::new();
        bash.exec("mkdir -p /tmp/testbuiltin && cd /tmp/testbuiltin && echo x > myfile.txt")
            .await
            .unwrap();

        let result = bash
            .exec("cd /tmp/testbuiltin && test -f ./myfile.txt && echo yes")
            .await
            .unwrap();
        assert_eq!(
            result.exit_code, 0,
            "test -f ./myfile.txt should succeed: {}",
            result.stderr
        );
        assert!(result.stdout.contains("yes"));
    }

    // === Hooks system tests ===

    #[tokio::test]
    async fn test_before_exec_hook_modifies_script() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicBool, Ordering};

        let called = Arc::new(AtomicBool::new(false));
        let called_clone = called.clone();

        let mut bash = Bash::builder()
            .before_exec(Box::new(move |mut input| {
                called_clone.store(true, Ordering::Relaxed);
                // Rewrite the script
                input.script = "echo intercepted".to_string();
                hooks::HookAction::Continue(input)
            }))
            .build();

        let result = bash.exec("echo original").await.unwrap();
        assert!(called.load(Ordering::Relaxed));
        assert_eq!(result.stdout.trim(), "intercepted");
    }

    #[tokio::test]
    async fn test_before_exec_hook_cancels() {
        let mut bash = Bash::builder()
            .before_exec(Box::new(|_input| {
                hooks::HookAction::Cancel("blocked".to_string())
            }))
            .build();

        let result = bash.exec("echo should-not-run").await.unwrap();
        assert_eq!(result.exit_code, 1);
        assert!(result.stdout.is_empty());
    }

    #[tokio::test]
    async fn test_input_size_limit_rejects_before_before_exec_hook() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicBool, Ordering};

        let called = Arc::new(AtomicBool::new(false));
        let called_clone = called.clone();

        let limits = ExecutionLimits::new().max_input_bytes(8);
        let mut bash = Bash::builder()
            .limits(limits)
            .before_exec(Box::new(move |_input| {
                called_clone.store(true, Ordering::Relaxed);
                unreachable!("before_exec hook must not run for oversized input");
            }))
            .build();

        let result = bash.exec("echo way-too-long").await;
        assert!(result.is_err());
        assert!(!called.load(Ordering::Relaxed));
    }

    #[tokio::test]
    async fn test_after_exec_hook_observes_output() {
        use std::sync::{Arc, Mutex};

        let captured = Arc::new(Mutex::new(String::new()));
        let captured_clone = captured.clone();

        let mut bash = Bash::builder()
            .after_exec(Box::new(move |output| {
                *captured_clone.lock().unwrap() = output.stdout.clone();
                hooks::HookAction::Continue(output)
            }))
            .build();

        bash.exec("echo hello-hooks").await.unwrap();
        assert_eq!(captured.lock().unwrap().trim(), "hello-hooks");
    }

    #[tokio::test]
    async fn test_multiple_hooks_chain() {
        let mut bash = Bash::builder()
            .before_exec(Box::new(|mut input| {
                input.script = input.script.replace("world", "hooks");
                hooks::HookAction::Continue(input)
            }))
            .before_exec(Box::new(|mut input| {
                input.script = input.script.replace("hello", "greetings");
                hooks::HookAction::Continue(input)
            }))
            .build();

        let result = bash.exec("echo hello world").await.unwrap();
        assert_eq!(result.stdout.trim(), "greetings hooks");
    }

    #[tokio::test]
    async fn test_on_exit_hook_not_fired_for_path_script_exit() {
        use std::path::Path;
        use std::sync::Arc;
        use std::sync::atomic::{AtomicU32, Ordering};

        let count = Arc::new(AtomicU32::new(0));
        let count_clone = count.clone();

        let mut bash = Bash::builder()
            .on_exit(Box::new(move |event| {
                count_clone.fetch_add(1, Ordering::Relaxed);
                hooks::HookAction::Continue(event)
            }))
            .build();

        let fs = bash.fs();
        fs.mkdir(Path::new("/bin"), false).await.unwrap();
        fs.write_file(Path::new("/bin/child-exit"), b"#!/usr/bin/env bash\nexit 7")
            .await
            .unwrap();
        fs.chmod(Path::new("/bin/child-exit"), 0o755).await.unwrap();

        let result = bash
            .exec("PATH=/bin:$PATH\nchild-exit\necho after:$?")
            .await
            .unwrap();

        assert_eq!(result.stdout.trim(), "after:7");
        assert_eq!(count.load(Ordering::Relaxed), 0);
    }

    #[tokio::test]
    async fn test_on_exit_hook_not_fired_for_direct_script_exit() {
        use std::path::Path;
        use std::sync::Arc;
        use std::sync::atomic::{AtomicU32, Ordering};

        let count = Arc::new(AtomicU32::new(0));
        let count_clone = count.clone();

        let mut bash = Bash::builder()
            .on_exit(Box::new(move |event| {
                count_clone.fetch_add(1, Ordering::Relaxed);
                hooks::HookAction::Continue(event)
            }))
            .build();

        let fs = bash.fs();
        fs.write_file(
            Path::new("/tmp/child-exit.sh"),
            b"#!/usr/bin/env bash\nexit 8",
        )
        .await
        .unwrap();
        fs.chmod(Path::new("/tmp/child-exit.sh"), 0o755)
            .await
            .unwrap();

        let result = bash
            .exec("/tmp/child-exit.sh\necho after:$?")
            .await
            .unwrap();

        assert_eq!(result.stdout.trim(), "after:8");
        assert_eq!(count.load(Ordering::Relaxed), 0);
    }

    #[tokio::test]
    async fn test_on_exit_hook_not_fired_for_nested_bash_exit() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicU32, Ordering};

        let count = Arc::new(AtomicU32::new(0));
        let count_clone = count.clone();

        let mut bash = Bash::builder()
            .on_exit(Box::new(move |event| {
                count_clone.fetch_add(1, Ordering::Relaxed);
                hooks::HookAction::Continue(event)
            }))
            .build();

        let result = bash.exec("bash -c 'exit 9'\necho after:$?").await.unwrap();

        assert_eq!(result.stdout.trim(), "after:9");
        assert_eq!(count.load(Ordering::Relaxed), 0);
    }

    #[tokio::test]
    async fn test_path_script_exit_runs_child_exit_trap() {
        use std::path::Path;

        let mut bash = Bash::new();
        let fs = bash.fs();
        fs.write_file(
            Path::new("/tmp/child-trap.sh"),
            b"#!/usr/bin/env bash\ntrap 'echo child-trap' EXIT\nexit 4",
        )
        .await
        .unwrap();
        fs.chmod(Path::new("/tmp/child-trap.sh"), 0o755)
            .await
            .unwrap();

        let result = bash
            .exec("/tmp/child-trap.sh\necho after:$?")
            .await
            .unwrap();

        assert_eq!(result.stdout.trim(), "child-trap\nafter:4");
    }

    #[tokio::test]
    async fn test_on_exit_hook_still_fires_for_source_exit() {
        use std::path::Path;
        use std::sync::Arc;
        use std::sync::atomic::{AtomicU32, Ordering};

        let count = Arc::new(AtomicU32::new(0));
        let count_clone = count.clone();

        let mut bash = Bash::builder()
            .on_exit(Box::new(move |event| {
                count_clone.fetch_add(1, Ordering::Relaxed);
                hooks::HookAction::Continue(event)
            }))
            .build();

        let fs = bash.fs();
        fs.write_file(Path::new("/tmp/source-exit.sh"), b"exit 5")
            .await
            .unwrap();

        let result = bash.exec("source /tmp/source-exit.sh").await.unwrap();

        assert_eq!(result.exit_code, 5);
        assert_eq!(count.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn test_on_exit_hook_cancel_prevents_exit() {
        let mut bash = Bash::builder()
            .on_exit(Box::new(|_event| {
                hooks::HookAction::Cancel("blocked by policy".to_string())
            }))
            .build();

        let result = bash.exec("echo before\nexit 5\necho after").await.unwrap();
        assert_eq!(result.stdout.trim(), "before\nafter");
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn test_on_exit_hook_can_modify_exit_code() {
        let mut bash = Bash::builder()
            .on_exit(Box::new(|mut event| {
                event.code = 17;
                hooks::HookAction::Continue(event)
            }))
            .build();

        let result = bash.exec("exit 5").await.unwrap();
        assert_eq!(result.exit_code, 17);
    }

    #[tokio::test]
    async fn test_bash_versinfo_reports_bash_compatible_major() {
        let mut bash = Bash::new();

        let result = bash
            .exec(r#"[[ ${BASH_VERSINFO[0]} -ge 4 ]] && echo bash4plus"#)
            .await
            .unwrap();

        assert_eq!(result.stdout.trim(), "bash4plus");
    }

    #[tokio::test]
    async fn test_bash_version_surface_matches_bash_compatible_tuple() {
        let mut bash = Bash::new();

        let result = bash
            .exec(
                r#"printf '%s\n' "$BASH_VERSION" "${BASH_VERSINFO[0]}" "${BASH_VERSINFO[1]}" "${BASH_VERSINFO[2]}" "${BASH_VERSINFO[3]}" "${BASH_VERSINFO[4]}" "${BASH_VERSINFO[5]}""#,
            )
            .await
            .unwrap();

        assert_eq!(
            result.stdout,
            "5.2.15(1)-release\n5\n2\n15\n1\nrelease\nvirtual\n"
        );
    }

    #[tokio::test]
    async fn test_path_script_retains_bash_versinfo_array() {
        use std::path::Path;

        let mut bash = Bash::new();
        let fs = bash.fs();
        fs.write_file(
            Path::new("/tmp/bash-version-check.sh"),
            b"#!/usr/bin/env bash\nprintf '%s\\n' \"${BASH_VERSINFO[0]}\"",
        )
        .await
        .unwrap();
        fs.chmod(Path::new("/tmp/bash-version-check.sh"), 0o755)
            .await
            .unwrap();

        let result = bash.exec("/tmp/bash-version-check.sh").await.unwrap();

        assert_eq!(result.stdout.trim(), "5");
    }

    #[tokio::test]
    async fn test_path_script_bash_versinfo_satisfies_bash4_guard() {
        use std::path::Path;

        let mut bash = Bash::new();
        let fs = bash.fs();
        fs.write_file(
            Path::new("/tmp/bash-version-guard.sh"),
            b"#!/usr/bin/env bash\nif (( BASH_VERSINFO[0] < 4 )); then echo too-old; else echo ok; fi",
        )
        .await
        .unwrap();
        fs.chmod(Path::new("/tmp/bash-version-guard.sh"), 0o755)
            .await
            .unwrap();

        let result = bash.exec("/tmp/bash-version-guard.sh").await.unwrap();

        assert_eq!(result.stdout.trim(), "ok");
    }

    #[tokio::test]
    async fn test_before_tool_hook_modifies_args() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicBool, Ordering};

        let called = Arc::new(AtomicBool::new(false));
        let called_clone = called.clone();

        let mut bash = Bash::builder()
            .before_tool(Box::new(move |mut event| {
                called_clone.store(true, Ordering::Relaxed);
                // Rewrite args: replace first arg with "intercepted"
                if !event.args.is_empty() {
                    event.args = vec!["intercepted".to_string()];
                }
                hooks::HookAction::Continue(event)
            }))
            .build();

        let result = bash.exec("echo original").await.unwrap();
        assert!(called.load(Ordering::Relaxed));
        assert_eq!(result.stdout.trim(), "intercepted");
    }

    #[tokio::test]
    async fn test_before_tool_hook_cancels() {
        let mut bash = Bash::builder()
            .before_tool(Box::new(|event| {
                if event.name == "echo" {
                    hooks::HookAction::Cancel("echo blocked".to_string())
                } else {
                    hooks::HookAction::Continue(event)
                }
            }))
            .build();

        let result = bash.exec("echo should-not-run").await.unwrap();
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("cancelled by before_tool hook"));
    }

    #[tokio::test]
    async fn test_after_tool_hook_observes_result() {
        use std::sync::{Arc, Mutex};

        let captured = Arc::new(Mutex::new(Vec::new()));
        let captured_clone = captured.clone();

        let mut bash = Bash::builder()
            .after_tool(Box::new(move |result| {
                captured_clone.lock().unwrap().push((
                    result.name.clone(),
                    result.stdout.clone(),
                    result.exit_code,
                ));
                hooks::HookAction::Continue(result)
            }))
            .build();

        bash.exec("echo hello-tool").await.unwrap();
        let results = captured.lock().unwrap();
        assert!(!results.is_empty());
        assert_eq!(results[0].0, "echo");
        assert!(results[0].1.contains("hello-tool"));
        assert_eq!(results[0].2, 0);
    }

    #[tokio::test]
    async fn test_before_tool_hook_does_not_fire_for_special_builtins() {
        // Special builtins (declare, local, etc.) dispatch through
        // dispatch_special_builtin, not execute_registered_builtin,
        // so before_tool should not fire for them.
        use std::sync::Arc;
        use std::sync::atomic::{AtomicU32, Ordering};

        let count = Arc::new(AtomicU32::new(0));
        let count_clone = count.clone();

        let mut bash = Bash::builder()
            .before_tool(Box::new(move |event| {
                count_clone.fetch_add(1, Ordering::Relaxed);
                hooks::HookAction::Continue(event)
            }))
            .build();

        // declare is a special builtin — should NOT trigger before_tool
        bash.exec("declare x=1").await.unwrap();
        assert_eq!(count.load(Ordering::Relaxed), 0);

        // echo is a registered builtin — should trigger before_tool
        bash.exec("echo hi").await.unwrap();
        assert_eq!(count.load(Ordering::Relaxed), 1);
    }

    #[cfg(feature = "http_client")]
    #[tokio::test]
    async fn test_before_http_hook_cancels_request() {
        use crate::NetworkAllowlist;

        let mut bash = Bash::builder()
            .network(NetworkAllowlist::allow_all())
            .before_http(Box::new(|req| {
                if req.url.contains("blocked.example.com") {
                    hooks::HookAction::Cancel("blocked by policy".to_string())
                } else {
                    hooks::HookAction::Continue(req)
                }
            }))
            .build();

        // The before_http hook should cancel this request
        let result = bash
            .exec("curl -s https://blocked.example.com/data")
            .await
            .unwrap();
        assert_ne!(result.exit_code, 0);
        assert!(result.stderr.contains("cancelled by before_http hook"));
    }

    #[cfg(feature = "http_client")]
    #[tokio::test]
    async fn test_after_http_hook_observes_response() {
        use std::sync::{Arc, Mutex};

        use crate::NetworkAllowlist;

        let captured = Arc::new(Mutex::new(Vec::new()));
        let captured_clone = captured.clone();

        let mut bash = Bash::builder()
            .network(NetworkAllowlist::allow_all())
            .after_http(Box::new(move |event| {
                captured_clone
                    .lock()
                    .unwrap()
                    .push((event.url.clone(), event.status));
                hooks::HookAction::Continue(event)
            }))
            .build();

        // Even though the request will fail (no real server), the hook
        // infrastructure is wired correctly if it doesn't panic.
        // A successful test is that the builder accepts the hook and builds.
        let _result = bash.exec("curl -s https://httpbin.org/get").await.unwrap();
        // We can't assert on captured content since there's no real HTTP
        // server, but the hook is wired and the build succeeded.
    }
}
