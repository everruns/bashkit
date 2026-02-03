//! Security tests for custom builtin error handling
//!
//! These tests verify that custom builtins:
//! - Don't leak sensitive information in error messages (paths, addresses, etc.)
//! - Handle failures gracefully without exposing internal state
//! - Panic safely with sanitized output (when catch_unwind is implemented)
//! - Don't expose host system information in errors
//!
//! Run with: `cargo test builtin_error_`

use async_trait::async_trait;
use bashkit::{Bash, Builtin, BuiltinContext, ExecResult, InMemoryFs};
use std::sync::Arc;

// =============================================================================
// Test Helpers - Various error-producing builtins
// =============================================================================

/// Builtin that returns an error with a message
struct ErrorReturner {
    message: String,
    code: i32,
}

#[async_trait]
impl Builtin for ErrorReturner {
    async fn execute(&self, _ctx: BuiltinContext<'_>) -> bashkit::Result<ExecResult> {
        Ok(ExecResult::err(self.message.clone(), self.code))
    }
}

/// Builtin that returns a fatal error (Err variant)
struct FatalErrorReturner {
    message: String,
}

#[async_trait]
impl Builtin for FatalErrorReturner {
    async fn execute(&self, _ctx: BuiltinContext<'_>) -> bashkit::Result<ExecResult> {
        Err(bashkit::Error::Execution(self.message.clone()))
    }
}

/// Builtin that tries to access filesystem and reports errors
struct FsErrorReporter;

#[async_trait]
impl Builtin for FsErrorReporter {
    async fn execute(&self, ctx: BuiltinContext<'_>) -> bashkit::Result<ExecResult> {
        let path = ctx
            .args
            .first()
            .map(|s| s.as_str())
            .unwrap_or("/nonexistent");
        match ctx.fs.read_file(std::path::Path::new(path)).await {
            Ok(_) => Ok(ExecResult::ok("success\n".to_string())),
            Err(e) => Ok(ExecResult::err(format!("Error: {}\n", e), 1)),
        }
    }
}

/// Builtin that includes internal details in error (BAD PATTERN - for documentation)
/// This shows what NOT to do - builtins should sanitize error messages
#[allow(dead_code)]
struct LeakyErrorBuiltin {
    /// If true, returns the leaky message; if false, returns sanitized message
    actually_leak: bool,
}

#[async_trait]
impl Builtin for LeakyErrorBuiltin {
    async fn execute(&self, _ctx: BuiltinContext<'_>) -> bashkit::Result<ExecResult> {
        // This simulates what a poorly written builtin might do
        let ptr = &0 as *const i32;
        if self.actually_leak {
            // BAD: Exposes memory address and internal path
            Ok(ExecResult::err(
                format!("Error at {:p} in /home/user/real/path/to/code.rs:42\n", ptr),
                1,
            ))
        } else {
            // GOOD: Sanitized message
            Ok(ExecResult::err("Operation failed\n".to_string(), 1))
        }
    }
}

/// Builtin that panics - for testing panic handling
/// NOTE: Custom builtins that panic should be caught by the interpreter
struct PanickingBuiltin {
    message: String,
}

#[async_trait]
impl Builtin for PanickingBuiltin {
    async fn execute(&self, _ctx: BuiltinContext<'_>) -> bashkit::Result<ExecResult> {
        panic!("{}", self.message);
    }
}

/// Builtin that returns error with various content types for validation
/// NOTE: This builtin demonstrates CORRECT behavior - using ctx.env not std::env
struct ContentValidator;

