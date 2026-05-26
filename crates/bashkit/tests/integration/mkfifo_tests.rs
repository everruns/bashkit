//! Integration tests for mkfifo and named pipe (FIFO) support
//!
//! Tests mkfifo builtin, test -p, file command, and FIFO read/write behavior.

use bashkit::Bash;

#[tokio::test]
async fn mkfifo_creates_fifo_in_vfs() {
    let mut bash = Bash::new();
    let result = bash.exec("mkfifo /tmp/pipe && echo ok").await.unwrap();
    assert_eq!(result.exit_code, 0);
    assert_eq!(result.stdout.trim(), "ok");
}

#[tokio::test]
async fn test_p_detects_fifo() {
    let mut bash = Bash::new();
    let result = bash
        .exec("mkfifo /tmp/pipe && test -p /tmp/pipe && echo yes || echo no")
        .await
        .unwrap();
    assert_eq!(result.exit_code, 0);
    assert_eq!(result.stdout.trim(), "yes");
}

#[tokio::test]
async fn test_p_false_for_regular_file() {
    let mut bash = Bash::new();
    let result = bash
        .exec("echo hi > /tmp/file && test -p /tmp/file && echo yes || echo no")
        .await
        .unwrap();
    assert_eq!(result.stdout.trim(), "no");
}

#[tokio::test]
async fn test_p_false_for_directory() {
    let mut bash = Bash::new();
    let result = bash
        .exec("mkdir -p /tmp/dir && test -p /tmp/dir && echo yes || echo no")
        .await
        .unwrap();
    assert_eq!(result.stdout.trim(), "no");
}

#[tokio::test]
async fn test_p_false_for_nonexistent() {
    let mut bash = Bash::new();
    let result = bash
        .exec("test -p /tmp/nope && echo yes || echo no")
        .await
        .unwrap();
    assert_eq!(result.stdout.trim(), "no");
}

#[tokio::test]
async fn file_identifies_fifo() {
    let mut bash = Bash::new();
    let result = bash
        .exec("mkfifo /tmp/pipe && file /tmp/pipe")
        .await
        .unwrap();
    assert_eq!(result.exit_code, 0);
    assert!(result.stdout.contains("fifo"));
    assert!(result.stdout.contains("named pipe"));
}

#[tokio::test]
async fn stat_identifies_fifo() {
    let mut bash = Bash::new();
    let result = bash
        .exec("mkfifo /tmp/pipe && stat /tmp/pipe")
        .await
        .unwrap();
    assert_eq!(result.exit_code, 0);
    assert!(result.stdout.contains("fifo") || result.stdout.contains("named pipe"));
}

#[tokio::test]
async fn fifo_write_and_read() {
    let mut bash = Bash::new();
    let result = bash
        .exec("mkfifo /tmp/pipe && echo hello > /tmp/pipe && cat /tmp/pipe")
        .await
        .unwrap();
    assert_eq!(result.exit_code, 0);
    assert_eq!(result.stdout.trim(), "hello");
}

#[tokio::test]
async fn mkfifo_with_mode() {
    let mut bash = Bash::new();
    let result = bash
        .exec("mkfifo -m 0600 /tmp/pipe && stat /tmp/pipe")
        .await
        .unwrap();
    assert_eq!(result.exit_code, 0);
}

#[tokio::test]
async fn mkfifo_multiple_pipes() {
    let mut bash = Bash::new();
    let result = bash
        .exec(
            r#"
mkfifo /tmp/p1 /tmp/p2 /tmp/p3
test -p /tmp/p1 && test -p /tmp/p2 && test -p /tmp/p3 && echo all_fifos
"#,
        )
        .await
        .unwrap();
    assert_eq!(result.exit_code, 0);
    assert_eq!(result.stdout.trim(), "all_fifos");
}

#[tokio::test]
async fn mkfifo_already_exists_error() {
    let mut bash = Bash::new();
    let result = bash
        .exec("mkfifo /tmp/pipe && mkfifo /tmp/pipe")
        .await
        .unwrap();
    assert_ne!(result.exit_code, 0);
}

#[tokio::test]
async fn mkfifo_missing_operand() {
    let mut bash = Bash::new();
    let result = bash.exec("mkfifo").await.unwrap();
    assert_ne!(result.exit_code, 0);
    assert!(result.stderr.contains("missing operand"));
}

#[tokio::test]
async fn mkfifo_parent_not_found() {
    let mut bash = Bash::new();
    let result = bash.exec("mkfifo /no/such/dir/pipe").await.unwrap();
    assert_ne!(result.exit_code, 0);
}

#[tokio::test]
async fn bracket_p_detects_fifo() {
    let mut bash = Bash::new();
    let result = bash
        .exec("mkfifo /tmp/pipe && [ -p /tmp/pipe ] && echo yes || echo no")
        .await
        .unwrap();
    assert_eq!(result.stdout.trim(), "yes");
}

#[tokio::test]
async fn fifo_persists_type_after_write() {
    let mut bash = Bash::new();
    let result = bash
        .exec(
            r#"
mkfifo /tmp/pipe
echo data > /tmp/pipe
test -p /tmp/pipe && echo still_fifo || echo not_fifo
"#,
        )
        .await
        .unwrap();
    assert_eq!(result.exit_code, 0);
    assert_eq!(result.stdout.trim(), "still_fifo");
}
