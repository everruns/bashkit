# 009: Tool Contract

## Status
Implemented

## Overview

The `Tool` trait defines the public contract for LLM tool integration. This is a **public library contract** - any breaking changes require a major version bump.

## Decision

### Tool Trait (Public Contract)

```rust
#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn short_description(&self) -> &str;
    fn description(&self) -> String;
    fn llmtext(&self) -> String;
    fn system_prompt(&self) -> String;
    fn input_schema(&self) -> serde_json::Value;
    fn output_schema(&self) -> serde_json::Value;
    fn version(&self) -> &str;
    async fn execute(&mut self, req: ToolRequest) -> ToolResponse;
    async fn execute_with_status(
        &mut self,
        req: ToolRequest,
        status_callback: Box<dyn FnMut(ToolStatus) + Send>,
    ) -> ToolResponse;
}
```

### Method Purposes

| Method | Purpose | Dynamic |
|--------|---------|---------|
| `name()` | Tool identifier for registries | No |
| `short_description()` | One-liner for tool listings | No |
| `description()` | Full description with config | Yes |
| `llmtext()` | Full docs for LLM consumption | Yes |
| `system_prompt()` | Token-efficient for sysprompt | Yes |
| `input_schema()` | JSON Schema for validation | No |
| `output_schema()` | JSON Schema for output | No |
| `version()` | Library version | No |
| `execute()` | Run the tool | - |
| `execute_with_status()` | Run with progress callbacks | - |

### Real Outputs

#### `name()`
```
bashkit
```

#### `short_description()`
```
Sandboxed bash interpreter with virtual filesystem
```

#### `description()`
```
Sandboxed bash interpreter with virtual filesystem
```

With custom builtins:
```
Sandboxed bash interpreter with virtual filesystem. Custom: my_cmd, other_cmd
```

#### `system_prompt()` (token-efficient)
```
bashkit: sandboxed bash with vfs.
Input: {"commands": "..."}
Output: {stdout, stderr, exit_code}
Builtins: echo cat grep sed awk jq curl head tail sort uniq cut tr wc date sleep mkdir rm cp mv touch chmod printf test [ true false exit cd pwd ls find xargs basename dirname env export read
```

#### `llmtext()` (full documentation)
```markdown
# BashKit

Sandboxed bash interpreter with virtual filesystem.

## Capabilities

- Full bash syntax: variables, pipelines, redirects, loops, functions, arrays
- 30+ builtins: echo, cat, grep, sed, awk, jq, curl, etc.
- Virtual filesystem (all operations sandboxed)
- Resource limits (commands, iterations, function depth)

## Input

- `commands` (required): Bash commands to execute (like `bash -c`)

## Output

- `stdout`: Standard output
- `stderr`: Standard error
- `exit_code`: 0 = success

## Examples

{"commands": "echo 'Hello'"}
→ {"stdout": "Hello\n", "stderr": "", "exit_code": 0}

{"commands": "x=5; y=3; echo $((x + y))"}
→ {"stdout": "8\n", "stderr": "", "exit_code": 0}

{"commands": "echo '{\"n\":1}' | jq '.n'"}
→ {"stdout": "1\n", "stderr": "", "exit_code": 0}

## Running Scripts from VFS

{"commands": "source /path/to/script.sh"}

## Errors

- Syntax error: non-zero exit, error in stderr
- Command not found: exit code 127
- Resource limit: specific error message
```

### Request/Response

```rust
pub struct ToolRequest {
    pub commands: String,  // Like bash -c "commands"
}

pub struct ToolResponse {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    pub error: Option<String>,  // Error category if failed
}
```

### BashTool Implementation

`BashTool` is the sandboxed bash interpreter implementing the `Tool` trait.

```rust
let mut tool = BashTool::builder()
    .username("agent")
    .hostname("sandbox")
    .limits(ExecutionLimits::new().max_commands(1000))
    .env("API_KEY", "secret")
    .builtin("custom_cmd", Box::new(MyBuiltin))
    .build();

let response = tool.execute(ToolRequest {
    commands: "echo hello".to_string(),
}).await;
```

### Dynamic Documentation

When configured, `llmtext()` and `system_prompt()` automatically include:

- Custom builtin names
- Sandbox identity (username/hostname)
- Resource limits
- Pre-set environment variable names (not values)

## Design Rationale

### Why a trait?

Allows multiple tool implementations to share the same interface. Future tools (calculator, file search, etc.) can implement `Tool` for uniform LLM integration.

### Why `commands` not `script`?

Aligns with `bash -c "commands"` semantics. Clearer that it's inline commands, not a script file.

### Why no `timeout_ms`?

Use `timeout` builtin in commands: `timeout 5 long_running_cmd`. Keeps the API simple.

### Why `system_prompt()` separate from `llmtext()`?

- `llmtext()`: Full docs with examples, for tool discovery and help
- `system_prompt()`: Minimal tokens, for embedding in system prompts

## Verification

```bash
cargo test tool::
cargo run --example show_tool_output
```

## See Also

- [001-architecture.md](001-architecture.md) - Overall architecture
- [005-builtins.md](005-builtins.md) - Builtin implementation