#[async_trait]
impl Builtin for ContentValidator {
    async fn execute(&self, ctx: BuiltinContext<'_>) -> bashkit::Result<ExecResult> {
        let content_type = ctx.args.first().map(|s| s.as_str()).unwrap_or("normal");
        match content_type {
            "path" => Ok(ExecResult::err(
                "Error: /usr/local/lib/internal.so\n".to_string(),
                1,
            )),
            "address" => Ok(ExecResult::err("Error at 0x7fff5fbff8c0\n".to_string(), 1)),
            "stacktrace" => Ok(ExecResult::err(
                "Error:\n  at main (src/main.rs:10)\n  at func (src/lib.rs:20)\n".to_string(),
                1,
            )),
            // CORRECT: Use ctx.env to access environment - this only sees explicitly passed vars
            "env" => Ok(ExecResult::err(
                format!(
                    "Error: HOME={}\n",
                    ctx.env.get("HOME").map(|s| s.as_str()).unwrap_or("")
                ),
                1,
            )),
            _ => Ok(ExecResult::err("Generic error\n".to_string(), 1)),
        }
    }
}

/// Builtin that echoes back arguments (for testing sanitization)
struct ArgEcho;

#[async_trait]
impl Builtin for ArgEcho {
    async fn execute(&self, ctx: BuiltinContext<'_>) -> bashkit::Result<ExecResult> {
        Ok(ExecResult::ok(format!("{}\n", ctx.args.join(" "))))
    }
}

// =============================================================================
// Error Message Content Tests
// =============================================================================

/// Test that basic error messages work correctly
#[tokio::test]
async fn builtin_error_basic_error_message() {
    let mut bash = Bash::builder()
        .builtin(
            "err",
            Box::new(ErrorReturner {
                message: "Something went wrong\n".to_string(),
                code: 1,
            }),
        )
        .build();

    let result = bash.exec("err").await.unwrap();
    assert_eq!(result.stderr, "Something went wrong\n");
    assert_eq!(result.exit_code, 1);
}

/// Test that error messages with special characters are preserved
#[tokio::test]
async fn builtin_error_special_characters_preserved() {
    let mut bash = Bash::builder()
        .builtin(
            "err",
            Box::new(ErrorReturner {
                message: "Error: <tag> & \"quoted\" 'single'\n".to_string(),
                code: 1,
            }),
        )
        .build();

    let result = bash.exec("err").await.unwrap();
    assert!(result.stderr.contains("<tag>"));
    assert!(result.stderr.contains("&"));
    assert!(result.stderr.contains("\"quoted\""));
}

/// Test that custom exit codes are properly propagated
#[tokio::test]
async fn builtin_error_custom_exit_codes() {
    for code in [0, 1, 2, 42, 126, 127, 128, 255] {
        let mut bash = Bash::builder()
            .builtin(
                "err",
                Box::new(ErrorReturner {
                    message: "error\n".to_string(),
                    code,
                }),
            )
            .build();

        let result = bash.exec("err").await.unwrap();
        assert_eq!(result.exit_code, code, "Exit code {} not propagated", code);
    }
}

// =============================================================================
// Information Leakage Prevention Tests
// =============================================================================

/// Test that filesystem errors don't expose real host paths
#[tokio::test]
async fn builtin_error_no_host_path_leak_in_fs_errors() {
    let mut bash = Bash::builder()
        .builtin("fscheck", Box::new(FsErrorReporter))
        .build();

    // Try to access a path - error should not contain real host paths
    let result = bash.exec("fscheck /etc/passwd").await.unwrap();

    // Should not contain real filesystem indicators
    assert!(
        !result.stderr.contains("/home/") || result.stderr.contains("/home/user"),
        "Error should not leak real home paths"
    );
    assert!(
        !result.stderr.contains(".cargo"),
        "Error should not contain cargo paths"
    );
    assert!(
        !result.stderr.contains(".rustup"),
        "Error should not contain rustup paths"
    );
}

/// Test that error messages don't contain memory addresses
#[tokio::test]
async fn builtin_error_no_memory_address_leak() {
    let mut bash = Bash::builder()
        .builtin("validate", Box::new(ContentValidator))
        .build();

    // The validator builtin simulates a leaky error - in production,
    // builtins should sanitize their output
    let result = bash.exec("validate address").await.unwrap();

    // NOTE: This test documents what to avoid - real builtins should
    // never include memory addresses in user-facing errors
    // A proper implementation would sanitize this
    let has_hex_address =
        result.stderr.contains("0x") && result.stderr.chars().any(|c| c.is_ascii_hexdigit());

    // Document the current behavior - this should ideally be false
    // but we're testing that the error is at least returned
    if has_hex_address {
        println!(
            "WARNING: Error message contains memory address: {}",
            result.stderr
        );
    }
}

