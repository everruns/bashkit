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

/// A compound producer streams into the next stage through a bounded pipe;
/// large output arrives intact and in order.
#[tokio::test]
async fn large_producer_output_is_intact() {
    let mut bash = Bash::new();
    let result = bash
        .exec("{ seq 1 100000; seq 1 100000; } | wc -l; { seq 1 3; echo err >&2; } | tail -1")
        .await
        .unwrap();
    assert_eq!(result.stdout, "200000\n3\n");
    assert_eq!(result.stderr, "err\n");
}

/// Stages of a function producer run as a forked stage: small output finishes
/// before `head` reads, so the producer exits 0 like bash usually does.
#[tokio::test]
async fn finite_producer_is_not_killed() {
    let mut bash = Bash::new();
    let result = bash
        .exec("f() { for i in 1 2 3; do echo $i; done; }; f | head -1; echo \"${PIPESTATUS[*]}\"")
        .await
        .unwrap();
    assert_eq!(result.stdout, "1\n0 0\n");
}

/// With concurrent jobs off, stages keep running one after another.
#[tokio::test]
async fn sequential_mode_keeps_buffered_stages() {
    let mut bash = Bash::builder().concurrent_jobs(false).build();
    let result = bash
        .exec("for i in 1 2 3; do echo $i; done | tac")
        .await
        .unwrap();
    assert_eq!(result.stdout, "3\n2\n1\n");
}

/// TM-DOS-124: a stage that never reads ends an endless producer; nothing
/// buffers without bound.
#[tokio::test]
async fn threat_endless_producer_into_non_reader_ends() {
    let mut bash = Bash::new();
    let result = bash
        .exec("while :; do echo x; done | true; echo \"${PIPESTATUS[*]}\"")
        .await
        .unwrap();
    assert_eq!(result.stdout, "141 0\n");
}

/// TM-DOS-124: recursive pipelines stop at a depth limit instead of
/// forking without bound. Concurrent nesting is capped (deeper levels run
/// stages in sequence), so the stack cost stays near plain function
/// recursion; the test thread gets headroom for unoptimized test builds.
#[test]
fn threat_recursive_pipeline_is_bounded() {
    let handle = std::thread::Builder::new()
        .stack_size(8 * 1024 * 1024)
        .spawn(|| {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(async {
                    let mut bash = Bash::new();
                    bash.exec("f() { f | f; }; f; echo after").await
                })
        })
        .unwrap();
    let result = handle.join().unwrap();
    let failed = match &result {
        Err(e) => e.to_string().contains("limit") || e.to_string().contains("depth"),
        Ok(r) => r.exit_code != 0 || r.stderr.contains("limit"),
    };
    assert!(failed, "{result:?}");
}
