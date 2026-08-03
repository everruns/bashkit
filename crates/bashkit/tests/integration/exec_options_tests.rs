//! Tests for the unified `Bash::exec_with_options` entry point.
//!
//! `exec_with_options` carries streaming + per-call extensions as fields of a
//! single `ExecOptions` request, and the older `exec*` methods are thin wrappers
//! over it. These tests pin that the request struct is wired through correctly.

use bashkit::{Bash, ExecOptions};
use std::sync::{Arc, Mutex};

#[tokio::test]
async fn exec_with_options_default_matches_exec() {
    let mut bash = Bash::new();
    let result = bash
        .exec_with_options("echo hello", ExecOptions::new())
        .await
        .unwrap();
    assert_eq!(result.exit_code, 0);
    assert_eq!(result.stdout, "hello\n");
}

#[tokio::test]
async fn exec_with_options_streams_output() {
    let chunks: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let chunks_cb = chunks.clone();
    let mut bash = Bash::new();

    let result = bash
        .exec_with_options(
            "for i in 1 2 3; do echo $i; done",
            ExecOptions::new().streaming(Box::new(move |stdout, _stderr| {
                if !stdout.is_empty() {
                    chunks_cb.lock().unwrap().push(stdout.to_string());
                }
            })),
        )
        .await
        .unwrap();

    assert_eq!(result.stdout, "1\n2\n3\n");
    assert_eq!(*chunks.lock().unwrap(), vec!["1\n", "2\n", "3\n"]);
}

#[tokio::test]
async fn exec_with_options_callback_cleared_after_call() {
    let count = Arc::new(Mutex::new(0usize));
    let count_cb = count.clone();
    let mut bash = Bash::new();

    bash.exec_with_options(
        "echo streamed",
        ExecOptions::new().streaming(Box::new(move |stdout, _stderr| {
            if !stdout.is_empty() {
                *count_cb.lock().unwrap() += 1;
            }
        })),
    )
    .await
    .unwrap();

    let before = *count.lock().unwrap();
    assert!(
        before > 0,
        "callback should have fired during streaming call"
    );

    // A subsequent plain exec must not invoke the previous call's callback.
    bash.exec("echo after").await.unwrap();
    assert_eq!(
        *count.lock().unwrap(),
        before,
        "streaming callback must not persist past its exec_with_options call"
    );
}

// ---------------------------------------------------------------------------
// Positional parameters
// ---------------------------------------------------------------------------

#[tokio::test]
async fn positional_params_are_visible_to_the_script() {
    let mut bash = Bash::new();
    let result = bash
        .exec_with_options(
            r#"echo "$#|$1|$2|$@""#,
            ExecOptions::new().positional(["alpha", "beta"]),
        )
        .await
        .unwrap();
    assert_eq!(result.stdout, "2|alpha|beta|alpha beta\n");
}

