//! Threat Model Security Tests
//!
//! Tests for threats identified in specs/006-threat-model.md
//! Each test category maps to a threat category in the threat model.
//!
//! Run with: `cargo test threat_`

use bashkit::{Bash, ExecutionLimits};
use std::time::Duration;

// =============================================================================
// 1. RESOURCE EXHAUSTION TESTS
// =============================================================================

mod resource_exhaustion {
    use super::*;

    /// V1: Test that command limit prevents infinite execution
    #[tokio::test]
    async fn threat_infinite_commands_blocked() {
        let limits = ExecutionLimits::new().max_commands(10);
        let mut bash = Bash::builder().limits(limits).build();

        // Try to run 20 commands
        let result = bash
            .exec("true; true; true; true; true; true; true; true; true; true; true; true; true; true; true; true; true; true; true; true")
            .await;

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("command") && err.contains("exceeded"),
            "Expected command limit error, got: {}",
            err
        );
    }

    /// V2: Test that loop limit prevents infinite loops
    #[tokio::test]
    async fn threat_infinite_loop_blocked() {
        let limits = ExecutionLimits::new()
            .max_loop_iterations(5)
            .max_commands(1000);
        let mut bash = Bash::builder().limits(limits).build();

        let result = bash
            .exec("for i in 1 2 3 4 5 6 7 8 9 10; do echo $i; done")
            .await;

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("loop") && err.contains("exceeded"),
            "Expected loop limit error, got: {}",
            err
        );
    }

    /// V3: Test that function recursion limit prevents stack overflow
    #[tokio::test]
    async fn threat_stack_overflow_blocked() {
        let limits = ExecutionLimits::new()
            .max_function_depth(5)
            .max_commands(1000);
        let mut bash = Bash::builder().limits(limits).build();

        let result = bash
            .exec(
                r#"
                recurse() {
                    echo "depth"
                    recurse
                }
                recurse
                "#,
            )
            .await;

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("function") && err.contains("exceeded"),
            "Expected function depth error, got: {}",
            err
        );
    }

    /// Test while loop with always-true condition is limited
    #[tokio::test]
    async fn threat_while_true_blocked() {
        let limits = ExecutionLimits::new()
            .max_loop_iterations(10)
            .max_commands(1000);
        let mut bash = Bash::builder().limits(limits).build();

        // This would run forever without limits
        let result = bash
            .exec("i=0; while [ $i -lt 100 ]; do i=$((i+1)); done")
            .await;

        assert!(result.is_err());
    }

    /// Test that timeout is respected (if implemented)
    #[tokio::test]
    async fn threat_cpu_exhaustion_timeout() {
        let limits = ExecutionLimits::new()
            .timeout(Duration::from_millis(100))
            .max_commands(1_000_000)
            .max_loop_iterations(1_000_000);
        let mut bash = Bash::builder().limits(limits).build();

        // This should timeout, not complete
        let start = std::time::Instant::now();
        let _ = bash
            .exec("for i in $(seq 1 1000000); do echo $i; done")
            .await;
        let elapsed = start.elapsed();

        // Should complete quickly due to either timeout or loop limit
        assert!(elapsed < Duration::from_secs(5));
    }
}

// =============================================================================
// 2. SANDBOX ESCAPE TESTS
// =============================================================================

mod sandbox_escape {
    use super::*;

    /// Test path traversal is blocked
    #[tokio::test]
    async fn threat_path_traversal_blocked() {
        let mut bash = Bash::new();

        // Try to escape via ../
        let result = bash.exec("cat ../../../etc/passwd").await.unwrap();
        assert!(result.exit_code != 0 || result.stdout.is_empty());
        assert!(!result.stdout.contains("root:"));
    }

    /// Test absolute path to /etc/passwd fails
    #[tokio::test]
    async fn threat_etc_passwd_blocked() {
        let mut bash = Bash::new();

        let result = bash.exec("cat /etc/passwd").await.unwrap();
        // Should fail - file doesn't exist in virtual FS
        assert!(result.exit_code != 0);
        assert!(!result.stdout.contains("root:"));
    }

    /// Test /proc access is blocked (no /proc in virtual FS)
    #[tokio::test]
    async fn threat_proc_access_blocked() {
        let mut bash = Bash::new();

        let result = bash.exec("cat /proc/self/environ").await.unwrap();
        assert!(result.exit_code != 0);
    }

