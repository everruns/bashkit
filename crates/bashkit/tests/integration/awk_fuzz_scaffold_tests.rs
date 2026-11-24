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

// Regression: fuzz crash-a10f82c7 (awk_fuzz). A short option whose character
// after `-` is multi-byte made the option parser slice at byte index 2, which
// is not a char boundary, panicking instead of reporting an invalid option.
#[tokio::test]
async fn awk_multibyte_short_option_does_not_panic() {
    let mut bash = fuzz_bash();
    fuzz_exec(
        &mut bash,
        "echo 'a b c' | awk '-\u{b8}'",
        "awk_multibyte_short_option",
        &[],
    )
    .await;
}

#[tokio::test]
async fn awk_multibyte_short_option_reports_invalid_option() {
    let mut bash = Bash::new();
    let result = bash
        .exec("awk '-\u{b8}'")
        .await
        .expect("awk invalid option must not panic");
    assert_ne!(
        result.exit_code, 0,
        "awk must fail for an unknown short option"
    );
    let stderr = result.stderr.to_string();
    assert!(
        stderr.contains("invalid option") && stderr.contains('\u{b8}'),
        "expected an invalid-option diagnostic naming the character, got: {stderr:?}"
    );
}

// A multi-byte value attached to a real short option stays attached as its value.
#[tokio::test]
async fn awk_multibyte_attached_field_separator() {
    let mut bash = Bash::new();
    let result = bash
        .exec("printf 'a\u{a7}b\\n' | awk -F'\u{a7}' '{print $2}'")
        .await
        .expect("multi-byte -F value must parse");
    assert_eq!(
        result.exit_code,
        0,
        "stderr: {:?}",
        result.stderr.to_string()
    );
    assert_eq!(result.stdout.trim(), "b");
}
