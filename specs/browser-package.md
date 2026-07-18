# Browser Package (`@everruns/bashkit-web`)

## Status

Implemented (reduced feature set). Local build + headless smoke test green.
npm publish wired via `publish-web.yml`.

## Abstract

Bashkit ships a slim, **single-threaded** WebAssembly package for the browser,
built with `wasm-bindgen` for `wasm32-unknown-unknown`. Unlike the WASI-threads
example (`examples/browser`, napi + `wasm32-wasip1-threads`), it needs **no
`SharedArrayBuffer` and no cross-origin isolation** (`COOP`/`COEP`) headers, so
it drops into any web app — including embedded and third-party iframe contexts
where those headers cannot be set. This is the distribution answer to issue
\#2172.

## Why a separate package (not the napi `bashkit-js`)

The napi `@everruns/bashkit` package is Node/Bun/Deno-first. Its browser story is
`wasm32-wasip1-threads`, which:

- requires `SharedArrayBuffer` → requires `COOP: same-origin` +
  `COEP: require-corp` on the hosting document (viral, blocks many embeds), and
- was never actually published (the wasm matrix entry in `publish-js.yml` is
  disabled because the native binding pulls tokio `full` features).

`@everruns/bashkit-web` is a distinct, browser-only artifact with a distinct
consumer contract. Keeping it separate avoids dragging the five native `.node`
binaries and the threads/headers requirement into browser bundles.

## Feature surface

Mirrors the `wasm` CI job: `scripted_tool` + `jq` on top of the default
interpreter. Present: full bash syntax, the text-tool builtins (`grep`, `sed`,
`awk`, `find`, `jq`, …), a virtual filesystem, resource limits, and JS custom
builtins (sync + async).

Absent (need sockets, threads, or a host FS the browser sandbox lacks):
`http_client` (`curl`/`wget`), `ssh`, `sqlite`, embedded `python`, `realfs`
mounts, and native `interop`. Reach the network from a custom builtin that calls
the app's own `fetch` instead.

## Execution model

`wasm32-unknown-unknown` is single-threaded; the whole future chain runs on the
browser's one event loop. Two entry points:

- **`executeSync(cmd)`** drives `Bash::exec` with `now_or_never` — a single
  poll. Correct for scripts that never suspend (plain bash + `jq`; `sleep` and
  background jobs do not suspend on wasm — see Limitations). If a script does
  suspend (e.g. an async JS custom builtin) it throws, directing the caller to
  `execute()`. While a sync call is in flight an `AtomicBool` is set so async
  custom builtins fail fast with a clear message instead of returning `Pending`
  forever.
- **`execute(cmd)`** returns a `Promise<ExecResult>` via
  `wasm-bindgen-futures::future_to_promise`. This is the path that can `await`
  async JS custom builtins (e.g. a GraphQL binary issuing a `fetch`/Relay
  request).

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
- `package.json` — the published `@everruns/bashkit-web` manifest (copied into
  `pkg/` at build time).
- `scripts/build.sh` — `cargo build` → `wasm-bindgen --target web` → optional
  `wasm-opt -Oz`, emitting `pkg/`.
- `__test__/bashkit-web.test.mjs` — headless Node integration suite
  (`node --test`) that feeds the `.wasm` bytes to init (no fetch, no headers),
  proving the no-configuration contract and covering sync/async execution, the
  VFS, custom builtins, and `ctx.fs`.
- `example/` — self-contained browser demos served by any static file server.

## Build

```bash
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli
bash crates/bashkit-wasm/scripts/build.sh                          # -> pkg/
node --test crates/bashkit-wasm/__test__/bashkit-web.test.mjs      # verify
```

`--target web` output is a bundler-agnostic ES module; the consumer calls
`initBashkit()` once before constructing `Bash`.

## Versioning & publish

Version tracks the workspace `Cargo.toml` (currently synced by the release
prepare step, same as the other packages). `publish-web.yml` triggers on release
published, builds `pkg/`, runs the smoke test, and `npm publish`es
`@everruns/bashkit-web` with provenance (`NPM_TOKEN`, `id-token: write`) — same
pattern as `publish-js.yml`.

## Limitations (see `specs/limitations.md`)

- No wall-clock time on `wasm32-unknown-unknown` (no timer driver). This is a
  hard platform constraint, so time-based behaviour degrades rather than
  enforces, and never blocks:
  - `sleep N` elapses **instantly** (it cannot suspend for real time).
  - the `timeout N cmd` builtin and the tool-level `timeoutMs` run the command
    **without** wall-clock enforcement.
  - Runaway work is still bounded by the parser fuel budget and `maxCommands` /
    `maxLoopIterations`, which do not depend on a clock.
- Single-threaded: no OS threads (`std::thread::spawn` is unsupported) and no
  `tokio::spawn` reactor. Paths that hop to a thread or a background task on
  native run **inline** on wasm instead — background jobs (`cmd &`) execute
  synchronously (they already did for output ordering), and `awk` file
  redirects (`print > f`, `getline < f`) drive the VFS future to completion with
  `now_or_never` rather than a writer thread. Correct because the browser build
  only ever runs over the in-memory VFS, which never suspends.
- `executeSync` cannot await JS callbacks; use `execute()` for async builtins.
- Custom-builtin `ctx` exposes `{ name, argv, stdin, env, cwd, fs }`, where `fs`
  is a live handle to the same VFS the script sees (mirrors the napi bindings).
