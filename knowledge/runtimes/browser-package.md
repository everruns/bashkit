---
type: Package Design
title: Browser Package
description: Slim single-threaded WebAssembly package design for browsers and JavaScript runtimes.
tags:
  - bashkit
  - wasm
  - packaging
  - npm
---

# WebAssembly Package (`@everruns/bashkit-wasm`)

> Naming: the crate is `bashkit-wasm` and the npm package is
> `@everruns/bashkit-wasm` — the `wasm` stem is deliberate. This is a
> `wasm-bindgen` module that runs in **any JavaScript host**, not just the
> browser (edge/serverless workers, Node, Deno, Bun), so the earlier `-web`
> name under-described its reach. It does **not** run in a non-JS/WASI wasm
> runtime (`wasmtime`, `wasmer`), which is why `-wasm` is scoped as "JS-host
> wasm", not "any wasm runtime". The spec filename stays `browser-package.md`
> for continuity; the browser is still the primary target.

## Status

Implemented (reduced feature set). Local build + headless smoke test green.
npm publish wired via `publish-wasm.yml`.

## Abstract

Bashkit ships a slim, **single-threaded** WebAssembly package built with
`wasm-bindgen` for `wasm32-unknown-unknown`. The browser is the primary target,
but because it's a plain wasm-bindgen module it also runs in other JavaScript
runtimes — edge/serverless workers (Cloudflare Workers, Vercel Edge, Deno
Deploy), Node, Deno, and Bun. Unlike the WASI-threads example
(`examples/browser`, napi + `wasm32-wasip1-threads`), it needs **no
`SharedArrayBuffer` and no cross-origin isolation** (`COOP`/`COEP`) headers, so
it drops into any web app — including embedded and third-party iframe contexts
where those headers cannot be set — and into the constrained edge runtimes that
can't use threads either. This is the distribution answer to issue \#2172.

## Why a separate package (not the napi `bashkit-js`)

The napi `@everruns/bashkit` package is Node/Bun/Deno-first. Its browser story is
`wasm32-wasip1-threads`, which:

- requires `SharedArrayBuffer` → requires `COOP: same-origin` +
  `COEP: require-corp` on the hosting document (viral, blocks many embeds), and
- was never actually published (the wasm matrix entry in `publish-js.yml` is
  disabled because the native binding pulls tokio `full` features).

`@everruns/bashkit-wasm` is a distinct, pure-wasm artifact with a distinct
consumer contract (browsers plus any other JS runtime). Keeping it separate
avoids dragging the five native `.node` binaries and the threads/headers
requirement into browser and edge bundles.

## Feature surface

Mirrors the `wasm` CI job: `scripted_tool` + `jq` on top of the default
interpreter. Present: full bash syntax, the text-tool builtins (`grep`, `sed`,
`awk`, `find`, `jq`, …), a binary-safe virtual filesystem, resource limits, JS
custom builtins (sync + async), streaming output, cancellation, static script
analysis, and content-addressed commit/checkout persistence.

Absent (need sockets, threads, or a host FS the browser sandbox lacks):
`http_client` (`curl`/`wget`), `ssh`, `sqlite`, embedded `python`, `realfs`
mounts, and native `interop`. Reach the network from a custom builtin that calls
the app's own `fetch` instead.

## Host-backed filesystem (`new Bash({ fs })`)

Embedders can replace the in-memory VFS with their own store by passing `fs`.
The bridge (`crates/bashkit-wasm/src/hostfs.rs`) implements `FsBackend` over a JS
object and wraps it in `PosixFs`, so hosts supply raw storage and inherit POSIX
semantics (parent checks, type checks, symlink resolution) for free.

Decisions:

- **Live, not copied.** Every read/write during a run is a call into the host
  object. No seeding pass and no write-back diff, so there is no workspace-size
  ceiling beyond the host's own and no lost-update window between runs.
- **`FsBackend`, not `FileSystem`.** Fourteen raw operations instead of the full
  POSIX surface; seven are required (`read`, `write`, `mkdir`, `remove`, `stat`,
  `readDir`, `exists`) and validated at construction so a missing method fails
  loudly rather than mid-script. `append`, `copy`, `rename` are synthesized from
  the required set when omitted; `chmod` is accepted and ignored (so `chmod +x`
  works against hosts with no permission model); `symlink` / `readLink` report
  `Unsupported` because they cannot be faked.
- **Async only.** Host methods may return a `Promise`, so filesystem access
  suspends the interpreter. `executeSync` and the synchronous `bash.readFile(...)`
  helpers report the suspension instead of blocking — a host filesystem implies
  `execute()`.
- **`files` + `fs` is rejected.** Seeding writes through `now_or_never`, which a
  promise-returning host can never satisfy. Rejecting at construction beats
  silently dropping the seed.
- **Errors map by `code`.** A thrown/rejected `Error` with `code` (`ENOENT`,
  `EEXIST`, `EACCES`, `EPERM`, `EISDIR`, `ENOTDIR`, `ENOTEMPTY`, `EXDEV`,
  `ENOSYS`) becomes the matching `io::ErrorKind`, so builtins that branch on kind
  (`ls`, `test -f`) behave as they do over the built-in VFS. Uncoded errors carry
  the host's message through as a generic I/O error.
