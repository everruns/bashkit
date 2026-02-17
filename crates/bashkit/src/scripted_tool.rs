//! Scripted tool orchestration
//!
//! Compose multiple [`CallableTool`]s into a single [`Tool`] that accepts bash
//! scripts. Each `CallableTool` becomes a builtin command inside the interpreter,
//! so an LLM can orchestrate many tools in one call using pipes, variables, loops,
//! and conditionals.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────┐
//! │  OrchestratorTool  (implements Tool)     │
//! │                                         │
//! │  ┌─────────┐ ┌─────────┐ ┌──────────┐  │
//! │  │get_user │ │get_order│ │inventory │  │
//! │  │(builtin)│ │(builtin)│ │(builtin) │  │
//! │  └─────────┘ └─────────┘ └──────────┘  │
//! │        ↑           ↑           ↑        │
//! │  bash script: pipes, vars, jq, loops    │
//! └─────────────────────────────────────────┘
//! ```
//!
//! # Example
//!
//! ```rust
//! use bashkit::{
//!     OrchestratorTool, CallableTool, Tool, ToolRequest,
//! };
//!
//! struct GetUser;
//!
//! impl CallableTool for GetUser {
//!     fn name(&self) -> &str { "get_user" }
//!     fn description(&self) -> &str { "Fetch user by id. Usage: get_user <id>" }
//!     fn call(&self, args: &[String], _stdin: Option<&str>) -> Result<String, String> {
//!         let id = args.first().ok_or("missing id")?;
//!         Ok(format!("{{\"id\":{},\"name\":\"Alice\"}}\n", id))
//!     }
//! }
//!
//! # tokio_test::block_on(async {
//! let mut tool = OrchestratorTool::builder("api")
//!     .tool(Box::new(GetUser))
//!     .build();
//!
//! let resp = tool.execute(ToolRequest {
//!     commands: "get_user 42 | jq '.name'".to_string(),
//! }).await;
//!
//! assert_eq!(resp.stdout.trim(), "\"Alice\"");
//! # });
//! ```

use crate::builtins::{Builtin, Context};
use crate::error::Result;
use crate::interpreter::ExecResult;
use crate::tool::{Tool, ToolRequest, ToolResponse, ToolStatus, VERSION};
use crate::{Bash, ExecutionLimits};
use async_trait::async_trait;
use schemars::schema_for;
use std::sync::Arc;

// ============================================================================
// CallableTool — sub-tool trait
// ============================================================================

/// A callable sub-tool that can be exposed as a bash builtin.
///
/// Implement this for each external operation (API call, DB query, etc.)
/// you want to compose via bash scripts.
///
/// # Sync design
///
/// `call` is intentionally synchronous. Most tool calls are fast (mock data,
/// HTTP via blocking client, cache lookups). If you need async, use
/// `tokio::runtime::Handle::current().block_on()` or similar inside `call`.
pub trait CallableTool: Send + Sync {
    /// Command name used as the builtin (e.g. `"get_user"`).
    fn name(&self) -> &str;

    /// One-line description for LLM consumption.
    fn description(&self) -> &str;

    /// Execute the tool.
    ///
    /// - `args`: command arguments (not including the command name itself).
    ///   For `get_user --id 5`, args is `["--id", "5"]`.
    /// - `stdin`: pipeline input from the previous command, if any.
    ///
    /// Return `Ok(stdout)` on success or `Err(message)` on failure.
    fn call(&self, args: &[String], stdin: Option<&str>) -> std::result::Result<String, String>;
}

// ============================================================================
// CallableToolBuiltin — adapter from CallableTool to Builtin
// ============================================================================

/// Wraps an `Arc<dyn CallableTool>` as a [`Builtin`] so the interpreter can
/// execute it. Using Arc allows the same tool to be shared across multiple
/// Bash instances (one per `execute()` call).
struct CallableToolBuiltin {
    inner: Arc<dyn CallableTool>,
}

