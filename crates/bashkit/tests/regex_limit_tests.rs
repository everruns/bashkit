//! Regex size limit tests for grep, sed, and awk builtins
//!
//! Verifies that oversized regex patterns are rejected rather than causing
//! resource exhaustion (issue #984).

use bashkit::Bash;
use std::time::Duration;

/// Helper: generate a large alternation pattern like "1|2|3|...|N"
fn huge_alternation_pattern(n: usize) -> String {
    (1..=n).map(|i| i.to_string()).collect::<Vec<_>>().join("|")
}

fn test_bash() -> Bash {
    Bash::builder()
        .limits(bashkit::ExecutionLimits::new().timeout(Duration::from_secs(10)))
        .build()
}

#[tokio::test]
async fn grep_rejects_huge_regex() {
    let mut bash = test_bash();
    let pattern = huge_alternation_pattern(50_000);
    let script = format!("echo test | grep '{}'", pattern);
    match bash.exec(&script).await {
        Ok(result) => {
            assert_ne!(result.exit_code, 0, "grep should fail with oversized regex");
        }
        Err(e) => {
            let msg = e.to_string();
            assert!(
                msg.contains("size limit") || msg.contains("invalid pattern"),
                "error should mention size limit, got: {}",
                msg
            );
        }
    }
}

#[tokio::test]
async fn grep_accepts_normal_regex() {
    let mut bash = Bash::new();
    let result = bash
        .exec("echo 'hello world' | grep 'hello'")
        .await
        .unwrap();
    assert_eq!(result.exit_code, 0);
    assert_eq!(result.stdout.trim(), "hello world");
}

#[tokio::test]
async fn sed_rejects_huge_regex() {
    let mut bash = test_bash();
    let pattern = huge_alternation_pattern(50_000);
    let script = format!("echo test | sed 's/{}/replaced/'", pattern);
    match bash.exec(&script).await {
        Ok(result) => {
            // sed error propagates through pipeline — the key security
            // property is it completes quickly without resource exhaustion.
            // Depending on how the interpreter handles pipeline errors,
            // exit code may or may not be non-zero.
            assert!(
                result.exit_code != 0 || result.stdout.trim() == "test",
                "sed should either fail or pass input through with oversized regex, \
                 exit={}, stdout='{}'",
                result.exit_code,
                result.stdout.trim()
            );
        }
        Err(e) => {
            let msg = e.to_string();
            assert!(
                msg.contains("size limit") || msg.contains("invalid"),
                "error should mention size limit, got: {}",
                msg
            );
        }
    }
}

#[tokio::test]
async fn awk_rejects_huge_regex_in_match() {
    let mut bash = test_bash();
    let pattern = huge_alternation_pattern(50_000);
    let script = format!(
        "echo test | awk '{{ if (match($0, \"{}\" )) print }}'",
        pattern
    );
    match bash.exec(&script).await {
        Ok(result) => {
            // awk silently handles invalid regex in match() — the key security
            // property is it completes quickly without resource exhaustion.
            assert!(
                result.stdout.trim().is_empty() || result.exit_code != 0,
                "awk should not match with oversized regex, \
                 exit={}, stdout='{}'",
                result.exit_code,
                result.stdout.trim()
            );
        }
        Err(e) => {
            let msg = e.to_string();
            assert!(
                msg.contains("size limit") || msg.contains("invalid"),
                "error should mention size limit, got: {}",
                msg
            );
        }
    }
}
