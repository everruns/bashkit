---
type: Interface Contract
title: Tool Contract
description: Public LLM tool trait behavior, schemas, callbacks, and error semantics.
tags:
  - bashkit
  - tools
  - llm
  - api
---

# Tool Contract

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

## Feature gating

The entire tool layer (`tool` module: `Tool` trait, `BashTool*`,
`ToolExecution`, `ToolService`, schema/OpenAI helpers) is gated behind the
`bash_tool` feature, which is **on by default**. Building with
`--no-default-features` drops the module and its exclusive dependencies
(`tower`, `futures-core`), leaving just the embeddable `Bash` interpreter.

- `scripted_tool` builds on this layer and so enables `bash_tool`.
- Consumers that only drive `Bash` directly (e.g. `bashkit-cli`) set
  `default-features = false` to avoid pulling in the tool dependencies.

## Public API

`Tool` trait, builders, `ToolExecution`, `ToolOutput`, `ToolOutputChunk`,
`ToolError`: see `crates/bashkit/src/tool.rs` / rustdoc.

### Builder rules

- `build()` is non-consuming.
- `build_service()` returns `tower::Service<Value, Response = Value, Error = ToolError>`.
- `build_tool_definition()` emits OpenAI-compatible function JSON.
- `build_input_schema()` / `build_output_schema()` match the built tool metadata.

### Tool metadata rules

- `description()` is token-efficient, one sentence, locale-aware.
- `system_prompt()` is terse plain text that starts with the tool name.
- `help()` is Markdown, not man-page text.
- `execution()` validates JSON args before returning a runnable execution.
- Legacy `execute()` / `execute_with_status()` stay available as convenience helpers.

### Tool execution rules

- `ToolExecution` is single-use.
- `output_stream()` must be called before `execute()`.
- Final truth is `ToolOutput`, not concatenated streamed chunks.
- `images` is empty for bashkit today.
- Callback-owned `ToolArgs` keeps tenant/surface context behind the originating
  execution lease. `tenant_id()` / `surface()` return
  `ExecutionCapabilityError::Revoked` after completion or cancellation; params
  and stdin remain ordinary caller-supplied data.

### Error rules

`ToolError::{UserFacing, Internal}`:

- `UserFacing` is safe for LLMs and localized.
- `Internal` is for logs/diagnostics and stays English.
- `ToolError::is_user_facing()` drives consumer mapping.

## BashTool specifics

- `name()`: `bashkit`; `display_name()`: localized `Bash` / `Баш`

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

`BashTool::execution(...).output_stream()` emits chunks with `kind = "stdout"`
or `kind = "stderr"`; chunk data is JSON string content.

### Metadata

`ToolOutput.metadata.extra` currently includes `{ "exit_code": 0 }`.

## Scripted tool specifics

`ScriptedToolBuilder` and `ScriptingToolSetBuilder` follow the same contract:
locale-aware metadata, OpenAI tool definition helpers, `tower::Service` helper,
`ToolExecution` runtime path. `ScriptedTool` keeps `help` and `discover`
builtins for runtime schema discovery.

## Locale

Localized strings implemented for `en-US` and `uk-UA`. Unsupported locales fall
back to English.

Locale affects: `display_name()`, `description()`, `help()`, `system_prompt()`,
`ToolError::UserFacing`.

Locale does not affect: `name()`, JSON property names and schemas, `version()`.

## Verification

Contract enforced by unit tests: builder helpers, OpenAI tool definition output,
`tower::Service` execution, JSON-arg validation via `execution()`, streamed
chunks, locale-aware metadata.

## See also

- [Scripted Tool Orchestration](scripted-tool-orchestration.md), composing tool definitions into scripted orchestrators
- [Script Analysis](script-analysis.md), pre-execution introspection for host permission gating
- [Bashkit Architecture](../foundations/architecture.md), interpreter this contract exposes
- [HTTP Transport](../security/http-transport.md), host-controlled egress reachable from tool scripts
- [Known Limitations](../operations/limitations.md), behavior the contract deliberately does not promise