#[async_trait]
impl Builtin for CallableToolBuiltin {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        match self.inner.call(ctx.args, ctx.stdin) {
            Ok(stdout) => Ok(ExecResult::ok(stdout)),
            Err(msg) => Ok(ExecResult::err(msg, 1)),
        }
    }

    fn llm_hint(&self) -> Option<&'static str> {
        None
    }
}

// ============================================================================
// OrchestratorToolBuilder
// ============================================================================

/// Builder for [`OrchestratorTool`].
///
/// ```rust
/// use bashkit::{OrchestratorTool, CallableTool};
///
/// struct Ping;
/// impl CallableTool for Ping {
///     fn name(&self) -> &str { "ping" }
///     fn description(&self) -> &str { "Ping a host" }
///     fn call(&self, args: &[String], _stdin: Option<&str>) -> Result<String, String> {
///         Ok(format!("pong {}\n", args.first().unwrap_or(&String::new())))
///     }
/// }
///
/// let tool = OrchestratorTool::builder("net")
///     .short_description("Network tools")
///     .tool(Box::new(Ping))
///     .build();
/// ```
pub struct OrchestratorToolBuilder {
    name: String,
    short_desc: Option<String>,
    tools: Vec<Box<dyn CallableTool>>,
    limits: Option<ExecutionLimits>,
    env_vars: Vec<(String, String)>,
}

impl OrchestratorToolBuilder {
    fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            short_desc: None,
            tools: Vec::new(),
            limits: None,
            env_vars: Vec::new(),
        }
    }

    /// One-line description for tool listings.
    pub fn short_description(mut self, desc: impl Into<String>) -> Self {
        self.short_desc = Some(desc.into());
        self
    }

    /// Register a sub-tool. It becomes a bash builtin with the same name.
    pub fn tool(mut self, tool: Box<dyn CallableTool>) -> Self {
        self.tools.push(tool);
        self
    }

    /// Set execution limits for the bash interpreter.
    pub fn limits(mut self, limits: ExecutionLimits) -> Self {
        self.limits = Some(limits);
        self
    }

    /// Add an environment variable visible inside scripts.
    pub fn env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env_vars.push((key.into(), value.into()));
        self
    }

    /// Build the [`OrchestratorTool`].
    pub fn build(self) -> OrchestratorTool {
        let tool_descriptions: Vec<ToolDescription> = self
            .tools
            .iter()
            .map(|t| ToolDescription {
                name: t.name().to_string(),
                description: t.description().to_string(),
            })
            .collect();

        // Wrap in Arc so each execute() can clone refs into new Bash instances
        let tools: Vec<Arc<dyn CallableTool>> = self.tools.into_iter().map(Arc::from).collect();

        let short_desc = self
            .short_desc
            .unwrap_or_else(|| format!("Scripted orchestrator: {}", self.name));

        OrchestratorTool {
            name: self.name,
            short_desc,
            tool_descriptions,
            tools,
            limits: self.limits,
            env_vars: self.env_vars,
        }
    }
}

// ============================================================================
// OrchestratorTool
// ============================================================================

/// Metadata kept for documentation generation.
struct ToolDescription {
    name: String,
    description: String,
}

/// A [`Tool`] that orchestrates multiple [`CallableTool`]s via bash scripts.
///
/// Each registered `CallableTool` becomes a bash builtin. The LLM sends a
/// bash script that can pipe, loop, branch, and compose these builtins
/// together with standard utilities like `jq`, `grep`, `sed`, etc.
///
/// The tool is reusable — `execute()` can be called multiple times. Each call
/// gets a fresh Bash interpreter with the same set of tool-builtins.
///
/// Create via [`OrchestratorTool::builder`].
pub struct OrchestratorTool {
    name: String,
    short_desc: String,
    tool_descriptions: Vec<ToolDescription>,
    /// Arc-wrapped tools that are cloned into each Bash instance.
    tools: Vec<Arc<dyn CallableTool>>,
    limits: Option<ExecutionLimits>,
    env_vars: Vec<(String, String)>,
}

