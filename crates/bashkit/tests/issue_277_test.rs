//! Regression test for #277: jq stderr output cannot be suppressed via 2>/dev/null

use bashkit::Bash;

#[tokio::test]
async fn issue_277_jq_stderr_suppressed() {
    let mut bash = Bash::new();
    let r = bash
        .exec(r#"echo "not json" | jq '.foo' 2>/dev/null; echo "exit=$?""#)
        .await
        .unwrap();
    // stderr should be suppressed by 2>/dev/null; stdout should only contain exit code line
    assert!(
        !r.stdout.contains("error"),
        "jq error should be suppressed, stdout={:?}",
        r.stdout
    );
    assert!(
        r.stdout.contains("exit="),
        "should see exit code echo, stdout={:?}",
        r.stdout
    );
}