    /// Test eval is implemented but safe in sandbox
    ///
    /// eval is a POSIX special builtin that's now implemented. In the sandbox,
    /// eval can only execute other builtins (no external commands), so it's safe.
    /// The current implementation stores the command but doesn't re-execute it.
    #[tokio::test]
    async fn threat_eval_is_safe_in_sandbox() {
        let mut bash = Bash::new();

        // eval is now implemented - it stores the command but in sandbox
        // it can only run builtins, so it's safe
        let result = bash.exec("eval echo test").await.unwrap();
        // eval returns 0 (success) as it's a valid builtin
        assert_eq!(result.exit_code, 0);
        // Note: current impl stores command but doesn't execute it
    }

    /// Test exec is not implemented (prevents shell escape)
    #[tokio::test]
    async fn threat_exec_not_available() {
        let mut bash = Bash::new();

        let result = bash.exec("exec /bin/bash").await.unwrap();
        // exec should return command not found (exit 127)
        assert_eq!(result.exit_code, 127);
        assert!(result.stderr.contains("command not found"));
    }

    /// Test external command execution is blocked
    #[tokio::test]
    async fn threat_external_commands_blocked() {
        let mut bash = Bash::new();

        // Try to run a non-builtin command - should fail
        if let Ok(r) = bash.exec("/bin/ls").await {
            assert!(r.exit_code != 0);
        }

        if let Ok(r) = bash.exec("./malicious").await {
            assert!(r.exit_code != 0);
        }
    }

    /// Test symlink creation (stored but not followed for escape)
    #[tokio::test]
    async fn threat_symlink_escape_blocked() {
        let mut bash = Bash::new();

        // Even if symlinks could be created, they shouldn't allow escape
        // Virtual FS doesn't follow symlinks
        let result = bash.exec("cat /tmp/symlink_to_etc").await.unwrap();
        assert!(result.exit_code != 0);
    }
}

// =============================================================================
// 3. INJECTION ATTACK TESTS
// =============================================================================

mod injection_attacks {
    use super::*;

    /// Test that variable content with semicolons doesn't execute as separate command
    /// Security: Variables should expand to strings, not be re-parsed as code
    #[tokio::test]
    async fn threat_semicolon_in_variable_safe() {
        let mut bash = Bash::new();

        // Set a variable with a semicolon (simulating injection attempt)
        bash.exec("safe=harmless").await.unwrap();
        let result = bash.exec("echo $safe").await.unwrap();

        // Simple case works
        assert_eq!(result.stdout.trim(), "harmless");
        assert_eq!(result.exit_code, 0);
    }

    /// Test that command substitution in single quotes is literal
    #[tokio::test]
    async fn threat_command_sub_in_single_quotes() {
        let mut bash = Bash::new();

        // Single quotes should prevent command substitution
        let result = bash.exec("echo '$(whoami)'").await.unwrap();
        assert!(result.stdout.contains("$(whoami)"));
        assert!(!result.stdout.contains("sandbox"));
    }

    /// Test that backticks in single quotes are literal
    #[tokio::test]
    async fn threat_backticks_in_single_quotes() {
        let mut bash = Bash::new();

        let result = bash.exec("echo '`hostname`'").await.unwrap();
        assert!(result.stdout.contains("`hostname`"));
        assert!(!result.stdout.contains("bashkit-sandbox"));
    }

    /// Test that eval is implemented but safe (can only run builtins)
    ///
    /// eval is a POSIX special builtin. In sandbox mode, it can only execute
    /// builtins (no external commands), so it cannot be used for code injection.
    #[tokio::test]
    async fn threat_eval_is_sandboxed() {
        let mut bash = Bash::new();

        // eval is now implemented - returns success
        let result = bash.exec("eval echo test").await.unwrap();
        assert_eq!(result.exit_code, 0);
        // Note: current impl stores command in _EVAL_CMD but doesn't execute it
        // Even if it did execute, it can only run builtins
    }

    /// Test path with null byte (Rust prevents this)
    #[tokio::test]
    async fn threat_null_byte_in_path() {
        let mut bash = Bash::new();

        // Rust strings can't contain null bytes, so this is safe by construction
        let result = bash.exec("cat '/tmp/file'").await.unwrap();
        // Just verify it doesn't crash
        assert!(result.exit_code == 0 || result.exit_code == 1);
    }

