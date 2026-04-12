//! ScriptedTool execution: Tool impl, builtin adapter, flag parser, documentation helpers.

use super::{
    ScriptedCommandInvocation, ScriptedCommandKind, ScriptedExecutionTrace, ScriptedTool, ToolArgs,
    ToolCallback,
};
use crate::Bash;
use crate::builtins::{Builtin, Context};
use crate::error::Result;
use crate::interpreter::ExecResult;
use crate::tool::{
    Tool, ToolError, ToolExecution, ToolOutputChunk, ToolRequest, ToolResponse, ToolStatus,
    VERSION, localized, tool_output_from_response, tool_request_from_value,
};
use async_trait::async_trait;
use schemars::schema_for;
use std::sync::{Arc, Mutex};

type InvocationLog = Arc<Mutex<Vec<ScriptedCommandInvocation>>>;

fn push_invocation(
    log: &InvocationLog,
    name: &str,
    kind: ScriptedCommandKind,
    args: &[String],
    exit_code: i32,
) {
    let mut invocations = log.lock().expect("scripted invocation log poisoned");
    invocations.push(ScriptedCommandInvocation {
        name: name.to_string(),
        kind,
        args: args.to_vec(),
        exit_code,
    });
}

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
    name: String,
    callback: ToolCallback,
    schema: serde_json::Value,
    log: InvocationLog,
    sanitize_errors: bool,
}

#[async_trait]
impl Builtin for ToolBuiltinAdapter {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let exit_result = match parse_flags(ctx.args, &self.schema) {
            Ok(params) => {
                let tool_args = ToolArgs {
                    params,
                    stdin: ctx.stdin.map(String::from),
                };

                match (self.callback)(&tool_args) {
                    Ok(stdout) => ExecResult::ok(stdout),
                    Err(msg) => {
                        // THREAT[TM-INF-030]: Sanitize callback errors to prevent
                        // leaking internal details (connection strings, file paths,
                        // stack traces) in tool output visible to LLM agents.
                        if self.sanitize_errors {
                            #[cfg(feature = "tracing")]
                            tracing::debug!(
                                tool = %self.name,
                                error = %msg,
                                "tool callback error (sanitized)"
                            );
                            ExecResult::err(format!("{}: callback failed\n", self.name), 1)
                        } else {
                            ExecResult::err(msg, 1)
                        }
                    }
                }
            }
            Err(msg) => ExecResult::err(msg, 2),
        };

        push_invocation(
            &self.log,
            &self.name,
            ScriptedCommandKind::Tool,
            ctx.args,
            exit_result.exit_code,
        );
        Ok(exit_result)
    }
}

// ============================================================================
// HelpBuiltin — runtime schema introspection
// ============================================================================

/// Snapshot of a tool definition for the `help` and `discover` builtins.
#[derive(Clone)]
struct ToolDefSnapshot {
    name: String,
    description: String,
    input_schema: serde_json::Value,
    tags: Vec<String>,
    category: Option<String>,
}

/// Built-in `help` command for runtime tool schema introspection.
///
/// Modes:
/// - `help --list` — list all tool names + descriptions
/// - `help <tool>` — human-readable usage
/// - `help <tool> --json` — machine-readable JSON schema
struct HelpBuiltin {
    tools: Vec<ToolDefSnapshot>,
    log: InvocationLog,
}

#[async_trait]
impl Builtin for HelpBuiltin {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let args = ctx.args;

        let result = if args.is_empty() || (args.len() == 1 && args[0] == "--list") {
            // List all tools
            let mut out = String::new();
            for t in &self.tools {
                out.push_str(&format!("{:<20} {}\n", t.name, t.description));
            }
            ExecResult::ok(out)
        } else {
            // Find the tool name (first non-flag arg)
            let tool_name = args.iter().find(|a| !a.starts_with("--"));
            let json_mode = args.iter().any(|a| a == "--json");

            let Some(tool_name) = tool_name else {
                let result =
                    ExecResult::err("usage: help [--list] [<tool>] [--json]".to_string(), 1);
                push_invocation(
                    &self.log,
                    "help",
                    ScriptedCommandKind::Help,
                    args,
                    result.exit_code,
                );
                return Ok(result);
            };

            let Some(tool) = self.tools.iter().find(|t| t.name == *tool_name) else {
                let result = ExecResult::err(format!("help: unknown tool: {tool_name}"), 1);
                push_invocation(
                    &self.log,
                    "help",
                    ScriptedCommandKind::Help,
                    args,
                    result.exit_code,
                );
                return Ok(result);
            };

            if json_mode {
                // Machine-readable JSON output
                let obj = serde_json::json!({
                    "name": tool.name,
                    "description": tool.description,
                    "input_schema": tool.input_schema,
                });
                let json_str = serde_json::to_string_pretty(&obj).unwrap_or_default();
                ExecResult::ok(format!("{json_str}\n"))
            } else {
                // Human-readable output
                let mut out = format!("{} - {}\n", tool.name, tool.description);
                if let Some(usage) = usage_from_schema(&tool.input_schema) {
                    out.push_str(&format!("Usage: {} {}\n", tool.name, usage));
                }
                ExecResult::ok(out)
            }
        };

