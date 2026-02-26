//! Regression tests verifying issues #275, #279, #282 are not reproducible.
//!
//! These issues were reported but code review and testing confirm
//! they work correctly in the current implementation.

use bashkit::Bash;

async fn run(script: &str) -> String {
    let mut bash = Bash::builder().build();
    let result = bash.exec(script).await.expect("exec failed");
    result.stdout
}

/// Issue #275: Parser corrupts \( in single-quoted strings
/// Single-quoted strings should be completely literal.
#[tokio::test]
async fn test_issue_275_backslash_paren_in_single_quotes() {
    // \( inside single quotes should be passed literally to sed
    let output = run(r#"echo "test pattern here" | sed 's/\(pattern\)/[\1]/'"#).await;
    assert!(
        output.contains("[pattern]"),
        "Expected '[pattern]' in output, got: {}",
        output
    );
}

/// Issue #279: "done" as case pattern should work
#[tokio::test]
async fn test_issue_279_done_as_case_pattern() {
    let output = run(r#"
status="done"
case "$status" in
  done) echo "matched" ;;
  *) echo "nope" ;;
esac
"#)
    .await;
    assert_eq!(output.trim(), "matched");
}

/// Issue #279: Other reserved words as case patterns
#[tokio::test]
async fn test_issue_279_reserved_words_as_case_patterns() {
    let output = run(r#"
word="in"
case "$word" in
  in) echo "matched_in" ;;
  do) echo "matched_do" ;;
  *) echo "nope" ;;
esac
"#)
    .await;
    assert_eq!(output.trim(), "matched_in");
}

/// Issue #282: find -type f should enumerate VFS files
#[tokio::test]
async fn test_issue_282_find_type_f_vfs() {
    let mut bash = Bash::builder()
        .mount_text("/data/file1.txt", "hello")
        .mount_text("/data/file2.txt", "world")
        .mount_text("/data/subdir/file3.txt", "nested")
        .build();
    let result = bash
        .exec("find /data -type f | sort")
        .await
        .expect("exec failed");
    let lines: Vec<&str> = result.stdout.trim().lines().collect();
    assert!(
        lines.len() >= 3,
        "Expected at least 3 files, got {}: {:?}",
        lines.len(),
        lines
    );
    assert!(
        result.stdout.contains("file1.txt"),
        "Missing file1.txt in: {}",
        result.stdout
    );
    assert!(
        result.stdout.contains("file2.txt"),
        "Missing file2.txt in: {}",
        result.stdout
    );
    assert!(
        result.stdout.contains("file3.txt"),
        "Missing file3.txt in: {}",
        result.stdout
    );
}
