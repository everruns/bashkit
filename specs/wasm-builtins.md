# WASM/WASI Builtins

> **Status: proposal / direction analysis.** No implementation. Evaluates
> WASM/WASI as (a) a security layer and (b) an extensibility point for
> external builtins. Threat IDs reserved as `TM-WASM-*`. Accepting this spec
> means Phase 1 below; later phases are separate decisions.

## Problem

Two gaps in the current model:

1. **Security is logic-level only.** Single trust boundary is
   `Bash::exec(&str)` (`specs/threat-model.md`). Everything below it —
   parser, interpreter, every builtin, every wrapped third-party library
   (jaq, serde_yaml, chrono, regex-lite, monty, zapcode, turso) — runs as
   trusted native code in the host process, fed directly with hostile script
   input. A memory-safety or logic bug in any of them is a host-process
   compromise or DoS. Precedent: TM-INF-023 — jaq-std's native `halt`
   called `std::process::exit()` and could kill the host. Mitigations today
   are conventions and tests (depth caps, TM-INF-022 no-Debug-leak scan,
   fuzz invariants, `catch_unwind`), not mechanical isolation.
2. **All extension points require full trust.** Custom `Builtin` impls,
   `Extension`, `BuiltinRegistry`, `ToolDef` callbacks — all trusted host
   Rust code linked into the process. No way to load a third-party builtin
   without trusting it completely; no non-Rust plugins; no plugin
   distribution story.

## What WASM/WASI adds

| Property | Today | With WASM guest |
|----------|-------|-----------------|
| Memory isolation | rustc safety + `unsafe` in deps | Guest linear memory; cannot read/corrupt host memory |
| Capability confinement | Convention + tests (env canary TM-INF-013, host-path scan TM-INF-016) | Structural: guest reaches only host-provided imports. Host env/FS/network/process unreachable by construction |
| Process control | `std::process::exit` in a dep kills host (TM-INF-023 class) | Guest `exit` ends the guest, maps to exit code |
| CPU limit | Command/loop/parser counters + wall clock | Deterministic per-instruction fuel + epoch deadline |
| Memory limit | Native builtins: none. Embedded runtimes: cooperative allocator tracking | Hard linear-memory cap via store limiter |
| Fault isolation | `catch_unwind` around builtin execution | Trap → error result; host unaffected |

What it does **not** change:

- Bash parser/interpreter stays native — script input still hits native code
  first. WASM hardens the builtin/plugin layer, not the core.