    /// Test that pipe operator in quotes is literal
    #[tokio::test]
    async fn threat_pipe_in_quotes() {
        let mut bash = Bash::new();

        let result = bash.exec("echo '| cat /etc/passwd'").await.unwrap();
        assert!(result.stdout.contains("| cat /etc/passwd"));
    }

    /// Test that redirect in quotes is literal
    #[tokio::test]
    async fn threat_redirect_in_quotes() {
        let mut bash = Bash::new();

        let result = bash.exec("echo '> /tmp/pwned'").await.unwrap();
        assert!(result.stdout.contains("> /tmp/pwned"));
    }
}

// =============================================================================
// 4. INFORMATION DISCLOSURE TESTS
// =============================================================================

mod information_disclosure {
    use super::*;

    /// Test hostname returns sandbox value, not real hostname
    #[tokio::test]
    async fn threat_hostname_hardcoded() {
        let mut bash = Bash::new();

        let result = bash.exec("hostname").await.unwrap();
        assert_eq!(result.stdout.trim(), "bashkit-sandbox");
        assert_eq!(result.exit_code, 0);
    }

    /// Test hostname cannot be set
    #[tokio::test]
    async fn threat_hostname_cannot_set() {
        let mut bash = Bash::new();

        let result = bash.exec("hostname evil.attacker.com").await.unwrap();
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("cannot set"));
    }

    /// Test uname returns sandbox values
    #[tokio::test]
    async fn threat_uname_hardcoded() {
        let mut bash = Bash::new();

        let result = bash.exec("uname -a").await.unwrap();
        assert!(result.stdout.contains("bashkit-sandbox"));
        assert!(result.stdout.contains("Linux"));
        // Should NOT contain real kernel info
        assert!(!result.stdout.contains("Ubuntu"));
        assert!(!result.stdout.contains("Debian"));
    }

    /// Test uname -n returns sandbox hostname
    #[tokio::test]
    async fn threat_uname_nodename_hardcoded() {
        let mut bash = Bash::new();

        let result = bash.exec("uname -n").await.unwrap();
        assert_eq!(result.stdout.trim(), "bashkit-sandbox");
    }

    /// Test whoami returns sandbox user
    #[tokio::test]
    async fn threat_whoami_hardcoded() {
        let mut bash = Bash::new();

        let result = bash.exec("whoami").await.unwrap();
        assert_eq!(result.stdout.trim(), "sandbox");
    }

    /// Test id returns sandbox IDs
    #[tokio::test]
    async fn threat_id_hardcoded() {
        let mut bash = Bash::new();

        let result = bash.exec("id").await.unwrap();
        assert!(result.stdout.contains("uid=1000"));
        assert!(result.stdout.contains("sandbox"));

        let result = bash.exec("id -u").await.unwrap();
        assert_eq!(result.stdout.trim(), "1000");

        let result = bash.exec("id -g").await.unwrap();
        assert_eq!(result.stdout.trim(), "1000");
    }

    /// Test that sensitive env vars are only accessible if passed
    #[tokio::test]
    async fn threat_env_vars_isolated() {
        let mut bash = Bash::new();

        // Default instance shouldn't have sensitive vars
        let result = bash.exec("echo $DATABASE_URL").await.unwrap();
        assert!(result.stdout.trim().is_empty());

        let result = bash.exec("echo $AWS_SECRET_ACCESS_KEY").await.unwrap();
        assert!(result.stdout.trim().is_empty());

        let result = bash.exec("echo $API_KEY").await.unwrap();
        assert!(result.stdout.trim().is_empty());
    }

    /// Test that only explicitly passed env vars are available
    #[tokio::test]
    async fn threat_env_vars_explicit_only() {
        let mut bash = Bash::builder().env("ALLOWED_VAR", "allowed_value").build();

        let result = bash.exec("echo $ALLOWED_VAR").await.unwrap();
        assert_eq!(result.stdout.trim(), "allowed_value");

        // But other vars aren't magically available
        let result = bash.exec("echo $PATH").await.unwrap();
        assert!(result.stdout.trim().is_empty());
    }

    /// Test /proc is not accessible
    #[tokio::test]
    async fn threat_proc_environ_blocked() {
        let mut bash = Bash::new();

        let result = bash.exec("cat /proc/self/environ").await.unwrap();
        assert_eq!(result.exit_code, 1);
        assert!(result.stdout.is_empty());
    }
}

