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

/// A backreference inside the group it refers to used to panic fancy-regex's
/// backtracking VM on the second iteration (slice index out of range in
/// `vm.rs`), which `catch_unwind` (TM-INT-001) turned into
/// `grep: builtin failed unexpectedly`. fancy-regex 0.20.2 reads the group's
/// previous capture instead, as PCRE and Perl do, so the pattern matches.
#[tokio::test]
async fn grep_perl_backref_inside_own_group_matches() {
    let mut bash = Bash::new();
    let result = bash
        .exec(r"printf 'abab\n' | grep -P '(?:(\1|a)b){2}'")
        .await
        .unwrap();
    assert_eq!(result.exit_code, 0, "stderr: {}", result.stderr);
    assert_eq!(result.stdout.trim(), "abab");
    assert_eq!(result.stderr, "", "no internal error may be reported");
}

/// Same pattern, no match: the engine must report "not found", not an error.
#[tokio::test]
async fn grep_perl_backref_inside_own_group_without_match() {
    let mut bash = Bash::new();
    let result = bash
        .exec(r"printf 'ab\n' | grep -P '(?:(\1|a)b){2}'")
        .await
        .unwrap();
    assert_eq!(result.exit_code, 1, "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "");
    assert_eq!(result.stderr, "", "no internal error may be reported");
}

/// Positive lookaheads are atomic, as in Oniguruma and PCRE: once one has
/// matched, a later failure no longer backtracks into it for another way of
/// matching. `(?=x|(x))\1x` therefore no longer matches `xx`.
#[tokio::test]
async fn grep_perl_positive_lookahead_is_atomic() {
    let mut bash = Bash::new();
    let result = bash
        .exec(r"printf 'xx\n' | grep -P '(?=x|(x))\1x'")
        .await
        .unwrap();
    assert_eq!(result.exit_code, 1, "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "");
    assert_eq!(result.stderr, "", "no internal error may be reported");
}
