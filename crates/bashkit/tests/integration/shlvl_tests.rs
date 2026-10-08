//! `$SHLVL` in child shells: the warning bash prints when the level would pass
//! 999, and that a child's stderr honors the `bash` command's own redirects.
//! Stdout behavior lives in `spec_cases/bash/shlvl.test.sh`.

use bashkit::Bash;

#[tokio::test]
async fn shlvl_too_high_warns_and_resets() {
    let mut bash = Bash::new();
    let r = bash
        .exec("SHLVL=999 bash -c 'echo \"g=$SHLVL\"; true'")
        .await
        .unwrap();
    assert_eq!(r.stdout, "g=1\n");
    assert_eq!(
        r.stderr,
        "bash: warning: shell level (1000) too high, resetting to 1\n"
    );
}

#[tokio::test]
async fn shlvl_in_range_does_not_warn() {
    let mut bash = Bash::new();
    let r = bash
        .exec("SHLVL=998 bash -c 'echo \"g=$SHLVL\"; true'")
        .await
        .unwrap();
    assert_eq!(r.stdout, "g=999\n");
    assert_eq!(r.stderr, "");
}

#[tokio::test]
async fn shlvl_warning_follows_the_commands_redirect() {
    let mut bash = Bash::new();
    let r = bash
        .exec("SHLVL=999 bash -c 'true' 2>/tmp/err; cat /tmp/err")
        .await
        .unwrap();
    assert_eq!(
        r.stdout,
        "bash: warning: shell level (1000) too high, resetting to 1\n"
    );
    assert_eq!(r.stderr, "");
}

#[tokio::test]
async fn child_shell_stderr_honors_its_redirect_when_streaming() {
    let mut bash = Bash::new();
    let streamed = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
    let sink = std::sync::Arc::clone(&streamed);
    let r = bash
        .exec_streaming(
            "bash -c 'echo leak >&2; true' 2>/dev/null; echo next",
            Box::new(move |_stdout, stderr| {
                sink.lock().unwrap().push_str(&stderr.to_string());
            }),
        )
        .await
        .unwrap();
    assert_eq!(r.stdout, "next\n");
    assert_eq!(r.stderr, "");
    assert_eq!(streamed.lock().unwrap().as_str(), "");
}
