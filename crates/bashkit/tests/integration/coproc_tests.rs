//! Tests for coproc (coprocess) support
//!
//! Covers: coproc parsing, NAME array setup, NAME_PID variable,
//! reading from coproc via read -u FD, reading via <&FD redirect,
//! named coprocs, and default COPROC name.

use bashkit::Bash;
use std::sync::{Arc, Mutex};

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
    assert!(result.stdout.contains("write_fd=60"));
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

/// Coproc stdout should not be emitted to streaming callbacks.
#[tokio::test]
async fn coproc_stdout_not_streamed() {
    let streamed_stdout = Arc::new(Mutex::new(Vec::new()));
    let cb_stdout = streamed_stdout.clone();
    let mut bash = Bash::new();

    let result = bash
        .exec_streaming(
            r#"
coproc { echo hidden_value; }
read -u ${COPROC[0]} line
echo "visible:$line"
"#,
            Box::new(move |stdout, _stderr| {
                if !stdout.is_empty() {
                    cb_stdout.lock().unwrap().push(stdout.to_string());
                }
            }),
        )
        .await
        .unwrap();

    assert_eq!(result.stdout.trim(), "visible:hidden_value");
    assert_eq!(
        streamed_stdout.lock().unwrap().as_slice(),
        ["visible:hidden_value\n"],
        "coproc body output must not be visible to streaming consumers",
    );
}

async fn run(script: &str) -> bashkit::ExecResult {
    Bash::new().exec(script).await.unwrap()
}

/// Writes to `${NAME[1]}` feed the coproc's stdin; closing the write end
/// delivers EOF (bash-oracle `coproc-basic`).
#[tokio::test]
async fn coproc_stdin_fed_by_write_fd() {
    let result = run(r#"
coproc UPPER { tr a-z A-Z; }
echo "texto" >&"${UPPER[1]}"
exec {UPPER[1]}>&-
read -r line <&"${UPPER[0]}"
echo "$line"
wait
echo "after wait: [${UPPER[@]}] [${UPPER_PID-unset}] ${#UPPER[@]}"
"#)
    .await;
    assert_eq!(result.stdout, "TEXTO\nafter wait: [] [unset] 0\n");
    assert_eq!(result.stderr, "");
}

/// Request/response: write a line, read a line, repeatedly, without
/// deadlock. Default name COPROC, fds 63/60 like bash.
#[tokio::test]
async fn coproc_request_response_loop() {
    let result = run(r#"
coproc { while read l; do echo "got $l"; done; }
echo "${COPROC[@]}"
[ "$!" = "$COPROC_PID" ] && echo bang-ok
for i in 1 2 3; do echo "req $i" >&"${COPROC[1]}"; read -r r <&"${COPROC[0]}"; echo "$r"; done
echo x >&${COPROC[1]}; read -u ${COPROC[0]} r; echo "u: $r"
exec {COPROC[1]}>&-
read -r r <&"${COPROC[0]}"; echo "eof rc=$? [$r]"
wait $COPROC_PID; echo "wait rc=$?"
echo "[${COPROC[@]}] [${COPROC_PID-unset}]"
"#)
    .await;
    assert_eq!(
        result.stdout,
        "63 60\nbang-ok\ngot req 1\ngot req 2\ngot req 3\nu: got x\neof rc=1 []\nwait rc=0\n[] [unset]\n"
    );
    assert_eq!(result.stderr, "");
}

/// `wait $NAME_PID` reports the coproc's status, then NAME and NAME_PID
/// are unset and the fd pair is free again.
#[tokio::test]
async fn coproc_exit_status_and_cleanup() {
    let result = run(r#"
coproc C { exit 3; }
wait $C_PID; echo "C rc=$?"
echo "[${C[@]}] [${C_PID-unset}]"
coproc D { read x; exit 4; }
echo "[${D[@]}]"
echo hi >&${D[1]}
wait $D_PID; echo "D rc=$?"
echo "[${D[@]}] [${D_PID-unset}]"
"#)
    .await;
    assert_eq!(
        result.stdout,
        "C rc=3\n[] [unset]\n[63 60]\nD rc=4\n[] [unset]\n"
    );
}

/// A second coproc while one still runs: bash warns and starts it anyway.
#[tokio::test]
async fn coproc_still_exists_warning() {
    let result = run(r#"
coproc A { cat; }
coproc B { cat; }
echo "rc=$? ${A[@]} ${B[@]}"
exec {A[1]}>&- {B[1]}>&-
wait
"#)
    .await;
    assert_eq!(result.stdout, "rc=0 63 60 62 58\n");
    assert!(
        result.stderr.contains("warning: execute_coproc: coproc [")
            && result.stderr.contains(":A] still exists"),
        "stderr: {}",
        result.stderr
    );
}

/// After the coproc is reaped its descriptors are closed.
#[tokio::test]
async fn coproc_fds_closed_after_reap() {
    let result = run(r#"
coproc cat
echo one >&${COPROC[1]}
read -r -u ${COPROC[0]} x; echo "x=$x"
fd=${COPROC[1]}
exec {COPROC[1]}>&-
wait
echo hi >&$fd; echo "write rc=$?"
"#)
    .await;
    assert_eq!(result.stdout, "x=one\nwrite rc=1\n");
    assert!(
        result.stderr.contains("Bad file descriptor"),
        "stderr: {}",
        result.stderr
    );
}

/// The end of `exec()` closes the shell's coproc ends, so a coproc waiting
/// for input ends instead of hanging the call.
#[tokio::test]
async fn coproc_unclosed_ends_with_exec() {
    let mut bash = Bash::new();
    let result = bash.exec("coproc cat\necho done").await.unwrap();
    assert_eq!(result.stdout, "done\n");
    let result = bash.exec("echo \"[${COPROC[@]}]\"").await.unwrap();
    assert_eq!(result.stdout, "[]\n");
}

/// Sequential mode (no concurrent jobs) runs the body up front with its
/// stdin at end of input.
#[tokio::test]
async fn coproc_sequential_mode_runs_body_eagerly() {
    let mut bash = Bash::builder().concurrent_jobs(false).build();
    let result = bash
        .exec("coproc { echo a; cat; echo b; }\nread -r x <&${COPROC[0]}; read -r y <&${COPROC[0]}; echo \"$x$y\"")
        .await
        .unwrap();
    assert_eq!(result.stdout, "ab\n");
}

/// A coproc holds a job slot: the background-job cap bounds coprocs too.
#[tokio::test]
async fn coproc_counts_against_job_limit() {
    let mut bash = Bash::builder()
        .limits(bashkit::ExecutionLimits::new().max_background_jobs(1))
        .build();
    let result = bash
        .exec("coproc A { cat; }\ncoproc B { cat; } 2>/dev/null\necho \"rc=$? [${B[@]}]\"\nexec {A[1]}>&-")
        .await
        .unwrap();
    assert_eq!(result.stdout, "rc=1 []\n");
}
