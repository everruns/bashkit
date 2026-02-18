//! Streaming output example
//!
//! Demonstrates `exec_streaming` which delivers output incrementally
//! via a callback, while still returning the full result at the end.
//!
//! Run with: cargo run --example streaming_output

use bashkit::Bash;
use std::sync::{Arc, Mutex};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // --- Bash-level streaming ---
    println!("=== exec_streaming: for loop ===");
    let mut bash = Bash::new();
    let result = bash
        .exec_streaming(
            "for i in 1 2 3 4 5; do echo \"iteration $i\"; done",
            Box::new(|stdout, _stderr| {
                // Called after each loop iteration
                print!("[stream] {stdout}");
            }),
        )
        .await?;
    println!("--- final stdout ---\n{}", result.stdout);

    // --- Collecting chunks ---
    println!("=== exec_streaming: collecting chunks ===");
    let chunks: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let chunks_cb = chunks.clone();
    let mut bash = Bash::new();
    let result = bash
        .exec_streaming(
            "echo start; for x in a b c; do echo $x; done; echo end",
            Box::new(move |stdout, _stderr| {
                chunks_cb.lock().unwrap().push(stdout.to_string());
            }),
        )
        .await?;
    {
        let chunks = chunks.lock().unwrap();
        println!("Chunks received: {chunks:?}");
        // Concatenated chunks == full stdout
        let reassembled: String = chunks.iter().cloned().collect();
        assert_eq!(reassembled, result.stdout);
        println!("Reassembled matches final stdout: OK");
    }

    // --- Streaming stderr ---
    println!("\n=== exec_streaming: stderr ===");
    let mut bash = Bash::new();
    let result = bash
        .exec_streaming(
            "echo out1; echo err1 >&2; echo out2",
            Box::new(|stdout, stderr| {
                if !stdout.is_empty() {
                    print!("[stdout] {stdout}");
                }
                if !stderr.is_empty() {
                    eprint!("[stderr] {stderr}");
                }
            }),
        )
        .await?;
    println!("--- final stdout: {:?}", result.stdout);
    println!("--- final stderr: {:?}", result.stderr);

    // --- Non-streaming still works ---
    println!("\n=== exec (non-streaming) for comparison ===");
    let mut bash = Bash::new();
    let result = bash.exec("for i in 1 2 3; do echo $i; done").await?;
    println!("stdout: {}", result.stdout);

    Ok(())
}
