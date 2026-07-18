//! ScriptedTool execution: Tool impl and documentation helpers.

use super::{ScriptedExecutionTrace, ScriptedTool, ToolDefExtension, extension::InvocationLog};
use crate::Bash;
use crate::tool::{
    Tool, ToolError, ToolExecution, ToolOutputChunk, ToolRequest, ToolResponse, ToolStatus,
    VERSION, localized, tool_output_from_response, tool_request_from_value, tool_request_schema,
    tool_response_schema,
};
// timeout_response + Duration are only used on the native timeout path; wasm32
// runs without wall-clock enforcement (no timer driver), so they are gated out
// there to keep the wasm build warning-clean (CI checks wasm with `-D warnings`).
#[cfg(not(target_family = "wasm"))]
use crate::tool::timeout_response;
use crate::tool_def::usage_from_schema;
use async_trait::async_trait;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
#[cfg(not(target_family = "wasm"))]
use std::time::Duration;

// ============================================================================
// ScriptedTool — internal helpers
// ============================================================================

impl ScriptedTool {
    /// Create a fresh Bash instance with all tool builtins registered.
    fn create_bash(&self, log: InvocationLog) -> Bash {
        let mut builder = Bash::builder().logic_only();

        if let Some(ref limits) = self.limits {
            builder = builder.limits(limits.clone());
        }
        for (key, value) in &self.env_vars {
            builder = builder.env(key, value);
        }
        builder = builder.extension(
            ToolDefExtension::from_registered_tools(self.tools.clone())
                .sanitize_errors(self.sanitize_errors)
                .with_invocation_log(log),
        );

        builder.build()
    }

    fn build_help(&self) -> String {
        let mut doc = format!(
            "# {}\n\n{}\n\n**Version:** {}\n**Name:** `{}`\n**Locale:** `{}`\n\n## Parameters\n\n| Name | Type | Required | Default | Description |\n|------|------|----------|---------|-------------|\n| `commands` | string | yes | — | Bash script that may call the registered tool commands |\n| `timeout_ms` | integer | no | — | Per-call timeout in milliseconds |\n\n## Tool Commands\n\n| Name | Description | Usage |\n|------|-------------|-------|\n",
            self.display_name, self.description, VERSION, self.name, self.locale
        );

        for t in &self.tools {
            let usage = usage_from_schema(&t.def.input_schema)
                .map(|u| format!("`{} {}`", t.def.name, u))
                .unwrap_or_else(|| format!("`{}`", t.def.name));
            doc.push_str(&format!(
                "| `{}` | {} | {} |\n",
                t.def.name, t.def.description, usage
            ));
        }

        doc.push_str(
            "\n## Result\n\n| Field | Type | Description |\n|------|------|-------------|\n| `stdout` | string | Combined standard output |\n| `stderr` | string | Tool or bash errors |\n| `exit_code` | integer | Shell exit code |\n| `error` | string | Error category when execution fails |\n\n## Examples\n\n```json\n{\"commands\":\"get_user --id 42\"}\n```\n\n```json\n{\"commands\":\"user=$(get_user --id 42)\\necho \\\"$user\\\" | jq -r '.name'\"}\n```\n\n## Notes\n\n- Pass arguments as `--key value` or `--key=value`.\n- Shell logic, variables, loops, conditionals, pipes, heredocs, here-strings, and stdin-based transforms are available.\n- Filesystem primitives are unavailable: file commands, path script execution, file redirection, and process substitution are rejected.\n- Use `help <tool> --json` inside the tool for runtime schema inspection.\n",
        );

        doc
    }

    fn build_system_prompt(&self) -> String {
        let mut parts = vec![format!(
            "{}: {}.",
            self.name,
            localized(
                self.locale.as_str(),
                "run bash scripts that orchestrate registered tool commands",
                "виконує bash-скрипти для оркестрації зареєстрованих команд",
            )
        )];

        let tools = self
            .tools
            .iter()
            .map(|tool| {
                if self.compact_prompt {
                    format!("{} ({})", tool.def.name, tool.def.description)
                } else if let Some(usage) = usage_from_schema(&tool.def.input_schema) {
                    format!("{} [{}]", tool.def.name, usage)
                } else {
                    tool.def.name.clone()
                }
            })
            .collect::<Vec<_>>()
            .join(", ");
        parts.push(format!(
            "{}: {}.",
            localized(self.locale.as_str(), "Commands", "Команди"),
            tools
        ));
        parts.push(localized(
            self.locale.as_str(),
            "Pass args as --key value or --key=value. Shell logic and stdin pipelines are available; filesystem primitives are unavailable. Use help/discover builtins for runtime details.",
            "Передавайте аргументи як --key value або --key=value. Доступні логіка shell і stdin-конвеєри; файлові примітиви недоступні. Використовуйте help/discover для деталей.",
        ).to_string());

        parts.join(" ")
    }

