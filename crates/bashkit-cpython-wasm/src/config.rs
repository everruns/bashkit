// Shared by build.rs (via `include!`) and the runtime so the precompiled
// module and the loading engine always agree on compile-affecting settings.

/// Wasmtime configuration the embedded module is compiled for.
///
/// Decisions:
/// - `pulley64` (default): portable interpreter bytecode, one artifact for
///   every 64-bit little-endian host; no native code generation or executable
///   memory.
/// - `native` feature: machine code for the build target instead, ~2-3x faster
///   per call, but the host must allow executable memory and the first load
///   copies the module (no in-place mapping). Compiled for the target's
///   baseline ISA so a binary built on one machine runs on any CPU of that
///   architecture.
/// - Fuel: lets the host meter guest work and yield cooperatively.
/// - Copy-on-write memory init with a dense image: the snapshot's ~40 MB
///   linear memory is mapped, not copied, per instance.
/// - 1 GiB memory reservation, 64 KiB guard: see below.
pub fn engine_config() -> wasmtime::Config {
    engine_config_for(cfg!(feature = "native"), None)
}

/// [`engine_config`] for an explicit code kind; `target` is the triple to
/// compile native code for (`None`: the host). Used by the build script.
pub fn engine_config_for(native: bool, target: Option<&str>) -> wasmtime::Config {
    let mut config = wasmtime::Config::new();
    if !native {
        config
            .target("pulley64")
            .expect("pulley64 target is always available with the `pulley` feature");
    } else if let Some(target) = target {
        config.target(target).expect("native target supported by Cranelift");
    }
    config.consume_fuel(true);
    config.memory_init_cow(true);
    config.memory_guaranteed_dense_image_size(128 << 20);
    // Pulley checks every memory access explicitly, so large virtual
    // reservations and guard regions buy nothing. Native code keeps the same
    // small sizes (explicit bounds checks) so pooled slots stay identical and
    // cheap in address space.
    config.memory_reservation(1 << 30);
    config.memory_guard_size(64 << 10);
    config
}
