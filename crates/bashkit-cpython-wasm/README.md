# bashkit-cpython-wasm

CPython 3.14 compiled to WebAssembly (`wasm32-wasip1`), pre-initialized with
Wizer and precompiled at build time to Wasmtime's portable Pulley bytecode.

This crate ships bytes, not behavior: the precompiled interpreter module, the
zipped standard library and the matching `wasmtime::Config`. It is consumed by
[bashkit](https://crates.io/crates/bashkit)'s `cpython` feature, which provides
the WASI host over bashkit's virtual filesystem and the `python3` builtin.
Most users want that feature, not this crate directly:

```toml
[dependencies]
bashkit = { version = "0.18.2", features = ["cpython"] }
```

## Contents

| Item | Description |
|------|-------------|
| `load_module(&engine)` | The interpreter snapshot as a `wasmtime::Module` (mapped in place on first load) |
| `engine_config()` / `engine()` | The compile-affecting Wasmtime configuration the module was built for |
| `STDLIB_ZIP`, `STDLIB_ZIP_PATH` | Stdlib sources; the host must serve these exact bytes at this guest path |
| `PYTHON_VERSION` | `3.14.8` |

Guest contract: export `bashkit_run() -> i32` runs one `python3` invocation,
reading argv, environment (including `PWD`) and stdio through WASI preview1;
exactly one preopen, `/`, at fd 3; one instance per call.

## Rebuilding

`guest/build.sh` reproduces `artifacts/` from pinned sources (CPython, WASI
SDK, zlib, SQLite, the Wasmtime CLI for `wizer`). `build.rs` compiles the
snapshot to Pulley on every build; no network access is needed at build or
run time.

## License

Crate code: MIT. Embedded CPython: Python Software Foundation License
(Python-2.0), plus the bundled libraries listed in
[THIRD_PARTY_LICENSES.md](THIRD_PARTY_LICENSES.md).
