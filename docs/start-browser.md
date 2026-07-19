# Get started in the browser (WASM)

Run the Bashkit sandbox in the browser, at the edge, or in any JavaScript
runtime that can't load a native addon. `@everruns/bashkit-wasm` is a slim,
**single-threaded** `wasm-bindgen` module: unlike a WASI-threads build it needs
**no `SharedArrayBuffer` and no cross-origin isolation** (`COOP`/`COEP`)
headers, so it drops into any web app — including embedded and third-party
iframe contexts where those headers can't be set — and into edge/serverless
runtimes (Cloudflare Workers, Vercel Edge, Deno Deploy) that can't use threads.

For server-side Node, Bun, or Deno where a native addon loads, prefer the faster
[`@everruns/bashkit`](start-node.md) NAPI package instead.

## Install

```bash
npm install @everruns/bashkit-wasm
```

## First script

Load the `.wasm` once with `initBashkit()` before constructing `Bash`:

```js
import { initBashkit, Bash } from "@everruns/bashkit-wasm";

await initBashkit();

const bash = new Bash();
const result = bash.executeSync('echo "Hello, browser!" | tr a-z A-Z');
console.log(result.stdout); // HELLO, BROWSER!
```

## Try it live

This is the exact package above, running in your browser — no server, nothing
you type leaves this page. Launch it and run some bash:

<div data-bashkit-terminal></div>

## No bundler

Straight from a CDN, in a plain `<script type="module">`:

```html
<script type="module">
  import { initBashkit, Bash } from "https://esm.sh/@everruns/bashkit-wasm";
  await initBashkit();
  const bash = new Bash();
  document.body.textContent = bash.executeSync("seq 1 5 | paste -sd+ | bc").stdout;
</script>
```

## Async custom builtins

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

## Caveats

- It's a `wasm-bindgen` module: it runs in any **JavaScript** host but **not** a
  non-JS/WASI wasm runtime (`wasmtime`, `wasmer`).
- Reduced feature set relative to the native bindings. Prefer
  [`@everruns/bashkit`](start-node.md) when a native addon can load.

A full interactive terminal built on this package lives in
[`examples/browser`](https://github.com/everruns/bashkit/tree/main/examples/browser)
(single `index.html` on Vite, no build step, no special headers).

## Next steps

- [Get started in Pyodide](start-pyodide.md) — for Python running in the browser.
- [Sandbox configuration & limits](configuration.md) — resource limits and sandbox options.
- [Security](security.md) — sandbox boundaries and what scripts cannot do.
