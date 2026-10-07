//! Compile the committed CPython snapshot to Pulley bytecode ahead of time.
//!
//! Decision: compile at build time, never at run time (python3 startup is a
//! product feature). Set `BASHKIT_CPYTHON_CWASM=/path/python.cwasm` to reuse a
//! module precompiled by the same wasmtime version and configuration.

use std::io::Read;
use std::path::PathBuf;

include!("src/config.rs");

fn main() {
    let manifest = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let out = PathBuf::from(std::env::var_os("OUT_DIR").unwrap()).join("python.cwasm");
    let snapshot = manifest.join("artifacts/python.wasm.gz");
    println!("cargo:rerun-if-changed={}", snapshot.display());
    println!("cargo:rerun-if-changed=src/config.rs");
    println!("cargo:rerun-if-env-changed=BASHKIT_CPYTHON_CWASM");

    if let Some(prebuilt) = std::env::var_os("BASHKIT_CPYTHON_CWASM") {
        std::fs::copy(&prebuilt, &out).expect("copy BASHKIT_CPYTHON_CWASM");
        return;
    }

    let mut wasm = Vec::new();
    flate2::read::GzDecoder::new(std::fs::File::open(&snapshot).expect("open snapshot"))
        .read_to_end(&mut wasm)
        .expect("decompress snapshot");
    let engine = wasmtime::Engine::new(&engine_config()).expect("engine");
    let cwasm = engine
        .precompile_module(&wasm)
        .expect("precompile CPython snapshot to Pulley");
    std::fs::write(&out, cwasm).expect("write python.cwasm");
}