- Trusted host-code extension points remain (they're features, not bugs).
- In-process timing/speculation side channels between tenants: out of scope,
  unchanged.

## Decision (proposed)

Do **not** retrofit existing builtins into WASM — native perf matters on hot
paths (awk, jq, grep) and they're already covered by the testing layers.
Introduce WASM as the contract for **external builtins**: a new, feature-gated
plugin surface where isolation buys a genuinely new capability — running
semi-trusted, language-agnostic plugins. Per-builtin migration of risky
parsing cores stays a possible later, bench-gated decision.

### Runtime: wasmtime + component model + WASI 0.2

- Reference implementation, strong security track record, fuel + epoch
  interruption, async host functions (fits async `Builtin` + async VFS),
  `StoreLimits`, ahead-of-time precompilation. License Apache-2.0 WITH
  LLVM-exception (verify `cargo deny check` before Phase 1).
- Costs: large dependency (cranelift), compile time, binary size. Mitigate:
  new `wasm` cargo feature, **not** in `default`, native-only.
- **Cannot run inside wasm32 builds of bashkit** (browser /
  Pyodide wheel, `specs/emscripten-wheels.md`). Gate exactly like
  `http_client`/`sqlite`: excluded from wasm targets, loud `RuntimeError`
  for the kwarg on the wheel. Follow-up option (Phase 3): `wasmi`
  interpreter backend — itself compiles to wasm32 — for plugin support in
  browser builds. Out of scope initially.

### Guest contract: standard `wasi:cli/command` world

A plugin is just a CLI program. argv/env/stdin/stdout/stderr/exit-code map
1:1 onto `Context`/`ExecResult`, which is already the builtin contract. No
bashkit-specific SDK; any toolchain targeting `wasm32-wasip2` (Rust, Go,
C/C++, JS via componentize-js, Python via componentize-py) produces a valid
plugin; existing wasip2 CLI tools work as-is.

**Enforcement model.** A wasm guest has no syscalls and no ambient
authority: guest libc `open()`/`connect()` compile to calls of *imported
functions*, and the host decides at link time what implementation — if any —
sits behind each import. So "runs against the VFS" is not a per-call policy
check; it is the only topology that exists. WASI path operations are also
capability-scoped: every op is relative to a preopened directory descriptor
the host hands over — there is no ambient root through which the host FS
could even be named. Guarantee is as strong as (a) bashkit's shim code
(bug in shim = hole; hence the no-leak/fuzz obligations below) and
(b) wasmtime's isolation correctness (TM-WASM-009).

Host-side WASI implementation:

- `wasi:filesystem`: bashkit implements the filesystem host interface
  itself, backed by `Arc<dyn FileSystem>` (the VFS). **Never use
  wasmtime-wasi's stock implementation — its preopens map to the real OS
  FS.** This custom shim is the main Phase 1 work. Preopen exactly one
  root: `/` = VFS root; cwd from `Context`. `FsLimits`, `ReadOnlyFs`
  layers, and symlink policy (stored, never followed, L-FS-001) are
  enforced inside the VFS below the shim, so plugins can't bypass them any
  more than `cat` can. No host FS reachable by construction — `realfs`
  mounts visible only if mounted into the VFS.
- `wasi:http` (**required**, not optional): plugin networking goes through
  the exact same channel as `curl`/`wget` today. Bashkit implements
  `wasi:http/outgoing-handler` on top of the existing `HttpClient`, so
  every plugin request runs the full pipeline per hop: allowlist (literal
  host match, no DNS) → private-IP/SSRF filter → `before_http` hooks
  (credential injection — placeholders like `bk_placeholder_<32hex>`
  resolve host-side; the guest never sees raw secrets) → bot-auth signing
  → `after_http` hooks. No redirect following in the host (allowlist-bypass
  prevention, as today): 3xx is returned to the guest; if the guest
  re-requests, the new host is re-checked. Single egress path — the shim
  must not construct its own reqwest client. Gated on the `http_client`
  cargo feature *and* a configured allowlist, exactly like the curl
  builtin; otherwise linked as denying stubs. Guest-side implication:
  plugins must use wasi:http-native clients (Rust: `wstd`/`waki`;
  TinyGo `net/http` via wasi-http; componentize-js `fetch()`); HTTP stacks
  built on raw sockets won't work — by design, since sockets are stubbed.
- `wasi:sockets`: linked as denying stubs permanently — every operation
  returns `access-denied` (L-NET-001/002 parity: raw TCP/UDP would bypass
  the allowlist and the no-DNS-resolution model). Stubs, not omission:
  components whose toolchain pulls in the import without using it still
  instantiate.
- `wasi:cli` environment = `ctx.env` only; host process env is never
  passed in (TM-INF-013 fuzz canary applies). stdin from pipeline input;
  stdout/stderr are host-side sinks feeding `ExecResult`, so existing 1 MB
  caps apply before guest output is observable.
- Clocks/random: allowed (virtualizable later if determinism needed).

Phase 3 (optional, additive): custom `bashkit:host` WIT world for richer
plugins — shell variable access, execution extensions.

### Host integration

- `WasmBuiltin` implements `Builtin` (`Send + Sync`): holds a precompiled
  `Component` (`Send + Sync`); creates a fresh `Store` per `execute()`
  (`Store` is not `Sync`; per-exec instantiation is µs-cheap with AOT
  compilation and gives clean state — `reset_session_state` is a no-op,
  no cross-exec leakage). Async execution with fuel-yield keeps the
  executor responsive; futures stay `Send`
  (`specs/parallel-execution.md` constraints hold).
- Registration: `BashBuilder::wasm_builtin(name, plugin)` at build time;
  `BuiltinRegistry::insert` for runtime loading — the registry already
  supports post-build registration and is the natural plugin insertion
  point. Loading a plugin is an explicit host API call, so no
  `BASHKIT_ALLOW_*` env gate: those gates exist because in-process
  interpreters *weaken* isolation; WASM plugins strengthen it.
- Compilation: compile/validate at registration (not per exec), cap module
  size, cache AOT artifacts keyed by content hash. Accept precompiled
  `.cwasm` only through an explicitly-unsafe API (deserialization trusts
  the artifact).
- Limits: `WasmLimits { max_fuel, max_memory (64 MB), max_duration,
  max_module_bytes }`. `max_duration` clamped to
  `ExecutionDeadline::remaining()` like `PythonLimits`
  (`python.rs` deadline-min pattern). stdout/stderr flow through existing
  1 MB caps and truncation flags.
- Errors: traps/WASI errors → nonzero exit + capped stderr. TM-INF-022
  applies in full: no `{:?}` of wasmtime errors, diagnostics ≤ 1 KB,
  disable/strip trap backtraces (host paths). Requires `no_leak_wasm`
  test + fuzz target via `bashkit::testing::fuzz_exec`.

### Threat model additions (reserve on implementation)

| ID | Threat | Mitigation |
|----|--------|------------|
| TM-WASM-001 | Plugin CPU exhaustion | Fuel + epoch deadline |
| TM-WASM-002 | Plugin memory exhaustion | Store limiter cap |
| TM-WASM-003 | Output flooding | Existing stdout/stderr caps |
| TM-WASM-004 | Compile bomb / huge module | `max_module_bytes`; compile at registration |
| TM-WASM-005 | Trap/backtrace info leak | TM-INF-022 machinery, backtraces off |
| TM-WASM-006 | VFS quota bypass via `wasi:filesystem` | Shim routes through `FileSystem`; `FsLimits` unchanged |
| TM-WASM-007 | Cross-execution state leak | Fresh `Store` per exec |
| TM-WASM-008 | Malicious precompiled artifact | Source-bytes compilation by default; `.cwasm` behind unsafe API |
| TM-WASM-009 | Supply chain: wasmtime itself | Pin + `cargo deny`; feature off by default |
| TM-WASM-010 | Plugin HTTP bypasses allowlist/SSRF/credential pipeline | Single egress path: `wasi:http` shim delegates to `HttpClient` only; no redirect following host-side; `wasi:sockets` stubbed |
| TM-WASM-011 | Plugin exfiltrates injected credentials | Placeholder resolution host-side only; response bodies pass through `after_http` hooks; guest sees placeholders, never raw secrets |
| TM-WASM-012 | HTTP response flood into guest | Guest linear-memory cap bounds what the plugin can hold; response size cap in shim (open question below) |

### Phasing

1. **Phase 1 (MVP):** `wasm` feature (native-only). `WasmBuiltin` running
   `wasi:cli/command` components; VFS-backed `wasi:filesystem` shim; sockets
   stubbed; `wasi:http` shim over `HttpClient` (with `http_client` feature;
   stubs otherwise) — the two shims are the bulk of the work; limits
   mapping; example Rust plugin built to `wasm32-wasip2` and exercised in
   CI, including an HTTP-using plugin against the allowlist tests; security
   tests + `no_leak_wasm` + fuzz target; criterion baseline saved under
   `crates/bashkit/benches/results/`.
2. **Phase 2:** runtime loading via `BuiltinRegistry`; AOT cache; expose in
   `bashkit-python`/`bashkit-js` and the Tool layer (kwarg fails loudly on
   wasm targets, like `sqlite=`).
3. **Phase 3 (each a separate decision):** `bashkit:host` WIT world
   (variables, execution extensions); `wasmi` backend for wasm32 hosts;
   per-builtin migration of high-risk native parsers, gated on
   `just bench` deltas.

### Open questions

- Binary/compile-time budget: measure wasmtime impact before committing;
  if unacceptable, evaluate `wasmi` (smaller, slower) as the only backend.
- WASI p2/component vs p1/core-wasm: p2 recommended (ecosystem direction,
  typed interfaces); p1 fallback shrinks the dependency if needed.
- Plugin packaging/discovery: host application's job initially; no registry
  or search-path magic in bashkit.
- Per-plugin HTTP limits: response size cap and max requests per exec in
  the `wasi:http` shim (guest memory cap bounds retention, not transfer);
  whether to stream or buffer responses (buffer in Phase 1; `HttpClient`
  is buffered today).
- Streaming stdout: `ExecResult` is buffered; fine for Phase 1.

## See also

- `specs/threat-model.md` — trust boundary, TM-INF-022/023
- `specs/builtins.md` — `Builtin` trait, `Context`, `ExecutionPlan`
- `specs/vfs.md` — `FileSystem`/`FsBackend` the WASI shim maps onto
- `specs/python-builtin.md`, `specs/zapcode-runtime.md` — prior art for
  embedded runtimes with resource limits
- `specs/emscripten-wheels.md` — wasm32 host targets that exclude this
  feature
- `specs/parallel-execution.md` — Send/Sync constraints
