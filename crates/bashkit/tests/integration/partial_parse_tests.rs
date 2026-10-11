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

/// A multiline token and long $0 must not multiply diagnostic storage.
#[tokio::test]
async fn child_syntax_report_is_bounded() {
    let script = format!("for x \"{}\"", "a\n".repeat(16));
    let request = format!("bash -c '{script}' '{}'", "x".repeat(256));
    let mut bash = Bash::new();
    let result = bash.exec(&request).await.unwrap();
    assert_eq!(result.exit_code, 2);
    assert!(result.stdout.is_empty());
    assert!(
        result.stderr.len() <= 1024,
        "report was {} bytes",
        result.stderr.len()
    );
    assert!(result.stderr.ends_with("[diagnostic truncated]\n"));
    assert_eq!(bash.exec("echo alive").await.unwrap().stdout, "alive\n");
}

#[tokio::test]
async fn small_child_syntax_reports_match_bash() {
    // Host /bin/bash varies by platform (macOS bash 3.2 reports `-c` input as
    // `line 0`, modern GNU bash reports `line 1`, matching our parser), so
    // compare the diagnostic shape with the line number normalized away.
    fn normalize_line(input: &[u8]) -> Vec<u8> {
        let mut out = input.to_vec();
        let mut i = 0;
        while i + 5 < out.len() {
            if out[i..].starts_with(b"line ") {
                let mut j = i + 5;
                while j < out.len() && out[j].is_ascii_digit() {
                    j += 1;
                }
                if j > i + 5 {
                    out.splice(i + 5..j, [b'#']);
                }
            }
            i += 1;
        }
        out
    }
    for script in ["if then fi", "echo kept\nif then", "for x ; wat"] {
        let expected = std::process::Command::new("bash")
            .args(["-c", script, "CASE"])
            .output()
            .unwrap();
        let mut bash = Bash::new();
        let result = bash
            .exec(&format!("bash -c '{script}' CASE"))
            .await
            .unwrap();
        assert_eq!(result.exit_code, expected.status.code().unwrap());
        assert_eq!(result.stdout.as_bytes(), expected.stdout);
        assert_eq!(
            normalize_line(result.stderr.as_bytes()),
            normalize_line(&expected.stderr),
            "script: {script:?}"
        );
    }
}
