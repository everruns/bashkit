---
type: Subsystem Design
title: ZapCode Runtime
description: Embedded TypeScript runtime, external functions, VFS bridging, and resource limits.
tags:
  - bashkit
  - typescript
  - runtime
  - sandbox
---

# ZapCode TypeScript Runtime

> **Experimental.** ZapCode is an early-stage TypeScript interpreter. Resource
> limits are enforced by ZapCode's VM. Do not rely on it for untrusted-input
> safety without additional hardening.

## Status
Experimental

## Decision

Sandboxed TypeScript/JavaScript execution via `typescript`, `ts`, `node`,
`deno`, and `bun` builtins, powered by the
[ZapCode](https://github.com/TheUncharted/zapcode) embedded TypeScript
interpreter written in Rust (`zapcode-core` 1.5.1, vendored in-tree as
`crates/bashkit/src/zapcode_vendor`, see "Vendored source"). Runs directly in
the host process, no subprocess, no IPC.

### Registration (Opt-in)

The `typescript` cargo feature enables compilation; registration is explicit,
matching the `python` pattern: `Bash::builder().typescript()`,
`.typescript_with_limits(limits)`, or `.extension(TypeScriptExtension::default())`
(works on both `BashBuilder` and `BashToolBuilder`). TypeScript builtins are
**not** auto-registered.

### Why ZapCode

- Pure Rust, no V8 or Node.js dependency
- Microsecond cold starts (~2 µs)
- Built-in resource limits (memory, time, stack depth)
- No filesystem/network/eval access by design (sandbox-safe)
- Snapshotable execution state (<2 KB)
- External function suspend/resume for VFS bridging
- MIT, small enough to vendor and patch (`zapcode-core`)

### Usage

Inline code (`ts -c "..."` / `node -e "..."`; `-c` and `-e` both accepted by
all aliases), expression evaluation (last expression printed), script file
from VFS, stdin (`ts -`), `--version`. All aliases (`ts`, `typescript`,
`node`, `deno`, `bun`) map to the same ZapCode builtin. Shebang lines
(`#!/usr/bin/env ts`) stripped automatically. Works in pipelines, command
substitution, and conditionals like any builtin.

### Resource Limits

ZapCode enforces its own resource limits independent of Bashkit's shell limits.
Configurable via `TypeScriptLimits`:

| Limit | Default | Builder Method | Purpose |
|-------|---------|----------------|---------|
| Max duration | 30 seconds | `.max_duration(d)` | Prevent infinite loops |
| Max memory | 64 MB | `.max_memory(bytes)` | Prevent memory exhaustion |
| Max stack depth | 512 | `.max_stack_depth(n)` | Prevent stack overflow |
| Max allocations | 1,000,000 | `.max_allocations(n)` | Bound VM allocation count |

`max_allocations` counts VM stack pushes and array/object creations, and the
counter never decreases within a run (`zapcode_vendor/sandbox.rs`,
`track_allocation`), so it is a per-run budget of operations rather than a live
heap bound - that is `max_memory`. A `while (true) {}` spends about one per
iteration, so the 1M default stops a runaway loop after about 1M iterations
regardless of how little heap it holds.

Every VM entry charges source bytes, a fixed 4096 work units, and the
microseconds each synchronous VM step ran to the request-scoped
`ExecutionBudget`. Suspend/resume cycles and external/VFS callbacks retain the
same shared clone. Separate `ts`/`node`/`deno`/`bun` commands inside one host
execution therefore cannot refresh aggregate work, while ZapCode's own limits
remain in force. Until 2026-11-05 each entry reserved `max_allocations` (1M)
units up front, which capped a script at about 95 `ts` calls against the
default 100M budget (same bug as the Python runtime had).

**Source nesting.** ZapCode 1.5.1 parses (oxc), lowers and compiles with plain
recursion and no depth guard; about 6 KB of nested source (`[[[...`,
`1+1+...`) overflowed the native stack and aborted the host process in release
builds (about 100 levels on a 2 MB debug thread). Bashkit adds two layers:

1. `builtins/typescript_nesting.rs` computes a conservative upper bound on AST
   depth before ZapCode sees the source (frames plus tokens since the last
   `,`/`;`/statement newline in each open frame; closers fold counts into the
   parent, regex bodies are charged per token). Above 1000 the command fails
   with `ts: source is too deeply nested` (exit 1).
2. Every synchronous ZapCode step (parse, compile and first run, and each
   `resume` after a VFS/external call) runs on a dedicated thread with a
   128 MiB stack (reserved, not committed). 32 MiB was the smallest stack on
   which every shape in `deep_nesting_at_the_limit_does_not_overflow` passed
   in a debug build, so this is 4x margin. If the thread cannot be started the
   command fails with `ts: cannot start execution thread` (exit 1); it never
   falls back to the caller's smaller stack.

Residual risk on wasm32: there are no threads, so ZapCode runs on the default
wasm stack with the same 1000 cap, which was validated only against the
128 MiB native thread. An overflow there traps the wasm instance (no host
process abort, but the instance is lost). A lower wasm cap waits on measuring
the wasm stack per AST level.

Upstream fix wanted: a depth guard in zapcode-core's `AstLowerer` and
compiler (Monty and ruff already return "Source is too deeply nested").

### Language Support

ZapCode implements a TypeScript/JavaScript (ES2024) subset, see ZapCode docs
for details. Type annotations parsed but not enforced. Not supported by
design: `import`/`require` (no module system), `eval()`/`Function()`,
filesystem access (use bridged functions), network access,
`process`/`Deno`/`Bun` globals, DOM APIs, most Node/Deno/Bun stdlib.

### VFS Bridging

TypeScript code accesses Bashkit's VFS through external functions registered
by the builtin, available as globals:

- `readFile(path: string): Promise<string>`
- `writeFile(path: string, content: string): Promise<void>`
- `exists(path: string): Promise<boolean>`
- `readDir(path: string): Promise<string[]>`
- `mkdir(path: string): Promise<void>`
- `remove(path: string): Promise<void>`
- `stat(path: string): Promise<{size, isFile, isDir}>`

Architecture: ZapCode suspends execution at external function calls
(`TS code → ZapCode VM → ExternalFn → Bashkit VFS → resume`), Bashkit bridges
the call to the VFS, and resumes with the result.

**Limitation: stdout after VFS calls.** `ZapcodeSnapshot::resume()` returns
`VmState` but does not expose the VM's accumulated stdout, so `console.log()`
output produced *after* a VFS call is lost. Use the return-value pattern
instead (last expression's value is printed); `console.log` *before* the VFS
call works. `zapcode-core` API limitation; upstream fix tracked.

### External Functions

Hosts can register custom external functions callable by name from TypeScript
(tool calls, API requests, etc.) via
`Bash::builder().typescript_with_external_handler(limits, names, handler)`
where `handler: TypeScriptExternalFnHandler` is an
`Arc` async closure `(name, args) -> Result<serde_json::Value>`. See rustdoc.

Inside the handler, `RuntimeCallContext::current()` returns the calling
request's extensions, budget, execution lease, and remaining deadline (same
contract as Python). `TypeScriptExtension::with_external_handler_and_prelude`
(public) adds a source prelude and `(from, to)` call rewrites so hosts can
expose namespaced APIs (`tools.orders.list(...)`) without core knowing them.

### Security

See [Threat Model](../security/threat-model.md) section "TypeScript / ZapCode Security (TM-TS)"
for the full threat analysis.

- **Code injection via bash variable expansion**: variables expand before
  reaching the builtin, by-design consistent with all builtins. Use single
  quotes to prevent expansion.
- **Resource exhaustion**: time, stack and allocation-count caps apply even if
  shell limits are generous. Memory is enforced by Bashkit, not ZapCode (see
  [Memory limit](#memory-limit)).
- **Sandbox escape via filesystem**: no direct FS access; bridged functions go
  through the VFS (`/etc/passwd` reads VFS, not host).
- **Sandbox escape via eval/import**: `eval()`, `Function()`, `import`,
  `require` blocked at the language level (not implemented).
- **DoS via large output**: console.log output captured in memory; the memory
  limit bounds it.

### Error Handling

- Syntax errors: Exit code 1, error message on stderr
- Runtime errors: Exit code 1, error on stderr, any stdout preserved
- Time limit: Exit code 124, `ts: execution timed out after Ns` on stderr
  (ZapCode's `TimeLimitExceeded` variant, so a script's `throw` cannot fake it)
- Source nested past the cap: Exit code 1, `ts: source is too deeply nested`
- File not found: Exit code 2, error on stderr
- Missing `-c`/`-e` argument: Exit code 2, error on stderr
- Unknown option: Exit code 2, error on stderr

### Tool registry access

With the `bashkit-scripted-tool` crate (feature `typescript`), `ToolRegistry::install(builder)` exposes dot-separated
`ToolDef` names as `await tools.orders.list({customer: "acme"})` and provides
`tools.discover({...})`. Calls reuse the existing external-function
suspend/resume bridge and the registry's one schema, policy, deadline, sanitizer,
callback instance, and request-local trace.

ZapCode snapshots cannot serialize a function nested inside a `tools` object.
The adapter token-rewrites executable registry access to validated hidden
external names and JSON-encodes the argument before suspension; quoted strings
and comments are not rewritten. Use the existing return-value pattern after an
external call because post-resume `console.log` output is not retained upstream.

### LLM Hints

When registered via `BashToolBuilder::typescript()`, the builtin contributes
a hint to `help()` and `system_prompt()`:

> ts/node/deno/bun: Embedded TypeScript (ZapCode). Supports ES2024 subset.
> File I/O via readFile()/writeFile() async functions. No npm/import/require.
> No HTTP/network. No eval().

### Memory limit

Decision (2026-11-10, TM-TS-026): ZapCode 1.5.1 stores `memory_limit_bytes` but
never reads it, and a failed Rust allocation aborts the process with every
tenant. Bashkit enforces `max_memory` itself with the meter Monty uses
(`crate::vm_meter`, charged by `MeteredAllocator`):

- Each `ts` run gets its own `VmMeter`. Every ZapCode step (start, each resume
  after an external call, formatting the result value) runs inside a
  `VmWindow` for it, on the same thread, so only that run's allocations count.
- Every instruction (`check_time`) stops the run with `memory limit exceeded`
  once live heap passes the limit. That catches gradual growth: doubling a
  string with `+` or a template literal, `concat`, spread, `push` loops.
- Strings are shared `Arc<str>`, so `[s, s, s]` costs pointers but `join`,
  array `toString`, `JSON.stringify`, `''.concat(...)` and `console.log(...)`
  produce full copies. `src/zapcode_guard.rs` sizes those outputs before
  allocating (the walk stops once past the remaining budget) and refuses.
  `repeat`, `split`, `replace`/`replaceAll` and `padStart`/`padEnd` reserve
  their computed size the same way. Refusal is sticky, so a caller that cannot
  return an error (array `toString`) returns an empty string and the next
  instruction stops the run.
- Without `MeteredAllocator` as the global allocator the meter would read
  zero, so native `ts` fails closed with an error naming the allocator. Every
  bashkit binary, binding, example and test harness that enables
  `typescript` installs it. On wasm32 the check is skipped, and
  bashkit-wasm installs the allocator for its `typescript` feature. Residual
  risk: a third-party wasm32 embedder that enables `typescript` without the
  allocator gets only the per-operation size checks, not the cumulative
  limit (a tab or isolate OOM, not a shared host abort).

Not metered: memory allocated by the host between steps (VFS bridging, the
external-function callbacks), which is bounded by Bashkit's own limits.

### Wall-clock limit and stress

Decision (2026-11-10, TM-TS-027): ZapCode restarts its time tracker at every
resume, so `max_duration` alone bounded one VM step, not the run. The host-call
loop in `typescript.rs` checks a wall-clock deadline (run start plus
`max_duration`) before every host call, caps the call itself at the remaining
time, and yields to the executor, so caller-side timeouts can cancel a run and
co-tenants on the worker thread keep running. A single VM step can still run
up to `max_duration`, so a run ends within about twice the limit.

`tests/integration/ts_tenant_stress_tests.rs` mixes hostile `ts` tenants
(memory doubling, shared-string amplification, spin, recursion, abandoned
calls, huge single allocations) with good ones on shared worker threads; each
must hit only its own limit. It runs 3 rounds on every PR;
`BASHKIT_STRESS_ROUNDS` sets a longer soak. Each host call snapshots the VM
heap, so a held 8 MiB string costs about 3x that during the call and counts
against `max_memory`.

Decision (2026-11-11): the spin tenant asserts only that it stopped on one of
its own limits, because `max_allocations` and `max_duration` race and the
winner depends on build speed. A release build burns the 1M push budget in a
few milliseconds; a debug build reaches a 300 ms `max_duration` first. Pinning
exit 124 there passed every debug PR run and failed the release nightly soak.
Each limit is instead pinned on its own by
`ts_runaway_loop_stops_on_the_wall_clock` (budget raised so the clock binds)
and `ts_runaway_loop_stops_on_the_allocation_budget` (budget lowered so it
binds), both deterministic in either profile.

Nightly (`nightly.yml`): an ASan pass over the ts unit, threat-model, security
and stress tests (`--features typescript`, filters `typescript threat_ts
ts_tenant_stress zapcode`), and a `ts-stress-soak` job at 300 rounds in
release. Fuzzing: `fuzz/fuzz_targets/ts_fuzz.rs` runs in the nightly
`fuzz.yml` matrix under the metered allocator with a 32 MiB `max_memory`, so a
memory-limit bypass trips libFuzzer's RSS limit; a canary run asserts `ts`
really executes. Locally:
`cargo +nightly-2026-10-30 fuzz run --features typescript ts_fuzz -- -max_total_time=300`.

### Vendored source

Decision (2026-11-10): every target builds the in-tree copy of ZapCode 1.5.1 in
`crates/bashkit/src/zapcode_vendor`; the registry `zapcode-core` crate is no
longer a dependency. Two reasons:

1. **Resource-limit fixes.** Upstream allocates unchecked in places, and an
   allocation failure aborts the whole process (all tenants). Bashkit carries
   those fixes as `bashkit fork:` edits, listed in `fork-edits.json`. Current:
   `padStart`/`padEnd` reject a target length above 10,000,000 with
   `AllocationLimitExceeded`, the same cap upstream applies to `repeat`
   (TM-TS-025, security audit run-1 F5), and the memory-limit hooks in
   [Memory limit](#memory-limit) (TM-TS-026).
2. **Browser clocks.** ZapCode uses std Instant in its resource tracker and
   Instant/SystemTime in tracing, which panic on wasm32-unknown-unknown. The
   copy reads Bashkit's clock shim (`crate::time`), which is std::time on
   native, so time limits stay enabled everywhere.

A crates.io fork (like `bashkit-monty`) was not needed: ZapCode has no
dependents outside bashkit, and the copy ships inside the `bashkit` crate.

Upstream is MIT, with its license retained. `upstream-sha256.json` records every
original source file. Mechanical changes: `crate::` to
`crate::zapcode_vendor::`, lib.rs to mod.rs. `scripts/check_zapcode_vendor.py`
(CI) reverses those, the two clock substitutions and every listed
`bashkit fork:` edit, then checks hashes and the complete file inventory; an
unlisted marker or drifted edit fails. Rustfmt and Clippy skip this upstream
source like the jaq-json vendor. To update, copy a reviewed crate release,
regenerate hashes, re-apply the namespace/clock changes and fork edits (drop
any upstream fixed), retain the license, and rerun the TypeScript threat-model
tests and the real browser runtime suite. Keep oxc/postcard/indexmap versions
aligned with upstream. Never disable deadlines or limits in the copy.

See [Browser Package](browser-package.md) for bundle and callback session rules.
