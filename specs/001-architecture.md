# 001: Architecture

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
| `crates/bashkit-eval/` | LLM evaluation harness |

The core library modules: `parser/`, `interpreter/`, `fs/`, `builtins/`,
`network/`, `git/`, `ssh/`, `scripted_tool/`. See the source for current
structure — it evolves as features are added.

### Public API

Main entry point is `Bash` (library) and `BashTool` (LLM tool contract).
See `crates/bashkit/src/lib.rs` for the full public API surface.

### Design Principles

1. **Async-first**: All filesystem and execution is async (tokio)
2. **Virtual**: No real filesystem access by default
3. **Multi-tenant safe**: Isolated state per Bash instance
4. **Trait-based**: FileSystem and Builtin traits for extensibility

## Alternatives Considered

### Single crate vs workspace
Rejected single crate because:
- CLI binary would bloat the library
- Python/JS packages need separate crates
- Cleaner separation of concerns

### Sync vs async filesystem
Rejected sync because:
- Bashkit is fully async
- Network operations need async
- tokio is already a dependency

## Verification

```bash
cargo build
cargo test
```
