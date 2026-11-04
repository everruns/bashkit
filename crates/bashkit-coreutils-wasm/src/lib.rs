//! [uutils/coreutils](https://github.com/uutils/coreutils) compiled to
//! `wasm32-wasip1` as one multicall guest, precompiled to Wasmtime's Pulley
//! bytecode at build time.
//!
//! This crate only ships bytes and the matching engine configuration. The
//! WASI host that maps the guest onto a virtual filesystem lives in
//! `bashkit` (`wasm-coreutils` feature). See
//! `knowledge/runtimes/wasm-coreutils.md`.
//!
//! # Guest contract
//!
//! - A WASI command: export `_start` runs one utility and ends with
//!   `proc_exit(status)`.
//! - `argv[0]` names the utility (one of [`UTILS`]); unknown names exit 127.
//! - `PWD` in the environment becomes the working directory.
//! - Paths resolve through exactly one preopened directory, `/`, at fd 3.
//! - Each instance serves a single call.

include!("config.rs");

/// uutils release the guest is built from.
pub const UUTILS_VERSION: &str = "0.12.0";

/// Utility names the guest dispatches on, sorted.
pub static UTILS: &[&str] = &include!(concat!(env!("OUT_DIR"), "/utils.rs"));

// Wasmtime maps data segments from immutable host-page-aligned bytes. Keep
// 64 KiB alignment for every supported host. Mach-O only preserves host-page
// alignment through linking/ASLR, and Windows COFF cannot encode a 64 KiB
// aligned static, so on those hosts a padded, aligned copy is cached once
// when the embedded slice needs one (same scheme as bashkit-cpython-wasm).
const MODULE_ALIGNMENT: usize = 65536;

#[cfg(not(any(target_vendor = "apple", windows)))]
#[repr(C, align(65536))]
struct Aligned<T: ?Sized>(T);

#[cfg(not(any(target_vendor = "apple", windows)))]
static CWASM: &Aligned<[u8]> = &Aligned(*include_bytes!(concat!(
    env!("OUT_DIR"),
    "/coreutils.cwasm"
)));
#[cfg(any(target_vendor = "apple", windows))]
static CWASM: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/coreutils.cwasm"));

struct PaddedModule {
    // Keep the Vec: converting to a boxed slice could relocate it after the
    // aligned offset was chosen. Nothing mutates it after the copy.
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

/// Precompiled module bytes (for diagnostics and size reporting). The slice is
/// immutable and 64 KiB aligned for the process lifetime.
pub fn cwasm_bytes() -> &'static [u8] {
    #[cfg(not(any(target_vendor = "apple", windows)))]
    let bytes: &[u8] = &CWASM.0;
    #[cfg(any(target_vendor = "apple", windows))]
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

/// Load the embedded coreutils module into `engine`.
///
/// `engine` must come from [`engine`] or use [`engine_config`] unchanged
/// (plus runtime-only settings). The first load in a process maps the
/// embedded bytes in place; later loads copy them. Hosts should load once and
/// share the module.
pub fn load_module(engine: &wasmtime::Engine) -> wasmtime::Result<wasmtime::Module> {
    static MAPPED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    let bytes = cwasm_bytes();
    if MAPPED.swap(true, std::sync::atomic::Ordering::AcqRel) {
        // SAFETY: same provenance as below; `deserialize` copies the bytes.
        return unsafe { wasmtime::Module::deserialize(engine, bytes) };
    }
    // SAFETY: the bytes were produced by `Engine::precompile_module` in this
    // crate's build script with the same wasmtime version and
    // `engine_config`, live in immutable process-lifetime storage, and are mapped by at most one module (guarded by `MAPPED`).
    unsafe { wasmtime::Module::deserialize_raw(engine, std::ptr::NonNull::from(bytes)) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn module_loads_and_is_a_command() {
        let engine = engine().unwrap();
        let module = load_module(&engine).unwrap();
        assert!(module.get_export("_start").is_some());
        assert!(module.get_export("memory").is_some());
    }

    #[test]
    fn repeated_loads_coexist() {
        let a = load_module(&engine().unwrap()).unwrap();
        let b = load_module(&engine().unwrap()).unwrap();
        assert!(a.get_export("_start").is_some() && b.get_export("_start").is_some());
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
    fn utils_are_sorted_and_cover_basics() {
        assert!(UTILS.windows(2).all(|w| w[0] < w[1]));
        for name in ["cat", "ls", "sort", "wc", "sha256sum"] {
            assert!(UTILS.contains(&name), "{name}");
        }
    }
}