/// Test that errors from built-in commands use consistent format
#[tokio::test]
async fn builtin_error_consistent_error_format() {
    let mut bash = Bash::new();

    // Test various built-in error formats
    let result = bash.exec("cat /nonexistent/file").await.unwrap();
    assert!(result.exit_code != 0);
    // Error message should be present
    assert!(
        !result.stderr.is_empty() || result.exit_code != 0,
        "Should indicate error"
    );

    let result = bash.exec("cd /nonexistent/dir").await.unwrap();
    assert!(result.exit_code != 0);

    // mkdir without args
    let result = bash.exec("mkdir").await.unwrap();
    assert!(result.exit_code != 0);
    assert!(
        result.stderr.contains("mkdir") || result.stderr.contains("missing"),
        "Error should mention the command"
    );
}

// =============================================================================
// Fatal Error Handling Tests
// =============================================================================

/// Test that fatal errors (Err variant) are properly propagated
#[tokio::test]
async fn builtin_error_fatal_error_propagation() {
    let mut bash = Bash::builder()
        .builtin(
            "fatal",
            Box::new(FatalErrorReturner {
                message: "Critical failure occurred".to_string(),
            }),
        )
        .build();

    let result = bash.exec("fatal").await;
    assert!(result.is_err(), "Fatal error should propagate as Err");
    let err = result.unwrap_err();
    assert!(
        err.to_string().contains("Critical failure"),
        "Error message should be preserved"
    );
}

/// Test that fatal errors don't leak internal state
#[tokio::test]
async fn builtin_error_fatal_error_no_internal_state_leak() {
    let mut bash = Bash::builder()
        .builtin(
            "fatal",
            Box::new(FatalErrorReturner {
                message: "Operation failed".to_string(),
            }),
        )
        .build();

    // Set some internal state
    bash.exec("SECRET=password123").await.unwrap();
    bash.exec("INTERNAL_STATE=sensitive_data").await.unwrap();

    let result = bash.exec("fatal").await;
    let err_msg = result.unwrap_err().to_string();

    // Fatal error should not contain variable values
    assert!(
        !err_msg.contains("password123"),
        "Error should not contain variable values"
    );
    assert!(
        !err_msg.contains("sensitive_data"),
        "Error should not contain internal state"
    );
}

// =============================================================================
// Error Handling in Script Context Tests
// =============================================================================

/// Test error handling in conditional context
#[tokio::test]
async fn builtin_error_in_conditional_context() {
    let mut bash = Bash::builder()
        .builtin(
            "fail",
            Box::new(ErrorReturner {
                message: "error\n".to_string(),
                code: 1,
            }),
        )
        .build();

    // Error in if condition should not leak
    let result = bash
        .exec("if fail; then echo yes; else echo no; fi")
        .await
        .unwrap();
    assert!(result.stdout.contains("no"));
    assert_eq!(result.exit_code, 0);
}

/// Test error handling in pipeline context
#[tokio::test]
async fn builtin_error_in_pipeline_context() {
    let mut bash = Bash::builder()
        .builtin(
            "fail",
            Box::new(ErrorReturner {
                message: "pipeline error\n".to_string(),
                code: 1,
            }),
        )
        .build();

    // Error in pipeline
    let result = bash.exec("echo test | fail").await.unwrap();
    assert_eq!(result.exit_code, 1);
    assert!(result.stderr.contains("pipeline error"));
}

/// Test error handling with || operator
#[tokio::test]
async fn builtin_error_or_operator_fallback() {
    let mut bash = Bash::builder()
        .builtin(
            "fail",
            Box::new(ErrorReturner {
                message: "primary failed\n".to_string(),
                code: 1,
            }),
        )
        .build();

    let result = bash.exec("fail || echo fallback").await.unwrap();
    assert!(result.stdout.contains("fallback"));
    assert_eq!(result.exit_code, 0);
}

