//! ScriptedTool execution: Tool impl, builtin adapter, flag parser, documentation helpers.

use super::{ScriptedTool, ToolArgs, ToolCallback};
use crate::Bash;
use crate::builtins::{Builtin, Context};
use crate::error::Result;
use crate::interpreter::ExecResult;
use crate::tool::{Tool, ToolRequest, ToolResponse, ToolStatus, VERSION};
use async_trait::async_trait;
use schemars::schema_for;
use std::sync::Arc;

// ============================================================================
// Flag parser — `--key value` / `--key=value` → JSON object
// ============================================================================

/// Parse `--key value` and `--key=value` flags into a JSON object.
/// Types are coerced according to the schema's property definitions.
/// Unknown flags (not in schema) are kept as strings.
/// Bare `--flag` without a value is treated as `true` if the schema says boolean,
/// otherwise as `true` when the next arg also starts with `--` or is absent.
fn parse_flags(
    raw_args: &[String],
    schema: &serde_json::Value,
) -> std::result::Result<serde_json::Value, String> {
    let properties = schema
        .get("properties")
        .and_then(|p| p.as_object())
        .cloned()
        .unwrap_or_default();

    let mut result = serde_json::Map::new();
    let mut i = 0;

    while i < raw_args.len() {
        let arg = &raw_args[i];

        let Some(flag) = arg.strip_prefix("--") else {
            return Err(format!("expected --flag, got: {arg}"));
        };

        // --key=value
        if let Some((key, raw_value)) = flag.split_once('=') {
            let value = coerce_value(raw_value, properties.get(key));
            result.insert(key.to_string(), value);
            i += 1;
            continue;
        }

        // --flag (boolean) or --key value
        let key = flag;
        let prop_schema = properties.get(key);
        let is_boolean = prop_schema
            .and_then(|s| s.get("type"))
            .and_then(|t| t.as_str())
            == Some("boolean");

        if is_boolean {
            result.insert(key.to_string(), serde_json::Value::Bool(true));
            i += 1;
        } else if i + 1 < raw_args.len() && !raw_args[i + 1].starts_with("--") {
            let raw_value = &raw_args[i + 1];
            let value = coerce_value(raw_value, prop_schema);
            result.insert(key.to_string(), value);
            i += 2;
        } else {
            // No value follows and not boolean — treat as true
            result.insert(key.to_string(), serde_json::Value::Bool(true));
            i += 1;
        }
    }

    Ok(serde_json::Value::Object(result))
}

/// Coerce a raw string value to the type declared in the property schema.
fn coerce_value(raw: &str, prop_schema: Option<&serde_json::Value>) -> serde_json::Value {
    let type_str = prop_schema
        .and_then(|s| s.get("type"))
        .and_then(|t| t.as_str())
        .unwrap_or("string");

    match type_str {
        "integer" => raw
            .parse::<i64>()
            .map(serde_json::Value::from)
            .unwrap_or_else(|_| serde_json::Value::String(raw.to_string())),
        "number" => raw
            .parse::<f64>()
            .map(|n| serde_json::json!(n))
            .unwrap_or_else(|_| serde_json::Value::String(raw.to_string())),
        "boolean" => match raw {
            "true" | "1" | "yes" => serde_json::Value::Bool(true),
            "false" | "0" | "no" => serde_json::Value::Bool(false),
            _ => serde_json::Value::String(raw.to_string()),
        },
        _ => serde_json::Value::String(raw.to_string()),
    }
}

/// Generate a usage hint from schema properties: `--id <integer> --name <string>`.
fn usage_from_schema(schema: &serde_json::Value) -> Option<String> {
    let props = schema.get("properties")?.as_object()?;
    if props.is_empty() {
        return None;
    }
    let flags: Vec<String> = props
        .iter()
        .map(|(key, prop)| {
            let ty = prop.get("type").and_then(|t| t.as_str()).unwrap_or("value");
            format!("--{key} <{ty}>")
        })
        .collect();
    Some(flags.join(" "))
}

// ============================================================================
// ToolBuiltinAdapter — wraps ToolCallback as a Builtin
// ============================================================================

/// Adapts a [`ToolCallback`] into a [`Builtin`] so the interpreter can execute it.
/// Parses `--key value` flags from `ctx.args` using the schema for type coercion.
struct ToolBuiltinAdapter {
    callback: ToolCallback,
    schema: serde_json::Value,
}

#[async_trait]
impl Builtin for ToolBuiltinAdapter {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let params = match parse_flags(ctx.args, &self.schema) {
            Ok(p) => p,
            Err(msg) => return Ok(ExecResult::err(msg, 2)),
        };

