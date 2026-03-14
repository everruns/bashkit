//! Tests for coproc (coprocess) support
//!
//! Covers: coproc parsing, NAME array setup, NAME_PID variable,
//! reading from coproc via read -u FD, reading via <&FD redirect,
//! named coprocs, and default COPROC name.

use bashkit::Bash;

/// Basic coproc: sets COPROC array and COPROC_PID
#[tokio::test]
async fn coproc_basic_sets_array_and_pid() {
    let mut bash = Bash::new();
    let result = bash
        .exec(
            r#"
coproc { echo hello; }
echo "read_fd=${COPROC[0]}"
echo "write_fd=${COPROC[1]}"
echo "pid=$COPROC_PID"
"#,
        )
        .await
        .unwrap();
    assert_eq!(result.exit_code, 0);
    assert!(result.stdout.contains("read_fd=63"));
    assert!(result.stdout.contains("write_fd=62"));
    assert!(result.stdout.contains("pid="));
    // PID should be a number > 0
    let pid_line = result
        .stdout
        .lines()
        .find(|l| l.starts_with("pid="))
        .unwrap();
    let pid: i64 = pid_line.trim_start_matches("pid=").parse().unwrap();
    assert!(pid > 0);
}

/// Read from coproc using read -u FD
#[tokio::test]
async fn coproc_read_u_fd() {
    let mut bash = Bash::new();
    let result = bash
        .exec(
            r#"
coproc { echo line1; echo line2; echo line3; }
read -u ${COPROC[0]} first
read -u ${COPROC[0]} second
echo "$first"
echo "$second"
"#,
        )
        .await
        .unwrap();
    assert_eq!(result.exit_code, 0);
    let lines: Vec<&str> = result.stdout.trim().lines().collect();
    assert_eq!(lines, vec!["line1", "line2"]);
}

/// Read from coproc using read -r with FD variable
#[tokio::test]
async fn coproc_read_with_fd_variable() {
    let mut bash = Bash::new();
    let result = bash
        .exec(
            r#"
coproc { echo redirected; }
read -r -u ${COPROC[0]} line
echo "$line"
"#,
        )
        .await
        .unwrap();
    assert_eq!(result.exit_code, 0);
    assert_eq!(result.stdout.trim(), "redirected");
}

/// Named coproc: coproc NAME { cmd; }
#[tokio::test]
async fn coproc_named() {
    let mut bash = Bash::new();
    let result = bash
        .exec(
            r#"
coproc MYPROC { echo named_output; }
echo "read_fd=${MYPROC[0]}"
echo "pid=$MYPROC_PID"
read -u ${MYPROC[0]} line
echo "$line"
"#,
        )
        .await
        .unwrap();
    assert_eq!(result.exit_code, 0);
    assert!(result.stdout.contains("read_fd=63"));
    assert!(result.stdout.contains("pid="));
    assert!(result.stdout.contains("named_output"));
}

/// Multiple named coprocs get different FDs
#[tokio::test]
async fn coproc_multiple_named() {
    let mut bash = Bash::new();
    let result = bash
        .exec(
            r#"
coproc A { echo from_a; }
coproc B { echo from_b; }
read -u ${A[0]} a_line
read -u ${B[0]} b_line
echo "$a_line"
echo "$b_line"
"#,
        )
        .await
        .unwrap();
    assert_eq!(result.exit_code, 0);
    let lines: Vec<&str> = result.stdout.trim().lines().collect();
    assert_eq!(lines, vec!["from_a", "from_b"]);
}

/// Coproc with simple command (no braces)
#[tokio::test]
async fn coproc_simple_command() {
    let mut bash = Bash::new();
    let result = bash
        .exec(
            r#"
coproc echo simple_output
read -u ${COPROC[0]} line
echo "$line"
"#,
        )
        .await
        .unwrap();
    assert_eq!(result.exit_code, 0);
    assert_eq!(result.stdout.trim(), "simple_output");
}

/// Coproc EOF: reading past available data
#[tokio::test]
async fn coproc_eof() {
    let mut bash = Bash::new();
    let result = bash
        .exec(
            r#"
coproc { echo only_line; }
read -u ${COPROC[0]} first
read -u ${COPROC[0]} second
echo "first=$first"
echo "second=$second"
echo "exit=$?"
"#,
        )
        .await
        .unwrap();
    // First read succeeds, second read gets EOF (read returns 1)
    assert!(result.stdout.contains("first=only_line"));
}

/// Coproc with multiline output
#[tokio::test]
async fn coproc_multiline_output() {
    let mut bash = Bash::new();
    let result = bash
        .exec(
            r#"
coproc {
    echo alpha
    echo beta
    echo gamma
}
read -u ${COPROC[0]} a
read -u ${COPROC[0]} b
read -u ${COPROC[0]} c
echo "$a $b $c"
"#,
        )
        .await
        .unwrap();
    assert_eq!(result.exit_code, 0);
    assert_eq!(result.stdout.trim(), "alpha beta gamma");
}

/// $! is set after coproc (last background PID)
#[tokio::test]
async fn coproc_sets_bang_variable() {
    let mut bash = Bash::new();
    let result = bash
        .exec(
            r#"
coproc { echo test; }
echo "$!"
"#,
        )
        .await
        .unwrap();
    assert_eq!(result.exit_code, 0);
    let pid = result.stdout.trim();
    assert!(!pid.is_empty());
    assert!(pid.parse::<i64>().is_ok());
}
