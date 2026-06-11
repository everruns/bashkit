# Scripted Tool Orchestration

## Status

Implemented

## Summary

Compose tool definitions (`ToolDef`) + execution callbacks into a single `ScriptedTool` that accepts bash scripts. Each sub-tool becomes a builtin command, letting LLMs orchestrate N tools in one call using pipes, variables, loops, and conditionals.

`ScriptedTool` always runs in code/logic mode: bash is the control-flow and data-transformation language, not a VFS shell — filesystem primitives, path script execution, file redirection, and process substitution are unavailable.

`ScriptedToolBuilder` and `ScriptingToolSetBuilder` also implement the shared toolkit-library contract from [the tool contract](./tool-contract.md): locale-aware metadata, `build_service()`, `build_tool_definition()`, `build_input_schema()`, `build_output_schema()`, single-use `ToolExecution`.

## Feature flag

`scripted_tool` — entire module gated behind `#[cfg(feature = "scripted_tool")]`.

## Motivation

With many tools, each LLM tool call is a separate round-trip; a 5-tool data-gathering task costs 5+ turns. `ScriptedTool` lets the LLM write one bash script that calls all tools, pipes results through `jq`, and returns composed output — reducing latency and token cost.

Intended use is "code mode" only. Not for project/file manipulation; use `Bash` / `BashTool` when a virtual filesystem is part of the task.

## Design

### ToolDef — OpenAPI-style tool definition

```rust
pub struct ToolDef {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,  // JSON Schema, `{}` if unset
    pub tags: Vec<String>,                // optional, for discovery
    pub category: Option<String>,         // optional, for discovery
}
```

Builder: `new(name, description)`, `with_schema`, `with_tags`, `with_category`. Tags are free-form labels (e.g. `["admin", "billing"]`); category is a grouping key (e.g. `"payments"`) for progressive discovery.

### ToolArgs — parsed arguments passed to callbacks

```rust
pub struct ToolArgs {
    pub params: serde_json::Value,  // JSON object from --key value flags
    pub stdin: Option<String>,      // pipeline input from prior command
}
```

Typed accessors: `param_str` / `param_i64` / `param_f64` / `param_bool`.

### Callbacks

```rust
pub type ToolCallback = Arc<dyn Fn(&ToolArgs) -> Result<String, String> + Send + Sync>;
pub type AsyncToolCallback = Arc<
    dyn Fn(ToolArgs) -> Pin<Box<dyn Future<Output = Result<String, String>> + Send>> + Send + Sync,
>;
```

Return stdout string on success, error message on failure. Async takes owned `ToolArgs` (future may outlive the borrow); register via `.async_tool_fn(def, cb)`. Sync and async mix freely in one `ScriptedTool` — internally `CallbackKind::Async`, `.await`-ed inside `ToolBuiltinAdapter::execute()` (already `async fn`).

### ToolImpl — unified tool unit

```rust
pub struct ToolImpl {
    pub def: ToolDef,
    pub exec: Option<AsyncToolExec>,
    pub exec_sync: Option<SyncToolExec>,
    pub sanitize_errors: bool,
}
```

Combines metadata with optional sync + async exec fns (`with_exec`, `with_exec_sync`). Implements `Builtin`, so registrable in both `Bash` (`.builtin()`) and `ScriptedTool`/`ScriptingToolSet` (`.tool()`). Async path prefers `exec`, falls back to `exec_sync`; sync path the reverse (blocking on `exec`).

When used directly as a `Builtin`, callback `Err(String)` values are sanitized by default to `"<tool>: callback failed\n"` before reaching script-visible stderr — matches `ScriptedTool`'s safe default and prevents leaking host-side secrets, paths, connection strings, or stack traces. Trusted deployments opt out with `.sanitize_errors(false)`.

Backward-compat aliases: `ToolCallback = SyncToolExec`, `AsyncToolCallback = AsyncToolExec`.

### ToolDefExtension — Bash extension for ToolDef-backed commands

Implements the shared `Extension` trait; registers a group of `ToolDef`/callback pairs into any `Bash` instance. Contributes: one builtin per tool, `help`, `discover`, plus `--help`, `--dry-run`, callback error sanitization, and invocation tracing shared with `ScriptedTool`. `ScriptedTool` builds its per-call logic-only shell by installing this extension, so plain `Bash` and `ScriptedTool` use one command adapter path.

