# Bashkit as a non-JS wasm component (Hyperlight-Wasm experiment)

Runs the bashkit interpreter as a WebAssembly **component with no JS engine and
no WASI** — the shape a [Hyperlight](https://github.com/hyperlight-dev/hyperlight)
micro-VM guest has to be.

Hyperlight runs untrusted code in a VM with *no kernel and no OS*: no syscalls,
no threads, no filesystem. Bashkit already executes single-threaded over an
in-memory VFS and never touches the host, so the interpreter itself needs no
changes. Only its clock and entropy did — see
`knowledge/runtimes/non-js-wasm.md`.

## The host boundary

Two imports, and nothing else (`wit/bashkit.wit`):

```wit
interface host {
    now-micros: func() -> u64;
    random-bytes: func(len: u32) -> list<u8>;
}
```

```
$ wasm-tools print target/wasm32-unknown-unknown/release/bashkit_hyperlight_guest.wasm | grep '(import'
  (import "bashkit:sandbox/host" "now-micros" (func ...))
  (import "bashkit:sandbox/host" "random-bytes" (func ...))
```

## Run it

```bash
rustup target add wasm32-unknown-unknown
cargo install wasm-tools
./build-and-run.sh
```

Output (`smoke.sh` running inside the component):

```
==> guest imports (must be host-only)
(import "bashkit:sandbox/host"
==> running under wasmtime
beta gamma alpha
6
slept 1s
```

That exercises the VFS, a `sort | awk | tr` pipeline, `jq`, `date`, and `sleep`
— i.e. the host clock import and the spin-timer path.

## Layout

| Path | What |
|---|---|
| `wit/bashkit.wit` | The world: two host imports, one `exec` export |
| `src/lib.rs` | Guest — bashkit + the clock and entropy hooks |
| `host/` | wasmtime host runner (Hyperlight's guest *is* wasmtime) |
| `smoke.sh` | Script executed inside the sandbox |

The guest is its own cargo workspace: it only ever builds for
`wasm32-unknown-unknown`, so it must stay out of host-target CI builds.

## Getting to an actual micro-VM

`build-and-run.sh` stops at wasmtime. The remaining steps, and where they stand:

1. **AOT-compile for the guest** — done, works:
   ```
   hyperlight-wasm-aot compile --component target/bashkit-sandbox.component.wasm
   Aot Compiling ... to [x86_64-unknown-none] (LTS wasmtime)
   ```
   7.2 MB core module → 6.9 MB component → 22 MB `.aot`.
2. **Boot it** — `hyperlight_wasm::SandboxBuilder`, `load_module(aot)`, then
   `call_guest_function`. Needs `/dev/kvm` (Linux) or WHP (Windows); untested
   here for lack of virtualization hardware.
3. **Host functions** — `now-micros` and `random-bytes` are declared with
   `hyperlight_component_macro::host_bindgen!` against this same `.wit`.

Note that Hyperlight-Wasm is explicitly experimental and Linux/Windows only.
