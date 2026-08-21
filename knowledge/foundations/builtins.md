---
type: Subsystem Design
title: Builtin Commands
description: Builtin command trait, execution planning, registration, and implementation conventions.
tags:
  - bashkit
  - builtins
---

# Builtin Commands

## Status
Implemented

## Decision

Bashkit provides built-in commands for script execution in a virtual environment.
All builtins operate on the virtual filesystem. For the complete list, see
the generated [`builtins.json`](../status/builtins.json); for known gaps, [Known Limitations](../operations/limitations.md).

### Standard Flags

All external-style builtins support `--help` and `--version` flags via the
`check_help_version()` helper in `builtins/mod.rs` (long flags only, short
flags `-h`/`-V` have different meanings in many tools). Tools where `-h`/`-V`
genuinely mean help/version handle them directly in `execute()`.

### Command Dispatch Order

functions → special commands → builtins → path execution → $PATH search →
`CommandResolver` → "command not found"

Scripts containing `/` are resolved against VFS. Commands without `/` are
searched in `$PATH` directories. Shebang lines are stripped; content executed
as bash. Exit 127: not found; Exit 126: not executable or is a directory.

### Builtin Trait

`Builtin` trait (`execute(ctx)` + optional `execution_plan(ctx)`, default
`Ok(None)`) and `Context` (args, env, variables, cwd, fs, stdin,
feature-gated borrowed http/git clients, `pub(crate) shell: Option<ShellRef>`, None
for custom builtins, and public lease-backed `execution_extension::<T>()`): see
`crates/bashkit/src/builtins/mod.rs` / rustdoc.

### Clap-Backed Custom Builtins

Custom Rust builtins can implement `ClapBuiltin` instead of `Builtin` when
their arguments are better represented as a `#[derive(clap::Parser)]` struct
(see `builtins/mod.rs` / rustdoc for the trait and an example). `clap` is an
unconditional dependency of `bashkit` (also used by ported coreutils argument
surfaces, see [Coreutils Argument Port](../runtimes/coreutils-args-port.md)), so this trait is always
available. Bashkit parses `Context::args` through clap, passes parsed args
plus a mutable `BashkitContext` to the handler, maps `--help`/`--version` to
successful stdout results, and maps clap parse failures to stderr with clap's
exit code. Parse diagnostics are capped to 1 KB to preserve TM-INF-022 stderr
constraints.

### Extension Trait

Extensions bundle a related set of builtins so embedders can add one capability
to `BashBuilder` or `BashToolBuilder` instead of registering each command
manually: `Extension::builtins() -> Vec<(String, Box<dyn Builtin>)>`
(`builtins/mod.rs`).

Rules:

- `BashBuilder::extension(ext)` / `BashToolBuilder::extension(ext)` expand each
  returned builtin into the builder's custom builtin map/list
- For `BashBuilder`, later registrations with the same command name override
  earlier registrations, matching `BashBuilder::builtin`
- Extensions must construct fresh builtin values or use shared ownership
  internally; builders may call `builtins()` when configuring reusable tools

Current extension: `TypeScriptExtension` registers `ts`/`typescript` and, when
enabled by `TypeScriptConfig`, `node`/`deno`/`bun`.

### BuiltinRegistry, Host-Owned Mutable Builtins

`BashBuilder::builtin(name, ...)` and `Extension::builtins()` are both
*build-time* registration: the set of builtins is frozen once the `Bash`
instance is built. For embedders that need to register or remove builtins
*after* construction (FFI bindings, REPLs, plugin systems),
`BuiltinRegistry` provides a host-owned mutable registry consulted at
command-dispatch time. API (`insert`/`insert_trusted`/`remove`/`lookup`/`names`/`is_empty`):
see `builtins/mod.rs` / rustdoc.

Wired in via `BashBuilder::builtin_registry(registry)`. The handle is
`Clone`; clones share the same underlying storage, so the embedder keeps a
clone for runtime mutation while the builder takes another.

