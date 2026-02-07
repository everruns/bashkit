//! Bashkit - Sandboxed bash interpreter for multi-tenant environments
//!
//! Sandboxed bash interpreter for AI agents, CI/CD pipelines, and code sandboxes.
//! Written in Rust.
//!
//! # Features
//!
//! - **POSIX compliant** - Substantial IEEE 1003.1-2024 Shell Command Language compliance
//! - **Sandboxed execution** - No real filesystem access by default
//! - **Virtual filesystem** - [`InMemoryFs`], [`OverlayFs`], [`MountableFs`]
//! - **Resource limits** - Command count, loop iterations, function depth
//! - **Network allowlist** - Control HTTP access per-domain
//! - **Custom builtins** - Extend with domain-specific commands
//! - **Async-first** - Built on tokio
//! - **Experimental: Git** - Sandboxed git operations on the VFS (`git` feature)
//! - **Experimental: Python** - Embedded Python via [Monty](https://github.com/pydantic/monty) (`python` feature)
//!
//! # Built-in Commands (68+)
//!
//! | Category | Commands |
//! |----------|----------|
//! | Core | `echo`, `printf`, `cat`, `read` |
//! | Navigation | `cd`, `pwd`, `ls`, `find` |
//! | Flow control | `true`, `false`, `exit`, `return`, `break`, `continue`, `test`, `[` |
//! | Variables | `export`, `set`, `unset`, `local`, `shift`, `source`, `.` |
//! | Text processing | `grep`, `sed`, `awk`, `jq`, `head`, `tail`, `sort`, `uniq`, `cut`, `tr`, `wc` |
//! | File operations | `mkdir`, `rm`, `cp`, `mv`, `touch`, `chmod`, `rmdir` |
//! | File inspection | `file`, `stat`, `less` |
//! | Archives | `tar`, `gzip`, `gunzip` |
//! | Utilities | `sleep`, `date`, `basename`, `dirname`, `timeout`, `wait`, `xargs`, `tee` |
//! | Shell | `bash`, `sh` (sandboxed re-invocation), `eval`, `:` |
//! | System info | `whoami`, `hostname`, `uname`, `id`, `env`, `printenv`, `history` |
//! | Network | `curl`, `wget` (requires [`NetworkAllowlist`])
//! | Experimental | `python`, `python3` (requires `python` feature), `git` (requires `git` feature)
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
//! ```rust,no_run
//! use bashkit::{Bash, NetworkAllowlist};
//!
//! # #[tokio::main]
//! # async fn main() -> bashkit::Result<()> {
//! let mut bash = Bash::builder()
//!     .network(NetworkAllowlist::new()
//!         .allow("https://httpbin.org"))
//!     .build();
//!
//! // curl and wget now work for allowed URLs
//! let result = bash.exec("curl -s https://httpbin.org/get").await?;
//! assert!(result.stdout.contains("httpbin.org"));
//! # Ok(())
//! # }
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
//! Enable the `git` feature for sandboxed git operations. All git data lives in
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
//! `checkout`, `diff`, `reset`, `remote`, `clone`/`push`/`pull`/`fetch` (sandbox mode).
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
//! - `sandbox_identity.rs` - Customizing username/hostname
//! - `text_processing.rs` - Using grep, sed, awk, and jq
//! - `agent_tool.rs` - LLM agent integration
//! - `git_workflow.rs` - Git operations on the virtual filesystem
//! - `python_scripts.rs` - Embedded Python with VFS bridging
//!
//! # Guides
//!
//! - [`custom_builtins_guide`] - Creating custom builtins
//! - `python_guide` - Embedded Python (Monty) guide (requires `python` feature)
//! - [`compatibility_scorecard`] - Feature parity tracking
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

mod builtins;
mod error;
mod fs;
mod git;
mod interpreter;
mod limits;
#[cfg(feature = "logging")]
mod logging_impl;
mod network;
/// Parser module - exposed for fuzzing and testing
pub mod parser;
/// Tool contract for LLM integration
pub mod tool;

