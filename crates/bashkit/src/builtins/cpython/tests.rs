//! Unit tests for the CPython builtin plumbing (limits, trap mapping).

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
fn trap_messages_are_display_only() {
    let e = wasmtime::Error::from(wasmtime::Trap::StackOverflow);
    assert_eq!(
        trap_message(&e),
        "python3: fatal error: stack overflow in the interpreter\n"
    );
    let e = wasmtime::Error::from(wasmtime::Trap::UnreachableCodeReached);
    assert!(trap_message(&e).contains("aborted"));
    let long = wasmtime::Error::msg("x".repeat(4096));
    assert!(trap_message(&long).len() < 600);
}

#[test]
fn runtime_loads() {
    CPython::warm_up().expect("embedded CPython loads");
}

#[test]
fn on_demand_fallback_links() {
    // The path taken when the pooled engine cannot reserve address space.
    let rt = link(Engine::new(&runtime_config(false)).unwrap(), None).unwrap();
    assert!(rt.slots.is_none());
}

#[test]
fn pooled_engine_links() {
    let rt = link(Engine::new(&runtime_config(true)).unwrap(), None).unwrap();
    drop(rt);
}
