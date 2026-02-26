//! Regression test for #289: backslash line continuation fails in some contexts

use bashkit::Bash;

#[tokio::test]
async fn issue_289_backslash_continuation_if() {
    let mut bash = Bash::new();
    let r = bash
        .exec(
            "A=\"\"\nB=\"\"\nif [ -z \"$A\" ] || \\\n   [ -z \"$B\" ]; then\n    echo missing\nfi",
        )
        .await
        .unwrap();
    assert_eq!(r.stdout.trim(), "missing");
}

#[tokio::test]
async fn issue_289_backslash_continuation_command() {
    let mut bash = Bash::new();
    let r = bash.exec("echo hello \\\n    world").await.unwrap();
    assert_eq!(r.stdout.trim(), "hello world");
}
