//! Tool trait and BashTool implementation
//!
//! # Public Library Contract
//!
//! The `Tool` trait is a **public contract** - breaking changes require a major version bump.
//! See `specs/009-tool-contract.md` for the full specification.
//!
//! # Architecture
//!
//! - [`Tool`] trait: Contract that all tools must implement
//! - [`BashTool`]: Virtual bash interpreter implementing Tool
//! - [`BashToolBuilder`]: Builder pattern for configuring BashTool
//!
//! # Example
//!
//! ```
//! use bashkit::{BashTool, Tool, ToolRequest};
//!
//! # tokio_test::block_on(async {
//! let mut tool = BashTool::default();
//!
//! // Introspection
//! assert_eq!(tool.name(), "bashkit");
//! assert!(!tool.help().is_empty());
//!
//! // Execution
//! let resp = tool.execute(ToolRequest {
//!     commands: "echo hello".to_string(),
//!     timeout_ms: None,
//! }).await;
//! assert_eq!(resp.stdout, "hello\n");
//! # });
//! ```

use crate::builtins::Builtin;
use crate::error::Error;
use crate::{Bash, ExecResult, ExecutionLimits, OutputCallback};
use async_trait::async_trait;
use schemars::{schema_for, JsonSchema};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Library version from Cargo.toml
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// List of built-in commands (organized by category)
const BUILTINS: &str = "\
echo printf cat read \
grep sed awk jq head tail sort uniq cut tr wc nl paste column comm diff strings tac rev \
cd pwd ls find mkdir mktemp rm rmdir cp mv touch chmod chown ln \
file stat less tar gzip gunzip du df \
test [ true false exit return break continue \
export set unset local shift source eval declare typeset readonly shopt getopts \
sleep date seq expr yes wait timeout xargs tee watch \
basename dirname realpath \
pushd popd dirs \
whoami hostname uname id env printenv history \
curl wget \
od xxd hexdump base64 \
kill";

/// Base help documentation template (generic help format)
const BASE_HELP: &str = r#"BASH(1)                          User Commands                         BASH(1)

NAME
       bashkit - virtual bash interpreter with virtual filesystem

SYNOPSIS
       {"commands": "<bash commands>"}

DESCRIPTION
       Bashkit executes bash commands in a virtual environment with a virtual
       filesystem. All file operations are contained within the virtual environment.

       Supports full bash syntax including variables, pipelines, redirects,
       loops, conditionals, functions, and arrays.

