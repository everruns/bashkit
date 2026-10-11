//! CPython 3.14 compiled to `wasm32-wasip1`, pre-initialized with Wizer and
//! precompiled to Wasmtime's Pulley bytecode at build time.
//!
//! This crate only ships bytes and the matching engine configuration. The
//! WASI host that maps the guest onto a virtual filesystem lives in
//! `bashkit` (`cpython` feature). See `knowledge/runtimes/cpython-wasm.md`.
//!
//! # Guest contract
//!
//! - Export `bashkit_run() -> i32` runs one `python3` invocation and returns
//!   its exit status. Argv, environment (including `PWD`) and stdio come from
//!   WASI preview1 imports. `proc_exit` may also end the call.
//! - The snapshot was taken with exactly one preopened directory, `/`, at
//!   file descriptor 3. Hosts must present the same preopen.
//! - The stdlib must be readable at [`STDLIB_ZIP_PATH`] with exactly the
//!   bytes of [`STDLIB_ZIP`]; the snapshot caches its zip directory.
//! - Each instance must serve a single call; state is never reused.

include!("config.rs");

/// CPython version inside the guest.
pub const PYTHON_VERSION: &str = "3.14.8";

/// Guest path the stdlib zip must be served at.
pub const STDLIB_ZIP_PATH: &str = "/usr/local/lib/python314.zip";

/// Guest directory that holds the stdlib (read-only to the guest).
pub const STDLIB_PREFIX: &str = "/usr/local/lib";

/// Stdlib sources, deflate-compressed zip read by CPython's `zipimport`.
pub static STDLIB_ZIP: &[u8] = include_bytes!("../artifacts/python314.zip");

// Wasmtime maps data segments from immutable host-page-aligned bytes. Keep
// 64 KiB alignment for every supported host. Mach-O only preserves host-page
// alignment through linking/ASLR; do not form an over-aligned Rust reference
// there. Cache a padded, aligned copy once when the embedded slice needs one.
const MODULE_ALIGNMENT: usize = 65536;

#[cfg(not(target_vendor = "apple"))]
#[repr(C, align(65536))]
struct Aligned<T: ?Sized>(T);

#[cfg(not(target_vendor = "apple"))]
static CWASM: &Aligned<[u8]> = &Aligned(*include_bytes!(concat!(env!("OUT_DIR"), "/python.cwasm")));
#[cfg(target_vendor = "apple")]
static CWASM: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/python.cwasm"));

struct PaddedModule {
    // Retain the Vec allocation: converting to a boxed slice can relocate it
    // after we chose the aligned offset. No mutation is exposed after copying.
    storage: Vec<u8>,
    start: usize,
    len: usize,
}

impl PaddedModule {
    fn new(bytes: &[u8]) -> Self {
        let mut storage = vec![0; bytes.len() + MODULE_ALIGNMENT - 1];
        let start = storage.as_ptr().align_offset(MODULE_ALIGNMENT);
        storage[start..start + bytes.len()].copy_from_slice(bytes);
        Self {
            storage,
            start,
            len: bytes.len(),
        }
    }

    fn bytes(&self) -> &[u8] {
        &self.storage[self.start..self.start + self.len]
    }
}

/// Precompiled module bytes (Pulley, or native with the `native` feature) (for diagnostics and size reporting).
/// The returned slice is immutable and 64 KiB aligned for the process lifetime.
pub fn cwasm_bytes() -> &'static [u8] {
    #[cfg(not(target_vendor = "apple"))]
    let bytes: &[u8] = &CWASM.0;
    #[cfg(target_vendor = "apple")]
    let bytes = CWASM;
    if bytes.as_ptr().align_offset(MODULE_ALIGNMENT) == 0 {
        return bytes;
    }
    static PADDED: std::sync::OnceLock<PaddedModule> = std::sync::OnceLock::new();
    PADDED.get_or_init(|| PaddedModule::new(bytes)).bytes()
}

/// Create an engine compatible with the embedded module.
pub fn engine() -> wasmtime::Result<wasmtime::Engine> {
    wasmtime::Engine::new(&engine_config())
}

/// Load the embedded CPython module into `engine`.
///
/// `engine` must come from [`engine`] or use [`engine_config`] unchanged
/// (plus runtime-only settings).
///
/// The first load in a process maps the aligned bytes in place. On hosts whose
/// linker/loader cannot preserve 64 KiB alignment, these bytes are copied once
/// into cached aligned storage first.
/// Wasmtime registers code by address, so the same bytes cannot back two live
/// modules; every later load copies them instead (tens of milliseconds).
/// Hosts should load once and share the module.
pub fn load_module(engine: &wasmtime::Engine) -> wasmtime::Result<wasmtime::Module> {
    static MAPPED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    let bytes = cwasm_bytes();
    // Native code must live in executable memory, never the read-only static,
    // so it is always copied (the first load costs ~35 ms more than Pulley's).
    if cfg!(feature = "native") || MAPPED.swap(true, std::sync::atomic::Ordering::AcqRel) {
        // SAFETY: same provenance as below; `deserialize` copies the bytes.
        return unsafe { wasmtime::Module::deserialize(engine, bytes) };
    }
    // SAFETY: the bytes were produced by `Engine::precompile_module` in this
    // crate's build script with the same wasmtime version and `engine_config`,
    // live in immutable process-lifetime storage, and are
    // mapped by at most one module (guarded by `MAPPED`).
    unsafe { wasmtime::Module::deserialize_raw(engine, std::ptr::NonNull::from(bytes)) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn module_loads_and_exports_entrypoint() {
        let engine = engine().unwrap();
        let module = load_module(&engine).unwrap();
        assert!(module.get_export("bashkit_run").is_some());
        assert!(module.get_export("memory").is_some());
    }

    #[test]
    fn repeated_loads_coexist() {
        let a = load_module(&engine().unwrap()).unwrap();
        let b = load_module(&engine().unwrap()).unwrap();
        assert!(a.get_export("bashkit_run").is_some() && b.get_export("bashkit_run").is_some());
    }

    #[test]
    fn module_bytes_are_page_aligned() {
        assert_eq!(cwasm_bytes().as_ptr() as usize % 65536, 0);
    }

    #[test]
    fn padded_module_preserves_bytes_and_alignment() {
        for len in [0, 1, 127, 65535, 65536, 65537] {
            let input: Vec<_> = (0..len).map(|i| (i % 251) as u8).collect();
            let module = PaddedModule::new(&input);
            assert_eq!(module.bytes().as_ptr() as usize % 65536, 0);
            assert_eq!(module.bytes(), input);
        }
    }

    #[test]
    fn stdlib_zip_is_a_zip() {
        assert_eq!(&STDLIB_ZIP[..4], b"PK\x03\x04");
    }
}