Command-resolution order (see `Interpreter::dispatch_command`):

1. Shell functions (defined in scripts)
2. POSIX special builtins (`exec`, `set`, `:`, `eval`, …)
3. **Host registry** (`BuiltinRegistry::lookup`)
4. Baked-in + builder-registered builtins
5. Script execution by path / `$PATH` search
6. `CommandResolver::resolve` (last chance, see below)

So registry entries can override baked-in commands (e.g. wrap `cat` with
tracing) but shell functions still win, matching standard bash
precedence. `command -v` / `command -V` / `command name args…` consult
the registry too.

Implementation notes:

- Storage is an `Arc<RwLock<HashMap<...>>>` of builtin plus access mode (std
  only, no extra deps). Lookup clones the entry out of the lock before execution.
- `insert` is execution-scoped. `insert_trusted` is the explicit escape hatch
  for trusted host integrations that intentionally retain the raw VFS across calls.
- `Interpreter::builtins` was migrated from `HashMap<String, Box<dyn Builtin>>`
  to `HashMap<String, Arc<dyn Builtin>>` so registered and host-registry
  paths share one execution helper (`execute_builtin_arc`).
- The registry is host-owned: not part of interpreter state, so
  `reset_transient_state` leaves it untouched and snapshots do not
  serialize it. Restoring from a snapshot requires re-attaching the
  registry handle.

### CommandResolver, Last-Chance Name Resolution

`BuiltinRegistry` answers "which names are registered" from a map fixed before
the name is known. `CommandResolver` is asked *about a specific name*, after
every other route has missed and immediately before the 127 path, so an
embedder bridging an open-ended command space (host executables, a remote tool
catalog) does not have to enumerate it up front.

```rust
pub trait CommandResolver: Send + Sync {
    fn resolve(&self, name: &str) -> Option<Arc<dyn Builtin>>;
}
```

Decision: resolvers return a `Builtin` rather than executing directly. The
resolved builtin runs through `execute_builtin_arc`, the same path as every
other builtin, so `before_tool` fires with the resolved name and can veto it,
`catch_unwind` contains panics from both resolution and execution, and
stdin/redirects behave identically. Resolver panic details are discarded and
the command returns a sanitized non-zero shell result. A bespoke execution path
would have to re-earn all of that.

Consequences to keep in mind:

- Resolver-provided names are **not enumerable**: they do not appear in
  `Bash::builtin_names()` or in `command not found` suggestions. Security
  implications are in `../integrations/script-analysis.md`.
- Being last, a resolver can never shadow a function, builtin, or `$PATH`
  script. Use `BashBuilder::builtin` to override one.
- `resolve` is called for every unresolved command, so it must be cheap;
  cache rather than probing the filesystem per call.
- Resolver-provided builtins receive the same execution-scoped VFS lease as
  builder and ordinary registry builtins.
- The `$PATH` search consumes the pipeline stdin, so the interpreter clones it
  for the resolver **only when a resolver is installed**: the common path does
  not pay for the clone.

### Execution Extensions

`Bash::exec_with_extensions()` and `Bash::exec_streaming_with_extensions()`
accept a typed, per-call extension bag. Builtins receive an
`ExecutionCapability<T>` via `ctx.execution_extension::<T>()` and access it
through `try_with`; retained handles fail with `ExecutionCapabilityError::Revoked`.

Use this for request-scoped data that is not shell state: tracing/request IDs,
auth or tenant context, host-language runtime sessions (Python/JS callback
bridges), metrics/audit sinks for one execution.

Rules:

- One lease covers extension values, scoped VFS handles, host-call brokers, and
  tool-callback context.
- Completion, timeout, or dropped execution futures revoke the lease before the
  interpreter restores its prior request state.
- Cleanup is idempotent and bounded; `ExecResult::capability_cleanup` and retained
  handles expose only a failure count, never poisoned-lock or host diagnostics.