        let tool_args = ToolArgs {
            params,
            stdin: ctx.stdin.map(String::from),
        };

        match (self.callback)(&tool_args) {
            Ok(stdout) => Ok(ExecResult::ok(stdout)),
            Err(msg) => Ok(ExecResult::err(msg, 1)),
        }
    }
}

// ============================================================================
// HelpBuiltin — runtime schema introspection
// ============================================================================

/// Snapshot of a tool definition for the `help` builtin.
#[derive(Clone)]
struct ToolDefSnapshot {
    name: String,
    description: String,
    input_schema: serde_json::Value,
}

/// Built-in `help` command for runtime tool schema introspection.
///
/// Modes:
/// - `help --list` — list all tool names + descriptions
/// - `help <tool>` — human-readable usage
/// - `help <tool> --json` — machine-readable JSON schema
struct HelpBuiltin {
    tools: Vec<ToolDefSnapshot>,
}

#[async_trait]
impl Builtin for HelpBuiltin {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let args = ctx.args;

        if args.is_empty() || (args.len() == 1 && args[0] == "--list") {
            // List all tools
            let mut out = String::new();
            for t in &self.tools {
                out.push_str(&format!("{:<20} {}\n", t.name, t.description));
            }
            return Ok(ExecResult::ok(out));
        }

        // Find the tool name (first non-flag arg)
        let tool_name = args.iter().find(|a| !a.starts_with("--"));
        let json_mode = args.iter().any(|a| a == "--json");

        let Some(tool_name) = tool_name else {
            return Ok(ExecResult::err(
                "usage: help [--list] [<tool>] [--json]".to_string(),
                1,
            ));
        };

        let Some(tool) = self.tools.iter().find(|t| t.name == *tool_name) else {
            return Ok(ExecResult::err(
                format!("help: unknown tool: {tool_name}"),
                1,
            ));
        };

        if json_mode {
            // Machine-readable JSON output
            let obj = serde_json::json!({
                "name": tool.name,
                "description": tool.description,
                "input_schema": tool.input_schema,
            });
            let json_str = serde_json::to_string_pretty(&obj).unwrap_or_default();
            return Ok(ExecResult::ok(format!("{json_str}\n")));
        }

        // Human-readable output
        let mut out = format!("{} - {}\n", tool.name, tool.description);
        if let Some(usage) = usage_from_schema(&tool.input_schema) {
            out.push_str(&format!("Usage: {} {}\n", tool.name, usage));
        }
        Ok(ExecResult::ok(out))
    }
}

// ============================================================================
// ScriptedTool — internal helpers
// ============================================================================

impl ScriptedTool {
    /// Create a fresh Bash instance with all tool builtins registered.
    fn create_bash(&self) -> Bash {
        let mut builder = Bash::builder();

        if let Some(ref limits) = self.limits {
            builder = builder.limits(limits.clone());
        }
        for (key, value) in &self.env_vars {
            builder = builder.env(key, value);
        }
        for tool in &self.tools {
            let name = tool.def.name.clone();
            let builtin: Box<dyn Builtin> = Box::new(ToolBuiltinAdapter {
                callback: Arc::clone(&tool.callback),
                schema: tool.def.input_schema.clone(),
            });
            builder = builder.builtin(name, builtin);
        }

        // Register the help builtin
        let snapshots: Vec<ToolDefSnapshot> = self
            .tools
            .iter()
            .map(|t| ToolDefSnapshot {
                name: t.def.name.clone(),
                description: t.def.description.clone(),
                input_schema: t.def.input_schema.clone(),
            })
            .collect();
        builder = builder.builtin(
            "help".to_string(),
            Box::new(HelpBuiltin { tools: snapshots }),
        );

        builder.build()
    }

