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
    fn help(&self) -> String;
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
| `description()` | Full description with supported tools | Yes |
| `help()` | Man-page style docs for LLMs | Yes |
| `system_prompt()` | Structured prompt header | Yes |
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
Virtual bash interpreter with virtual filesystem
```

#### `description()`
```
Virtual bash interpreter with virtual filesystem. Supported tools: echo cat grep sed awk jq curl head tail sort uniq cut tr wc date sleep mkdir rm cp mv touch chmod printf test [ true false exit cd pwd ls find xargs basename dirname env export read
```

#### `system_prompt()`
```
# Bash Tool

Virtual bash interpreter with virtual filesystem.

Input: {"commands": "<bash commands>"}
Output: {stdout, stderr, exit_code}
```

With username configured:
```
# Bash Tool

Virtual bash interpreter with virtual filesystem.
Home: /home/agent

Input: {"commands": "<bash commands>"}
Output: {stdout, stderr, exit_code}
```

#### `help()` (man-page format)
```
BASH(1)                          User Commands                         BASH(1)

NAME
       bashkit - virtual bash interpreter with virtual filesystem

SYNOPSIS
       {"commands": "<bash commands>"}

DESCRIPTION
       Bashkit executes bash commands in a virtual environment with a virtual
       filesystem. All file operations are contained within the virtual environment.

       Supports full bash syntax including variables, pipelines, redirects,
       loops, conditionals, functions, and arrays.

BUILTINS
       echo, cat, grep, sed, awk, jq, curl, head, tail, sort, uniq, cut, tr,
       wc, date, sleep, mkdir, rm, cp, mv, touch, chmod, printf, test, [,
       true, false, exit, cd, pwd, ls, find, xargs, basename, dirname, env,
       export, read

INPUT
       commands    Bash commands to execute (like bash -c "commands")

OUTPUT
       stdout      Standard output from the commands
       stderr      Standard error from the commands
       exit_code   Exit status (0 = success)

EXAMPLES
       Simple echo:
           {"commands": "echo 'Hello, World!'"}

       Arithmetic:
           {"commands": "x=5; y=3; echo $((x + y))"}

       Pipeline:
           {"commands": "echo -e 'apple\nbanana' | grep a"}

       JSON processing:
           {"commands": "echo '{\"n\":1}' | jq '.n'"}

       File operations (virtual):
           {"commands": "echo data > /tmp/f.txt && cat /tmp/f.txt"}

       Run script from VFS:
           {"commands": "source /path/to/script.sh"}

EXIT STATUS
       0      Success
       1-125  Command-specific error
       126    Command not executable
       127    Command not found

SEE ALSO
       bash(1), sh(1)
```

With configuration, appends:
```
CONFIGURATION
       User: agent (whoami)
       Host: sandbox (hostname)
       Limits: 500 commands, 10000 iterations, 100 depth
       Environment: API_KEY
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

`BashTool` is the virtual bash interpreter implementing the `Tool` trait.

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

When configured, outputs automatically include:

- `description()`: Appends custom builtin names to supported tools
- `system_prompt()`: Adds `Home: /home/<username>` if username set
- `help()`: Adds CONFIGURATION section with user, host, limits, env vars

### Streaming Output

`execute_with_status()` emits incremental output via `ToolStatus` events with `phase: "output"`.

```rust
pub struct ToolStatus {
    pub phase: String,           // "validate" | "parse" | "execute" | "output" | "complete"
    pub message: Option<String>,
    pub percent_complete: Option<f32>,
    pub eta_ms: Option<u64>,
    pub output: Option<String>,  // Chunk content (when phase == "output")
    pub stream: Option<String>,  // "stdout" or "stderr"
}
```

Constructors: `ToolStatus::stdout("chunk")`, `ToolStatus::stderr("chunk")`.

At the `Bash` level, `exec_streaming()` provides the same capability:

```rust
let chunks = Arc::new(Mutex::new(Vec::new()));
let chunks_cb = chunks.clone();
let result = bash.exec_streaming(
    "for i in 1 2 3; do echo $i; done",
    Box::new(move |stdout, _stderr| {
        chunks_cb.lock().unwrap().push(stdout.to_string());
    }),
).await?;
// result.stdout == "1\n2\n3\n"  (complete)
// chunks == ["1\n", "2\n", "3\n"]  (incremental)
```

#### Emission granularity

Output is emitted after each:
- Loop iteration (`for`, `while`, `until`, arithmetic `for`)
- Command in a list (`cmd1; cmd2 && cmd3`)
- Command in a sequence (loop body, if branch)
- Top-level script command

A dedup counter prevents double-emission when inner constructs already emitted.
Pipeline intermediate output is not emitted (only the final pipeline stage).

#### Backward compatibility

- `ToolResponse` unchanged — always returns complete buffered output
- `execute()` unaffected — no streaming without `execute_with_status()`
- `ToolStatus.output` and `.stream` are `Option` + `skip_serializing_if`
- `OutputCallback` type: `Box<dyn FnMut(&str, &str) + Send + Sync>`

## Design Rationale

### Why a trait?

Allows multiple tool implementations to share the same interface. Future tools (calculator, file search, etc.) can implement `Tool` for uniform LLM integration.

### Why `commands` not `script`?

Aligns with `bash -c "commands"` semantics. Clearer that it's inline commands, not a script file.

### Why no `timeout_ms`?

Use `timeout` builtin in commands: `timeout 5 long_running_cmd`. Keeps the API simple.

### Why man-page format for `help()`?

- Universal format familiar to developers
- Structured sections (NAME, SYNOPSIS, DESCRIPTION, EXAMPLES)
- Works well with LLM context windows

### Why `system_prompt()` separate from `help()`?

- `help()`: Full docs with examples, for tool discovery and help
- `system_prompt()`: Minimal tokens, for embedding in system prompts

## Verification

```bash
cargo test tool::
cargo run --example show_tool_output
```

## See Also

- [001-architecture.md](001-architecture.md) - Overall architecture
- [005-builtins.md](005-builtins.md) - Builtin implementation
