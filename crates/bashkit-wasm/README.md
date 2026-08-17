# @everruns/bashkit-wasm

Sandboxed bash interpreter compiled to WebAssembly, for the **browser and any
other JavaScript runtime**: edge/serverless workers (Cloudflare Workers, Vercel
Edge, Deno Deploy), Node, Deno, and Bun.

Unlike a WASI-threads build, this package is **single-threaded**: it needs no
`SharedArrayBuffer` and **no cross-origin isolation** (`COOP`/`COEP`) headers.
That makes it a drop-in for any web app, including embedded and third-party
iframe contexts where those headers can't be set, and for the constrained edge
runtimes that can't use threads either.

It's a `wasm-bindgen` module, so it runs in any JS host but **not** a
non-JS/WASI wasm runtime (`wasmtime`, `wasmer`). For a native Node.js / Bun /
Deno addon (faster, no wasm), use
[`@everruns/bashkit`](https://www.npmjs.com/package/@everruns/bashkit) instead;
reach for this package when a native addon can't load, browsers and edge
runtimes.

## Live demo

A full interactive terminal built on this package,
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
`fetch` / GraphQL request) are awaited by `execute()`, the async API:

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

`ctx.fs` has `readFile`, `writeFile`, `appendFile`, their binary-safe
`*FileBytes` variants, `exists`, `mkdir`, `remove`, and `ls`, the same surface
as the `Bash` VFS helpers below.

## Sync vs async

- `executeSync(cmd)`, for plain bash and `jq`. Fast, returns an `ExecResult`
  directly. Throws if the script suspends, for example `sleep` or an async
  custom builtin.
- `execute(cmd)`, returns `Promise<ExecResult>`. Required whenever an async
  custom builtin or wall-clock operation may run.
- `executeWithOutput(cmd, callback)`, async execution plus incremental
  `(stdout, stderr)` chunks. The returned `ExecResult` remains authoritative.

## Options

```ts
new Bash({
  username, hostname, cwd,
  env: { KEY: "value" },
  maxCommands, maxLoopIterations, maxMemory,
  files: { "/config.json": '{"debug":true}' },
  customBuiltins: { name: (ctx) => "..." },
  fs: hostFileSystem, // host-backed filesystem; see below
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

// Binary-safe I/O avoids UTF-8 conversion.
bash.writeFileBytes("/data/blob", Uint8Array.from([0, 255]));
bash.readFileBytes("/data/blob");

// bash.fs() returns the same live handle passed to builtins as ctx.fs
const fs = bash.fs();
```

## Analysis, cancellation, and persistence

`analyze(script)` provides the same advisory Gatekeeper projection as the
native bindings. Treat parse errors or `isOpaque` as deny/prompt, not safe:

```js
const analysis = bash.analyze("cat input.txt | jq . > output.json");
if (analysis.isOpaque) throw new Error("permission prompt required");
```

`cancel()` sets a sticky cooperative flag checked at command boundaries;
`clearCancel()` makes the instance reusable. `commit()` returns a
content-addressed object set and `checkout(id, objects, policy?)` restores it:

```js
const saved = bash.commit();
const resumed = new Bash();
resumed.checkout(saved.id, saved.objects); // policy defaults to "superset"
```

## Host-backed filesystem

Pass `fs` to run scripts directly against storage you own, a Durable Object, an
OPFS handle, IndexedDB, instead of the in-memory VFS. Nothing is copied in or
diffed back out: every read and write during the run is a call into your object.

```js
const bash = new Bash({ cwd: "/workspace", fs: myHost });
const r = await bash.execute("grep -rl TODO . | head -5");
```

Implement seven required methods; each may return its value directly or as a
`Promise`:

```ts
read(path)                 // -> Uint8Array | string   (throw ENOENT when absent)
write(path, bytes)         // -> void
mkdir(path, recursive)     // -> void
remove(path, recursive)    // -> void
stat(path)                 // -> { type: "file" | "dir" | "symlink", size?, mode?, mtimeMs? }
readDir(path)              // -> [{ name, type, size?, mode?, mtimeMs? }]
exists(path)               // -> boolean
```

`append`, `copy`, `rename`, and `chmod` are optional: omit them and they are
synthesized from the required primitives (`chmod` is accepted and ignored, so
`chmod +x` still works). `symlink` and `readLink` are optional too, but scripts
that reach for them fail with `ENOSYS` when the host omits them.

Your host implements raw storage only, POSIX semantics (parent-directory
checks, "is a directory", symlink resolution) are enforced above it. Throw an
`Error` carrying a `code` (`ENOENT`, `EEXIST`, `EACCES`, `EPERM`, `EISDIR`,
`ENOTDIR`, `ENOTEMPTY`, `EXDEV`, `ENOSYS`) so bash reports the failure the way a
real shell does.

Two contract notes:

- **`execute()` only.** A host call can suspend the interpreter, and
  `executeSync` cannot await, it reports the suspension instead of blocking.
  The synchronous `bash.readFile(...)` helpers behave the same way.
- **`files` is rejected alongside `fs`.** Seeding writes through the VFS
  synchronously, which a promise-returning host can never satisfy. Write seed
  data through the host directly.

Provide `/dev/null` in the host if your scripts redirect to it; with a host
filesystem there is no built-in VFS underneath to supply it.

## What's included

Plain bash plus the built-in text tooling (`grep`, `sed`, `awk`, `jq`, `find`,
…) and `jq`. Not included in the browser build: outbound HTTP (`curl`/`wget`),
`ssh`, `sqlite`, and embedded `python`, these need sockets, threads, or a host
filesystem the browser sandbox doesn't provide. Bridge to the network through a
custom builtin (see above) so requests go through your app's own `fetch`.

## Limitations

- **Cancellation is cooperative.** Host timers drive `sleep`, `timeout`, and
  configured execution deadlines. A synchronous CPU loop cannot observe a JS
  callback or `cancel()` until control returns to the event loop, so command,
  loop, parser-fuel, and memory limits remain the hard bound for such work.
- **`executeSync` can't run async builtins.** The single-threaded event loop
  can't settle a `Promise` without yielding; an async builtin under
  `executeSync` fails fast with a clear message. Use `execute()`.

## Examples

- [**`examples/browser`**](https://github.com/everruns/bashkit/tree/main/examples/browser)
, the full interactive terminal shown above, on Vite (no build step, no headers).
- Minimal, dependency-free demos in [`example/`](./example), an interactive
  terminal and an async-builtin/`ctx.fs` demo, served by any static file server.
  See [`example/README.md`](./example/README.md).

## Development

```bash
# Build the bundle and run the headless integration tests:
bash scripts/build.sh
node --test "__test__/*.test.mjs"
# or, from the repo root:
just build-wasm
```

## License

MIT, part of the [Bashkit](https://github.com/everruns/bashkit) project.
