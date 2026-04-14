// ToolDef, ToolArgs, ToolImpl — reusable tool primitives.
//
// These types live here (not in scripted_tool/) so that both Bash and
// ScriptedTool can import them without circular dependencies.
//
// Dependency direction:  builtins → tool_def → {lib.rs, scripted_tool, tool.rs}

use crate::builtins::{Builtin, Context};
use crate::error::Result;
use crate::interpreter::ExecResult;
use async_trait::async_trait;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

// ============================================================================
// ToolDef — OpenAPI-style tool definition (metadata only)
// ============================================================================

/// OpenAPI-style tool definition: name, description, input schema.
///
/// Describes a sub-tool registered with [`ScriptedToolBuilder`] or usable
/// standalone. The `input_schema` is optional JSON Schema for documentation /
/// LLM prompts and for type coercion of `--key value` flags.
#[derive(Clone)]
pub struct ToolDef {
    /// Command name used as bash builtin (e.g. `"get_user"`).
    pub name: String,
    /// Human-readable description for LLM consumption.
    pub description: String,
    /// JSON Schema describing accepted arguments. Empty object if unspecified.
    pub input_schema: serde_json::Value,
    /// Categorical tags for discovery (e.g. `["admin", "billing"]`).
    pub tags: Vec<String>,
    /// Grouping category for discovery (e.g. `"payments"`).
    pub category: Option<String>,
}

impl ToolDef {
    /// Create a tool definition with name and description.
    pub fn new(name: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            input_schema: serde_json::Value::Object(Default::default()),
            tags: Vec::new(),
            category: None,
        }
    }

    /// Attach a JSON Schema for the tool's input parameters.
    pub fn with_schema(mut self, schema: serde_json::Value) -> Self {
        self.input_schema = schema;
        self
    }

    /// Add categorical tags for discovery filtering.
    pub fn with_tags(mut self, tags: &[&str]) -> Self {
        self.tags = tags.iter().map(|s| s.to_string()).collect();
        self
    }

    /// Set the grouping category for discovery.
    pub fn with_category(mut self, category: &str) -> Self {
        self.category = Some(category.to_string());
        self
    }
}

// ============================================================================
// ToolArgs — parsed arguments passed to exec functions
// ============================================================================

/// Parsed arguments passed to a tool exec function.
///
/// `params` is a JSON object built from `--key value` flags, with values
/// type-coerced per the `ToolDef`'s `input_schema`.
/// `stdin` carries pipeline input from a prior command, if any.
pub struct ToolArgs {
    /// Parsed parameters as a JSON object. Keys from `--key value` flags.
    pub params: serde_json::Value,
    /// Pipeline input from a prior command (e.g. `echo data | tool`).
    pub stdin: Option<String>,
}

impl ToolArgs {
    /// Get a string parameter by name.
    pub fn param_str(&self, key: &str) -> Option<&str> {
        self.params.get(key).and_then(|v| v.as_str())
    }

    /// Get an integer parameter by name.
    pub fn param_i64(&self, key: &str) -> Option<i64> {
        self.params.get(key).and_then(|v| v.as_i64())
    }

    /// Get a float parameter by name.
    pub fn param_f64(&self, key: &str) -> Option<f64> {
        self.params.get(key).and_then(|v| v.as_f64())
    }

    /// Get a boolean parameter by name.
    pub fn param_bool(&self, key: &str) -> Option<bool> {
        self.params.get(key).and_then(|v| v.as_bool())
    }
}

// ============================================================================
// Exec types — sync and async execution functions
// ============================================================================

/// Synchronous execution function for a tool.
///
/// Receives parsed [`ToolArgs`] with typed parameters and optional stdin.
/// Return `Ok(stdout)` on success or `Err(message)` on failure.
pub type SyncToolExec = Arc<dyn Fn(&ToolArgs) -> std::result::Result<String, String> + Send + Sync>;

/// Asynchronous execution function for a tool.
///
/// Same contract as [`SyncToolExec`] but returns a `Future`, allowing
/// non-blocking I/O. Takes owned [`ToolArgs`] because the future may
/// outlive the borrow.
pub type AsyncToolExec = Arc<
    dyn Fn(ToolArgs) -> Pin<Box<dyn Future<Output = std::result::Result<String, String>> + Send>>
        + Send
        + Sync,
>;

// Keep old names as aliases for backward compatibility.
/// Alias for [`SyncToolExec`] (backward compatibility).
pub type ToolCallback = SyncToolExec;
/// Alias for [`AsyncToolExec`] (backward compatibility).
pub type AsyncToolCallback = AsyncToolExec;

// ============================================================================
// ToolImpl — complete tool: metadata + execution
// ============================================================================

/// Complete tool: definition + sync/async exec functions.
///
/// Implements [`Builtin`] so it can be registered directly in a Bash
/// interpreter or used inside a [`ScriptedTool`].
///
/// # Example
///
/// ```rust
/// use bashkit::{ToolDef, ToolImpl};
///
/// let tool = ToolImpl::new(
///     ToolDef::new("greet", "Greet a user")
///         .with_schema(serde_json::json!({
///             "type": "object",
///             "properties": { "name": {"type": "string"} }
///         })),
/// )
/// .with_exec_sync(|args| {
///     let name = args.param_str("name").unwrap_or("world");
///     Ok(format!("hello {name}\n"))
/// });
/// ```
#[derive(Clone)]
pub struct ToolImpl {
    /// Tool metadata (name, description, schema, tags).
    pub def: ToolDef,
    /// Async exec (preferred when running in async context).
    pub exec: Option<AsyncToolExec>,
    /// Sync exec (preferred when running in sync context).
    pub exec_sync: Option<SyncToolExec>,
}

