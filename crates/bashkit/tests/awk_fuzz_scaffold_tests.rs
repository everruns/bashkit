// Scaffold tests for the awk_fuzz target.
// Validates that the awk builtin handles arbitrary programs and input
// data without panicking AND without leaking Debug shapes, host paths,
// or the host-env canary into stderr/stdout.

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
async fn awk_valid_program() {
    let mut bash = fuzz_bash();
    fuzz_exec(
        &mut bash,
        "echo 'a b c' | awk '{print $2}'",
        "awk_valid_program",
        &[],
    )
    .await;
}

#[tokio::test]
async fn awk_invalid_program() {
    let mut bash = fuzz_bash();
    fuzz_exec(
        &mut bash,
        "echo 'x' | awk '{{{{{ '",
        "awk_invalid_program",
        &[],
    )
    .await;
}

#[tokio::test]
async fn awk_begin_end() {
    let mut bash = fuzz_bash();
    fuzz_exec(
        &mut bash,
        "echo 'x' | awk 'BEGIN{print \"start\"} END{print \"end\"}'",
        "awk_begin_end",
        &[],
    )
    .await;
}

#[tokio::test]
async fn awk_regex_pattern() {
    let mut bash = fuzz_bash();
    fuzz_exec(
        &mut bash,
        "echo 'hello' | awk '/[[[/'",
        "awk_regex_pattern",
        &[],
    )
    .await;
}

#[tokio::test]
async fn awk_field_separator() {
    let mut bash = fuzz_bash();
    fuzz_exec(
        &mut bash,
        "echo 'a:b:c' | awk -F: '{print $2}'",
        "awk_field_separator",
        &[],
    )
    .await;
}
