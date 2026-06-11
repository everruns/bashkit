# Emscripten / Pyodide Wheels

## Status

Implemented (reduced feature set). CI build + smoke test green; PyPI publish wired.

## Abstract

Bashkit ships an additional Python wheel targeting `wasm32-unknown-emscripten`
(the Pyodide ABI), so `bashkit` runs **in the browser, JupyterLite, and other
WASM hosts** with no native toolchain. Reduced-feature variant of the native
package: in-VFS shell plus embedded `jq` and Monty `python`, driven through
blocking `execute_sync()`. Approach mirrors Pydantic's Emscripten-wheel
recipe (<https://pydantic.dev/articles/emscripten-wheels-pydantic>).

## Why a separate, reduced wheel

Pyodide runs single-threaded with no OS sockets and no host filesystem.
Several native-wheel deps contain hard `compile_error!`s or missing modules
on wasm:

- `http_client` → `reqwest` → `mio`: wasm target unsupported by mio's net feature.
- `sqlite` → `turso_core` + `tokio/rt-multi-thread`: tokio supports only sync/macros/io-util/rt/time on wasm.
- `realfs` → `tokio::fs`: absent on wasm.
- `interop` (capsule FS) → `tokio/rt-multi-thread`: unsupported.
- async `execute()` bridge → `pyo3-async-runtimes` (tokio-runtime): hard-pulls `rt-multi-thread` + tokio `net` (mio).

The core `bashkit` crate was already wasm-aware (gates
`rt-multi-thread`/`fs` behind `cfg(not(target_arch = "wasm32"))`); the work
is confined to `crates/bashkit-python`.

## Feature surface: native vs wasm

Present on both: `Bash`/`BashTool`/`ScriptedTool`, `execute_sync()` /
`execute_sync_or_throw()`, Monty `python=True`, `jq`, sync custom-builtin
callbacks, async custom-builtin callbacks (wasm: private-loop fallback only —
no caller-loop).

Absent on wasm: async `execute()` / `execute_or_throw()` (methods absent),
`FileSystem.real()` / capsule `to/from_capsule` (methods absent). Gated-off
*configuration* kwargs — `network=`, `sqlite=True`, `mounts=`,
`external_handler=` — **fail loudly** with `RuntimeError` at construction
rather than silently no-op, so callers learn immediately the WASM build
can't do it.

## Implementation

All gating lives in `crates/bashkit-python`:

### `Cargo.toml`

Per-target dependency split:

```toml
[target.'cfg(not(target_arch = "wasm32"))'.dependencies]
bashkit = { path = "../bashkit", features = ["scripted_tool","python","realfs","jq","interop","http_client","sqlite"] }
tokio = { workspace = true, features = ["rt-multi-thread"] }
pyo3-async-runtimes = { workspace = true }

[target.'cfg(target_arch = "wasm32")'.dependencies]
bashkit = { path = "../bashkit", features = ["scripted_tool","python","jq"] }
tokio = { workspace = true }   # wasm-safe base features only
```

### `src/lib.rs`

- `#[cfg(not(target_arch = "wasm32"))]` on: async `execute*()` `#[pymethods]`,
  `pyo3-async-runtimes` imports, `make_external_handler`, caller-loop callback
  machinery (`PyCancellableLoopFuture`, `call_soon_threadsafe_with_context`,
  `cancel_python_task`), network/credential parsing + `apply`,
  `FileSystem.real()`, capsule bridge.
- `type CallerLoopLocals` aliases `TaskLocals` (native) /
  `std::convert::Infallible` (wasm); `caller_loop_locals` is always `None`
  on wasm, so caller-loop branches are statically dead.
- Construction-time `RuntimeError` guards for the four gated kwargs.
- Wasm-scoped `#![cfg_attr(target_arch = "wasm32", allow(dead_code, unused_imports))]`
  silences lints from native-only helpers.

Decision comments are inline at each gate; this spec is the index.

## Toolchain pins

Versions are pinned in CI via job-level `RUST_NIGHTLY` /
`PYODIDE_BUILD_VERSION` env vars in `.github/workflows/python.yml` (`wasm`
job) and `.github/workflows/publish-python.yml` (`build-emscripten` job) —
those are the source of truth. Host Python **3.13** selects pyodide-build's
modern config (pyodide-build → Pyodide 0.29.x / Emscripten 4.0.9 ABI;
Emscripten is managed by pyodide-build). Nightly Rust is required because
Pyodide injects `-Z link-native-libraries=no`, and the nightly must satisfy
monty's MSRV + edition 2024.

**Invariant: bump the trio (host Python / pyodide-build / Rust nightly)
together** — they must agree on the wasm feature set and
exception-handling ABI (version triangle below) — and re-verify the wheel
*imports* (not just builds) after any bump. Python 3.11/3.12 pin
pyodide-build ≤0.25.1 → Emscripten 3.1.x, which fails against modern Rust;
use 3.13.

## Building locally

Use the same pins as CI:

```bash
rustup toolchain install <RUST_NIGHTLY> --target wasm32-unknown-emscripten
python3.13 -m pip install "pyodide-build==<PYODIDE_BUILD_VERSION>"
pyodide xbuildenv install                 # downloads matching Emscripten + ABI
cd crates/bashkit-python
RUSTUP_TOOLCHAIN=<RUST_NIGHTLY> pyodide build
pyodide venv .venv-pyodide && .venv-pyodide/bin/pip install dist/*.whl
# Smoke test from a scratch dir — the crate's own bashkit/ source package
# otherwise shadows the installed extension (ModuleNotFoundError: bashkit._bashkit)
( cd "$(mktemp -d)" && /abs/path/.venv-pyodide/bin/python -c \
  "from bashkit import Bash; print(Bash(python=True).execute_sync('echo hi | jq -R .').stdout)" )
```

Fast Rust-only type check: `PYO3_CROSS_PYTHON_VERSION=3.13 cargo check -p
bashkit-python --target wasm32-unknown-emscripten`.

### Browser / JupyterLite verification

CI's `pyodide venv` smoke test installs via `pip`; the actual end-user flow
installs via `micropip` into freshly loaded Pyodide. Verifying that path is
a deliberate one-off manual check, **not** a CI job — it pulls `micropip`
from the jsdelivr CDN (network flakiness), and the venv test already
exercises the wasm runtime + EH ABI. Recipe: `npm install pyodide@<ABI
version>`, then a Node script doing `loadPyodide()` →
`micropip.install(wheel file URL)` → `import bashkit` → `execute_sync(...)`.
Confirmed working: `Bash(python=True).execute_sync('echo hello && echo 1 |
jq .')` → `'hello\n1\n'`, and `Bash(sqlite=True)` raises `RuntimeError`.

## The version triangle (the hard part)

Three independently-versioned toolchains must agree on the wasm feature set:

1. **Rust/LLVM** emits a `target_features` section; modern LLVM (19+,
   required by edition 2024 and monty's MSRV) marks features like
   `bulk-memory-opt` and `call-indirect-overlong`.
2. **Emscripten/binaryen** runs `wasm-opt --detect-features` and passes
   `--enable-<feature>` for each; binaryen must recognize every name or the
   link fails (`Unknown option '--enable-bulk-memory-opt'`). Emscripten
   4.0.9's binaryen knows them; 3.1.x's does not.
3. **Pyodide runtime** must support the **exception-handling ABI** the wheel
   uses. Modern Rust emits the new wasm-EH `__cpp_exception` *tag*; older
   Pyodide (0.25 / Emscripten 3.1.46) only supports legacy EH → load-time
   `LinkError: tag import requires a WebAssembly.Tag`.

Old Emscripten fails (2) and (3) against modern Rust, and the edition-2024 +
monty MSRV floor forbids dropping to an old-enough nightly. Resolution is to
move *up*: Python 3.13 → pyodide-build's Emscripten 4.0.9 config, matching
modern nightly Rust on both feature naming and the wasm-EH ABI. No `-O1` /
wasm-opt-skip / target-feature disabling needed.

## CI

`.github/workflows/python.yml` `wasm` job: Python 3.13 + nightly Rust +
`pyodide build`, then imports the wheel in a `pyodide venv` (from a scratch
dir) to smoke-test `execute_sync`. Wired into the `python-check` gate.
`.github/workflows/publish-python.yml` `build-emscripten` job feeds the
`inspect` → `publish` pipeline so the Pyodide wheel ships to PyPI alongside
the native wheels.

## See also

- `specs/python-package.md` — native wheel matrix, PyPI publishing, public API.
- `specs/architecture.md` — core interpreter, wasm-aware tokio gating.