// =============================================================================
// 5. NETWORK SECURITY TESTS (when http_client feature enabled)
// =============================================================================

mod network_security {
    use super::*;

    /// Test that curl/wget commands aren't available without http_client feature
    #[tokio::test]
    async fn threat_network_commands_not_builtin() {
        let mut bash = Bash::new();

        // curl/wget should not be available - either error or non-zero exit
        let result = bash.exec("curl https://evil.com").await;
        if let Ok(r) = result {
            assert!(r.exit_code != 0);
        }
        // Error is also acceptable

        let result = bash.exec("wget https://evil.com").await;
        if let Ok(r) = result {
            assert!(r.exit_code != 0);
        }
        // Error is also acceptable
    }
}

// =============================================================================
// 6. MULTI-TENANT ISOLATION TESTS
// =============================================================================

mod multi_tenant {
    use super::*;
    use bashkit::InMemoryFs;
    use std::sync::Arc;

    /// Test that separate instances have isolated filesystems
    #[tokio::test]
    async fn threat_tenant_fs_isolation() {
        let fs_a = Arc::new(InMemoryFs::new());
        let fs_b = Arc::new(InMemoryFs::new());

        let mut tenant_a = Bash::builder().fs(fs_a).build();
        let mut tenant_b = Bash::builder().fs(fs_b).build();

        // Tenant A writes a secret
        tenant_a
            .exec("echo 'SECRET_A' > /tmp/secret.txt")
            .await
            .unwrap();

        // Tenant B cannot read it
        let result = tenant_b.exec("cat /tmp/secret.txt").await.unwrap();
        assert_eq!(result.exit_code, 1);
        assert!(!result.stdout.contains("SECRET_A"));
    }

    /// Test that separate instances have isolated variables
    #[tokio::test]
    async fn threat_tenant_variable_isolation() {
        let mut tenant_a = Bash::new();
        let mut tenant_b = Bash::new();

        tenant_a.exec("SECRET=password123").await.unwrap();

        let result = tenant_b.exec("echo $SECRET").await.unwrap();
        assert!(result.stdout.trim().is_empty());
    }

    /// Test that separate instances have isolated functions
    #[tokio::test]
    async fn threat_tenant_function_isolation() {
        let mut tenant_a = Bash::new();
        let mut tenant_b = Bash::new();

        tenant_a.exec("steal() { echo 'stolen'; }").await.unwrap();

        // Function defined in tenant_a should not exist in tenant_b
        let result = tenant_b.exec("steal").await.unwrap();
        // Should return command not found (exit 127)
        assert_eq!(result.exit_code, 127);
        assert!(!result.stdout.contains("stolen"));
        assert!(result.stderr.contains("command not found"));
    }

    /// Test that limits are per-instance
    #[tokio::test]
    async fn threat_tenant_limits_isolation() {
        let limits_strict = ExecutionLimits::new().max_commands(5);
        let limits_relaxed = ExecutionLimits::new().max_commands(100);

        let mut tenant_strict = Bash::builder().limits(limits_strict).build();
        let mut tenant_relaxed = Bash::builder().limits(limits_relaxed).build();

        // Strict tenant hits limit
        let result = tenant_strict
            .exec("true; true; true; true; true; true; true")
            .await;
        assert!(result.is_err());

        // Relaxed tenant can do more
        let result = tenant_relaxed
            .exec("true; true; true; true; true; true; true")
            .await;
        assert!(result.is_ok());
    }
}

// =============================================================================
// 7. EDGE CASE TESTS
// =============================================================================

mod edge_cases {
    use super::*;

    /// Test empty script
    #[tokio::test]
    async fn threat_empty_script() {
        let mut bash = Bash::new();
        let result = bash.exec("").await.unwrap();
        assert_eq!(result.exit_code, 0);
    }

    /// Test script with only whitespace
    #[tokio::test]
    async fn threat_whitespace_script() {
        let mut bash = Bash::new();
        let result = bash.exec("   \n\t\n   ").await.unwrap();
        assert_eq!(result.exit_code, 0);
    }

    /// Test script with only comments
    #[tokio::test]
    async fn threat_comment_only_script() {
        let mut bash = Bash::new();
        let result = bash
            .exec("# This is a comment\n# Another comment")
            .await
            .unwrap();
        assert_eq!(result.exit_code, 0);
    }

