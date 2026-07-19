# Get started

Bashkit is a single Rust core — a sandboxed bash interpreter with a virtual
filesystem, no `fork`/`exec`, and no host access — shipped as a package for
every major runtime. Pick the one that matches where your code runs, and follow
its quickstart to a first script in a couple of minutes.

## Choose your target

| Target | Install | Runs in | Quickstart |
|--------|---------|---------|-----------|
| **Rust** | `cargo add bashkit` | Any Rust app | [Get started in Rust](start-rust.md) |
| **Python** | `pip install bashkit` | CPython 3.9+ | [Get started in Python](start-python.md) |
| **Node / Bun / Deno** | `npm i @everruns/bashkit` | Node ≥ 18, Bun, Deno | [Get started in Node](start-node.md) |
| **Browser (WASM)** | `npm i @everruns/bashkit-wasm` | Browser, edge runtimes | [Get started in the browser](start-browser.md) |
| **Pyodide** | `micropip.install("bashkit")` | Pyodide, JupyterLite | [Get started in Pyodide](start-pyodide.md) |
| **CLI** | `cargo install bashkit-cli` | Terminal | [CLI](cli.md) |

**Which one?**

- Embedding in a **Rust, Python, or Node/Bun/Deno** service → the native crate,
  wheel, or NAPI addon. These share the full feature set.
- Running in a **browser or edge/serverless runtime** (Cloudflare Workers,
  Vercel Edge, Deno Deploy), or anywhere a native addon can't load → the
  [browser (WASM) package](start-browser.md). It even has a
  [live terminal](start-browser.md#try-it-live) you can try right on the page.
- Running inside **Pyodide or JupyterLite** (Python in the browser) → the
  [Pyodide wheel](start-pyodide.md).
- Running scripts from a **terminal** → the [CLI](cli.md).

## The same core everywhere

Whichever target you choose, the shell semantics are identical: the same
builtins, the same virtual filesystem, the same sandbox. What differs is the
host API surface (async vs sync, available features) and packaging — each
quickstart calls out its specifics.

Once you have a first script running, [Sandbox configuration &
limits](configuration.md) covers the knobs shared across every binding:
resource limits, filesystem backends, identity, and the network allowlist.

## Next steps

- [LLM tools](llm-tools.md) — expose Bashkit as a sandboxed tool for agent frameworks.
- [Sandbox configuration & limits](configuration.md) — resource limits and sandbox options.
- [Security](security.md) — sandbox boundaries and what scripts cannot do.