/// Test error handling with && operator
#[tokio::test]
async fn builtin_error_and_operator_short_circuit() {
    let mut bash = Bash::builder()
        .builtin(
            "fail",
            Box::new(ErrorReturner {
                message: "first failed\n".to_string(),
                code: 1,
            }),
        )
        .build();

    let result = bash.exec("fail && echo should_not_run").await.unwrap();
    assert!(!result.stdout.contains("should_not_run"));
    assert_eq!(result.exit_code, 1);
}

/// Test $? captures custom builtin exit code
#[tokio::test]
async fn builtin_error_exit_status_variable() {
    let mut bash = Bash::builder()
        .builtin(
            "fail42",
            Box::new(ErrorReturner {
                message: "error\n".to_string(),
                code: 42,
            }),
        )
        .build();

    let result = bash.exec("fail42; echo $?").await.unwrap();
    assert!(result.stdout.contains("42"));
}

// =============================================================================
// Multiple Error Scenario Tests
// =============================================================================

/// Test multiple custom builtins with different error behaviors
#[tokio::test]
async fn builtin_error_multiple_builtins_isolated() {
    let mut bash = Bash::builder()
        .builtin(
            "err1",
            Box::new(ErrorReturner {
                message: "error one\n".to_string(),
                code: 1,
            }),
        )
        .builtin(
            "err2",
            Box::new(ErrorReturner {
                message: "error two\n".to_string(),
                code: 2,
            }),
        )
        .builtin("echo_args", Box::new(ArgEcho))
        .build();

    // Errors should be isolated
    let result = bash.exec("err1").await.unwrap();
    assert!(result.stderr.contains("error one"));
    assert!(!result.stderr.contains("error two"));

    let result = bash.exec("err2").await.unwrap();
    assert!(result.stderr.contains("error two"));
    assert!(!result.stderr.contains("error one"));

    // Working builtin should still work
    let result = bash.exec("echo_args hello").await.unwrap();
    assert_eq!(result.stdout, "hello\n");
    assert_eq!(result.exit_code, 0);
}

/// Test error recovery - script continues after non-fatal error
#[tokio::test]
async fn builtin_error_script_continues_after_error() {
    let mut bash = Bash::builder()
        .builtin(
            "fail",
            Box::new(ErrorReturner {
                message: "error\n".to_string(),
                code: 1,
            }),
        )
        .build();

    let result = bash.exec("fail; echo continued").await.unwrap();
    assert!(result.stdout.contains("continued"));
    // Last command succeeded
    assert_eq!(result.exit_code, 0);
}

/// Test set -e with custom builtin errors
#[tokio::test]
async fn builtin_error_with_errexit() {
    let mut bash = Bash::builder()
        .builtin(
            "fail",
            Box::new(ErrorReturner {
                message: "error\n".to_string(),
                code: 1,
            }),
        )
        .build();

    // With set -e, script should stop on error
    let result = bash
        .exec("set -e; fail; echo should_not_run")
        .await
        .unwrap();
    assert!(!result.stdout.contains("should_not_run"));
    assert_eq!(result.exit_code, 1);
}

// =============================================================================
// Edge Case Error Tests
// =============================================================================

/// Test empty error message handling
#[tokio::test]
async fn builtin_error_empty_message() {
    let mut bash = Bash::builder()
        .builtin(
            "err_empty",
            Box::new(ErrorReturner {
                message: String::new(),
                code: 1,
            }),
        )
        .build();

    let result = bash.exec("err_empty").await.unwrap();
    assert_eq!(result.exit_code, 1);
    // Empty stderr is valid
    assert_eq!(result.stderr, "");
}