#[tokio::test]
async fn arg0_sets_dollar_zero() {
    let mut bash = Bash::new();
    let result = bash
        .exec_with_options(r#"echo "$0""#, ExecOptions::new().arg0("deploy.sh"))
        .await
        .unwrap();
    assert_eq!(result.stdout, "deploy.sh\n");
}

#[tokio::test]
async fn dollar_zero_defaults_to_bash() {
    let mut bash = Bash::new();
    let result = bash.exec(r#"echo "$0""#).await.unwrap();
    assert_eq!(result.stdout, "bash\n");
}

#[tokio::test]
async fn positional_params_support_shift_and_loops() {
    let mut bash = Bash::new();
    let result = bash
        .exec_with_options(
            "shift; for a in \"$@\"; do echo $a; done",
            ExecOptions::new().positional(["one", "two", "three"]),
        )
        .await
        .unwrap();
    assert_eq!(result.stdout, "two\nthree\n");
}

#[tokio::test]
async fn positional_params_do_not_leak_into_the_next_exec() {
    let mut bash = Bash::new();
    bash.exec_with_options(r#"echo "$1""#, ExecOptions::new().positional(["kept"]))
        .await
        .unwrap();

    let result = bash.exec(r#"echo "[$1] $# [$0]""#).await.unwrap();
    assert_eq!(result.stdout, "[] 0 [bash]\n");
}

#[tokio::test]
async fn positional_params_do_not_leak_after_a_failed_exec() {
    let mut bash = Bash::new();
    let failed = bash
        .exec_with_options(
            "echo start; false; exit 4",
            ExecOptions::new().arg0("x.sh").positional(["leaky"]),
        )
        .await
        .unwrap();
    assert_eq!(failed.exit_code, 4);

    let result = bash.exec(r#"echo "[$1] $#""#).await.unwrap();
    assert_eq!(result.stdout, "[] 0\n");
}

#[tokio::test]
async fn empty_positional_list_clears_dollar_hash() {
    let mut bash = Bash::new();
    let result = bash
        .exec_with_options(
            r#"echo "$#""#,
            ExecOptions::new().positional(Vec::<String>::new()),
        )
        .await
        .unwrap();
    assert_eq!(result.stdout, "0\n");
}

#[tokio::test]
async fn script_can_override_positional_params_with_set() {
    let mut bash = Bash::new();
    let result = bash
        .exec_with_options(
            r#"set -- x y; echo "$# $1""#,
            ExecOptions::new().positional(["a", "b", "c"]),
        )
        .await
        .unwrap();
    assert_eq!(result.stdout, "2 x\n");
}

// ---------------------------------------------------------------------------
// Stdin
// ---------------------------------------------------------------------------

#[tokio::test]
async fn stdin_feeds_cat() {
    let mut bash = Bash::new();
    let result = bash
        .exec_with_options("cat", ExecOptions::new().stdin("piped\nlines\n"))
        .await
        .unwrap();
    assert_eq!(result.stdout, "piped\nlines\n");
}

#[tokio::test]
async fn stdin_feeds_read() {
    let mut bash = Bash::new();
    let result = bash
        .exec_with_options(
            r#"read -r line; echo "got=$line""#,
            ExecOptions::new().stdin("hello world\n"),
        )
        .await
        .unwrap();
    assert_eq!(result.stdout, "got=hello world\n");
}

#[tokio::test]
async fn stdin_is_consumed_by_a_filter_pipeline() {
    let mut bash = Bash::new();
    let result = bash
        .exec_with_options(
            "grep -c line",
            ExecOptions::new().stdin("one line\ntwo line\nnope\n"),
        )
        .await
        .unwrap();
    assert_eq!(result.stdout.trim(), "2");
}

#[tokio::test]
async fn an_inline_pipe_wins_over_supplied_stdin() {
    let mut bash = Bash::new();
    let result = bash
        .exec_with_options("echo inline | cat", ExecOptions::new().stdin("outer\n"))
        .await
        .unwrap();
    assert_eq!(result.stdout, "inline\n");
}

#[tokio::test]
async fn stdin_does_not_leak_into_the_next_exec() {
    let mut bash = Bash::new();
    bash.exec_with_options("cat", ExecOptions::new().stdin("first\n"))
        .await
        .unwrap();

    let result = bash.exec("cat").await.unwrap();
    assert_eq!(result.stdout, "");
}

#[tokio::test]
async fn stdin_and_positional_params_combine() {
    let mut bash = Bash::new();
    let result = bash
        .exec_with_options(
            r#"read -r line; echo "$1:$line""#,
            ExecOptions::new().positional(["tag"]).stdin("body\n"),
        )
        .await
        .unwrap();
    assert_eq!(result.stdout, "tag:body\n");
}

#[tokio::test]
async fn set_dashdash_does_not_clobber_dollar_zero() {
    // Positional params live in a call frame; the synthetic frame `set --`
    // creates at top level must keep `$0` at the default shell name.
    let mut bash = Bash::new();
    let result = bash.exec(r#"set -- a b; echo "$0 $# $1""#).await.unwrap();
    assert_eq!(result.stdout, "bash 2 a\n");
}

#[tokio::test]
async fn set_dashdash_keeps_a_supplied_arg0() {
    let mut bash = Bash::new();
    let result = bash
        .exec_with_options(
            r#"set -- x; echo "$0 $1""#,
            ExecOptions::new().arg0("run.sh").positional(["a", "b"]),
        )
        .await
        .unwrap();
    assert_eq!(result.stdout, "run.sh x\n");
}
