//! BashKit - Sandboxed bash interpreter for multi-tenant environments
//!
//! BashKit provides a fully sandboxed bash interpreter with a virtual filesystem,
//! making it safe to execute untrusted scripts in multi-tenant environments like
//! AI agents, CI/CD pipelines, and code sandboxes.
//!
//! # Features
//!
//! - **Virtual Filesystem**: All file operations happen in memory by default
//! - **Resource Limits**: Control command count, loop iterations, and function depth
//! - **Sandboxed Identity**: Customizable username/hostname for `whoami`, `hostname`, etc.
//! - **30+ Built-in Commands**: `echo`, `cat`, `grep`, `sed`, `awk`, `jq`, and more
//! - **Full Bash Syntax**: Variables, pipelines, redirects, loops, functions, arrays
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
//! # Virtual Filesystem
//!
//! BashKit provides three filesystem implementations:
//!
//! - [`InMemoryFs`]: Simple in-memory filesystem (default)
//! - [`OverlayFs`]: Copy-on-write overlay for layered storage
//! - [`MountableFs`]: Mount multiple filesystems at different paths
//!
//! See the [`fs`] module documentation for details and examples.
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
//! # Examples
//!
//! See the `examples/` directory for complete working examples:
//!
//! - `basic.rs` - Getting started with BashKit
//! - `custom_fs.rs` - Using different filesystem implementations
//! - `custom_filesystem_impl.rs` - Implementing the [`FileSystem`] trait
//! - `resource_limits.rs` - Setting execution limits
//! - `sandbox_identity.rs` - Customizing username/hostname
//! - `text_processing.rs` - Using grep, sed, awk, and jq
//! - `agent_tool.rs` - LLM agent integration

// Stricter panic prevention - prefer proper error handling over unwrap()
#![warn(clippy::unwrap_used)]

mod builtins;
mod error;
mod fs;
mod interpreter;
mod limits;
mod network;
/// Parser module - exposed for fuzzing and testing
pub mod parser;

pub use async_trait::async_trait;
pub use error::{Error, Result};
pub use fs::{DirEntry, FileSystem, FileType, InMemoryFs, Metadata, MountableFs, OverlayFs};
pub use interpreter::{ControlFlow, ExecResult};
pub use limits::{ExecutionCounters, ExecutionLimits, LimitExceeded};
pub use network::NetworkAllowlist;

#[cfg(feature = "network")]
pub use network::HttpClient;

use interpreter::Interpreter;
use parser::Parser;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

