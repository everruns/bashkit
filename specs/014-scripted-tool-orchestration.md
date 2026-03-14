# Spec 014: Scripted Tool Orchestration

## Status

Implemented

## Summary

Compose tool definitions (`ToolDef`) + execution callbacks into a single `ScriptedTool` that accepts bash scripts. Each sub-tool becomes a builtin command, letting LLMs orchestrate N tools in one call using pipes, variables, loops, and conditionals.

`ScriptedToolBuilder` and `ScriptingToolSetBuilder` also implement the shared toolkit-library contract from [spec 009](./009-tool-contract.md): locale-aware metadata, `build_service()`, `build_tool_definition()`, `build_input_schema()`, `build_output_schema()`, and single-use `ToolExecution`.

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
    pub tags: Vec<String>,               // categorical tags for discovery
    pub category: Option<String>,        // grouping category for discovery
}

impl ToolDef {
    pub fn new(name: impl Into<String>, description: impl Into<String>) -> Self;
    pub fn with_schema(self, schema: serde_json::Value) -> Self;
    pub fn with_tags(self, tags: &[&str]) -> Self;
    pub fn with_category(self, category: &str) -> Self;
}
```

Standard OpenAPI fields: `name`, `description`, `input_schema`. Schema is optional — defaults to `{}`.

Tags and category are optional metadata for progressive discovery. Tags are free-form labels
(e.g. `["admin", "billing"]`), category is a grouping key (e.g. `"payments"`).

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
    .locale("en-US")
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

### Built-in `help` command

Registered automatically alongside user tools. Provides runtime schema introspection:

```bash
help --list              # List all tool names + descriptions
help get_user            # Human-readable usage
help get_user --json     # Machine-readable JSON (pipeable to jq)
```

JSON output includes `name`, `description`, and `input_schema` — letting LLMs discover
enum values, required fields, etc. at runtime without loading all schemas into context.

### Compact prompt mode

`ScriptedToolBuilder::compact_prompt(true)` switches `system_prompt()` to a compact form
that lists only tool names + one-liners, deferring full schemas to `help`:

```rust
ScriptedTool::builder("api")
    .compact_prompt(true)
    .tool(...)
    .build()
```

This reduces context window usage for large tool sets (50+). Default: `false` (full
schemas in prompt, backward compatible).

### Built-in `discover` command

Registered automatically alongside `help`. Provides progressive tool discovery for large tool sets:

```bash
discover --categories           # List all categories with tool counts
discover --category payments    # List tools in a category
discover --tag admin            # Filter by tag
discover --search user          # Search name + description (case-insensitive)
discover --category payments --json  # Any mode supports --json output
```

Tools must have `tags` and/or `category` set via `ToolDef::with_tags()` / `ToolDef::with_category()` to appear in filtered results.

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

### Shared context across callbacks

Use the standard Rust closure-capture pattern with `Arc` to share resources:

```rust
let client = Arc::new(build_authenticated_client());
let c = client.clone();
builder.tool(ToolDef::new("get_user", "..."), move |args| {
    let resp = c.get(&format!("/users/{}", args.param_i64("id").unwrap()));
    Ok(resp.text()?)
});
```

For mutable state, use `Arc<Mutex<T>>`. No API change needed — closures handle it naturally.

### State across execute() calls

Each `execute()` creates a fresh Bash interpreter (security: clean sandbox per call).
The LLM carries state via its context window — it sees stdout from each call and passes
relevant data into the next script.

For callback-level persistence, `Arc` state in closures persists across `execute()` calls
since the same `Arc<ToolCallback>` instances are reused.

### MCP integration

`McpServer` in `bashkit-cli` can expose ScriptedTools over MCP's JSON-RPC protocol.
Each registered ScriptedTool appears as a separate MCP tool in `tools/list`:

```rust
let mut server = McpServer::new();
server.register_scripted_tool(my_tool);
server.run().await?;
```

- `tools/list` returns the default `bash` tool plus all registered ScriptedTools
- `tools/call` routes by tool name: ScriptedTool names go to `ScriptedTool::execute()`,
  `bash` goes to the default handler
- Gated behind `scripted_tool` feature flag on `bashkit-cli`
- Existing `bash` tool unaffected (backward compatible)

### ScriptingToolSet — mode-controlled wrapper

`ScriptingToolSet` is a higher-level wrapper around `ScriptedTool` that controls
`system_prompt()` generation based on a `DiscoveryMode`:

| Mode | system_prompt() | When to use |
|------|----------------|-------------|
| `Exclusive` (default) | Full schemas, usage hints | Only tool the LLM has |
| `WithDiscovery` | Semantic descriptions + discover/help instructions | Alongside other tools, or large tool sets |

```rust
// Exclusive mode (default): full schemas in prompt
let toolset = ScriptingToolSet::builder("api")
    .short_description("My API")
    .tool(ToolDef::new("get_user", "Fetch user").with_schema(...), callback)
    .build();

// Discovery mode: semantic-only prompt
let toolset = ScriptingToolSet::builder("api")
    .short_description("My API")
    .tool(ToolDef::new("get_user", "Fetch user").with_category("users"), callback)
    .with_discovery()
    .build();
```

`ScriptingToolSet` implements `Tool` — delegates `execute()` to inner `ScriptedTool`,
overrides `system_prompt()` based on mode. In discovery mode, the prompt tells the LLM
to use `discover` and `help` builtins rather than listing full schemas.

Builder API mirrors `ScriptedToolBuilder`: `.tool()`, `.env()`, `.limits()`,
`.short_description()`, plus `.with_discovery()` to switch mode.

## Module location

`crates/bashkit/src/scripted_tool/`

```
scripted_tool/
├── mod.rs       — ToolDef, ToolCallback, ScriptedToolBuilder, ScriptedTool struct, tests
├── execute.rs   — Tool impl, ToolBuiltinAdapter, documentation helpers
└── toolset.rs   — ScriptingToolSet, ScriptingToolSetBuilder, DiscoveryMode
```

Public exports from `lib.rs` (gated by `scripted_tool` feature):
`ToolDef`, `ToolArgs`, `ToolCallback`, `ScriptedTool`, `ScriptedToolBuilder`,
`ScriptingToolSet`, `ScriptingToolSetBuilder`, `DiscoveryMode`.

## Example

`crates/bashkit/examples/scripted_tool.rs` — e-commerce API demo with get_user, list_orders, get_inventory, create_discount. Uses `ToolDef` + closures (no trait impls needed).

Run: `cargo run --example scripted_tool --features scripted_tool`

## Test coverage

50 unit tests covering:
- Builder configuration (name, description, defaults, compact_prompt)
- Introspection (help, system_prompt, schemas, schema rendering)
- Help builtin (--list, human-readable, --json, unknown tool, jq piping, compact vs full prompt)
- Discover builtin (--categories, --category, --tag, --search, --json, no-args usage, case-insensitive search, tag JSON, ToolDef with_tags/with_category)
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
- Shared context: Arc across callbacks, mutable Arc<Mutex<T>>
- Interpreter isolation: fresh per execute(), Arc callback persistence

## Security

Inherits all bashkit sandbox guarantees:
- Virtual filesystem (no host access)
- Resource limits (max commands, loop iterations, function depth)
- No network access unless explicitly configured

Sub-tool callback implementations control their own security boundaries.
