// Shared by build.rs (via `include!`) and the runtime so the precompiled
// module and the loading engine always agree on compile-affecting settings.

/// Wasmtime configuration the embedded module is compiled for.
///
/// Decisions:
/// - `pulley64`: portable interpreter bytecode, one artifact for every 64-bit
///   little-endian host; no native code generation or executable memory.
/// - Fuel: lets the host meter guest work and yield cooperatively.
/// - Copy-on-write memory init with a dense image: the snapshot's ~40 MB
///   linear memory is mapped, not copied, per instance.
/// - 1 GiB memory reservation, 64 KiB guard: see below.
pub fn engine_config() -> wasmtime::Config {
    let mut config = wasmtime::Config::new();
    config
        .target("pulley64")
        .expect("pulley64 target is always available with the `pulley` feature");
    config.consume_fuel(true);
    config.memory_init_cow(true);
    config.memory_guaranteed_dense_image_size(128 << 20);
    // Pulley checks every memory access explicitly, so large virtual
    // reservations and guard regions buy nothing. Small ones keep many
    // concurrent instances (and pooled slots) cheap in address space.
    config.memory_reservation(1 << 30);
    config.memory_guard_size(64 << 10);
    config
}
