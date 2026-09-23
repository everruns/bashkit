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
const READERS: &[(&str, &str)] = &[
    ("cat", "cat /nope"),
    ("head", "head /nope"),
    ("tail", "tail /nope"),
    ("strings", "strings /nope"),
    ("wc", "wc /nope"),
    ("sort", "sort /nope"),
    ("uniq", "uniq /nope"),
    ("cut", "cut -c1 /nope"),
    ("nl", "nl /nope"),
    ("awk", "awk '{print}' /nope"),
    ("paste", "paste /nope"),
    ("rev", "rev /nope"),
    ("tac", "tac /nope"),
    ("column", "column /nope"),
];

#[tokio::test]
async fn missing_operand_reports_the_real_errno_text() {
    for (name, script) in READERS {
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
        assert_eq!(result.exit_code, 1, "{name} exit code");
    }
}

/// The specific shape that regressed: the `Display` of `bashkit::Error`.
#[tokio::test]
async fn missing_operand_does_not_leak_the_error_enum_shape() {
    for (name, script) in READERS {
        let mut bash = Bash::new();
        let result = bash.exec(script).await.unwrap();
        let stderr = result.stderr.to_string();
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