impl OrchestratorTool {
    /// Create a builder with the given tool name.
    pub fn builder(name: impl Into<String>) -> OrchestratorToolBuilder {
        OrchestratorToolBuilder::new(name)
    }

    /// Create a fresh Bash instance with all tool-builtins registered.
    fn create_bash(&self) -> Bash {
        let mut builder = Bash::builder();

        if let Some(ref limits) = self.limits {
            builder = builder.limits(limits.clone());
        }
        for (key, value) in &self.env_vars {
            builder = builder.env(key, value);
        }
        // Clone Arc refs into fresh builtin adapters for this Bash instance
        for tool in &self.tools {
            let name = tool.name().to_string();
            let builtin: Box<dyn Builtin> = Box::new(CallableToolBuiltin {
                inner: Arc::clone(tool),
            });
            builder = builder.builtin(name, builtin);
        }

        builder.build()
    }

    fn build_description(&self) -> String {
        let mut desc = format!(
            "Scripted tool orchestrator. Available tool-commands: {}",
            self.tool_descriptions
                .iter()
                .map(|t| t.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        );
        desc.push_str(". Also supports standard bash builtins (echo, jq, grep, sed, awk, etc.).");
        desc
    }

    fn build_help(&self) -> String {
        let mut doc = format!(
            "{name}(1)                       Tool Commands                       {name}(1)\n\n\
             NAME\n\
             \x20      {name} - {short_desc}\n\n\
             SYNOPSIS\n\
             \x20      {{\"commands\": \"<bash script using tool commands>\"}}\n\n\
             DESCRIPTION\n\
             \x20      Executes a bash script with access to tool-specific commands\n\
             \x20      and standard bash builtins. Use pipes, variables, loops, and\n\
             \x20      conditionals to orchestrate multiple tool calls in one request.\n\n\
             TOOL COMMANDS\n",
            name = self.name,
            short_desc = self.short_desc,
        );

        for t in &self.tool_descriptions {
            doc.push_str(&format!("       {:<20} {}\n", t.name, t.description));
        }

        doc.push_str(
            "\nBASH BUILTINS\n\
             \x20      echo, cat, grep, sed, awk, jq, head, tail, sort, uniq, cut, tr,\n\
             \x20      wc, printf, test, [, true, false, cd, pwd, ls, find, xargs\n\n\
             INPUT\n\
             \x20      commands    Bash script to execute\n\n\
             OUTPUT\n\
             \x20      stdout      Combined standard output\n\
             \x20      stderr      Errors from tool commands or bash\n\
             \x20      exit_code   0 on success\n\n\
             EXAMPLES\n\
             \x20      Single tool call:\n\
             \x20          {{\"commands\": \"get_user 42\"}}\n\n\
             \x20      Pipeline:\n\
             \x20          {{\"commands\": \"get_user 42 | jq '.name'\"}}\n\n\
             \x20      Multi-step orchestration:\n\
             \x20          {{\"commands\": \"user=$(get_user 42)\\norders=$(get_orders 42)\\necho $user | jq -r '.name'\"}}\n",
        );

        doc
    }

    fn build_system_prompt(&self) -> String {
        let mut prompt = format!("# {}\n\n", self.name);
        prompt.push_str(&format!("{}\n\n", self.short_desc));

        prompt.push_str("Input: {\"commands\": \"<bash script>\"}\n");
        prompt.push_str("Output: {stdout, stderr, exit_code}\n\n");

        prompt.push_str("## Available tool commands\n\n");
        for t in &self.tool_descriptions {
            prompt.push_str(&format!("- `{}`: {}\n", t.name, t.description));
        }

        prompt.push_str(
            "\n## Tips\n\n\
             - Pipe tool output through `jq` for JSON processing\n\
             - Use variables to pass data between tool calls\n\
             - Use `set -e` to stop on first error\n\
             - Standard builtins (echo, grep, sed, awk, etc.) are available\n",
        );

        prompt
    }
}

#[async_trait]
impl Tool for OrchestratorTool {
    fn name(&self) -> &str {
        &self.name
    }

