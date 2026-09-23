//! Regression: a builtin that cannot read an operand must report the reason
//! the way the real tool does — `No such file or directory`, not the `Display`
//! of `bashkit::Error`, whose `io error: ` prefix is a Rust enum shape no tool
//! prints (TM-INF-022, sibling of the `internal error:` leak fixed for the hex
//! dumpers).
//!
//! Most of these tools share `builtins::read_text_file` / `read_stream_file`,
//! so one leaking formatter reached all of them at once; `cat`, `head` and
//! `strings` read the VFS directly and leaked it separately.
//!
//! Expectations taken from GNU coreutils 9.8.

use bashkit::Bash;
use bashkit::testing::assert_no_leak;

/// Every tool here takes a file operand and reads it through the VFS.
/// The exit code is each tool's own: GNU `grep`, `sed` and `rg` report an
/// unreadable operand as 2, the coreutils text tools as 1.
const READERS: &[(&str, &str, i32)] = &[
    ("cat", "cat /nope", 1),
    ("head", "head /nope", 1),
    ("tail", "tail /nope", 1),
    ("strings", "strings /nope", 1),
    ("wc", "wc /nope", 1),
    ("sort", "sort /nope", 1),
    ("uniq", "uniq /nope", 1),
    ("cut", "cut -c1 /nope", 1),
    ("nl", "nl /nope", 1),
    ("awk", "awk '{print}' /nope", 1),
    ("paste", "paste /nope", 1),
    ("rev", "rev /nope", 1),
    ("tac", "tac /nope", 1),
    ("column", "column /nope", 1),
    ("split", "split /nope", 1),
    ("comm", "comm /nope /nope2", 1),
    ("shuf", "shuf /nope", 1),
    ("diff", "diff /nope /nope2", 1),
    ("dotenv", "dotenv /nope", 1),
    ("template", "template /nope", 1),
    ("json", "json get x /nope", 1),
    ("od", "od /nope", 1),
    ("xxd", "xxd /nope", 1),
    ("hexdump", "hexdump /nope", 1),
    ("base64", "base64 /nope", 1),
    ("fold", "fold /nope", 1),
    ("expand", "expand /nope", 1),
    ("grep", "grep x /nope", 2),
    ("sed", "sed s/a/b/ /nope", 2),
    ("rg", "rg foo /nope", 2),
];

#[tokio::test]
async fn missing_operand_reports_the_real_errno_text() {
    for (name, script, code) in READERS {
        let mut bash = Bash::new();
        let result = bash
            .exec(script)
            .await
            .unwrap_or_else(|e| panic!("{script:?} aborted the script: {e}"));
        let stderr = result.stderr.to_string();
        assert!(
            stderr.contains("No such file or directory"),
            "{name}: expected the real errno text, got {stderr:?}"
        );
        assert_eq!(result.exit_code, *code, "{name} exit code");
    }
}

/// The specific shape that regressed: the `Display` of `bashkit::Error`.
#[tokio::test]
async fn missing_operand_does_not_leak_the_error_enum_shape() {
    for (name, script, _) in READERS {
        let mut bash = Bash::new();
        let result = bash.exec(script).await.unwrap();
        let stderr = result.stderr.to_string();
        // The diagnostic belongs on stderr, never in the data stream.
        assert!(
            !result
                .stdout
                .to_string()
                .contains("No such file or directory"),
            "{name} put its diagnostic in stdout"
        );
        assert!(
            !stderr.contains("io error:"),
            "{name} leaks the Error enum shape: {stderr:?}"
        );
        assert!(
            !stderr.contains("internal error:"),
            "{name} leaks the Error enum shape: {stderr:?}"
        );
        // And the whole cross-tool banned list, so a reworded leak still fails.
        assert_no_leak(&result, name, &[]);
    }
}

/// A backend reason richer than its errno is kept: collapsing it to the bare
/// errno would drop the only actionable detail.
#[tokio::test]
async fn read_only_backend_reason_survives() {
    let mut bash = Bash::builder()
        .fs(std::sync::Arc::new(bashkit::ReadOnlyFs::new(
            std::sync::Arc::new(bashkit::InMemoryFs::new()),
        )))
        .build();
    let result = bash.exec("echo hi > /nope.txt").await.unwrap();
    assert_eq!(
        result.stderr.to_string(),
        "bash: /nope.txt: filesystem is read-only\n"
    );
}

