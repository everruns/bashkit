//! Pipeline execution: stage isolation and streaming output.
//!
//! Bash-parity cases live in `spec_cases/bash/pipes-redirects.test.sh`; these
//! cover what spec files cannot see, like the streaming output callback.

use bashkit::Bash;
use std::sync::{Arc, Mutex};

/// Output of a non-last stage feeds the next stage; it must never reach the
/// streaming callback directly (`for ... | tac` printed in input order).
#[tokio::test]
async fn non_last_stage_output_is_not_streamed() {
    let streamed = Arc::new(Mutex::new(String::new()));
    let sink = streamed.clone();
    let mut bash = Bash::new();
    let result = bash
        .exec_streaming(
            "for i in 1 2 3; do echo $i; done | tac\nf() { echo a; echo b; }; f | sort -r",
            Box::new(move |stdout, _stderr| {
                sink.lock().unwrap().push_str(&stdout.to_string());
            }),
        )
        .await
        .unwrap();
    assert_eq!(result.stdout, "3\n2\n1\nb\na\n");
    assert_eq!(*streamed.lock().unwrap(), "3\n2\n1\nb\na\n");
}

/// The last stage still streams as it runs.
#[tokio::test]
async fn last_stage_output_streams() {
    let chunks = Arc::new(Mutex::new(Vec::new()));
    let sink = chunks.clone();
    let mut bash = Bash::new();
    let result = bash
        .exec_streaming(
            "echo go | while read -r w; do echo $w-1; echo $w-2; done",
            Box::new(move |stdout, _stderr| {
                if !stdout.is_empty() {
                    sink.lock().unwrap().push(stdout.to_string());
                }
            }),
        )
        .await
        .unwrap();
    assert_eq!(result.stdout, "go-1\ngo-2\n");
    assert_eq!(chunks.lock().unwrap().concat(), "go-1\ngo-2\n");
}
