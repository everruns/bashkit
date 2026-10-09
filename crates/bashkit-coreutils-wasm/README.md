# bashkit-coreutils-wasm

[uutils/coreutils](https://github.com/uutils/coreutils) 0.12 compiled to
WebAssembly (`wasm32-wasip1`) as one multicall guest, precompiled at build
time to Wasmtime's portable Pulley bytecode.

This crate ships bytes, not behavior: the precompiled module, the list of
utilities and the matching `wasmtime::Config`. It is consumed by
[bashkit](https://crates.io/crates/bashkit)'s `wasm-coreutils` feature, which
provides the WASI host over bashkit's virtual filesystem and the builtins:

```toml
[dependencies]
bashkit = { version = "0.18.2", features = ["wasm-coreutils"] }
```

## Contents

| Item | Description |
|------|-------------|
| `load_module(&engine)` | The guest as a `wasmtime::Module` (mapped in place on first load) |
| `engine_config()` / `engine()` | The compile-affecting Wasmtime configuration the module was built for |
| `UTILS` | Utility names the guest dispatches on (71, sorted) |
| `UUTILS_VERSION` | `0.12.0` |

Guest contract: a WASI command (`_start`); `argv[0]` names the utility;
`PWD` becomes the working directory; exactly one preopen, `/`, at fd 3; one
instance per call; the status comes from `proc_exit`.

## Rebuilding

The guest sources live in the bashkit repository under
`crates/bashkit-coreutils-wasm/guest/` (`utils.txt`, `gen.py`, `build.sh`).
`build.rs` compiles the committed `artifacts/coreutils.wasm.xz` to Pulley on
every build; no network access is needed at build or run time.

## License

Crate code and uutils: MIT. The embedded guest also contains code from
uutils' dependencies under MIT, Apache-2.0, Unicode-3.0 and Zlib terms; see
[THIRD_PARTY_LICENSES.md](THIRD_PARTY_LICENSES.md).
