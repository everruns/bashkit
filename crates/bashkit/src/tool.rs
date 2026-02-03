//! Tool builder and contract for BashKit
//!
//! Provides a standardized interface for LLM tool integration.

use crate::error::Error;
use crate::{Bash, ExecResult, ExecutionLimits};
use schemars::{schema_for, JsonSchema};
use serde::{Deserialize, Serialize};

/// Tool description for LLM consumption
pub const TOOL_DESCRIPTION: &str = r#"Executes bash scripts in a sandboxed environment with a virtual filesystem.

- Full bash syntax: variables, pipelines, redirects, loops, functions, arrays
- 30+ built-in commands: echo, cat, grep, sed, awk, jq, and more
- Resource limits for safety
- All file operations in virtual filesystem"#;

/// Extended documentation for LLM consumption (llmtxt)
pub const TOOL_LLMTXT: &str = r#"# BashKit Tool

Executes bash scripts in a sandboxed environment with a virtual filesystem.

## Capabilities
- Full bash syntax support (variables, pipelines, redirects, loops, functions, arrays)
- 30+ built-in commands (echo, cat, grep, sed, awk, jq, curl, etc.)
- Virtual filesystem (all file operations are sandboxed)
- Resource limits (max commands, loop iterations, function depth)
- Custom identity (username, hostname)

## Input Parameters
- `script` (required): The bash script to execute
- `timeout_ms` (optional): Maximum execution time in milliseconds

## Output Fields
- `stdout`: Standard output from the script
- `stderr`: Standard error from the script
- `exit_code`: Exit code (0 = success)

## Examples

### Simple echo
```json
{"script": "echo 'Hello, World!'"}
```
Output: `{"stdout": "Hello, World!\n", "stderr": "", "exit_code": 0}`

### Pipeline with grep
```json
{"script": "echo -e 'apple\\nbanana\\ncherry' | grep a"}
```
Output: `{"stdout": "apple\nbanana\n", "stderr": "", "exit_code": 0}`

### Variables and arithmetic
```json
{"script": "x=5; y=3; echo $((x + y))"}
```
Output: `{"stdout": "8\n", "stderr": "", "exit_code": 0}`

### File operations (virtual filesystem)
```json
{"script": "echo 'data' > /tmp/file.txt && cat /tmp/file.txt"}
```
Output: `{"stdout": "data\n", "stderr": "", "exit_code": 0}`

### JSON processing with jq
```json
{"script": "echo '{\"name\": \"test\"}' | jq '.name'"}
```
Output: `{"stdout": "\"test\"\n", "stderr": "", "exit_code": 0}`

## Error Handling
- Syntax errors return non-zero exit code with error in stderr
- Resource limit violations return specific error messages
- Command not found returns exit code 127
"#;

