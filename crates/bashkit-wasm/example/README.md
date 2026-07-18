# Browser examples

Self-contained demos of `@everruns/bashkit-wasm`. They load the package as a
plain ES module and need **no bundler and no cross-origin isolation** (no
`COOP`/`COEP` headers) — any static file server works.

## Run

```bash
# From crates/bashkit-wasm/:
bash scripts/build.sh            # produces ./pkg (imported by the examples)
python3 -m http.server 8000      # or any static server
# then open:
#   http://localhost:8000/example/                       (interactive terminal)
#   http://localhost:8000/example/custom-builtins.html   (async builtins + ctx.fs)
```

Both pages `import { initBashkit, Bash } from "../pkg/index.js"`, so the `pkg/`
directory must exist (build it first).

## Files

| File | Shows |
|------|-------|
| `index.html` | Interactive terminal; `execute()` with a small async builtin |
| `custom-builtins.html` | Async builtin issuing a real `fetch` (the issue #2172 pattern) + `ctx.fs` read/write over the shared VFS |

## Using a bundler instead

The same imports work under Vite/webpack/esbuild — the `.wasm` is resolved via
`import.meta.url`, which bundlers understand. Install the published package
(`npm install @everruns/bashkit-wasm`) and import from `@everruns/bashkit-wasm`.