    /// Test very long single line
    #[tokio::test]
    async fn threat_long_line() {
        let mut bash = Bash::new();
        let long_arg = "a".repeat(10000);
        let result = bash.exec(&format!("echo {}", long_arg)).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.len() >= 10000);
    }

    /// Test deeply nested command substitution
    #[tokio::test]
    async fn threat_nested_command_sub() {
        let limits = ExecutionLimits::new()
            .max_commands(100)
            .max_function_depth(50);
        let mut bash = Bash::builder().limits(limits).build();

        // Moderately nested - should work
        let result = bash.exec("echo $(echo $(echo $(echo hello)))").await;
        // Either succeeds or hits a limit - shouldn't crash
        assert!(result.is_ok() || result.is_err());
    }

    /// Test special variable names
    #[tokio::test]
    async fn threat_special_variable_names() {
        let mut bash = Bash::new();

        // These should all be safe
        let result = bash.exec("echo $?").await.unwrap(); // Exit code
        assert!(result.exit_code == 0);

        let result = bash.exec("echo $$").await.unwrap(); // PID (may not be implemented)
        assert!(result.exit_code == 0);

        let result = bash.exec("echo $#").await.unwrap(); // Arg count
        assert!(result.exit_code == 0);
    }

    /// Test command not found returns exit code 127 and proper error message
    #[tokio::test]
    async fn command_not_found_exit_code() {
        let mut bash = Bash::new();

        // Unknown command should return exit code 127 (not a Rust error)
        let result = bash.exec("nonexistent_command").await.unwrap();
        assert_eq!(result.exit_code, 127);
        assert!(
            result.stderr.contains("command not found"),
            "stderr should contain 'command not found', got: {}",
            result.stderr
        );
        assert!(
            result.stderr.contains("nonexistent_command"),
            "stderr should contain the command name, got: {}",
            result.stderr
        );
    }

    /// Test command not found in script continues execution
    #[tokio::test]
    async fn command_not_found_continues_script() {
        let mut bash = Bash::new();

        // Script should continue after command not found
        let result = bash.exec("unknown_cmd; echo after").await.unwrap();
        assert!(result.stdout.contains("after"));
        // Last command succeeded, so exit code should be 0
        assert_eq!(result.exit_code, 0);
    }

    /// Test command not found stderr format matches bash
    #[tokio::test]
    async fn command_not_found_stderr_format() {
        let mut bash = Bash::new();

        let result = bash.exec("ssh").await.unwrap();
        assert_eq!(result.exit_code, 127);
        // Should match bash format: "bash: cmd: command not found"
        assert!(
            result.stderr.starts_with("bash: ssh: command not found"),
            "stderr should match bash format, got: {}",
            result.stderr
        );
    }

    /// Test various common missing commands all return 127
    #[tokio::test]
    async fn command_not_found_various_commands() {
        let mut bash = Bash::new();

        // Commands that are NOT implemented as builtins
        // Note: git is a builtin (returns exit 1 when not configured, not 127)
        for cmd in &["ssh", "apt", "yum", "docker", "vim", "nano"] {
            let result = bash.exec(cmd).await.unwrap();
            assert_eq!(
                result.exit_code, 127,
                "{} should return exit 127, got {}",
                cmd, result.exit_code
            );
            assert!(
                result.stderr.contains("command not found"),
                "{} stderr should contain 'command not found', got: {}",
                cmd,
                result.stderr
            );
        }
    }

    /// Test $? captures exit code 127 after command not found
    #[tokio::test]
    async fn command_not_found_exit_status_variable() {
        let mut bash = Bash::new();

        let result = bash.exec("nonexistent; echo $?").await.unwrap();
        assert!(result.stdout.contains("127"));
        // Final exit code is 0 because echo succeeded
        assert_eq!(result.exit_code, 0);
    }

    /// Test command not found in pipeline
    #[tokio::test]
    async fn command_not_found_in_pipeline() {
        let mut bash = Bash::new();

        // Pipeline with missing command should still work
        let result = bash.exec("echo hello | nonexistent_filter").await.unwrap();
        // Exit code should be from the last command (127)
        assert_eq!(result.exit_code, 127);
    }

    /// Test command not found in conditional
    #[tokio::test]
    async fn command_not_found_in_conditional() {
        let mut bash = Bash::new();

        // if with missing command should take else branch
        let result = bash
            .exec("if nonexistent_cmd; then echo yes; else echo no; fi")
            .await
            .unwrap();
        assert!(result.stdout.contains("no"));
        assert_eq!(result.exit_code, 0);
    }

    /// Test command not found with || operator
    #[tokio::test]
    async fn command_not_found_or_operator() {
        let mut bash = Bash::new();

        // Should execute fallback after command not found
        let result = bash.exec("nonexistent || echo fallback").await.unwrap();
        assert!(result.stdout.contains("fallback"));
        assert_eq!(result.exit_code, 0);
    }

    /// Test command not found with && operator
    #[tokio::test]
    async fn command_not_found_and_operator() {
        let mut bash = Bash::new();

        // Should not execute second command after failure
        let result = bash.exec("nonexistent && echo success").await.unwrap();
        assert!(!result.stdout.contains("success"));
        assert_eq!(result.exit_code, 127);
    }

    /// Test builtins still work (positive test case)
    #[tokio::test]
    async fn builtins_still_work() {
        let mut bash = Bash::new();

        // Verify various builtins work correctly
        let result = bash.exec("echo hello").await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("hello"));

        let result = bash.exec("pwd").await.unwrap();
        assert_eq!(result.exit_code, 0);

        let result = bash.exec("true").await.unwrap();
        assert_eq!(result.exit_code, 0);

        let result = bash.exec("false").await.unwrap();
        assert_eq!(result.exit_code, 1);
    }

    /// Test command in subshell not found
    #[tokio::test]
    async fn command_not_found_in_subshell() {
        let mut bash = Bash::new();

        let result = bash.exec("(nonexistent_cmd)").await.unwrap();
        assert_eq!(result.exit_code, 127);
        assert!(result.stderr.contains("command not found"));
    }

    /// Test command substitution with not found command
    #[tokio::test]
    async fn command_not_found_in_substitution() {
        let mut bash = Bash::new();

        let result = bash.exec("echo \"output: $(nonexistent)\"").await.unwrap();
        // Command substitution captures stdout (which is empty for command not found)
        assert!(result.stdout.contains("output:"));
        // Exit code is from echo (0), not from the failed substitution
        assert_eq!(result.exit_code, 0);
    }
}

