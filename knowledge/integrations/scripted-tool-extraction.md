---
type: Subsystem Design
title: Scripted Tool Extraction
description: Plan for moving the scripted-tool layer out of the bashkit core crate into a separate bashkit-tools crate.
tags:
  - bashkit
  - tools
  - orchestration
  - refactoring
---

# Scripted Tool Extraction

## Status

Proposed. Nothing moved yet; this document is the plan and the design of the
core-side seams the move needs.

## Problem

The scripted-tool layer ([Scripted Tool Orchestration](scripted-tool-orchestration.md))
lives inside `crates/bashkit`, ~6.9k lines across four modules:

| Module | Lines | Role |
|---|---|---|
| `src/tool_def.rs` | 1552 | `ToolDef`, `ToolArgs`, `ToolImpl`, exec types, `parse_flags`, `usage_from_schema` |
| `src/tool_registry.rs` | 904 | cross-runtime registry, policy, validation, Python/TypeScript prelude codegen |
| `src/scripted_tool/extension.rs` | 674 | `ToolDefExtension`, per-tool builtins, `help` / `discover`, invocation trace |
| `src/scripted_tool/{mod,execute,toolset}.rs` | 3734 | `ScriptedTool`, `ScriptingToolSet`, builders, `Tool` impl |

None of it is shell, parser, VFS, or interpreter. It is an LLM-integration
layer built *on top of* `Bash`, in the same position as an embedder. It is
also not universally wanted: `bashkit-wasm` and `bashkit-cli` carry
`scripted_tool` in their feature graph while using none of it.

Costs of leaving it in core:

- The core crate's public API and compile matrix carry an integration concern.
- `scripted_tool` is a cross-cutting cfg: it appears in `limits.rs`,
  `execution_capability.rs`, `builtins/mod.rs`, `builtins/python.rs`,
  `builtins/typescript.rs`, and `lib.rs`.
- Core reaches *into* the tool layer: `builtins/python.rs` and
  `builtins/typescript.rs` call `crate::tool_registry::scope_runtime_call`.
  That is a layering inversion, and it is the one real blocker to extraction.

## Adoption evidence

Surveyed 2026-08-21. Every traceable Rust consumer of `bashkit` compiles
**without** the `scripted_tool` feature.

crates.io reverse dependencies of `bashkit` (5 total, feature sets as
published):

| Crate | Features |
|---|---|
| `whipplescript-kernel` | `jq` |
| `bashkit-cli` | `http_client`, `git`, `jq` |
| `loa-core` | default (`bash_tool`) |
| `crabot` | `http_client`, `realfs`, `python` |
| `everruns-integrations-bashkit` | `jq`, `http_client`, `bot-auth` |

`default = ["bash_tool"]`, so the default-features consumer does not pick it
up either.

GitHub's dependency graph lists 19 repositories and 6 packages. Tracing the
third-party ones through their lockfiles:

- `bionic-gpt/bionic-gpt` (2.4k stars) declares
  `bashkit = { version = "0.14.5", features = ["python"] }` in its
  `tool-runtime` crate. A tool-runtime built on bashkit that does not use the
  scripted-tool layer.
- `GaugeWright/gaugedesk` and `GaugeWright/whipplescript` reach bashkit only
  transitively via `whipplescript-kernel`.
- `wuwei-labs/antegen` reaches it only transitively via `loa-core`.
- `alexkehayias/hq` has vendored its way off the dependency
  ("previously transitive via bashkit").
- `nobodywho-ooo/nobodywho` and `tuist/condukt` show no current bashkit
  dependency in any manifest.

The Python and TypeScript packages are the one place the layer is
unconditionally present: `ScriptedTool` is a top-level export of the `bashkit`
PyPI package (`bashkit/__init__.py` `__all__`) and of `@everruns/bashkit`, with
no feature flag for consumers to decline. Public search surfaces only
first-party documentation using it, so measured third-party adoption there is
unknown rather than zero.

Caveat on method: this session's proxy blocks global GitHub code search, so the
survey is dependency-graph and manifest based, not a code-level search. It
cannot see private consumers or public repos outside the dependency graph.

Bearing on the plan: extraction removes a feature nobody in the Rust dependency
graph enables, and costs nothing to the language bindings, which gain the crate
as an ordinary dependency and keep `ScriptedTool` exported unchanged.