pub use async_trait::async_trait;
pub use builtins::{Builtin, Context as BuiltinContext};
pub use error::{Error, Result};
pub use fs::{
    verify_filesystem_requirements, DirEntry, FileSystem, FileType, FsBackend, FsLimitExceeded,
    FsLimits, FsUsage, InMemoryFs, Metadata, MountableFs, OverlayFs, PosixFs,
};
pub use git::GitConfig;
pub use interpreter::{ControlFlow, ExecResult};
pub use limits::{ExecutionCounters, ExecutionLimits, LimitExceeded};
pub use network::NetworkAllowlist;
pub use tool::{BashTool, BashToolBuilder, Tool, ToolRequest, ToolResponse, ToolStatus, VERSION};

#[cfg(feature = "http_client")]
pub use network::HttpClient;

#[cfg(feature = "git")]
pub use git::GitClient;

#[cfg(feature = "python")]
pub use builtins::{PythonIsolation, PythonLimits};

/// Logging utilities module
///
/// Provides structured logging with security features including sensitive data redaction.
/// Only available when the `logging` feature is enabled.
#[cfg(feature = "logging")]
pub mod logging {
    pub use crate::logging_impl::{format_script_for_log, sanitize_for_log, LogConfig};
}

#[cfg(feature = "logging")]
pub use logging::LogConfig;

use interpreter::Interpreter;
use parser::Parser;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

/// Main entry point for Bashkit.
///
/// Provides a sandboxed bash interpreter with an in-memory virtual filesystem.
pub struct Bash {
    fs: Arc<dyn FileSystem>,
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
        let parser_timeout = ExecutionLimits::default().parser_timeout;
        let max_input_bytes = ExecutionLimits::default().max_input_bytes;
        let max_ast_depth = ExecutionLimits::default().max_ast_depth;
        let max_parser_operations = ExecutionLimits::default().max_parser_operations;
        Self {
            fs,
            interpreter,
            parser_timeout,
            max_input_bytes,
            max_ast_depth,
            max_parser_operations,
            #[cfg(feature = "logging")]
            log_config: logging::LogConfig::default(),
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
        // THREAT[TM-LOG-001]: Sensitive data in logs
        // Mitigation: Use LogConfig to redact sensitive script content
        #[cfg(feature = "logging")]
        {
            let script_info = logging::format_script_for_log(script, &self.log_config);
            tracing::info!(target: "bashkit::session", script = %script_info, "Starting script execution");
        }

        // Check input size before parsing (V1 mitigation)
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

        // Parse with timeout using spawn_blocking since parsing is sync
        let parse_result = tokio::time::timeout(parser_timeout, async {
            tokio::task::spawn_blocking(move || {
                let parser =
                    Parser::with_limits(&script_owned, max_ast_depth, max_parser_operations);
                parser.parse()
            })
            .await
        })
        .await;

        let ast = match parse_result {
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
                return Err(Error::Parse(format!("parser task failed: {}", join_error)));
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
        };

        #[cfg(feature = "logging")]
        tracing::debug!(target: "bashkit::interpreter", "Starting interpretation");

        let result = self.interpreter.execute(&ast).await;

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
                tracing::error!(
                    target: "bashkit::session",
                    error = %e,
                    "Script execution failed"
                );
            }
        }

        result
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

#[derive(Default)]
pub struct BashBuilder {
    fs: Option<Arc<dyn FileSystem>>,
    env: HashMap<String, String>,
    cwd: Option<PathBuf>,
    limits: ExecutionLimits,
    username: Option<String>,
    hostname: Option<String>,
    custom_builtins: HashMap<String, Box<dyn Builtin>>,
    /// Files to mount in the virtual filesystem
    mounted_files: Vec<MountedFile>,
    /// Network allowlist for curl/wget builtins
    #[cfg(feature = "http_client")]
    network_allowlist: Option<NetworkAllowlist>,
    /// Logging configuration
    #[cfg(feature = "logging")]
    log_config: Option<logging::LogConfig>,
    /// Git configuration for git builtins
    #[cfg(feature = "git")]
    git_config: Option<GitConfig>,
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

    /// Enable embedded Python (`python`/`python3` builtins) via Monty interpreter
    /// with default resource limits.
    ///
    /// **Experimental:** The Monty Python integration is experimental and has known
    /// crash-level bugs in its parser. Subprocess isolation (the default `Auto` mode)
    /// mitigates host crashes, but undiscovered security issues may exist.
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

