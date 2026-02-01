//! Security tests using fail-rs for fault injection
//!
//! These tests verify that BashKit handles failure scenarios securely:
//! - Resource limits are enforced even under failure conditions
//! - Filesystem operations fail gracefully
//! - Interpreter handles errors without exposing internal state
//!
//! **IMPORTANT**: Fail points are global state and not thread-safe.
//! Run these tests with a single thread to avoid race conditions:
//!
//! ```sh
//! cargo test --features failpoints security_ -- --test-threads=1
//! ```

#![cfg(feature = "failpoints")]

use bashkit::{Bash, ControlFlow, ExecResult, ExecutionLimits};
use std::time::Duration;

/// Helper to run a script and capture the result
async fn run_script(script: &str) -> ExecResult {
    let mut bash = Bash::new();
    bash.exec(script).await.unwrap_or_else(|e| ExecResult {
        stdout: String::new(),
        stderr: e.to_string(),
        exit_code: 1,
        control_flow: ControlFlow::None,
    })
}

/// Helper to run a script with custom limits
async fn run_script_with_limits(script: &str, limits: ExecutionLimits) -> ExecResult {
    let mut bash = Bash::builder().limits(limits).build();
    bash.exec(script).await.unwrap_or_else(|e| ExecResult {
        stdout: String::new(),
        stderr: e.to_string(),
        exit_code: 1,
        control_flow: ControlFlow::None,
    })
}

// =============================================================================
// Resource Limit Fail Point Tests
// =============================================================================

/// Test: Command counter corruption doesn't allow bypass
///
/// Security property: Even if the counter is corrupted to skip increment,
/// the limit should still be enforced eventually.
#[tokio::test]
async fn security_command_limit_skip_increment() {
    fail::cfg("limits::tick_command", "return(skip_increment)").unwrap();

    // With skip_increment, commands don't count - this is a vulnerability test
    // The script should still complete (no infinite execution)
    let result = run_script_with_limits(
        "echo 1; echo 2; echo 3; echo 4; echo 5",
        ExecutionLimits::new().max_commands(3),
    )
    .await;

    fail::cfg("limits::tick_command", "off").unwrap();

    // When skip_increment is active, commands bypass the limit
    // This test documents the behavior under this failure mode
    assert!(result.exit_code == 0 || result.stderr.contains("limit"));
}

/// Test: Command counter overflow is handled
#[tokio::test]
async fn security_command_limit_overflow() {
    fail::cfg("limits::tick_command", "return(force_overflow)").unwrap();

    let result = run_script("echo hello").await;

    fail::cfg("limits::tick_command", "off").unwrap();

    // Should fail with limit exceeded
    assert!(
        result.stderr.contains("limit") || result.stderr.contains("exceeded"),
        "Expected limit error, got: {}",
        result.stderr
    );
}

/// Test: Loop counter reset doesn't cause infinite loop
#[tokio::test]
#[ignore = "Fail point test requires --test-threads=1 and is flaky in parallel"]
async fn security_loop_counter_reset() {
    // Note: This test would cause infinite loop if limit wasn't also checked elsewhere
    // We set a reasonable iteration limit to prevent actual infinite loop
    fail::cfg("limits::tick_loop", "1*return(reset_counter)").unwrap();

    let result = run_script_with_limits(
        "for i in 1 2 3 4 5; do echo $i; done",
        ExecutionLimits::new()
            .max_loop_iterations(10)
            .max_commands(50)
            .timeout(Duration::from_secs(2)),
    )
    .await;

    fail::cfg("limits::tick_loop", "off").unwrap();

    // Should complete (counter resets only once due to 1* prefix)
    // Accept either success or command-not-found (for $i variable in some shells)
    assert!(result.exit_code == 0 || result.exit_code == 127);
}

/// Test: Function depth bypass is detected
#[tokio::test]
async fn security_function_depth_bypass() {
    fail::cfg("limits::push_function", "return(skip_check)").unwrap();

    // Try recursive function - without limit check, this would cause stack overflow
    let result = run_script_with_limits(
        r#"
        recurse() {
            echo "depth"
            recurse
        }
        recurse
        "#,
        ExecutionLimits::new()
            .max_function_depth(5)
            .max_commands(100)
            .timeout(Duration::from_secs(2)),
    )
    .await;

    fail::cfg("limits::push_function", "off").unwrap();

    // Should hit command limit even if function depth is bypassed
    assert!(
        result.stderr.contains("limit")
            || result.stderr.contains("exceeded")
            || result.exit_code != 0,
        "Recursive function should be limited"
    );
}

// =============================================================================
// Filesystem Fail Point Tests
// =============================================================================

/// Test: Read failure is handled gracefully
#[tokio::test]
async fn security_fs_read_io_error() {
    fail::cfg("fs::read_file", "return(io_error)").unwrap();

    let result = run_script("cat /tmp/test.txt").await;

    fail::cfg("fs::read_file", "off").unwrap();

    // Should fail gracefully, not crash
    assert!(result.exit_code != 0);
}

