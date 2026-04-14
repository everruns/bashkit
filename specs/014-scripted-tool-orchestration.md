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

### AsyncToolCallback

```rust
pub type AsyncToolCallback = Arc<
    dyn Fn(ToolArgs) -> Pin<Box<dyn Future<Output = Result<String, String>> + Send>>
        + Send + Sync,
>;
```

Async variant of `ToolCallback`. Takes owned `ToolArgs` (the future may outlive the
borrow). Register via `builder.async_tool_fn(def, callback)`. Both sync and async
callbacks can be mixed in a single `ScriptedTool`.

Internally represented as `CallbackKind::Async` and `.await`-ed inside
`ToolBuiltinAdapter::execute()`, which is already `async fn`.

### ToolImpl — unified tool unit

```rust
pub struct ToolImpl {
    pub def: ToolDef,
    pub exec: Option<AsyncToolExec>,
    pub exec_sync: Option<SyncToolExec>,
}
```

Combines metadata (`ToolDef`) with optional sync and async exec functions.
Implements `Builtin`, so it can be registered in both `Bash` (via `.builtin()`)
and `ScriptedTool`/`ScriptingToolSet` (via `.tool()`).

When running async, prefers `exec`; falls back to `exec_sync`.
When running sync, prefers `exec_sync`; falls back to blocking on `exec`.

Builder API:

```rust
let tool = ToolImpl::new(
    ToolDef::new("get_user", "Fetch user by ID")
        .with_schema(json!({"type": "object", "properties": {"id": {"type": "integer"}}})),
)
.with_exec_sync(|args| {
    let id = args.param_i64("id").ok_or("missing --id")?;
    Ok(format!("{{\"id\":{id}}}\n"))
})
.with_exec(|args| async move {
    let id = args.param_i64("id").ok_or("missing --id")?;
    Ok(format!("{{\"id\":{id}}}\n"))
});

// Register in ScriptedTool
ScriptedTool::builder("api").tool(tool).build();
```

Type aliases for backward compatibility: `ToolCallback = SyncToolExec`,
`AsyncToolCallback = AsyncToolExec`.

### ContextVar propagation (Python)

Python callbacks (both sync and async) automatically see `contextvars.ContextVar`
values that were set by the caller at `execute()` / `execute_sync()` time:

1. `build_rust_tool()` (called at execute time) snapshots the caller's context
   via `contextvars.copy_context()`.
2. Sync callbacks are invoked via `ctx.run(fn, params, stdin)`.
3. Async callbacks are invoked via
   `ctx.run(lambda: loop.run_until_complete(fn(params, stdin)))` so that the
   `asyncio.Task` inherits the captured context.

This enables framework patterns like LangGraph's `get_stream_writer()` and
FastAPI's request-scoped state.

**Caveat:** `execute_sync()` must not be called from an async endpoint that runs
on the same thread as a Python event loop. Use `await execute()` from async
contexts instead.

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

Two arguments per tool: definition + callback. Use `.tool_fn()` for sync and
`.async_tool_fn()` for async callbacks.

```rust
ScriptedTool::builder("api_name")
    .locale("en-US")
    .short_description("...")
    .tool_fn(
        ToolDef::new("get_user", "Fetch user by ID")
            .with_schema(json!({"type": "object", "properties": {"id": {"type": "integer"}}})),
        |args| {
            let id = args.param_i64("id").ok_or("missing --id")?;
            Ok(format!("{{\"id\":{id}}}\n"))
        },
    )
    .async_tool_fn(
        ToolDef::new("fetch_url", "Fetch a URL"),
        |args| async move {
            let url = args.param_str("url").unwrap_or("?");
            Ok(format!("{{\"url\":\"{url}\",\"status\":200}}\n"))
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
    .tool_fn(...)
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
builder.tool_fn(ToolDef::new("get_user", "..."), move |args| {
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

### Execution trace access

`ScriptedTool` records inner command invocations from the most recent `execute()` call and
exposes them via `take_last_execution_trace()`. This trace is for observability and eval
telemetry, not scoring:

```rust
let mut tool = ScriptedTool::builder("api").tool_fn(...).build();
let _resp = tool.execute(ToolRequest::new("discover --search user\nhelp get_user")).await;
let trace = tool.take_last_execution_trace().unwrap();
assert_eq!(trace.invocations[0].name, "discover");
```

Trace entries record:
- command name
- kind: `tool`, `help`, or `discover`
- raw argv tokens
- exit code

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

### ScriptingToolSet — mode-controlled multi-tool wrapper

`ScriptingToolSet` wraps `ScriptedTool` and exposes one or two tools via `tools()`
based on `DiscoveryMode`:

| Mode | `tools()` returns | When to use |
|------|------------------|-------------|
| `Exclusive` (default) | 1 tool: `ScriptedTool` with full schemas | Only tool the LLM has |
| `WithDiscovery` | 2 tools: `ScriptedTool` (compact) + `DiscoverTool` | Alongside other tools, or large tool sets |

```rust
// Exclusive mode (default): tools() returns [ScriptedTool]
let toolset = ScriptingToolSet::builder("api")
    .short_description("My API")
    .tool_fn(ToolDef::new("get_user", "Fetch user").with_schema(...), callback)
    .build();
let tools = toolset.tools(); // vec![ScriptedTool]

// Discovery mode: tools() returns [ScriptedTool, DiscoverTool]
let toolset = ScriptingToolSet::builder("api")
    .short_description("My API")
    .tool_fn(ToolDef::new("get_user", "Fetch user").with_category("users"), callback)
    .with_discovery()
    .build();
let tools = toolset.tools(); // vec![ScriptedTool(compact), DiscoverTool]
assert_eq!(tools[0].name(), "api");           // script tool
assert_eq!(tools[1].name(), "api_discover");   // discover tool
```

`ScriptingToolSet` does **not** implement `Tool` itself. Instead, call `tools()` to
get `Vec<Box<dyn Tool>>` and register each with your LLM.

In discovery mode:
- The **script tool** (`{name}`) has a compact prompt (tool names only, no schemas)
  and all builtins (tools + help + discover).
- The **discover tool** (`{name}_discover`) has a prompt focused on `discover`/`help`
  commands. It shares the same inner `ScriptedTool`, so the LLM can explore schemas
  before writing scripts.

Builder API mirrors `ScriptedToolBuilder`: `.tool()`, `.env()`, `.limits()`,
`.short_description()`, plus `.with_discovery()` to switch mode.

## Module location

```
tool_def.rs          — ToolDef, ToolArgs, ToolImpl, SyncToolExec, AsyncToolExec, parse_flags
scripted_tool/
├── mod.rs           — CallbackKind, ScriptedToolBuilder, ScriptedTool, re-exports from tool_def
├── execute.rs       — Tool impl, ToolBuiltinAdapter, documentation helpers
└── toolset.rs       — ScriptingToolSet, ScriptingToolSetBuilder, DiscoveryMode
```

Public exports from `lib.rs` (gated by `scripted_tool` feature):
`ToolDef`, `ToolArgs`, `ToolImpl`, `SyncToolExec`, `AsyncToolExec`,
`ToolCallback`, `AsyncToolCallback` (aliases), `ScriptedTool`, `ScriptedToolBuilder`,
`ScriptingToolSet`, `ScriptingToolSetBuilder`, `DiscoverTool`, `DiscoveryMode`.

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
