//! Compile the committed coreutils guest to Pulley bytecode ahead of time.
//!
//! Decision: compile at build time, never at run time (startup is a product
//! feature). Set `BASHKIT_COREUTILS_CWASM=/path/coreutils.cwasm` to reuse a
//! module precompiled by the same wasmtime version and configuration.

use std::path::PathBuf;

include!("src/config.rs");

fn main() {
    let manifest = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let out = PathBuf::from(std::env::var_os("OUT_DIR").unwrap()).join("coreutils.cwasm");
    let guest = manifest.join("artifacts/coreutils.wasm.xz");
    println!("cargo:rerun-if-changed={}", guest.display());
    println!("cargo:rerun-if-changed=src/config.rs");
    println!("cargo:rerun-if-env-changed=BASHKIT_COREUTILS_CWASM");

    // Utility list, from the same file guest/build.sh compiled in.
    let utils_txt = manifest.join("artifacts/utils.txt");
    println!("cargo:rerun-if-changed={}", utils_txt.display());
    let names: Vec<String> = std::fs::read_to_string(&utils_txt)
        .expect("read utils.txt")
        .lines()
        .filter(|l| !l.is_empty())
        .map(|l| format!("{l:?}"))
        .collect();
    let utils_rs = PathBuf::from(std::env::var_os("OUT_DIR").unwrap()).join("utils.rs");
    std::fs::write(&utils_rs, format!("[{}]", names.join(", "))).expect("write utils.rs");

    if let Some(prebuilt) = std::env::var_os("BASHKIT_COREUTILS_CWASM") {
        std::fs::copy(&prebuilt, &out).expect("copy BASHKIT_COREUTILS_CWASM");
        return;
    }

    let mut wasm = Vec::new();
    let file = std::fs::File::open(&guest).expect("open coreutils guest");
    lzma_rs::xz_decompress(&mut std::io::BufReader::new(file), &mut wasm)
        .expect("decompress coreutils guest");
    let engine = wasmtime::Engine::new(&engine_config()).expect("engine");
    let cwasm = engine
        .precompile_module(&wasm)
        .expect("precompile coreutils guest");
    std::fs::write(&out, cwasm).expect("write coreutils.cwasm");
}