    fn build_description(&self) -> String {
        let mut desc = format!(
            "Scripted tool. Available tool-commands: {}",
            self.tools
                .iter()
                .map(|t| t.def.name.as_str())
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
             \x20      conditionals to orchestrate multiple tool calls in one request.\n\
             \x20      Arguments are passed as --key value or --key=value flags.\n\n\
             TOOL COMMANDS\n",
            name = self.name,
            short_desc = self.short_desc,
        );

        for t in &self.tools {
            let usage = usage_from_schema(&t.def.input_schema)
                .map(|u| format!(" ({})", u))
                .unwrap_or_default();
            doc.push_str(&format!(
                "       {:<20} {}{}\n",
                t.def.name, t.def.description, usage
            ));
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
             \x20          {{\"commands\": \"get_user --id 42\"}}\n\n\
             \x20      Pipeline:\n\
             \x20          {{\"commands\": \"get_user --id 42 | jq '.name'\"}}\n\n\
             \x20      Multi-step orchestration:\n\
             \x20          {{\"commands\": \"user=$(get_user --id 42)\\necho $user | jq -r '.name'\"}}\n",
        );

        doc
    }

    fn build_system_prompt(&self) -> String {
        let mut prompt = format!("# {}\n\n", self.name);
        prompt.push_str(&format!("{}\n\n", self.short_desc));

        prompt.push_str("Input: {\"commands\": \"<bash script>\"}\n");
        prompt.push_str("Output: {stdout, stderr, exit_code}\n\n");

        prompt.push_str("## Available tool commands\n\n");

        if self.compact_prompt {
            // Compact mode: names + one-liners, defer details to `help`
            for t in &self.tools {
                prompt.push_str(&format!(
                    "- `{}`: {} (use `help {}` for params)\n",
                    t.def.name, t.def.description, t.def.name
                ));
            }
        } else {
            // Full mode: include usage hints from schema
            for t in &self.tools {
                prompt.push_str(&format!("- `{}`: {}\n", t.def.name, t.def.description));
                if let Some(usage) = usage_from_schema(&t.def.input_schema) {
                    prompt.push_str(&format!("  Usage: `{} {}`\n", t.def.name, usage));
                }
            }
        }

        prompt.push_str(
            "\n## Tips\n\n\
             - Pass arguments as `--key value` or `--key=value` flags\n\
             - Pipe tool output through `jq` for JSON processing\n\
             - Use variables to pass data between tool calls\n\
             - Use `set -e` to stop on first error\n\
             - Standard builtins (echo, grep, sed, awk, etc.) are available\n",
        );

        if self.compact_prompt {
            prompt.push_str(
                "- Use `help <tool>` for full usage, `help <tool> --json` for schema\n\
                 - Use `help --list` to see all available tools\n",
            );
        }

        prompt
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
                error: Some(e.to_string()),
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
                error: Some(e.to_string()),
            },
        };

        status_callback(ToolStatus::new("complete").with_percent(100.0));
        response
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ToolDef;

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
            .tool(
                ToolDef::new("get_user", "Fetch user by ID").with_schema(serde_json::json!({
                    "type": "object",
                    "properties": {
                        "id": {"type": "integer"}
                    }
                })),
                |_args: &super::ToolArgs| Ok("{\"id\":1}\n".to_string()),
            )
            .tool(
                ToolDef::new("list_orders", "List orders for user").with_schema(
                    serde_json::json!({
                        "type": "object",
                        "properties": {
                            "user_id": {"type": "integer"},
                            "limit": {"type": "integer"}
                        }
                    }),
                ),
                |_args: &super::ToolArgs| Ok("[]\n".to_string()),
            )
            .build()
    }

    #[tokio::test]
    async fn test_help_list() {
        let mut tool = build_help_test_tool();
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
        let mut tool = build_help_test_tool();
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
        let mut tool = build_help_test_tool();
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
        let mut tool = build_help_test_tool();
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
        let mut tool = build_help_test_tool();
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
        let mut tool = build_help_test_tool();
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
            .tool(
                ToolDef::new("get_user", "Fetch user").with_schema(serde_json::json!({
                    "type": "object",
                    "properties": { "id": {"type": "integer"} }
                })),
                |_args: &super::ToolArgs| Ok("ok\n".to_string()),
            )
            .build();
        let sp = tool.system_prompt();
        assert!(sp.contains("use `help get_user` for params"));
        assert!(!sp.contains("Usage: `get_user --id <integer>`"));
        assert!(sp.contains("help <tool> --json"));
    }

    #[tokio::test]
    async fn test_non_compact_prompt_has_usage() {
        let tool = ScriptedTool::builder("full_test")
            .tool(
                ToolDef::new("get_user", "Fetch user").with_schema(serde_json::json!({
                    "type": "object",
                    "properties": { "id": {"type": "integer"} }
                })),
                |_args: &super::ToolArgs| Ok("ok\n".to_string()),
            )
            .build();
        let sp = tool.system_prompt();
        assert!(sp.contains("Usage: `get_user --id <integer>`"));
        assert!(!sp.contains("use `help get_user` for params"));
    }

    #[tokio::test]
    async fn test_error_uses_display_not_debug() {
        use super::ScriptedTool;
        use crate::ToolDef;
        use crate::tool::Tool;

        let mut tool = ScriptedTool::builder("test")
            .short_description("test")
            .tool(
                ToolDef::new("fail", "Always fails"),
                |_args: &super::ToolArgs| Err("service error".to_string()),
            )
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
}