    async fn run_request_with_stream(
        &self,
        req: ToolRequest,
        stream_sender: Option<tokio::sync::mpsc::UnboundedSender<ToolOutputChunk>>,
    ) -> ToolResponse {
        if req.commands.is_empty() {
            self.store_last_execution_trace(ScriptedExecutionTrace::default());
            return ToolResponse {
                stdout: String::new(),
                stderr: String::new(),
                exit_code: 0,
                error: None,
                ..Default::default()
            };
        }

        let timeout_ms = req.timeout_ms;
        let commands = req.commands;
        let log: InvocationLog = Arc::new(Mutex::new(VecDeque::new()));
        let mut bash = self.create_bash(Arc::clone(&log));

        let fut = async {
            let result = if let Some(sender) = stream_sender {
                let output_cb = Box::new(move |stdout_chunk: &str, stderr_chunk: &str| {
                    if !stdout_chunk.is_empty() {
                        let _ = sender.send(ToolOutputChunk {
                            data: serde_json::json!(stdout_chunk),
                            kind: "stdout".to_string(),
                        });
                    }
                    if !stderr_chunk.is_empty() {
                        let _ = sender.send(ToolOutputChunk {
                            data: serde_json::json!(stderr_chunk),
                            kind: "stderr".to_string(),
                        });
                    }
                });
                bash.exec_streaming(&commands, output_cb).await
            } else {
                bash.exec(&commands).await
            };

            match result {
                Ok(result) => result.into(),
                Err(err) => ToolResponse {
                    stdout: String::new(),
                    stderr: err.to_string(),
                    exit_code: 1,
                    error: Some(err.to_string()),
                    ..Default::default()
                },
            }
        };

        // Keep ScriptedTool on the shared ToolRequest contract: per-call
        // timeouts must abort the whole orchestration, including callbacks.
        // wasm32 has no timer driver (tokio::time::timeout panics), so there the
        // orchestration runs without wall-clock enforcement; the timer path
        // stays on native.
        #[cfg(not(target_family = "wasm"))]
        let response = if let Some(ms) = timeout_ms {
            let duration = Duration::from_millis(ms);
            match tokio::time::timeout(duration, fut).await {
                Ok(response) => response,
                Err(_) => timeout_response(duration),
            }
        } else {
            fut.await
        };
        #[cfg(target_family = "wasm")]
        let response = {
            let _ = timeout_ms;
            fut.await
        };

        let invocations: Vec<_> = log
            .lock()
            .expect("scripted invocation log poisoned")
            .iter()
            .cloned()
            .collect();
        self.store_last_execution_trace(ScriptedExecutionTrace { invocations });
        response
    }
}

// ============================================================================
// Tool trait implementation
// ============================================================================

#[async_trait]
impl Tool for ScriptedTool {
    fn name(&self) -> &str {
        &self.name
    }

    fn display_name(&self) -> &str {
        &self.display_name
    }

    fn short_description(&self) -> &str {
        &self.short_desc
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn help(&self) -> String {
        self.build_help()
    }

    fn system_prompt(&self) -> String {
        self.build_system_prompt()
    }

    fn locale(&self) -> &str {
        &self.locale
    }

    fn input_schema(&self) -> serde_json::Value {
        tool_request_schema()
    }

    fn output_schema(&self) -> serde_json::Value {
        tool_response_schema()
    }

    fn version(&self) -> &str {
        VERSION
    }

    fn execution(&self, args: serde_json::Value) -> std::result::Result<ToolExecution, ToolError> {
        let req = tool_request_from_value(self.locale(), args)?;
        let tool = self.clone();
        Ok(ToolExecution::new(move |stream_sender| async move {
            let start = crate::time_compat::Instant::now();
            let response = tool.run_request_with_stream(req, stream_sender).await;
            tool_output_from_response(response, start.elapsed())
        }))
    }

    async fn execute(&self, req: ToolRequest) -> ToolResponse {
        self.run_request_with_stream(req, None).await
    }

    async fn execute_with_status(
        &self,
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
                ..Default::default()
            };
        }