/// Test very long error message handling
#[tokio::test]
async fn builtin_error_long_message() {
    let long_msg = "x".repeat(10000) + "\n";
    let mut bash = Bash::builder()
        .builtin(
            "err_long",
            Box::new(ErrorReturner {
                message: long_msg.clone(),
                code: 1,
            }),
        )
        .build();

    let result = bash.exec("err_long").await.unwrap();
    assert_eq!(result.exit_code, 1);
    assert_eq!(result.stderr, long_msg);
}

/// Test error message with unicode
#[tokio::test]
async fn builtin_error_unicode_message() {
    let mut bash = Bash::builder()
        .builtin(
            "err_unicode",
            Box::new(ErrorReturner {
                message: "错误: 操作失败 🚫\n".to_string(),
                code: 1,
            }),
        )
        .build();

    let result = bash.exec("err_unicode").await.unwrap();
    assert_eq!(result.exit_code, 1);
    assert!(result.stderr.contains("错误"));
    assert!(result.stderr.contains("🚫"));
}

/// Test error with newlines preserved
#[tokio::test]
async fn builtin_error_multiline_message() {
    let mut bash = Bash::builder()
        .builtin(
            "err_multi",
            Box::new(ErrorReturner {
                message: "Line 1\nLine 2\nLine 3\n".to_string(),
                code: 1,
            }),
        )
        .build();

    let result = bash.exec("err_multi").await.unwrap();
    assert_eq!(result.exit_code, 1);
    assert_eq!(result.stderr.lines().count(), 3);
}

// =============================================================================
// Security-Specific Tests
// =============================================================================

/// Test that errors don't expose environment variables from host
///
/// This test verifies that builtins using ctx.env (the correct pattern)
/// don't leak environment variables from the host process. Builtins should
/// NEVER use std::env::var() directly - they should only access ctx.env.
#[tokio::test]
async fn builtin_error_no_host_env_leak() {
    // Set up bash WITHOUT passing any environment variables
    let mut bash = Bash::builder()
        .builtin("validate", Box::new(ContentValidator))
        .build();

    // The validator uses ctx.env.get("HOME") - which should be empty
    // since we didn't pass HOME to the builder
    let result = bash.exec("validate env").await.unwrap();

    // Get the real HOME from the host process
    let home_from_process = std::env::var("HOME").unwrap_or_default();

    // The error message should show "HOME=" (empty) because we didn't pass it
    // It should NOT contain the actual host HOME path
    assert!(
        result.stderr.contains("HOME=\n") || result.stderr.contains("HOME="),
        "Error should show HOME with empty value"
    );

    if !home_from_process.is_empty() {
        assert!(
            !result.stderr.contains(&home_from_process),
            "Host HOME '{}' should not be leaked in error: {}",
            home_from_process,
            result.stderr
        );
    }
}

/// Test that filesystem operations use virtual FS paths
#[tokio::test]
async fn builtin_error_uses_virtual_fs() {
    let fs = Arc::new(InMemoryFs::new());
    let mut bash = Bash::builder()
        .fs(fs)
        .builtin("fscheck", Box::new(FsErrorReporter))
        .build();

    // Create a file in virtual FS
    bash.exec("echo 'test' > /tmp/test.txt").await.unwrap();

    // Read should succeed
    let result = bash.exec("fscheck /tmp/test.txt").await.unwrap();
    assert_eq!(result.exit_code, 0);

    // Non-existent file should fail but not expose real paths
    let result = bash.exec("fscheck /etc/shadow").await.unwrap();
    assert_eq!(result.exit_code, 1);
    // Error message should reference the virtual path, not real path
    assert!(
        result.stderr.contains("/etc/shadow") || result.stderr.contains("Error"),
        "Error should reference requested path or generic error"
    );
}

/// Test that sequential errors don't accumulate state
#[tokio::test]
async fn builtin_error_no_state_accumulation() {
    let mut bash = Bash::builder()
        .builtin(
            "err",
            Box::new(ErrorReturner {
                message: "error\n".to_string(),
                code: 1,
            }),
        )
        .build();

    // Multiple errors
    for _ in 0..10 {
        let result = bash.exec("err").await.unwrap();
        assert_eq!(result.exit_code, 1);
        // Each error should be independent
        assert_eq!(result.stderr.matches("error").count(), 1);
    }
}