/// Main entry point for BashKit - a sandboxed bash interpreter.
///
/// `Bash` provides a sandboxed bash execution environment with a virtual filesystem.
/// Each instance is completely isolated and safe for multi-tenant use.
///
/// # Creating Instances
///
/// Use [`Bash::new()`] for quick setup with defaults, or [`Bash::builder()`]
/// for customized configuration:
///
/// ```rust
/// use bashkit::Bash;
///
/// // Quick setup with defaults
/// let mut bash = Bash::new();
///
/// // Or use builder for customization
/// let mut bash = Bash::builder()
///     .env("MY_VAR", "value")
///     .username("alice")
///     .build();
/// ```
///
/// # Executing Scripts
///
/// Use [`Bash::exec()`] to execute bash scripts:
///
/// ```rust
/// use bashkit::Bash;
///
/// # #[tokio::main]
/// # async fn main() -> bashkit::Result<()> {
/// let mut bash = Bash::new();
///
/// // Simple command
/// let result = bash.exec("echo hello").await?;
/// assert_eq!(result.stdout, "hello\n");
///
/// // Multi-line script
/// let script = r#"
///     NAME="World"
///     echo "Hello, $NAME!"
/// "#;
/// let result = bash.exec(script).await?;
/// assert_eq!(result.stdout, "Hello, World!\n");
/// # Ok(())
/// # }
/// ```
///
/// # State Persistence
///
/// Variables, functions, and files persist across multiple [`exec()`](Bash::exec) calls
/// on the same instance:
///
/// ```rust
/// use bashkit::Bash;
///
/// # #[tokio::main]
/// # async fn main() -> bashkit::Result<()> {
/// let mut bash = Bash::new();
///
/// // Set a variable
/// bash.exec("COUNTER=1").await?;
///
/// // Use it in a later command
/// let result = bash.exec("echo $COUNTER").await?;
/// assert_eq!(result.stdout, "1\n");
///
/// // Define a function
/// bash.exec("greet() { echo \"Hello, $1!\"; }").await?;
///
/// // Call it later
/// let result = bash.exec("greet World").await?;
/// assert_eq!(result.stdout, "Hello, World!\n");
/// # Ok(())
/// # }
/// ```
///
/// # Filesystem Access
///
/// Access the virtual filesystem directly via [`Bash::fs()`] to pre-populate
/// files or read script output:
///
/// ```rust
/// use bashkit::{Bash, FileSystem};
/// use std::path::Path;
///
/// # #[tokio::main]
/// # async fn main() -> bashkit::Result<()> {
/// let mut bash = Bash::new();
/// let fs = bash.fs();
///
/// // Pre-populate a config file
/// fs.mkdir(Path::new("/config"), false).await?;
/// fs.write_file(Path::new("/config/app.conf"), b"port=8080").await?;
///
/// // Script can read it
/// let result = bash.exec("cat /config/app.conf").await?;
/// assert_eq!(result.stdout, "port=8080");
/// # Ok(())
/// # }
/// ```
///
/// # Built-in Commands
///
/// BashKit includes 30+ built-in commands:
///
/// - **I/O**: `echo`, `printf`, `cat`, `read`
/// - **Text processing**: `grep`, `sed`, `awk`, `jq`, `head`, `tail`, `wc`, `sort`, `uniq`, `cut`, `tr`
/// - **File operations**: `mkdir`, `rm`, `cp`, `mv`, `touch`, `chmod`, `ln`
/// - **Navigation**: `cd`, `pwd`
/// - **Testing**: `test`, `[`, `true`, `false`
/// - **Variables**: `export`, `unset`, `local`, `set`
/// - **Identity**: `whoami`, `id`, `hostname`, `uname`
/// - **Misc**: `sleep`, `date`, `basename`, `dirname`, `seq`, `env`
pub struct Bash {
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
    ///
    /// This creates a Bash interpreter with:
    /// - An [`InMemoryFs`] virtual filesystem
    /// - Default username "sandbox" and hostname "bashkit-sandbox"
    /// - Default execution limits (10,000 commands, 10,000 loop iterations, 100 function depth)
    /// - Working directory `/home/user`
    ///
    /// For customization, use [`Bash::builder()`] instead.
    ///
    /// # Example
    ///
    /// ```rust
    /// use bashkit::Bash;
    ///
    /// # #[tokio::main]
    /// # async fn main() -> bashkit::Result<()> {
    /// let mut bash = Bash::new();
    /// let result = bash.exec("echo hello").await?;
    /// assert_eq!(result.stdout, "hello\n");
    /// # Ok(())
    /// # }
    /// ```
    pub fn new() -> Self {
        let fs: Arc<dyn FileSystem> = Arc::new(InMemoryFs::new());
        let interpreter = Interpreter::new(Arc::clone(&fs));
        Self { fs, interpreter }
    }

    /// Create a new [`BashBuilder`] for customized configuration.
    ///
    /// The builder allows you to configure:
    /// - Custom filesystem (e.g., [`OverlayFs`], [`MountableFs`])
    /// - Environment variables
    /// - Working directory
    /// - Execution limits
    /// - Sandbox identity (username/hostname)
    ///
    /// # Example
    ///
    /// ```rust
    /// use bashkit::{Bash, ExecutionLimits, InMemoryFs};
    /// use std::sync::Arc;
    ///
    /// # #[tokio::main]
    /// # async fn main() -> bashkit::Result<()> {
    /// let mut bash = Bash::builder()
    ///     .fs(Arc::new(InMemoryFs::new()))
    ///     .env("HOME", "/home/alice")
    ///     .cwd("/home/alice")
    ///     .username("alice")
    ///     .hostname("my-server")
    ///     .limits(ExecutionLimits::new().max_commands(1000))
    ///     .build();
    ///
    /// let result = bash.exec("whoami").await?;
    /// assert_eq!(result.stdout, "alice\n");
    /// # Ok(())
    /// # }
    /// ```
    pub fn builder() -> BashBuilder {
        BashBuilder::default()
    }

