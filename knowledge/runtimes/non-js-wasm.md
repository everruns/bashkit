---
type: Package Design
title: Non-JS WebAssembly Embedding
description: Running bashkit as a wasm component with no JS engine and no WASI, including the Hyperlight micro-VM guest.
tags:
  - bashkit
  - wasm
  - component-model
  - hyperlight
---

# Non-JS WebAssembly Embedding

## Status

Experiment, proven end-to-end under wasmtime. See `examples/hyperlight/`.
Not published as a package; no micro-VM boot test yet (needs KVM/WHP hardware).

## Abstract

`wasm32-unknown-unknown` is not synonymous with "JS host". A wasm component can
run in a runtime with no JS engine at all — `wasmtime` embedded in a plain Rust
program, or the `no_std` wasmtime that
[hyperlight-wasm](https://github.com/hyperlight-dev/hyperlight-wasm) compiles
into a Hyperlight micro-VM guest. Bashkit fits that shape unusually well: it
already runs single-threaded over an in-memory VFS and makes no syscalls, which
is exactly the workload class Hyperlight is built for (its guests have "no
kernel or OS in the VM").

The obstacle was never the interpreter. It was three dependencies that silently
assumed JS.

## Decision: `wasm_js` is a feature, not an assumption

The `bashkit` crate gained a `wasm_js` feature that pulls in every JS-backed
path. It is enabled by the JS packages (`bashkit-wasm`) and by the
`wasm32-unknown-unknown` CI check; a non-JS embedder leaves it off.

| Concern | With `wasm_js` | Without |
|---|---|---|
| Clock | `web-time` (`Performance.now`, `Date.now`) | `time_compat::host_clock`, embedder symbol |
| Timers (`sleep`, `timeout`) | `gloo-timers` (`setTimeout`) | spin on the host clock |
| Entropy | `getrandom/wasm_js` (`crypto.getRandomValues`) | `getrandom` custom backend, embedder's |
| `chrono::Utc::now` | `chrono/wasmbind` (JS `Date`) | `time_compat::now_utc` |
| Host-call driver | `wasm-bindgen-futures::spawn_local` | none, `next_event` polls inline |

Two consequences worth stating plainly:

- **`chrono`'s `wasmbind` is off for everyone now.** Bashkit reads the clock
  through `time_compat` on every target, so `Utc::now()` must not be called
  directly anywhere in the crate — `time_compat::now_utc()` is the entry point.
  The JS packages re-enable `wasmbind` only so behavior is unchanged there.
- **The unresolved-import failure mode is nasty**, which is why these are
  compile/link-time contracts rather than runtime fallbacks: a JS import in a
  non-JS runtime fails at *instantiation*, with an error naming a mangled
  `__wbg_*` symbol and no hint about which dependency wanted it.

## Decision: the host boundary is two functions

`time_compat::host_clock` declares `__bashkit_host_now_micros() -> u64`, which
the embedder defines (mirroring `getrandom`'s `__getrandom_v03_custom`
contract). Omitting it is a link error, not a panic on first use. Microseconds
since the Unix epoch, assumed non-decreasing, backing both `Instant` and
`SystemTime`.

Together with entropy, that is the entire outside world bashkit needs:

```wit
interface host {
    now-micros: func() -> u64;
    random-bytes: func(len: u32) -> list<u8>;
}
```

Verified, not assumed — the built guest's imports are exactly those two:

```
$ wasm-tools print bashkit_hyperlight_guest.wasm | grep '(import'
  (import "bashkit:sandbox/host" "now-micros" ...)
  (import "bashkit:sandbox/host" "random-bytes" ...)
```

## Decision: timers spin

Without JS there is no `setTimeout`, and in a micro-VM there is no other thread
to make progress. `sleep` therefore blocks, spinning on the host clock, and
`timeout` polls the future and compares against a deadline. Blocking is the
correct choice here, not a compromise: these embedders drive execution with a
single poll (`now_or_never`), so a pending timer future could never be woken.
A guest that burns VM cycles during `sleep 1` is the price of having no timer
hardware.

The same "no other thread" fact decides who drives a parked host-call
execution. `host_call::spawn_execution` hands the execution future to a task
spawner on every target that has one, so the wall-clock deadline keeps running
while the host sits on a `HostCallRequest`. There is nothing to spawn onto
here, so the function hands the future back and `ExecutionHandle::next_event`
polls it inline — the deadline is enforced on the host's next poll instead of
autonomously. The observable API contract does not change: a timed-out
execution still drops its session and still fails `into_bash()`. Because CI
only *builds* this target, the inline path is covered by native unit tests in
`host_call.rs` that force `Driver::Inline`.

## Hyperlight specifics

- Hyperlight-Wasm guests are built for **`wasm32-unknown-unknown` with no WASI
  imports**, wrapped as components, then AOT-compiled with `hyperlight-wasm-aot`
  for `x86_64-unknown-none`. The wasip1/wasip2 targets are *not* the route in
  (they are still useful for `wasmtime`/`wasmer` hosts).
- Absent by construction, same reasons as the browser package: `http_client`,
  `ssh`, `realfs`, `python`. `sqlite` additionally does not build on WASI.
- Sizes measured on the `jq`-only feature set: 7.2 MB core module, 6.9 MB
  component, 22 MB `.aot`. Hyperlight's own component example configures a
  200 MB heap, so the VFS has room, but the AOT image is the number to watch
  when sizing a sandbox.

## Testing

Three tiers, only the first two are automatable without special hardware:

1. **Under `wasmtime`** (`examples/hyperlight/build-and-run.sh`) — builds the
   guest, wraps it as a component, asserts the import list is host-only, and
   runs a script through it. Hyperlight-Wasm's guest *is* wasmtime, so this
   covers the component, the WIT world, and all interpreter behavior.
2. **AOT compile** with `hyperlight-wasm-aot --component` — proves the artifact
   is consumable by the micro-VM guest.
3. **Micro-VM boot** — needs `/dev/kvm` (Linux) or WHP (Windows). Not run in
   this repo's CI yet.

## See also

- [Browser Package](browser-package.md) — the JS-host wasm package, same
  reduced feature surface and the same single-threaded execution model.
- [Architecture](../foundations/architecture.md) — where `time_compat` sits
  relative to the interpreter.
- [Known Limitations](../operations/limitations.md) — the negative spec these
  absent features belong to.