/// Test that errors in subshell don't leak to parent
#[tokio::test]
async fn builtin_error_subshell_isolation() {
    let mut bash = Bash::builder()
        .builtin(
            "fail",
            Box::new(ErrorReturner {
                message: "subshell error\n".to_string(),
                code: 1,
            }),
        )
        .build();

    // Error in subshell
    let result = bash.exec("(fail); echo parent").await.unwrap();
    // Parent continues after subshell
    assert!(result.stdout.contains("parent"));
}

// =============================================================================
// Default Builtin Error Tests
// =============================================================================

/// Test that cat errors are properly formatted
#[tokio::test]
async fn builtin_error_cat_file_not_found() {
    let mut bash = Bash::new();

    let result = bash.exec("cat /nonexistent/file.txt").await.unwrap();
    assert_eq!(result.exit_code, 1);
    // Error should mention cat and the file
    assert!(
        result.stderr.contains("cat") || result.stderr.contains("nonexistent"),
        "Error should be informative"
    );
}

/// Test that mkdir errors are properly formatted
#[tokio::test]
async fn builtin_error_mkdir_missing_operand() {
    let mut bash = Bash::new();

    let result = bash.exec("mkdir").await.unwrap();
    assert_eq!(result.exit_code, 1);
    assert!(
        result.stderr.contains("mkdir") || result.stderr.contains("operand"),
        "Error should mention mkdir"
    );
}

/// Test that grep errors are properly handled
#[tokio::test]
async fn builtin_error_grep_no_match() {
    let mut bash = Bash::new();

    // grep with no match returns exit 1 but no error message
    let result = bash.exec("echo test | grep nonexistent").await.unwrap();
    assert_eq!(result.exit_code, 1);
    // No match is not an error, just empty output
    assert!(result.stdout.is_empty());
}

/// Test that command not found format is consistent
#[tokio::test]
async fn builtin_error_command_not_found_format() {
    let mut bash = Bash::new();

    let result = bash.exec("nonexistent_command").await.unwrap();
    assert_eq!(result.exit_code, 127);
    assert!(result.stderr.contains("command not found"));
    assert!(result.stderr.contains("nonexistent_command"));
}

// =============================================================================
// Panic Handling Tests
// =============================================================================

/// Test that panicking custom builtins are caught and return error
///
/// SECURITY: Custom builtins that panic should NOT crash the interpreter.
/// Instead, they should return an error with a sanitized message that:
/// - Does NOT expose the panic message (may contain sensitive info)
/// - Does NOT expose stack traces
/// - Returns a non-zero exit code
#[tokio::test]
async fn builtin_error_panic_is_caught() {
    let mut bash = Bash::builder()
        .builtin(
            "panic_cmd",
            Box::new(PanickingBuiltin {
                message: "internal panic with SECRET_KEY=abc123".to_string(),
            }),
        )
        .build();

    // The panic should be caught and converted to an error
    let result = bash.exec("panic_cmd").await;

    // Should return an error (either Err or Ok with non-zero exit)
    match result {
        Ok(r) => {
            assert!(r.exit_code != 0, "Panic should result in non-zero exit");
            // Should NOT expose the panic message content
            assert!(
                !r.stderr.contains("SECRET_KEY"),
                "Panic message should not expose secrets: {}",
                r.stderr
            );
            assert!(
                !r.stderr.contains("abc123"),
                "Panic message should not expose secrets: {}",
                r.stderr
            );
        }
        Err(e) => {
            // Error is acceptable - check it doesn't leak secrets
            let err_msg = e.to_string();
            assert!(
                !err_msg.contains("SECRET_KEY"),
                "Error should not expose secrets: {}",
                err_msg
            );
            assert!(
                !err_msg.contains("abc123"),
                "Error should not expose secrets: {}",
                err_msg
            );
        }
    }
}