## Target shape

New workspace member `crates/bashkit-tools`, depending on `bashkit`:

```
bashkit-tools  (ToolDef, ToolImpl, ToolRegistry, ToolDefExtension, ScriptedTool, ScriptingToolSet)
    |
    v
bashkit        (Bash, Builtin, Extension, Context, Tool trait, BashTool)
```

The `Tool` trait and `BashTool` **stay in core**: `BashTool` implements
`Tool`, and [the tool contract](tool-contract.md) is core's public contract.
`bashkit-tools` implements `Tool` for `ScriptedTool` from the outside, which
is exactly what a third-party embedder would do, so the split doubles as a
proof that the public trait surface is sufficient.

After the move the `scripted_tool` cargo feature on `bashkit` is deleted, not
renamed. Every cfg listed above disappears or degrades to the existing
`python` / `typescript` cfgs.

## Core-side seams

Four things in core are currently `pub(crate)` or inverted and must be fixed
before anything moves. Each is small and independently testable.

### 1. Logic-only shell profile

`BashBuilder::logic_only()` is `pub(crate)` and `interpreter::ShellProfile` is
a private enum. `ScriptedTool` needs it to build its code-mode shell.

Make `BashBuilder::logic_only()` public. Keep `ShellProfile` private; the
builder method is the whole API. The method only *removes* command surface,
so publishing it widens no sandbox boundary.

### 2. Execution deadline accessor

`tool_registry` reads `builtins::ExecutionDeadline` (a `pub(crate)` struct)
out of `Context` to bound callback futures. Rather than publishing the struct,
add a narrow accessor on the public `BuiltinContext`:

```rust
impl Context<'_> {
    /// Remaining wall-clock budget for this execution, if one is set.
    pub fn remaining_deadline(&self) -> Option<Duration>;
}
```

That is the only thing the registry uses it for, and it keeps
`ExecutionDeadline`'s representation private.

### 3. Prelude-carrying external handlers

`Python::with_external_handler_and_prelude` and
`TypeScriptExtension::with_external_handler_and_prelude` are `pub(crate)`.
The prelude is what makes `tools.orders.list({...})` resolve to a hidden
external function, so the registry cannot wire Python/TypeScript without it.
Make both public (the `with_external_handler` variants already are).

### 4. Runtime call scope inversion — the blocker

Today:

```rust
// crates/bashkit/src/builtins/python.rs
#[cfg(feature = "scripted_tool")]
let future = crate::tool_registry::scope_runtime_call(
    crate::tool_registry::ToolCallScope::from_context(&ctx), future);
```

Core names the tool layer. The task-local exists only because
`PythonExternalFnHandler` / `TypeScriptExternalFnHandler` receive no execution
context, so the handler cannot see the deadline, budget, tenant, or trace of
the call that suspended into it.

**Decision: move the task-local into core as an opaque, generic scope.** Core
owns a `RuntimeCallScope` holding an `Arc<dyn Any + Send + Sync>` payload plus
the deadline/budget it already knows about; the Python and TypeScript builtins
install it unconditionally (gated only by their own features), and
`bashkit-tools` downcasts it inside its handler.

```rust
// core, public
pub struct RuntimeCallScope { /* deadline, budget, opaque payload */ }
impl RuntimeCallScope {
    pub fn from_context(ctx: &BuiltinContext<'_>, payload: Arc<dyn Any + Send + Sync>) -> Self;
    pub fn current() -> Option<Self>;
    pub fn payload<T: 'static>(&self) -> Option<Arc<T>>;
}
```

This is mechanical, preserves the Monty/ZapCode suspension behavior verbatim,
and removes every `scripted_tool` cfg from `builtins/`.

The root-cause fix — passing a `RuntimeCallContext` argument to the handler
signature and deleting the task-local — is deliberately **not** in this plan.
It changes the external-function contract for every embedder and touches
suspension/resume in both runtimes; it belongs in its own change once the
crate split has settled. Recorded here so it is not lost.

## Migration plan

Three PRs. The first two are required; the third is cleanup.

### PR 1 — core seams, no move

