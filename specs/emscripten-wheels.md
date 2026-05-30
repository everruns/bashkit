# Emscripten / Pyodide Wheels

## Status

Implemented (reduced feature set). CI build + smoke test green; PyPI publish wired.

## Abstract

Bashkit ships an additional Python wheel targeting `wasm32-unknown-emscripten`
(the Pyodide ABI), so `bashkit` can run **in the browser, JupyterLite, and other
WASM hosts** with no native toolchain. This is a *reduced-feature* variant of the
native package: the in-VFS shell plus the embedded `jq` and Monty `python`
interpreters, driven through the blocking `execute_sync()` API.

Approach mirrors Pydantic's recipe for building Emscripten wheels for a
Rust + maturin + PyO3 package
(<https://pydantic.dev/articles/emscripten-wheels-pydantic>).

## Why a separate, reduced wheel

Pyodide runs single-threaded with no OS sockets and no host filesystem. Several
deps the native wheel relies on contain hard `compile_error!`s or missing modules
on wasm:

| Capability | Native crate/feature | Why it can't build on wasm |
|---|---|---|
| Outbound HTTP (`curl`/`wget`/`http`) | `http_client` → `reqwest` → `mio` | `mio`: "This wasm target is unsupported by mio. Disable the net feature." |
| SQLite (`sqlite`/`sqlite3`) | `sqlite` → `turso_core` + `tokio/rt-multi-thread` | tokio: "Only features sync,macros,io-util,rt,time are supported on wasm." |
| Host directory mounts | `realfs` → `tokio::fs` | `tokio::fs` absent on wasm |
| Capsule FS interop | `interop` → `tokio/rt-multi-thread` | multi-thread runtime unsupported |
| Async `execute()` bridge | `pyo3-async-runtimes` (`tokio-runtime`) | hard-pulls `rt-multi-thread` + tokio `net` (mio) |

The core `bashkit` crate was already wasm-aware (it gates `rt-multi-thread`/`fs`
behind `cfg(not(target_arch = "wasm32"))`). The work is confined to
`crates/bashkit-python`.

## Feature matrix: native vs wasm

| Surface | Native wheel | Pyodide wheel |
|---|---|---|
| `Bash` / `BashTool` / `ScriptedTool` | ✅ | ✅ |
| `execute_sync()` / `execute_sync_or_throw()` | ✅ | ✅ |
| `async execute()` / `execute_or_throw()` | ✅ | ❌ (method absent) |
| `python=True` (Monty) | ✅ | ✅ |
| `jq` builtin | ✅ | ✅ |
| Custom builtins — sync callbacks | ✅ | ✅ |
| Custom builtins — async callbacks | ✅ (caller-loop or private-loop) | ✅ (private-loop fallback only) |
| `network=` (allowlist / credentials) | ✅ | ❌ (raises at construction) |
| `sqlite=True` | ✅ | ❌ (raises at construction) |
| `mounts=` (host dirs) | ✅ | ❌ (raises at construction) |
| `external_handler=` | ✅ | ❌ (raises at construction) |
| `FileSystem.real()` / capsule `to/from_capsule` | ✅ | ❌ (method absent) |

Unavailable *configuration* kwargs **fail loudly** with a `RuntimeError` rather
than silently no-op, so callers learn immediately the WASM build can't do it.

## Implementation

All gating lives in `crates/bashkit-python`:

### `Cargo.toml`

Dependencies are split per target:

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

- `#[cfg(not(target_arch = "wasm32"))]` on: the async `execute*()` `#[pymethods]`,
  `pyo3-async-runtimes` imports, `make_external_handler`, the caller-loop callback
  machinery (`PyCancellableLoopFuture`, `call_soon_threadsafe_with_context`,
  `cancel_python_task`), network/credential parsing + `apply`, `FileSystem.real()`
  and the capsule bridge.
- `type CallerLoopLocals` aliases `TaskLocals` (native) / `std::convert::Infallible`
  (wasm). `caller_loop_locals` is always `None` on wasm, so the caller-loop branches
  in `capture_callback_state` and `call_python_callback_async` are statically dead.
- Construction-time guards raise `RuntimeError` for `network=`, `sqlite=True`,
  `mounts=`, and `external_handler=` on wasm.
- A wasm-scoped `#![cfg_attr(target_arch = "wasm32", allow(dead_code, unused_imports))]`
  silences lints from helpers referenced only by native-gated paths.

Decision comments are inline at each gate; this spec is the index.

## Building locally

### Toolchain matrix (verified)

| Component | Version | Why |
|---|---|---|
| Python (host for build) | **3.13** | Selects pyodide-build's modern config |
| pyodide-build | 0.34.x (latest) | → Pyodide 0.29.x ABI |
| Emscripten | **4.0.9** | Managed by pyodide-build; binaryen knows modern LLVM wasm features |
| Rust | **nightly** (≥1.95-equivalent) | `-Z link-native-libraries=no`; satisfies monty's MSRV + edition 2024 |

The Emscripten/ABI versions are dictated by the installed `pyodide-build` for the
host Python. **Python 3.11/3.12 pin pyodide-build ≤0.25.1 → Emscripten 3.1.x**,
whose binaryen is too old for modern LLVM (see "version triangle" below) — use
Python 3.13.

```bash
# 1. nightly Rust (Pyodide passes -Z link-native-libraries=no)
rustup toolchain install nightly --target wasm32-unknown-emscripten

# 2. pyodide-build (under Python 3.13) — manages its own matching emsdk
python3.13 -m pip install pyodide-build
pyodide xbuildenv install            # downloads Emscripten 4.0.9 + ABI

# 3. build (no RUSTFLAGS hacks, no separate emsdk needed)
cd crates/bashkit-python
RUSTUP_TOOLCHAIN=nightly pyodide build

# 4. smoke test — run from a scratch dir so the crate's own bashkit/ source
#    package doesn't shadow the installed extension module
pyodide venv .venv-pyodide
.venv-pyodide/bin/pip install dist/*.whl
( cd "$(mktemp -d)" && /abs/path/.venv-pyodide/bin/python -c \
  "from bashkit import Bash; print(Bash(python=True).execute_sync('echo hi | jq -R .').stdout)" )
```

For a fast Rust-only type check without the full wheel build:

```bash
PYO3_CROSS_PYTHON_VERSION=3.13 \
  cargo check -p bashkit-python --target wasm32-unknown-emscripten
```

### The version triangle (the hard part)

The build sits at the intersection of three independently-versioned toolchains
that must agree on the wasm feature set:

1. **Rust/LLVM** emits a `target_features` section. Modern LLVM (19+, required by
   `edition 2024` and monty's `rustc 1.95` MSRV) marks features like
   `bulk-memory-opt` and `call-indirect-overlong`.
2. **Emscripten/binaryen** runs `wasm-opt --detect-features`, reading that section
   and passing `--enable-<feature>` for each. Binaryen must recognize every name
   or the link fails: `Unknown option '--enable-bulk-memory-opt'`. Binaryen ≥
   the one in **Emscripten 4.0.9** knows them; the one in Emscripten 3.1.x does not.
3. **Pyodide runtime** must support the **exception-handling ABI** the wheel uses.
   Modern Rust emits the new wasm-EH `__cpp_exception` *tag*; older Pyodide
   (0.25/Emscripten 3.1.46) only supports legacy EH, giving a load-time
   `LinkError: tag import requires a WebAssembly.Tag`.

Older Emscripten (3.1.x) fails (2) and (3) against modern Rust, and the
edition-2024 + monty MSRV floor forbids dropping to an old-enough nightly. The
resolution is to move *up*: Python 3.13 → pyodide-build's Emscripten 4.0.9 config,
which matches modern nightly Rust on both feature naming and the wasm-EH ABI. No
`-O1` / wasm-opt-skip or target-feature disabling is needed.

## CI

`.github/workflows/python.yml` adds a `wasm` job: Python 3.13 + nightly Rust +
`pyodide build`, then imports the wheel in a `pyodide venv` (from a scratch dir) to
smoke-test `execute_sync`. Wired into the `python-check` gate.

`.github/workflows/publish-python.yml` adds a `build-emscripten` job feeding the
`inspect` → `publish` pipeline so the Pyodide wheel ships to PyPI alongside the
native wheels.

## Gotchas (from the Pydantic article, confirmed here)

- **Nightly Rust required**: Pyodide injects `-Z link-native-libraries=no`.
- **No threads / no asyncio loop bridging**: async `execute()` is native-only;
  async custom-builtin callbacks use the private-loop fallback on wasm.
- **No sockets / no host FS**: network, sqlite, realfs, interop all gated off.
- **Version triangle**: Rust/LLVM ↔ Emscripten/binaryen ↔ Pyodide-runtime EH ABI
  must agree (see above). Pin via the host Python version (3.13) + a recent
  nightly; bump deliberately.
- **Source shadowing in the smoke test**: run the import test from a scratch
  directory, or the crate's `bashkit/` source package shadows the installed
  extension (`ModuleNotFoundError: No module named 'bashkit._bashkit'`).

## See also

- `specs/python-package.md` — native wheel matrix, PyPI publishing, public API.
- `specs/architecture.md` — core interpreter, wasm-aware tokio gating.
