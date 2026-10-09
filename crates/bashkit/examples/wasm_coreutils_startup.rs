//! Raw start time of wasm coreutils in a fresh process.
//!
//! Prints the wall time of the first `coreutils` call in this process
//! (includes the one-time engine and module load), a second call, and the
//! native builtin for comparison. `scripts/bench-wasm-coreutils.sh` runs it
//! repeatedly, each run a new OS process.
//!
//! Run: cargo run --release --example wasm_coreutils_startup --features wasm-coreutils

use bashkit::Bash;
use std::time::Instant;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let start = Instant::now();
    let mut bash = Bash::builder().wasm_coreutils().build();
    let first = bash.exec("coreutils echo 1").await.unwrap();
    let first_ms = start.elapsed().as_secs_f64() * 1e3;
    assert_eq!(first.stdout, "1\n", "{}", first.stderr);
    let t = Instant::now();
    let second = bash.exec("coreutils echo 2").await.unwrap();
    let second_ms = t.elapsed().as_secs_f64() * 1e3;
    assert_eq!(second.stdout, "2\n");
    let t = Instant::now();
    let native = bash.exec("echo 3").await.unwrap();
    let native_ms = t.elapsed().as_secs_f64() * 1e3;
    assert_eq!(native.stdout, "3\n");
    println!(
        "first_call_ms={first_ms:.3} second_call_ms={second_ms:.3} native_call_ms={native_ms:.3}"
    );
}