Invocation tracing is isolated by default: every `ToolDefExtensionBuilder::build()` mints a fresh bounded trace log, and `Clone` copies command configuration into a new empty log rather than sharing trace state. Hosts that need traces after moving an extension into a `Bash` must call `ToolDefExtension::invocation_trace()` first and retain the returned `ToolDefInvocationTrace` handle. Do not share one trace handle across tenants.

### ContextVar propagation (Python)

Python callbacks (sync and async) automatically see `contextvars.ContextVar` values set by the caller at `execute()` / `execute_sync()` time:

1. Each Python surface owns one long-lived callback engine holding reusable machinery only: `ctx.run(...)` callback entry and one cached private asyncio loop for sync fallback.
2. Each `execute()` / `execute_sync()` call creates a fresh callback session snapshotting the caller's `contextvars` state.
3. `execute()` also captures the caller's active asyncio loop via `TaskLocals`, so async callbacks schedule back onto that loop.
4. `Bash` / `BashTool` pass the callback session through bashkit's generic execution extensions, so persistent builtin adapters resolve request-scoped callback state without mutating shared runtime state.
5. Sync callbacks invoke via `ctx.run(fn, params, stdin)`.
6. Async callbacks are created under `ctx.run(...)`; when run on the caller loop, the session owns the spawned Python tasks so cancellation only affects that execution's callbacks.
7. `execute_sync()` has no caller-owned loop, so async callbacks fall back to the engine's private loop on the worker thread. Sync support preserved, but loop-bound caller resources only work with `await execute()`.

Enables framework patterns like LangGraph's `get_stream_writer()` and FastAPI request-scoped state.

**Caveat:** `execute_sync()` must not be called from an async endpoint running on the same thread as a Python event loop; use `await execute()` instead.

### Flag parsing

Implementation and per-syntax precedence: `parse_flags` in `crates/bashkit/src/tool_def.rs`. Rules:

- `--key value` and `--key=value` parse into a JSON object. Type coercion follows `input_schema` property types (`integer`, `number`, `boolean`, `string`, `array`, `object`); bare `--flag` is `true` when schema says boolean. Unknown flags (not in schema) stay strings.
- Bounded before callback execution: parsed flag value bytes capped at 64 KiB per command invocation; array-typed flags capped at 4096 items after JSON parsing, comma splitting, and repeated-invocation appends. Oversized input fails before allocating the full `ToolArgs.params`.
- Aggregate types resolve through `$ref`, `oneOf`/`anyOf`/`allOf` branches, nullable shorthand (`type: ["array","null"]`), and implicit signals (`items` ⇒ array, `properties` ⇒ object). When the resolved type is aggregate and the raw value starts `[` or `{`, it parses as JSON; on parse failure the original string is preserved so downstream serde validation produces the real error.
- Schema-driven shorthand for aggregate flags:
  - **Object via pairs**: `--flag key=value ...` collected into one object, terminating at the next `--flag` or end of args; keys matched against object-schema property names (unknown keys error); values coerced per nested property schema.
  - **Array of objects via repeated pair groups**: each `--flag <pairs...>` contributes one object; repeats append. Mixing JSON and pair forms across invocations is allowed, within one invocation rejected.
  - **Array of scalars**: single arg comma-split (`--tags a,b,c`); repeats append; JSON form still works.
- Help output (`usage_from_schema`) advertises both forms: `--server <json|key=value...>`, `--tags <json|a,b,c>`.

### ScriptedToolBuilder

Two arguments per tool: definition + callback. `.tool_fn()` sync, `.async_tool_fn()` async; plus `.locale()`, `.short_description()`, `.env()`, `.limits()`, `.compact_prompt()`. Full example: `crates/bashkit/examples/scripted_tool.rs`.

### ToolBuiltinAdapter (internal)

Wraps a callback as a `Builtin`: parses `--key value` flags from `ctx.args` using the tool's schema, then calls the callback with `ToolArgs`.

### ScriptedTool

Implements the `Tool` trait. Each `execute()`: fresh logic-only `Bash`, callbacks registered via `Arc::clone`, run script, return `ToolResponse { stdout, stderr, exit_code }`. Reusable — multiple `execute()` calls share the same `Arc` callback instances.