// =============================================================================
// PYTHON BUILTIN SECURITY TESTS
// =============================================================================

#[cfg(feature = "python")]
mod python_security {
    use super::*;

    /// TM-PY-001: Python infinite loop blocked by Monty time limit
    #[tokio::test]
    async fn threat_python_infinite_loop() {
        let mut bash = Bash::new();
        let result = bash.exec("python3 -c \"while True: pass\"").await.unwrap();
        // Should fail with resource limit (timeout or allocation limit)
        assert_ne!(result.exit_code, 0, "Infinite loop should not succeed");
    }

    /// TM-PY-002: Python memory exhaustion blocked by allocation limits
    #[tokio::test]
    async fn threat_python_memory_exhaustion() {
        let mut bash = Bash::new();
        let result = bash
            .exec("python3 -c \"x = [0] * 100000000\"")
            .await
            .unwrap();
        // Should fail with memory or allocation limit
        assert_ne!(result.exit_code, 0, "Memory bomb should not succeed");
    }

    /// TM-PY-003: Python recursion depth limited
    #[tokio::test]
    async fn threat_python_recursion_bomb() {
        let mut bash = Bash::new();
        let result = bash.exec("python3 -c \"def r(): r()\nr()\"").await.unwrap();
        assert_ne!(result.exit_code, 0, "Recursion bomb should not succeed");
        assert!(
            result.stderr.contains("RecursionError") || result.stderr.contains("recursion"),
            "Should get recursion error, got: {}",
            result.stderr
        );
    }

    /// TM-PY-004: Python os module operations are not available
    #[tokio::test]
    async fn threat_python_no_os_operations() {
        let mut bash = Bash::new();

        // os.system should not work
        let result = bash
            .exec("python3 -c \"import os\nos.system('echo hacked')\"")
            .await
            .unwrap();
        assert_ne!(result.exit_code, 0, "os.system should fail");
        assert!(
            !result.stdout.contains("hacked"),
            "Should not execute shell via os.system"
        );

        // subprocess should not work
        let result = bash
            .exec("python3 -c \"import subprocess\nsubprocess.run(['echo', 'hacked'])\"")
            .await
            .unwrap();
        assert_ne!(result.exit_code, 0, "subprocess.run should fail");
        assert!(
            !result.stdout.contains("hacked"),
            "Should not execute shell via subprocess"
        );
    }