/// A readable operand still works — the error path must not have swallowed
/// the success path.
#[tokio::test]
async fn readable_operand_still_reads() {
    let mut bash = Bash::builder().mount_text("/ok.txt", "hello\n").build();
    for script in ["cat /ok.txt", "wc -l /ok.txt", "head /ok.txt"] {
        let result = bash.exec(script).await.unwrap();
        assert_eq!(result.exit_code, 0, "{script} exit code");
        assert_eq!(result.stderr.to_string(), "", "{script} stderr");
    }
}

/// `grep` had three defects beyond the wording, all verified against GNU
/// grep 3.11:
///  1. the diagnostic went into stdout, so `grep foo *.log | wc -l` counted it
///     as a match;
///  2. an unreadable operand did not set status 2;
///  3. the filename prefix followed the files it managed to read, not the
///     files it was asked for.
mod grep_unreadable_operand {
    use bashkit::Bash;

    async fn run(script: &str) -> bashkit::ExecResult {
        let mut bash = Bash::builder().mount_text("/w/ok.txt", "hello\n").build();
        bash.exec(&format!("cd /w; {script}")).await.unwrap()
    }

    #[tokio::test]
    async fn diagnostic_goes_to_stderr_not_stdout() {
        let r = run("grep hello ok.txt /nope").await;
        assert_eq!(r.stdout.to_string(), "ok.txt:hello\n");
        assert_eq!(
            r.stderr.to_string(),
            "grep: /nope: No such file or directory\n"
        );
    }

    /// The prefix is present because two operands were named, even though only
    /// one could be opened.
    #[tokio::test]
    async fn filename_prefix_follows_operands_asked_for() {
        let r = run("grep hello ok.txt /nope").await;
        assert!(r.stdout.to_string().starts_with("ok.txt:"));
        // A single readable operand still gets no prefix.
        let single = run("grep hello ok.txt").await;
        assert_eq!(single.stdout.to_string(), "hello\n");
    }

    #[tokio::test]
    async fn unreadable_operand_is_status_2() {
        // Status 2 outranks both "matched" and "no match".
        assert_eq!(run("grep hello ok.txt /nope").await.exit_code, 2);
        assert_eq!(run("grep zzz ok.txt /nope").await.exit_code, 2);
        assert_eq!(run("grep hello /nope").await.exit_code, 2);
        // No error, no change.
        assert_eq!(run("grep hello ok.txt").await.exit_code, 0);
        assert_eq!(run("grep zzz ok.txt").await.exit_code, 1);
    }

    /// `-s` silences the message but not the status.
    #[tokio::test]
    async fn suppress_errors_keeps_the_status() {
        let r = run("grep -s hello ok.txt /nope").await;
        assert_eq!(r.exit_code, 2);
        assert_eq!(r.stderr.to_string(), "");
        assert_eq!(r.stdout.to_string(), "ok.txt:hello\n");
    }

    /// `-q` stops at the first match, so GNU grep never reaches the bad
    /// operand: status 0 and no message. Without a match it does reach it.
    #[tokio::test]
    async fn quiet_short_circuits_on_a_match_only() {
        let matched = run("grep -q hello ok.txt /nope").await;
        assert_eq!(matched.exit_code, 0);
        assert_eq!(matched.stderr.to_string(), "");

        let unmatched = run("grep -q zzz ok.txt /nope").await;
        assert_eq!(unmatched.exit_code, 2);
        assert_eq!(
            unmatched.stderr.to_string(),
            "grep: /nope: No such file or directory\n"
        );
    }

    /// The bug that motivated the stream split.
    #[tokio::test]
    async fn pipeline_does_not_see_the_diagnostic() {
        let r = run("grep hello ok.txt /nope 2>/dev/null | wc -l").await;
        assert_eq!(r.stdout.to_string().trim(), "1");
    }
}