    /// Execute a bash script and return the result.
    ///
    /// Parses and executes the given script, returning an [`ExecResult`] containing
    /// stdout, stderr, and the exit code.
    ///
    /// State (variables, functions, files) persists across multiple calls on the
    /// same `Bash` instance.
    ///
    /// # Arguments
    ///
    /// * `script` - A bash script string to execute
    ///
    /// # Returns
    ///
    /// * `Ok(ExecResult)` - Execution completed (check `exit_code` for success)
    /// * `Err(Error)` - Parse error, resource limit exceeded, or I/O error
    ///
    /// # Example
    ///
    /// ```rust
    /// use bashkit::Bash;
    ///
    /// # #[tokio::main]
    /// # async fn main() -> bashkit::Result<()> {
    /// let mut bash = Bash::new();
    ///
    /// // Successful command
    /// let result = bash.exec("echo hello").await?;
    /// assert_eq!(result.exit_code, 0);
    /// assert_eq!(result.stdout, "hello\n");
    ///
    /// // Failed command (non-existent file)
    /// let result = bash.exec("cat /nonexistent").await?;
    /// assert_ne!(result.exit_code, 0);
    /// assert!(!result.stderr.is_empty());
    ///
    /// // Multi-line script
    /// let script = r#"
    ///     for i in 1 2 3; do
    ///         echo "Number: $i"
    ///     done
    /// "#;
    /// let result = bash.exec(script).await?;
    /// assert!(result.stdout.contains("Number: 1"));
    /// # Ok(())
    /// # }
    /// ```
    pub async fn exec(&mut self, script: &str) -> Result<ExecResult> {
        let parser = Parser::new(script);
        let ast = parser.parse()?;
        self.interpreter.execute(&ast).await
    }

    /// Get a reference to the underlying virtual filesystem.
    ///
    /// Returns an `Arc<dyn FileSystem>` that provides direct access to the
    /// virtual filesystem. This is useful for:
    ///
    /// - **Pre-populating files** before script execution
    /// - **Reading binary outputs** after execution (bash commands only output text)
    /// - **Injecting test data** or configuration
    /// - **Checking file metadata** (size, permissions, etc.)
    ///
    /// The returned `Arc` can be cloned and used independently.
    ///
    /// # Example
    ///
    /// ```rust
    /// use bashkit::{Bash, FileSystem};
    /// use std::path::Path;
    ///
    /// # #[tokio::main]
    /// # async fn main() -> bashkit::Result<()> {
    /// let mut bash = Bash::new();
    /// let fs = bash.fs();
    ///
    /// // Pre-populate config file
    /// fs.mkdir(Path::new("/config"), false).await?;
    /// fs.write_file(Path::new("/config/app.conf"), b"debug=true\nport=8080").await?;
    ///
    /// // Bash script can read pre-populated files
    /// let result = bash.exec("cat /config/app.conf").await?;
    /// assert!(result.stdout.contains("debug=true"));
    ///
    /// // Create output from bash
    /// bash.exec("echo 'processed' > /output.txt").await?;
    ///
    /// // Read output directly (useful for binary data)
    /// let output = fs.read_file(Path::new("/output.txt")).await?;
    /// assert_eq!(output, b"processed\n");
    ///
    /// // Check file metadata
    /// let stat = fs.stat(Path::new("/output.txt")).await?;
    /// assert_eq!(stat.size, 10); // "processed\n" = 10 bytes
    /// # Ok(())
    /// # }
    /// ```
    pub fn fs(&self) -> Arc<dyn FileSystem> {
        Arc::clone(&self.fs)
    }
}