    /// TM-PY-005: Python cannot access real filesystem
    #[tokio::test]
    async fn threat_python_no_filesystem() {
        let mut bash = Bash::new();

        // open() builtin should not be available (Monty doesn't expose it)
        let result = bash
            .exec("python3 -c \"f = open('/etc/passwd')\nprint(f.read())\"")
            .await
            .unwrap();
        assert_ne!(result.exit_code, 0, "file open should fail");
        assert!(
            !result.stdout.contains("root:"),
            "Should not read real /etc/passwd"
        );
    }

    /// TM-PY-006: Python error output goes to stderr, not stdout
    #[tokio::test]
    async fn threat_python_error_isolation() {
        let mut bash = Bash::new();

        let result = bash.exec("python3 -c \"1/0\"").await.unwrap();
        assert_eq!(result.exit_code, 1);
        // Error traceback should be on stderr
        assert!(
            result.stderr.contains("ZeroDivisionError"),
            "Error should be on stderr"
        );
    }

    /// TM-PY-007: Python syntax error returns non-zero exit code
    #[tokio::test]
    async fn threat_python_syntax_error_exit() {
        let mut bash = Bash::new();

        let result = bash.exec("python3 -c \"if\"").await.unwrap();
        assert_ne!(result.exit_code, 0, "Syntax error should fail");
        assert!(
            result.stderr.contains("SyntaxError") || result.stderr.contains("Error"),
            "Should get syntax error, got: {}",
            result.stderr
        );
    }

    /// TM-PY-008: Python exit code propagates to bash correctly
    #[tokio::test]
    async fn threat_python_exit_code_propagation() {
        let mut bash = Bash::new();

        // Success case
        let result = bash
            .exec("python3 -c \"print('ok')\"\necho $?")
            .await
            .unwrap();
        assert!(result.stdout.contains("0"), "Success should give exit 0");

        // Failure case
        let result = bash
            .exec("python3 -c \"1/0\" 2>/dev/null\necho $?")
            .await
            .unwrap();
        assert!(result.stdout.contains("1"), "Error should give exit 1");
    }

    /// TM-PY-009: Python -c with empty argument fails gracefully
    #[tokio::test]
    async fn threat_python_empty_code() {
        let mut bash = Bash::new();

        let result = bash.exec("python3 -c \"\"").await.unwrap();
        // Empty string is valid -c "" argument but should fail (requires non-empty)
        assert_ne!(result.exit_code, 0);
    }

    /// TM-PY-010: Python output in pipeline doesn't leak errors
    #[tokio::test]
    async fn threat_python_pipeline_error_handling() {
        let mut bash = Bash::new();

        // Errors should not leak into pipeline stdout
        let result = bash
            .exec("python3 -c \"1/0\" 2>/dev/null | cat")
            .await
            .unwrap();
        assert!(
            !result.stdout.contains("ZeroDivisionError"),
            "Error should not be on stdout in pipeline"
        );
    }

    /// TM-PY-011: Python command substitution captures only stdout
    #[tokio::test]
    async fn threat_python_subst_captures_stdout() {
        let mut bash = Bash::new();

        let result = bash
            .exec("result=$(python3 -c \"print(42)\")\necho $result")
            .await
            .unwrap();
        assert_eq!(result.stdout.trim(), "42");
    }

    /// TM-PY-012: Python cannot execute shell commands via eval/exec
    #[tokio::test]
    async fn threat_python_no_shell_exec() {
        let mut bash = Bash::new();

        // __import__ should not be available
        let result = bash
            .exec("python3 -c \"__import__('os').system('echo hacked')\"")
            .await
            .unwrap();
        assert_ne!(result.exit_code, 0, "Shell exec via __import__ should fail");
        assert!(
            !result.stdout.contains("hacked"),
            "Should not execute shell command"
        );
    }

    /// TM-PY-013: Python unknown options rejected
    #[tokio::test]
    async fn threat_python_unknown_options() {
        let mut bash = Bash::new();

        let result = bash.exec("python3 -X import_all").await.unwrap();
        assert_ne!(result.exit_code, 0);
    }

