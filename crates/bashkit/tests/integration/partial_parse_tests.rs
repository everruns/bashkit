//! Bash reads a script line by line: commands before a syntax error run, then
//! the error is reported with exit status 2. A syntax error used to discard
//! the whole script, so agents lost all earlier work in a long heredoc'd
//! script. Commands on the error's own line never run, as in bash.

use bashkit::Bash;

#[tokio::test]
async fn commands_before_error_line_run() {
    let mut bash = Bash::builder().build();
    let r = bash
        .exec("echo first\necho second\nif then\necho never")
        .await
        .unwrap();
    assert_eq!(r.stdout, "first\nsecond\n");
    assert_eq!(r.exit_code, 2);
    assert!(r.stderr.contains("syntax error"), "stderr: {}", r.stderr);
}

#[tokio::test]
async fn same_line_commands_do_not_run() {
    let mut bash = Bash::builder().build();
    let err = bash.exec("echo a; if then").await;
    assert!(err.is_err(), "single-line syntax error stays a hard error");
}

#[tokio::test]
async fn unterminated_compound_at_eof() {
    let mut bash = Bash::builder().build();
    let r = bash.exec("echo a\nfoo() {\necho x\n").await.unwrap();
    assert_eq!(r.stdout, "a\n");
    assert_eq!(r.exit_code, 2);
}

#[tokio::test]
async fn side_effects_before_error_persist() {
    let mut bash = Bash::builder().build();
    let r = bash
        .exec("mkdir -p /w && echo data > /w/f\nX=1\nfor x in; do\n")
        .await
        .unwrap();
    assert_eq!(r.exit_code, 2);
    let r = bash.exec("cat /w/f; echo $X").await.unwrap();
    assert_eq!(r.stdout, "data\n1\n");
}

#[tokio::test]
async fn exit_before_error_wins() {
    let mut bash = Bash::builder().build();
    let r = bash.exec("echo a\nexit 3\nif then").await.unwrap();
    assert_eq!(r.stdout, "a\n");
    assert_eq!(r.exit_code, 3);
    assert!(!r.stderr.contains("syntax error"));
}

#[tokio::test]
async fn errexit_before_error_wins() {
    let mut bash = Bash::builder().build();
    let r = bash.exec("set -e\nfalse\nif then").await.unwrap();
    assert_eq!(r.exit_code, 1);
    assert!(!r.stderr.contains("syntax error"));
}

#[tokio::test]
async fn exit_trap_runs_after_syntax_error() {
    let mut bash = Bash::builder().build();
    let r = bash
        .exec("trap 'echo bye' EXIT\necho hi\nif then")
        .await
        .unwrap();
    assert_eq!(r.stdout, "hi\nbye\n");
    assert_eq!(r.exit_code, 2);
}

#[tokio::test]
async fn nested_bash_c_runs_prefix() {
    let mut bash = Bash::builder().build();
    let r = bash
        .exec("bash -c $'echo in\\nif then'; echo rc=$?")
        .await
        .unwrap();
    assert_eq!(r.stdout, "in\nrc=2\n");
}

#[tokio::test]
async fn bash_n_still_rejects_whole_script() {
    let mut bash = Bash::builder().build();
    let r = bash
        .exec("bash -n -c $'echo in\\nif then'; echo rc=$?")
        .await
        .unwrap();
    assert_eq!(r.stdout, "rc=2\n");
}