        push_invocation(
            &self.log,
            "help",
            ScriptedCommandKind::Help,
            args,
            result.exit_code,
        );
        Ok(result)
    }
}

// ============================================================================
// DiscoverBuiltin — progressive tool discovery
// ============================================================================

/// Built-in `discover` command for exploring large tool sets.
struct DiscoverBuiltin {
    tools: Vec<ToolDefSnapshot>,
    log: InvocationLog,
}

impl DiscoverBuiltin {
    fn filter_tools(&self, args: &[String]) -> (Vec<&ToolDefSnapshot>, bool) {
        let json_mode = args.iter().any(|a| a == "--json");

        if args.iter().any(|a| a == "--categories") {
            return (Vec::new(), json_mode);
        }

        if let Some(pos) = args.iter().position(|a| a == "--category") {
            let cat = args.get(pos + 1).map(|s| s.as_str()).unwrap_or("");
            let filtered: Vec<&ToolDefSnapshot> = self
                .tools
                .iter()
                .filter(|t| t.category.as_deref() == Some(cat))
                .collect();
            return (filtered, json_mode);
        }

        if let Some(pos) = args.iter().position(|a| a == "--tag") {
            let tag = args.get(pos + 1).map(|s| s.as_str()).unwrap_or("");
            let filtered: Vec<&ToolDefSnapshot> = self
                .tools
                .iter()
                .filter(|t| t.tags.iter().any(|tg| tg == tag))
                .collect();
            return (filtered, json_mode);
        }

        if let Some(pos) = args.iter().position(|a| a == "--search") {
            let keyword = args
                .get(pos + 1)
                .map(|s| s.to_lowercase())
                .unwrap_or_default();
            let filtered: Vec<&ToolDefSnapshot> = self
                .tools
                .iter()
                .filter(|t| {
                    t.name.to_lowercase().contains(&keyword)
                        || t.description.to_lowercase().contains(&keyword)
                })
                .collect();
            return (filtered, json_mode);
        }

        (self.tools.iter().collect(), json_mode)
    }
}

#[async_trait]
impl Builtin for DiscoverBuiltin {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let args = ctx.args;

        let result = if args.is_empty() {
            ExecResult::err(
                "usage: discover --categories | --category <name> | --tag <tag> | --search <keyword> [--json]".to_string(),
                1,
            )
        } else {
            let json_mode = args.iter().any(|a| a == "--json");

            // --categories
            if args.iter().any(|a| a == "--categories") {
                let mut cats: std::collections::BTreeMap<String, usize> =
                    std::collections::BTreeMap::new();
                for t in &self.tools {
                    if let Some(ref cat) = t.category {
                        *cats.entry(cat.clone()).or_insert(0) += 1;
                    }
                }
                if json_mode {
                    let arr: Vec<serde_json::Value> = cats
                        .iter()
                        .map(|(name, count)| serde_json::json!({"category": name, "count": count}))
                        .collect();
                    let json_str =
                        serde_json::to_string_pretty(&arr).unwrap_or_else(|_| "[]".to_string());
                    ExecResult::ok(format!("{json_str}\n"))
                } else {
                    let mut out = String::new();
                    for (name, count) in &cats {
                        let plural = if *count == 1 { "tool" } else { "tools" };
                        out.push_str(&format!("{name} ({count} {plural})\n"));
                    }
                    ExecResult::ok(out)
                }
            } else {
                let (filtered, _) = self.filter_tools(args);

                if json_mode {
                    let arr: Vec<serde_json::Value> = filtered
                        .iter()
                        .map(|t| {
                            let mut obj = serde_json::json!({
                                "name": t.name,
                                "description": t.description,
                            });
                            if !t.tags.is_empty() {
                                obj["tags"] = serde_json::json!(t.tags);
                            }
                            if let Some(ref cat) = t.category {
                                obj["category"] = serde_json::json!(cat);
                            }
                            obj
                        })
                        .collect();
                    let json_str =
                        serde_json::to_string_pretty(&arr).unwrap_or_else(|_| "[]".to_string());
                    ExecResult::ok(format!("{json_str}\n"))
                } else {
                    let mut out = String::new();
                    for t in &filtered {
                        out.push_str(&format!("{:<20} {}\n", t.name, t.description));
                    }
                    ExecResult::ok(out)
                }
            }
        };

        push_invocation(
            &self.log,
            "discover",
            ScriptedCommandKind::Discover,
            args,
            result.exit_code,
        );
        Ok(result)
    }
}

// ============================================================================
// ScriptedTool — internal helpers
// ============================================================================

