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
> runtime. Do not rely on it for untrusted-input safety without additional
> hardening.

## Status
Implemented (experimental)

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

`python3 -c "code"` (REPL-like: last expression printed), `python3 script.py`
(from VFS), stdin (`echo "code" | python3`, `python3 - <<< ...`),
`--version`/`-V`. Shebang lines are stripped automatically.

### Resource Limits

Monty enforces its own limits independent of Bashkit's shell limits,
configurable via `PythonLimits`:

| Limit | Default | Builder Method |
|-------|---------|----------------|
| Max duration | 30 s | `.max_duration(d)` |
| Max memory | 64 MB | `.max_memory(bytes)` |
| Max recursion | 200 | `.max_recursion(depth)` |

`max_memory` does double duty: Monty caps the host-side buffer that collects
`print` output at the same value, so a print loop cannot outgrow the declared
budget even though it allocates nothing on the VM heap. There is no
allocation-count knob, Monty removed `max_allocations` in 0.0.19.

Each Python entry also consumes the request-scoped `ExecutionBudget`: source
bytes are charged as aggregate input, configured memory contributes a
conservative non-refundable admission reservation, and Monty's time/allocation
checkpoints consume shared work units. VFS pauses and external host calls keep
the same budget clone. Re-entering Python from a later command, substitution,
or pipeline stage therefore cannot obtain fresh aggregate fuel; Monty's limits
remain independently enforced.

Since Monty 0.0.4 the parser also enforces a nesting-depth limit (200
release / 35 debug) against stack overflow from deeply nested expressions.

### Upgrade blocker: Monty is held at 0.0.19

`monty` and `monty-types` are pinned to 0.0.19 and ignored in
`.github/dependabot.yml`. This is a security hold, not API-churn convenience.

Monty **0.0.20** changed how `max_memory` is enforced, and 0.0.21 carries the
same change. Verified 2026-08-20 against the published sources: the
`LIVE_MEMORY` / `BASELINE_MEMORY` statics first appear in
`monty-types-0.0.20/src/resource.rs:29-32`, with the probe at line 413. There is
**no safe intermediate version** — 0.0.20 is not a stepping stone:

| | 0.0.19 | 0.0.20 and 0.0.21 |
|---|---|---|
| Enforcement | `LimitedTracker::on_grow` accounts VM heap growth in-process | `probe_memory()` reads the `LIVE_MEMORY` / `BASELINE_MEMORY` statics |
| Who populates it | the tracker itself | only `monty-alloc`, installed as the **process-wide global allocator** |
| Embedder needs | nothing | to own the host's global allocator |

Neither `monty` nor `monty-types` depends on `monty-alloc`. When it is absent
the statics keep their initial values and `probe_memory()` evaluates to
`0.saturating_sub(usize::MAX)` == 0, so `check_allocation`, and therefore both
`max_memory` and `check_large_result`, can never trip. The ceiling is not
merely weakened; it is silently unenforced, with no error or warning.

Bashkit cannot supply the missing allocator. It is an embeddable library, so
the global allocator belongs to the downstream binary, and `bashkit-python` is a
CPython extension module where the allocator is CPython's. `monty-alloc` is
built for Monty's own out-of-process worker model ("a Monty *worker's* hard
memory ceiling"), which bashkit does not use.

Four existing threat-model regression tests fail on the bump and are the
standing guard for this, they need no new test to be added:

- `python_security_tests::whitebox_resource_limits::nested_list_bomb`
- `python_security_tests::whitebox_resource_limits::successive_allocations_accumulate`
- `python_security_tests::whitebox_resource_limits::tight_memory_blocks_many_small_objects`
- `threat_model_tests::python_security_regressions::threat_python_pow_exhaustion`

The duration, recursion, and print-collect-cap limits are unaffected and still
pass, which localises the regression to allocator-backed memory.

Two further changes land in the same bump and must be handled when the hold is
lifted; neither is a blocker on its own:

