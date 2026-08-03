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

Both are installed late (just before `execute`) so the size, hook, and
parse checks that can return early cannot leave state behind.

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
