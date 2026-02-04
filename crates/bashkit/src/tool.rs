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

/// Base llmtext documentation template
const BASE_LLMTEXT: &str = r#"# BashKit

Sandboxed bash interpreter with virtual filesystem.

## Capabilities

- Full bash syntax: variables, pipelines, redirects, loops, functions, arrays
- 30+ builtins: echo, cat, grep, sed, awk, jq, curl, etc.
- Virtual filesystem (all operations sandboxed)
- Resource limits (commands, iterations, function depth)

## Input

- `commands` (required): Bash commands to execute (like `bash -c`)

## Output

- `stdout`: Standard output
- `stderr`: Standard error
- `exit_code`: 0 = success

## Examples

```json
{"commands": "echo 'Hello'"}
```
→ `{"stdout": "Hello\n", "stderr": "", "exit_code": 0}`

```json
{"commands": "x=5; y=3; echo $((x + y))"}
```
→ `{"stdout": "8\n", "stderr": "", "exit_code": 0}`

```json
{"commands": "echo '{\"n\":1}' | jq '.n'"}
```
→ `{"stdout": "1\n", "stderr": "", "exit_code": 0}`

## Running Scripts from VFS

```json
{"commands": "source /path/to/script.sh"}
```

## Errors

- Syntax error: non-zero exit, error in stderr
- Command not found: exit code 127
- Resource limit: specific error message
"#;

/// Condensed system prompt template (token-efficient)
const BASE_SYSTEM_PROMPT: &str = r#"bashkit: sandboxed bash with vfs.
Input: {"commands": "..."}
Output: {stdout, stderr, exit_code}
Builtins: echo cat grep sed awk jq curl head tail sort uniq cut tr wc date sleep mkdir rm cp mv touch chmod printf test [ true false exit cd pwd ls find xargs basename dirname env export read
"#;

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
    pub fn builtin(mut self, name: impl Into<String>, builtin: Box<dyn Builtin>) -> Self {
        self.builtins.push((name.into(), builtin));
        self
    }

    /// Build the BashTool
    pub fn build(self) -> BashTool {
        let builtin_names: Vec<String> = self.builtins.iter().map(|(n, _)| n.clone()).collect();
        BashTool {
            username: self.username,
            hostname: self.hostname,
            limits: self.limits,
            env_vars: self.env_vars,
            builtins: self.builtins,
            builtin_names,
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

    /// Build dynamic description with custom builtins
    fn build_description(&self) -> String {
        const SHORT: &str = "Sandboxed bash interpreter with virtual filesystem";
        if self.builtin_names.is_empty() {
            SHORT.to_string()
        } else {
            format!("{}. Custom: {}", SHORT, self.builtin_names.join(", "))
        }
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
            doc.push_str("\n## Current Configuration\n\n");

            if !self.builtin_names.is_empty() {
                doc.push_str("### Custom Commands\n\n");
                doc.push_str("The following custom commands are available in addition to built-in commands:\n\n");
                for name in &self.builtin_names {
                    doc.push_str(&format!("- `{name}`\n"));
                }
                doc.push('\n');
            }

            if self.username.is_some() || self.hostname.is_some() {
                doc.push_str("### Sandbox Identity\n\n");
                if let Some(ref username) = self.username {
                    doc.push_str(&format!(
                        "- Username: `{username}` (returned by `whoami`)\n"
                    ));
                }
                if let Some(ref hostname) = self.hostname {
                    doc.push_str(&format!(
                        "- Hostname: `{hostname}` (returned by `hostname`)\n"
                    ));
                }
                doc.push('\n');
            }

            if let Some(ref limits) = self.limits {
                doc.push_str("### Resource Limits\n\n");
                doc.push_str(&format!("- Max commands: {}\n", limits.max_commands));
                doc.push_str(&format!(
                    "- Max loop iterations: {}\n",
                    limits.max_loop_iterations
                ));
                doc.push_str(&format!(
                    "- Max function depth: {}\n",
                    limits.max_function_depth
                ));
                doc.push('\n');
            }

            if !self.env_vars.is_empty() {
                doc.push_str("### Environment Variables\n\n");
                doc.push_str("The following environment variables are pre-set:\n\n");
                for (key, _) in &self.env_vars {
                    doc.push_str(&format!("- `{key}`\n"));
                }
                doc.push('\n');
            }
        }

        doc
    }

    /// Build dynamic system prompt with custom builtins
    fn build_system_prompt(&self) -> String {
        let mut prompt = BASE_SYSTEM_PROMPT.to_string();
        if !self.builtin_names.is_empty() {
            prompt.push_str("Custom: ");
            prompt.push_str(&self.builtin_names.join(" "));
            prompt.push('\n');
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
        Error::Parse(_) => "parse_error".to_string(),
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
        assert!(tool.description().contains("Sandboxed bash"));
        assert!(tool.llmtext().contains("# BashKit"));
        assert!(tool.system_prompt().contains("bashkit"));
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

        // llmtxt should include configuration
        let llmtxt = tool.llmtext();
        assert!(llmtxt.contains("## Current Configuration"));
        assert!(llmtxt.contains("### Sandbox Identity"));
        assert!(llmtxt.contains("Username: `agent`"));
        assert!(llmtxt.contains("Hostname: `sandbox`"));
        assert!(llmtxt.contains("### Resource Limits"));
        assert!(llmtxt.contains("Max commands: 50"));
        assert!(llmtxt.contains("### Environment Variables"));
        assert!(llmtxt.contains("`API_KEY`"));
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
                    phases_clone.lock().unwrap().push(status.phase.clone());
                }),
            )
            .await;

        assert_eq!(resp.stdout, "test\n");
        let phases = phases.lock().unwrap();
        assert!(phases.contains(&"validate".to_string()));
        assert!(phases.contains(&"complete".to_string()));
    }
}
