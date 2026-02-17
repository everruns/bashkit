//! Tool orchestration
//!
//! Compose tool definitions + callbacks into a single [`Tool`] that accepts bash
//! scripts. Each tool becomes a builtin command inside the interpreter, so an LLM
//! can orchestrate many tools in one call using pipes, variables, loops, and
//! conditionals.
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
//! use bashkit::{OrchestratorTool, ToolDef, Tool, ToolRequest};
//!
//! # tokio_test::block_on(async {
//! let mut tool = OrchestratorTool::builder("api")
//!     .tool(
//!         ToolDef::new("get_user", "Fetch user by id. Usage: get_user <id>"),
//!         |args, _stdin| {
//!             let id = args.first().ok_or("missing id")?;
//!             Ok(format!("{{\"id\":{},\"name\":\"Alice\"}}\n", id))
//!         },
//!     )
//!     .build();
//!
//! let resp = tool.execute(ToolRequest {
//!     commands: "get_user 42 | jq '.name'".to_string(),
//! }).await;
//!
//! assert_eq!(resp.stdout.trim(), "\"Alice\"");
//! # });
//! ```

mod execute;

use crate::ExecutionLimits;
use std::sync::Arc;

// ============================================================================
// ToolDef — OpenAPI-style tool definition
// ============================================================================

/// OpenAPI-style tool definition: name, description, input schema.
///
/// Describes a sub-tool registered with [`OrchestratorToolBuilder`].
/// The `input_schema` is optional JSON Schema for documentation / LLM prompts.
pub struct ToolDef {
    /// Command name used as bash builtin (e.g. `"get_user"`).
    pub name: String,
    /// Human-readable description for LLM consumption.
    pub description: String,
    /// JSON Schema describing accepted arguments. Empty object if unspecified.
    pub input_schema: serde_json::Value,
}

impl ToolDef {
    /// Create a tool definition with name and description.
    pub fn new(name: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            input_schema: serde_json::Value::Object(Default::default()),
        }
    }

    /// Attach a JSON Schema for the tool's input parameters.
    pub fn with_schema(mut self, schema: serde_json::Value) -> Self {
        self.input_schema = schema;
        self
    }
}

// ============================================================================
// ToolCallback — execution callback type
// ============================================================================

/// Execution callback for a registered tool.
///
/// - `args`: positional arguments after command name.
///   For `get_user --id 5`, args is `["--id", "5"]`.
/// - `stdin`: pipeline input from prior command, if any.
///
/// Return `Ok(stdout)` on success or `Err(message)` on failure.
pub type ToolCallback =
    Arc<dyn Fn(&[String], Option<&str>) -> Result<String, String> + Send + Sync>;

// ============================================================================
// RegisteredTool — internal definition + callback pair
// ============================================================================

/// A registered tool: definition + callback.
pub(crate) struct RegisteredTool {
    pub(crate) def: ToolDef,
    pub(crate) callback: ToolCallback,
}

// ============================================================================
// OrchestratorToolBuilder
// ============================================================================

/// Builder for [`OrchestratorTool`].
///
/// ```rust
/// use bashkit::{OrchestratorTool, ToolDef};
///
/// let tool = OrchestratorTool::builder("net")
///     .short_description("Network tools")
///     .tool(
///         ToolDef::new("ping", "Ping a host"),
///         |args, _stdin| {
///             Ok(format!("pong {}\n", args.first().unwrap_or(&String::new())))
///         },
///     )
///     .build();
/// ```
pub struct OrchestratorToolBuilder {
    name: String,
    short_desc: Option<String>,
    tools: Vec<RegisteredTool>,
    limits: Option<ExecutionLimits>,
    env_vars: Vec<(String, String)>,
}

