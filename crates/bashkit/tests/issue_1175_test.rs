//! Test for issue #1175: alias expansion should not bypass static budget validation.
//!
//! Aliases that expand to expensive constructs (huge brace ranges, deeply nested
//! loops) must be caught by the budget validator after expansion.

use bashkit::Bash;

/// THREAT[TM-DOS-031]: Alias expanding to huge brace range is caught.
/// The alias value is constructed from variables so the brace range literal
/// doesn't appear in the original AST — only in the expanded alias.
#[tokio::test]
async fn alias_huge_brace_range_rejected() {
    let mut bash = Bash::new();
    // Define and use alias in one exec, constructing the alias value
    // from variables so the static budget check on the initial AST doesn't
    // see the brace range.
    let result = bash
        .exec(
            r#"
shopt -s expand_aliases
x='echo {1..9999'
y='99999}'
eval "alias boom='$x$y'"
boom
"#,
        )
        .await
        .unwrap();
    // Either exit_code != 0 (budget rejection) or the alias wasn't expanded
    // If the alias DID expand, the brace range would generate ~1B elements
    // and either be caught by budget validation or hit runtime limits
    assert!(
        result.exit_code != 0 || result.stdout.len() < 1000, // If it passes, output should be tiny (not expanded)
        "should reject huge brace range via alias, got exit={} stdout_len={}",
        result.exit_code,
        result.stdout.len()
    );
}

/// Normal aliases should still work fine.
#[tokio::test]
async fn normal_alias_works() {
    let mut bash = Bash::new();
    let result = bash
        .exec(
            r#"
shopt -s expand_aliases
alias hi='echo hello'
hi world
"#,
        )
        .await
        .unwrap();
    assert_eq!(result.exit_code, 0);
    assert!(
        result.stdout.contains("hello world"),
        "expected 'hello world', got: {}",
        result.stdout
    );
}