BUILTINS
   Core I/O:        echo, printf, cat, read
   Text Processing: grep, sed, awk, jq, head, tail, sort, uniq, cut, tr, wc,
                     nl, paste, column, comm, diff, strings, tac, rev
   File Operations: cd, pwd, ls, find, mkdir, mktemp, rm, rmdir, cp, mv,
                     touch, chmod, chown, ln
   File Inspection: file, stat, less, tar, gzip, gunzip, du, df
   Flow Control:    test, [, true, false, exit, return, break, continue
   Shell/Variables:  export, set, unset, local, shift, source, eval, declare,
                     typeset, readonly, shopt, getopts
   Utilities:       sleep, date, seq, expr, yes, wait, timeout, xargs, tee,
                     watch, basename, dirname, realpath
   Dir Stack:       pushd, popd, dirs
   System Info:     whoami, hostname, uname, id, env, printenv, history
   Network:         curl, wget
   Binary/Hex:      od, xxd, hexdump, base64
   Signals:         kill

INPUT
       commands    Bash commands to execute (like bash -c "commands")

OUTPUT
       stdout      Standard output from the commands
       stderr      Standard error from the commands
       exit_code   Exit status (0 = success)

EXAMPLES
       Simple echo:
           {"commands": "echo 'Hello, World!'"}

       Arithmetic:
           {"commands": "x=5; y=3; echo $((x + y))"}

       Pipeline:
           {"commands": "echo -e 'apple\nbanana' | grep a"}

       JSON processing:
           {"commands": "echo '{\"n\":1}' | jq '.n'"}

       File operations (virtual):
           {"commands": "echo data > /tmp/f.txt && cat /tmp/f.txt"}

       Run script from VFS:
           {"commands": "source /path/to/script.sh"}

EXIT STATUS
       0      Success
       1-125  Command-specific error
       126    Command not executable
       127    Command not found

SEE ALSO
       bash(1), sh(1)
"#;

// Note: system_prompt() is built dynamically in build_system_prompt()

/// Request to execute bash commands
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ToolRequest {
    /// Bash commands to execute (like `bash -c "commands"`)
    pub commands: String,
    /// Optional per-call timeout in milliseconds.
    /// When set, execution is aborted after this duration and a response
    /// with `exit_code = 124` is returned (matching the bash `timeout` convention).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
}

impl ToolRequest {
    /// Create a request with just commands (no timeout).
    pub fn new(commands: impl Into<String>) -> Self {
        Self {
            commands: commands.into(),
            timeout_ms: None,
        }
    }
}

/// Response from executing a bash script
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ToolResponse {
    /// Standard output from the script
    pub stdout: String,
    /// Standard error from the script
    pub stderr: String,
    /// Exit code (0 = success)
    pub exit_code: i32,
    /// Error message if execution failed before running
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl From<ExecResult> for ToolResponse {
    fn from(result: ExecResult) -> Self {
        Self {
            stdout: result.stdout,
            stderr: result.stderr,
            exit_code: result.exit_code,
            error: None,
        }
    }
}

/// Status update during tool execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolStatus {
    /// Current phase (e.g., "validate", "parse", "execute", "output", "complete")
    pub phase: String,
    /// Optional message
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// Estimated completion percentage (0-100)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub percent_complete: Option<f32>,
    /// Estimated time remaining in milliseconds
    #[serde(skip_serializing_if = "Option::is_none")]
    pub eta_ms: Option<u64>,
    /// Incremental stdout/stderr chunk (only present when `phase == "output"`)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
    /// Which stream the output belongs to: `"stdout"` or `"stderr"`
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<String>,
}

impl ToolStatus {
    /// Create a new status with phase
    pub fn new(phase: impl Into<String>) -> Self {
        Self {
            phase: phase.into(),
            message: None,
            percent_complete: None,
            eta_ms: None,
            output: None,
            stream: None,
        }
    }

    /// Create an output status carrying a stdout chunk.
    pub fn stdout(chunk: impl Into<String>) -> Self {
        Self {
            phase: "output".to_string(),
            message: None,
            percent_complete: None,
            eta_ms: None,
            output: Some(chunk.into()),
            stream: Some("stdout".to_string()),
        }
    }

    /// Create an output status carrying a stderr chunk.
    pub fn stderr(chunk: impl Into<String>) -> Self {
        Self {
            phase: "output".to_string(),
            message: None,
            percent_complete: None,
            eta_ms: None,
            output: Some(chunk.into()),
            stream: Some("stderr".to_string()),
        }
    }

    /// Set message
    pub fn with_message(mut self, message: impl Into<String>) -> Self {
        self.message = Some(message.into());
        self
    }

    /// Set completion percentage
    pub fn with_percent(mut self, percent: f32) -> Self {
        self.percent_complete = Some(percent);
        self
    }

    /// Set ETA
    pub fn with_eta(mut self, eta_ms: u64) -> Self {
        self.eta_ms = Some(eta_ms);
        self
    }
}

// ============================================================================
// Tool Trait - Public Library Contract
// ============================================================================

/// Tool contract for LLM integration.
///
/// # Public Contract
///
/// This trait is a **public library contract**. Breaking changes require a major version bump.
/// See `specs/009-tool-contract.md` for the full specification.
///
/// All tools must implement this trait to be usable by LLMs and agents.
/// The trait provides introspection (schemas, docs) and execution methods.
///
/// # Implementors
///
/// - [`BashTool`]: Virtual bash interpreter
#[async_trait]
pub trait Tool: Send + Sync {
    /// Tool identifier (e.g., "bashkit", "calculator")
    fn name(&self) -> &str;

    /// One-line description for tool listings
    fn short_description(&self) -> &str;

    /// Full description, may include dynamic config info
    fn description(&self) -> String;

    /// Full documentation for LLMs (human readable, with examples)
    fn help(&self) -> String;

    /// Condensed description for system prompts (token-efficient)
    fn system_prompt(&self) -> String;

    /// JSON Schema for input validation
    fn input_schema(&self) -> serde_json::Value;

    /// JSON Schema for output structure
    fn output_schema(&self) -> serde_json::Value;

    /// Library/tool version
    fn version(&self) -> &str;

    /// Execute the tool
    async fn execute(&mut self, req: ToolRequest) -> ToolResponse;

    /// Execute with status callbacks for progress tracking
    async fn execute_with_status(
        &mut self,
        req: ToolRequest,
        status_callback: Box<dyn FnMut(ToolStatus) + Send>,
    ) -> ToolResponse;
}

// ============================================================================
// BashTool - Implementation
// ============================================================================

/// Builder for configuring BashTool
#[derive(Default)]
pub struct BashToolBuilder {
    /// Custom username for virtual identity
    username: Option<String>,
    /// Custom hostname for virtual identity
    hostname: Option<String>,
    /// Execution limits
    limits: Option<ExecutionLimits>,
    /// Environment variables to set
    env_vars: Vec<(String, String)>,
    /// Custom builtins (name, implementation)
    builtins: Vec<(String, Box<dyn Builtin>)>,
}

impl BashToolBuilder {
    /// Create a new tool builder with defaults
    pub fn new() -> Self {
        Self::default()
    }

    /// Set custom username for virtual identity
    pub fn username(mut self, username: impl Into<String>) -> Self {
        self.username = Some(username.into());
        self
    }

    /// Set custom hostname for virtual identity
    pub fn hostname(mut self, hostname: impl Into<String>) -> Self {
        self.hostname = Some(hostname.into());
        self
    }

    /// Set execution limits
    pub fn limits(mut self, limits: ExecutionLimits) -> Self {
        self.limits = Some(limits);
        self
    }

    /// Add an environment variable
    pub fn env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env_vars.push((key.into(), value.into()));
        self
    }

    /// Register a custom builtin command
    ///
    /// Custom builtins extend the shell with domain-specific commands.
    /// They will be documented in the tool's `help()` output.
    /// If the builtin implements [`Builtin::llm_hint`], its hint will be
    /// included in `help()` and `system_prompt()`.
    pub fn builtin(mut self, name: impl Into<String>, builtin: Box<dyn Builtin>) -> Self {
        self.builtins.push((name.into(), builtin));
        self
    }

    /// Enable embedded Python (`python`/`python3` builtins) via Monty interpreter
    /// with default resource limits.
    ///
    /// Requires the `python` feature flag. Python `pathlib.Path` operations are
    /// bridged to the virtual filesystem. Limitations (no `open()`, no HTTP) are
    /// automatically documented in `help()` and `system_prompt()`.
    #[cfg(feature = "python")]
    pub fn python(self) -> Self {
        self.python_with_limits(crate::builtins::PythonLimits::default())
    }

    /// Enable embedded Python with custom resource limits.
    #[cfg(feature = "python")]
    pub fn python_with_limits(self, limits: crate::builtins::PythonLimits) -> Self {
        use crate::builtins::Python;
        self.builtin("python", Box::new(Python::with_limits(limits.clone())))
            .builtin("python3", Box::new(Python::with_limits(limits)))
    }

    /// Build the BashTool
    pub fn build(self) -> BashTool {
        let builtin_names: Vec<String> = self.builtins.iter().map(|(n, _)| n.clone()).collect();

        // Collect LLM hints from builtins, deduplicated
        let mut builtin_hints: Vec<String> = self
            .builtins
            .iter()
            .filter_map(|(_, b)| b.llm_hint().map(String::from))
            .collect();
        builtin_hints.sort();
        builtin_hints.dedup();

        BashTool {
            username: self.username,
            hostname: self.hostname,
            limits: self.limits,
            env_vars: self.env_vars,
            builtins: self.builtins,
            builtin_names,
            builtin_hints,
        }
    }
}

