# Get started in Pyodide & JupyterLite

For **Python running in the browser** (Pyodide, JupyterLite), Bashkit ships an
Emscripten (`wasm32-unknown-emscripten`) wheel — a reduced-feature variant of
the native PyPI package that needs no native toolchain.

## Install

```python
import micropip
await micropip.install("bashkit")   # pulls the Pyodide-ABI wheel
```

If your Pyodide host doesn't resolve the ABI wheel automatically, pass the wheel
URL to `micropip.install(...)`.

## First script

```python
from bashkit import Bash

bash = Bash(python=True)
print(bash.execute_sync("echo hello && echo 1 | jq .").stdout)
```

## Feature surface

The Pyodide wheel is the in-VFS shell plus embedded `jq` and Monty `python`,
driven through blocking `execute_sync()`.

**Present on both native and wasm:** `Bash` / `BashTool` / `ScriptedTool`,
`execute_sync()`, Monty `python=True`, `jq`, and sync/async custom-builtin
callbacks.

**Absent on wasm:** the async `execute()` / `execute_or_throw()` methods,
`FileSystem.real()`, and capsule `to/from_capsule`. Gated-off configuration
kwargs — `network=`, `sqlite=True`, `mounts=`, `external_handler=` — **fail
loudly** with `RuntimeError` at construction rather than silently no-op, so you
learn immediately that the WASM build can't do it.

## Next steps

- [Get started in Python](start-python.md) — the full-featured native PyPI wheel.
- [Get started in the browser](start-browser.md) — the JavaScript WASM package.
- [Python builtin](python.md) — the embedded Monty runtime in depth.