- Borrowed HTTP/Git/SSH clients were already lifetime-scoped and remain borrowed;
  no parallel wrapper is added for facilities safe Rust cannot retain.
- Long-lived registrations may retain capability handles, but late access fails.

### Shell State Access (ShellRef)

Internal builtins that need interpreter state receive it via `Context.shell`:

**Design rationale:**
- **Direct mutation** for aliases/traps, simple HashMaps with no invariants
- **Side effects** for arrays (budget checks), positional params (call stack),
  history (VFS persistence), state with invariants the interpreter must enforce
- **Read-only methods** for introspection (functions, builtins, keywords,
  call stack, history, jobs), builtins shouldn't mutate these
- `pub(crate)` keeps ShellRef out of the public API; custom builtins use
  public `execution_extension()` instead of direct shell access
- No dynamic dispatch, concrete struct, not trait

**Builtins using ShellRef:**
- `type`, `which`, read-only: check builtin/function/keyword names
- `alias`, `unalias`, direct mutation of `shell.aliases`
- `trap`, direct mutation of `shell.traps`
- `caller`, read call stack depth/frame names
- `history`, read history entries, clear via `ClearHistory` side effect
- `wait`, read job table, set exit code via `SetLastExitCode` side effect
- `mapfile`/`readarray`, set arrays via `SetIndexedArray` side effect

**Builtins still in interpreter dispatch chain** (fundamentally need interpreter):
- `exec`, redirect management, VFS I/O
- `local`, call frame locals mutation
- `source`/`.`, `eval`, parse and execute in current context
- `bash`/`sh`, script execution
- `command`, dispatch to builtins/functions
- `declare`/`typeset`, arrays, assoc arrays, variable attributes
- `unset`, functions, arrays, namerefs, call stack locals
- `let`, arithmetic evaluation with assignment
- `getopts`, complex variable + call stack interaction

`time` is deliberately absent from the builtin registry. Bash grammar makes it
a reserved-word wrapper around a complete pipeline, so the interpreter measures
the AST directly. This preserves groups, functions, pipeline status, redirects,
errexit, cancellation, and the shared request budget.

### Execution Plans (Sub-Command Delegation)

Builtins cannot access the interpreter directly. When a builtin needs to run
other commands (e.g. `timeout`, `xargs`, `find -exec`), it returns a declarative
`ExecutionPlan` from `execution_plan()`. The interpreter checks this method
before `execute()`, when it returns `Some(plan)`, the interpreter fulfills the
plan instead of using the `execute()` result.

Variants: `Timeout { duration, preserve_status, command }`,
`Batch { commands }` (`builtins/mod.rs`).

Each `SubCommand` carries optional command-scoped `assignments`
(`VAR=value cmd ...`), which the interpreter applies as the inner command's
environment. `xargs --process-slot-var=VAR` uses this to expose a
per-invocation parallel-slot index.

**Current users:** `timeout` → Timeout, `xargs` → Batch, `find -exec` → Batch.

#### `xargs -P` / `--process-slot-var` (parallelism)

bashkit runs a single `Bash` interpreter sequentially, even background `&`
jobs run synchronously for deterministic output (see
[Parallel Execution](parallel-execution.md)). So `xargs -P N` / `--max-procs=N` does **not**
spawn N OS processes for wall-clock speedup. Instead it allocates N
round-robin *slots* and the commands still run in order, with the slot index
(0..N-1, `idx % N`) surfaced via `--process-slot-var`. This is the behaviour
sharding logic depends on (`worker $SLOT of $N`) and matches GNU's
`--process-slot-var` for the deterministic case (single slot ⇒ index always
0). `-P 0` means "as many as possible" (one slot per command).

**Adding new execution plans:** Add a variant to `ExecutionPlan` and handle it
in the interpreter's plan fulfillment code (`interpreter/mod.rs`).

### Process-Local Host-Call Suspension

