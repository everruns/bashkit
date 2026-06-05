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
    /// Whether callback error strings are sanitized before script-visible stderr.
    pub sanitize_errors: bool,
}

impl ToolImpl {
    /// Create a `ToolImpl` from a [`ToolDef`] with no exec functions.
    pub fn new(def: ToolDef) -> Self {
        Self {
            def,
            exec: None,
            exec_sync: None,
            sanitize_errors: true,
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

    /// Control whether callback errors are sanitized before reaching stderr.
    ///
    /// Default: `true`, replacing callback `Err(String)` with
    /// `"<tool>: callback failed\n"` to avoid exposing host-side secrets,
    /// paths, connection strings, or stack traces to untrusted scripts.
    /// Set to `false` only when callers are trusted and raw diagnostics are safe.
    // THREAT[TM-INF-030]: ToolImpl may be registered directly as a Builtin, so
    // it must match ScriptedTool's safe default instead of leaking raw callback errors.
    pub fn sanitize_errors(mut self, sanitize: bool) -> Self {
        self.sanitize_errors = sanitize;
        self
    }

    fn callback_failed_message(&self) -> String {
        format!("{}: callback failed\n", self.def.name)
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
            Err(msg) if self.sanitize_errors => {
                #[cfg(not(feature = "tracing"))]
                let _ = &msg;
                #[cfg(feature = "tracing")]
                tracing::debug!(
                    tool = %self.def.name,
                    error = %msg,
                    "tool callback error (sanitized)"
                );
                Ok(ExecResult::err(self.callback_failed_message(), 1))
            }
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
///
/// For aggregate flags (`type: "object"` or `type: "array"`), `key=value`
/// pair tokens are accepted alongside JSON: a sequence of pair tokens
/// after `--flag` is collected into one object; repeated invocations of an
/// array-of-object flag append one object per group. Arrays of scalars
/// accept comma-split values (`--tags a,b,c`) and repeated invocations.
pub(crate) fn parse_flags(
    raw_args: &[String],
    schema: &serde_json::Value,
) -> std::result::Result<serde_json::Value, String> {
    let mut budget = FlagParseBudget::default();
    parse_flags_with_budget(raw_args, schema, &mut budget)
}

fn parse_flags_with_budget(
    raw_args: &[String],
    schema: &serde_json::Value,
    budget: &mut FlagParseBudget,
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
            budget.add_bytes(raw_value.len(), key)?;
            let value = coerce_value(raw_value, properties.get(key), schema);
            result.insert(key.to_string(), value);
            i += 1;
            continue;
        }

        let key = flag.to_string();
        let prop_schema = properties.get(&key).cloned();
        let effective = prop_schema
            .as_ref()
            .map(|s| resolve_effective_type(s, schema, 0))
            .unwrap_or(EffectiveType::Unknown);

        i += 1;

        match effective {
            EffectiveType::Boolean => {
                result.insert(key, serde_json::Value::Bool(true));
            }
            EffectiveType::Array => {
                let items_schema = prop_schema.as_ref().and_then(|s| s.get("items")).cloned();
                let items_effective = items_schema
                    .as_ref()
                    .map(|s| resolve_effective_type(s, schema, 0))
                    .unwrap_or(EffectiveType::Unknown);

                match consume_array_value(
                    raw_args,
                    &mut i,
                    items_schema.as_ref(),
                    items_effective,
                    schema,
                    &key,
                    budget,
                )? {
                    ArrayInput::Items(items) => {
                        let entry = result
                            .entry(key.clone())
                            .or_insert_with(|| serde_json::Value::Array(Vec::new()));
                        if let serde_json::Value::Array(arr) = entry {
                            ensure_array_item_limit(&key, arr.len(), items.len())?;
                            arr.extend(items);
                        } else {
                            ensure_array_item_limit(&key, 0, items.len())?;
                            *entry = serde_json::Value::Array(items);
                        }
                    }
                    ArrayInput::Raw(value) => {
                        // Invalid JSON — preserve as raw string so serde
                        // produces the real type-mismatch error downstream.
                        result.insert(key, value);
                    }
                }
            }
            EffectiveType::Object => {
                let value = consume_object_value(
                    raw_args,
                    &mut i,
                    prop_schema.as_ref(),
                    schema,
                    &key,
                    budget,
                )?;
                result.insert(key, value);
            }
            _ => {
                if i < raw_args.len() && !raw_args[i].starts_with("--") {
                    let raw_value = &raw_args[i];
                    budget.add_bytes(raw_value.len(), &key)?;
                    let value = coerce_value(raw_value, prop_schema.as_ref(), schema);
                    result.insert(key, value);
                    i += 1;
                } else {
                    result.insert(key, serde_json::Value::Bool(true));
                }
            }
        }
    }

    Ok(serde_json::Value::Object(result))
}

/// Walk a schema to extract object property definitions, following `$ref`
/// and merging properties across `oneOf`/`anyOf`/`allOf` branches.
fn resolve_object_properties(
    schema: &serde_json::Value,
    root_schema: &serde_json::Value,
    depth: usize,
) -> serde_json::Map<String, serde_json::Value> {
    if depth > MAX_REF_DEPTH {
        return Default::default();
    }
    if let Some(ref_str) = schema.get("$ref").and_then(|r| r.as_str()) {
        if let Some(target) = resolve_ref(ref_str, root_schema) {
            return resolve_object_properties(target, root_schema, depth + 1);
        }
        return Default::default();
    }
    if let Some(props) = schema.get("properties").and_then(|p| p.as_object()) {
        return props.clone();
    }
    let mut merged = serde_json::Map::new();
    for key in ["oneOf", "anyOf", "allOf"] {
        if let Some(branches) = schema.get(key).and_then(|v| v.as_array()) {
            for branch in branches {
                let props = resolve_object_properties(branch, root_schema, depth + 1);
                for (k, v) in props {
                    merged.entry(k).or_insert(v);
                }
            }
        }
    }
    merged
}

/// Collect `key=value` tokens into an object until the next `--flag` or end of args.
fn collect_object_from_pairs(
    args: &[String],
    i: &mut usize,
    object_schema: Option<&serde_json::Value>,
    root_schema: &serde_json::Value,
    flag_name: &str,
    budget: &mut FlagParseBudget,
) -> std::result::Result<serde_json::Map<String, serde_json::Value>, String> {
    let mut obj = serde_json::Map::new();
    let inner_props = object_schema
        .map(|s| resolve_object_properties(s, root_schema, 0))
        .unwrap_or_default();

    while *i < args.len() {
        let arg = &args[*i];
        if arg.starts_with("--") {
            break;
        }
        let Some((k, v)) = arg.split_once('=') else {
            return Err(format!(
                "--{flag_name}: expected --flag or key=value, got '{arg}'"
            ));
        };

        if !inner_props.is_empty() && !inner_props.contains_key(k) {
            let mut valid: Vec<&str> = inner_props.keys().map(|s| s.as_str()).collect();
            valid.sort();
            return Err(format!(
                "--{flag_name}: unknown key '{k}'; valid keys: {}",
                valid.join(", ")
            ));
        }

        budget.add_bytes(k.len().saturating_add(v.len()), flag_name)?;
        let nested_schema = inner_props.get(k);
        let value = coerce_value(v, nested_schema, root_schema);
        obj.insert(k.to_string(), value);
        *i += 1;
    }
    Ok(obj)
}

/// Result of consuming the value(s) after an array-typed `--flag`.
enum ArrayInput {
    /// Items to append to the accumulated array for this flag.
    Items(Vec<serde_json::Value>),
    /// Raw fallback (typically when JSON parse failed) — overrides the
    /// array entry with this value so serde surfaces the type error.
    Raw(serde_json::Value),
}

/// Consume the value(s) following an array-typed `--flag`.
/// Accepts: JSON array, JSON object (single-element append), `key=value` pair
/// group (single-element append for arrays of objects), or comma-split scalars.
fn consume_array_value(
    args: &[String],
    i: &mut usize,
    items_schema: Option<&serde_json::Value>,
    items_effective: EffectiveType,
    root_schema: &serde_json::Value,
    flag_name: &str,
    budget: &mut FlagParseBudget,
) -> std::result::Result<ArrayInput, String> {
    if *i >= args.len() || args[*i].starts_with("--") {
        return Err(format!("--{flag_name}: missing value"));
    }
    let next = &args[*i];
    let trimmed = next.trim_start();

    if trimmed.starts_with('[') {
        if let Some(serde_json::Value::Array(arr)) = parse_aggregate_json_value(next) {
            ensure_array_item_limit(flag_name, 0, arr.len())?;
            budget.add_bytes(next.len(), flag_name)?;
            *i += 1;
            return Ok(ArrayInput::Items(arr));
        }
        *i += 1;
        return Ok(ArrayInput::Raw(serde_json::Value::String(next.clone())));
    }

    if items_effective == EffectiveType::Object {
        if trimmed.starts_with('{') {
            if let Some(parsed) = parse_aggregate_json_value(next) {
                budget.add_bytes(next.len(), flag_name)?;
                *i += 1;
                return Ok(ArrayInput::Items(vec![parsed]));
            }
            *i += 1;
            return Ok(ArrayInput::Raw(serde_json::Value::String(next.clone())));
        }
        if next.contains('=') {
            let obj =
                collect_object_from_pairs(args, i, items_schema, root_schema, flag_name, budget)?;
            return Ok(ArrayInput::Items(vec![serde_json::Value::Object(obj)]));
        }
        return Err(format!(
            "--{flag_name}: expected JSON or key=value pairs, got '{next}'"
        ));
    }

    // Scalar items: comma-split.
    let item_count = next.split(',').count();
    ensure_array_item_limit(flag_name, 0, item_count)?;
    budget.add_bytes(next.len(), flag_name)?;
    let mut out = Vec::with_capacity(item_count);
    for part in next.split(',') {
        out.push(coerce_value(part, items_schema, root_schema));
    }
    *i += 1;
    Ok(ArrayInput::Items(out))
}

/// Consume the value(s) following an object-typed `--flag`.
/// Accepts: JSON object/array literal, or a `key=value` pair group.
fn consume_object_value(
    args: &[String],
    i: &mut usize,
    prop_schema: Option<&serde_json::Value>,
    root_schema: &serde_json::Value,
    flag_name: &str,
    budget: &mut FlagParseBudget,
) -> std::result::Result<serde_json::Value, String> {
    if *i >= args.len() || args[*i].starts_with("--") {
        return Ok(serde_json::Value::Bool(true));
    }
    let next = &args[*i];
    let trimmed = next.trim_start();

    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        budget.add_bytes(next.len(), flag_name)?;
        let value = coerce_value(next, prop_schema, root_schema);
        *i += 1;
        return Ok(value);
    }

    if next.contains('=') {
        let obj = collect_object_from_pairs(args, i, prop_schema, root_schema, flag_name, budget)?;
        return Ok(serde_json::Value::Object(obj));
    }

    budget.add_bytes(next.len(), flag_name)?;
    let value = coerce_value(next, prop_schema, root_schema);
    *i += 1;
    Ok(value)
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
const MAX_AGGREGATE_FLAG_JSON_BYTES: usize = 64 * 1024;
const MAX_PARSED_FLAG_BYTES: usize = 64 * 1024;
const MAX_ARRAY_FLAG_ITEMS: usize = 4096;

#[derive(Default)]
struct FlagParseBudget {
    bytes: usize,
}

impl FlagParseBudget {
    fn add_bytes(&mut self, bytes: usize, flag_name: &str) -> std::result::Result<(), String> {
        self.bytes = self.bytes.saturating_add(bytes);
        if self.bytes > MAX_PARSED_FLAG_BYTES {
            return Err(format!(
                "--{flag_name}: parsed flag data exceeds {MAX_PARSED_FLAG_BYTES} bytes"
            ));
        }
        Ok(())
    }
}

fn ensure_array_item_limit(
    flag_name: &str,
    existing: usize,
    adding: usize,
) -> std::result::Result<(), String> {
    let total = existing.saturating_add(adding);
    if total > MAX_ARRAY_FLAG_ITEMS {
        return Err(format!(
            "--{flag_name}: array flag has {total} items; maximum is {MAX_ARRAY_FLAG_ITEMS}"
        ));
    }
    Ok(())
}

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
                && let Some(parsed) = parse_aggregate_json_value(raw)
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

fn parse_aggregate_json_value(raw: &str) -> Option<serde_json::Value> {
    if raw.len() > MAX_AGGREGATE_FLAG_JSON_BYTES {
        return None;
    }
    serde_json::from_str::<serde_json::Value>(raw).ok()
}

/// Generate a usage hint from schema properties: `--id <integer> --name <string>`.
/// Aggregate flags advertise both JSON and `key=value` forms in their hint.
pub(crate) fn usage_from_schema(schema: &serde_json::Value) -> Option<String> {
    let props = schema.get("properties")?.as_object()?;
    if props.is_empty() {
        return None;
    }
    let flags: Vec<String> = props
        .iter()
        .map(|(key, prop)| {
            let hint = match resolve_effective_type(prop, schema, 0) {
                EffectiveType::Object => "<json|key=value...>".to_string(),
                EffectiveType::Array => {
                    let items = prop.get("items");
                    let items_eff = items
                        .map(|s| resolve_effective_type(s, schema, 0))
                        .unwrap_or(EffectiveType::Unknown);
                    if items_eff == EffectiveType::Object {
                        "<json|key=value...>".to_string()
                    } else {
                        "<json|a,b,c>".to_string()
                    }
                }
                EffectiveType::Integer => "<integer>".to_string(),
                EffectiveType::Number => "<number>".to_string(),
                EffectiveType::Boolean => "<boolean>".to_string(),
                EffectiveType::String => "<string>".to_string(),
                EffectiveType::Unknown => {
                    let ty = prop.get("type").and_then(|t| t.as_str()).unwrap_or("value");
                    format!("<{ty}>")
                }
            };
            format!("--{key} {hint}")
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
        // Aggregate scalar array: comma-split (single token, no comma → length-1 array).
        let schema = serde_json::json!({
            "type": "object",
            "properties": {"tags": {"type": "array", "items": {"type": "string"}}}
        });
        let args = vec!["--tags".to_string(), "abc".to_string()];
        let result = parse_flags(&args, &schema).unwrap();
        assert_eq!(result["tags"], serde_json::json!(["abc"]));
    }

    #[test]
    fn test_parse_flags_array_missing_value_is_error() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {"tags": {"type": "array", "items": {"type": "string"}}}
        });
        let args = vec!["--tags".to_string()];
        let err = parse_flags(&args, &schema).unwrap_err();
        assert_eq!(err, "--tags: missing value");
    }

