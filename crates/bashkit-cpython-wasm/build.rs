//! Compile the committed CPython snapshot ahead of time: to Pulley bytecode,
//! or to native code for the target with the `native` feature.
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
    // The build script's own cfg is the host's; read the crate feature and
    // the cross-compilation target from Cargo instead.
    let native = std::env::var_os("CARGO_FEATURE_NATIVE").is_some();
    let target = std::env::var("TARGET").expect("TARGET");
    let engine = wasmtime::Engine::new(&engine_config_for(native, Some(&target))).expect("engine");
    let cwasm = engine
        .precompile_module(&wasm)
        .expect("precompile CPython snapshot");
    std::fs::write(&out, cwasm).expect("write python.cwasm");
}
