# Spec 014: Scripted Tool Orchestration

## Summary

Compose multiple `CallableTool`s into a single `OrchestratorTool` that accepts bash scripts. Each sub-tool becomes a builtin command, letting LLMs orchestrate N tools in one call using pipes, variables, loops, and conditionals.

## Motivation

When an LLM has access to many tools (get_user, list_orders, get_inventory, etc.), each tool call is a separate round-trip. A data-gathering task that needs 5 tools requires 5+ turns. With `OrchestratorTool`, the LLM writes a single bash script that calls all tools, pipes results through `jq`, and returns composed output — reducing latency and token cost.

## Design

### CallableTool trait

```rust
pub trait CallableTool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn call(&self, args: &[String], stdin: Option<&str>) -> Result<String, String>;
}
```

- Sync by design. Most sub-tools are fast (HTTP stubs, cache, mock data).
- `args`: positional args after the command name.
- `stdin`: pipeline input from prior command.
- Returns stdout string on success, error message on failure.

### CallableToolBuiltin adapter

Internal struct wrapping `Arc<dyn CallableTool>`. Implements `Builtin` so the interpreter can execute it. Arc sharing ensures the same tools survive across multiple `execute()` calls.

### OrchestratorToolBuilder

```rust
OrchestratorTool::builder("api_name")
    .short_description("...")
    .tool(Box::new(GetUser))
    .tool(Box::new(ListOrders))
    .env("API_KEY", "...")
    .limits(ExecutionLimits::new().max_commands(500))
    .build()
```

### OrchestratorTool

Implements the `Tool` trait. On each `execute()`:

1. Creates a fresh `Bash` instance.
2. Registers each `CallableTool` as a builtin via `Arc::clone`.
3. Runs the user-provided script.
4. Returns `ToolResponse { stdout, stderr, exit_code }`.

Reusable — multiple `execute()` calls share the same `Arc<dyn CallableTool>` instances.

### LLM integration

`system_prompt()` generates markdown with available tool commands and tips. Example output:

```markdown
# api_name

Input: {"commands": "<bash script>"}
Output: {stdout, stderr, exit_code}

## Available tool commands

- `get_user`: Fetch user by ID. Usage: get_user <id>
- `list_orders`: List orders for user. Usage: list_orders <user_id>

## Tips

- Pipe tool output through `jq` for JSON processing
- Use variables to pass data between tool calls
```

## Module location

`crates/bashkit/src/scripted_tool.rs`

Public exports from `lib.rs`: `CallableTool`, `OrchestratorTool`, `OrchestratorToolBuilder`.

## Example

`crates/bashkit/examples/scripted_tool_orchestration.rs` — e-commerce API demo with get_user, list_orders, get_inventory, create_discount.

## Test coverage

19 unit tests covering:
- Builder configuration (name, description, defaults)
- Introspection (help, system_prompt, schemas)
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

Sub-tool implementations control their own security boundaries.