impl ToolImpl {
    /// Create a `ToolImpl` from a [`ToolDef`] with no exec functions.
    pub fn new(def: ToolDef) -> Self {
        Self {
            def,
            exec: None,
            exec_sync: None,
        }
    }

    /// Set the async exec function.
    pub fn with_exec<F, Fut>(mut self, f: F) -> Self
    where
        F: Fn(ToolArgs) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = std::result::Result<String, String>> + Send + 'static,
    {
        self.exec = Some(Arc::new(move |args| Box::pin(f(args))));
        self
    }

    /// Set the sync exec function.
    pub fn with_exec_sync(
        mut self,
        f: impl Fn(&ToolArgs) -> std::result::Result<String, String> + Send + Sync + 'static,
    ) -> Self {
        self.exec_sync = Some(Arc::new(f));
        self
    }
}

#[async_trait]
impl Builtin for ToolImpl {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let params = parse_flags(ctx.args, &self.def.input_schema)
            .map_err(|e| crate::error::Error::Execution(format!("{}: {e}", self.def.name)))?;
        let tool_args = ToolArgs {
            params,
            stdin: ctx.stdin.map(String::from),
        };

        // Prefer async, fall back to sync.
        let result = if let Some(cb) = &self.exec {
            (cb)(tool_args).await
        } else if let Some(cb) = &self.exec_sync {
            (cb)(&tool_args)
        } else {
            return Err(crate::error::Error::Execution(format!(
                "{}: no exec defined",
                self.def.name
            )));
        };

        match result {
            Ok(stdout) => Ok(ExecResult::ok(stdout)),
            Err(msg) => Ok(ExecResult::err(msg, 1)),
        }
    }
}

// ============================================================================
// Flag parser — `--key value` / `--key=value` → JSON object
// ============================================================================

/// Parse `--key value` and `--key=value` flags into a JSON object.
/// Types are coerced according to the schema's property definitions.
/// Unknown flags (not in schema) are kept as strings.
/// Bare `--flag` without a value is treated as `true` if the schema says boolean,
/// otherwise as `true` when the next arg also starts with `--` or is absent.
pub(crate) fn parse_flags(
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
pub(crate) fn usage_from_schema(schema: &serde_json::Value) -> Option<String> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_flags_basic() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "id": {"type": "integer"},
                "name": {"type": "string"},
                "verbose": {"type": "boolean"}
            }
        });
        let args = vec![
            "--id".to_string(),
            "42".to_string(),
            "--name".to_string(),
            "Alice".to_string(),
            "--verbose".to_string(),
        ];
        let result = parse_flags(&args, &schema).unwrap();
        assert_eq!(result["id"], 42);
        assert_eq!(result["name"], "Alice");
        assert_eq!(result["verbose"], true);
    }

    #[test]
    fn test_parse_flags_equals_syntax() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {"id": {"type": "integer"}}
        });
        let args = vec!["--id=42".to_string()];
        let result = parse_flags(&args, &schema).unwrap();
        assert_eq!(result["id"], 42);
    }

    #[test]
    fn test_tool_impl_sync() {
        let tool = ToolImpl::new(ToolDef::new("greet", "Greet a user").with_schema(
            serde_json::json!({
                "type": "object",
                "properties": { "name": {"type": "string"} }
            }),
        ))
        .with_exec_sync(|args| {
            let name = args.param_str("name").unwrap_or("world");
            Ok(format!("hello {name}\n"))
        });

        assert!(tool.exec_sync.is_some());
        assert!(tool.exec.is_none());
        assert_eq!(tool.def.name, "greet");
    }

    #[tokio::test]
    async fn test_tool_impl_as_builtin() {
        let tool = ToolImpl::new(ToolDef::new("greet", "Greet a user").with_schema(
            serde_json::json!({
                "type": "object",
                "properties": { "name": {"type": "string"} }
            }),
        ))
        .with_exec_sync(|args| {
            let name = args.param_str("name").unwrap_or("world");
            Ok(format!("hello {name}\n"))
        });

        // Verify it works as a Builtin
        let args = vec!["--name".to_string(), "Alice".to_string()];
        let mut vars = std::collections::HashMap::new();
        let env = std::collections::HashMap::new();
        let mut cwd = std::path::PathBuf::from("/");
        let fs = Arc::new(crate::fs::InMemoryFs::new());
        let ctx = Context::new_for_test(&args, &env, &mut vars, &mut cwd, fs, None);
        let result = tool.execute(ctx).await.unwrap();
        assert_eq!(result.stdout, "hello Alice\n");
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn test_tool_impl_async_exec() {
        let tool =
            ToolImpl::new(ToolDef::new("echo_async", "Async echo")).with_exec(|args| async move {
                let msg = args.stdin.unwrap_or_default();
                Ok(format!("async: {msg}"))
            });

        assert!(tool.exec.is_some());
        assert!(tool.exec_sync.is_none());
    }

    #[tokio::test]
    async fn test_tool_impl_no_exec_errors() {
        let tool = ToolImpl::new(ToolDef::new("empty", "No exec"));

        let args = vec![];
        let mut vars = std::collections::HashMap::new();
        let env = std::collections::HashMap::new();
        let mut cwd = std::path::PathBuf::from("/");
        let fs = Arc::new(crate::fs::InMemoryFs::new());
        let ctx = Context::new_for_test(&args, &env, &mut vars, &mut cwd, fs, None);
        let result = tool.execute(ctx).await;
        assert!(result.is_err());
    }
}
