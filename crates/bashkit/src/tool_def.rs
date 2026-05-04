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
/// Describes a sub-tool registered with a `ScriptedToolBuilder` or usable
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
/// interpreter or used inside a `ScriptedTool`.
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
            let value = coerce_value(raw_value, properties.get(key), schema);
            result.insert(key.to_string(), value);
            i += 1;
            continue;
        }

        // --flag (boolean) or --key value
        let key = flag;
        let prop_schema = properties.get(key);
        let is_boolean = matches!(
            prop_schema
                .map(|s| resolve_effective_type(s, schema, 0))
                .unwrap_or(EffectiveType::Unknown),
            EffectiveType::Boolean
        );

        if is_boolean {
            result.insert(key.to_string(), serde_json::Value::Bool(true));
            i += 1;
        } else if i + 1 < raw_args.len() && !raw_args[i + 1].starts_with("--") {
            let raw_value = &raw_args[i + 1];
            let value = coerce_value(raw_value, prop_schema, schema);
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

/// Effective type of a property schema after resolving `$ref`,
/// `oneOf`/`anyOf`/`allOf` branches, nullable arrays
/// (`type: ["array","null"]`), and implicit signals
/// (`items` ⇒ array, `properties` ⇒ object).
#[derive(PartialEq, Clone, Copy, Debug)]
enum EffectiveType {
    String,
    Integer,
    Number,
    Boolean,
    Array,
    Object,
    Unknown,
}

const MAX_REF_DEPTH: usize = 16;

fn type_str_to_effective(s: &str) -> EffectiveType {
    match s {
        "string" => EffectiveType::String,
        "integer" => EffectiveType::Integer,
        "number" => EffectiveType::Number,
        "boolean" => EffectiveType::Boolean,
        "array" => EffectiveType::Array,
        "object" => EffectiveType::Object,
        _ => EffectiveType::Unknown,
    }
}

fn resolve_ref<'a>(
    ref_str: &str,
    root_schema: &'a serde_json::Value,
) -> Option<&'a serde_json::Value> {
    let suffix = ref_str.strip_prefix("#/")?;
    let mut current = root_schema;
    for segment in suffix.split('/') {
        let decoded = segment.replace("~1", "/").replace("~0", "~");
        current = current.get(&decoded)?;
    }
    Some(current)
}

fn resolve_effective_type(
    schema: &serde_json::Value,
    root_schema: &serde_json::Value,
    depth: usize,
) -> EffectiveType {
    if depth > MAX_REF_DEPTH {
        return EffectiveType::Unknown;
    }

    if let Some(ref_str) = schema.get("$ref").and_then(|r| r.as_str()) {
        if let Some(target) = resolve_ref(ref_str, root_schema) {
            return resolve_effective_type(target, root_schema, depth + 1);
        }
        return EffectiveType::Unknown;
    }

    match schema.get("type") {
        Some(serde_json::Value::String(s)) => return type_str_to_effective(s),
        Some(serde_json::Value::Array(arr)) => {
            // Prefer aggregate types when present; fall back to first non-null scalar.
            for t in arr {
                if let Some(s) = t.as_str()
                    && (s == "array" || s == "object")
                {
                    return type_str_to_effective(s);
                }
            }
            for t in arr {
                if let Some(s) = t.as_str()
                    && s != "null"
                {
                    return type_str_to_effective(s);
                }
            }
        }
        _ => {}
    }

    for key in ["oneOf", "anyOf", "allOf"] {
        if let Some(branches) = schema.get(key).and_then(|v| v.as_array()) {
            for branch in branches {
                let et = resolve_effective_type(branch, root_schema, depth + 1);
                if matches!(et, EffectiveType::Array | EffectiveType::Object) {
                    return et;
                }
            }
            for branch in branches {
                let et = resolve_effective_type(branch, root_schema, depth + 1);
                if !matches!(et, EffectiveType::Unknown) {
                    return et;
                }
            }
        }
    }

    if schema.get("items").is_some() {
        return EffectiveType::Array;
    }
    if schema.get("properties").is_some() {
        return EffectiveType::Object;
    }

    EffectiveType::Unknown
}

