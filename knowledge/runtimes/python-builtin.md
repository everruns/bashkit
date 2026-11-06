---
type: Subsystem Design
title: Python Builtin
description: Embedded Python execution through Monty with security and resource controls.
tags:
  - bashkit
  - python
  - runtime
  - sandbox
---

# Python Builtin (Monty)

> **Experimental.** Monty is an early-stage Python interpreter that may have
> undiscovered crash or security bugs. Resource limits are enforced by Monty's
> runtime plus bashkit's per-VM allocator meter. Do not rely on it for untrusted-input safety without additional
> hardening.

## Status
Implemented (experimental)

For full CPython semantics in a WebAssembly sandbox, see
[CPython WebAssembly Runtime](cpython-wasm.md) (`cpython` feature).

## Decision

Bashkit provides sandboxed Python execution via `python` and `python3` builtins,
powered by the [Monty](https://github.com/pydantic/monty) embedded Python
interpreter written in Rust.

### Feature Flag and Registration

The `python` cargo feature enables compilation; registration is opt-in via
the builder (matching the `git` pattern):

```rust
let bash = Bash::builder()
    .python()                                   // or .python_with_limits(PythonLimits::default()...)
    .env("BASHKIT_ALLOW_INPROCESS_PYTHON", "1") // runtime gate, required
    .build();
```

For security, execution is runtime-gated on `BASHKIT_ALLOW_INPROCESS_PYTHON=1`
(builder `.env(...)` or `export`).

### Why Monty

- Pure Rust, no CPython dependency
- Sub-microsecond startup
- Built-in resource limits (memory, time, recursion depth)
- No filesystem/network access by design (sandbox-safe)
- Snapshotable execution state

### Supported Usage

`python3 -c "code"`, `python3 script.py`
(from VFS), stdin (`echo "code" | python3`, `python3 - <<< ...`),
`--version`/`-V`. Shebang lines are stripped automatically.

The value of the last expression is not echoed, matching CPython. Bashkit
appends a `None` statement to every program so Monty's final value is always
`None`: Monty converts the final value to a host object outside its memory and
time limits, and a shared list graph (`x = [x, x]` thirty times) expanded to
2^30 leaves there (~500 MiB, past the timeout). Until 2026-11-05 bashkit echoed
the value REPL-style.

### Resource Limits

A Monty time-limit hit (uncatchable `TimeoutError: time limit exceeded: ...`)
exits 124 with `python3: execution timed out after Ns` and keeps earlier
`print` output, matching the CPython runtime and coreutils `timeout`. A
script's own `TimeoutError` stays an ordinary exit-1 exception, even one
that copies the limit message: the tracker records when its time check fired.

Monty enforces its own limits independent of Bashkit's shell limits,
configurable via `PythonLimits`:

| Limit | Default | Builder Method |
|-------|---------|----------------|
| Max duration | 30 s | `.max_duration(d)` |
| Max memory | 64 MB | `.max_memory(bytes)` |
| Max recursion | 200 | `.max_recursion(depth)` |

`max_memory` counts real allocator bytes held by the run (see "Per-VM memory
metering" below), so it is stricter than the 0.0.19 estimate: a dict of
~500k short strings reaches the 64 MiB default. It also caps the host-side
buffer that collects `print` output. There is no allocation-count knob.

`max_duration` bounds wall-clock time. Monty 1.x pauses its own clock while
the VM waits on the host (VFS calls, external functions), so Monty alone
bounds VM time only: a loop of cheap file writes ran ~12x past the limit and
never yielded the worker thread. Bashkit checks the wall-clock deadline at
every host call (exit 124 like any time limit) and yields to the executor
there, so a caller-side timeout can cancel the call and co-tenants on the
same worker thread are scheduled (TM-PY-036).

Each Python entry also consumes the request-scoped `ExecutionBudget`: source
bytes are charged as aggregate input, Monty heap growth past the run's
high-water mark is charged as work (one unit per 64 bytes), and every 64 VM
time checkpoints consume one shared work unit. VFS pauses and external host
calls keep the same budget clone. Re-entering Python from a later command,
substitution, or pipeline stage therefore cannot obtain fresh aggregate fuel;
Monty's limits remain independently enforced. Until 2026-11-05 each entry
reserved `max_memory / 64` units up front (1M at the 64 MiB default), which
capped a default script at ~95 `python3` calls even for `pass`.

The parser enforces a nesting-depth limit (200 release / 35 debug) against
stack overflow from deeply nested expressions.

### Monty 1.4 embedding: forks, per-VM metering, checkpoint hook

Bashkit embeds Monty 1.4.0 through two forks in this workspace, published to
crates.io as `bashkit-monty` and `bashkit-monty-types` with each bashkit
release (like `bashkit-cpython-wasm`). Their library names stay `monty` and
`monty_types`. Source is upstream 1.4.0 with every local edit marked
`bashkit fork:`; rustfmt and lints are off for them, and upstream tests are not
vendored. Two panic/assert messages in `heap/` also drop the formatted id
or uuid, so CodeQL's cleartext-logging rule (a false positive there) stays
quiet. Drop the forks once upstream ships equivalent hooks.

Why forks (security): upstream 1.x enforces `max_memory` by reading
process-wide `LIVE_MEMORY`/`BASELINE_MEMORY` statics that only `monty-alloc`
writes, installed as the global allocator. That counter is shared by every
interpreter in the process and `monty-alloc` aborts the whole process at its
hard limit, so in a multi-tenant host one tenant is charged for another's
memory and can kill all of them. Without `monty-alloc` the probe reads 0 and
the limit is silently unenforced. 1.x also made `ResourceTracker` a struct, so
an embedder has no in-VM hook for cancellation or shared work budgets. The
fork adds two process-wide hooks in `monty_types::resource`:

- `set_memory_probe(fn() -> usize)` replaces the statics read (the same API as
  open upstream PR pydantic/monty#740).
- `set_checkpoint_hook(fn() -> Result<(), ResourceError>)` runs at every VM
  time checkpoint before the tracker's own time check; `Err` stops the run
  uncatchably.

Per-VM memory metering (`src/python_meter.rs`): `bashkit::MeteredAllocator`
wraps `System` and charges each allocation to the `VmMeter` of the Monty VM
currently running on that thread (a const thread-local pointer, set by an RAII
`VmWindow` around `MontyRun::new`, `start` and every `resume`). Those calls are
synchronous, so a window is exact: no other task runs on the thread while it is
open. The probe returns that meter's live bytes; the checkpoint hook runs the
request's `ExecutionBudget` check (cancellation, deadline, work units). The
allocator never refuses or aborts; Monty raises an uncatchable `MemoryError` at
its next check. Frees outside a window are not refunded, so the meter can only
over-count. Measured cost: none outside a window, ~3 ns per alloc/free pair
inside; `python3 -c pass` stays ~8 ms end to end in the CLI.

Fail closed: the meter only works when `MeteredAllocator` is the global
allocator. The CLI, the Python/Node/C/wasm bindings and bashkit's own tests
install it (a `#[global_allocator]` in a `cdylib` governs only that library's
Rust allocations, not CPython's or Node's). A Rust embedder must add it:

```rust
#[global_allocator]
static ALLOC: bashkit::MeteredAllocator = bashkit::MeteredAllocator;
```

Without it `python3` exits 1 with `python3: Monty memory limits need
bashkit::MeteredAllocator as the global allocator` instead of running unbounded.

Host policy: each run uses `OsPolicy` with a fixed virtual clock
(2024-01-01T00:00:00Z, the same instant bashkit's `date` uses),
`SleepMode::Zero`, and the shell cwd via `set_cwd`; Monty's defaults (host
clock, real sleep, system random) would leak host state. Every Monty OS call
pauses to bashkit: VFS, env, time, `Path.glob`/`rglob`/`walk` through
`OsFunctionCall::Scan` (a VFS walk capped at 100,000 entries), and `import`
of anything Monty does not bundle returns `ModuleNotFoundError`.

Shared object graphs: 1.x `MontyObject` is an opaque graph that can share
children, so a host conversion can expand exponentially. Every bashkit
converter (scripted-tool JSON, Python binding, runtime values) walks it with a
node cap.

Guarding tests: `python_tenant_isolation_tests` (concurrent tenants under one
limit, a greedy tenant beside small ones, many small objects, `str.join` bomb)
and the `whitebox_resource_limits` / `threat_python_pow_exhaustion` regression
tests that failed on every earlier bump attempt.

Amortized checks (fork edit, TM-PY-037): upstream `check_time_every(i)` and
`check_memory_time_every(i)` fire when the caller's loop index hits a multiple
of 64, so a recursive walk over many short loops never checked. `x = [x, x]`
thirty times, then `repr`/`print`/f-string/`==`/`<`/`in`, ran for minutes past
a 2 s limit, synchronously, so the shell timeout could not stop it either. The
fork counts every `*_every` call on one tracker-wide tick instead, and the
`repr` walk returns a time-limit error (exit 124) where upstream truncated to
`...[timeout]`; a memory hit still truncates. Pinned by
`whitebox_resource_limits::shared_graph_walks_respect_time_limit`.

Known upstream gap in 1.4.0: `str.join` can overshoot `max_memory` before the
next check (bounded by the meter, not by Monty).

### Python Feature Support

Monty implements a subset of Python 3.12. Supported: functions (incl.
defaults, `*args`/`**kwargs`, star unpacking, PEP 448), control flow,
exceptions, comprehensions/generator expressions, f-strings, core data
structures (list/dict/tuple/set/frozenset/namedtuple) with their operators
and views, `@property`, the common builtins (print, len, range, sorted,
isinstance, open, input, ...), `exec`/`eval`, and stdlib modules: json,
math, re, datetime, pathlib, os, sys, typing, csv, collections, itertools,
functools, dataclasses, hashlib, base64, random, decimal, unicodedata.

Simple classes (methods, attributes) work. Not supported
(Monty limitations): class inheritance and metaclasses, match statements, third-party imports, most stdlib modules, and HTTP/network I/O,
no `socket`/`urllib`/`requests`/`http.client`; Monty has no OsCall variants
for network operations, so there is no way to bridge these.

### VFS Bridging

Python `pathlib.Path` and `open()` operations are bridged to Bashkit's VFS
via Monty's OsCall pause/resume mechanism, so Python and bash share files:

```
Python code → Monty VM → OsCall(Open/ReadText, path) → Bashkit VFS → resume
```

Monty pauses at filesystem operations, yields an `OsCall` event with the
operation + arguments, Bashkit bridges it to the VFS, and resumes with the
result (or a Python exception).

Supported operations (one line): `open()`/`Path.open()` (read/write/append),
`Path.read_text/read_bytes/write_text/write_bytes/exists/is_file/is_dir/
is_symlink/mkdir/unlink/rmdir/iterdir/stat/rename/resolve/absolute/glob/rglob/walk`,
`os.getenv()`/`os.environ`, `datetime.date.today()`/`datetime.now(tz)`.

> **Note:** Monty has native filesystem mounting (`MountTable`,
> `MountDir`, `MountMode`) against host directories. Bashkit uses the OsCall
> bridge instead because our VFS is in-memory and may not be backed by host
> directories; the native mount system suits standalone real-filesystem use.

### External Functions

Host applications can register async external function handlers that Python
code calls by name, host capabilities (tool calls, lookups) without
serialization overhead; arguments arrive as raw `MontyObject` values.

```rust
let handler: PythonExternalFnHandler = Arc::new(|name, args, kwargs| {
    Box::pin(async move { ExtFunctionResult::Return(MontyObject::int(42)) })
});
let bash = Bash::builder()
    .python_with_external_handler(PythonLimits::default(), vec!["get_answer".into()], handler)
    .build();
```

- Handler signature: `(function_name: String, positional_args: Vec<MontyObject>, keyword_args: Vec<(MontyObject, MontyObject)>) -> Pin<Box<dyn Future<Output = ExtFunctionResult> + Send>>`.
- Returns `ExtFunctionResult::Return(MontyObject)` (value to Python) or `ExtFunctionResult::Error(MontyException)` (raises).
- **Dispatch:** one handler receives all registered names; dispatch on `function_name` inside it.
- **Request context:** inside the handler, `RuntimeCallContext::current()` exposes the calling `exec*` request: typed `ExecOptions` extensions, the aggregate `ExecutionBudget`, an execution lease (`execution_capability`), and the remaining wall-clock deadline. Outside a Python run every accessor returns `None`. Decision: handler signatures stay unchanged; the builtin wraps each run in a task-local scope instead.
- **Prelude:** `Python::with_external_handler_and_prelude(names, handler, prelude)` (public) evaluates host-provided Python before user code, e.g. classes that route `tools.orders.list(...)` to one external function.
- **Timeouts:** Each awaited handler call is wrapped in the remaining `PythonLimits::max_duration` wall-clock budget for the current Python invocation. If the budget expires while a handler is pending, Bashkit resumes Python with a `RuntimeError` instead of waiting for the handler indefinitely.
- **Trust model:** same as `BashBuilder::builtin()` and `ScriptedTool` callbacks, host registers trusted Rust code, untrusted scripts invoke by name. Handlers are trusted host code and should still enforce independent limits for outbound I/O, remote services, and other resources they consume.
- **Values:** `MontyObject` is an opaque graph. Build with constructors (`MontyObject::int`, `string`, `list`, `dict`, `tuple`, `none`, ...); read through `obj.as_ref()` (`ObjectRef::as_int`, `as_str`, `items`, `pairs`, `type_name`). `bashkit::monty_unstable` re-exports Monty's node-level API for full walks.
- **Unstable re-exports:** `MontyObject`, `ObjectRef`, `ExtFunctionResult`, `MontyException`, `ExcType` re-exported from the forked `monty_types`; may break between bashkit releases.

### Security

See [Threat Model](../security/threat-model.md) § "Python / Monty Security (TM-PY)" for the full
analysis. Summary:

- **Code injection via bash expansion**: variables expand before reaching the builtin (by-design, consistent with all builtins); use single quotes to prevent.
- **Resource exhaustion**: Monty's time/memory caps apply even when shell limits are generous; print output is captured in memory and bounded by the same memory cap.
- **Sandbox escape via filesystem**: all path ops go through the VFS; `/etc/passwd` reads VFS, not host. Relative paths resolve against the shell cwd; `../..` traversal constrained by VFS path normalization.
- **Sandbox escape via os/subprocess/socket**: not implemented in Monty; raise errors.

### Error Handling

Exit code 1: syntax/runtime errors (Python traceback on stderr; stdout
produced before a runtime error is preserved). Exit code 2: usage errors,
file not found, missing `-c` argument, unknown option.

### LLM Hints

Registration via `BashToolBuilder::python()` contributes a hint to `help()` /
`system_prompt()` documenting the limitations (stdlib subset, VFS-only file
I/O, no network/third-party imports, simple classes only, no generators);
`test_llm_hint_matches_runtime` fails if the hint drifts from what Monty runs through the general
`Builtin::llm_hint()` mechanism (hints deduplicated automatically).

`re` is enabled (decision 2026-11-05). It was disabled on 0.0.19 because one
native match bypassed Monty's time checks (ReDoS). In 1.4, `re` runs plain
patterns on the linear-time `regex` engine and only backreference/lookaround
patterns on `fancy-regex`, whose backtrack cap raises `re.PatternError`
instead of spinning; a Python loop of matches is stopped by the time limit.
`exec`/`eval` exist in 1.4, so an import gate could not be complete anyway.
`python_security_tests::regex_*` pins the bounds (seven classic ReDoS
patterns, a match loop that must exit 124, and a huge pattern).

### Integration with Bashkit

`python`/`python3` map to the same builtin; works in pipelines (stdin
provides *code*, not data, matches real python's no-arg behavior), command
substitution, and conditionals.

With the `bashkit-scripted-tool` crate (feature `python`), `ToolRegistry::install(builder)` generates an
explicit `tools` namespace from dot-separated `ToolDef` names. Calls such as
`tools.orders.list({"customer": "acme"})` use Monty's existing external-function
suspend/resume bridge and dispatch through the registry's shared schema, policy,
deadline, sanitizer, callback, and request-local trace. `tools.discover({...})`
returns registry metadata. Tenant context comes only from the current
`ToolCallRequest` execution extension.

## Verification

```bash
cargo test --features python --lib -- python
cargo test --features python --test spec_tests -- python
cargo test --features python --test threat_model_tests -- python
```

Fuzzing: `crates/bashkit/fuzz/fuzz_targets/python_fuzz.rs` runs nightly (`fuzz.yml`)
and is build-checked on every PR (`fuzz-check`). It installs `MeteredAllocator`
like the shipped bindings, caps each run at 32 MiB / 500 ms, and runs input as a
file, stdin, `-c` and CLI arguments. A memory-limit bypass trips libFuzzer's RSS
limit, a time-limit bypass the 5 s per-exec assertion, and stderr goes through
`fuzz_exec`'s leak checks. The fork is upstream code we now own, so this target
is where inherited interpreter bugs show up.

```bash
cargo +nightly-2026-10-30 fuzz run --features python python_fuzz -- -max_total_time=300
```

Stress: `tests/integration/python_tenant_stress_tests.rs` runs 20 tenants on 4
worker threads per round, mixing memory bombs, runaway loops, unbounded
recursion and well-behaved scripts; it also drops calls mid-run and then checks
that the worker threads still meter correctly, and reuses one session's budget
across many calls. 3 rounds per PR; `BASHKIT_STRESS_ROUNDS=N` for a soak.
