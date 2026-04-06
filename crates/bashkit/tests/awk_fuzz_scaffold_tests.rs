// Scaffold tests for the awk_fuzz target.
// Validates that the awk builtin handles arbitrary programs and input
// data without panicking.

use bashkit::{Bash, ExecutionLimits};

fn fuzz_bash() -> Bash {
    Bash::builder()
        .limits(
            ExecutionLimits::new()
                .max_commands(50)
                .max_subst_depth(3)
                .max_stdout_bytes(4096)
                .max_stderr_bytes(4096)
                .timeout(std::time::Duration::from_secs(2)),
        )
        .build()
}

#[tokio::test]
async fn awk_valid_program() {
    let mut bash = fuzz_bash();
    let result = bash.exec("echo 'a b c' | awk '{print $2}'").await.unwrap();
    assert_eq!(result.stdout.trim(), "b");
}

#[tokio::test]
async fn awk_invalid_program() {
    let mut bash = fuzz_bash();
    let _ = bash.exec("echo 'x' | awk '{{{{{' 2>/dev/null; true").await;
    // Must not panic
}

#[tokio::test]
async fn awk_begin_end() {
    let mut bash = fuzz_bash();
    let result = bash
        .exec("echo 'x' | awk 'BEGIN{print \"start\"} END{print \"end\"}'")
        .await
        .unwrap();
    assert!(result.stdout.contains("start"));
    assert!(result.stdout.contains("end"));
}

#[tokio::test]
async fn awk_regex_pattern() {
    let mut bash = fuzz_bash();
    let _ = bash
        .exec("echo 'hello' | awk '/[[[/' 2>/dev/null; true")
        .await;
    // Must not panic on malformed regex
}

#[tokio::test]
async fn awk_field_separator() {
    let mut bash = fuzz_bash();
    let result = bash
        .exec("echo 'a:b:c' | awk -F: '{print $2}'")
        .await
        .unwrap();
    assert_eq!(result.stdout.trim(), "b");
}
