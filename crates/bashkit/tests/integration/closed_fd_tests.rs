//! Writes to a descriptor closed with `exec N>&-`.
//!
//! Bash keeps the descriptor closed and every later write to it fails with
//! `write error: Bad file descriptor`, named after the command that tried,
//! and the command's status is 1. The output is dropped, not re-routed.

use bashkit::Bash;

async fn run(script: &str) -> bashkit::ExecResult {
    Bash::builder().build().exec(script).await.unwrap()
}

#[tokio::test]
async fn writing_to_a_closed_stdout_reports_and_fails() {
    let r = run("exec 1>&-\necho x").await;
    assert_eq!(r.stdout.to_string(), "");
    assert_eq!(
        r.stderr.to_string(),
        "bash: line 2: echo: write error: Bad file descriptor\n"
    );
    assert_eq!(r.exit_code, 1);
}

#[tokio::test]
async fn the_failing_command_names_itself() {
    let r = run("exec 1>&-\nprintf 'hi\\n'").await;
    assert_eq!(r.stdout.to_string(), "");
    assert_eq!(
        r.stderr.to_string(),
        "bash: line 2: printf: write error: Bad file descriptor\n"
    );
    assert_eq!(r.exit_code, 1);
}

#[tokio::test]
async fn a_closed_stderr_swallows_the_report_but_keeps_the_status() {
    let r = run("exec 1>&- 2>&-\necho x").await;
    assert_eq!(r.stdout.to_string(), "");
    assert_eq!(r.stderr.to_string(), "");
    assert_eq!(r.exit_code, 1);
}

#[tokio::test]
async fn closing_stderr_alone_still_lets_stdout_through() {
    let r = run("exec 2>&-\necho x\necho y >&2").await;
    assert_eq!(r.stdout.to_string(), "x\n");
    assert_eq!(r.stderr.to_string(), "");
    assert_eq!(r.exit_code, 1);
}