- `ResourceTracker` became a concrete struct (upstream pydantic/monty#613), so
  the host can no longer wrap VM checkpoints. `BudgetTracker`'s bridge into the
  shared `ExecutionBudget` has to move outside the VM, charging work around
  each synchronous `start`/`resume` section instead of inside it. The
  per-entry admission reservation that enforces TM-DOS-096 is independent of
  the tracker and survives either way.
- `ResourceLimits::new()` is replaced by `Default`, and its builders take plain
  values rather than `Option`s.

Lifting the hold also **removes** a `[patch.crates-io]` git pin: the patch in
the root `Cargo.toml` exists only because `monty 0.0.19` requires
`jiter ^0.15.0`, whose published release holds the graph below pyo3 0.29.
Monty 0.0.21 depends on the published `jiter 0.16.0`, which already tracks
pyo3 0.29, so the workspace would build from crates.io alone with no git
dependency in the release graph. That is a real supply-chain win waiting on the
memory fix, it is not a reason to take the bump early.

### Python Feature Support

Monty implements a subset of Python 3.12. Supported: functions (incl.
defaults, `*args`/`**kwargs`, star unpacking, PEP 448), control flow,
exceptions, comprehensions/generator expressions, f-strings, core data
structures (list/dict/tuple/set/frozenset/namedtuple) with their operators
and views, `@property`, the common builtins (print, len, range, sorted,
isinstance, open, input, ...), and stdlib modules: sys, typing, math,
pathlib, os (getenv/environ), json, datetime (incl. `date.today()`,
`datetime.now(tz)`).

Not supported (Monty limitations): classes (planned upstream), match
statements, third-party imports, most stdlib modules, and HTTP/network I/O,
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
is_symlink/mkdir/unlink/rmdir/iterdir/stat/rename/resolve/absolute`,
`os.getenv()`/`os.environ`, `datetime.date.today()`/`datetime.now(tz)`.

> **Note:** Monty 0.0.10+ has native filesystem mounting (`MountTable`,
> `MountDir`, `MountMode`) against host directories. Bashkit uses the OsCall
> bridge instead because our VFS is in-memory and may not be backed by host
> directories; the native mount system suits standalone real-filesystem use.

### External Functions

Host applications can register async external function handlers that Python
code calls by name, host capabilities (tool calls, lookups) without
serialization overhead; arguments arrive as raw `MontyObject` values.

```rust
let handler: PythonExternalFnHandler = Arc::new(|name, args, kwargs| {
    Box::pin(async move { ExtFunctionResult::Return(MontyObject::Int(42)) })
});
let bash = Bash::builder()
    .python_with_external_handler(PythonLimits::default(), vec!["get_answer".into()], handler)
    .build();
```

- Handler signature: `(function_name: String, positional_args: Vec<MontyObject>, keyword_args: Vec<(MontyObject, MontyObject)>) -> Pin<Box<dyn Future<Output = ExtFunctionResult> + Send>>`.
- Returns `ExtFunctionResult::Return(MontyObject)` (value to Python) or `ExtFunctionResult::Error(MontyException)` (raises).
- **Dispatch:** one handler receives all registered names; dispatch on `function_name` inside it.
- **Timeouts:** Each awaited handler call is wrapped in the remaining `PythonLimits::max_duration` wall-clock budget for the current Python invocation. If the budget expires while a handler is pending, Bashkit resumes Python with a `RuntimeError` instead of waiting for the handler indefinitely.
- **Trust model:** same as `BashBuilder::builtin()` and `ScriptedTool` callbacks, host registers trusted Rust code, untrusted scripts invoke by name. Handlers are trusted host code and should still enforce independent limits for outbound I/O, remote services, and other resources they consume.
- **Unstable re-exports:** `MontyObject`, `ExtFunctionResult`, `MontyException`, `ExcType` re-exported from the `monty` crate (pre-1.0, tracked at `0.0.x`); may break between bashkit releases.

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
I/O, no network/classes/third-party imports) through the general
`Builtin::llm_hint()` mechanism (hints deduplicated automatically).

The regex module `re` is intentionally disabled in Bashkit due to
catastrophic-backtracking DoS risk in untrusted code execution.

### Integration with Bashkit

`python`/`python3` map to the same builtin; works in pipelines (stdin
provides *code*, not data, matches real python's no-arg behavior), command
substitution, and conditionals.

With the `scripted_tool` feature, `BashBuilder::tool_registry` generates an
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
