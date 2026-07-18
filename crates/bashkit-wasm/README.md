# @everruns/bashkit-wasm

Sandboxed bash interpreter compiled to WebAssembly, for the **browser and any
other JavaScript runtime** — edge/serverless workers (Cloudflare Workers, Vercel
Edge, Deno Deploy), Node, Deno, and Bun.

Unlike a WASI-threads build, this package is **single-threaded**: it needs no
`SharedArrayBuffer` and **no cross-origin isolation** (`COOP`/`COEP`) headers.
That makes it a drop-in for any web app — including embedded and third-party
iframe contexts where those headers can't be set — and for the constrained edge
runtimes that can't use threads either.

It's a `wasm-bindgen` module, so it runs in any JS host but **not** a
non-JS/WASI wasm runtime (`wasmtime`, `wasmer`). For a native Node.js / Bun /
Deno addon (faster, no wasm), use
[`@everruns/bashkit`](https://www.npmjs.com/package/@everruns/bashkit) instead;
reach for this package when a native addon can't load — browsers and edge
runtimes.

## Live demo

A full interactive terminal built on this package —
[**`examples/browser`**](https://github.com/everruns/bashkit/tree/main/examples/browser).
It's a single `index.html` on Vite: `pnpm install && pnpm start`, no build step
and no special headers.

[![Bashkit browser terminal](https://github.com/everruns/bashkit/raw/main/examples/browser/demo.png)](https://github.com/everruns/bashkit/tree/main/examples/browser)

## Install

```bash
npm install @everruns/bashkit-wasm
```

## Quick start

```js
import { initBashkit, Bash } from "@everruns/bashkit-wasm";

// Load the .wasm once before constructing Bash.
await initBashkit();

const bash = new Bash();
const result = bash.executeSync('echo "Hello, browser!" | tr a-z A-Z');
console.log(result.stdout); // HELLO, BROWSER!
```

### Plain `<script type="module">` (no bundler)

```html
<script type="module">
  import { initBashkit, Bash } from "https://esm.sh/@everruns/bashkit-wasm";
  await initBashkit();
  const bash = new Bash();
  document.body.textContent = bash.executeSync("seq 1 5 | paste -sd+ | bc").stdout;
</script>
```

## Async custom builtins

Register JS callbacks as bash commands. Async callbacks (e.g. issuing a
`fetch` / GraphQL request) are awaited by `execute()` — the async API:

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

`ctx` is `{ name, argv, stdin, env, cwd, fs }`. Return the builtin's stdout as a
string (or a `Promise<string>`); throwing becomes stderr with exit code 1.

`ctx.fs` is a live handle to the **same** virtual filesystem the script sees, so
a builtin can read inputs and write outputs that later commands pick up:

```js
const bash = new Bash({
  customBuiltins: {
    "uppercase-file": (ctx) => {
      const text = ctx.fs.readFile(ctx.argv[0]);
      ctx.fs.writeFile("/out.txt", text.toUpperCase());
      return "done\n";
    },
  },
});
bash.writeFile("/in.txt", "hello\n");
await bash.execute("uppercase-file /in.txt && cat /out.txt"); // -> HELLO
```

`ctx.fs` has `readFile`, `writeFile`, `appendFile`, `exists`, `mkdir`, `remove`,
and `ls` — the same surface as the `Bash` VFS helpers below.

## Sync vs async

- `executeSync(cmd)` — for plain bash and `jq`. Fast, returns an `ExecResult`
  directly. Throws only if the script suspends — an async custom builtin;
  `sleep` and background jobs do not suspend on wasm (see Limitations).
- `execute(cmd)` — returns `Promise<ExecResult>`. Required whenever an async
  custom builtin may run.

## Options

```ts
new Bash({
  username, hostname, cwd,
  env: { KEY: "value" },
  maxCommands, maxLoopIterations, maxMemory,
  files: { "/config.json": '{"debug":true}' },
  customBuiltins: { name: (ctx) => "..." },
});
```

## Virtual filesystem

Files created via the helpers are visible to scripts and vice versa:

```js
bash.mkdir("/data");
bash.writeFile("/data/x.txt", "hi\n");
bash.appendFile("/data/x.txt", "there\n");
bash.readFile("/data/x.txt"); // "hi\nthere\n"
bash.exists("/data/x.txt");   // true
bash.ls("/data");             // ["x.txt"]
bash.executeSync("cat /data/x.txt").stdout; // "hi\nthere\n"
bash.remove("/data/x.txt");

// bash.fs() returns the same live handle passed to builtins as ctx.fs
const fs = bash.fs();
```

## What's included

Plain bash plus the built-in text tooling (`grep`, `sed`, `awk`, `jq`, `find`,
…) and `jq`. Not included in the browser build: outbound HTTP (`curl`/`wget`),
`ssh`, `sqlite`, and embedded `python` — these need sockets, threads, or a host
filesystem the browser sandbox doesn't provide. Bridge to the network through a
custom builtin (see above) so requests go through your app's own `fetch`.

## Limitations

- **No wall-clock time.** `wasm32-unknown-unknown` has no reliable timer driver,
  so `sleep N` elapses instantly and neither the `timeout N` builtin nor
  `timeoutMs` is enforced. Runaway scripts are instead bounded by `maxCommands`,
  `maxLoopIterations`, and the parser fuel budget — a `while true` loop throws a
  resource-limit error rather than hanging.
- **`executeSync` can't run async builtins.** The single-threaded event loop
  can't settle a `Promise` without yielding; an async builtin under
  `executeSync` fails fast with a clear message. Use `execute()`.

## Examples

- [**`examples/browser`**](https://github.com/everruns/bashkit/tree/main/examples/browser)
  — the full interactive terminal shown above, on Vite (no build step, no headers).
- Minimal, dependency-free demos in [`example/`](./example) — an interactive
  terminal and an async-builtin/`ctx.fs` demo, served by any static file server.
  See [`example/README.md`](./example/README.md).

## Development

```bash
# Build the bundle and run the headless integration tests:
bash scripts/build.sh
node --test __test__/bashkit-wasm.test.mjs
# or, from the repo root:
just build-wasm
```

## License

MIT — part of the [Bashkit](https://github.com/everruns/bashkit) project.
