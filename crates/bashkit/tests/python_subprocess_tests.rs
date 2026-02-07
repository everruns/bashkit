// Integration tests for monty subprocess isolation (crash protection).
// These tests verify that Python execution works correctly when routed
// through the bashkit-monty-worker child process.
//
// The worker binary must be built first: `cargo build -p bashkit-monty-worker`
// (cargo builds all workspace bins into the same target dir, so
// find_worker_binary() locates it adjacent to the test binary.)

#![cfg(feature = "python")]

use bashkit::{Bash, PythonIsolation, PythonLimits};
use serial_test::serial;

/// Helper: create Bash with python in subprocess mode.
/// Relies on find_worker_binary() discovering the worker adjacent to the test exe.
/// Clears BASHKIT_MONTY_WORKER to avoid interference from env-mutating tests.
fn bash_subprocess() -> Bash {
    std::env::remove_var("BASHKIT_MONTY_WORKER");
    Bash::builder()
        .python_with_limits(PythonLimits::default().isolation(PythonIsolation::Subprocess))
        .build()
}

// ---------------------------------------------------------------------------
// Basic functionality via subprocess
// ---------------------------------------------------------------------------

#[tokio::test]
async fn subprocess_print() {
    let mut bash = bash_subprocess();
    let r = bash.exec("python3 -c \"print('hello')\"").await.unwrap();
    assert_eq!(r.exit_code, 0);
    assert_eq!(r.stdout, "hello\n");
}

#[tokio::test]
async fn subprocess_expression() {
    let mut bash = bash_subprocess();
    let r = bash.exec("python3 -c \"2 + 3\"").await.unwrap();
    assert_eq!(r.exit_code, 0);
    assert_eq!(r.stdout, "5\n");
}

#[tokio::test]
async fn subprocess_multiline() {
    let mut bash = bash_subprocess();
    let r = bash
        .exec("python3 -c \"x = 10\ny = 20\nprint(x + y)\"")
        .await
        .unwrap();
    assert_eq!(r.exit_code, 0);
    assert_eq!(r.stdout, "30\n");
}

#[tokio::test]
async fn subprocess_syntax_error() {
    let mut bash = bash_subprocess();
    let r = bash.exec("python3 -c \"def\"").await.unwrap();
    assert_eq!(r.exit_code, 1);
    assert!(
        r.stderr.contains("SyntaxError") || r.stderr.contains("Error"),
        "stderr: {}",
        r.stderr
    );
}

#[tokio::test]
async fn subprocess_runtime_error() {
    let mut bash = bash_subprocess();
    let r = bash.exec("python3 -c \"1/0\"").await.unwrap();
    assert_eq!(r.exit_code, 1);
    assert!(
        r.stderr.contains("ZeroDivisionError"),
        "stderr: {}",
        r.stderr
    );
}

#[tokio::test]
async fn subprocess_output_before_error() {
    let mut bash = bash_subprocess();
    let r = bash
        .exec("python3 -c \"print('before')\n1/0\"")
        .await
        .unwrap();
    assert_eq!(r.exit_code, 1);
    assert_eq!(r.stdout, "before\n");
    assert!(r.stderr.contains("ZeroDivisionError"));
}

// ---------------------------------------------------------------------------
// VFS bridging over IPC
// ---------------------------------------------------------------------------

#[tokio::test]
async fn subprocess_vfs_read_write() {
    let mut bash = bash_subprocess();
    bash.exec("echo -n 'hello from bash' > /tmp/test.txt")
        .await
        .unwrap();
    let r = bash
        .exec("python3 -c \"from pathlib import Path\nprint(Path('/tmp/test.txt').read_text())\"")
        .await
        .unwrap();
    assert_eq!(r.exit_code, 0);
    assert_eq!(r.stdout, "hello from bash\n");
}

#[tokio::test]
async fn subprocess_vfs_write_then_read() {
    let mut bash = bash_subprocess();
    let r = bash
        .exec(
            "python3 -c \"from pathlib import Path\nPath('/tmp/out.txt').write_text('from python')\nprint(Path('/tmp/out.txt').read_text())\"",
        )
        .await
        .unwrap();
    assert_eq!(r.exit_code, 0);
    assert_eq!(r.stdout, "from python\n");
}