The logic-only shell keeps: variables, arrays, functions, arithmetic, command substitution; `if`/`case`/`for`/`while`; pipelines, heredocs, here-strings; callback commands plus `help` and `discover`; stdin transforms (`jq`, `grep`, `sed`, `awk`, `sort`, `cut`, `tr`, `wc`, `head`, `tail`, `seq`, `expr`).

Rejected filesystem surfaces: file commands (`cat`, `ls`, `find`, `mkdir`, `rm`, `cp`, `mv`, `touch`, `chmod`, `ln`, `stat`, `source`, `.`); path execution (`/tmp/script.sh`, `$PATH` lookup); file redirection (`<`, `>`, `>>`, `&>`) except `/dev/null`; process substitution; file operands to dual-use tools — the internal filesystem rejects all real operations with `filesystem access disabled`.

### Built-in `help` command

`help --list` (names + descriptions), `help <tool>` (usage), `help <tool> --json` (machine-readable: `name`, `description`, `input_schema`) — lets LLMs discover enum values, required fields, etc. at runtime without loading all schemas into context.

### Compact prompt mode

`ScriptedToolBuilder::compact_prompt(true)` switches `system_prompt()` to tool names + one-liners, deferring full schemas to `help`. For large tool sets (50+). Default `false` (full schemas, backward compatible).

### Built-in `discover` command

`discover --categories | --category X | --tag Y | --search text`, each with optional `--json`. Tools need `tags`/`category` set via `ToolDef` to appear in filtered results.

### LLM integration

`system_prompt()` generates markdown with available tool commands, input schemas (when present), and usage tips — see the `Tool` impl in `crates/bashkit/src/scripted_tool/execute.rs`.

### Shared context across callbacks

Standard Rust closure capture: clone an `Arc` (e.g. an authenticated client) into each closure; `Arc<Mutex<T>>` for mutable state. No API change needed.

### State across execute() calls

Each `execute()` creates a fresh Bash interpreter (security: clean sandbox per call). The LLM carries state via its context window. Callback-level persistence: `Arc` state captured in closures persists across `execute()` calls since the same callback instances are reused.

### Execution trace access

`take_last_execution_trace()` returns inner command invocations from the most recent `execute()` — observability/eval telemetry, not scoring. Entries record command name, kind (`tool` / `help` / `discover`), raw argv tokens, exit code.

### ScriptingToolSet — mode-controlled multi-tool wrapper

Wraps `ScriptedTool`; `tools()` returns one or two tools by `DiscoveryMode`:

| Mode | `tools()` returns | When to use |
|------|------------------|-------------|
| `Exclusive` (default) | 1 tool: `ScriptedTool` with full schemas | Only tool the LLM has |
| `WithDiscovery` | 2 tools: `ScriptedTool` (compact) + `DiscoverTool` (`{name}_discover`) | Alongside other tools, or large tool sets |

`ScriptingToolSet` does **not** implement `Tool` itself — call `tools()` for `Vec<Box<dyn Tool>>` and register each. In discovery mode the script tool gets a compact prompt (names only) and all builtins; the discover tool's prompt focuses on `discover`/`help` and shares the same inner `ScriptedTool`, so the LLM can explore schemas before writing scripts. Builder mirrors `ScriptedToolBuilder` plus `.with_discovery()`.

## Module location

`crates/bashkit/src/tool_def.rs` (ToolDef, ToolArgs, ToolImpl, exec types, `parse_flags`) and `crates/bashkit/src/scripted_tool/` (builder, extension + builtins, `Tool` impl, toolset). Public exports gated by the `scripted_tool` feature in `lib.rs`.

## Example

`crates/bashkit/examples/scripted_tool.rs` — e-commerce API demo using `ToolDef` + closures (no trait impls). Run: `cargo run --example scripted_tool --features scripted_tool`.

## Test coverage

Unit tests in the module cover builder configuration, help/discover introspection, flag parsing and coercion, pipelines, multi-step orchestration, error handling/fallback, stdin piping, loops, env vars, Arc reuse and fresh-interpreter isolation across `execute()` calls, and shared `Arc`/`Arc<Mutex<T>>` context.

## Security

Inherits all bashkit sandbox guarantees: virtual filesystem (no host access), resource limits, no network unless explicitly configured. `ScriptedTool` further uses a disabled filesystem backend and a reduced builtin surface so scripts cannot use VFS storage or path-based script execution. Sub-tool callback implementations control their own security boundaries.
