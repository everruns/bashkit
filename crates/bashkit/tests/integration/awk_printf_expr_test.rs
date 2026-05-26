//! Test for awk printf accepting expressions as format string

use bashkit::Bash;

/// Issue #810: awk printf should accept expressions (not just string literals)
#[tokio::test]
async fn awk_printf_expression_format() {
    let mut bash = Bash::new();
    let result = bash
        .exec(r#"echo "my-project" | awk '{for(i=1;i<=NF;i++) printf substr($i,1,1)}'"#)
        .await
        .unwrap();
    assert_eq!(result.stdout.trim(), "m");
}

/// Printf with string literal should still work
#[tokio::test]
async fn awk_printf_string_literal() {
    let mut bash = Bash::new();
    let result = bash
        .exec(r#"echo test | awk '{printf "%s\n", $1}'"#)
        .await
        .unwrap();
    assert_eq!(result.stdout.trim(), "test");
}
