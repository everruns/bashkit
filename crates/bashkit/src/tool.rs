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
//! - [`BashTool`]: Sandboxed bash interpreter implementing Tool
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
//! assert!(!tool.llmtext().is_empty());
//!
//! // Execution
//! let resp = tool.execute(ToolRequest {
//!     commands: "echo hello".to_string(),
//! }).await;
//! assert_eq!(resp.stdout, "hello\n");
//! # });
//! ```

use crate::builtins::Builtin;
use crate::error::Error;
use crate::{Bash, ExecResult, ExecutionLimits};
use async_trait::async_trait;
use schemars::{schema_for, JsonSchema};
use serde::{Deserialize, Serialize};

/// Library version from Cargo.toml
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// List of built-in commands
const BUILTINS: &str = "echo cat grep sed awk jq curl head tail sort uniq cut tr wc date sleep mkdir rm cp mv touch chmod printf test [ true false exit cd pwd ls find xargs basename dirname env export read";

/// Base llmtext documentation template (generic help format)
const BASE_LLMTEXT: &str = r#"BASH(1)                          User Commands                         BASH(1)

NAME
       bashkit - sandboxed bash-like interpreter with virtual filesystem

SYNOPSIS
       {"commands": "<bash commands>"}

DESCRIPTION
       Bashkit executes bash commands in an isolated sandbox with a virtual
       filesystem. All file operations are contained within the sandbox.

       Supports full bash syntax including variables, pipelines, redirects,
       loops, conditionals, functions, and arrays.

BUILTINS
       echo, cat, grep, sed, awk, jq, curl, head, tail, sort, uniq, cut, tr,
       wc, date, sleep, mkdir, rm, cp, mv, touch, chmod, printf, test, [,
       true, false, exit, cd, pwd, ls, find, xargs, basename, dirname, env,
       export, read

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
    /// Current phase (e.g., "validate", "parse", "execute", "complete")
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
}

impl ToolStatus {
    /// Create a new status with phase
    pub fn new(phase: impl Into<String>) -> Self {
        Self {
            phase: phase.into(),
            message: None,
            percent_complete: None,
            eta_ms: None,
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
/// - [`BashTool`]: Sandboxed bash interpreter
#[async_trait]
pub trait Tool: Send + Sync {
    /// Tool identifier (e.g., "bashkit", "calculator")
    fn name(&self) -> &str;

    /// One-line description for tool listings
    fn short_description(&self) -> &str;

    /// Full description, may include dynamic config info
    fn description(&self) -> String;

    /// Full documentation for LLMs (human readable, with examples)
    fn llmtext(&self) -> String;

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
    /// Custom username for sandbox identity
    username: Option<String>,
    /// Custom hostname for sandbox identity
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

    /// Set custom username for sandbox identity
    pub fn username(mut self, username: impl Into<String>) -> Self {
        self.username = Some(username.into());
        self
    }

    /// Set custom hostname for sandbox identity
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
    /// They will be documented in the tool's llmtxt output.
    /// If the builtin implements [`Builtin::llm_hint`], its hint will be
    /// included in `llmtext()` and `system_prompt()`.
    pub fn builtin(mut self, name: impl Into<String>, builtin: Box<dyn Builtin>) -> Self {
        self.builtins.push((name.into(), builtin));
        self
    }

    /// Enable embedded Python (`python`/`python3` builtins) via Monty interpreter
    /// with default resource limits.
    ///
    /// Requires the `python` feature flag. Python `pathlib.Path` operations are
    /// bridged to the virtual filesystem. Limitations (no `open()`, no HTTP) are
    /// automatically documented in `llmtext()` and `system_prompt()`.
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

/// Sandboxed bash interpreter implementing the Tool trait
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
        let mut desc = String::from(
            "Sandboxed bash-like interpreter with virtual filesystem. Supported tools: ",
        );
        desc.push_str(BUILTINS);
        if !self.builtin_names.is_empty() {
            desc.push(' ');
            desc.push_str(&self.builtin_names.join(" "));
        }
        desc
    }

    /// Build dynamic llmtext with configuration
    fn build_llmtext(&self) -> String {
        let mut doc = BASE_LLMTEXT.to_string();

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

        // Append language interpreter warnings
        let warnings = self.language_warnings();
        if !warnings.is_empty() {
            doc.push_str("\nWARNINGS\n");
            for warning in &warnings {
                doc.push_str(&format!("       {warning}\n"));
            }
        }

        doc
    }

    /// Check which language interpreters are missing from registered builtins.
    ///
    /// Returns warnings for python/python3 (add via `.python()` or custom builtin)
    /// and perl (add via custom builtin) when they are not available.
    fn language_warnings(&self) -> Vec<String> {
        let mut warnings = Vec::new();

        let has_python = self
            .builtin_names
            .iter()
            .any(|n| n == "python" || n == "python3");
        if !has_python {
            warnings.push(
                "python/python3 not available. Not added via Monty (.python()) or custom builtin."
                    .to_string(),
            );
        }

        let has_perl = self.builtin_names.iter().any(|n| n == "perl");
        if !has_perl {
            warnings.push("perl not available. Not added via custom builtin.".to_string());
        }

        warnings
    }

    /// Build dynamic system prompt
    fn build_system_prompt(&self) -> String {
        let mut prompt = String::from("# Bash Tool\n\n");

        // Description with workspace info
        prompt.push_str("Sandboxed bash-like interpreter with virtual filesystem.\n");

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

        // Language interpreter warnings
        let warnings = self.language_warnings();
        if !warnings.is_empty() {
            prompt.push('\n');
            for warning in &warnings {
                prompt.push_str(&format!("Warning: {warning}\n"));
            }
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
        "Sandboxed bash interpreter with virtual filesystem"
    }

    fn description(&self) -> String {
        self.build_description()
    }

    fn llmtext(&self) -> String {
        self.build_llmtext()
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

        match bash.exec(&req.commands).await {
            Ok(result) => result.into(),
            Err(e) => ToolResponse {
                stdout: String::new(),
                stderr: e.to_string(),
                exit_code: 1,
                error: Some(error_kind(&e)),
            },
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

        let response = match bash.exec(&req.commands).await {
            Ok(result) => result.into(),
            Err(e) => ToolResponse {
                stdout: String::new(),
                stderr: e.to_string(),
                exit_code: 1,
                error: Some(error_kind(&e)),
            },
        };

        status_callback(ToolStatus::new("complete").with_percent(100.0));

        response
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
        Error::Internal(_) => "internal_error".to_string(),
    }
}

#[cfg(test)]
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
            "Sandboxed bash interpreter with virtual filesystem"
        );
        assert!(tool
            .description()
            .contains("Sandboxed bash-like interpreter"));
        assert!(tool.description().contains("Supported tools:"));
        assert!(tool.llmtext().contains("BASH(1)"));
        assert!(tool.llmtext().contains("SYNOPSIS"));
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

        // llmtxt should include configuration in man-page style
        let llmtxt = tool.llmtext();
        assert!(llmtxt.contains("CONFIGURATION"));
        assert!(llmtxt.contains("User: agent"));
        assert!(llmtxt.contains("Host: sandbox"));
        assert!(llmtxt.contains("50 commands"));
        assert!(llmtxt.contains("API_KEY"));

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
        };
        let resp = tool.execute(req).await;
        assert_eq!(resp.stdout, "hello\n");
        assert_eq!(resp.exit_code, 0);
        assert!(resp.error.is_none());
    }

