//! Tests for issue #853: `result=$(cmd 2>&1 >file)` redirect ordering.
//!
//! Bash processes redirects left-to-right. `2>&1 >file` means:
//! 1. stderr → where stdout currently points (the $() capture pipe)
//! 2. stdout → file
//!
//! So `result=$(cmd 2>&1 >file)` captures stderr in result, stdout goes to file.

use bashkit::Bash;
/// Core reproduction: 2>&1 >file inside command substitution
#[tokio::test]
async fn redirect_2_to_1_then_file_in_cmdsub() {
    let mut bash = Bash::new();
    let result = bash
        .exec(
            r#"
f() { echo "stdout"; echo "stderr" >&2; }
result=$(f 2>&1 >"/tmp/out.txt")
echo "result=[$result]"
echo "file=[$(cat /tmp/out.txt)]"
"#,
        )
        .await
        .unwrap();

    let stdout = result.stdout;
    // result should capture stderr (because 2>&1 copies stdout's fd which is the capture pipe)
    assert!(
        stdout.contains("result=[stderr]"),
        "expected result=[stderr], got: {stdout}"
    );
    // file should contain stdout (because >file redirects stdout to file)
    assert!(
        stdout.contains("file=[stdout]"),
        "expected file=[stdout], got: {stdout}"
    );
}

/// Simpler case: 2>&1 >file outside command substitution
#[tokio::test]
async fn redirect_2_to_1_then_file_outside_cmdsub() {
    let mut bash = Bash::new();
    let result = bash
        .exec(
            r#"
f() { echo "stdout"; echo "stderr" >&2; }
f 2>&1 >"/tmp/out2.txt"
echo "---"
cat /tmp/out2.txt
"#,
        )
        .await
        .unwrap();

    let stdout = result.stdout;
    // stderr should go to where stdout was (the terminal/capture) since 2>&1 comes first
    // stdout should go to the file since >file comes second
    assert!(
        stdout.contains("stderr"),
        "stderr should appear in stdout: {stdout}"
    );
    assert!(
        stdout.contains("---\nstdout"),
        "file should contain stdout: {stdout}"
    );
}

/// Reverse order: >file 2>&1 should send both to file
#[tokio::test]
async fn redirect_file_then_2_to_1() {
    let mut bash = Bash::new();
    let result = bash
        .exec(
            r#"
f() { echo "stdout"; echo "stderr" >&2; }
f >"/tmp/out3.txt" 2>&1
cat /tmp/out3.txt
"#,
        )
        .await
        .unwrap();

    let stdout = result.stdout;
    // Both should go to file (stdout→file first, then stderr→where stdout points = file)
    assert!(
        stdout.contains("stdout"),
        "file should contain stdout: {stdout}"
    );
    assert!(
        stdout.contains("stderr"),
        "file should contain stderr: {stdout}"
    );
}

/// Regression guard: mixed 2>&1 + >file must still truncate/create file on empty output.
#[tokio::test]
async fn redirect_2_to_1_then_file_truncates_on_empty_output() {
    let mut bash = Bash::new();
    let result = bash
        .exec(
            r#"
echo "stale" >"/tmp/out-empty.txt"
true 2>&1 >"/tmp/out-empty.txt"
wc -c <"/tmp/out-empty.txt"
"#,
        )
        .await
        .unwrap();

    assert_eq!(
        result.stdout.trim(),
        "0",
        "expected redirected file to be truncated to zero bytes"
    );
}
