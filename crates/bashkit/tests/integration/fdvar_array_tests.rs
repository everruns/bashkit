//! `{arr[i]}` as the fd-variable of a redirect.
//!
//! The name lexes as several literal parts, because `[` opens a glob bracket,
//! so a redirect written that way used to parse as a command named
//! `{arr[1]}` and fail with `command not found`.

use bashkit::Bash;

async fn run(script: &str) -> bashkit::ExecResult {
    Bash::builder().build().exec(script).await.unwrap()
}

#[tokio::test]
async fn an_array_element_can_hold_the_fd_of_an_exec_redirect() {
    let r = run("declare -a A\nA[1]=7\nexec {A[1]}> o.txt\necho hi >&\"${A[1]}\"\nexec {A[1]}>&-\ncat o.txt").await;
    assert_eq!(r.stderr.to_string(), "");
    assert_eq!(r.stdout.to_string(), "hi\n");
}

#[tokio::test]
async fn closing_an_array_held_fd_is_not_a_command() {
    let r = run("declare -a A\nA[1]=7\nexec {A[1]}>&-\necho \"rc=$?\"").await;
    assert_eq!(r.stderr.to_string(), "");
    assert_eq!(r.stdout.to_string(), "rc=0\n");
}

#[tokio::test]
async fn a_subscript_can_be_a_variable() {
    let r = run("declare -a A\ni=1\nA[1]=7\nexec {A[i]}> o.txt\necho hi >&\"${A[i]}\"\nexec {A[i]}>&-\ncat o.txt").await;
    assert_eq!(r.stderr.to_string(), "");
    assert_eq!(r.stdout.to_string(), "hi\n");
}

/// A word that only looks like one is still a command.
#[tokio::test]
async fn a_brace_word_that_is_not_an_fd_var_stays_a_command() {
    let r = run("{A-1}> o.txt").await;
    let stderr = r.stderr.to_string();
    assert!(stderr.contains("command not found"), "stderr: {stderr}");
}