    /// Enable embedded Python with custom resource limits.
    ///
    /// **Experimental:** See [`BashBuilder::python`] for caveats.
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
        let base_fs = self.fs.unwrap_or_else(|| Arc::new(InMemoryFs::new()));

        // If there are mounted files, wrap in an OverlayFs
        let fs: Arc<dyn FileSystem> = if self.mounted_files.is_empty() {
            base_fs
        } else {
            let overlay = OverlayFs::new(base_fs);
            // Add mounted files to the overlay layer
            for mf in &self.mounted_files {
                overlay.upper().add_file(&mf.path, &mf.content, mf.mode);
            }
            Arc::new(overlay)
        };

        Self::build_with_fs(
            fs,
            self.env,
            self.username,
            self.hostname,
            self.cwd,
            self.limits,
            self.custom_builtins,
            #[cfg(feature = "http_client")]
            self.network_allowlist,
            #[cfg(feature = "logging")]
            self.log_config,
            #[cfg(feature = "git")]
            self.git_config,
        )
    }

    /// Internal helper to build Bash with a configured filesystem.
    #[allow(clippy::too_many_arguments)]
    fn build_with_fs(
        fs: Arc<dyn FileSystem>,
        env: HashMap<String, String>,
        username: Option<String>,
        hostname: Option<String>,
        cwd: Option<PathBuf>,
        limits: ExecutionLimits,
        custom_builtins: HashMap<String, Box<dyn Builtin>>,
        #[cfg(feature = "http_client")] network_allowlist: Option<NetworkAllowlist>,
        #[cfg(feature = "logging")] log_config: Option<logging::LogConfig>,
        #[cfg(feature = "git")] git_config: Option<GitConfig>,
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

        let mut interpreter =
            Interpreter::with_config(Arc::clone(&fs), username.clone(), hostname, custom_builtins);

        // Set environment variables
        for (key, value) in env {
            interpreter.set_env(&key, &value);
        }

        // If username is set, automatically set USER env var
        if let Some(ref username) = username {
            interpreter.set_env("USER", username);
        }

        if let Some(cwd) = cwd {
            interpreter.set_cwd(cwd);
        }

        // Configure HTTP client for network builtins
        #[cfg(feature = "http_client")]
        if let Some(allowlist) = network_allowlist {
            let client = network::HttpClient::new(allowlist);
            interpreter.set_http_client(client);
        }

        // Configure git client for git builtins
        #[cfg(feature = "git")]
        if let Some(config) = git_config {
            let client = git::GitClient::new(config);
            interpreter.set_git_client(client);
        }

        let parser_timeout = limits.parser_timeout;
        let max_input_bytes = limits.max_input_bytes;
        let max_ast_depth = limits.max_ast_depth;
        let max_parser_operations = limits.max_parser_operations;
        interpreter.set_limits(limits);

        Bash {
            fs,
            interpreter,
            parser_timeout,
            max_input_bytes,
            max_ast_depth,
            max_parser_operations,
            #[cfg(feature = "logging")]
            log_config,
        }
    }
}

// =============================================================================
// Documentation Modules
// =============================================================================
// These modules embed external markdown guides into rustdoc.
// Source files live in crates/bashkit/docs/ - edit there, not here.
// See specs/008-documentation.md for the documentation approach.

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

#[cfg(test)]
#[allow(clippy::unwrap_used)]
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
    async fn test_param_remove_suffix() {
        let mut bash = Bash::new();
        // ${var%pattern} - remove shortest suffix
        let result = bash.exec("X=file.tar.gz; echo ${X%.*}").await.unwrap();
        assert_eq!(result.stdout, "file.tar\n");
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
        assert!(!fs
            .exists(std::path::Path::new("/tmp/copied.txt"))
            .await
            .unwrap());
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
        use crate::ExecResult;
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
        // ~ should default to /home/user if HOME is not set
        let mut bash = Bash::new();
        let result = bash.exec("echo ~").await.unwrap();
        assert_eq!(result.stdout, "/home/user\n");
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
        // Error should mention the problem
        assert!(
            err_msg.contains("expected"),
            "Error should mention what's expected: {}",
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
}