Implements seams 1–4 above, everything still inside `bashkit`, feature flag
unchanged. Deliverable: `crates/bashkit/src/builtins/` contains zero
`scripted_tool` cfgs, and `tool_registry` compiles against only public core
API.

Verification: `just check`; existing `tool_registry_tests.rs` and
`request_lifecycle_contract_tests.rs` pass unmodified. Cross-runtime tenant
and trace isolation (TM-ISO-026) is the behavior most at risk — those tests
are the gate.

### PR 2 — create the crate, move the layer

Mechanical `git mv` plus import rewrite. No behavior change, no compatibility
shim (internal code, per `AGENTS.md`); every call site is updated in the same
PR.

Moves:

| From | To |
|---|---|
| `crates/bashkit/src/tool_def.rs` | `crates/bashkit-tools/src/tool_def.rs` |
| `crates/bashkit/src/tool_registry.rs` | `crates/bashkit-tools/src/registry.rs` |
| `crates/bashkit/src/scripted_tool/` | `crates/bashkit-tools/src/scripted_tool/` |
| `crates/bashkit/examples/scripted_tool.rs` | `crates/bashkit-tools/examples/scripted_tool.rs` |
| `crates/bashkit/tests/integration/tool_registry_tests.rs` | `crates/bashkit-tools/tests/` |
| scripted-tool sections of `crates/bashkit/docs/` | `crates/bashkit-tools/docs/` |

Consumer updates:

| Crate | Change |
|---|---|
| `bashkit-python` | add `bashkit-tools` dep; 25 references in `src/lib.rs` |
| `bashkit-js` | add `bashkit-tools` dep; 17 references in `src/lib.rs` |
| `bashkit-eval` | add `bashkit-tools` dep; `scripting_agent.rs`, `scripting_dataset.rs`, providers |
| `bashkit-cli` | delete the unused `scripted_tool` feature passthrough |
| `bashkit-wasm` | drop `scripted_tool` from its feature list (unused) |
| `bashkit` | delete the `scripted_tool` feature and all its cfgs |

`request_lifecycle_contract_tests.rs` is split: the parts that assert core
capability revocation stay in `bashkit`, the parts that drive a registry move
out.

### PR 3 — cleanup

Re-narrow anything PR 1 published more widely than PR 2 proved necessary, and
schedule the handler-signature fix from seam 4.

## Risks

**TM-INF-022 guard loses coverage.** `no_debug_fmt_in_builtin_source` in
`builtins/mod.rs` walks `CARGO_MANIFEST_DIR/src/builtins` only. The per-tool
adapter builtins in `scripted_tool/extension.rs` leave that tree, so the
static scan silently stops covering them. PR 2 must add an equivalent scan in
`bashkit-tools` over its own sources, and the dynamic `assert_no_leak`
counterpart must keep running there. See
[the threat model](../security/threat-model.md).

**Public surface widening.** Seams 1–3 publish four APIs that were internal.
All are additive and restrictive-or-inert; none grants a capability a script
did not already have. They ship in PR 1 so they are reviewed as an API change
rather than buried in a 7k-line move.

**Publish ordering.** `bashkit-tools` is a new crates.io artifact between
`bashkit` and the language bindings. `publish.yml` must wait for the core
registry version before publishing it, the same way `bashkit-cli` already
does. Update [the release process](../operations/release-process.md) and the
publish-readiness dry run in the same PR.

**Doc links.** `docs/scripted-tools.md`, `docs/start-rust.md`,
`docs/custom_builtins_js.md`, `site/src/pages/docs/_meta.ts`, and
`site/src/content/` reference the moved paths. `just check-doc-links` is the
gate; cross-tree links must stay source-relative.

## Non-goals

- No behavior change to `ScriptedTool`, `ScriptingToolSet`, or `ToolRegistry`.
- No change to the Python or TypeScript external-function contract in PR 1–2.
- `Tool`, `BashTool`, and the `bash_tool` feature stay in core.

## See also

- [Scripted Tool Orchestration](scripted-tool-orchestration.md) — what is being moved.
- [Tool Contract](tool-contract.md) — the trait `ScriptedTool` implements from outside.
- [Architecture](../foundations/architecture.md) — core module boundaries.
- [Release Process](../operations/release-process.md) — publish ordering for the new crate.