/// Virtual bash interpreter implementing the Tool trait
#[derive(Default)]
pub struct BashTool {
    username: Option<String>,
    hostname: Option<String>,
    limits: Option<ExecutionLimits>,
    env_vars: Vec<(String, String)>,
    builtins: Vec<(String, Box<dyn Builtin>)>,
    /// Names of custom builtins (for documentation)
    builtin_names: Vec<String>,
    /// LLM hints from registered builtins
    builtin_hints: Vec<String>,
}

impl BashTool {
    /// Create a new tool builder
    pub fn builder() -> BashToolBuilder {
        BashToolBuilder::new()
    }

    /// Create a Bash instance with configured settings
    fn create_bash(&mut self) -> Bash {
        let mut builder = Bash::builder();

        if let Some(ref username) = self.username {
            builder = builder.username(username);
        }
        if let Some(ref hostname) = self.hostname {
            builder = builder.hostname(hostname);
        }
        if let Some(ref limits) = self.limits {
            builder = builder.limits(limits.clone());
        }
        for (key, value) in &self.env_vars {
            builder = builder.env(key, value);
        }
        // Move builtins out to avoid borrow issues
        for (name, builtin) in std::mem::take(&mut self.builtins) {
            builder = builder.builtin(name, builtin);
        }

        builder.build()
    }

    /// Build dynamic description with supported tools
    fn build_description(&self) -> String {
        let mut desc =
            String::from("Virtual bash interpreter with virtual filesystem. Supported tools: ");
        desc.push_str(BUILTINS);
        if !self.builtin_names.is_empty() {
            desc.push(' ');
            desc.push_str(&self.builtin_names.join(" "));
        }
        desc
    }

