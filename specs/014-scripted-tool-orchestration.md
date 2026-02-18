# Spec 014: Scripted Tool Orchestration

## Summary

Compose tool definitions (`ToolDef`) + execution callbacks into a single `ScriptedTool` that accepts bash scripts. Each sub-tool becomes a builtin command, letting LLMs orchestrate N tools in one call using pipes, variables, loops, and conditionals.

## Feature flag

`scripted_tool` — the entire module is gated behind `#[cfg(feature = "scripted_tool")]`.

## Motivation

When an LLM has access to many tools (get_user, list_orders, get_inventory, etc.), each tool call is a separate round-trip. A data-gathering task that needs 5 tools requires 5+ turns. With `ScriptedTool`, the LLM writes a single bash script that calls all tools, pipes results through `jq`, and returns composed output — reducing latency and token cost.

## Design

### ToolDef — OpenAPI-style tool definition

```rust
pub struct ToolDef {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,  // JSON Schema, empty object if unset
}

impl ToolDef {
    pub fn new(name: impl Into<String>, description: impl Into<String>) -> Self;
    pub fn with_schema(self, schema: serde_json::Value) -> Self;
}
```

Standard OpenAPI fields: `name`, `description`, `input_schema`. Schema is optional — defaults to `{}`.

### ToolArgs — parsed arguments passed to callbacks

```rust
pub struct ToolArgs {
    pub params: serde_json::Value,  // JSON object from --key value flags
    pub stdin: Option<String>,      // pipeline input from prior command
}

impl ToolArgs {
    pub fn param_str(&self, key: &str) -> Option<&str>;
    pub fn param_i64(&self, key: &str) -> Option<i64>;
    pub fn param_f64(&self, key: &str) -> Option<f64>;
    pub fn param_bool(&self, key: &str) -> Option<bool>;
}
```

The adapter parses `--key value` and `--key=value` flags from the bash command line,
coerces types according to the tool's `input_schema`, and passes the result as `ToolArgs`.

### ToolCallback

```rust
pub type ToolCallback =
    Arc<dyn Fn(&ToolArgs) -> Result<String, String> + Send + Sync>;
```

- `args.params`: JSON object with parsed `--key value` flags, typed per schema.
- `args.stdin`: pipeline input from prior command.
- Returns stdout string on success, error message on failure.

### Flag parsing

Bash command args are parsed into a JSON object:

| Syntax | Result |
|--------|--------|
| `--id 42` | `{"id": 42}` (if schema says integer) |
| `--id=42` | `{"id": 42}` |
| `--verbose` | `{"verbose": true}` (if schema says boolean) |
| `--name Alice` | `{"name": "Alice"}` |

Type coercion follows the `input_schema` property types: `integer`, `number`, `boolean`, `string`.
Unknown flags (not in schema) are kept as strings.

### ScriptedToolBuilder

Two arguments per tool: definition + callback.

```rust
ScriptedTool::builder("api_name")
    .short_description("...")
    .tool(
        ToolDef::new("get_user", "Fetch user by ID")
            .with_schema(json!({"type": "object", "properties": {"id": {"type": "integer"}}})),
        |args| {
            let id = args.param_i64("id").ok_or("missing --id")?;
            Ok(format!("{{\"id\":{id}}}\n"))
        },
    )
    .env("API_KEY", "...")
    .limits(ExecutionLimits::new().max_commands(500))
    .build()
```

### ToolBuiltinAdapter (internal)

Wraps `ToolCallback` (Arc) as a `Builtin` for the interpreter. Parses `--key value` flags
from `ctx.args` using the tool's schema for type coercion, then calls the callback with `ToolArgs`.

### ScriptedTool

Implements the `Tool` trait. On each `execute()`:

1. Creates a fresh `Bash` instance.
2. Registers each callback as a builtin via `Arc::clone`.
3. Runs the user-provided script.
4. Returns `ToolResponse { stdout, stderr, exit_code }`.

Reusable — multiple `execute()` calls share the same `Arc<ToolCallback>` instances.

### LLM integration

`system_prompt()` generates markdown with available tool commands, input schemas (when present), and tips. Example output:

```markdown
# api_name

Input: {"commands": "<bash script>"}
Output: {stdout, stderr, exit_code}

## Available tool commands

- `get_user`: Fetch user by ID
  Usage: `get_user --id <integer>`
- `list_orders`: List orders for user
  Usage: `list_orders --user_id <integer>`

## Tips

- Pass arguments as `--key value` or `--key=value` flags
- Pipe tool output through `jq` for JSON processing
- Use variables to pass data between tool calls
```

## Module location

`crates/bashkit/src/scripted_tool/`

```
scripted_tool/
├── mod.rs       — ToolDef, ToolCallback, ScriptedToolBuilder, ScriptedTool struct, tests
└── execute.rs   — Tool impl, ToolBuiltinAdapter, documentation helpers
```

Public exports from `lib.rs` (gated by `scripted_tool` feature):
`ToolDef`, `ToolArgs`, `ToolCallback`, `ScriptedTool`, `ScriptedToolBuilder`.

## Example

`crates/bashkit/examples/scripted_tool.rs` — e-commerce API demo with get_user, list_orders, get_inventory, create_discount. Uses `ToolDef` + closures (no trait impls needed).

Run: `cargo run --example scripted_tool --features scripted_tool`

## Test coverage

31 unit tests covering:
- Builder configuration (name, description, defaults)
- Introspection (help, system_prompt, schemas, schema rendering)
- Flag parsing (`--key value`, `--key=value`, boolean flags, type coercion)
- Single tool execution
- Pipeline with jq
- Multi-step orchestration (variables, command substitution)
- Error handling and fallback (`||`)
- Stdin piping
- Loops and conditionals
- Environment variables
- Status callbacks
- Multiple sequential `execute()` calls (Arc reuse)

## Security

Inherits all bashkit sandbox guarantees:
- Virtual filesystem (no host access)
- Resource limits (max commands, loop iterations, function depth)
- No network access unless explicitly configured

Sub-tool callback implementations control their own security boundaries.
