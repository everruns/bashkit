//! Bashkit as a Hyperlight-Wasm guest component.
//!
//! Important decisions:
//! - Target is `wasm32-unknown-unknown` with **no WASI and no JS imports**.
//!   Hyperlight-Wasm guests are built that way (see its `Justfile`), and the
//!   micro-VM has neither a JS engine nor an OS. Every outside dependency is an
//!   explicit import in `wit/bashkit.wit`.
//! - Clock and entropy come from host imports. Bashkit's normal
//!   `wasm32-unknown-unknown` config uses `web-time` + `getrandom`'s `wasm_js`
//!   backend, both of which need JS; here they are replaced by
//!   `host::now-micros` and `host::random-bytes`.
//! - Execution is driven with a one-shot poll (`now_or_never`). The guest runs
//!   over the in-memory VFS with no async host calls, so the interpreter future
//!   never suspends — the same invariant the browser package's `executeSync`
//!   relies on.

wit_bindgen::generate!({
    path: "wit",
    world: "sandbox",
});

use exports::bashkit::sandbox::shell::{ExecResult, Guest};
use futures_util::FutureExt;

/// Entropy for `getrandom`, routed to the host instead of JS.
#[unsafe(no_mangle)]
unsafe extern "Rust" fn __getrandom_v03_custom(dest: *mut u8, len: usize) -> Result<(), getrandom::Error> {
    let bytes = crate::bashkit::sandbox::host::random_bytes(len as u32);
    if bytes.len() < len {
        return Err(getrandom::Error::UNEXPECTED);
    }
    // SAFETY: caller guarantees `dest` is valid for `len` bytes.
    unsafe { core::ptr::copy_nonoverlapping(bytes.as_ptr(), dest, len) };
    Ok(())
}

/// Clock for `bashkit::time_compat::host_clock`, routed to the host.
#[unsafe(no_mangle)]
extern "Rust" fn __bashkit_host_now_micros() -> u64 {
    crate::bashkit::sandbox::host::now_micros()
}

struct Component;

impl Guest for Component {
    fn exec(script: String) -> ExecResult {
        let mut bash = ::bashkit::Bash::new();
        match bash.exec(&script).now_or_never() {
            Some(Ok(result)) => ExecResult {
                stdout: result.stdout.to_string(),
                stderr: result.stderr.to_string(),
                exit_code: result.exit_code,
            },
            Some(Err(err)) => ExecResult {
                stdout: String::new(),
                stderr: format!("{err}\n"),
                exit_code: 1,
            },
            // Only reachable if a script suspends, which needs an async host
            // call this world does not expose.
            None => ExecResult {
                stdout: String::new(),
                stderr: "bashkit: execution suspended, no async host calls in this world\n".into(),
                exit_code: 1,
            },
        }
    }
}

export!(Component);