`BashBuilder::host_call_builtin(name)` registers a command fulfilled by the
host through `Bash::start_execution`. `ExecutionHandle::next_event()` polls the
ordinary interpreter future until it completes or the builtin sends a
`HostCallRequest`; `resume(id, ExecResult)` resolves the one-shot response and
lets the same future continue. The bounded request channel applies
backpressure, request IDs prevent mismatched responses, ordinary `exec()`
fails the builtin promptly, and the normal execution timeout remains armed
while a request is pending.

This mechanism intentionally does not change interpreter control flow into a
serializable state machine. The handle owns both a pinned Rust future and the
`Bash` instance; completion makes the session recoverable through `into_bash`,
while dropping a suspended handle drops the session so partially unwound state
cannot be reused. Pending calls cannot be included in snapshots or resumed in
another process. Portable mid-execution resume would require explicit
continuation frames for shell control flow, pipelines, substitutions,
redirects, accumulated output, and budgets; see
[Snapshot History](snapshot-history.md).

### Adding Internal Builtins

Simple builtins (zero-arg unit structs) are registered via the `register_builtins!`
macro in `interpreter/mod.rs`. To add a new one:

1. Create the builtin module in `crates/bashkit/src/builtins/` (implement `Builtin` trait)
2. Add `mod mycommand;` and `pub use mycommand::MyCommand;` in `builtins/mod.rs`
3. Add one line to the `register_builtins!` table in `interpreter/mod.rs`
4. Add spec tests in `tests/spec_cases/`
5. Run `just regen-builtins`; record any gaps in [Known Limitations](../operations/limitations.md)

### Structured Query Builtins

`jq` and `yq` are registered together by the `jq` Cargo feature. `jq` owns the
jaq evaluator, compatibility definitions, execution-budget accounting, deadline,
depth, and output controls. `yq` is a format boundary around that implementation:
it parses YAML/JSON into a JSON stream, calls `Jq::execute`, then serializes the
results. It must not add YAML-specific expression parsing or duplicate evaluator
logic.

The old `yaml get/keys/length/type` helper was replaced rather than retained as
a nonstandard compatibility surface. `yq -i` evaluates and serializes fully,
writes a sibling temporary VFS file, and renames it over the source only after
all earlier stages succeed. See [Known Limitations](../operations/limitations.md)
for deliberate mikefarah/yq gaps and [Threat Model](../security/threat-model.md)
for input/output bounds.

### Network Builtins

`curl`, `wget`, `http` require the `http_client` feature + URL allowlist.
When `bot-auth` feature is enabled, all outbound HTTP requests are transparently
signed with Ed25519 per RFC 9421 (see [Request Signing](../security/request-signing.md)).

### Archive Compression

`tar` supports plain, gzip, and bzip2 archives. It accepts GNU short/old-style
bundles (`-cjf`, `cjf`) and `--bzip2`, and detects gzip/bzip2 magic while listing
or extracting so `.tar.bz2`/`.tbz2` inputs work without an explicit codec flag.
`bzip2`, `bunzip2`, and `bzcat` share the same byte-native codec path.

The implementation uses `bzip2` 0.6 with its default pure-Rust
`libbz2-rs-sys` backend. The crate is maintained by the Trifecta Tech
Foundation, dual MIT/Apache-2.0; its backend retains the permissive
`bzip2-1.0.6` license. Both are admitted by `deny.toml`. The release supports
the repository's Rust floor and WASM targets and is newer than the 0.4.4 fix
for RUSTSEC-2023-0004. Cargo-vet exemptions are limited to these versions after
reviewing the wrapper's FFI slice bounds and stream lifecycle plus the enabled
backend's no-stdio Rust-allocator path, pointer bounds, allocation lifecycle,
and CRC failure path. Decoder output is checked and request-memory charged
before buffer growth; bzip2 stream/CRC errors fail closed. Filesystem quotas,
expansion ratio, aggregate input/work, live-memory leases, and extraction path
validation remain layered controls.

See [Threat Model](../security/threat-model.md) for TM-DOS-007/008/096/102 and
TM-INJ-010.