impl OrchestratorToolBuilder {
    pub(crate) fn new(name: impl Into<String>) -> Self {
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

    /// Register a tool with its definition and execution callback.
    pub fn tool(
        mut self,
        def: ToolDef,
        callback: impl Fn(&[String], Option<&str>) -> Result<String, String> + Send + Sync + 'static,
    ) -> Self {
        self.tools.push(RegisteredTool {
            def,
            callback: Arc::new(callback),
        });
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
        let short_desc = self
            .short_desc
            .unwrap_or_else(|| format!("Orchestrator: {}", self.name));

        OrchestratorTool {
            name: self.name,
            short_desc,
            tools: self.tools,
            limits: self.limits,
            env_vars: self.env_vars,
        }
    }
}

// ============================================================================
// OrchestratorTool
// ============================================================================

/// A [`Tool`](crate::tool::Tool) that orchestrates multiple tools via bash scripts.
///
/// Each registered tool (defined by [`ToolDef`] + callback) becomes a bash builtin.
/// The LLM sends a bash script that can pipe, loop, branch, and compose these
/// builtins together with standard utilities like `jq`, `grep`, `sed`, etc.
///
/// Reusable — `execute()` can be called multiple times. Each call gets a fresh
/// Bash interpreter with the same set of tool builtins.
///
/// Create via [`OrchestratorTool::builder`].
pub struct OrchestratorTool {
    pub(crate) name: String,
    pub(crate) short_desc: String,
    pub(crate) tools: Vec<RegisteredTool>,
    pub(crate) limits: Option<ExecutionLimits>,
    pub(crate) env_vars: Vec<(String, String)>,
}

impl OrchestratorTool {
    /// Create a builder with the given tool name.
    pub fn builder(name: impl Into<String>) -> OrchestratorToolBuilder {
        OrchestratorToolBuilder::new(name)
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool::{Tool, ToolRequest, VERSION};

    fn build_test_tool() -> OrchestratorTool {
        OrchestratorTool::builder("test_api")
            .short_description("Test API orchestrator")
            .tool(
                ToolDef::new("get_user", "Fetch user by id. Usage: get_user <id>"),
                |args, _stdin| {
                    let id = args.first().ok_or("missing user id")?;
                    Ok(format!(
                        "{{\"id\":{id},\"name\":\"Alice\",\"email\":\"alice@example.com\"}}\n"
                    ))
                },
            )
            .tool(
                ToolDef::new(
                    "get_orders",
                    "List orders for user. Usage: get_orders <user_id>",
                ),
                |args, _stdin| {
                    let uid = args.first().ok_or("missing user id")?;
                    Ok(format!(
                        "[{{\"order_id\":1,\"user_id\":{uid},\"total\":29.99}},\
                         {{\"order_id\":2,\"user_id\":{uid},\"total\":49.50}}]\n"
                    ))
                },
            )
            .tool(
                ToolDef::new("fail_tool", "Always fails (for testing error handling)"),
                |_args, _stdin| Err("service unavailable".to_string()),
            )
            .tool(
                ToolDef::new("from_stdin", "Read from stdin, uppercase it"),
                |_args, stdin| match stdin {
                    Some(input) => Ok(input.to_uppercase()),
                    None => Err("no stdin".to_string()),
                },
            )
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
            .tool(
                ToolDef::new("api_echo", "Echo args back as JSON"),
                |args, _stdin| {
                    Ok(format!(
                        "{{\"args\":[{}]}}\n",
                        args.iter()
                            .map(|a| format!("\"{}\"", a))
                            .collect::<Vec<_>>()
                            .join(",")
                    ))
                },
            )
            .build();
        assert_eq!(tool.short_description(), "Orchestrator: mytools");
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
    fn test_system_prompt_includes_schema() {
        let tool = OrchestratorTool::builder("schema_test")
            .tool(
                ToolDef::new("get_user", "Fetch user by id").with_schema(serde_json::json!({
                    "type": "object",
                    "properties": {
                        "id": {"type": "integer"}
                    },
                    "required": ["id"]
                })),
                |_args, _stdin| Ok("ok\n".to_string()),
            )
            .build();
        let sp = tool.system_prompt();
        assert!(
            sp.contains("Schema:"),
            "system prompt should include schema"
        );
        assert!(sp.contains("\"type\":\"integer\""));
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
            .tool(
                ToolDef::new("api_echo", "Echo args back as JSON"),
                |args, _stdin| {
                    Ok(format!(
                        "{{\"args\":[{}]}}\n",
                        args.iter()
                            .map(|a| format!("\"{}\"", a))
                            .collect::<Vec<_>>()
                            .join(",")
                    ))
                },
            )
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
