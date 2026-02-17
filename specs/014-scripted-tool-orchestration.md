# Spec 014: Tool Orchestration

## Summary

Compose tool definitions (`ToolDef`) + execution callbacks into a single `OrchestratorTool` that accepts bash scripts. Each sub-tool becomes a builtin command, letting LLMs orchestrate N tools in one call using pipes, variables, loops, and conditionals.

## Feature flag

`orchestrator` — the entire module is gated behind `#[cfg(feature = "orchestrator")]`.

## Motivation

When an LLM has access to many tools (get_user, list_orders, get_inventory, etc.), each tool call is a separate round-trip. A data-gathering task that needs 5 tools requires 5+ turns. With `OrchestratorTool`, the LLM writes a single bash script that calls all tools, pipes results through `jq`, and returns composed output — reducing latency and token cost.

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

### ToolCallback

```rust
pub type ToolCallback =
    Arc<dyn Fn(&[String], Option<&str>) -> Result<String, String> + Send + Sync>;
```

- `args`: positional args after the command name.
- `stdin`: pipeline input from prior command.
- Returns stdout string on success, error message on failure.

### OrchestratorToolBuilder

Two arguments per tool: definition + callback.

```rust
OrchestratorTool::builder("api_name")
    .short_description("...")
    .tool(
        ToolDef::new("get_user", "Fetch user by ID")
            .with_schema(json!({"type": "object", "properties": {"id": {"type": "integer"}}})),
        |args, _stdin| {
            let id = args.first().ok_or("missing id")?;
            Ok(format!("{{\"id\":{id}}}\n"))
        },
    )
    .env("API_KEY", "...")
    .limits(ExecutionLimits::new().max_commands(500))
    .build()
```

### ToolBuiltinAdapter (internal)

Wraps `ToolCallback` (Arc) as a `Builtin` for the interpreter. Cheap Arc clone into each Bash instance.

### OrchestratorTool

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
  Schema: {"type":"object","properties":{"id":{"type":"integer"}}}
- `list_orders`: List orders for user. Usage: list_orders <user_id>

## Tips

- Pipe tool output through `jq` for JSON processing
- Use variables to pass data between tool calls
```

## Module location

`crates/bashkit/src/orchestrator/`

```
orchestrator/
├── mod.rs       — ToolDef, ToolCallback, OrchestratorToolBuilder, OrchestratorTool struct, tests
└── execute.rs   — Tool impl, ToolBuiltinAdapter, documentation helpers
```

Public exports from `lib.rs` (gated by `orchestrator` feature):
`ToolDef`, `ToolCallback`, `OrchestratorTool`, `OrchestratorToolBuilder`.

## Example

`crates/bashkit/examples/orchestrator.rs` — e-commerce API demo with get_user, list_orders, get_inventory, create_discount. Uses `ToolDef` + closures (no trait impls needed).

Run: `cargo run --example orchestrator --features orchestrator`

## Test coverage

20 unit tests covering:
- Builder configuration (name, description, defaults)
- Introspection (help, system_prompt, schemas, schema rendering)
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