    /// Build dynamic help with configuration
    fn build_help(&self) -> String {
        let mut doc = BASE_HELP.to_string();

        // Append configuration section if any dynamic config exists
        let has_config = !self.builtin_names.is_empty()
            || self.username.is_some()
            || self.hostname.is_some()
            || self.limits.is_some()
            || !self.env_vars.is_empty();

        if has_config {
            doc.push_str("\nCONFIGURATION\n");

            if !self.builtin_names.is_empty() {
                doc.push_str("       Custom commands: ");
                doc.push_str(&self.builtin_names.join(", "));
                doc.push('\n');
            }

            if let Some(ref username) = self.username {
                doc.push_str(&format!("       User: {} (whoami)\n", username));
            }
            if let Some(ref hostname) = self.hostname {
                doc.push_str(&format!("       Host: {} (hostname)\n", hostname));
            }

            if let Some(ref limits) = self.limits {
                doc.push_str(&format!(
                    "       Limits: {} commands, {} iterations, {} depth\n",
                    limits.max_commands, limits.max_loop_iterations, limits.max_function_depth
                ));
            }

            if !self.env_vars.is_empty() {
                doc.push_str("       Environment: ");
                let keys: Vec<&str> = self.env_vars.iter().map(|(k, _)| k.as_str()).collect();
                doc.push_str(&keys.join(", "));
                doc.push('\n');
            }
        }

        // Append builtin hints (capabilities/limitations for LLMs)
        if !self.builtin_hints.is_empty() {
            doc.push_str("\nNOTES\n");
            for hint in &self.builtin_hints {
                doc.push_str(&format!("       {hint}\n"));
            }
        }

        // Append language interpreter warning
        if let Some(warning) = self.language_warning() {
            doc.push_str(&format!("\nWARNINGS\n       {warning}\n"));
        }

        doc
    }

    /// Single-line warning listing language interpreters not registered as builtins.
    /// Returns `None` when all tracked languages are available.
    fn language_warning(&self) -> Option<String> {
        let mut missing = Vec::new();

        let has_perl = self.builtin_names.iter().any(|n| n == "perl");
        if !has_perl {
            missing.push("perl");
        }

        let has_python = self
            .builtin_names
            .iter()
            .any(|n| n == "python" || n == "python3");
        if !has_python {
            missing.push("python/python3");
        }

        if missing.is_empty() {
            None
        } else {
            Some(format!("{} not available.", missing.join(", ")))
        }
    }

    /// Build dynamic system prompt
    fn build_system_prompt(&self) -> String {
        let mut prompt = String::from("# Bash Tool\n\n");

        // Description with workspace info
        prompt.push_str("Virtual bash interpreter with virtual filesystem.\n");

        // Home directory info if username is set
        if let Some(ref username) = self.username {
            prompt.push_str(&format!("Home: /home/{}\n", username));
        }

        prompt.push('\n');

        // Input/Output format
        prompt.push_str("Input: {\"commands\": \"<bash commands>\"}\n");
        prompt.push_str("Output: {stdout, stderr, exit_code}\n");

        // Builtin hints (capabilities/limitations)
        if !self.builtin_hints.is_empty() {
            prompt.push('\n');
            for hint in &self.builtin_hints {
                prompt.push_str(&format!("Note: {hint}\n"));
            }
        }

        // Language interpreter warning
        if let Some(warning) = self.language_warning() {
            prompt.push_str(&format!("\nWarning: {warning}\n"));
        }

        prompt
    }
}

#[async_trait]
impl Tool for BashTool {
    fn name(&self) -> &str {
        "bashkit"
    }

    fn short_description(&self) -> &str {
        "Virtual bash interpreter with virtual filesystem"
    }

    fn description(&self) -> String {
        self.build_description()
    }

    fn help(&self) -> String {
        self.build_help()
    }

    fn system_prompt(&self) -> String {
        self.build_system_prompt()
    }

    fn input_schema(&self) -> serde_json::Value {
        let schema = schema_for!(ToolRequest);
        serde_json::to_value(schema).unwrap_or_default()
    }

    fn output_schema(&self) -> serde_json::Value {
        let schema = schema_for!(ToolResponse);
        serde_json::to_value(schema).unwrap_or_default()
    }

    fn version(&self) -> &str {
        VERSION
    }

    async fn execute(&mut self, req: ToolRequest) -> ToolResponse {
        if req.commands.is_empty() {
            return ToolResponse {
                stdout: String::new(),
                stderr: String::new(),
                exit_code: 0,
                error: None,
            };
        }

        let mut bash = self.create_bash();

        let fut = async {
            match bash.exec(&req.commands).await {
                Ok(result) => result.into(),
                Err(e) => ToolResponse {
                    stdout: String::new(),
                    stderr: e.to_string(),
                    exit_code: 1,
                    error: Some(error_kind(&e)),
                },
            }
        };

        if let Some(ms) = req.timeout_ms {
            let dur = Duration::from_millis(ms);
            match tokio::time::timeout(dur, fut).await {
                Ok(resp) => resp,
                Err(_elapsed) => timeout_response(dur),
            }
        } else {
            fut.await
        }
    }

