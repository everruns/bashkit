//! BashKit - Sandboxed bash interpreter for multi-tenant environments
//!
//! Part of the Everruns ecosystem.
//!
//! # Features
//!
//! - **Sandboxed execution**: In-memory virtual filesystem, no host access
//! - **Async-first**: Built on tokio for efficient async execution
//! - **Multi-tenant safe**: Isolated state per Bash instance
//! - **Custom builtins**: Extend with domain-specific commands (psql, kubectl, etc.)
//! - **Resource limits**: Configurable limits on commands, loops, and recursion
//!
//! # Example
//!
//! ```rust
//! use bashkit::Bash;
//!
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     let mut bash = Bash::new();
//!     let result = bash.exec("echo hello").await?;
//!     assert_eq!(result.stdout, "hello\n");
//!     assert_eq!(result.exit_code, 0);
//!     Ok(())
//! }
//! ```
//!
//! # Custom Builtins
//!
//! Register custom commands to extend BashKit with domain-specific functionality:
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
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     let mut bash = Bash::builder()
//!         .builtin("greet", Box::new(Greet))
//!         .build();
//!
//!     let result = bash.exec("greet Alice").await?;
//!     assert_eq!(result.stdout, "Hello, Alice!\n");
//!     Ok(())
//! }
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
pub use builtins::{Builtin, Context as BuiltinContext};
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

/// Main entry point for BashKit.
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
        // Check input size before parsing (V1 mitigation)
        let input_len = script.len();
        if input_len > self.max_input_bytes {
            return Err(Error::ResourceLimit(LimitExceeded::InputTooLarge(
                input_len,
                self.max_input_bytes,
            )));
        }

        let parser_timeout = self.parser_timeout;
        let max_ast_depth = self.max_ast_depth;
        let max_parser_operations = self.max_parser_operations;
        let script_owned = script.to_owned();

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
            Ok(Ok(result)) => result?,
            Ok(Err(join_error)) => {
                return Err(Error::Parse(format!("parser task failed: {}", join_error)))
            }
            Err(_elapsed) => {
                return Err(Error::ResourceLimit(LimitExceeded::ParserTimeout(
                    parser_timeout,
                )))
            }
        };

        self.interpreter.execute(&ast).await
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
#[derive(Default)]
pub struct BashBuilder {
    fs: Option<Arc<dyn FileSystem>>,
    env: HashMap<String, String>,
    cwd: Option<PathBuf>,
    limits: ExecutionLimits,
    username: Option<String>,
    hostname: Option<String>,
    custom_builtins: HashMap<String, Box<dyn Builtin>>,
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

    /// Build the Bash instance.
    pub fn build(self) -> Bash {
        let fs = self.fs.unwrap_or_else(|| Arc::new(InMemoryFs::new()));
        let mut interpreter = Interpreter::with_config(
            Arc::clone(&fs),
            self.username.clone(),
            self.hostname,
            self.custom_builtins,
        );

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

        let parser_timeout = self.limits.parser_timeout;
        let max_input_bytes = self.limits.max_input_bytes;
        let max_ast_depth = self.limits.max_ast_depth;
        let max_parser_operations = self.limits.max_parser_operations;
        interpreter.set_limits(self.limits);

        Bash {
            fs,
            interpreter,
            parser_timeout,
            max_input_bytes,
            max_ast_depth,
            max_parser_operations,
        }
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
}
