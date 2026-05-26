// Scaffold tests for the jq_fuzz target.
// Validates that the jq builtin handles arbitrary filter expressions and
// malformed JSON without panicking AND without leaking Debug shapes,
// host paths, or the host-env canary into stderr/stdout.
//
// Requires the `jq` feature (gated in #1223).
#![cfg(feature = "jq")]

use bashkit::testing::{fuzz_exec, fuzz_init};
use bashkit::{Bash, ExecutionLimits};

fn fuzz_bash() -> Bash {
    fuzz_init();
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
    fuzz_exec(
        &mut bash,
        "echo '{\"a\":1}' | jq '.a'",
        "jq_valid_filter",
        &[],
    )
    .await;
}

#[tokio::test]
async fn jq_malformed_json() {
    let mut bash = fuzz_bash();
    fuzz_exec(
        &mut bash,
        "echo 'not json' | jq '.'",
        "jq_malformed_json",
        &[],
    )
    .await;
}

#[tokio::test]
async fn jq_invalid_filter() {
    let mut bash = fuzz_bash();
    fuzz_exec(
        &mut bash,
        "echo '{}' | jq '.[[[['",
        "jq_invalid_filter",
        &[],
    )
    .await;
}

#[tokio::test]
async fn jq_deeply_nested_filter() {
    let mut bash = fuzz_bash();
    let filter = ".a".repeat(50);
    let script = format!("echo '{{}}' | jq '{}'", filter);
    fuzz_exec(&mut bash, &script, "jq_deeply_nested_filter", &[]).await;
}

#[tokio::test]
async fn jq_null_bytes_in_input() {
    let mut bash = fuzz_bash();
    fuzz_exec(
        &mut bash,
        "printf '{\"a\":\\x00}' | jq '.'",
        "jq_null_bytes_in_input",
        &[],
    )
    .await;
}