    async fn execute_with_status(
        &mut self,
        req: ToolRequest,
        mut status_callback: Box<dyn FnMut(ToolStatus) + Send>,
    ) -> ToolResponse {
        status_callback(ToolStatus::new("validate").with_percent(0.0));

        if req.commands.is_empty() {
            status_callback(ToolStatus::new("complete").with_percent(100.0));
            return ToolResponse {
                stdout: String::new(),
                stderr: String::new(),
                exit_code: 0,
                error: None,
            };
        }

        status_callback(ToolStatus::new("parse").with_percent(10.0));

        let mut bash = self.create_bash();

        status_callback(ToolStatus::new("execute").with_percent(20.0));

        // Wire streaming: forward output chunks as ToolStatus events
        let status_cb = Arc::new(Mutex::new(status_callback));
        let status_cb_output = status_cb.clone();
        let output_cb: OutputCallback = Box::new(move |stdout_chunk, stderr_chunk| {
            if let Ok(mut cb) = status_cb_output.lock() {
                if !stdout_chunk.is_empty() {
                    cb(ToolStatus::stdout(stdout_chunk));
                }
                if !stderr_chunk.is_empty() {
                    cb(ToolStatus::stderr(stderr_chunk));
                }
            }
        });

        let timeout_ms = req.timeout_ms;

        let fut = async {
            let response = match bash.exec_streaming(&req.commands, output_cb).await {
                Ok(result) => result.into(),
                Err(e) => ToolResponse {
                    stdout: String::new(),
                    stderr: e.to_string(),
                    exit_code: 1,
                    error: Some(error_kind(&e)),
                },
            };

            if let Ok(mut cb) = status_cb.lock() {
                cb(ToolStatus::new("complete").with_percent(100.0));
            }

            response
        };

        if let Some(ms) = timeout_ms {
            let dur = Duration::from_millis(ms);
            match tokio::time::timeout(dur, fut).await {
                Ok(resp) => resp,
                Err(_elapsed) => timeout_response(dur),
            }
        } else {
            fut.await
        }
    }
}

/// Extract error kind from Error for categorization
fn error_kind(e: &Error) -> String {
    match e {
        Error::Parse(_) | Error::ParseAt { .. } => "parse_error".to_string(),
        Error::Execution(_) => "execution_error".to_string(),
        Error::Io(_) => "io_error".to_string(),
        Error::CommandNotFound(_) => "command_not_found".to_string(),
        Error::ResourceLimit(_) => "resource_limit".to_string(),
        Error::Network(_) => "network_error".to_string(),
        Error::Regex(_) => "regex_error".to_string(),
        Error::Internal(_) => "internal_error".to_string(),
    }
}

