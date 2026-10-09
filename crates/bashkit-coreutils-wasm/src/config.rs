// Shared by build.rs (via `include!`) and the runtime so the precompiled
// module and the loading engine always agree on compile-affecting settings.

/// Wasmtime configuration the embedded module is compiled for.
///
/// Decisions:
/// - `pulley64`: portable interpreter bytecode, one artifact for every 64-bit
///   little-endian host; no native code generation or executable memory.
/// - Fuel: lets the host meter guest work and yield cooperatively.
/// - Copy-on-write memory init with a dense image: data segments are mapped,
///   not copied, per instance (without the dense image wasmtime falls back to
///   copying them on every instantiation).
/// - 256 MiB memory reservation, 64 KiB guard: Pulley bounds-checks every
///   access, so large reservations buy nothing; coreutils rarely need more
///   than a few MiB, and a smaller slot keeps the instance pool cheap in
///   address space.
pub fn engine_config() -> wasmtime::Config {
    let mut config = wasmtime::Config::new();
    config
        .target("pulley64")
        .expect("pulley64 target is always available with the `pulley` feature");
    config.consume_fuel(true);
    config.memory_init_cow(true);
    config.memory_guaranteed_dense_image_size(64 << 20);
    config.memory_reservation(256 << 20);
    config.memory_guard_size(64 << 10);
    config
}
