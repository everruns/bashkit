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

/// An `exec` redirect set inside a subshell applies to the subshell's own
/// output. It used to apply to nothing at all.
#[tokio::test]
async fn exec_inside_a_subshell_redirects_the_subshell() {
    let mut bash = Bash::builder().build();
    let r = bash
        .exec("( exec > log.txt; echo inside )\necho sep\ncat log.txt")
        .await
        .unwrap();
    assert_eq!(r.stdout.to_string(), "sep\ninside\n");
}

#[tokio::test]
async fn output_before_the_subshell_exec_still_reaches_the_caller() {
    let mut bash = Bash::builder().build();
    let r = bash
        .exec("( echo pre; exec > log.txt; echo post )\necho sep\ncat log.txt")
        .await
        .unwrap();
    assert_eq!(r.stdout.to_string(), "pre\nsep\npost\n");
}

#[tokio::test]
async fn a_subshell_can_redirect_its_stderr_with_exec() {
    let mut bash = Bash::builder().build();
    let r = bash
        .exec("( exec 2>err.txt; ls /nope )\ncat err.txt")
        .await
        .unwrap();
    assert_eq!(r.stderr.to_string(), "");
    assert_eq!(
        r.stdout.to_string(),
        "ls: cannot access '/nope': No such file or directory\n"
    );
}

#[tokio::test]
async fn closing_stdout_inside_a_subshell_names_the_failing_command() {
    let mut bash = Bash::builder().build();
    let r = bash.exec("( exec 1>&- ; echo x )").await.unwrap();
    assert_eq!(r.stdout.to_string(), "");
    assert_eq!(
        r.stderr.to_string(),
        "bash: line 1: echo: write error: Bad file descriptor\n"
    );
    assert_eq!(r.exit_code, 1);
}