/// Build a ToolResponse for a timed-out execution (exit code 124, like bash `timeout`).
fn timeout_response(dur: Duration) -> ToolResponse {
    ToolResponse {
        stdout: String::new(),
        stderr: format!(
            "bashkit: execution timed out after {:.1}s\n",
            dur.as_secs_f64()
        ),
        exit_code: 124,
        error: Some("timeout".to_string()),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn test_bash_tool_builder() {
        let tool = BashTool::builder()
            .username("testuser")
            .hostname("testhost")
            .env("FOO", "bar")
            .limits(ExecutionLimits::new().max_commands(100))
            .build();

        assert_eq!(tool.username, Some("testuser".to_string()));
        assert_eq!(tool.hostname, Some("testhost".to_string()));
        assert_eq!(tool.env_vars, vec![("FOO".to_string(), "bar".to_string())]);
    }

    #[test]
    fn test_tool_trait_methods() {
        let tool = BashTool::default();

        // Test trait methods
        assert_eq!(tool.name(), "bashkit");
        assert_eq!(
            tool.short_description(),
            "Virtual bash interpreter with virtual filesystem"
        );
        assert!(tool.description().contains("Virtual bash interpreter"));
        assert!(tool.description().contains("Supported tools:"));
        assert!(tool.help().contains("BASH(1)"));
        assert!(tool.help().contains("SYNOPSIS"));
        assert!(tool.system_prompt().contains("# Bash Tool"));
        assert_eq!(tool.version(), VERSION);
    }

    #[test]
    fn test_tool_description_with_config() {
        let tool = BashTool::builder()
            .username("agent")
            .hostname("sandbox")
            .env("API_KEY", "secret")
            .limits(ExecutionLimits::new().max_commands(50))
            .build();

        // helptext should include configuration in man-page style
        let helptext = tool.help();
        assert!(helptext.contains("CONFIGURATION"));
        assert!(helptext.contains("User: agent"));
        assert!(helptext.contains("Host: sandbox"));
        assert!(helptext.contains("50 commands"));
        assert!(helptext.contains("API_KEY"));

        // system_prompt should include home
        let sysprompt = tool.system_prompt();
        assert!(sysprompt.contains("# Bash Tool"));
        assert!(sysprompt.contains("Home: /home/agent"));
    }

    #[test]
    fn test_tool_schemas() {
        let tool = BashTool::default();
        let input_schema = tool.input_schema();
        let output_schema = tool.output_schema();

        // Input schema should have commands property
        assert!(input_schema["properties"]["commands"].is_object());

        // Output schema should have stdout, stderr, exit_code
        assert!(output_schema["properties"]["stdout"].is_object());
        assert!(output_schema["properties"]["stderr"].is_object());
        assert!(output_schema["properties"]["exit_code"].is_object());
    }

    #[test]
    fn test_tool_status() {
        let status = ToolStatus::new("execute")
            .with_message("Running commands")
            .with_percent(50.0)
            .with_eta(5000);

        assert_eq!(status.phase, "execute");
        assert_eq!(status.message, Some("Running commands".to_string()));
        assert_eq!(status.percent_complete, Some(50.0));
        assert_eq!(status.eta_ms, Some(5000));
    }

    #[tokio::test]
    async fn test_tool_execute_empty() {
        let mut tool = BashTool::default();
        let req = ToolRequest {
            commands: String::new(),
            timeout_ms: None,
        };
        let resp = tool.execute(req).await;
        assert_eq!(resp.exit_code, 0);
        assert!(resp.error.is_none());
    }

    #[tokio::test]
    async fn test_tool_execute_echo() {
        let mut tool = BashTool::default();
        let req = ToolRequest {
            commands: "echo hello".to_string(),
            timeout_ms: None,
        };
        let resp = tool.execute(req).await;
        assert_eq!(resp.stdout, "hello\n");
        assert_eq!(resp.exit_code, 0);
        assert!(resp.error.is_none());
    }

    #[test]
    fn test_builtin_hints_in_help_and_system_prompt() {
        use crate::builtins::Builtin;
        use crate::error::Result;
        use crate::interpreter::ExecResult;

        struct HintedBuiltin;

        #[async_trait]
        impl Builtin for HintedBuiltin {
            async fn execute(&self, _ctx: crate::builtins::Context<'_>) -> Result<ExecResult> {
                Ok(ExecResult::ok(String::new()))
            }
            fn llm_hint(&self) -> Option<&'static str> {
                Some("mycommand: Processes CSV. Max 10MB. No streaming.")
            }
        }

        let tool = BashTool::builder()
            .builtin("mycommand", Box::new(HintedBuiltin))
            .build();

        // Hint should appear in help
        let helptext = tool.help();
        assert!(helptext.contains("NOTES"), "help should have NOTES section");
        assert!(
            helptext.contains("mycommand: Processes CSV"),
            "help should contain the hint"
        );

        // Hint should appear in system_prompt
        let sysprompt = tool.system_prompt();
        assert!(
            sysprompt.contains("mycommand: Processes CSV"),
            "system_prompt should contain the hint"
        );
    }

    #[test]
    fn test_no_hints_without_hinted_builtins() {
        let tool = BashTool::default();

        let helptext = tool.help();
        assert!(
            !helptext.contains("NOTES"),
            "help should not have NOTES without hinted builtins"
        );

        let sysprompt = tool.system_prompt();
        assert!(
            !sysprompt.contains("Note:"),
            "system_prompt should not have notes without hinted builtins"
        );
    }

    #[test]
    fn test_language_warning_default() {
        let tool = BashTool::default();

        let sysprompt = tool.system_prompt();
        assert!(
            sysprompt.contains("Warning: perl, python/python3 not available."),
            "system_prompt should have single combined warning"
        );

        let helptext = tool.help();
        assert!(
            helptext.contains("WARNINGS"),
            "help should have WARNINGS section"
        );
        assert!(
            helptext.contains("perl, python/python3 not available."),
            "help should have single combined warning"
        );
    }

    #[test]
    fn test_language_warning_suppressed_by_custom_builtins() {
        use crate::builtins::Builtin;
        use crate::error::Result;
        use crate::interpreter::ExecResult;

        struct NoopBuiltin;

        #[async_trait]
        impl Builtin for NoopBuiltin {
            async fn execute(&self, _ctx: crate::builtins::Context<'_>) -> Result<ExecResult> {
                Ok(ExecResult::ok(String::new()))
            }
        }

        let tool = BashTool::builder()
            .builtin("python", Box::new(NoopBuiltin))
            .builtin("perl", Box::new(NoopBuiltin))
            .build();

        let sysprompt = tool.system_prompt();
        assert!(
            !sysprompt.contains("Warning:"),
            "no warning when all languages registered"
        );

        let helptext = tool.help();
        assert!(
            !helptext.contains("WARNINGS"),
            "no WARNINGS section when all languages registered"
        );
    }

    #[test]
    fn test_language_warning_partial() {
        use crate::builtins::Builtin;
        use crate::error::Result;
        use crate::interpreter::ExecResult;

        struct NoopBuiltin;

        #[async_trait]
        impl Builtin for NoopBuiltin {
            async fn execute(&self, _ctx: crate::builtins::Context<'_>) -> Result<ExecResult> {
                Ok(ExecResult::ok(String::new()))
            }
        }

        // python3 registered -> only perl warned
        let tool = BashTool::builder()
            .builtin("python3", Box::new(NoopBuiltin))
            .build();

        let sysprompt = tool.system_prompt();
        assert!(
            sysprompt.contains("Warning: perl not available."),
            "should warn about perl only"
        );
        assert!(
            !sysprompt.contains("python/python3"),
            "python warning suppressed when python3 registered"
        );
    }

    #[test]
    fn test_duplicate_hints_deduplicated() {
        use crate::builtins::Builtin;
        use crate::error::Result;
        use crate::interpreter::ExecResult;

        struct SameHint;

        #[async_trait]
        impl Builtin for SameHint {
            async fn execute(&self, _ctx: crate::builtins::Context<'_>) -> Result<ExecResult> {
                Ok(ExecResult::ok(String::new()))
            }
            fn llm_hint(&self) -> Option<&'static str> {
                Some("same hint")
            }
        }

        let tool = BashTool::builder()
            .builtin("cmd1", Box::new(SameHint))
            .builtin("cmd2", Box::new(SameHint))
            .build();

        let helptext = tool.help();
        // Should appear exactly once
        assert_eq!(
            helptext.matches("same hint").count(),
            1,
            "Duplicate hints should be deduplicated"
        );
    }

    #[cfg(feature = "python")]
    #[test]
    fn test_python_hint_via_builder() {
        let tool = BashTool::builder().python().build();

        let helptext = tool.help();
        assert!(helptext.contains("python"), "help should mention python");
        assert!(
            helptext.contains("no open()"),
            "help should document open() limitation"
        );
        assert!(
            helptext.contains("No HTTP"),
            "help should document HTTP limitation"
        );

        let sysprompt = tool.system_prompt();
        assert!(
            sysprompt.contains("python"),
            "system_prompt should mention python"
        );

        // Python warning should be suppressed when python is enabled via Monty
        assert!(
            !sysprompt.contains("python/python3 not available"),
            "python warning should not appear when Monty python enabled"
        );
    }

    #[tokio::test]
    async fn test_tool_execute_with_status() {
        use std::sync::{Arc, Mutex};

        let mut tool = BashTool::default();
        let req = ToolRequest {
            commands: "echo test".to_string(),
            timeout_ms: None,
        };

        let phases = Arc::new(Mutex::new(Vec::new()));
        let phases_clone = phases.clone();

        let resp = tool
            .execute_with_status(
                req,
                Box::new(move |status| {
                    phases_clone
                        .lock()
                        .expect("lock poisoned")
                        .push(status.phase.clone());
                }),
            )
            .await;

        assert_eq!(resp.stdout, "test\n");
        let phases = phases.lock().expect("lock poisoned");
        assert!(phases.contains(&"validate".to_string()));
        assert!(phases.contains(&"complete".to_string()));
    }

    #[tokio::test]
    async fn test_execute_with_status_streams_output() {
        let mut tool = BashTool::default();
        let req = ToolRequest {
            commands: "for i in a b c; do echo $i; done".to_string(),
            timeout_ms: None,
        };

        let events = Arc::new(Mutex::new(Vec::new()));
        let events_clone = events.clone();

        let resp = tool
            .execute_with_status(
                req,
                Box::new(move |status| {
                    events_clone.lock().expect("lock poisoned").push(status);
                }),
            )
            .await;

        assert_eq!(resp.stdout, "a\nb\nc\n");
        assert_eq!(resp.exit_code, 0);

        let events = events.lock().expect("lock poisoned");
        // Should have output events for each iteration
        let output_events: Vec<_> = events.iter().filter(|s| s.phase == "output").collect();
        assert_eq!(
            output_events.len(),
            3,
            "expected 3 output events, got {output_events:?}"
        );
        assert_eq!(output_events[0].output.as_deref(), Some("a\n"));
        assert_eq!(output_events[0].stream.as_deref(), Some("stdout"));
        assert_eq!(output_events[1].output.as_deref(), Some("b\n"));
        assert_eq!(output_events[2].output.as_deref(), Some("c\n"));
    }

    #[tokio::test]
    async fn test_execute_with_status_streams_list_commands() {
        let mut tool = BashTool::default();
        let req = ToolRequest {
            commands: "echo start; echo end".to_string(),
            timeout_ms: None,
        };

        let events = Arc::new(Mutex::new(Vec::new()));
        let events_clone = events.clone();

        let resp = tool
            .execute_with_status(
                req,
                Box::new(move |status| {
                    events_clone.lock().expect("lock poisoned").push(status);
                }),
            )
            .await;

        assert_eq!(resp.stdout, "start\nend\n");

        let events = events.lock().expect("lock poisoned");
        let output_events: Vec<_> = events.iter().filter(|s| s.phase == "output").collect();
        assert_eq!(
            output_events.len(),
            2,
            "expected 2 output events, got {output_events:?}"
        );
        assert_eq!(output_events[0].output.as_deref(), Some("start\n"));
        assert_eq!(output_events[1].output.as_deref(), Some("end\n"));
    }

    #[tokio::test]
    async fn test_execute_with_status_no_duplicate_output() {
        let mut tool = BashTool::default();
        // mix of list + loop: should get 5 distinct events, no duplicates
        let req = ToolRequest {
            commands: "echo start; for i in 1 2 3; do echo $i; done; echo end".to_string(),
            timeout_ms: None,
        };

        let events = Arc::new(Mutex::new(Vec::new()));
        let events_clone = events.clone();

        let resp = tool
            .execute_with_status(
                req,
                Box::new(move |status| {
                    events_clone.lock().expect("lock poisoned").push(status);
                }),
            )
            .await;

        assert_eq!(resp.stdout, "start\n1\n2\n3\nend\n");

        let events = events.lock().expect("lock poisoned");
        let output_events: Vec<_> = events
            .iter()
            .filter(|s| s.phase == "output")
            .map(|s| s.output.as_deref().unwrap_or(""))
            .collect();
        assert_eq!(
            output_events,
            vec!["start\n", "1\n", "2\n", "3\n", "end\n"],
            "should have exactly 5 distinct output events"
        );
    }

    #[test]
    fn test_tool_status_stdout_constructor() {
        let status = ToolStatus::stdout("hello\n");
        assert_eq!(status.phase, "output");
        assert_eq!(status.output.as_deref(), Some("hello\n"));
        assert_eq!(status.stream.as_deref(), Some("stdout"));
        assert!(status.message.is_none());
    }

    #[test]
    fn test_tool_status_stderr_constructor() {
        let status = ToolStatus::stderr("error\n");
        assert_eq!(status.phase, "output");
        assert_eq!(status.output.as_deref(), Some("error\n"));
        assert_eq!(status.stream.as_deref(), Some("stderr"));
    }

    #[tokio::test]
    async fn test_tool_execute_timeout() {
        let mut tool = BashTool::default();
        let req = ToolRequest {
            commands: "sleep 10".to_string(),
            timeout_ms: Some(100),
        };
        let resp = tool.execute(req).await;
        assert_eq!(resp.exit_code, 124);
        assert!(resp.stderr.contains("timed out"));
        assert_eq!(resp.error, Some("timeout".to_string()));
    }

    #[tokio::test]
    async fn test_tool_execute_no_timeout() {
        let mut tool = BashTool::default();
        let req = ToolRequest {
            commands: "echo fast".to_string(),
            timeout_ms: Some(5000),
        };
        let resp = tool.execute(req).await;
        assert_eq!(resp.exit_code, 0);
        assert_eq!(resp.stdout, "fast\n");
    }

    #[test]
    fn test_tool_request_new() {
        let req = ToolRequest::new("echo test");
        assert_eq!(req.commands, "echo test");
        assert_eq!(req.timeout_ms, None);
    }

    #[test]
    fn test_tool_request_deserialize_without_timeout() {
        let json = r#"{"commands":"echo hello"}"#;
        let req: ToolRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.commands, "echo hello");
        assert_eq!(req.timeout_ms, None);
    }

    #[test]
    fn test_tool_request_deserialize_with_timeout() {
        let json = r#"{"commands":"echo hello","timeout_ms":5000}"#;
        let req: ToolRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.commands, "echo hello");
        assert_eq!(req.timeout_ms, Some(5000));
    }
}