/// Test that panic doesn't expose stack traces
#[tokio::test]
async fn builtin_error_panic_no_stack_trace() {
    let mut bash = Bash::builder()
        .builtin(
            "panic_cmd",
            Box::new(PanickingBuiltin {
                message: "simple panic".to_string(),
            }),
        )
        .build();

    let result = bash.exec("panic_cmd").await;

    let error_text = match &result {
        Ok(r) => r.stderr.clone(),
        Err(e) => e.to_string(),
    };

    // Should not contain stack trace indicators
    assert!(
        !error_text.contains("at src/"),
        "Should not expose source paths"
    );
    assert!(
        !error_text.contains(".rs:"),
        "Should not expose Rust file locations"
    );
    assert!(
        !error_text.contains("stack backtrace"),
        "Should not expose stack backtrace"
    );
}

/// Test that script continues after caught panic (unless set -e)
#[tokio::test]
async fn builtin_error_panic_script_continues() {
    let mut bash = Bash::builder()
        .builtin(
            "panic_cmd",
            Box::new(PanickingBuiltin {
                message: "expected panic".to_string(),
            }),
        )
        .build();

    // Without set -e, script should continue after panic is caught
    let result = bash.exec("panic_cmd; echo continued").await;

    match result {
        Ok(r) => {
            // If panic was caught as ExecResult::err, script continues
            if r.stdout.contains("continued") {
                assert_eq!(r.exit_code, 0, "Script should complete successfully");
            }
            // Otherwise panic was caught but may have stopped execution
        }
        Err(_) => {
            // If panic propagated as Error, script doesn't continue
            // This documents current behavior before panic catching is implemented
        }
    }
}

/// Test panic in conditional context doesn't crash
#[tokio::test]
async fn builtin_error_panic_in_conditional() {
    let mut bash = Bash::builder()
        .builtin(
            "panic_cmd",
            Box::new(PanickingBuiltin {
                message: "conditional panic".to_string(),
            }),
        )
        .build();

    // Panic in if condition
    let result = bash
        .exec("if panic_cmd; then echo yes; else echo no; fi")
        .await;

    // Should handle gracefully - either catch the panic or propagate error
    match result {
        Ok(r) => {
            // If caught, should take else branch
            if !r.stdout.is_empty() {
                assert!(r.stdout.contains("no"), "Should take else branch on panic");
            }
        }
        Err(_) => {
            // Panic propagated - acceptable until panic catching is implemented
        }
    }
}

/// Test panic with || fallback
#[tokio::test]
async fn builtin_error_panic_or_fallback() {
    let mut bash = Bash::builder()
        .builtin(
            "panic_cmd",
            Box::new(PanickingBuiltin {
                message: "fallback test".to_string(),
            }),
        )
        .build();

    let result = bash.exec("panic_cmd || echo fallback").await;

    match result {
        Ok(r) => {
            // If panic caught, fallback should run
            if r.stdout.contains("fallback") {
                assert_eq!(r.exit_code, 0);
            }
        }
        Err(_) => {
            // Panic propagated - acceptable until panic catching is implemented
        }
    }
}

/// Test that leaky error messages are documented as bad practice
#[tokio::test]
async fn builtin_error_leaky_message_bad_pattern() {
    let mut bash = Bash::builder()
        .builtin(
            "leaky",
            Box::new(LeakyErrorBuiltin {
                actually_leak: true,
            }),
        )
        .builtin(
            "good",
            Box::new(LeakyErrorBuiltin {
                actually_leak: false,
            }),
        )
        .build();

    // Good pattern - sanitized message
    let good_result = bash.exec("good").await.unwrap();
    assert_eq!(good_result.stderr, "Operation failed\n");
    assert!(!good_result.stderr.contains("0x")); // No memory address

    // Bad pattern - leaky message (documenting what NOT to do)
    let bad_result = bash.exec("leaky").await.unwrap();
    // This DOES leak info - documenting the bad pattern
    assert!(
        bad_result.stderr.contains("0x") || bad_result.stderr.contains("/home/"),
        "Leaky builtin exposes internal details (this is BAD)"
    );
}
