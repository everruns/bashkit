//! Tests for the unified `Bash::exec_with_options` entry point.
//!
//! `exec_with_options` carries streaming + per-call extensions as fields of a
//! single `ExecOptions` request, and the older `exec*` methods are thin wrappers
//! over it. These tests pin that the request struct is wired through correctly.

use bashkit::{Bash, ExecOptions};
use std::sync::{Arc, Mutex};

#[tokio::test]
async fn exec_with_options_default_matches_exec() {
    let mut bash = Bash::new();
    let result = bash
        .exec_with_options("echo hello", ExecOptions::new())
        .await
        .unwrap();
    assert_eq!(result.exit_code, 0);
    assert_eq!(result.stdout, "hello\n");
}

#[tokio::test]
async fn exec_with_options_streams_output() {
    let chunks: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let chunks_cb = chunks.clone();
    let mut bash = Bash::new();

    let result = bash
        .exec_with_options(
            "for i in 1 2 3; do echo $i; done",
            ExecOptions::new().streaming(Box::new(move |stdout, _stderr| {
                if !stdout.is_empty() {
                    chunks_cb.lock().unwrap().push(stdout.to_string());
                }
            })),
        )
        .await
        .unwrap();

    assert_eq!(result.stdout, "1\n2\n3\n");
    assert_eq!(*chunks.lock().unwrap(), vec!["1\n", "2\n", "3\n"]);
}

#[tokio::test]
async fn exec_with_options_callback_cleared_after_call() {
    let count = Arc::new(Mutex::new(0usize));
    let count_cb = count.clone();
    let mut bash = Bash::new();

    bash.exec_with_options(
        "echo streamed",
        ExecOptions::new().streaming(Box::new(move |stdout, _stderr| {
            if !stdout.is_empty() {
                *count_cb.lock().unwrap() += 1;
            }
        })),
    )
    .await
    .unwrap();

    let before = *count.lock().unwrap();
    assert!(
        before > 0,
        "callback should have fired during streaming call"
    );

    // A subsequent plain exec must not invoke the previous call's callback.
    bash.exec("echo after").await.unwrap();
    assert_eq!(
        *count.lock().unwrap(),
        before,
        "streaming callback must not persist past its exec_with_options call"
    );
}
