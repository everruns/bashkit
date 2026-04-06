// Scaffold tests for the jq_fuzz target.
// Validates that the jq builtin handles arbitrary filter expressions and
// malformed JSON without panicking.

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
async fn jq_valid_filter() {
    let mut bash = fuzz_bash();
    let result = bash.exec("echo '{\"a\":1}' | jq '.a'").await.unwrap();
    assert_eq!(result.stdout.trim(), "1");
}

#[tokio::test]
async fn jq_malformed_json() {
    let mut bash = fuzz_bash();
    let _ = bash.exec("echo 'not json' | jq '.' 2>/dev/null").await;
    // Must not panic
}

#[tokio::test]
async fn jq_invalid_filter() {
    let mut bash = fuzz_bash();
    let _ = bash.exec("echo '{}' | jq '.[[[[[' 2>/dev/null; true").await;
    // Must not panic
}

#[tokio::test]
async fn jq_deeply_nested_filter() {
    let mut bash = fuzz_bash();
    let filter = ".a".repeat(50);
    let script = format!("echo '{{}}' | jq '{}' 2>/dev/null; true", filter);
    let _ = bash.exec(&script).await;
    // Must not panic or hang
}

#[tokio::test]
async fn jq_null_bytes_in_input() {
    let mut bash = fuzz_bash();
    let _ = bash
        .exec("printf '{\"a\":\\x00}' | jq '.' 2>/dev/null; true")
        .await;
    // Must not panic
}
