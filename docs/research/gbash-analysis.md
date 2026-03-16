# gbash Analysis & Comparison

Analysis of [ewhauser/gbash](https://github.com/ewhauser/gbash) — a Go-based deterministic, sandbox-only bash runtime for AI agents.

## What is gbash?

Go-based bash-like runtime. Delegates parsing to `mvdan/sh` (mature Go shell parser). Project owns VFS, registry-backed commands, policy enforcement, structured tracing. ~90+ commands. Apache 2.0. Alpha status (created 2026-03-10).

## Architecture Comparison

| Aspect | bashkit (Rust) | gbash (Go) |
|--------|---------------|------------|
| Parser | Custom recursive descent | Delegates to `mvdan/sh` |
| Execution | Custom interpreter | `mvdan/sh` `interp.Runner` |
| VFS | InMemory, Overlay, Mountable | Memory, Overlay, Host, Snapshot, Searchable, Mountable |
| Commands | 148+ builtins, all in-tree | 90+ core + contrib modules (awk, jq, sqlite3, yq) |
| Language | Rust, async/tokio | Go, sync with goroutines |
| Bindings | Python (PyO3), JS (NAPI) | WASM (browser + Node) |
| Server mode | None | JSON-RPC over Unix socket or TCP |
| Security | Resource limits, threat model | Policy trait, execution budgets, trace redaction |
| Dependencies | Many crates | 3 direct (mvdan/sh, x/crypto, x/term) |

## Key Ideas Evaluated

### Adopted (issues created)

1. **Output capture limits** (#648) — `max_stdout_bytes` / `max_stderr_bytes` to prevent OOM from unbounded output. gbash has `MaxStdoutBytes` in policy with `StdoutTruncated` flag in results.

2. **FinalEnv in ExecResult** (#649) — Return environment state after execution so agent frameworks can track mutations across tool calls.

3. **Static budget validation** (#650) — Analyze parsed AST before execution to reject obviously expensive scripts (literal loop bounds, brace expansion size). gbash does this plus loop budget instrumentation via AST callbacks.

### Under Discussion

4. **Structured traces** — gbash has `TraceConfig{Mode: TraceRedacted}` with events returned as `result.Events`. Event types: `command.start`, `command.exit`, `file.access`, `file.mutation`, `policy.denied`. Also has `OnEvent` callback for real-time streaming. This is a library feature (like `result.get_events()`), not a logging framework.

5. **Policy trait** — Pluggable `Policy` interface with `AllowCommand()`, `AllowBuiltin()`, `AllowPath(action, target)`, `Limits()`, `SymlinkMode()`. Gives embedders fine-grained per-path and per-command access control.

6. **CommandSpec framework** — Declarative CLI option system shared across all commands. Our 133+ builtins each parse args ad-hoc. A shared framework would reduce boilerplate.

7. **Searchable FS** — `SearchCapable` trait on `FileSystem` with pluggable `SearchProvider`. FS mutations auto-update index. Commands like `grep`/`rg` check if FS implements `SearchCapable` and use index. Should live on the VFS interface side — implementers of custom VFS backends provide fast search primitives.

### Declined

| Idea | Reason |
|------|--------|
| Persistent sessions | No clear use case yet |
| JSON-RPC server mode | No need currently |
| Contrib module split | Not worth the complexity now |
| Lazy command registration | Builtins are unit structs, near-zero allocation cost |
| Snapshot FS | Fun but no use case |
| WASM target | Already have full support (`bashkit-js` + `examples/browser/`) |
| Lazy file seeding | We don't have it, but no pressing need identified |

## Notable gbash Design Patterns

### AST Normalization
Modifies parsed AST to fix behavioral differences between `mvdan/sh` and real Bash (e.g., wrapping pipeline RHS in subshells for `lastpipe=off` semantics).

### Capability-Based Command Context
Commands never get raw filesystem access. They get a `CommandFS` wrapper that enforces policy checks and records trace events on every operation. Network access only through `Invocation.Fetch`.

### OAuth Network Extension
Auth injection happens at the transport layer outside the sandbox. The sandbox never sees bearer tokens. Relevant pattern for our HTTP support.

### Reusable Filesystem Factory
For repeated sessions sharing the same base data, materializes the base filesystem once and gives each session a fresh overlay. Avoids redundant work.

### Trace Redaction Modes
Three modes: Off (default), Redacted (scrubs secret-bearing argv), Raw (unsafe). `TraceRedacted` is recommended for agent workloads where traces might end up in shared sinks.

## What bashkit Does Better

- **Parser ownership** — full control, can add non-standard features
- **Command breadth** — 148+ vs 90+, structured data builtins (json, csv, yaml, semver)
- **Performance** — Rust async + zero-copy, 6.9x faster than just-bash
- **Memory safety** — Rust ownership model
- **Python bindings** — PyO3 with LangChain/PydanticAI integration
- **Threat model depth** — 60+ identified threats
- **Git support** — virtual git on VFS
- **Eval harness** — 58-task LLM evaluation suite
- **Spec-driven design** — 14 living specification documents