/// Builder for customized [`Bash`] configuration.
///
/// Use [`Bash::builder()`] to create a `BashBuilder`, then chain configuration
/// methods before calling [`build()`](BashBuilder::build).
///
/// # Example
///
/// ```rust
/// use bashkit::{Bash, ExecutionLimits, InMemoryFs, OverlayFs};
/// use std::sync::Arc;
///
/// # #[tokio::main]
/// # async fn main() -> bashkit::Result<()> {
/// // Basic configuration
/// let mut bash = Bash::builder()
///     .env("HOME", "/home/alice")
///     .env("PATH", "/usr/bin:/bin")
///     .username("alice")
///     .hostname("dev-server")
///     .build();
///
/// let result = bash.exec("echo $HOME").await?;
/// assert_eq!(result.stdout, "/home/alice\n");
/// # Ok(())
/// # }
/// ```
///
/// # Available Configuration
///
/// | Method | Description |
/// |--------|-------------|
/// | [`fs()`](BashBuilder::fs) | Custom filesystem implementation |
/// | [`env()`](BashBuilder::env) | Environment variable |
/// | [`cwd()`](BashBuilder::cwd) | Current working directory |
/// | [`limits()`](BashBuilder::limits) | Execution resource limits |
/// | [`username()`](BashBuilder::username) | Sandbox username (for `whoami`, `id`) |
/// | [`hostname()`](BashBuilder::hostname) | Sandbox hostname (for `hostname`, `uname -n`) |
#[derive(Default)]
pub struct BashBuilder {
    fs: Option<Arc<dyn FileSystem>>,
    env: HashMap<String, String>,
    cwd: Option<PathBuf>,
    limits: ExecutionLimits,
    username: Option<String>,
    hostname: Option<String>,
}

impl BashBuilder {
    /// Set a custom filesystem.
    ///
    /// By default, [`Bash`] uses [`InMemoryFs`]. Use this method to provide
    /// a different filesystem implementation like [`OverlayFs`] or [`MountableFs`].
    ///
    /// # Example
    ///
    /// ```rust
    /// use bashkit::{Bash, InMemoryFs, OverlayFs};
    /// use std::sync::Arc;
    ///
    /// # #[tokio::main]
    /// # async fn main() -> bashkit::Result<()> {
    /// // Use OverlayFs with a base filesystem
    /// let base = Arc::new(InMemoryFs::new());
    /// let overlay = Arc::new(OverlayFs::new(base));
    ///
    /// let mut bash = Bash::builder()
    ///     .fs(overlay)
    ///     .build();
    /// # Ok(())
    /// # }
    /// ```
    pub fn fs(mut self, fs: Arc<dyn FileSystem>) -> Self {
        self.fs = Some(fs);
        self
    }

