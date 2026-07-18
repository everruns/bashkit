# Targets & bindings

Bashkit is a single Rust core shipped as several distribution artifacts. The
shell semantics, virtual filesystem, and sandbox are the same everywhere — you
pick a package by **where your code runs** and **what host language you use**.

## At a glance

| Target | Install | Runs in | Notes |
|--------|---------|---------|-------|
| **Rust crate** | `cargo add bashkit` | Any Rust app | Core crate. Opt-in features for HTTP, git, Python, SQLite, real FS. |
| **CLI** | `cargo install bashkit-cli` | Terminal | Standalone binary, three modes. See [CLI](cli.md). |
| **Python (native)** | `pip install bashkit` | CPython 3.9+ | PyO3 wheel, native extension, no Rust toolchain. |
| **Node / Bun / Deno** | `npm i @everruns/bashkit` | Node ≥ 18, Bun, Deno | NAPI native addon — the fastest JS binding. |
| **WebAssembly** | `npm i @everruns/bashkit-wasm` | Browser, edge runtimes, Node/Bun/Deno | Single-threaded `wasm-bindgen` module. No `SharedArrayBuffer`, no COOP/COEP. |
| **Pyodide / JupyterLite** | `micropip.install("bashkit")` | Pyodide, JupyterLite, WASM Python hosts | Reduced-feature Emscripten wheel. |

**Which one?**

- Embedding in a **Rust, Python, or Node/Bun/Deno** service → the native crate,
  wheel, or NAPI addon. These share the full feature set — see
  [Embedding](embedding.md) for the API.
- Running in a **browser or edge/serverless runtime** (Cloudflare Workers,
  Vercel Edge, Deno Deploy), or anywhere a native addon can't load → the
  [WebAssembly package](#webassembly-browser--edge).
- Running inside **Pyodide or JupyterLite** (Python in the browser) → the
  [Pyodide wheel](#pyodide--emscripten-wheel).

## Rust

```bash
cargo add bashkit
```

The core crate. Heavier capabilities are opt-in features (`http_client`, `git`,
`typescript`, `sqlite`, `realfs`, `scripted_tool`). Full walkthrough —
persistent state, the sandbox builder, network allowlist — in
[Embedding › Rust](embedding.md#rust).

## Python (native)

```bash
pip install bashkit
```

```python
from bashkit import Bash

bash = Bash()
print(bash.execute_sync("echo 'Hello, World!'").stdout)
```

Pre-built binary wheels on PyPI — no Rust toolchain needed. See
[Embedding › Python](embedding.md#python).

## Node / Bun / Deno

```bash
npm i @everruns/bashkit
```

```typescript
import { Bash } from "@everruns/bashkit";

const bash = new Bash();
console.log(bash.executeSync('echo "Hello, World!"').stdout);
```

A NAPI native addon — the fastest JavaScript binding, for server-side runtimes
(Node ≥ 18, Bun, Deno). For browsers and edge runtimes, use the WebAssembly
package below instead. See [Embedding › TypeScript](embedding.md#typescript--javascript).

## WebAssembly (browser & edge)

```bash
npm i @everruns/bashkit-wasm
```

`@everruns/bashkit-wasm` is a slim, **single-threaded** `wasm-bindgen` module.
Unlike a WASI-threads build it needs **no `SharedArrayBuffer` and no
cross-origin isolation** (`COOP`/`COEP`) headers, so it drops into any web app —
including embedded and third-party iframe contexts where those headers can't be
set — and into edge/serverless runtimes that can't use threads.

Load the `.wasm` once with `initBashkit()` before constructing `Bash`:

```js
import { initBashkit, Bash } from "@everruns/bashkit-wasm";

await initBashkit();

const bash = new Bash();
const result = bash.executeSync('echo "Hello, browser!" | tr a-z A-Z');
console.log(result.stdout); // HELLO, BROWSER!
```

No bundler, straight from a CDN:

```html
<script type="module">
  import { initBashkit, Bash } from "https://esm.sh/@everruns/bashkit-wasm";
  await initBashkit();
  const bash = new Bash();
  document.body.textContent = bash.executeSync("seq 1 5 | paste -sd+ | bc").stdout;
</script>
```

Register JS callbacks as bash commands. Async callbacks (e.g. issuing a `fetch`)
are awaited by the async `execute()` API:

```js
const bash = new Bash({
  customBuiltins: {
    graphql: async (ctx) => {
      const res = await fetch("/graphql", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: ctx.stdin ?? "{}",
      });
      return await res.text();
    },
  },
});

const out = await bash.execute('echo "{ me { id } }" | graphql | jq .data');
console.log(out.stdout);
```

**Caveats**

- It's a `wasm-bindgen` module: it runs in any **JavaScript** host but **not** a
  non-JS/WASI wasm runtime (`wasmtime`, `wasmer`).
- Reduced feature set relative to the native bindings. Prefer
  [`@everruns/bashkit`](#node--bun--deno) when a native addon can load
  (server-side Node/Bun/Deno); reach for this package when it can't — browsers
  and edge runtimes.

A full interactive terminal built on this package lives in
[`examples/browser`](https://github.com/everruns/bashkit/tree/main/examples/browser)
(single `index.html` on Vite, no build step, no special headers).

## Pyodide / Emscripten wheel

For **Python running in the browser** (Pyodide, JupyterLite), Bashkit also
ships an Emscripten (`wasm32-unknown-emscripten`) wheel:

```python
import micropip
await micropip.install("bashkit")   # pulls the Pyodide-ABI wheel

from bashkit import Bash
print(Bash(python=True).execute_sync("echo hello && echo 1 | jq .").stdout)
```

If your Pyodide host doesn't resolve the ABI wheel automatically, pass the wheel
URL to `micropip.install(...)`.

This is a reduced-feature variant of the native PyPI wheel — the in-VFS shell
plus embedded `jq` and Monty `python`, driven through blocking `execute_sync()`.

**Present on both native and wasm:** `Bash` / `BashTool` / `ScriptedTool`,
`execute_sync()`, Monty `python=True`, `jq`, and sync/async custom-builtin
callbacks.

**Absent on wasm:** the async `execute()` / `execute_or_throw()` methods,
`FileSystem.real()`, and capsule `to/from_capsule`. Gated-off configuration
kwargs — `network=`, `sqlite=True`, `mounts=`, `external_handler=` — **fail
loudly** with `RuntimeError` at construction rather than silently no-op, so you
learn immediately that the WASM build can't do it.

## Next steps

- [Embedding](embedding.md) — the library API for Rust, Python, and JS: persistent
  state, the sandbox builder, resource limits, and the network allowlist.
- [CLI](cli.md) — run scripts from the terminal with `bashkit-cli`.
- [LLM tools](llm-tools.md) — expose Bashkit as a sandboxed tool for agent frameworks.
- [Security](security.md) — sandbox boundaries and what scripts cannot do.