#[tokio::test]
async fn subprocess_vfs_file_not_found() {
    let mut bash = bash_subprocess();
    let r = bash
        .exec("python3 -c \"from pathlib import Path\ntry:\n    Path('/no/such/file').read_text()\nexcept FileNotFoundError as e:\n    print('caught:', e)\"")
        .await
        .unwrap();
    assert_eq!(r.exit_code, 0);
    assert!(r.stdout.contains("caught:"), "stdout: {}", r.stdout);
}

#[tokio::test]
async fn subprocess_vfs_mkdir_iterdir() {
    let mut bash = bash_subprocess();
    // Create /tmp first so mkdir /tmp/sub succeeds (VFS starts empty)
    bash.exec("mkdir -p /tmp/sub").await.unwrap();
    let r = bash
        .exec("python3 -c \"from pathlib import Path\nPath('/tmp/sub/a.txt').write_text('a')\nPath('/tmp/sub/b.txt').write_text('b')\nfor p in Path('/tmp/sub').iterdir():\n    print(p.name)\"")
        .await
        .unwrap();
    assert_eq!(r.exit_code, 0, "stderr: {}", r.stderr);
    assert!(r.stdout.contains("a.txt"));
    assert!(r.stdout.contains("b.txt"));
}

// ---------------------------------------------------------------------------
// Crash isolation (the whole point)
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn subprocess_worker_crash_via_false_binary() {
    // Use /bin/false as the worker — it exits immediately with code 1.
    // This tests the "worker exited unexpectedly" path.
    std::env::set_var("BASHKIT_MONTY_WORKER", "/bin/false");
    let mut bash = Bash::builder()
        .python_with_limits(PythonLimits::default().isolation(PythonIsolation::Subprocess))
        .build();

    let r = bash.exec("python3 -c \"print('hi')\"").await.unwrap();
    assert_ne!(r.exit_code, 0);
    assert!(
        r.stderr.contains("crashed")
            || r.stderr.contains("exited unexpectedly")
            || r.stderr.contains("error"),
        "Expected crash/error message, got stderr: {}",
        r.stderr
    );

    std::env::remove_var("BASHKIT_MONTY_WORKER");
}

// ---------------------------------------------------------------------------
// Resource limits via subprocess
// ---------------------------------------------------------------------------

#[tokio::test]
async fn subprocess_recursion_limit() {
    let mut bash = bash_subprocess();
    let r = bash.exec("python3 -c \"def r(): r()\nr()\"").await.unwrap();
    assert_ne!(r.exit_code, 0);
    assert!(
        r.stderr.contains("RecursionError") || r.stderr.contains("recursion"),
        "stderr: {}",
        r.stderr
    );
}

// ---------------------------------------------------------------------------
// Auto mode fallback
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn auto_mode_falls_back_to_in_process() {
    // Point at a nonexistent worker, Auto mode should fall back to in-process
    std::env::set_var("BASHKIT_MONTY_WORKER", "/nonexistent/worker");
    let mut bash = Bash::builder()
        .python_with_limits(PythonLimits::default().isolation(PythonIsolation::Auto))
        .build();

    let r = bash.exec("python3 -c \"print('fallback')\"").await.unwrap();
    // Auto falls back to in-process, which should succeed
    assert_eq!(r.exit_code, 0);
    assert_eq!(r.stdout, "fallback\n");

    std::env::remove_var("BASHKIT_MONTY_WORKER");
}

#[tokio::test]
#[serial]
async fn subprocess_mode_fails_when_worker_missing() {
    std::env::set_var("BASHKIT_MONTY_WORKER", "/nonexistent/worker");
    let mut bash = Bash::builder()
        .python_with_limits(PythonLimits::default().isolation(PythonIsolation::Subprocess))
        .build();

    let r = bash.exec("python3 -c \"print('hi')\"").await.unwrap();
    assert_ne!(r.exit_code, 0);
    assert!(
        r.stderr.contains("not found") || r.stderr.contains("No such file"),
        "stderr: {}",
        r.stderr
    );

    std::env::remove_var("BASHKIT_MONTY_WORKER");
}

#[tokio::test]
async fn subprocess_version() {
    let mut bash = bash_subprocess();
    let r = bash.exec("python3 --version").await.unwrap();
    assert_eq!(r.exit_code, 0);
    assert!(r.stdout.contains("Python 3.12.0"));
}