/// Request to execute a bash script
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ToolRequest {
    /// The bash script to execute
    pub script: String,
    /// Maximum execution time in milliseconds (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
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

/// Builder for configuring the BashKit tool
#[derive(Debug, Clone, Default)]
pub struct ToolBuilder {
    /// Custom username for sandbox identity
    username: Option<String>,
    /// Custom hostname for sandbox identity
    hostname: Option<String>,
    /// Execution limits
    limits: Option<ExecutionLimits>,
    /// Environment variables to set
    env_vars: Vec<(String, String)>,
}

impl ToolBuilder {
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

    /// Build the tool
    pub fn build(self) -> Tool {
        Tool {
            username: self.username,
            hostname: self.hostname,
            limits: self.limits,
            env_vars: self.env_vars,
        }
    }
}

/// Configured BashKit tool
#[derive(Debug, Clone, Default)]
pub struct Tool {
    username: Option<String>,
    hostname: Option<String>,
    limits: Option<ExecutionLimits>,
    env_vars: Vec<(String, String)>,
}

impl Tool {
    /// Create a new tool builder
    pub fn builder() -> ToolBuilder {
        ToolBuilder::new()
    }

    /// Get tool description
    pub fn description(&self) -> &'static str {
        TOOL_DESCRIPTION
    }

    /// Get system prompt (empty for this tool)
    pub fn system_prompt(&self) -> &'static str {
        ""
    }

    /// Get full documentation (llmtxt)
    pub fn llmtxt(&self) -> &'static str {
        TOOL_LLMTXT
    }

    /// Get input schema as JSON
    pub fn input_schema(&self) -> serde_json::Value {
        let schema = schema_for!(ToolRequest);
        serde_json::to_value(schema).unwrap_or_default()
    }

    /// Get output schema as JSON
    pub fn output_schema(&self) -> serde_json::Value {
        let schema = schema_for!(ToolResponse);
        serde_json::to_value(schema).unwrap_or_default()
    }

    /// Create a new Bash instance with tool configuration
    fn create_bash(&self) -> Bash {
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

        builder.build()
    }

    /// Execute the tool with the given request
    pub async fn execute(&self, req: ToolRequest) -> ToolResponse {
        if req.script.is_empty() {
            return ToolResponse {
                stdout: String::new(),
                stderr: String::new(),
                exit_code: 0,
                error: None,
            };
        }

        let mut bash = self.create_bash();

        match bash.exec(&req.script).await {
            Ok(result) => result.into(),
            Err(e) => ToolResponse {
                stdout: String::new(),
                stderr: e.to_string(),
                exit_code: 1,
                error: Some(error_kind(&e)),
            },
        }
    }

    /// Execute the tool with status updates
    pub async fn execute_with_status<F>(
        &self,
        req: ToolRequest,
        mut status_callback: F,
    ) -> ToolResponse
    where
        F: FnMut(ToolStatus),
    {
        status_callback(ToolStatus::new("validate").with_percent(0.0));

        if req.script.is_empty() {
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

        let response = match bash.exec(&req.script).await {
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_builder() {
        let tool = Tool::builder()
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
    fn test_tool_description() {
        let tool = Tool::default();
        assert!(!tool.description().is_empty());
        assert!(tool.system_prompt().is_empty());
        assert!(!tool.llmtxt().is_empty());
    }

    #[test]
    fn test_tool_schemas() {
        let tool = Tool::default();
        let input_schema = tool.input_schema();
        let output_schema = tool.output_schema();

        // Input schema should have script property
        assert!(input_schema["properties"]["script"].is_object());

        // Output schema should have stdout, stderr, exit_code
        assert!(output_schema["properties"]["stdout"].is_object());
        assert!(output_schema["properties"]["stderr"].is_object());
        assert!(output_schema["properties"]["exit_code"].is_object());
    }

    #[test]
    fn test_tool_status() {
        let status = ToolStatus::new("execute")
            .with_message("Running script")
            .with_percent(50.0)
            .with_eta(5000);

        assert_eq!(status.phase, "execute");
        assert_eq!(status.message, Some("Running script".to_string()));
        assert_eq!(status.percent_complete, Some(50.0));
        assert_eq!(status.eta_ms, Some(5000));
    }

    #[tokio::test]
    async fn test_tool_execute_empty() {
        let tool = Tool::default();
        let req = ToolRequest {
            script: String::new(),
            timeout_ms: None,
        };
        let resp = tool.execute(req).await;
        assert_eq!(resp.exit_code, 0);
        assert!(resp.error.is_none());
    }

    #[tokio::test]
    async fn test_tool_execute_echo() {
        let tool = Tool::default();
        let req = ToolRequest {
            script: "echo hello".to_string(),
            timeout_ms: None,
        };
        let resp = tool.execute(req).await;
        assert_eq!(resp.stdout, "hello\n");
        assert_eq!(resp.exit_code, 0);
        assert!(resp.error.is_none());
    }

    #[tokio::test]
    async fn test_tool_execute_with_status() {
        let tool = Tool::default();
        let req = ToolRequest {
            script: "echo test".to_string(),
            timeout_ms: None,
        };

        let mut phases = Vec::new();
        let resp = tool
            .execute_with_status(req, |status| {
                phases.push(status.phase.clone());
            })
            .await;

        assert_eq!(resp.stdout, "test\n");
        assert!(phases.contains(&"validate".to_string()));
        assert!(phases.contains(&"complete".to_string()));
    }
}
