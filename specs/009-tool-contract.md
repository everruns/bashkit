# 009: Tool Contract

## Status
Implemented

## Summary

`bashkit` follows the Everruns toolkit library contract from
`everruns/specs/toolkit-library-contract.md`.

Public shape:

```text
ToolBuilder (config) -> Tool (metadata) -> ToolExecution (single-use runtime)
```

`BashToolBuilder` is the primary builder. `ScriptedToolBuilder` and
`ScriptingToolSetBuilder` mirror the same contract for orchestration tools.

## Public API

### Builders

All tool builders expose:

```rust
pub fn new() -> Self;
pub fn locale(self, locale: &str) -> Self;
pub fn build(&self) -> ToolImpl;
pub fn build_service(&self) -> ToolService;
pub fn build_tool_definition(&self) -> serde_json::Value;
pub fn build_input_schema(&self) -> serde_json::Value;
pub fn build_output_schema(&self) -> serde_json::Value;
```

Notes:

- `build()` is non-consuming.
- `build_service()` returns `tower::Service<Value, Response = Value, Error = ToolError>`.
- `build_tool_definition()` emits OpenAI-compatible function JSON.
- `build_input_schema()` and `build_output_schema()` match the built tool metadata.

### Tool metadata

All tool implementations conform to the shared `Tool` trait:

```rust
#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn display_name(&self) -> &str;
    fn short_description(&self) -> &str;
    fn description(&self) -> &str;
    fn help(&self) -> String;
    fn system_prompt(&self) -> String;
    fn locale(&self) -> &str;
    fn input_schema(&self) -> serde_json::Value;
    fn output_schema(&self) -> serde_json::Value;
    fn version(&self) -> &str;
    fn execution(&self, args: serde_json::Value) -> Result<ToolExecution, ToolError>;
    async fn execute(&self, req: ToolRequest) -> ToolResponse;
    async fn execute_with_status(
        &self,
        req: ToolRequest,
        status_callback: Box<dyn FnMut(ToolStatus) + Send>,
    ) -> ToolResponse;
}
```

Contract notes:

- `description()` is token-efficient, one sentence, locale-aware.
- `system_prompt()` is terse plain text that starts with the tool name.
- `help()` is Markdown, not man-page text.
- `execution()` validates JSON args before returning a runnable execution.
- Legacy `execute()` / `execute_with_status()` stay available as convenience helpers.

### Tool execution

```rust
pub struct ToolExecution {
    pub fn output_stream(&self) -> Option<ToolOutputStream>;
    pub async fn execute(self) -> Result<ToolOutput, ToolError>;
}

pub struct ToolOutput {
    pub result: serde_json::Value,
    pub images: Vec<ToolImage>,
    pub metadata: ToolOutputMetadata,
}

pub struct ToolOutputMetadata {
    pub duration: std::time::Duration,
    pub extra: serde_json::Value,
}

pub struct ToolOutputChunk {
    pub data: serde_json::Value,
    pub kind: String,
}
```

Rules:

- `ToolExecution` is single-use.
- `output_stream()` must be called before `execute()`.
- Final truth is `ToolOutput`, not concatenated streamed chunks.
- `images` is empty for bashkit today.

### Errors

```rust
pub enum ToolError {
    UserFacing(String),
    Internal(String),
}
```

Rules:

- `UserFacing` is safe for LLMs and localized.
- `Internal` is for logs/diagnostics and stays English.
- `ToolError::is_user_facing()` drives consumer mapping.

## BashTool specifics

### Names

- `name()`: `bashkit`
- `display_name()`: localized `Bash` / `Баш`

### Input schema

```json
{
  "type": "object",
  "properties": {
    "commands": { "type": "string" },
    "timeout_ms": { "type": ["integer", "null"] }
  },
  "required": ["commands"]
}
```

### Output schema

`ToolOutput::result` matches:

```json
{
  "stdout": "string",
  "stderr": "string",
  "exit_code": 0,
  "error": "string|null"
}
```

### Streaming

`BashTool::execution(...).output_stream()` emits:

- `kind = "stdout"` for stdout chunks
- `kind = "stderr"` for stderr chunks

Chunk data is JSON string content.

### Metadata

`ToolOutput.metadata.extra` currently includes:

```json
{ "exit_code": 0 }
```

## Scripted tool specifics

`ScriptedToolBuilder` and `ScriptingToolSetBuilder` follow the same contract:

- locale-aware metadata
- OpenAI tool definition helpers
- `tower::Service` helper
- `ToolExecution` runtime path

`ScriptedTool` keeps `help` and `discover` builtins for runtime schema discovery.

## Locale

Current localized strings are implemented for:

- `en-US`
- `uk-UA`

Unsupported locales fall back to English.

Locale affects:

- `display_name()`
- `description()`
- `help()`
- `system_prompt()`
- `ToolError::UserFacing`

Locale does not affect:

- `name()`
- JSON property names and schemas
- `version()`

## Verification

The contract is enforced by unit tests covering:

- builder helper methods
- OpenAI tool definition output
- `tower::Service` execution
- JSON-arg validation via `execution()`
- streamed output chunks
- locale-aware metadata
