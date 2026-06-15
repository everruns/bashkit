# Python Package

## Status

Implemented

## Abstract

Bashkit ships a Python package as pre-built binary wheels on PyPI. Users install with
`pip install bashkit` and get a native extension — no Rust toolchain needed.

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
(`dynamic = ["version"]` in pyproject.toml) — no manual sync. Chain:
workspace `Cargo.toml` → `bashkit-python` `Cargo.toml` (inherits) → maturin → wheel metadata.

## Supported Platforms

Python 3.9–3.14 × 7 platforms ≈ 42 wheels: Linux x86_64/aarch64 (manylinux
glibc + musllinux_1_1), macOS x86_64/aarch64, Windows x86_64 MSVC. Exact
matrix and runners: `.github/workflows/publish-python.yml`.

In addition, a **reduced-feature Pyodide/Emscripten wheel**
(`wasm32-unknown-emscripten`) ships for browser / JupyterLite use — built and
published separately (different toolchain, single Python version, no
async/network/sqlite/realfs). See `specs/emscripten-wheels.md`.

## PyPI Publishing

`.github/workflows/publish-python.yml`, triggered on GitHub Release: sdist +
platform wheels → `twine check` → per-platform smoke test
(`BashTool().execute_sync('echo hello')`) → `uv publish` to PyPI.

Auth: PyPI trusted publishing (OIDC) — no API tokens. Prerequisites: GitHub
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
etc. Methods: `await execute(cmd)` / `execute_sync(cmd)` / `reset()`; direct
text-oriented VFS helpers (`read_file`, `write_file`, `append_file`, `mkdir`,
`exists`, `remove`, `stat`, `chmod`, `symlink`, `read_link`, `read_dir`,
`ls`, `glob`); LLM metadata (`name`, `short_description`, `description()`,
`help()`, `system_prompt()`, `input_schema()`, `output_schema()`, `version`).

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
is available unconditionally — gating happens at the Python API layer.
Configuration is persisted on the wrapper struct so `reset()` and
`from_snapshot(...)` rebuild with the same allowlist.

Phase 2 (#1348) adds per-host credential injection via two optional keys on
the same dict:

- `credentials`: injection rules — `pattern`, `kind` (`"bearer"`, `"header"`,
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

Request callbacks (`http_handler`, `before_http`, `after_http`) and bot-auth
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

`BuiltinContext` exposes `name`, `argv`, `stdin`, `env`, `cwd`, and `fs` — a
`FileSystem` handle to the interpreter's *live* VFS (same API as
`Bash.fs()`): reads see files created by earlier commands, writes are visible
to later ones. It wraps the same `Arc<dyn FileSystem>` the interpreter uses
(mirroring how the embedded `python3`/Monty builtin receives `ctx.fs`) and
operates without the interpreter lock. Because a custom builtin runs inside
`execute_sync`'s current-thread `block_on`, `PyFileSystem::with_fs` detects
the active runtime (`Handle::try_current`) and dispatches `ctx.fs` ops on a
throwaway worker thread to avoid a nested-runtime panic; each op spawns a
short-lived thread + runtime, so batching fs work in a callback beats many
small ops in a tight loop. This is distinct from — and safe unlike — calling
back into the owning instance's `Bash.fs()` / `Bash.read_file()`, which is
unsupported re-entrancy: it re-enters the interpreter's runtime and panics
with a nested-runtime error (not a deadlock, and not caught by the
`external_handler` reentry guard, which does not fire for custom builtins).
A callback may retain `ctx.fs` beyond the invocation: the handle stays valid
after the `Bash` drops and keeps the underlying VFS and its tokio runtime
alive until released — stashing it extends resource lifetime past `del bash`
(see teardown determinism below).

**Sync callbacks** are called directly under the session's captured
`contextvars` snapshot.

**Async callbacks** are driven to completion by one of three mechanisms:

| Calling context | Mechanism |
|---|---|
| `await execute()` | Callback scheduled as a `Task` on the **caller's running loop** |
| `execute_sync()` — no running loop | **Private event loop** shared across calls on the same `Bash` instance |
| `execute_sync()` — running loop present (e.g. Jupyter / IPython) | **Background daemon thread** with its own fresh event loop |

The background-thread path is selected via `asyncio.get_running_loop()`
succeeding; the awaitable's `run_until_complete` is wrapped in
`context.run()` so ContextVars propagate despite the thread switch. The
helper is cached on the `PyPrivateAsyncLoop` to avoid repeated module
compilation.

**Teardown determinism** (TM-PY-030): while the interpreter is alive,
dropping the last reference to a `Bash`/`BashTool`/`ScriptedTool`
deterministically releases everything it owns *before* the drop returns —
in-flight private-loop callbacks are cancelled cooperatively (each runs as an
`asyncio.Task`; cancellation raises `asyncio.CancelledError` at the next
await point), the private-loop worker thread is joined and closes its event
loop (freeing fds), and the tokio runtime's blocking pool is joined. All
joins release the GIL first, so teardown cannot deadlock against callbacks
that need to attach. Callbacks that block without awaiting (e.g. `time.sleep`
inside `async def`) cannot be cancelled mid-section; teardown waits for the
current section to reach an await point or return. At interpreter exit
(boundary: an `atexit` handler registered at module import), teardown goes
hands-off — native threads must not touch a finalizing CPython — and the OS
reclaims resources. The same hands-off path applies when the last runtime
handle is dropped *inside* a tokio context (a `Bash` dropped while
`await execute()` is in flight finishes on a runtime worker thread): a
blocking runtime join there would panic, so the drop falls back to
`shutdown_background()` instead of the deterministic join.

**ContextVar propagation**: ContextVars set before `execute()` /
`execute_sync()` are captured at call time and replayed inside each callback
invocation regardless of mechanism.

## Optional Dependencies

`bashkit[langchain]`, `bashkit[deepagents]`, `bashkit[pydantic-ai]`,
`bashkit[dev]` (pytest, pytest-asyncio).

## CI

`.github/workflows/python.yml` — on push to main and PRs (path-filtered).
Jobs: lint (ruff check + format), test (maturin develop + pytest on
3.9/3.12/3.13/3.14), examples (wheel + `crates/bashkit-python/examples/` +
`examples/*.ipynb` via `jupyter nbconvert --execute`, cell error fails CI),
build-wheel (maturin + twine check), python-check (branch-protection gate).

## Linting

ruff (config in `crates/bashkit-python/pyproject.toml`; rules E/F/W/I/UP,
target 3.9, line-length 120). Commands in `AGENTS.md` § Python.

## Local Development

```bash
cd crates/bashkit-python
pip install maturin && maturin develop      # --release for optimized
pip install pytest pytest-asyncio && pytest tests/ -v
```

## Design Decisions

- **No PGO**: Profile-guided optimization adds build complexity for minimal gain.
  Bashkit is a thin PyO3 extension — hot paths are in Rust, not Python dispatch.
  Can revisit if profiling shows benefit.
- **No exotic architectures**: armv7, ppc64le, s390x, i686 omitted. Target audience
  is AI agent developers on standard server/desktop platforms.
- **Dynamic version**: Eliminates version drift between Rust and Python packages.
- **Trusted publishing**: No secrets to rotate. OIDC tokens are scoped per-workflow.
