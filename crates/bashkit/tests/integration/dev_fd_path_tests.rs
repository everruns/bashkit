//! Redirections to `/dev/stdin`, `/dev/stdout`, `/dev/stderr`, `/dev/fd/N`.
//!
//! On Linux these paths alias the shell's current file descriptors, so
//! `echo err > /dev/stderr` behaves like `echo err >&2`. Bashkit resolves
//! them at the interpreter level (like `/dev/null`): they never reach the
//! filesystem, so no regular VFS file named `/dev/stderr` is ever created and
//! error messages are never silently swallowed.

use bashkit::Bash;
use std::path::Path;

async fn run(script: &str) -> (Bash, bashkit::ExecResult) {
    let mut bash = Bash::builder().build();
    let result = bash.exec(script).await.unwrap();
    (bash, result)
}

#[tokio::test]
async fn stdout_to_dev_stderr_goes_to_stderr() {
    let (bash, r) = run("echo err > /dev/stderr").await;
    assert_eq!(r.stdout, "");
    assert_eq!(r.stderr, "err\n");
    assert!(!bash.fs().exists(Path::new("/dev/stderr")).await.unwrap());
}

#[tokio::test]
async fn append_to_dev_stderr_goes_to_stderr() {
    let (bash, r) = run("echo a >> /dev/stderr; echo b >>/dev/stderr").await;
    assert_eq!(r.stderr, "a\nb\n");
    assert!(!bash.fs().exists(Path::new("/dev/stderr")).await.unwrap());
}

#[tokio::test]
async fn stderr_to_dev_stdout_goes_to_stdout() {
    let (bash, r) = run("echo err >&2 2> /dev/stdout; { echo e2 >&2; } 2>/dev/stdout").await;
    // `>&2 2>/dev/stdout`: fd1 dups the original fd2 first, so the first
    // line stays on stderr; the group's stderr is redirected to stdout.
    assert_eq!(r.stderr, "err\n");
    assert_eq!(r.stdout, "e2\n");
    assert!(!bash.fs().exists(Path::new("/dev/stdout")).await.unwrap());
}

#[tokio::test]
async fn dev_fd_numbers_alias_descriptors() {
    let (_, r) = run("echo one > /dev/fd/2; echo two > /dev/fd/1").await;
    assert_eq!(r.stderr, "one\n");
    assert_eq!(r.stdout, "two\n");
}

#[tokio::test]
async fn dev_fd_path_follows_exec_opened_descriptor() {
    let (bash, r) = run("exec 3>/tmp/log; echo hi > /dev/fd/3; exec 3>&-; cat /tmp/log").await;
    assert_eq!(r.stdout, "hi\n");
    assert!(!bash.fs().exists(Path::new("/dev/fd/3")).await.unwrap());
}

#[tokio::test]
async fn dev_stderr_variable_target() {
    let (_, r) = run("LOG=/dev/stderr; echo warn >> \"$LOG\"").await;
    assert_eq!(r.stdout, "");
    assert_eq!(r.stderr, "warn\n");
}

#[tokio::test]
async fn dev_stderr_respects_outer_redirect() {
    let (_, r) = run("f() { echo err > /dev/stderr; }; f 2>/dev/null; echo done").await;
    assert_eq!(r.stdout, "done\n");
    assert_eq!(r.stderr, "");
}

#[tokio::test]
async fn both_to_dev_stderr() {
    let (_, r) = run("{ echo out; echo err >&2; } &> /dev/stderr").await;
    assert_eq!(r.stdout, "");
    assert_eq!(r.stderr, "out\nerr\n");
}

#[tokio::test]
async fn mixed_dup_and_dev_stdout() {
    // Exercises the fd-table path: `2>&1` mixed with a /dev path target.
    // `2>&1` points fd2 at stdout, so `/dev/stderr` (fd2's current target)
    // is stdout too: everything lands on stdout, as on Linux with a pipe.
    let (_, r) = run("{ echo out; echo err >&2; } 2>&1 >/dev/stderr").await;
    assert_eq!(r.stdout, "out\nerr\n");
    assert_eq!(r.stderr, "");
}

#[tokio::test]
async fn exec_redirect_to_dev_stderr() {
    let (bash, r) = run("exec 3>/dev/stderr; echo via3 >&3; exec 3>&-").await;
    assert_eq!(r.stdout, "");
    assert_eq!(r.stderr, "via3\n");
    assert!(!bash.fs().exists(Path::new("/dev/stderr")).await.unwrap());
}

#[tokio::test]
async fn input_from_dev_stdin_keeps_stdin() {
    let (_, r) = run("echo piped | cat < /dev/stdin; cat < /dev/fd/0 <<< here").await;
    assert_eq!(r.stdout, "piped\nhere\n");
}

#[tokio::test]
async fn dot_dot_spelling_is_normalized() {
    let (bash, r) = run("echo err > /dev/../dev/stderr").await;
    assert_eq!(r.stderr, "err\n");
    assert!(!bash.fs().exists(Path::new("/dev/stderr")).await.unwrap());
}
