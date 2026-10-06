# Migrating to bashkit-scripted-tool

`ScriptedTool`, `ToolDef`, `ToolRegistry`, and everything built on them moved
out of the `bashkit` crate into **`bashkit-scripted-tool`**. It is published to
crates.io and released in lockstep with `bashkit` (same version number).
`bashkit` keeps the interpreter and the [`BashTool`](llm-tools.md) contract.

The `scripted_tool` feature of `bashkit` is gone (removed after 0.18.2). Behavior is unchanged: same
types, same builders, same logic-only shell, same security defaults.

## Rust

### 1. Cargo.toml

Before:

```toml
bashkit = { version = "0.18.2", features = ["scripted_tool", "python", "typescript", "jq"] }
```

After:

```toml
bashkit = { version = "0.18.2", features = ["python", "typescript", "jq"] }
bashkit-scripted-tool = { version = "0.18.2", features = ["python", "typescript", "jq"] }
```

Keep `bashkit` only if you use it directly (`Bash`, `BashTool`, the `Tool`
trait). `bashkit-scripted-tool` features:

| Feature | Effect |
|---------|--------|
| `python` | `tools.*` in embedded Python for `ToolRegistry::install` (enables `bashkit/python`) |
| `typescript` | `tools.*` in embedded TypeScript for `ToolRegistry::install` (enables `bashkit/typescript`) |
| `jq` | `jq` available inside the logic-only shell (enables `bashkit/jq`) |
| `tracing` | Debug logs for sanitized callback errors |

### 2. Imports

Moved types now come from `bashkit_scripted_tool`. Core types (`Bash`,
`BashTool`, `Tool`, `ExecutionLimits`, ...) stay in `bashkit`.

Before:

```rust,ignore
use bashkit::{ScriptedTool, Tool, ToolArgs, ToolDef, ToolRegistry};
```

After:

```rust,ignore
use bashkit::Tool;
use bashkit_scripted_tool::{ScriptedTool, ToolArgs, ToolDef, ToolRegistry};
```

Moved: `ScriptedTool`, `ScriptedToolBuilder`, `ScriptingToolSet`,
`ScriptingToolSetBuilder`, `DiscoveryMode`, `DiscoverTool`, `ToolDef`,
`ToolArgs`, `ToolImpl`, `ToolCallback`, `AsyncToolCallback`, `SyncToolExec`,
`AsyncToolExec`, `CallbackKind`, `ToolDefExtension`, `ToolDefExtensionBuilder`,
`ToolDefInvocationTrace`, `ScriptedExecutionTrace`, `ScriptedCommandInvocation`,
`ScriptedCommandKind`, `ToolRegistry`, `ToolRegistryBuilder`, `ToolCall`,
`ToolCallDecision`, `ToolCallRequest`, `ToolCallSurface`.

### 3. Registry installation

`BashBuilder::tool_registry` is replaced by `ToolRegistry::install`, which takes
and returns the builder.

Before:

```rust,ignore
let bash = Bash::builder().limits(limits).tool_registry(registry).build();
```

After:

```rust,ignore
let bash = registry.install(Bash::builder().limits(limits)).build();
```

Configure limits on the builder before `install`: Python and TypeScript tool
bridges read the builder's execution profile at install time.

## Python and JavaScript

No change in this release: the bindings re-export the same classes from the
same import paths.

## See also

- [Scripted tool orchestration](scripted-tools.md)
- [LLM tools](llm-tools.md)