    /// Set an environment variable.
    ///
    /// Can be called multiple times to set multiple variables.
    /// These variables will be available to bash scripts via `$NAME` syntax.
    ///
    /// # Example
    ///
    /// ```rust
    /// use bashkit::Bash;
    ///
    /// # #[tokio::main]
    /// # async fn main() -> bashkit::Result<()> {
    /// let mut bash = Bash::builder()
    ///     .env("API_KEY", "secret123")
    ///     .env("DEBUG", "true")
    ///     .build();
    ///
    /// let result = bash.exec("echo $API_KEY").await?;
    /// assert_eq!(result.stdout, "secret123\n");
    /// # Ok(())
    /// # }
    /// ```
    pub fn env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.insert(key.into(), value.into());
        self
    }

    /// Set the current working directory.
    ///
    /// The default working directory is `/home/user`. This sets the initial
    /// value of `$PWD` and affects relative path resolution.
    ///
    /// # Example
    ///
    /// ```rust
    /// use bashkit::Bash;
    ///
    /// # #[tokio::main]
    /// # async fn main() -> bashkit::Result<()> {
    /// let mut bash = Bash::builder()
    ///     .cwd("/tmp")
    ///     .build();
    ///
    /// let result = bash.exec("pwd").await?;
    /// assert_eq!(result.stdout, "/tmp\n");
    /// # Ok(())
    /// # }
    /// ```
    pub fn cwd(mut self, cwd: impl Into<PathBuf>) -> Self {
        self.cwd = Some(cwd.into());
        self
    }

    /// Set execution resource limits.
    ///
    /// Controls resource usage to prevent runaway scripts:
    ///
    /// - `max_commands` - Maximum number of commands to execute (default: 10,000)
    /// - `max_loop_iterations` - Maximum loop iterations (default: 10,000)
    /// - `max_function_depth` - Maximum function recursion depth (default: 100)
    /// - `timeout` - Maximum execution time (default: 30 seconds)
    ///
    /// # Example
    ///
    /// ```rust
    /// use bashkit::{Bash, ExecutionLimits};
    /// use std::time::Duration;
    ///
    /// # #[tokio::main]
    /// # async fn main() -> bashkit::Result<()> {
    /// let limits = ExecutionLimits::new()
    ///     .max_commands(100)
    ///     .max_loop_iterations(50)
    ///     .max_function_depth(5)
    ///     .timeout(Duration::from_secs(5));
    ///
    /// let mut bash = Bash::builder()
    ///     .limits(limits)
    ///     .build();
    ///
    /// // This will fail with resource limit exceeded
    /// let result = bash.exec("for i in $(seq 1 100); do echo $i; done").await;
    /// assert!(result.is_err());
    /// # Ok(())
    /// # }
    /// ```
    pub fn limits(mut self, limits: ExecutionLimits) -> Self {
        self.limits = limits;
        self
    }

    /// Set the sandbox username.
    ///
    /// Configures the username returned by `whoami` and `id` commands.
    /// Also automatically sets the `USER` environment variable.
    ///
    /// Default: "sandbox"
    ///
    /// # Example
    ///
    /// ```rust
    /// use bashkit::Bash;
    ///
    /// # #[tokio::main]
    /// # async fn main() -> bashkit::Result<()> {
    /// let mut bash = Bash::builder()
    ///     .username("alice")
    ///     .build();
    ///
    /// let result = bash.exec("whoami").await?;
    /// assert_eq!(result.stdout, "alice\n");
    ///
    /// let result = bash.exec("echo $USER").await?;
    /// assert_eq!(result.stdout, "alice\n");
    ///
    /// let result = bash.exec("id").await?;
    /// assert!(result.stdout.contains("alice"));
    /// # Ok(())
    /// # }
    /// ```
    pub fn username(mut self, username: impl Into<String>) -> Self {
        self.username = Some(username.into());
        self
    }

    /// Set the sandbox hostname.
    ///
    /// Configures the hostname returned by `hostname` and `uname -n` commands.
    ///
    /// Default: "bashkit-sandbox"
    ///
    /// # Example
    ///
    /// ```rust
    /// use bashkit::Bash;
    ///
    /// # #[tokio::main]
    /// # async fn main() -> bashkit::Result<()> {
    /// let mut bash = Bash::builder()
    ///     .hostname("prod-server-01")
    ///     .build();
    ///
    /// let result = bash.exec("hostname").await?;
    /// assert_eq!(result.stdout, "prod-server-01\n");
    ///
    /// let result = bash.exec("uname -n").await?;
    /// assert_eq!(result.stdout, "prod-server-01\n");
    /// # Ok(())
    /// # }
    /// ```
    pub fn hostname(mut self, hostname: impl Into<String>) -> Self {
        self.hostname = Some(hostname.into());
        self
    }

    /// Build the configured [`Bash`] instance.
    ///
    /// Consumes the builder and returns a ready-to-use `Bash` instance.
    ///
    /// # Example
    ///
    /// ```rust
    /// use bashkit::Bash;
    ///
    /// let mut bash = Bash::builder()
    ///     .env("GREETING", "Hello")
    ///     .username("user")
    ///     .build();
    /// ```
    pub fn build(self) -> Bash {
        let fs = self.fs.unwrap_or_else(|| Arc::new(InMemoryFs::new()));
        let mut interpreter =
            Interpreter::with_config(Arc::clone(&fs), self.username.clone(), self.hostname);

        // Set environment variables
        for (key, value) in self.env {
            interpreter.set_env(&key, &value);
        }

        // If username is set, automatically set USER env var
        if let Some(ref username) = self.username {
            interpreter.set_env("USER", username);
        }

        if let Some(cwd) = self.cwd {
            interpreter.set_cwd(cwd);
        }

        interpreter.set_limits(self.limits);

        Bash { fs, interpreter }
    }
}

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
        // This specific case causes a parsing issue - "done" after for loop
        // TODO: Fix the parser to handle "done" as a regular word after for loop ends
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
}
