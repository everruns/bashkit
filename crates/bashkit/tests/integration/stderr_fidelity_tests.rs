//! Where diagnostics go: a command's own redirects apply to its own error
//! (`nocmd 2>/dev/null`, `return 2>&1`), stderr written inside `$(...)`
//! reaches the outer stderr, `eval` keeps the caller's `$LINENO`, and
//! `/proc` refuses new files like procfs.
//!
//! Every expected stdout/stderr pair was checked against real bash 5.2
//! running the same text as `bash SCRIPT` (`$0` is `SCRIPT`).

use bashkit::{Bash, ExecOptions};

/// Run `script` the way `bash SCRIPT` would: `$0` is `SCRIPT`.
async fn run(script: &str) -> (String, String, i32) {
    let mut bash = Bash::new();
    let r = bash
        .exec_with_options(script, ExecOptions::new().arg0("SCRIPT"))
        .await
        .unwrap();
    (r.stdout.to_string(), r.stderr.to_string(), r.exit_code)
}

#[tokio::test]
async fn command_not_found_honors_its_redirects() {
    let (out, err, _) = run("nocmdq 2>/dev/null\necho \"rc=$?\"\n").await;
    assert_eq!(out, "rc=127\n");
    assert_eq!(err, "");

    let (out, err, _) = run("nocmdq 2>&1\n").await;
    assert_eq!(out, "SCRIPT: line 1: nocmdq: command not found\n");
    assert_eq!(err, "");
}

#[tokio::test]
async fn missing_path_command_honors_its_redirects() {
    let (out, err, _) = run("./missingq 2>/dev/null\necho \"rc=$?\"\n").await;
    assert_eq!(out, "rc=127\n");
    assert_eq!(err, "");
}

#[tokio::test]
async fn toplevel_return_error_follows_its_redirects() {
    let (out, err, _) = run("return 2>&1; echo \"rc=$?\"\n").await;
    assert_eq!(
        out,
        "SCRIPT: line 1: return: can only `return' from a function or sourced script\nrc=2\n"
    );
    assert_eq!(err, "");
}

#[tokio::test]
async fn special_builtin_errors_follow_their_redirects() {
    let script = "source /nofileq 2>&1\n. 2>&1\ngetopts 2>&1\nbuiltin nocmdq 2>&1\n";
    let (out, err, _) = run(script).await;
    assert_eq!(
        out,
        "SCRIPT: line 1: /nofileq: No such file or directory\n\
         SCRIPT: line 2: .: filename argument required\n\
         .: usage: . filename [arguments]\n\
         getopts: usage: getopts optstring name [arg ...]\n\
         SCRIPT: line 4: builtin: nocmdq: not a shell builtin\n"
    );
    assert_eq!(err, "");
}

#[tokio::test]
async fn exit_reports_bad_arguments() {
    let (out, err, code) = run("exit abc 2>&1\necho after\n").await;
    assert_eq!(
        out,
        "SCRIPT: line 1: exit: abc: numeric argument required\n"
    );
    assert_eq!(err, "");
    assert_eq!(code, 2);

    let (out, err, code) = run("false\nexit\n").await;
    assert_eq!((out.as_str(), err.as_str(), code), ("", "", 1));

    let (out, err, _) = run("exit 1 2\necho \"rc=$?\"\n").await;
    assert_eq!(out, "rc=1\n");
    assert_eq!(err, "SCRIPT: line 1: exit: too many arguments\n");
}

#[tokio::test]
async fn command_substitution_stderr_reaches_outer_stderr() {
    let (out, err, _) = run("x=$(nocmdq)\necho \"x=[$x]\"\n").await;
    assert_eq!(out, "x=[]\n");
    assert_eq!(err, "SCRIPT: line 1: nocmdq: command not found\n");

    // Words are expanded before the command's redirects apply.
    let (out, err, _) = run("echo \"a$(nocmdq)b\" 2>/dev/null\n").await;
    assert_eq!(out, "ab\n");
    assert_eq!(err, "SCRIPT: line 1: nocmdq: command not found\n");

    // A function's call redirects do not cover its argument words either.
    let (out, err, _) = run("f() { echo \"f:$1\"; }\nf \"$(nocmdq)\" 2>/dev/null\n").await;
    assert_eq!(out, "f:\n");
    assert_eq!(err, "SCRIPT: line 2: nocmdq: command not found\n");

    // The substitution's own redirects do.
    let (out, err, _) = run("y=$(nocmdq 2>&1)\necho \"y=[$y]\"\n").await;
    assert_eq!(out, "y=[SCRIPT: line 1: nocmdq: command not found]\n");
    assert_eq!(err, "");

    // A compound command's redirects cover its own expansions.
    let (out, err, _) = run("{ w=$(nocmdq); } 2>/dev/null\necho done\n").await;
    assert_eq!(out, "done\n");
    assert_eq!(err, "");

    let (out, err, _) = run("r=$(echo out; echo err >&2)\necho \"r=[$r]\"\n").await;
    assert_eq!(out, "r=[out]\n");
    assert_eq!(err, "err\n");

    let (out, err, _) = run("echo $(echo $(nocmdq))x\n").await;
    assert_eq!(out, "x\n");
    assert_eq!(err, "SCRIPT: line 1: nocmdq: command not found\n");

    let (out, err, _) = run("q=$(< /nofileq)\necho \"rc=$?\"\n").await;
    assert_eq!(out, "rc=1\n");
    assert_eq!(err, "SCRIPT: line 1: /nofileq: No such file or directory\n");
}

#[tokio::test]
async fn eval_keeps_the_callers_lineno() {
    let (out, err, _) = run("#\n#\n#\neval 'echo $LINENO'\neval 'nocmdq'\n").await;
    assert_eq!(out, "4\n");
    assert_eq!(err, "SCRIPT: line 5: nocmdq: command not found\n");
}

#[tokio::test]
async fn proc_refuses_new_files() {
    let (out, err, _) = run("echo y > /proc/naopode 2>/dev/null; echo \"rc=$?\"\n").await;
    assert_eq!(out, "rc=1\n");
    assert_eq!(
        err,
        "SCRIPT: line 1: /proc/naopode: No such file or directory\n"
    );

    let (out, err, _) = run("mkdir /proc/d; touch /proc/t; [ -e /proc/t ] || echo absent\n").await;
    assert_eq!(out, "absent\n");
    assert_eq!(
        err,
        "mkdir: cannot create directory '/proc/d': No such file or directory\n\
         touch: cannot touch '/proc/t': No such file or directory\n"
    );
}