- **No implicit `/dev/null`.** With a host filesystem there is no in-memory VFS
  underneath; hosts that expect redirects to `/dev/null` provide the entry.
- **`Send` bridging.** Same `SendWrapper` treatment as `JsBuiltin`: `!Send`
  `js_sys` values live only inside a synchronous scope, and the await crosses a
  `SendWrapper<JsFuture>`.

Covered by `__test__/host-fs.test.mjs`, whose fake host resolves every method on
a later microtask so the suspend/resume path is the one under test.

## Execution model

`wasm32-unknown-unknown` is single-threaded; the whole future chain runs on the
browser's one event loop. Two entry points:

- **`executeSync(cmd)`** drives `Bash::exec` with `now_or_never` — a single
  poll. Correct for scripts that never suspend (plain bash + `jq`; background
  jobs still run inline). If a script does
  suspend (e.g. an async JS custom builtin) it throws, directing the caller to
  `execute()`. While a sync call is in flight an `AtomicBool` is set so async
  custom builtins fail fast with a clear message instead of returning `Pending`
  forever.
- **`execute(cmd)`** returns a `Promise<ExecResult>` via
  `wasm-bindgen-futures::future_to_promise`. This is the path that can `await`
  async JS custom builtins (e.g. a GraphQL binary issuing a `fetch`/Relay
  request).
- **`executeWithOutput(cmd, callback)`** uses core streaming execution and
  invokes the callback with incremental stdout/stderr chunks.

Wall-clock futures use the JavaScript host's `setTimeout` through
`gloo-timers`. This supports `sleep`, builtin `timeout`, configured execution
deadlines, and tool-level `timeoutMs` without a tokio reactor or wasm threads.

### `Send` bridging

`bashkit::Builtin` is `Send + Sync` (via `#[async_trait]`), but `js_sys::Function`
and `JsFuture` are `!Send`. On single-threaded wasm we wrap both in
`send_wrapper::SendWrapper`, which only ever dereferences on its origin thread —
sound because there is exactly one thread. The `now_or_never` sync path and the
`future_to_promise` async path both avoid tokio's timer/thread-pool, which the
core already gates off under `cfg(target_family = "wasm")` (see
`crates/bashkit/src/lib.rs`).

## Package layout

`crates/bashkit-wasm/`:

- `src/lib.rs` — wasm-bindgen bindings (`Bash`, `ExecResult`, `JsBuiltin`).
- `js/index.js`, `js/index.d.ts` — hand-authored ESM wrapper + TS types. The
  wrapper resolves the `.wasm` relative to itself (`import.meta.url`), so it
  loads from a CDN, a bundler, or a plain `<script type="module">`.
- `package.json` — the published `@everruns/bashkit-wasm` manifest (copied into
  `pkg/` at build time).
- `scripts/build.sh` — `cargo build` → `wasm-bindgen --target web` → optional
  `wasm-opt -Oz`, emitting `pkg/`.
- `src/hostfs.rs` — the `FsBackend` bridge behind the `fs` option.
- `__test__/host-fs.test.mjs` — host-filesystem suite (async fake host).
- `__test__/bashkit-wasm.test.mjs` — headless Node integration suite
  (`node --test`) that feeds the `.wasm` bytes to init (no fetch, no headers),
  proving the no-configuration contract and covering sync/async execution, the
  VFS, custom builtins, and `ctx.fs`.
- `example/` — self-contained browser demos served by any static file server.

## Build

```bash
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli
bash crates/bashkit-wasm/scripts/build.sh                          # -> pkg/
node --test "crates/bashkit-wasm/__test__/*.test.mjs"                # verify
```

`--target web` output is a bundler-agnostic ES module; the consumer calls
`initBashkit()` once before constructing `Bash`.

## Versioning & publish

Version tracks the workspace `Cargo.toml` (currently synced by the release
prepare step, same as the other packages). `publish-wasm.yml` triggers on release
published, builds `pkg/`, runs the smoke test, and `npm publish`es
`@everruns/bashkit-wasm` with provenance (`NPM_TOKEN`, `id-token: write`) — same
pattern as `publish-js.yml`. Browser example smoke testing writes a file under
`/home/user`, reloads the page, and verifies `browserLocal` restores it from
`localStorage`.

## Limitations (see [Known Limitations](../operations/limitations.md))

- Cancellation and host deadlines are cooperative. Pending async work and
  wall-clock sleeps yield and can be timed out; synchronous CPU work cannot let
  the event loop deliver `cancel()` or a timer until it yields. Parser fuel,
  `maxCommands`, `maxLoopIterations`, and memory limits remain the hard bound
  for that work.
- Single-threaded: no OS threads (`std::thread::spawn` is unsupported) and no
  `tokio::spawn` reactor. Paths that hop to a thread or a background task on
  native run **inline** on wasm instead — background jobs (`cmd &`) execute
  synchronously (they already did for output ordering), and `awk` file
  redirects (`print > f`, `getline < f`) drive the VFS future to completion with
  `now_or_never` rather than a writer thread. Correct because the browser build
  only ever runs over the in-memory VFS, which never suspends.
- `executeSync` cannot await JS callbacks; use `execute()` for async builtins or
  any host filesystem.
- Custom-builtin `ctx` exposes `{ name, argv, stdin, env, cwd, fs }`, where `fs`
  is a live handle to the same VFS the script sees (mirrors the napi bindings).
