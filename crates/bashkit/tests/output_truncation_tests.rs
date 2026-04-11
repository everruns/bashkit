//! Tests for stdout/stderr output capture size limits (issue #648)

use bashkit::Bash;

async fn run(script: &str) -> bashkit::ExecResult {
    let mut bash = Bash::new();
    bash.exec(script).await.unwrap()
}

async fn run_with_limits(
    script: &str,
    max_stdout: usize,
    max_stderr: usize,
) -> bashkit::ExecResult {
    let limits = bashkit::ExecutionLimits::new()
        .max_stdout_bytes(max_stdout)
        .max_stderr_bytes(max_stderr);
    let mut bash = Bash::builder().limits(limits).build();
    bash.exec(script).await.unwrap()
}

// --- Default behavior: no truncation ---

#[tokio::test]
async fn no_truncation_by_default() {
    let result = run("echo hello").await;
    assert_eq!(result.stdout, "hello\n");
    assert!(!result.stdout_truncated);
    assert!(!result.stderr_truncated);
}

// --- stdout truncation ---

#[tokio::test]
async fn stdout_truncated_when_exceeds_limit() {
    // 10-byte limit; "hello\n" is 6 bytes, "world\n" is 6 bytes = 12 total
    let result = run_with_limits("echo hello; echo world", 10, 1_048_576).await;
    assert_eq!(result.stdout.len(), 10);
    assert!(result.stdout.starts_with("hello\n"));
    assert!(result.stdout_truncated);
    assert!(!result.stderr_truncated);
}

#[tokio::test]
async fn stdout_not_truncated_when_within_limit() {
    let result = run_with_limits("echo hi", 100, 1_048_576).await;
    assert_eq!(result.stdout, "hi\n");
    assert!(!result.stdout_truncated);
}

#[tokio::test]
async fn stdout_exact_limit_not_truncated() {
    // "hi\n" is 3 bytes, limit is 3
    let result = run_with_limits("echo hi", 3, 1_048_576).await;
    assert_eq!(result.stdout, "hi\n");
    assert!(!result.stdout_truncated);
}

#[tokio::test]
async fn stdout_one_byte_over_limit_truncated() {
    // "hi\n" is 3 bytes, limit is 2
    let result = run_with_limits("echo hi", 2, 1_048_576).await;
    assert_eq!(result.stdout.len(), 2);
    assert_eq!(result.stdout, "hi");
    assert!(result.stdout_truncated);
}

// --- stderr truncation ---

#[tokio::test]
async fn stderr_truncated_when_exceeds_limit() {
    let result = run_with_limits("echo err1 >&2; echo err2 >&2", 1_048_576, 8).await;
    assert!(result.stderr.len() <= 8);
    assert!(result.stderr_truncated);
}

#[tokio::test]
async fn stderr_not_truncated_when_within_limit() {
    let result = run_with_limits("echo oops >&2", 1_048_576, 100).await;
    assert!(!result.stderr_truncated);
}

// --- Execution continues after truncation ---

#[tokio::test]
async fn execution_continues_after_stdout_truncation() {
    // Script sets a variable after producing output.
    // Even though stdout is truncated, the script should finish.
    let result = run_with_limits(r#"echo aaa; echo bbb; x=done; echo "$x""#, 5, 1_048_576).await;
    assert!(result.stdout_truncated);
    // Script still ran to completion (exit code 0)
    assert_eq!(result.exit_code, 0);
}

// --- Minimal limit ---

#[tokio::test]
async fn minimal_stdout_limit_truncates_immediately() {
    // 0 is treated as "use default" per #1181, so use 1 for minimal limit
    let result = run_with_limits("echo hello", 1, 1_048_576).await;
    assert!(result.stdout.len() <= 1);
    assert!(result.stdout_truncated);
}

// --- Both truncated ---

#[tokio::test]
async fn both_stdout_and_stderr_truncated() {
    let result = run_with_limits("echo out; echo err >&2", 3, 3).await;
    assert!(result.stdout_truncated);
    assert!(result.stderr_truncated);
    assert!(result.stdout.len() <= 3);
    assert!(result.stderr.len() <= 3);
}

// --- Default limits ---

#[tokio::test]
async fn default_limits_are_1mb() {
    let limits = bashkit::ExecutionLimits::default();
    assert_eq!(limits.max_stdout_bytes, 1_048_576);
    assert_eq!(limits.max_stderr_bytes, 1_048_576);
}

// --- Builder ---

#[tokio::test]
async fn builder_sets_limits() {
    let limits = bashkit::ExecutionLimits::new()
        .max_stdout_bytes(500)
        .max_stderr_bytes(300);
    assert_eq!(limits.max_stdout_bytes, 500);
    assert_eq!(limits.max_stderr_bytes, 300);
}
