---
type: Package Design
title: Python Package
description: Python bindings, PyPI wheels, ABI strategy, and platform build matrix.
tags:
  - bashkit
  - python
  - packaging
  - pypi
---

# Python Package

## Status

Implemented

## Abstract

Bashkit ships a Python package as pre-built binary wheels on PyPI. Users install with
`pip install bashkit` and get a native extension, no Rust toolchain needed.

## Package Layout

`crates/bashkit-python/`: Rust crate (`src/lib.rs`, cdylib via PyO3),
`pyproject.toml` (maturin backend), `bashkit/` Python package (`__init__.py`
re-exports, `_bashkit.pyi` PEP 561 type stubs + `py.typed`, `langchain.py` /
`deepagents.py` / `pydantic_ai.py` integrations), `examples/`, `tests/`.

## Build System

- **Build backend**: [maturin](https://github.com/PyO3/maturin) (1.4–2.0)
- **Rust extension**: [PyO3](https://pyo3.rs/) 0.24 with `extension-module` feature
- **Async bridge**: `pyo3-async-runtimes` (tokio runtime)
- **Module name**: `bashkit._bashkit` (native), re-exported as `bashkit`

## Versioning

Python package version is read dynamically from workspace `Cargo.toml` via maturin
(`dynamic = ["version"]` in pyproject.toml), no manual sync. Chain:
workspace `Cargo.toml` → `bashkit-python` `Cargo.toml` (inherits) → maturin → wheel metadata.
`bashkit.__version__` delegates to the extension's `get_version()` so imported
package, native API, and distribution metadata identify the same release.
`tests/test_integration.py::test_package_version_matches_native_and_distribution`
checks all three against an installed wheel.

## Supported Platforms

7 platforms: Linux x86_64/aarch64 (manylinux glibc + musllinux_1_1), macOS
x86_64/aarch64, Windows x86_64 MSVC. Exact matrix and runners:
`.github/workflows/publish-python.yml`.

Thanks to abi3 (below) this is **7 native wheels per release** (one
`cp39-abi3` wheel per platform, each running on Python 3.9+), not one wheel per
`{Python minor × platform}` combination.

In addition, a **reduced-feature Pyodide/Emscripten wheel**
(`wasm32-unknown-emscripten`) ships for browser / JupyterLite use, built and
published separately (different toolchain, single Python version, no
async/network/sqlite/realfs). See [Emscripten Wheels](emscripten-wheels.md).

## PyPI Publishing

`.github/workflows/publish-python.yml`, triggered on GitHub Release: sdist +
platform wheels → `twine check` → per-platform smoke test
(`BashTool().execute_sync('echo hello')`) → `uv publish` to PyPI.

Auth: PyPI trusted publishing (OIDC), no API tokens. Prerequisites: GitHub
environment `release-python` exists; PyPI trusted publisher configured for
`everruns/bashkit`, workflow `publish-python.yml`, environment `release-python`.

## Public API

Full signatures: `crates/bashkit-python/bashkit/_bashkit.pyi`. Runnable
examples: `crates/bashkit-python/examples/`.

### BashTool / Bash

`BashTool` wraps the Rust `Bash` interpreter with `Arc<Mutex<>>` for thread
safety. Constructor kwargs: `username`, `hostname`, `cwd` (initial working
directory), `env` (initial environment variables), `max_commands`,
`max_loop_iterations`, `readonly_filesystem`, `files` (initial files; values
may be eager strings or lazy sync callables), `network`, `custom_builtins`,
etc. Methods: `await execute(cmd)` / `execute_sync(cmd)` / `reset()`;
`set_env(key, value)` and `mount(vfs_path, fs)` / `unmount(vfs_path)` for
host configuration applied *after* construction; direct
text-oriented VFS helpers (`read_file`, `write_file`, `append_file`, `mkdir`,
`exists`, `remove`, `stat`, `chmod`, `symlink`, `read_link`, `read_dir`,
`ls`, `glob`); LLM metadata (`name`, `short_description`, `description()`,
`help()`, `system_prompt()`, `input_schema()`, `output_schema()`, `version`).

### Embedded tool runtimes

`BashTool(python=True, typescript=True)` registers Monty Python (`python`/`python3`)
and ZapCode TypeScript (`ts`/`node`) in the same VFS as Bash. Both flags default
to `False`. They configure the live interpreter and LLM metadata together, and
are replayed on `reset()`. The Python crate enables the existing TypeScript
core feature in native and Emscripten builds; command registration stays opt-in.
`Bash` keeps its existing Python configuration.

Python's existing host permission remains separate from registration:
construct with `env={"BASHKIT_ALLOW_INPROCESS_PYTHON": "1"}`. The builtin
captures this grant at construction, so an agent's `export` cannot enable it.
Do not infer permission from untrusted script contents. See
[Python Builtin](python-builtin.md) and [ZapCode Runtime](zapcode-runtime.md).

LangChain's `BashkitTool` / `create_bash_tool`, deepagents' `BashkitMiddleware` /
`create_bash_middleware`, and `BashkitBackend` forward `python`, `typescript`,
and `env` to their native tool. Backend-created middleware shares the configured
tool and VFS. Initial env forwarding lets the host grant Python explicitly
without weakening the runtime gate.

Snapshot factories (`from_snapshot` / `from_snapshot_keyed`) accept the same
runtime flags and host env. Hosts supply them when reconstructing a tool;
snapshot bytes do not grant permission to enable Python. In-place restore and
reset retain the host's runtime selection.

Regression evidence: `crates/bashkit-python/tests/test_tool_runtimes.py` covers
defaults, independent runtime selection, metadata, sync/async execution, shared
VFS, reset, keyed/unkeyed snapshot factories, framework forwarding, invalid
programs, and rejection of script-only Python opt-in.

### Embedded runtime host functions

`BashTool` exposes Python `external_functions` / `external_handler`, matching
`Bash`; handlers are async `(name, args, kwargs)` callables. TypeScript has
separate `typescript_external_functions` / `typescript_external_handler`
kwargs on `BashTool`, with an async `(name, args)` callable. Both are forwarded
by LangChain and deepagents. Language selection and the explicit `BashTool`
Python host-env grant remain mandatory; handlers grant no other runtime access.
Names appear in tool metadata; hosts supply their function signatures.

Decision: use the existing `PyCallbackEngine` / per-execution
`PyCallbackSession` via `RuntimeCallContext` request extensions. Do not cache
caller event loops on handlers or derive them on a Tokio worker. Each async
execution schedules handlers on its caller's loop and captures that request's
ContextVars. Sync execution uses the instance's dedicated private-loop worker;
this includes FastAPI sync endpoints, ordinary threads and synchronous notebook
cells. A client owned by an outer loop requires async execution instead.
`Bash.execute_sync()` now supports external handlers through this same engine.
Deepagents provides native coroutine implementations for backend `aexecute`
and middleware `ainvoke` rather than offloading sync execution to a thread.

Same-instance calls serialize on the existing interpreter mutex. Reentry
protection uses a per-instance ContextVar set inside the handler task, not an
instance-wide activity counter: recursive interpreter/live-FS access fails
immediately, while unrelated requests can queue during callback I/O. Other
interpreter instances are usable from a handler. Cancelling a callback session
marks it synchronously before forwarding cancellation through the native bridge.
Both buffered and streaming execution check that flag after acquiring the shell
mutex, so a cancelled queued request cannot begin script side effects while its
native cancellation wakeup is pending. Other sessions remain independent.
Private-loop work items carry
their own cancellation state/task handle so timeout/revocation cancels that
item even if it has not started; cancelling an old item cannot cancel the next
request on the shared loop. Worker teardown retains the interpreter-exit guard.

Native result delivery has an additional exit boundary in `src/async_bridge.rs`.
Decision: retain upstream `pyo3-async-runtimes` cancellation, panic and task-local
behavior through its generic runtime interface, while enrolling blocking result
deliveries under a shared shutdown lock. A weak registry first closes private
loop creation, cancels outstanding private callbacks and joins their workers
while Python is still attachable. This also drains timeout-cancellation cleanup;
setting only an exit flag leaves a worker already inside Python racing finalization.
The atexit hook then closes result-delivery enrollment and
releases the GIL until enrolled deliveries finish releasing Python references.
Without that drain, `call_soon_threadsafe` can wake Python, which exits while
the native worker still owns attached references; CPython can destroy its thread
state before tuple cleanup, causing a native crash. Future deliveries skip Python
after the boundary closes. The deterministic subprocess test delays a native
delivery after publishing the result and checks Bash, BashTool and ScriptedTool;
parallel subprocess churn also exercises immediate exit after runtime callbacks.
Another deterministic test delays cancellation cleanup in private callbacks for
custom builtins, embedded Python and TypeScript, then immediately exits.

A per-instance count of active handlers on each Python thread protects sync
entry points too: another task on the handler's own loop cannot synchronously
wait for the interpreter lock without deadlocking that loop. Those sync calls
fail fast with an async/to-thread hint; async calls and sync callers on other
threads still queue. The subprocess regression bounds a native deadlock and
checks retained live-FS handles as well as execution and snapshot APIs. Guards
skip Python lookups entirely when no external handlers are configured.

Callbacks are host configuration: preserved on reset/in-place restore, supplied
again to keyed/unkeyed snapshot factories, never serialized. Both native
external-handler kwargs fail loudly on Pyodide. TypeScript conversions accept
JSON-shaped values, map undefined to None, cap nesting at 64, and reject function
inputs, non-finite numbers, unsupported outputs and integers outside i64.
Use a final expression or VFS output after TypeScript host calls; upstream
ZapCode does not retain post-suspension console.log output.

Regression evidence: `tests/test_external_functions.py` shares execution-model
coverage across Python/TypeScript (sync/async, multiple threads, successive loops,
Selector/uvloop, callback cancellation, timeout/reuse, reentry, mixed builtins,
FastAPI and framework invocation). `tests/test_typescript_external_functions.py`
covers direct TypeScript dispatch/conversion and both languages sharing the VFS.
`examples/runtime_external_functions.ipynb` runs in a real Jupyter kernel in CI.
Python CI uses a thread-based per-test watchdog so a native deadlock cannot
leave a runner hanging indefinitely. It runs the callback suite on macOS/Windows in addition to the Linux
3.9–3.14 matrix; the Windows default tests Proactor and explicit Selector loops.

### Runtime host mutations

`set_env()` and runtime `mount()` are recorded and replayed on every rebuild,
so `reset()` keeps them the way it already keeps `custom_builtins` and
constructor `files` (TM-ISO-025). This makes a reusable setup bundle,
mount + env + builtin, the extension shape from #2291, survive a reset whole
rather than half-applied. `unmount()` retracts its record, so a rebuild does
not resurrect a removed mount. Env a *script* exported is not recorded: `reset()`
still discards it, and only host-set values come back.

Same contract in the NAPI binding (`setEnv`), backed by `Bash::set_env()` in
the core. Absent from the CLI, browser wasm, and C ABI, which expose no
post-construction host API, see
[Public Capability Parity](../status/capability-parity.md).

Snapshot/restore on both `Bash` and `BashTool` (mirrors Node bindings):
`snapshot()` / `snapshot(exclude_filesystem=True)` / `from_snapshot(blob)` /
`restore_snapshot(blob)`, plus keyed variants `snapshot_keyed(secret)` /
`from_snapshot_keyed(blob, secret)` / `restore_snapshot_keyed(blob, secret)`
(secret ≥ 32 bytes). Unkeyed snapshot bytes are for local checkpoints and
accidental-corruption detection only; callers loading snapshots from uploads,
shared storage, or network transport must use the keyed variants so forged
state is rejected before restore.

### Network configuration

Outbound HTTP (`curl`, `wget`, `http`) is gated behind `NetworkAllowlist` in
the Rust core and exposed via the optional `network=` kwarg on `Bash(...)`
and `BashTool(...)`: a dict with `allow` (URL patterns) **or**
`allow_all=True`, plus optional `block_private_ips` (default `True`).
Omitting `network=` leaves the network disabled (secure default).

The `bashkit-python` crate compiles the core with `http_client`, so `reqwest`
is available unconditionally, gating happens at the Python API layer.
Configuration is persisted on the wrapper struct so `reset()` and
`from_snapshot(...)` rebuild with the same allowlist.

Phase 2 (#1348) adds per-host credential injection via two optional keys on
the same dict:

- `credentials`: injection rules, `pattern`, `kind` (`"bearer"`, `"header"`,
  `"headers"`), and payload (`token`, `name`/`value`, or `(name, value)`
  pairs). The script never sees the secret; the runtime adds headers
  transparently after the allowlist check.
- `credential_placeholders`: rules adding an `env` key (env-var name visible
  to scripts). The runtime sets the env var to a random
  `bk_placeholder_<hex>` token and substitutes the real credential on the
  wire only for requests matching `pattern`.

Credentials and placeholders are preserved across `reset()` and
`from_snapshot(...)`. Each rebuild generates a fresh placeholder string, so
scripts must re-read placeholder env vars after every reset/restore.

Request callbacks (`http_transport`, `before_http`, `after_http`) and bot-auth
ship in follow-up phases.

### ShellState

`Bash.shell_state()` / `BashTool.shell_state()` return a read-only
inspection view (not a full Rust `ShellState` mirror) for prompt rendering:
`cwd`, `env`, `variables`, `arrays`, `assoc_arrays`, `last_exit_code`,
`aliases`, `traps`. Transient fields follow Rust-core semantics:
`last_exit_code` and `traps` are captured on the state object, but the next
top-level execute clears them before running the new command.

### ExecResult

`stdout`, `stderr`, `exit_code`, `error`, `success` (`exit_code == 0`), `to_dict()`.

### create_langchain_tool_spec()

Returns dict with `name`, `description`, `args_schema` for LangChain.

### custom_builtins and Async Callbacks

`Bash` and `BashTool` accept `custom_builtins={"name": callback}`, callback =
`Callable[[BuiltinContext], str | BuiltinResult | Awaitable[str | BuiltinResult]]`.
`BuiltinResult` carries explicit `stdout`, `stderr`, `exit_code`.

`BuiltinContext` exposes `name`, `argv`, `stdin`, `env`, `cwd`, and `fs`, a
`FileSystem` handle to the interpreter's *live* VFS (same API as
`Bash.fs()`): reads see files created by earlier commands, writes are visible
to later ones. It wraps the same `Arc<dyn FileSystem>` the interpreter uses
(mirroring how the embedded `python3`/Monty builtin receives `ctx.fs`) and
operates without the interpreter lock. Because a custom builtin runs inside
`execute_sync`'s current-thread `block_on`, `PyFileSystem::with_fs` detects
the active runtime (`Handle::try_current`) and dispatches `ctx.fs` ops on a
throwaway worker thread to avoid a nested-runtime panic; each op spawns a
short-lived thread + runtime, so batching fs work in a callback beats many
small ops in a tight loop. This is distinct from, and safe unlike, calling
back into the owning instance's `Bash.fs()` / `Bash.read_file()`, which is
unsupported re-entrancy: it re-enters the interpreter's runtime and panics
with a nested-runtime error (not a deadlock, and not caught by the
`external_handler` reentry guard, which does not fire for custom builtins).
A callback may retain `ctx.fs` beyond the invocation: the handle stays valid
after the `Bash` drops and keeps the underlying VFS and its tokio runtime
alive until released, stashing it extends resource lifetime past `del bash`
(see teardown determinism below).

**Sync callbacks** are called directly under the session's captured
`contextvars` snapshot.

**Async callbacks** are driven to completion by one of three mechanisms:

| Calling context | Mechanism |
|---|---|
| `await execute()` | Callback scheduled as a `Task` on the **caller's running loop** |
| `execute_sync()`, no running loop | **Private event loop** shared across calls on the same `Bash` instance |
| `execute_sync()`, running loop present (e.g. Jupyter / IPython) | **Background daemon thread** with its own fresh event loop |

The background-thread path is selected via `asyncio.get_running_loop()`
succeeding; the awaitable's `run_until_complete` is wrapped in
`context.run()` so ContextVars propagate despite the thread switch. The
helper is cached on the `PyPrivateAsyncLoop` to avoid repeated module
compilation.

**Teardown determinism** (TM-PY-030): while the interpreter is alive,
dropping the last reference to a `Bash`/`BashTool`/`ScriptedTool`
deterministically releases everything it owns *before* the drop returns,
in-flight private-loop callbacks are cancelled cooperatively (each runs as an
`asyncio.Task`; cancellation raises `asyncio.CancelledError` at the next
await point), the private-loop worker thread is joined and closes its event
loop (freeing fds), and the tokio runtime's blocking pool is joined. All
joins release the GIL first, so teardown cannot deadlock against callbacks
that need to attach. Callbacks that block without awaiting (e.g. `time.sleep`
inside `async def`) cannot be cancelled mid-section; teardown waits for the
current section to reach an await point or return. At interpreter exit
(boundary: an `atexit` handler registered at module import), teardown goes
hands-off, native threads must not touch a finalizing CPython, and the OS
reclaims resources. The same hands-off path applies when the last runtime
handle is dropped *inside* a tokio context (a `Bash` dropped while
`await execute()` is in flight finishes on a runtime worker thread): a
blocking runtime join there would panic, so the drop falls back to
`shutdown_background()` instead of the deterministic join.

The regression test asserts **no thread growth**, not an exact process-wide
thread count. tokio reaps idle blocking-pool threads on a 10 s timer, so a test
session that ran async work earlier sheds an unrelated thread mid-loop
(measured: 6 threads holding, dropping to 5 at exactly t=10.0 s). Exact
equality read that shrink as a failure though nothing had leaked. Do not
re-tighten it, the process-wide count is not a single tool's to own, and a
genuine leak compounds across the churn iterations and still trips the check.

**ContextVar propagation**: ContextVars set before `execute()` /
`execute_sync()` are captured at call time and replayed inside each callback
invocation regardless of mechanism.

## Optional Dependencies

`bashkit[langchain]`, `bashkit[deepagents]`, `bashkit[pydantic-ai]`,
`bashkit[dev]` (pytest, pytest-asyncio, pytest-timeout).

## Deep Agents backend

The optional `deepagents` extra requires Deep Agents 0.7.13 or later and Python
3.11+ (the upstream framework minimum); the base package remains Python 3.9+.
CI installs the extra on Python 3.11–3.14 so protocol tests cannot silently skip. `BashkitBackend`
implements its structured `read`, `ls`, `glob`, `grep`, `write`, `edit`, and
`delete` results; obsolete `ls_info`, `glob_info`, and `grep_raw` are removed.
`execute` propagates either native stream's truncation flag. Timeout policy
remains configured on construction (`timeout_seconds`); per-call timeout
support is not advertised to the framework.

File operations use the live native VFS directly, avoiding shell output caps,
newline insertion, option injection, and colon-delimited path parsing. They
never access the host filesystem. Transfers preserve arbitrary bytes; writes
refuse existing files, edits report replacement counts, and uploads replace
files. Relative paths resolve against the shared shell's current directory;
discovery always returns absolute paths. Reads return raw selected lines with
pagination metadata, leaving line-number formatting to the framework.

Grep uses literal substring matching and the framework's shared include-glob
matcher: basename globs at any depth, rooted/path globs relative to the search
root, recursive `**`, braces, and character classes. Traversal patterns are
errors. A total `max_count` reports truncation only when a match is omitted.
When callers omit it, Bashkit enforces a 1,000-match default plus a 100 KB
matched-text ceiling. Direct reads and recursive discovery share per-operation
deadline, 10,000-file, and 10 MB traversal budgets. These are per-operation by
design — nothing is retained once an operation returns, so a session-lifetime
byte counter would only drain and eventually fail a healthy long-lived session.
Exhausted searches return partial results with `truncated=True`.
Line scans stream through file content instead of materializing `splitlines()`.
Recursive discovery skips symlink entries to avoid cyclic walks. Async read,
glob, and grep dispatch cooperatively signal cancellation to their worker,
sharing the same VFS with Bashkit middleware.
`test_deepagents_backend.py` exercises the installed framework protocol and
native extension, including stream caps, exact transfers, malicious names,
search filters, and shared async state.

## CI

`.github/workflows/python.yml`, on push to main and PRs (path-filtered).
Jobs: lint (ruff check + format), test (installs the single abi3 wheel from
build-wheel + pytest on 3.9–3.14; only the non-abi3 random-fs fixture is built
per version), examples (wheel + `crates/bashkit-python/examples/` +
`examples/*.ipynb` via `jupyter nbconvert --execute`, cell error fails CI),
build-wheel (maturin + twine check, produces the shared abi3 wheel artifact),
python-check (branch-protection gate).

The version matrix runs on a pinned `ubuntu-24.04`, not `ubuntu-latest`, in
both `python.yml`'s `test` job and `publish-python.yml`'s `test-builds`.
Ubuntu 26.04 ships no hosted 3.9 build and `setup-python` has no fallback, so
the 3.9 leg failed outright ("The version '3.9' with architecture 'x64' was
not found for Ubuntu 26.04") as soon as `ubuntu-latest` began rolling onto
26.04. The package claims `>=3.9` through one cp39-abi3 wheel, so the matrix
needs an image that can host the whole declared range; shrinking the matrix
would stop testing a version the wheel still promises. Unpin when 3.9 support
is dropped. The two matrices are kept in step by `just check-workflow-parity`,
which compares version coverage and ignores runner labels.

## Linting

ruff (config in `crates/bashkit-python/pyproject.toml`; rules E/F/W/I/UP,
target 3.9, line-length 120). Commands in `AGENTS.md` § Python.

## Local Development

```bash
cd crates/bashkit-python
pip install maturin && maturin develop      # --release for optimized
pip install pytest pytest-asyncio && pytest tests/ -v
```

## abi3 (stable ABI)

The extension is built against CPython's [limited API / stable
ABI](https://docs.python.org/3/c-api/stable.html) via PyO3's `abi3-py39`
feature (in the workspace `pyo3` dependency). One `cp39-abi3` wheel per
platform runs on **every Python ≥ 3.9**, including versions released after the
wheel was built, no per-minor-version rebuild.

Why: PyPI keeps every historical release forever, and each native wheel is a
~12 MB static binary (embedded jq / Monty / SQLite / SSH / TLS). Building one
wheel per `{Python minor × platform}` was 6 × 7 = 42 native wheels (~500 MB)
*per release*, growing unbounded across releases. abi3 collapses the Python
axis to one, cutting per-release native wheels 6× (42 → 7, ~500 MB → ~85 MB)
and shrinking the build matrix by the same factor. The perf cost of the limited
API is negligible here because hot paths are in Rust, not Python dispatch.

Because the build now emits one wheel instead of one-per-version, the
`test-builds` job in `publish-python.yml` installs and smoke-tests that single
wheel across the full 3.9–3.14 range on every OS (3 × 6 = 18 jobs), the check
that the abi3 wheel really runs everywhere it claims, not just on the build
interpreter.

Limited-API constraints observed in `crates/bashkit-python/src/lib.rs`:

- **No `PyGILState_Check`** (not in the stable ABI, and pyo3 documents it as
  unreliable). Teardown's `join_without_gil` probes attachment via the public
  `Python::try_attach` + `py.detach` instead.
- **No `PyString::to_str`** on the `&self`-only path (needs the non-limited API
  below 3.10). Lazy file providers use `to_cow()`, which is in the stable ABI.

The emscripten/Pyodide wheel is unaffected, it already ships a single Python
version on a separate toolchain (see [Emscripten Wheels](emscripten-wheels.md)).

## Design Decisions

- **No PGO**: Profile-guided optimization adds build complexity for minimal gain.
  Bashkit is a thin PyO3 extension, hot paths are in Rust, not Python dispatch.
  Can revisit if profiling shows benefit.
- **No exotic architectures**: armv7, ppc64le, s390x, i686 omitted. Target audience
  is AI agent developers on standard server/desktop platforms.
- **Dynamic version**: Eliminates version drift between Rust and Python packages.
- **Trusted publishing**: No secrets to rotate. OIDC tokens are scoped per-workflow.
