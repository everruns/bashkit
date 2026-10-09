//! Unit tests for the CPython builtin plumbing (limits, runtime linking).

use super::*;

#[test]
fn default_limits() {
    let l = CPythonLimits::default();
    assert_eq!(l.common.max_duration, Duration::from_secs(30));
    assert_eq!(l.common.max_memory, 256 * 1024 * 1024);
    assert_eq!(l.common.max_call_depth, 1000);
    assert_eq!(l.max_output, 16 * 1024 * 1024);
}

#[test]
fn limit_setters() {
    let l = CPythonLimits::default()
        .max_duration(Duration::from_secs(2))
        .max_memory(64 << 20)
        .max_recursion(77)
        .max_output(10);
    assert_eq!(l.common.max_duration, Duration::from_secs(2));
    assert_eq!(l.common.max_memory, 64 << 20);
    assert_eq!(l.common.max_call_depth, 77);
    assert_eq!(l.max_output, 10);
}

#[test]
fn runtime_loads() {
    CPython::warm_up().expect("embedded CPython loads");
}

#[test]
fn on_demand_fallback_links() {
    // The path taken when the pooled engine cannot reserve address space.
    let config = wasi_host::runtime_config(bashkit_cpython_wasm::engine_config(), &POOL, false);
    let rt = GuestRuntime::link(
        wasmtime::Engine::new(&config).unwrap(),
        None,
        usize::MAX,
        bashkit_cpython_wasm::load_module,
        http::add_to_linker,
    )
    .unwrap();
    assert!(!rt.pooled());
}

#[test]
fn pooled_engine_links() {
    let config = wasi_host::runtime_config(bashkit_cpython_wasm::engine_config(), &POOL, true);
    let rt = GuestRuntime::link(
        wasmtime::Engine::new(&config).unwrap(),
        None,
        POOL.max_memory,
        bashkit_cpython_wasm::load_module,
        http::add_to_linker,
    )
    .unwrap();
    drop(rt);
}