    #[test]
    fn test_parse_flags_array_missing_value_before_next_flag_is_error() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "tags": {"type": "array", "items": {"type": "string"}},
                "name": {"type": "string"}
            }
        });
        let args = vec![
            "--tags".to_string(),
            "--name".to_string(),
            "alice".to_string(),
        ];
        let err = parse_flags(&args, &schema).unwrap_err();
        assert_eq!(err, "--tags: missing value");
    }

    #[test]
    fn test_parse_flags_pair_object_single() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "server": {
                    "type": "object",
                    "properties": {
                        "name": {"type": "string"},
                        "url": {"type": "string"}
                    }
                }
            }
        });
        let args = vec![
            "--server".to_string(),
            "name=foo".to_string(),
            "url=https://example.com".to_string(),
        ];
        let result = parse_flags(&args, &schema).unwrap();
        assert_eq!(
            result["server"],
            serde_json::json!({"name": "foo", "url": "https://example.com"})
        );
    }

    #[test]
    fn test_parse_flags_pair_array_of_objects_repeated() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "endpoint": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "name": {"type": "string"},
                            "url": {"type": "string"}
                        }
                    }
                }
            }
        });
        let args = vec![
            "--endpoint".to_string(),
            "name=a".to_string(),
            "url=u1".to_string(),
            "--endpoint".to_string(),
            "name=b".to_string(),
            "url=u2".to_string(),
        ];
        let result = parse_flags(&args, &schema).unwrap();
        assert_eq!(
            result["endpoint"],
            serde_json::json!([
                {"name": "a", "url": "u1"},
                {"name": "b", "url": "u2"}
            ])
        );
    }

    #[test]
    fn test_parse_flags_array_string_comma_split() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "tags": {"type": "array", "items": {"type": "string"}}
            }
        });
        let args = vec!["--tags".to_string(), "a,b,c".to_string()];
        let result = parse_flags(&args, &schema).unwrap();
        assert_eq!(result["tags"], serde_json::json!(["a", "b", "c"]));
    }

    #[test]
    fn test_parse_flags_array_string_repeated_appends() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "tags": {"type": "array", "items": {"type": "string"}}
            }
        });
        let args = vec![
            "--tags".to_string(),
            "x".to_string(),
            "--tags".to_string(),
            "y".to_string(),
        ];
        let result = parse_flags(&args, &schema).unwrap();
        assert_eq!(result["tags"], serde_json::json!(["x", "y"]));
    }

    #[test]
    fn test_parse_flags_pair_nested_type_coercion() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "server": {
                    "type": "object",
                    "properties": {
                        "enabled": {"type": "boolean"},
                        "port": {"type": "integer"}
                    }
                }
            }
        });
        let args = vec![
            "--server".to_string(),
            "enabled=true".to_string(),
            "port=8080".to_string(),
        ];
        let result = parse_flags(&args, &schema).unwrap();
        assert_eq!(
            result["server"],
            serde_json::json!({"enabled": true, "port": 8080})
        );
    }

    #[test]
    fn test_parse_flags_pair_unknown_key_errors() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "server": {
                    "type": "object",
                    "properties": {
                        "name": {"type": "string"}
                    }
                }
            }
        });
        let args = vec!["--server".to_string(), "bogus=foo".to_string()];
        let err = parse_flags(&args, &schema).unwrap_err();
        assert!(err.contains("unknown key"), "got: {err}");
        assert!(err.contains("bogus"), "got: {err}");
    }

    #[test]
    fn test_parse_flags_object_json_form_unchanged() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "server": {
                    "type": "object",
                    "properties": {"name": {"type": "string"}}
                }
            }
        });
        let args = vec!["--server".to_string(), r#"{"name":"foo"}"#.to_string()];
        let result = parse_flags(&args, &schema).unwrap();
        assert_eq!(result["server"], serde_json::json!({"name": "foo"}));
    }

    #[test]
    fn test_parse_flags_pair_mixed_with_json_rejected() {
        // After consuming JSON `{...}` for --server, a stray `name=foo` token is
        // not a flag and not part of the consumed JSON value, so parse_flags
        // rejects it at the top of the loop.
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "server": {
                    "type": "object",
                    "properties": {"name": {"type": "string"}}
                }
            }
        });
        let args = vec![
            "--server".to_string(),
            r#"{"name":"foo"}"#.to_string(),
            "name=bar".to_string(),
        ];
        let err = parse_flags(&args, &schema).unwrap_err();
        assert!(err.contains("expected --flag"), "got: {err}");
    }

    #[test]
    fn test_parse_flags_array_of_objects_json_then_pair_appends() {
        // JSON for first invocation, pairs for second — both append.
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "endpoint": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "name": {"type": "string"}
                        }
                    }
                }
            }
        });
        let args = vec![
            "--endpoint".to_string(),
            r#"{"name":"j"}"#.to_string(),
            "--endpoint".to_string(),
            "name=p".to_string(),
        ];
        let result = parse_flags(&args, &schema).unwrap();
        assert_eq!(
            result["endpoint"],
            serde_json::json!([{"name": "j"}, {"name": "p"}])
        );
    }

    #[test]
    fn test_parse_flags_array_int_comma_split_coerced() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "ids": {"type": "array", "items": {"type": "integer"}}
            }
        });
        let args = vec!["--ids".to_string(), "1,2,3".to_string()];
        let result = parse_flags(&args, &schema).unwrap();
        assert_eq!(result["ids"], serde_json::json!([1, 2, 3]));
    }

    #[test]
    fn test_parse_flags_comma_split_array_item_limit() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {"tags": {"type": "array", "items": {"type": "string"}}}
        });
        let too_many_empty_items = ",".repeat(MAX_ARRAY_FLAG_ITEMS);
        let args = vec!["--tags".to_string(), too_many_empty_items];
        let err = parse_flags(&args, &schema).unwrap_err();
        assert!(err.contains("maximum"), "got: {err}");
    }

    #[test]
    fn test_parse_flags_repeated_array_item_limit() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {"tags": {"type": "array", "items": {"type": "string"}}}
        });
        let at_limit = std::iter::repeat_n("x", MAX_ARRAY_FLAG_ITEMS)
            .collect::<Vec<_>>()
            .join(",");
        let args = vec![
            "--tags".to_string(),
            at_limit,
            "--tags".to_string(),
            "overflow".to_string(),
        ];
        let err = parse_flags(&args, &schema).unwrap_err();
        assert!(err.contains("maximum"), "got: {err}");
    }

    #[test]
    fn test_parse_flags_total_parsed_bytes_limit() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {"name": {"type": "string"}}
        });
        let args = vec!["--name".to_string(), "x".repeat(MAX_PARSED_FLAG_BYTES + 1)];
        let err = parse_flags(&args, &schema).unwrap_err();
        assert!(err.contains("parsed flag data exceeds"), "got: {err}");
    }

    #[test]
    fn test_parse_flags_large_json_array_stays_string() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {"tags": {"type": "array", "items": {"type": "string"}}}
        });
        let large = format!(
            "[{}]",
            "\"a\",".repeat((MAX_AGGREGATE_FLAG_JSON_BYTES / 4) + 1)
        );
        let args = vec!["--tags".to_string(), large.clone()];
        let result = parse_flags(&args, &schema).unwrap();
        assert_eq!(result["tags"], serde_json::Value::String(large));
    }

    #[test]
    fn test_usage_from_schema_advertises_both_forms() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "server": {"type": "object"},
                "tags": {"type": "array", "items": {"type": "string"}},
                "id": {"type": "integer"}
            }
        });
        let usage = usage_from_schema(&schema).expect("usage");
        assert!(
            usage.contains("--server <json|key=value...>"),
            "got: {usage}"
        );
        assert!(usage.contains("--tags <json|a,b,c>"), "got: {usage}");
        assert!(usage.contains("--id <integer>"), "got: {usage}");
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
        assert!(tool.sanitize_errors);
        assert_eq!(tool.def.name, "greet");
    }

    fn tool_test_context<'a>(
        args: &'a [String],
        env: &'a std::collections::HashMap<String, String>,
        vars: &'a mut std::collections::HashMap<String, String>,
        cwd: &'a mut std::path::PathBuf,
    ) -> Context<'a> {
        let fs = Arc::new(crate::fs::InMemoryFs::new());
        Context::new_for_test(args, env, vars, cwd, fs, None)
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
        let env = std::collections::HashMap::new();
        let mut vars = std::collections::HashMap::new();
        let mut cwd = std::path::PathBuf::from("/");
        let ctx = tool_test_context(&args, &env, &mut vars, &mut cwd);
        let result = tool.execute(ctx).await.unwrap();
        assert_eq!(result.stdout, "hello Alice\n");
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn test_tool_impl_as_builtin_sanitizes_callback_errors() {
        let secret = "postgres://user:secret@internal-db/private";
        let tool =
            ToolImpl::new(ToolDef::new("lookup", "Lookup record")).with_exec_sync(move |_| {
                Err(format!(
                    "backend failed for {secret} at /srv/app/src/client.rs:42"
                ))
            });

        let args = vec![];
        let env = std::collections::HashMap::new();
        let mut vars = std::collections::HashMap::new();
        let mut cwd = std::path::PathBuf::from("/");
        let ctx = tool_test_context(&args, &env, &mut vars, &mut cwd);
        let result = tool.execute(ctx).await.unwrap();

        assert_eq!(result.exit_code, 1);
        assert_eq!(result.stderr, "lookup: callback failed\n");
        assert!(!result.stderr.contains("secret"));
        assert!(!result.stderr.contains("internal-db"));
        assert!(!result.stderr.contains("/srv/app"));
    }

    #[tokio::test]
    async fn test_tool_impl_as_builtin_can_opt_out_of_error_sanitization() {
        let tool = ToolImpl::new(ToolDef::new("lookup", "Lookup record"))
            .with_exec_sync(|_| Err("raw diagnostic".to_string()))
            .sanitize_errors(false);

        let args = vec![];
        let env = std::collections::HashMap::new();
        let mut vars = std::collections::HashMap::new();
        let mut cwd = std::path::PathBuf::from("/");
        let ctx = tool_test_context(&args, &env, &mut vars, &mut cwd);
        let result = tool.execute(ctx).await.unwrap();

        assert_eq!(result.exit_code, 1);
        assert_eq!(result.stderr, "raw diagnostic");
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
        let env = std::collections::HashMap::new();
        let mut vars = std::collections::HashMap::new();
        let mut cwd = std::path::PathBuf::from("/");
        let ctx = tool_test_context(&args, &env, &mut vars, &mut cwd);
        let result = tool.execute(ctx).await;
        assert!(result.is_err());
    }
}
