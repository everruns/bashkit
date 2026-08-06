---
type: Architecture
title: Bashkit Architecture
description: Core interpreter architecture, module boundaries, execution flow, and design principles.
tags:
  - bashkit
  - architecture
  - interpreter
---

# Architecture

## Status
Implemented

## Decision

**Official name:** "Bashkit" (not "BashKit"). Crate/package identifiers use lowercase `bashkit`.

Bashkit uses a Cargo workspace with multiple crates:

| Crate | Purpose |
|-------|---------|
| `crates/bashkit/` | Core library (parser, interpreter, VFS, builtins, tool contract) |
| `crates/bashkit-cli/` | CLI binary |
| `crates/bashkit-python/` | Python bindings (PyO3) |
| `crates/bashkit-js/` | JavaScript bindings (NAPI-RS) |
| `crates/bashkit-capi/` | Versioned native C ABI |
| `crates/bashkit-eval/` | LLM eval study (mira framework) |

Core library modules: `parser/`, `interpreter/`, `fs/`, `builtins/`,
`network/`, `git/`, `ssh/`, `scripted_tool/`. See source — structure evolves.

### Public API

Main entry points: `Bash` (library) and `BashTool` (LLM tool contract).
See `crates/bashkit/src/lib.rs` for the full public API surface.

### Per-invocation exec state

`ExecOptions` carries everything scoped to a single `exec_with_options`
call: streaming callback, builtin extensions, `arg0`, `positional`, and
`stdin`. All of it is per-call by construction, not session state.

- Positional parameters exist only inside a call frame, so the host
  boundary pushes a synthetic top-level frame before
  `Interpreter::execute` and truncates the call stack back to its
  baseline afterwards — including on error paths, so `$#` is 0 again on
  the next exec. `$0` defaults to `bash` when no `arg0` is supplied;
  `set --` at top level uses the same synthetic frame and must not
  change `$0`.
- `stdin` seeds `pipeline_stdin`, which `reset_transient_state` clears at
  the start of every exec — so it is installed *after* that reset and
  immediately before execution. A pipe or redirect inside the script
  still wins for the command it applies to.

### Byte-native streams

`StreamData` is the canonical stdin/stdout/stderr transport value. Its byte
buffer is authoritative through `ExecOptions`, interpreter pipeline state,
redirections, `BuiltinContext`, `ExecResult`, and streaming callbacks. Output
limits truncate by bytes, including in live callbacks. Byte-oriented builtins
and bindings use `as_bytes()`/`into_bytes()`; text-oriented consumers cross an
explicit UTF-8 boundary with `text()` or `text_lossy()`.

Shell words and variables remain text. Command substitution therefore removes
NUL bytes, as Bash does because variables cannot contain NUL, then decodes the
remaining bytes for expansion and strips trailing newlines. JSON tool results
are also text-only; native Rust, C, Python, Node, and browser results expose raw
stdout/stderr bytes alongside their display strings.

Both are installed late (just before `execute`) so the size, hook, and
parse checks that can return early cannot leave state behind.

### Process-local execution suspension

Event-backed host calls reuse the async-first execution model: an
`ExecutionHandle` owns and polls the live execution future, yields an owned
request to the host, then resolves a one-shot response when the host resumes
it. The handle exclusively owns the interpreter and its timeout remains
active. On completion, `into_bash` returns the reusable session; dropping a
suspended handle drops the session. This is process-local scheduling, not a
serializable continuation; snapshots still capture state only between
executions. See [Builtin Commands](builtins.md) and
[Snapshot History](snapshot-history.md).

### Shared execution budget

Each `exec_with_options` creates exactly one `ExecutionBudget` before hooks or
parsing. Its `Arc`-backed counters are cloned—not recreated—by nested parsers,
command/process substitutions, pipelines, builtin execution plans, embedded
Python/TypeScript/SQLite, traversal/search, archive/compression work, and host
callbacks. It meters three distinct resources: monotonic work units, monotonic
aggregate consumer input bytes, and RAII live/intermediate byte leases.

Exhaustion, the request deadline, or cancellation poisons the shared budget;
later descendants see the first failure and cannot resume with fresh counters.
This aggregate layer complements rather than replaces parser fuel, command and
loop counters, output caps, VFS quotas, and runtime-specific ceilings. The
budget is request-scoped: the next host exec receives a fresh budget, while
session limits continue to protect repeated host calls.

Runtime admission is deliberately conservative where a VM does not expose a
portable completed-instruction count: Python reserves work from its configured
memory allowance and TypeScript from its allocation allowance before entry.
Python additionally meters Monty allocation/statement checkpoints. The
reservation is not refunded, because refunding would recreate the reset path
this layer exists to remove.

Heap buffers grown from untrusted sizes use the budget-aware owning builders in
`limits.rs`. `BudgetedVec` (including byte buffers) and `BudgetedString` acquire
or grow the live-byte lease before reserving capacity, roll the charge back when
the allocator or producer fails, and release it with the buffer. Archive and
compression paths use these builders because their expansion ratios and nested
buffers make post-hoc leasing unsafe. Atomic compare/exchange admission keeps
concurrent descendants from wrapping or temporarily exceeding the shared cap.

### Design Principles

1. **Async-first**: All filesystem and execution is async (tokio)
2. **Virtual**: No real filesystem access by default
3. **Multi-tenant safe**: Isolated state per Bash instance
4. **Trait-based**: FileSystem and Builtin traits for extensibility

## Alternatives Considered

- Single crate: rejected — CLI bloats library; Python/JS packages need separate crates.
- Sync filesystem: rejected — network ops need async; tokio already a dep.

## See also

- [Parser](parser.md) — script text to AST, ahead of the interpreter
- [Virtual Filesystem](vfs.md) — filesystem abstraction the interpreter executes against
- [Builtin Commands](builtins.md) — command layer the interpreter dispatches into
- [Parallel Execution](parallel-execution.md) — threading model and shared-ownership rules
- [Threat Model](../security/threat-model.md) — trust boundaries these module boundaries enforce
- [Known Limitations](../operations/limitations.md) — what this architecture intentionally does not do
- [Public Capability Parity](../status/capability-parity.md) — generated wrapper support and explicit exclusions