    #[test]
    fn test_builtin_hints_in_llmtext_and_system_prompt() {
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

        // Hint should appear in llmtext
        let llmtxt = tool.llmtext();
        assert!(
            llmtxt.contains("NOTES"),
            "llmtext should have NOTES section"
        );
        assert!(
            llmtxt.contains("mycommand: Processes CSV"),
            "llmtext should contain the hint"
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

        let llmtxt = tool.llmtext();
        assert!(
            !llmtxt.contains("NOTES"),
            "llmtext should not have NOTES without hinted builtins"
        );

        let sysprompt = tool.system_prompt();
        assert!(
            !sysprompt.contains("Note:"),
            "system_prompt should not have notes without hinted builtins"
        );
    }

    #[test]
    fn test_language_warnings_default() {
        let tool = BashTool::default();

        let sysprompt = tool.system_prompt();
        assert!(
            sysprompt.contains("Warning: python/python3 not available"),
            "system_prompt should warn about missing python"
        );
        assert!(
            sysprompt.contains("Warning: perl not available"),
            "system_prompt should warn about missing perl"
        );

        let llmtxt = tool.llmtext();
        assert!(
            llmtxt.contains("WARNINGS"),
            "llmtext should have WARNINGS section"
        );
        assert!(
            llmtxt.contains("python/python3 not available"),
            "llmtext should warn about missing python"
        );
        assert!(
            llmtxt.contains("perl not available"),
            "llmtext should warn about missing perl"
        );
    }

    #[test]
    fn test_language_warnings_suppressed_by_custom_builtins() {
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
            !sysprompt.contains("python/python3 not available"),
            "python warning should be suppressed when python registered"
        );
        assert!(
            !sysprompt.contains("perl not available"),
            "perl warning should be suppressed when perl registered"
        );

        let llmtxt = tool.llmtext();
        assert!(
            !llmtxt.contains("WARNINGS"),
            "llmtext should not have WARNINGS when all languages registered"
        );
    }

    #[test]
    fn test_language_warning_python3_suppresses() {
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

        // Registering python3 alone should suppress the python warning
        let tool = BashTool::builder()
            .builtin("python3", Box::new(NoopBuiltin))
            .build();

        let sysprompt = tool.system_prompt();
        assert!(
            !sysprompt.contains("python/python3 not available"),
            "python warning should be suppressed when python3 registered"
        );
        // perl should still warn
        assert!(
            sysprompt.contains("perl not available"),
            "perl warning should still appear"
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

        let llmtxt = tool.llmtext();
        // Should appear exactly once
        assert_eq!(
            llmtxt.matches("same hint").count(),
            1,
            "Duplicate hints should be deduplicated"
        );
    }

    #[cfg(feature = "python")]
    #[test]
    fn test_python_hint_via_builder() {
        let tool = BashTool::builder().python().build();

        let llmtxt = tool.llmtext();
        assert!(llmtxt.contains("python"), "llmtext should mention python");
        assert!(
            llmtxt.contains("no open()"),
            "llmtext should document open() limitation"
        );
        assert!(
            llmtxt.contains("No HTTP"),
            "llmtext should document HTTP limitation"
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
}
