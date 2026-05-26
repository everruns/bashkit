//! Regression test for #274: Pipeline stdin not forwarded to user-defined functions

use bashkit::Bash;
use std::path::Path;

#[tokio::test]
async fn issue_274_pipeline_stdin_to_function() {
    let mut bash = Bash::new();
    let r = bash
        .exec("to_upper() { tr '[:lower:]' '[:upper:]'; }\necho hello | to_upper")
        .await
        .unwrap();
    assert_eq!(r.stdout.trim(), "HELLO");
}

#[tokio::test]
async fn issue_274_pipeline_stdin_to_sourced_function() {
    let mut bash = Bash::new();
    let fs = bash.fs();
    fs.write_file(
        Path::new("/lib.sh"),
        b"to_upper() { tr '[:lower:]' '[:upper:]'; }",
    )
    .await
    .unwrap();
    let r = bash
        .exec("source /lib.sh\necho hello world | to_upper")
        .await
        .unwrap();
    assert_eq!(r.stdout.trim(), "HELLO WORLD");
}

#[tokio::test]
async fn issue_274_pipeline_stdin_not_leaked_after_function_error() {
    let limits = bashkit::ExecutionLimits::new().max_commands(3);
    let mut bash = Bash::builder().limits(limits).build();

    let first = bash
        .exec("leak() { read x; echo $x; }\necho SECRET | leak")
        .await;
    assert!(
        first.is_err(),
        "expected first exec to fail due command limit"
    );

    let second = bash.exec("read x; echo ${x:-EMPTY}").await.unwrap();
    assert_eq!(second.stdout.trim(), "EMPTY");
}
