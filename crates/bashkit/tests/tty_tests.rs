//! Tests for [[ -t fd ]] terminal detection

use bashkit::Bash;

/// Issue #799: -t defaults to false in sandbox
#[tokio::test]
async fn tty_defaults_to_false() {
    let mut bash = Bash::new();
    let result = bash
        .exec("[[ -t 0 ]] && echo yes || echo no")
        .await
        .unwrap();
    assert_eq!(result.stdout.trim(), "no");
}

/// -t can be configured via builder
#[tokio::test]
async fn tty_configurable_via_builder() {
    let mut bash = Bash::builder().tty(0, true).tty(1, true).build();
    let result = bash
        .exec("[[ -t 0 ]] && echo stdin_tty || echo stdin_no")
        .await
        .unwrap();
    assert_eq!(result.stdout.trim(), "stdin_tty");

    let result = bash
        .exec("[[ -t 1 ]] && echo stdout_tty || echo stdout_no")
        .await
        .unwrap();
    assert_eq!(result.stdout.trim(), "stdout_tty");

    // fd 2 not configured, should be false
    let result = bash
        .exec("[[ -t 2 ]] && echo stderr_tty || echo stderr_no")
        .await
        .unwrap();
    assert_eq!(result.stdout.trim(), "stderr_no");
}

/// test builtin [ -t ] also works
#[tokio::test]
async fn tty_test_builtin_bracket() {
    let mut bash = Bash::builder().tty(1, true).build();
    let result = bash.exec("[ -t 1 ] && echo yes || echo no").await.unwrap();
    assert_eq!(result.stdout.trim(), "yes");
}