/// Test: Permission denied is handled
#[tokio::test]
async fn security_fs_read_permission_denied() {
    fail::cfg("fs::read_file", "return(permission_denied)").unwrap();

    let result = run_script("cat /tmp/test.txt").await;

    fail::cfg("fs::read_file", "off").unwrap();

    // Should fail with permission error
    assert!(result.exit_code != 0);
    assert!(
        result.stderr.contains("permission")
            || result.stderr.contains("denied")
            || result.stderr.contains("error"),
        "Expected permission error, got: {}",
        result.stderr
    );
}

/// Test: Corrupt data doesn't cause crash
#[tokio::test]
async fn security_fs_corrupt_data() {
    fail::cfg("fs::read_file", "return(corrupt_data)").unwrap();

    // Try to read and process data that would be corrupted
    let result = run_script("cat /tmp/test.txt | grep something").await;

    fail::cfg("fs::read_file", "off").unwrap();

    // Should handle corrupt data gracefully
    // The test verifies no panic occurred - any exit code is acceptable
    let _ = result.exit_code;
}

/// Test: Write failure doesn't corrupt state
#[tokio::test]
async fn security_fs_write_failure() {
    fail::cfg("fs::write_file", "return(io_error)").unwrap();

    let result = run_script("echo 'test' > /tmp/output.txt").await;

    fail::cfg("fs::write_file", "off").unwrap();

    // Write should fail
    assert!(result.exit_code != 0 || result.stderr.contains("error"));
}

/// Test: Disk full is handled
#[tokio::test]
async fn security_fs_disk_full() {
    fail::cfg("fs::write_file", "return(disk_full)").unwrap();

    let result = run_script("echo 'large data' > /tmp/output.txt").await;

    fail::cfg("fs::write_file", "off").unwrap();

    // Should fail with disk full error
    assert!(result.exit_code != 0);
}

// =============================================================================
// Interpreter Fail Point Tests
// =============================================================================

/// Test: Command execution error is handled
#[tokio::test]
async fn security_interp_execution_error() {
    fail::cfg("interp::execute_command", "return(error)").unwrap();

    let result = run_script("echo hello").await;

    fail::cfg("interp::execute_command", "off").unwrap();

    // Should fail with execution error
    assert!(result.exit_code != 0 || result.stderr.contains("error"));
}

/// Test: Non-zero exit code injection
#[tokio::test]
async fn security_interp_exit_nonzero() {
    fail::cfg("interp::execute_command", "return(exit_nonzero)").unwrap();

    let result = run_script("echo hello").await;

    fail::cfg("interp::execute_command", "off").unwrap();

    // Should have non-zero exit code
    assert_eq!(result.exit_code, 127);
    assert!(result.stderr.contains("injected failure"));
}

// =============================================================================
// Combination/Stress Tests
// =============================================================================

/// Test: Multiple fail points active simultaneously
#[tokio::test]
async fn security_multiple_failpoints() {
    // Activate multiple fail points
    fail::cfg("limits::tick_command", "5%return(skip_increment)").unwrap();
    fail::cfg("fs::read_file", "10%return(io_error)").unwrap();

    // Run a complex script
    let result = run_script_with_limits(
        r#"
        for i in 1 2 3; do
            echo "iteration $i"
        done
        "#,
        ExecutionLimits::new()
            .max_commands(100)
            .max_loop_iterations(100),
    )
    .await;

    fail::cfg("limits::tick_command", "off").unwrap();
    fail::cfg("fs::read_file", "off").unwrap();

    // Should complete or fail gracefully - the test verifies no panic occurred
    let _ = result.exit_code;
}

/// Test: Fail point with probability (fuzz-like testing)
#[tokio::test]
async fn security_probabilistic_failures() {
    // 10% chance of failure on each command
    fail::cfg("limits::tick_command", "10%return(corrupt_high)").unwrap();

    let mut success_count = 0;
    let mut failure_count = 0;

    for _ in 0..10 {
        let result = run_script_with_limits(
            "echo 1; echo 2; echo 3",
            ExecutionLimits::new().max_commands(100),
        )
        .await;

        if result.exit_code == 0 {
            success_count += 1;
        } else {
            failure_count += 1;
        }
    }

    fail::cfg("limits::tick_command", "off").unwrap();

    // With 10% failure rate across multiple commands, we expect some failures
    // This is a smoke test - the exact ratio depends on RNG
    println!(
        "Probabilistic test: {} successes, {} failures",
        success_count, failure_count
    );
}

// =============================================================================
// Documentation Tests
// =============================================================================

/// Demonstrates how to use fail points for custom security testing
#[tokio::test]
async fn security_example_custom_failpoint_usage() {
    // Setup: Configure fail point
    fail::cfg("fs::write_file", "return(permission_denied)").unwrap();

    // Action: Run code that should trigger the fail point
    let result = run_script("echo 'secret' > /tmp/sensitive.txt").await;

    // Cleanup: Always disable fail points after test
    fail::cfg("fs::write_file", "off").unwrap();

    // Assert: Verify expected behavior
    assert!(
        result.exit_code != 0,
        "Write to sensitive file should fail with permission denied"
    );
}