        status_callback(ToolStatus::new("parse").with_percent(10.0));
        status_callback(ToolStatus::new("execute").with_percent(20.0));
        let response = self.run_request_with_stream(req, None).await;

        status_callback(ToolStatus::new("complete").with_percent(100.0));
        response
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ToolArgs;
    use crate::ToolDef;
    use crate::tool_def::parse_flags;

    #[test]
    fn test_parse_flags_key_value() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "id": {"type": "integer"},
                "name": {"type": "string"}
            }
        });
        let args = vec!["--id".into(), "42".into(), "--name".into(), "Alice".into()];
        let result = parse_flags(&args, &schema).expect("parse_flags should succeed");
        assert_eq!(result["id"], 42);
        assert_eq!(result["name"], "Alice");
    }

    #[test]
    fn test_parse_flags_equals_syntax() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": { "id": {"type": "integer"} }
        });
        let args = vec!["--id=99".into()];
        let result = parse_flags(&args, &schema).expect("parse_flags should succeed");
        assert_eq!(result["id"], 99);
    }

    #[test]
    fn test_parse_flags_boolean() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "verbose": {"type": "boolean"},
                "query": {"type": "string"}
            }
        });
        let args = vec!["--verbose".into(), "--query".into(), "hello".into()];
        let result = parse_flags(&args, &schema).expect("parse_flags should succeed");
        assert_eq!(result["verbose"], true);
        assert_eq!(result["query"], "hello");
    }

    #[test]
    fn test_parse_flags_no_schema() {
        let schema = serde_json::json!({});
        let args = vec!["--name".into(), "Bob".into()];
        let result = parse_flags(&args, &schema).expect("parse_flags should succeed");
        assert_eq!(result["name"], "Bob");
    }

    #[test]
    fn test_parse_flags_empty() {
        let schema = serde_json::json!({});
        let result = parse_flags(&[], &schema).expect("parse_flags should succeed");
        assert_eq!(result, serde_json::json!({}));
    }

    #[test]
    fn test_parse_flags_rejects_positional() {
        let schema = serde_json::json!({});
        let result = parse_flags(&["42".into()], &schema);
        assert!(result.is_err());
        assert!(
            result
                .expect_err("should reject positional")
                .contains("expected --flag")
        );
    }

    #[test]
    fn test_usage_from_schema() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "id": {"type": "integer"},
                "name": {"type": "string"}
            }
        });
        let usage = usage_from_schema(&schema).expect("should produce usage string");
        assert!(usage.contains("--id <integer>"));
        assert!(usage.contains("--name <string>"));
    }

    #[test]
    fn test_usage_from_empty_schema() {
        assert!(usage_from_schema(&serde_json::json!({})).is_none());
        assert!(
            usage_from_schema(&serde_json::json!({"type": "object", "properties": {}})).is_none()
        );
    }

    // -- HelpBuiltin tests --

    fn build_help_test_tool() -> ScriptedTool {
        ScriptedTool::builder("test_api")
            .short_description("Test API")
            .tool_fn(
                ToolDef::new("get_user", "Fetch user by ID").with_schema(serde_json::json!({
                    "type": "object",
                    "properties": {
                        "id": {"type": "integer"}
                    }
                })),
                |_args: &ToolArgs| Ok("{\"id\":1}\n".to_string()),
            )
            .tool_fn(
                ToolDef::new("list_orders", "List orders for user").with_schema(
                    serde_json::json!({
                        "type": "object",
                        "properties": {
                            "user_id": {"type": "integer"},
                            "limit": {"type": "integer"}
                        }
                    }),
                ),
                |_args: &ToolArgs| Ok("[]\n".to_string()),
            )
            .build()
    }

    #[tokio::test]
    async fn test_scripted_tool_execute_honors_timeout_ms() {
        let tool = ScriptedTool::builder("timeout_test")
            .async_tool_fn(
                ToolDef::new("slow", "Slow async tool"),
                |_args: ToolArgs| async move {
                    tokio::time::sleep(Duration::from_secs(10)).await;
                    Ok("done\n".to_string())
                },
            )
            .build();
        let start = crate::time_compat::Instant::now();

        let resp = tool
            .execute(ToolRequest {
                commands: "slow".to_string(),
                timeout_ms: Some(50),
            })
            .await;

        assert_eq!(resp.exit_code, 124);
        assert_eq!(resp.error, Some("timeout".to_string()));
        assert!(resp.stderr.contains("timed out"));
        assert!(
            start.elapsed() < Duration::from_secs(2),
            "timeout_ms should abort before sleep completes"
        );
    }

    #[tokio::test]
    async fn test_scripted_tool_execute_with_status_honors_timeout_ms() {
        let tool = ScriptedTool::builder("timeout_status_test")
            .async_tool_fn(
                ToolDef::new("slow", "Slow async tool"),
                |_args: ToolArgs| async move {
                    tokio::time::sleep(Duration::from_secs(10)).await;
                    Ok("done\n".to_string())
                },
            )
            .build();
        let start = crate::time_compat::Instant::now();

        let resp = tool
            .execute_with_status(
                ToolRequest {
                    commands: "slow".to_string(),
                    timeout_ms: Some(50),
                },
                Box::new(|_| {}),
            )
            .await;

        assert_eq!(resp.exit_code, 124);
        assert_eq!(resp.error, Some("timeout".to_string()));
        assert!(resp.stderr.contains("timed out"));
        assert!(
            start.elapsed() < Duration::from_secs(2),
            "timeout_ms should abort before sleep completes"
        );
    }

    #[tokio::test]
    async fn test_help_list() {
        let tool = build_help_test_tool();
        let resp = tool
            .execute(ToolRequest {
                commands: "help --list".to_string(),
                timeout_ms: None,
            })
            .await;
        assert_eq!(resp.exit_code, 0);
        assert!(resp.stdout.contains("get_user"));
        assert!(resp.stdout.contains("Fetch user by ID"));
        assert!(resp.stdout.contains("list_orders"));
    }

    #[tokio::test]
    async fn test_help_tool_human_readable() {
        let tool = build_help_test_tool();
        let resp = tool
            .execute(ToolRequest {
                commands: "help get_user".to_string(),
                timeout_ms: None,
            })
            .await;
        assert_eq!(resp.exit_code, 0);
        assert!(resp.stdout.contains("get_user - Fetch user by ID"));
        assert!(resp.stdout.contains("--id <integer>"));
    }

    #[tokio::test]
    async fn test_help_tool_json() {
        let tool = build_help_test_tool();
        let resp = tool
            .execute(ToolRequest {
                commands: "help get_user --json".to_string(),
                timeout_ms: None,
            })
            .await;
        assert_eq!(resp.exit_code, 0);
        let parsed: serde_json::Value =
            serde_json::from_str(resp.stdout.trim()).expect("should be valid JSON");
        assert_eq!(parsed["name"], "get_user");
        assert_eq!(parsed["description"], "Fetch user by ID");
        assert!(parsed["input_schema"]["properties"]["id"].is_object());
    }

    #[tokio::test]
    async fn test_help_unknown_tool() {
        let tool = build_help_test_tool();
        let resp = tool
            .execute(ToolRequest {
                commands: "help nonexistent".to_string(),
                timeout_ms: None,
            })
            .await;
        assert_ne!(resp.exit_code, 0);
        assert!(resp.stderr.contains("unknown tool"));
    }

    #[tokio::test]
    async fn test_help_no_args_lists_all() {
        let tool = build_help_test_tool();
        let resp = tool
            .execute(ToolRequest {
                commands: "help".to_string(),
                timeout_ms: None,
            })
            .await;
        assert_eq!(resp.exit_code, 0);
        assert!(resp.stdout.contains("get_user"));
        assert!(resp.stdout.contains("list_orders"));
    }

    #[tokio::test]
    async fn test_help_json_pipe_jq() {
        let tool = build_help_test_tool();
        let resp = tool
            .execute(ToolRequest {
                commands: "help get_user --json | jq -r '.name'".to_string(),
                timeout_ms: None,
            })
            .await;
        assert_eq!(resp.exit_code, 0);
        assert_eq!(resp.stdout.trim(), "get_user");
    }

    #[tokio::test]
    async fn test_compact_prompt_omits_usage() {
        let tool = ScriptedTool::builder("compact_test")
            .compact_prompt(true)
            .tool_fn(
                ToolDef::new("get_user", "Fetch user").with_schema(serde_json::json!({
                    "type": "object",
                    "properties": { "id": {"type": "integer"} }
                })),
                |_args: &ToolArgs| Ok("ok\n".to_string()),
            )
            .build();
        let sp = tool.system_prompt();
        assert!(sp.contains("help/discover"));
        assert!(!sp.contains("Usage:"));
    }

    #[tokio::test]
    async fn test_non_compact_prompt_has_usage() {
        let tool = ScriptedTool::builder("full_test")
            .tool_fn(
                ToolDef::new("get_user", "Fetch user").with_schema(serde_json::json!({
                    "type": "object",
                    "properties": { "id": {"type": "integer"} }
                })),
                |_args: &ToolArgs| Ok("ok\n".to_string()),
            )
            .build();
        let sp = tool.system_prompt();
        assert!(sp.contains("--id <integer>"));
    }

    #[tokio::test]
    async fn test_error_uses_display_not_debug() {
        use super::ScriptedTool;
        use crate::ToolDef;
        use crate::tool::Tool;

        let tool = ScriptedTool::builder("test")
            .short_description("test")
            .tool_fn(ToolDef::new("fail", "Always fails"), |_args: &ToolArgs| {
                Err("service error".to_string())
            })
            .build();
        let req = ToolRequest {
            commands: "fail".into(),
            timeout_ms: None,
        };
        let resp = tool.execute(req).await;
        // Error messages use Display format, not Debug, to avoid leaking internals
        if let Some(ref err) = resp.error {
            assert!(
                !err.contains("Execution("),
                "error should use Display not Debug: {err}",
            );
        }
    }

    // -- DiscoverBuiltin tests --

    fn build_discover_test_tool() -> ScriptedTool {
        ScriptedTool::builder("big_api")
            .short_description("Big API")
            .tool_fn(
                ToolDef::new("create_charge", "Create a payment charge")
                    .with_category("payments")
                    .with_tags(&["billing", "write"]),
                |_args: &ToolArgs| Ok("ok\n".to_string()),
            )
            .tool_fn(
                ToolDef::new("refund", "Issue a refund")
                    .with_category("payments")
                    .with_tags(&["billing", "write"]),
                |_args: &ToolArgs| Ok("ok\n".to_string()),
            )
            .tool_fn(
                ToolDef::new("get_user", "Fetch user by ID")
                    .with_category("users")
                    .with_tags(&["read"]),
                |_args: &ToolArgs| Ok("ok\n".to_string()),
            )
            .tool_fn(
                ToolDef::new("delete_user", "Delete a user account")
                    .with_category("users")
                    .with_tags(&["admin", "write"]),
                |_args: &ToolArgs| Ok("ok\n".to_string()),
            )
            .tool_fn(
                ToolDef::new("get_inventory", "Check inventory levels").with_category("inventory"),
                |_args: &ToolArgs| Ok("ok\n".to_string()),
            )
            .build()
    }

    #[tokio::test]
    async fn test_discover_categories() {
        let tool = build_discover_test_tool();
        let resp = tool
            .execute(ToolRequest {
                commands: "discover --categories".to_string(),
                timeout_ms: None,
            })
            .await;
        assert_eq!(resp.exit_code, 0);
        assert!(resp.stdout.contains("payments (2 tools)"));
        assert!(resp.stdout.contains("users (2 tools)"));
        assert!(resp.stdout.contains("inventory (1 tool)"));
    }

    #[tokio::test]
    async fn test_discover_category_filter() {
        let tool = build_discover_test_tool();
        let resp = tool
            .execute(ToolRequest {
                commands: "discover --category payments".to_string(),
                timeout_ms: None,
            })
            .await;
        assert_eq!(resp.exit_code, 0);
        assert!(resp.stdout.contains("create_charge"));
        assert!(resp.stdout.contains("refund"));
        assert!(!resp.stdout.contains("get_user"));
    }

    #[tokio::test]
    async fn test_discover_tag_filter() {
        let tool = build_discover_test_tool();
        let resp = tool
            .execute(ToolRequest {
                commands: "discover --tag admin".to_string(),
                timeout_ms: None,
            })
            .await;
        assert_eq!(resp.exit_code, 0);
        assert!(resp.stdout.contains("delete_user"));
        assert!(!resp.stdout.contains("create_charge"));
    }

    #[tokio::test]
    async fn test_discover_search() {
        let tool = build_discover_test_tool();
        let resp = tool
            .execute(ToolRequest {
                commands: "discover --search user".to_string(),
                timeout_ms: None,
            })
            .await;
        assert_eq!(resp.exit_code, 0);
        assert!(resp.stdout.contains("get_user"));
        assert!(resp.stdout.contains("delete_user"));
        assert!(!resp.stdout.contains("create_charge"));
    }

    #[tokio::test]
    async fn test_discover_search_case_insensitive() {
        let tool = build_discover_test_tool();
        let resp = tool
            .execute(ToolRequest {
                commands: "discover --search REFUND".to_string(),
                timeout_ms: None,
            })
            .await;
        assert_eq!(resp.exit_code, 0);
        assert!(resp.stdout.contains("refund"));
    }

    #[tokio::test]
    async fn test_discover_categories_json() {
        let tool = build_discover_test_tool();
        let resp = tool
            .execute(ToolRequest {
                commands: "discover --categories --json".to_string(),
                timeout_ms: None,
            })
            .await;
        assert_eq!(resp.exit_code, 0);
        let arr: Vec<serde_json::Value> =
            serde_json::from_str(resp.stdout.trim()).expect("valid JSON");
        assert!(
            arr.iter()
                .any(|v| v["category"] == "payments" && v["count"] == 2)
        );
    }

    #[tokio::test]
    async fn test_discover_category_json() {
        let tool = build_discover_test_tool();
        let resp = tool
            .execute(ToolRequest {
                commands: "discover --category payments --json".to_string(),
                timeout_ms: None,
            })
            .await;
        assert_eq!(resp.exit_code, 0);
        let arr: Vec<serde_json::Value> =
            serde_json::from_str(resp.stdout.trim()).expect("valid JSON");
        assert_eq!(arr.len(), 2);
        assert!(arr.iter().any(|v| v["name"] == "create_charge"));
    }

    #[tokio::test]
    async fn test_discover_no_args_shows_usage() {
        let tool = build_discover_test_tool();
        let resp = tool
            .execute(ToolRequest {
                commands: "discover".to_string(),
                timeout_ms: None,
            })
            .await;
        assert_ne!(resp.exit_code, 0);
        assert!(resp.stderr.contains("usage:"));
    }

    #[tokio::test]
    async fn test_discover_tag_json() {
        let tool = build_discover_test_tool();
        let resp = tool
            .execute(ToolRequest {
                commands: "discover --tag billing --json".to_string(),
                timeout_ms: None,
            })
            .await;
        assert_eq!(resp.exit_code, 0);
        let arr: Vec<serde_json::Value> =
            serde_json::from_str(resp.stdout.trim()).expect("valid JSON");
        assert_eq!(arr.len(), 2);
        assert!(arr.iter().all(|v| {
            v["tags"]
                .as_array()
                .expect("tags array")
                .contains(&serde_json::json!("billing"))
        }));
    }

    #[tokio::test]
    async fn test_tooldef_with_tags_and_category() {
        let def = ToolDef::new("test", "A test tool")
            .with_tags(&["admin", "billing"])
            .with_category("payments");
        assert_eq!(def.tags, vec!["admin", "billing"]);
        assert_eq!(def.category.as_deref(), Some("payments"));
    }

    // THREAT[TM-INF-030]: Callback error sanitization tests

    #[tokio::test]
    async fn test_callback_error_sanitized_by_default() {
        let tool = ScriptedTool::builder("api")
            .tool_fn(ToolDef::new("fail", "Always fails"), |_args: &ToolArgs| {
                Err("connection failed: postgres://admin:secret@internal-db:5432/prod".into())
            })
            .build();
        let resp = tool
            .execute(ToolRequest {
                commands: "fail".to_string(),
                timeout_ms: None,
            })
            .await;
        assert_ne!(resp.exit_code, 0);
        // Internal details must NOT appear in output
        assert!(
            !resp.stderr.contains("postgres://"),
            "internal details leaked: {}",
            resp.stderr
        );
        assert!(resp.stderr.contains("callback failed"));
    }

    #[tokio::test]
    async fn test_callback_error_unsanitized_when_disabled() {
        let tool = ScriptedTool::builder("api")
            .sanitize_errors(false)
            .tool_fn(ToolDef::new("fail", "Always fails"), |_args: &ToolArgs| {
                Err("connection failed: postgres://admin:secret@internal-db:5432/prod".into())
            })
            .build();
        let resp = tool
            .execute(ToolRequest {
                commands: "fail".to_string(),
                timeout_ms: None,
            })
            .await;
        assert_ne!(resp.exit_code, 0);
        // With sanitization disabled, full error should appear
        assert!(resp.stderr.contains("postgres://"));
    }
}
