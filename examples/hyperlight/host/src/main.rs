//! Run the bashkit guest component under wasmtime.
//!
//! Usage: cargo run --release -- <component.wasm> [script]

use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Result;
use wasmtime::component::{Component, Linker, ResourceTable};
use wasmtime::{Config, Engine, Store};

wasmtime::component::bindgen!({
    path: "../wit",
    world: "sandbox",
});

struct HostState {
    _table: ResourceTable,
}

impl bashkit::sandbox::host::Host for HostState {
    fn now_micros(&mut self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_micros() as u64)
            .unwrap_or(0)
    }

    fn random_bytes(&mut self, len: u32) -> Vec<u8> {
        let mut buf = vec![0u8; len as usize];
        getrandom::fill(&mut buf).expect("host entropy");
        buf
    }
}

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let path = args.next().unwrap_or_else(|| {
        "../target/bashkit-sandbox.component.wasm".to_string()
    });
    let script = args.next().unwrap_or_else(|| "echo hello from the sandbox".to_string());

    let engine = Engine::new(Config::new().wasm_component_model(true))?;
    let component = Component::from_file(&engine, &path)?;

    let mut linker = Linker::new(&engine);
    Sandbox::add_to_linker::<_, wasmtime::component::HasSelf<_>>(&mut linker, |state| state)?;

    let mut store = Store::new(
        &engine,
        HostState {
            _table: ResourceTable::new(),
        },
    );
    let sandbox = Sandbox::instantiate(&mut store, &component, &linker)?;

    let result = sandbox
        .bashkit_sandbox_shell()
        .call_exec(&mut store, &script)?;

    print!("{}", result.stdout);
    eprint!("{}", result.stderr);
    std::process::exit(result.exit_code);
}
