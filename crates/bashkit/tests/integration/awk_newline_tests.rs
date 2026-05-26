//! Tests for awk newline handling as statement separators

use bashkit::Bash;

/// Issue #809: newlines between assignments should work as statement separators
#[tokio::test]
async fn awk_newline_separates_assignments() {
    let mut bash = Bash::new();
    let result = bash
        .exec(
            r#"echo test | awk '{
    x=1
    y=2
    print x, y
}'"#,
        )
        .await
        .unwrap();
    assert_eq!(result.stdout.trim(), "1 2");
}

/// Semicolons should still work
#[tokio::test]
async fn awk_semicolons_still_work() {
    let mut bash = Bash::new();
    let result = bash
        .exec(r#"echo test | awk '{ x=1; y=2; print x, y }'"#)
        .await
        .unwrap();
    assert_eq!(result.stdout.trim(), "1 2");
}

/// Assignment after if on separate line
#[tokio::test]
async fn awk_newline_after_if() {
    let mut bash = Bash::new();
    let result = bash
        .exec(
            r#"echo test | awk '{
    if (1) x=1
    y=2
    print x, y
}'"#,
        )
        .await
        .unwrap();
    assert_eq!(result.stdout.trim(), "1 2");
}