    fn short_description(&self) -> &str {
        &self.short_desc
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

        match bash.exec(&req.commands).await {
            Ok(result) => result.into(),
            Err(e) => ToolResponse {
                stdout: String::new(),
                stderr: e.to_string(),
                exit_code: 1,
                error: Some(format!("{:?}", e)),
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
                error: Some(format!("{:?}", e)),
            },
        };

        status_callback(ToolStatus::new("complete").with_percent(100.0));
        response
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // -- Test CallableTool implementations --

    struct Echo;

    impl CallableTool for Echo {
        fn name(&self) -> &str {
            "api_echo"
        }
        fn description(&self) -> &str {
            "Echo args back as JSON"
        }
        fn call(
            &self,
            args: &[String],
            _stdin: Option<&str>,
        ) -> std::result::Result<String, String> {
            Ok(format!(
                "{{\"args\":[{}]}}\n",
                args.iter()
                    .map(|a| format!("\"{}\"", a))
                    .collect::<Vec<_>>()
                    .join(",")
            ))
        }
    }

    struct GetUser;

    impl CallableTool for GetUser {
        fn name(&self) -> &str {
            "get_user"
        }
        fn description(&self) -> &str {
            "Fetch user by id. Usage: get_user <id>"
        }
        fn call(
            &self,
            args: &[String],
            _stdin: Option<&str>,
        ) -> std::result::Result<String, String> {
            let id = args.first().ok_or("missing user id".to_string())?;
            Ok(format!(
                "{{\"id\":{},\"name\":\"Alice\",\"email\":\"alice@example.com\"}}\n",
                id
            ))
        }
    }

    struct GetOrders;

    impl CallableTool for GetOrders {
        fn name(&self) -> &str {
            "get_orders"
        }
        fn description(&self) -> &str {
            "List orders for user. Usage: get_orders <user_id>"
        }
        fn call(
            &self,
            args: &[String],
            _stdin: Option<&str>,
        ) -> std::result::Result<String, String> {
            let uid = args.first().ok_or("missing user id".to_string())?;
            Ok(format!(
                "[{{\"order_id\":1,\"user_id\":{uid},\"total\":29.99}},\
                 {{\"order_id\":2,\"user_id\":{uid},\"total\":49.50}}]\n"
            ))
        }
    }

    struct FailTool;

    impl CallableTool for FailTool {
        fn name(&self) -> &str {
            "fail_tool"
        }
        fn description(&self) -> &str {
            "Always fails (for testing error handling)"
        }
        fn call(
            &self,
            _args: &[String],
            _stdin: Option<&str>,
        ) -> std::result::Result<String, String> {
            Err("service unavailable".to_string())
        }
    }

    struct StdinTool;

    impl CallableTool for StdinTool {
        fn name(&self) -> &str {
            "from_stdin"
        }
        fn description(&self) -> &str {
            "Read from stdin, uppercase it"
        }
        fn call(
            &self,
            _args: &[String],
            stdin: Option<&str>,
        ) -> std::result::Result<String, String> {
            match stdin {
                Some(input) => Ok(input.to_uppercase()),
                None => Err("no stdin".to_string()),
            }
        }
    }

    fn build_test_tool() -> OrchestratorTool {
        OrchestratorTool::builder("test_api")
            .short_description("Test API orchestrator")
            .tool(Box::new(GetUser))
            .tool(Box::new(GetOrders))
            .tool(Box::new(FailTool))
            .tool(Box::new(StdinTool))
            .build()
    }

    // -- Builder tests --

    #[test]
    fn test_builder_name_and_description() {
        let tool = build_test_tool();
        assert_eq!(tool.name(), "test_api");
        assert_eq!(tool.short_description(), "Test API orchestrator");
    }

    #[test]
    fn test_builder_default_short_description() {
        let tool = OrchestratorTool::builder("mytools")
            .tool(Box::new(Echo))
            .build();
        assert_eq!(tool.short_description(), "Scripted orchestrator: mytools");
    }

    #[test]
    fn test_description_lists_tools() {
        let tool = build_test_tool();
        let desc = tool.description();
        assert!(desc.contains("get_user"));
        assert!(desc.contains("get_orders"));
        assert!(desc.contains("fail_tool"));
        assert!(desc.contains("from_stdin"));
    }

    #[test]
    fn test_help_has_tool_commands_section() {
        let tool = build_test_tool();
        let help = tool.help();
        assert!(help.contains("TOOL COMMANDS"));
        assert!(help.contains("get_user"));
        assert!(help.contains("Fetch user by id"));
    }

    #[test]
    fn test_system_prompt_lists_tools() {
        let tool = build_test_tool();
        let sp = tool.system_prompt();
        assert!(sp.contains("# test_api"));
        assert!(sp.contains("- `get_user`:"));
        assert!(sp.contains("- `get_orders`:"));
        assert!(sp.contains("jq"));
    }

    #[test]
    fn test_schemas() {
        let tool = build_test_tool();
        let input = tool.input_schema();
        assert!(input["properties"]["commands"].is_object());
        let output = tool.output_schema();
        assert!(output["properties"]["stdout"].is_object());
    }

    #[test]
    fn test_version() {
        let tool = build_test_tool();
        assert_eq!(tool.version(), VERSION);
    }

    // -- Execution tests --

    #[tokio::test]
    async fn test_execute_empty() {
        let mut tool = build_test_tool();
        let resp = tool
            .execute(ToolRequest {
                commands: String::new(),
            })
            .await;
        assert_eq!(resp.exit_code, 0);
        assert!(resp.stdout.is_empty());
    }

    #[tokio::test]
    async fn test_execute_single_tool() {
        let mut tool = build_test_tool();
        let resp = tool
            .execute(ToolRequest {
                commands: "get_user 42".to_string(),
            })
            .await;
        assert_eq!(resp.exit_code, 0);
        assert!(resp.stdout.contains("\"name\":\"Alice\""));
        assert!(resp.stdout.contains("\"id\":42"));
    }

    #[tokio::test]
    async fn test_execute_pipeline_with_jq() {
        let mut tool = build_test_tool();
        let resp = tool
            .execute(ToolRequest {
                commands: "get_user 42 | jq -r '.name'".to_string(),
            })
            .await;
        assert_eq!(resp.exit_code, 0);
        assert_eq!(resp.stdout.trim(), "Alice");
    }

    #[tokio::test]
    async fn test_execute_multi_step() {
        let mut tool = build_test_tool();
        let script = r#"
            user=$(get_user 1)
            name=$(echo "$user" | jq -r '.name')
            orders=$(get_orders 1)
            total=$(echo "$orders" | jq '[.[].total] | add')
            echo "User: $name, Total: $total"
        "#;
        let resp = tool
            .execute(ToolRequest {
                commands: script.to_string(),
            })
            .await;
        assert_eq!(resp.exit_code, 0);
        assert_eq!(resp.stdout.trim(), "User: Alice, Total: 79.49");
    }

    #[tokio::test]
    async fn test_execute_tool_failure() {
        let mut tool = build_test_tool();
        let resp = tool
            .execute(ToolRequest {
                commands: "fail_tool".to_string(),
            })
            .await;
        assert_ne!(resp.exit_code, 0);
        assert!(resp.stderr.contains("service unavailable"));
    }

    #[tokio::test]
    async fn test_execute_tool_failure_with_fallback() {
        let mut tool = build_test_tool();
        let resp = tool
            .execute(ToolRequest {
                commands: "fail_tool || echo 'fallback'".to_string(),
            })
            .await;
        assert_eq!(resp.exit_code, 0);
        assert!(resp.stdout.contains("fallback"));
    }

    #[tokio::test]
    async fn test_execute_stdin_pipe() {
        let mut tool = build_test_tool();
        let resp = tool
            .execute(ToolRequest {
                commands: "echo hello | from_stdin".to_string(),
            })
            .await;
        assert_eq!(resp.exit_code, 0);
        assert_eq!(resp.stdout.trim(), "HELLO");
    }

    #[tokio::test]
    async fn test_execute_loop_over_tools() {
        let mut tool = build_test_tool();
        let script = r#"
            for uid in 1 2 3; do
                get_user $uid | jq -r '.name'
            done
        "#;
        let resp = tool
            .execute(ToolRequest {
                commands: script.to_string(),
            })
            .await;
        assert_eq!(resp.exit_code, 0);
        assert_eq!(resp.stdout.trim(), "Alice\nAlice\nAlice");
    }

    #[tokio::test]
    async fn test_execute_conditional() {
        let mut tool = build_test_tool();
        let script = r#"
            user=$(get_user 5)
            name=$(echo "$user" | jq -r '.name')
            if [ "$name" = "Alice" ]; then
                echo "found alice"
            else
                echo "not alice"
            fi
        "#;
        let resp = tool
            .execute(ToolRequest {
                commands: script.to_string(),
            })
            .await;
        assert_eq!(resp.exit_code, 0);
        assert_eq!(resp.stdout.trim(), "found alice");
    }

    #[tokio::test]
    async fn test_execute_with_env() {
        let mut tool = OrchestratorTool::builder("env_test")
            .env("API_BASE", "https://api.example.com")
            .tool(Box::new(Echo))
            .build();

        let resp = tool
            .execute(ToolRequest {
                commands: "echo $API_BASE".to_string(),
            })
            .await;
        assert_eq!(resp.exit_code, 0);
        assert_eq!(resp.stdout.trim(), "https://api.example.com");
    }

    #[tokio::test]
    async fn test_execute_with_status_callback() {
        use std::sync::{Arc, Mutex};

        let mut tool = build_test_tool();
        let phases = Arc::new(Mutex::new(Vec::new()));
        let phases_clone = phases.clone();

        let resp = tool
            .execute_with_status(
                ToolRequest {
                    commands: "get_user 1".to_string(),
                },
                Box::new(move |status| {
                    phases_clone
                        .lock()
                        .expect("lock poisoned")
                        .push(status.phase.clone());
                }),
            )
            .await;

        assert_eq!(resp.exit_code, 0);
        let phases = phases.lock().expect("lock poisoned");
        assert!(phases.contains(&"validate".to_string()));
        assert!(phases.contains(&"execute".to_string()));
        assert!(phases.contains(&"complete".to_string()));
    }

    /// Verify the tool can be called multiple times (Arc sharing works).
    #[tokio::test]
    async fn test_multiple_execute_calls() {
        let mut tool = build_test_tool();

        let resp1 = tool
            .execute(ToolRequest {
                commands: "get_user 1 | jq -r '.name'".to_string(),
            })
            .await;
        assert_eq!(resp1.stdout.trim(), "Alice");

        let resp2 = tool
            .execute(ToolRequest {
                commands: "get_orders 1 | jq 'length'".to_string(),
            })
            .await;
        assert_eq!(resp2.stdout.trim(), "2");

        let resp3 = tool
            .execute(ToolRequest {
                commands: "get_user 2 | jq -r '.email'".to_string(),
            })
            .await;
        assert_eq!(resp3.stdout.trim(), "alice@example.com");
    }
}
