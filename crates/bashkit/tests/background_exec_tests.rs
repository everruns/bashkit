//! Tests for background execution with & and wait
//!
//! Covers: cmd &, cmd & cmd2, wait, wait $pid, $!, multiple background jobs,
//! background jobs writing to VFS.

use bashkit::Bash;

/// Basic background execution: cmd & should succeed
#[tokio::test]
async fn background_basic() {
    let mut bash = Bash::new();
    let result = bash.exec("echo hello &\nwait").await.unwrap();
    assert_eq!(result.exit_code, 0);
}

/// Background writes to VFS, foreground waits then reads
#[tokio::test]
async fn background_writes_to_vfs() {
    let mut bash = Bash::new();
    let result = bash
        .exec("echo content > /tmp/bg_out.txt &\nwait\ncat /tmp/bg_out.txt")
        .await
        .unwrap();
    assert_eq!(result.exit_code, 0);
    assert_eq!(result.stdout.trim(), "content");
}

/// Multiple background jobs
#[tokio::test]
async fn multiple_background_jobs() {
    let mut bash = Bash::new();
    let result = bash
        .exec(
            r#"
echo a > /tmp/a.txt &
echo b > /tmp/b.txt &
echo c > /tmp/c.txt &
wait
cat /tmp/a.txt
cat /tmp/b.txt
cat /tmp/c.txt
"#,
        )
        .await
        .unwrap();
    assert_eq!(result.exit_code, 0);
    assert!(result.stdout.contains("a"));
    assert!(result.stdout.contains("b"));
    assert!(result.stdout.contains("c"));
}

/// $! is set after background command
#[tokio::test]
async fn background_sets_last_pid() {
    let mut bash = Bash::new();
    let result = bash.exec("sleep 0 &\necho $!").await.unwrap();
    assert_eq!(result.exit_code, 0);
    // $! should be a non-empty numeric value
    let pid = result.stdout.trim();
    assert!(!pid.is_empty(), "$! should be set after background command");
    assert!(
        pid.parse::<usize>().is_ok(),
        "$! should be numeric, got: {pid}"
    );
}

/// wait with specific PID
#[tokio::test]
async fn wait_specific_pid() {
    let mut bash = Bash::new();
    let result = bash
        .exec(
            r#"
echo done > /tmp/wait_pid.txt &
pid=$!
wait $pid
cat /tmp/wait_pid.txt
"#,
        )
        .await
        .unwrap();
    assert_eq!(result.exit_code, 0);
    assert_eq!(result.stdout.trim(), "done");
}

/// Background command exit code via wait
#[tokio::test]
async fn background_exit_code_via_wait() {
    let mut bash = Bash::new();
    let result = bash.exec("false &\nwait\necho $?").await.unwrap();
    // wait should return the exit code of the background job
    assert!(result.stdout.contains("1"));
}

/// cmd1 & cmd2 — cmd2 runs in foreground while cmd1 is backgrounded
#[tokio::test]
async fn background_and_foreground() {
    let mut bash = Bash::new();
    let result = bash.exec("echo bg > /tmp/bg.txt & echo fg").await.unwrap();
    assert_eq!(result.exit_code, 0);
    assert!(result.stdout.contains("fg"));
}