/// Coerce a raw string value to the type declared in the property schema.
fn coerce_value(
    raw: &str,
    prop_schema: Option<&serde_json::Value>,
    root_schema: &serde_json::Value,
) -> serde_json::Value {
    let effective = prop_schema
        .map(|s| resolve_effective_type(s, root_schema, 0))
        .unwrap_or(EffectiveType::Unknown);

    match effective {
        EffectiveType::Integer => raw
            .parse::<i64>()
            .map(serde_json::Value::from)
            .unwrap_or_else(|_| serde_json::Value::String(raw.to_string())),
        EffectiveType::Number => raw
            .parse::<f64>()
            .map(|n| serde_json::json!(n))
            .unwrap_or_else(|_| serde_json::Value::String(raw.to_string())),
        EffectiveType::Boolean => match raw {
            "true" | "1" | "yes" => serde_json::Value::Bool(true),
            "false" | "0" | "no" => serde_json::Value::Bool(false),
            _ => serde_json::Value::String(raw.to_string()),
        },
        EffectiveType::Array | EffectiveType::Object => {
            let trimmed = raw.trim_start();
            if (trimmed.starts_with('[') || trimmed.starts_with('{'))
                && let Ok(parsed) = serde_json::from_str::<serde_json::Value>(raw)
            {
                return parsed;
            }
            serde_json::Value::String(raw.to_string())
        }
        EffectiveType::String | EffectiveType::Unknown => {
            serde_json::Value::String(raw.to_string())
        }
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
    fn test_parse_flags_json_array_string() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {"tags": {"type": "array", "items": {"type": "string"}}}
        });
        let args = vec!["--tags".to_string(), r#"["a","b","c"]"#.to_string()];
        let result = parse_flags(&args, &schema).unwrap();
        assert_eq!(result["tags"], serde_json::json!(["a", "b", "c"]));
    }

    #[test]
    fn test_parse_flags_json_object_string() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {"server": {"type": "object"}}
        });
        let args = vec![
            "--server".to_string(),
            r#"{"name":"foo","port":8080}"#.to_string(),
        ];
        let result = parse_flags(&args, &schema).unwrap();
        assert_eq!(
            result["server"],
            serde_json::json!({"name": "foo", "port": 8080})
        );
    }

    #[test]
    fn test_parse_flags_nullable_array() {
        // utoipa-style nullable: type: ["array", "null"]
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "tags": {"type": ["array", "null"], "items": {"type": "string"}}
            }
        });
        let args = vec!["--tags".to_string(), r#"["x","y"]"#.to_string()];
        let result = parse_flags(&args, &schema).unwrap();
        assert_eq!(result["tags"], serde_json::json!(["x", "y"]));
    }

    #[test]
    fn test_parse_flags_oneof_null_and_ref() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "config": {
                    "oneOf": [
                        {"type": "null"},
                        {"$ref": "#/$defs/Config"}
                    ]
                }
            },
            "$defs": {
                "Config": {"type": "object", "properties": {"k": {"type": "string"}}}
            }
        });
        let args = vec!["--config".to_string(), r#"{"k":"v"}"#.to_string()];
        let result = parse_flags(&args, &schema).unwrap();
        assert_eq!(result["config"], serde_json::json!({"k": "v"}));
    }

    #[test]
    fn test_parse_flags_allof_composition() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "data": {
                    "allOf": [
                        {"type": "object"},
                        {"properties": {"x": {"type": "integer"}}}
                    ]
                }
            }
        });
        let args = vec!["--data".to_string(), r#"{"x":1}"#.to_string()];
        let result = parse_flags(&args, &schema).unwrap();
        assert_eq!(result["data"], serde_json::json!({"x": 1}));
    }

    #[test]
    fn test_parse_flags_invalid_json_left_as_string() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {"tags": {"type": "array"}}
        });
        // Malformed JSON — stays as raw string; serde produces the real error downstream.
        let args = vec!["--tags".to_string(), "[1, 2,".to_string()];
        let result = parse_flags(&args, &schema).unwrap();
        assert_eq!(
            result["tags"],
            serde_json::Value::String("[1, 2,".to_string())
        );
    }

    #[test]
    fn test_parse_flags_scalar_string_unchanged() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {"name": {"type": "string"}}
        });
        let args = vec!["--name".to_string(), "Alice".to_string()];
        let result = parse_flags(&args, &schema).unwrap();
        assert_eq!(result["name"], "Alice");
    }

    #[test]
    fn test_parse_flags_implicit_array_from_items() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {"tags": {"items": {"type": "string"}}}
        });
        let args = vec!["--tags".to_string(), r#"["p","q"]"#.to_string()];
        let result = parse_flags(&args, &schema).unwrap();
        assert_eq!(result["tags"], serde_json::json!(["p", "q"]));
    }

    #[test]
    fn test_parse_flags_implicit_object_from_properties() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "server": {"properties": {"port": {"type": "integer"}}}
            }
        });
        let args = vec!["--server".to_string(), r#"{"port":80}"#.to_string()];
        let result = parse_flags(&args, &schema).unwrap();
        assert_eq!(result["server"], serde_json::json!({"port": 80}));
    }

    #[test]
    fn test_parse_flags_ref_into_defs() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {"items": {"$ref": "#/$defs/Items"}},
            "$defs": {
                "Items": {"type": "array", "items": {"type": "integer"}}
            }
        });
        let args = vec!["--items".to_string(), "[1,2,3]".to_string()];
        let result = parse_flags(&args, &schema).unwrap();
        assert_eq!(result["items"], serde_json::json!([1, 2, 3]));
    }

    #[test]
    fn test_parse_flags_ref_into_definitions() {
        // Older draft uses `definitions` instead of `$defs`.
        let schema = serde_json::json!({
            "type": "object",
            "properties": {"items": {"$ref": "#/definitions/Items"}},
            "definitions": {
                "Items": {"type": "array"}
            }
        });
        let args = vec!["--items".to_string(), "[1,2]".to_string()];
        let result = parse_flags(&args, &schema).unwrap();
        assert_eq!(result["items"], serde_json::json!([1, 2]));
    }

    #[test]
    fn test_parse_flags_ref_cycle_bounded() {
        // Cyclical $ref must not stack-overflow.
        let schema = serde_json::json!({
            "type": "object",
            "properties": {"x": {"$ref": "#/$defs/A"}},
            "$defs": {
                "A": {"$ref": "#/$defs/B"},
                "B": {"$ref": "#/$defs/A"}
            }
        });
        let args = vec!["--x".to_string(), "value".to_string()];
        let result = parse_flags(&args, &schema).unwrap();
        // Falls back to string when ref cycle is detected.
        assert_eq!(result["x"], "value");
    }

    #[test]
    fn test_parse_flags_array_value_not_starting_with_bracket() {
        // Per spec: parse only when raw starts with [ or {.
        let schema = serde_json::json!({
            "type": "object",
            "properties": {"tags": {"type": "array"}}
        });
        let args = vec!["--tags".to_string(), "abc".to_string()];
        let result = parse_flags(&args, &schema).unwrap();
        assert_eq!(result["tags"], "abc");
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