impl ScriptedTool {
    /// Create a fresh Bash instance with all tool builtins registered.
    fn create_bash(&self, log: InvocationLog) -> Bash {
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
                name: name.clone(),
                callback: Arc::clone(&tool.callback),
                schema: tool.def.input_schema.clone(),
                log: Arc::clone(&log),
                sanitize_errors: self.sanitize_errors,
            });
            builder = builder.builtin(name, builtin);
        }

        // Register the help and discover builtins
        let snapshots: Vec<ToolDefSnapshot> = self
            .tools
            .iter()
            .map(|t| ToolDefSnapshot {
                name: t.def.name.clone(),
                description: t.def.description.clone(),
                input_schema: t.def.input_schema.clone(),
                tags: t.def.tags.clone(),
                category: t.def.category.clone(),
            })
            .collect();
        builder = builder.builtin(
            "help".to_string(),
            Box::new(HelpBuiltin {
                tools: snapshots.clone(),
                log: Arc::clone(&log),
            }),
        );
        builder = builder.builtin(
            "discover".to_string(),
            Box::new(DiscoverBuiltin {
                tools: snapshots,
                log,
            }),
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
            "\n## Result\n\n| Field | Type | Description |\n|------|------|-------------|\n| `stdout` | string | Combined standard output |\n| `stderr` | string | Tool or bash errors |\n| `exit_code` | integer | Shell exit code |\n| `error` | string | Error category when execution fails |\n\n## Examples\n\n```json\n{\"commands\":\"get_user --id 42\"}\n```\n\n```json\n{\"commands\":\"user=$(get_user --id 42)\\necho \\\"$user\\\" | jq -r '.name'\"}\n```\n\n## Notes\n\n- Pass arguments as `--key value` or `--key=value`.\n- Standard bash builtins like `echo`, `jq`, `grep`, `sed`, and `awk` are available.\n- Use `help <tool> --json` inside the tool for runtime schema inspection.\n",
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
            "Pass args as --key value or --key=value. Use help/discover builtins for runtime details.",
            "Передавайте аргументи як --key value або --key=value. Використовуйте help/discover для деталей.",
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

        let log: InvocationLog = Arc::new(Mutex::new(Vec::new()));
        let mut bash = self.create_bash(Arc::clone(&log));

        let response = if let Some(sender) = stream_sender {
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
            bash.exec_streaming(&req.commands, output_cb).await
        } else {
            bash.exec(&req.commands).await
        };

        let response = match response {
            Ok(result) => result.into(),
            Err(err) => ToolResponse {
                stdout: String::new(),
                stderr: err.to_string(),
                exit_code: 1,
                error: Some(err.to_string()),
                ..Default::default()
            },
        };
        let invocations = log
            .lock()
            .expect("scripted invocation log poisoned")
            .clone();
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

    fn execution(&self, args: serde_json::Value) -> std::result::Result<ToolExecution, ToolError> {
        let req = tool_request_from_value(self.locale(), args)?;
        let tool = self.clone();
        Ok(ToolExecution::new(move |stream_sender| async move {
            let start = std::time::Instant::now();
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
            .tool(
                ToolDef::new("get_user", "Fetch user").with_schema(serde_json::json!({
                    "type": "object",
                    "properties": { "id": {"type": "integer"} }
                })),
                |_args: &super::ToolArgs| Ok("ok\n".to_string()),
            )
            .build();
        let sp = tool.system_prompt();
        assert!(sp.contains("help/discover"));
        assert!(!sp.contains("Usage:"));
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
        assert!(sp.contains("--id <integer>"));
    }

    #[tokio::test]
    async fn test_error_uses_display_not_debug() {
        use super::ScriptedTool;
        use crate::ToolDef;
        use crate::tool::Tool;

        let tool = ScriptedTool::builder("test")
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

    // -- DiscoverBuiltin tests --

    fn build_discover_test_tool() -> ScriptedTool {
        ScriptedTool::builder("big_api")
            .short_description("Big API")
            .tool(
                ToolDef::new("create_charge", "Create a payment charge")
                    .with_category("payments")
                    .with_tags(&["billing", "write"]),
                |_args: &super::ToolArgs| Ok("ok\n".to_string()),
            )
            .tool(
                ToolDef::new("refund", "Issue a refund")
                    .with_category("payments")
                    .with_tags(&["billing", "write"]),
                |_args: &super::ToolArgs| Ok("ok\n".to_string()),
            )
            .tool(
                ToolDef::new("get_user", "Fetch user by ID")
                    .with_category("users")
                    .with_tags(&["read"]),
                |_args: &super::ToolArgs| Ok("ok\n".to_string()),
            )
            .tool(
                ToolDef::new("delete_user", "Delete a user account")
                    .with_category("users")
                    .with_tags(&["admin", "write"]),
                |_args: &super::ToolArgs| Ok("ok\n".to_string()),
            )
            .tool(
                ToolDef::new("get_inventory", "Check inventory levels").with_category("inventory"),
                |_args: &super::ToolArgs| Ok("ok\n".to_string()),
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
            .tool(
                ToolDef::new("fail", "Always fails"),
                |_args: &super::ToolArgs| {
                    Err("connection failed: postgres://admin:secret@internal-db:5432/prod".into())
                },
            )
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
            .tool(
                ToolDef::new("fail", "Always fails"),
                |_args: &super::ToolArgs| {
                    Err("connection failed: postgres://admin:secret@internal-db:5432/prod".into())
                },
            )
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
