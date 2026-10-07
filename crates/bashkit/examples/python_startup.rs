//! Raw python3 start time in a fresh process: Monty vs CPython (wasm).
//!
//! Prints one line per run with the wall time of the first `python3` call in
//! this process (includes any one-time runtime load) and of a second call.
//! `scripts/bench-python.sh` runs it repeatedly, each run a new OS process.
//!
//! Run: cargo run --release --example python_startup --features python,cpython -- cpython

use bashkit::Bash;
use std::time::Instant;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let runtime = std::env::args().nth(1).unwrap_or_else(|| "cpython".into());
    let start = Instant::now();
    let mut bash = match runtime.as_str() {
        "monty" => Bash::builder()
            .python()
            .env("BASHKIT_ALLOW_INPROCESS_PYTHON", "1")
            .build(),
        "cpython" => Bash::builder().cpython().build(),
        other => panic!("unknown runtime {other}; use monty or cpython"),
    };
    let first = bash.exec("python3 -c 'print(1)'").await.unwrap();
    let first_ms = start.elapsed().as_secs_f64() * 1e3;
    assert_eq!(first.stdout, "1\n", "{}", first.stderr);
    let t = Instant::now();
    let second = bash.exec("python3 -c 'print(2)'").await.unwrap();
    let second_ms = t.elapsed().as_secs_f64() * 1e3;
    assert_eq!(second.stdout, "2\n");
    println!("runtime={runtime} first_call_ms={first_ms:.3} second_call_ms={second_ms:.3}");
}