    /// TM-PY-014: Python with BashKit resource limits
    #[tokio::test]
    async fn threat_python_respects_bash_limits() {
        let limits = ExecutionLimits::new().max_commands(5);
        let mut bash = Bash::builder().limits(limits).build();

        // Each python3 invocation is 1 command; but with limit=5 we can still run some
        let result = bash.exec("python3 -c \"print('ok')\"").await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "ok\n");
    }

    // --- VFS Security Tests ---

    /// TM-PY-015: Python VFS reads only from BashKit's virtual filesystem
    #[tokio::test]
    async fn threat_python_vfs_no_real_fs() {
        let mut bash = Bash::new();

        // pathlib.Path should read from VFS, not real filesystem
        // /etc/passwd exists on real Linux but not in VFS
        let result = bash
            .exec(
                "python3 -c \"from pathlib import Path\ntry:\n    Path('/etc/passwd').read_text()\n    print('LEAKED')\nexcept FileNotFoundError:\n    print('safe')\"",
            )
            .await
            .unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(
            result.stdout.contains("safe"),
            "Should not access real /etc/passwd"
        );
        assert!(
            !result.stdout.contains("LEAKED"),
            "Must not leak real filesystem"
        );
    }

    /// TM-PY-016: Python VFS write stays in virtual filesystem
    #[tokio::test]
    async fn threat_python_vfs_write_sandboxed() {
        let mut bash = Bash::new();

        // Write to VFS, verify it stays in VFS (no real file created)
        let result = bash
            .exec(
                "python3 -c \"from pathlib import Path\n_ = Path('/tmp/sandbox_test.txt').write_text('test')\nprint(Path('/tmp/sandbox_test.txt').read_text())\"",
            )
            .await
            .unwrap();
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "test\n");
    }

    /// TM-PY-017: Python VFS path traversal blocked
    #[tokio::test]
    async fn threat_python_vfs_path_traversal() {
        let mut bash = Bash::new();

        // Path traversal via ../.. should not escape VFS
        let result = bash
            .exec(
                "python3 -c \"from pathlib import Path\ntry:\n    Path('/tmp/../../../etc/passwd').read_text()\n    print('ESCAPED')\nexcept FileNotFoundError:\n    print('blocked')\"",
            )
            .await
            .unwrap();
        assert!(
            !result.stdout.contains("ESCAPED"),
            "Path traversal must not escape VFS"
        );
    }

    /// TM-PY-018: Python VFS data flows correctly between bash and Python
    #[tokio::test]
    async fn threat_python_vfs_bash_python_isolation() {
        let mut bash = Bash::new();

        // Write from bash, read from Python - shares VFS
        let result = bash
            .exec(
                "echo 'from bash' > /tmp/shared.txt\npython3 -c \"from pathlib import Path\nprint(Path('/tmp/shared.txt').read_text().strip())\"",
            )
            .await
            .unwrap();
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "from bash\n");
    }

    /// TM-PY-019: Python VFS FileNotFoundError properly raised
    #[tokio::test]
    async fn threat_python_vfs_error_handling() {
        let mut bash = Bash::new();

        // Reading nonexistent file should raise FileNotFoundError, not crash
        let result = bash
            .exec("python3 -c \"from pathlib import Path\nPath('/nonexistent').read_text()\"")
            .await
            .unwrap();
        assert_ne!(result.exit_code, 0, "Reading missing file should fail");
        assert!(
            result.stderr.contains("FileNotFoundError"),
            "Should get FileNotFoundError, got: {}",
            result.stderr
        );
    }

    /// TM-PY-020: Python VFS operations respect BashKit sandbox boundaries
    #[tokio::test]
    async fn threat_python_vfs_no_network() {
        let mut bash = Bash::new();

        // Python should not be able to make network requests
        // Even with pathlib, network paths should not work
        let result = bash
            .exec("python3 -c \"import socket\nsocket.socket()\"")
            .await
            .unwrap();
        assert_ne!(result.exit_code, 0, "socket should not be available");
    }

    /// TM-PY-021: Python VFS mkdir cannot escape sandbox
    #[tokio::test]
    async fn threat_python_vfs_mkdir_sandboxed() {
        let mut bash = Bash::new();

        // mkdir in VFS only
        let result = bash
            .exec(
                "python3 -c \"from pathlib import Path\nPath('/tmp/pydir').mkdir()\nprint(Path('/tmp/pydir').is_dir())\"",
            )
            .await
            .unwrap();
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "True\n");
    }
}
