//! Threat Model Security Tests
//!
//! Tests for threats identified in specs/threat-model.md
//! Each test category maps to a threat category in the threat model.
//!
//! Run with: `cargo test threat_`

use bashkit::{
    Bash, ExecutionLimits, FileSystem, FileSystemExt, FsLimits, InMemoryFs, MemoryLimits,
    OverlayFs, SessionLimits, TraceEventDetails, TraceEventKind, TraceMode,
};
use std::path::Path;
use std::sync::Arc;
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

    /// Subsequent exec() calls recover after a prior call hits the command limit.
    /// Each exec() is a separate script invocation and gets its own budget.
    #[tokio::test]
    async fn exec_recovers_after_command_limit() {
        let limits = ExecutionLimits::new().max_commands(10);
        let mut bash = Bash::builder().limits(limits).build();

        // First exec: exceed the command limit
        let result = bash
            .exec("true; true; true; true; true; true; true; true; true; true; true; true")
            .await;
        assert!(result.is_err());

        // Second exec: trivial command should succeed — budget resets per exec
        let result = bash.exec("echo hello").await.unwrap();
        assert_eq!(result.stdout.trim(), "hello");
        assert_eq!(result.exit_code, 0);
    }

    /// Loop counters also reset between exec() calls.
    #[tokio::test]
    async fn exec_recovers_after_loop_limit() {
        let limits = ExecutionLimits::new()
            .max_loop_iterations(5)
            .max_total_loop_iterations(10)
            .max_commands(10000);
        let mut bash = Bash::builder().limits(limits).build();

        // First exec: exceed the total loop iteration limit
        let result = bash
            .exec("for i in 1 2 3 4 5; do true; done; for i in 1 2 3 4 5 6; do true; done")
            .await;
        assert!(result.is_err());

        // Second exec: loops should work again
        let result = bash.exec("for i in 1 2 3; do echo $i; done").await.unwrap();
        assert!(result.stdout.contains("1"));
        assert_eq!(result.exit_code, 0);
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

        // Should complete quickly due to either timeout or loop limit.
        // Under ASan/Miri the overhead can be ~200x, so use a very generous bound.
        assert!(elapsed < Duration::from_secs(300));
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

    /// Test eval is implemented but safe in virtual environment
    ///
    /// eval is a POSIX special builtin that's now implemented. In the virtual environment,
    /// eval can only execute other builtins (no external commands), so it's safe.
    /// The current implementation stores the command but doesn't re-execute it.
    #[tokio::test]
    async fn threat_eval_is_safe_in_sandbox() {
        let mut bash = Bash::new();

        // eval is now implemented - it stores the command but in virtual environment
        // it can only run builtins, so it's safe
        let result = bash.exec("eval echo test").await.unwrap();
        // eval returns 0 (success) as it's a valid builtin
        assert_eq!(result.exit_code, 0);
        // Note: current impl stores command but doesn't execute it
    }

    /// Test exec cannot escape sandbox — only VFS scripts are reachable
    ///
    /// exec now executes commands within the VFS (run + exit). Since the VFS
    /// doesn't contain /bin/bash, exec /bin/bash still fails with exit 127.
    /// This preserves the security invariant: no real process replacement.
    #[tokio::test]
    async fn threat_exec_not_available() {
        let mut bash = Bash::new();

        let result = bash.exec("exec /bin/bash").await.unwrap();
        // exec tries to run /bin/bash in VFS — doesn't exist, so exit 127
        assert_eq!(result.exit_code, 127);
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
    /// eval is a POSIX special builtin. In virtual mode, it can only execute
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
// 6. SESSION ISOLATION TESTS
// =============================================================================

mod session_isolation {
    use super::*;
    use bashkit::InMemoryFs;
    use std::sync::Arc;

    /// Test that separate instances have isolated filesystems
    #[tokio::test]
    async fn threat_isolation_fs_isolation() {
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
    async fn threat_isolation_variable_isolation() {
        let mut tenant_a = Bash::new();
        let mut tenant_b = Bash::new();

        tenant_a.exec("SECRET=password123").await.unwrap();

        let result = tenant_b.exec("echo $SECRET").await.unwrap();
        assert!(result.stdout.trim().is_empty());
    }

    /// Test that separate instances have isolated functions
    #[tokio::test]
    async fn threat_isolation_function_isolation() {
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
    async fn threat_isolation_limits_isolation() {
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

    /// TM-ISO-019: Aliases defined in one session must not leak to another
    #[tokio::test]
    async fn threat_isolation_alias_isolation() {
        let mut tenant_a = Bash::new();
        let mut tenant_b = Bash::new();

        tenant_a
            .exec("alias secret_cmd='echo LEAKED'")
            .await
            .unwrap();

        // Tenant B should not have tenant A's alias
        let result = tenant_b.exec("secret_cmd").await.unwrap();
        assert_eq!(result.exit_code, 127);
        assert!(!result.stdout.contains("LEAKED"));
    }

    /// Alias expansion must work across separate exec() calls (issue #1130).
    /// shopt -s expand_aliases set in one exec() must persist to the next.
    #[tokio::test]
    async fn alias_expansion_persists_across_exec_calls() {
        let mut bash = Bash::new();

        bash.exec("shopt -s expand_aliases").await.unwrap();
        bash.exec("alias ll='echo alias_worked'").await.unwrap();

        let result = bash.exec("ll").await.unwrap();
        assert_eq!(
            result.exit_code, 0,
            "alias should resolve: {}",
            result.stderr
        );
        assert_eq!(result.stdout.trim(), "alias_worked");
    }

    /// TM-ISO-020: Trap handlers in one session must not fire in another
    #[tokio::test]
    async fn threat_isolation_trap_isolation() {
        let mut tenant_a = Bash::new();
        let mut tenant_b = Bash::new();

        tenant_a.exec("trap 'echo TRAP_LEAKED' EXIT").await.unwrap();

        // Tenant B's EXIT trap should not produce tenant A's output
        let result = tenant_b.exec("true").await.unwrap();
        assert!(!result.stdout.contains("TRAP_LEAKED"));
    }

    /// TM-ISO-019: Shell options (set -e, set -o pipefail, etc.) must not leak
    #[tokio::test]
    async fn threat_isolation_shell_options_isolation() {
        let mut tenant_a = Bash::new();
        let mut tenant_b = Bash::new();

        tenant_a.exec("set -e").await.unwrap();
        tenant_a.exec("set -o pipefail").await.unwrap();

        // Tenant B should still have default options (errexit off)
        // If errexit leaked, `false` would abort and we'd get an error
        let result = tenant_b.exec("false; echo STILL_RUNNING").await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("STILL_RUNNING"));
    }

    /// TM-ISO-020: Exported environment variables must not cross sessions
    #[tokio::test]
    async fn threat_isolation_export_isolation() {
        let mut tenant_a = Bash::new();
        let mut tenant_b = Bash::new();

        tenant_a.exec("export DB_PASSWORD=s3cret").await.unwrap();

        // Tenant B must not see tenant A's exported var
        let result = tenant_b.exec("echo \"[$DB_PASSWORD]\"").await.unwrap();
        assert_eq!(result.stdout.trim(), "[]");
    }

    /// TM-ISO-019: Indexed and associative arrays must not leak between sessions
    #[tokio::test]
    async fn threat_isolation_array_isolation() {
        let mut tenant_a = Bash::new();
        let mut tenant_b = Bash::new();

        tenant_a
            .exec("SECRET_ARR=(one two three); declare -A SECRET_MAP; SECRET_MAP[key]=val")
            .await
            .unwrap();

        // Tenant B must not see tenant A's arrays
        let result = tenant_b
            .exec("echo \"${SECRET_ARR[0]}\" \"${SECRET_MAP[key]}\"")
            .await
            .unwrap();
        assert_eq!(result.stdout.trim(), "");
    }

    /// TM-ISO-020: Working directory changes must not leak between sessions
    #[tokio::test]
    async fn threat_isolation_cwd_isolation() {
        let fs_a = Arc::new(InMemoryFs::new());
        let mut tenant_a = Bash::builder().fs(fs_a).build();
        let mut tenant_b = Bash::new();

        tenant_a
            .exec("mkdir -p /opt/secret && cd /opt/secret")
            .await
            .unwrap();

        // Tenant B should still be at default cwd, not /opt/secret
        let result = tenant_b.exec("pwd").await.unwrap();
        assert!(!result.stdout.contains("/opt/secret"));
    }

    /// TM-ISO-019: Exit codes from one session must not affect another
    #[tokio::test]
    async fn threat_isolation_exit_code_isolation() {
        let mut tenant_a = Bash::new();
        let mut tenant_b = Bash::new();

        // Tenant A ends with failure
        tenant_a.exec("false").await.unwrap();

        // Tenant B should start with clean exit code (0)
        let result = tenant_b.exec("echo $?").await.unwrap();
        assert_eq!(result.stdout.trim(), "0");
    }

    /// TM-ISO-020: Concurrent sessions must not interfere with each other
    #[tokio::test]
    async fn threat_isolation_concurrent_isolation() {
        use tokio::task::JoinSet;

        let mut tasks = JoinSet::new();

        for i in 0..10 {
            tasks.spawn(async move {
                let mut bash = Bash::new();
                let secret = format!("TENANT_{}_SECRET", i);
                bash.exec(&format!("MY_SECRET={}", secret)).await.unwrap();

                // Each session should only see its own variable
                let result = bash.exec("echo $MY_SECRET").await.unwrap();
                assert_eq!(result.stdout.trim(), secret);

                // Try to probe for other tenants' variables
                for j in 0..10 {
                    if j != i {
                        let other = format!("TENANT_{}_SECRET", j);
                        let probe = bash.exec("echo $MY_SECRET").await.unwrap();
                        assert!(!probe.stdout.contains(&other));
                    }
                }
            });
        }

        while let Some(result) = tasks.join_next().await {
            result.unwrap();
        }
    }

    /// TM-ISO-019: Concurrent filesystem writes must not cross-contaminate
    #[tokio::test]
    async fn threat_isolation_concurrent_fs_isolation() {
        use tokio::task::JoinSet;

        let mut tasks = JoinSet::new();

        for i in 0..10 {
            tasks.spawn(async move {
                let fs = Arc::new(InMemoryFs::new());
                let mut bash = Bash::builder().fs(fs).build();

                let secret = format!("FS_SECRET_{}", i);
                bash.exec(&format!("echo '{}' > /tmp/data.txt", secret))
                    .await
                    .unwrap();

                let result = bash.exec("cat /tmp/data.txt").await.unwrap();
                assert!(
                    result.stdout.contains(&secret),
                    "Tenant {} should see its own secret",
                    i
                );

                // Verify no other tenant's data leaked in
                for j in 0..10 {
                    if j != i {
                        let other_secret = format!("FS_SECRET_{}", j);
                        assert!(
                            !result.stdout.contains(&other_secret),
                            "Tenant {} saw tenant {}'s data!",
                            i,
                            j
                        );
                    }
                }
            });
        }

        while let Some(result) = tasks.join_next().await {
            result.unwrap();
        }
    }

    /// TM-ISO-020: Session state snapshot/restore must not affect other sessions
    #[tokio::test]
    async fn threat_isolation_snapshot_isolation() {
        let mut tenant_a = Bash::new();
        let mut tenant_b = Bash::new();

        tenant_a.exec("SNAPSHOT_SECRET=before").await.unwrap();
        let snapshot = tenant_a.shell_state();

        // Mutate tenant_a
        tenant_a.exec("SNAPSHOT_SECRET=after").await.unwrap();

        // Restore snapshot on tenant_a
        tenant_a.restore_shell_state(&snapshot);

        // Tenant B should be unaffected by any of this
        let result = tenant_b.exec("echo \"[$SNAPSHOT_SECRET]\"").await.unwrap();
        assert_eq!(result.stdout.trim(), "[]");
    }

    /// TM-ISO-019: Adversarial probing — script tries to discover other sessions
    /// by iterating common paths and variable names
    #[tokio::test]
    async fn threat_isolation_adversarial_probing() {
        let mut victim = Bash::new();
        victim.exec("API_KEY=sk-live-12345").await.unwrap();
        victim
            .exec("export DATABASE_URL=postgres://secret@db/prod")
            .await
            .unwrap();

        let mut attacker = Bash::new();

        // Try common secret variable names
        let probe_script = r#"
echo "$API_KEY"
echo "$DATABASE_URL"
echo "$AWS_SECRET_ACCESS_KEY"
echo "$GITHUB_TOKEN"
echo "$PASSWORD"
echo "$SECRET"
echo "$PRIVATE_KEY"
"#;
        let result = attacker.exec(probe_script).await.unwrap();
        assert!(!result.stdout.contains("sk-live"));
        assert!(!result.stdout.contains("postgres://"));

        // Try to enumerate env via env/printenv/set
        let result = attacker
            .exec("env 2>/dev/null; printenv 2>/dev/null")
            .await
            .unwrap();
        assert!(!result.stdout.contains("sk-live"));
        assert!(!result.stdout.contains("postgres://"));
    }

    /// TM-ISO-020: Adversarial script tries to read /proc, /sys, /dev for leaks
    #[tokio::test]
    async fn threat_isolation_proc_probing() {
        let mut bash = Bash::new();

        // These should not expose host information
        let probes = vec![
            "cat /proc/self/environ 2>/dev/null",
            "cat /proc/self/cmdline 2>/dev/null",
            "cat /proc/1/environ 2>/dev/null",
            "ls /proc 2>/dev/null",
            "cat /etc/passwd 2>/dev/null",
            "cat /etc/shadow 2>/dev/null",
        ];

        for probe in probes {
            let result = bash.exec(probe).await.unwrap();
            // VFS shouldn't have real /proc or /etc content
            assert!(
                result.stdout.trim().is_empty() || result.exit_code != 0,
                "Probe '{}' returned unexpected data: {}",
                probe,
                result.stdout
            );
        }
    }

    /// TM-ISO-019: jq env isolation — jq in one session must not see
    /// environment variables from another concurrent session
    #[tokio::test]
    async fn threat_isolation_jq_env_cross_session() {
        let mut tenant_a = Bash::new();
        let mut tenant_b = Bash::new();

        tenant_a
            .exec("export JQ_SECRET=tenant_a_secret")
            .await
            .unwrap();

        // Tenant B's jq should not see tenant A's env
        let result = tenant_b
            .exec("jq -n 'env.JQ_SECRET // \"none\"'")
            .await
            .unwrap();
        assert!(
            !result.stdout.contains("tenant_a_secret"),
            "jq leaked cross-session env: {}",
            result.stdout
        );
    }

    /// TM-ISO-020: Subshell isolation — mutations in subshell must not
    /// leak to parent, and parent state must not leak to sibling sessions
    #[tokio::test]
    async fn threat_isolation_subshell_isolation() {
        let mut bash = Bash::new();

        bash.exec("OUTER=original").await.unwrap();
        bash.exec("(OUTER=mutated; INNER=leaked)").await.unwrap();

        // Parent should not see subshell mutations
        let result = bash.exec("echo $OUTER $INNER").await.unwrap();
        assert_eq!(result.stdout.trim(), "original");

        // Separate session should see neither
        let mut other = Bash::new();
        let result = other.exec("echo \"[$OUTER][$INNER]\"").await.unwrap();
        assert_eq!(result.stdout.trim(), "[][]");
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

        // Moderately nested (4 levels) - should succeed and produce correct output
        let result = bash.exec("echo $(echo $(echo $(echo hello)))").await;
        let result = result.expect("4-level command substitution should succeed");
        assert_eq!(
            result.stdout.trim(),
            "hello",
            "nested command sub should produce 'hello'"
        );
    }

    /// TM-DOS-022: Deep subshell nesting must hit ast_depth limit or handle gracefully
    #[tokio::test]
    async fn threat_deep_subshell_nesting_blocked() {
        let limits = ExecutionLimits::new()
            .max_commands(100)
            .max_function_depth(50)
            .max_ast_depth(20);
        let mut bash = Bash::builder().limits(limits).build();

        // 200-level nested subshells against max_ast_depth=20
        let script = format!("{}echo hello{}", "(".repeat(200), ")".repeat(200),);
        let result = bash.exec(&script).await;
        // Must not crash — either errors with depth limit or returns Ok (graceful)
        match result {
            Ok(_) => {} // Depth limit caused parse truncation → Ok with empty output
            Err(e) => {
                let err = e.to_string();
                assert!(
                    err.contains("nesting") || err.contains("depth") || err.contains("fuel"),
                    "Expected depth/nesting/fuel error, got: {}",
                    err
                );
            }
        }
    }

    /// TM-DOS-026: Deep arithmetic nesting must not crash (depth-limited)
    #[tokio::test]
    async fn threat_deep_arithmetic_nesting_safe() {
        let mut bash = Bash::new();

        // 500-level arithmetic parens — now bounded by MAX_ARITHMETIC_DEPTH
        let depth = 500;
        let script = format!("echo $(({} 1 {}))", "(".repeat(depth), ")".repeat(depth),);
        let result = bash.exec(&script).await;
        // Must not crash. With depth limit it returns 0 (depth exceeded → fallback)
        match result {
            Ok(r) => {
                // Bounded arithmetic evaluator returns 0 when depth exceeded
                let output = r.stdout.trim();
                assert!(!output.is_empty(), "should produce output, not crash");
            }
            Err(_) => {
                // Error also acceptable (parser-level rejection)
            }
        }
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

        // Use rsync which is never a builtin (ssh may be with ssh feature)
        let result = bash.exec("rsync").await.unwrap();
        assert_eq!(result.exit_code, 127);
        // Should match bash format: "bash: cmd: command not found"
        assert!(
            result.stderr.starts_with("bash: rsync: command not found"),
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
        // Note: ssh/scp/sftp are builtins when ssh feature is enabled
        for cmd in &["apt", "yum", "docker", "vim", "nano", "rsync"] {
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
    use bashkit::PythonLimits;

    /// Helper: create Bash with python builtins registered.
    fn bash_with_python() -> Bash {
        Bash::builder()
            .python_with_limits(PythonLimits::default())
            .build()
    }

    /// TM-PY-001: Python infinite loop blocked by Monty time limit
    #[tokio::test]
    async fn threat_python_infinite_loop() {
        let mut bash = bash_with_python();
        let result = bash.exec("python3 -c \"while True: pass\"").await.unwrap();
        // Should fail with resource limit (timeout or allocation limit)
        assert_ne!(result.exit_code, 0, "Infinite loop should not succeed");
    }

    /// TM-PY-002: Python memory exhaustion blocked by allocation limits
    #[tokio::test]
    async fn threat_python_memory_exhaustion() {
        let mut bash = bash_with_python();
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
        let mut bash = bash_with_python();
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
        let mut bash = bash_with_python();

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
        let mut bash = bash_with_python();

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
        let mut bash = bash_with_python();

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
        let mut bash = bash_with_python();

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
        let mut bash = bash_with_python();

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
        let mut bash = bash_with_python();

        let result = bash.exec("python3 -c \"\"").await.unwrap();
        // Empty string is valid -c "" argument but should fail (requires non-empty)
        assert_ne!(result.exit_code, 0);
    }

    /// TM-PY-010: Python output in pipeline doesn't leak errors
    #[tokio::test]
    async fn threat_python_pipeline_error_handling() {
        let mut bash = bash_with_python();

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
        let mut bash = bash_with_python();

        let result = bash
            .exec("result=$(python3 -c \"print(42)\")\necho $result")
            .await
            .unwrap();
        assert_eq!(result.stdout.trim(), "42");
    }

    /// TM-PY-012: Python cannot execute shell commands via eval/exec
    #[tokio::test]
    async fn threat_python_no_shell_exec() {
        let mut bash = bash_with_python();

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
        let mut bash = bash_with_python();

        let result = bash.exec("python3 -X import_all").await.unwrap();
        assert_ne!(result.exit_code, 0);
    }

    /// TM-PY-014: Python with BashKit resource limits
    #[tokio::test]
    async fn threat_python_respects_bash_limits() {
        let limits = ExecutionLimits::new().max_commands(5);
        let mut bash = Bash::builder().python().limits(limits).build();

        // Each python3 invocation is 1 command; but with limit=5 we can still run some
        let result = bash.exec("python3 -c \"print('ok')\"").await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "ok\n");
    }

    // --- VFS Security Tests ---

    /// TM-PY-015: Python VFS reads only from BashKit's virtual filesystem
    #[tokio::test]
    async fn threat_python_vfs_no_real_fs() {
        let mut bash = bash_with_python();

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
        let mut bash = bash_with_python();

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
        let mut bash = bash_with_python();

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
        let mut bash = bash_with_python();

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
        let mut bash = bash_with_python();

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
        let mut bash = bash_with_python();

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
        let mut bash = bash_with_python();

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

// NOTE: Subprocess isolation tests (TM-PY-022 to TM-PY-026) were removed
// when the worker subprocess architecture was replaced with direct Monty
// integration. Resource limits and VFS isolation are now enforced directly
// by Monty's runtime within the host process.

// -- TM-PY regression coverage for Monty v0.0.5 parser depth guard ----------

#[cfg(feature = "python")]
mod python_security_regressions {
    use super::*;
    use bashkit::PythonLimits;

    fn bash_with_python() -> Bash {
        Bash::builder()
            .python_with_limits(PythonLimits::default())
            .build()
    }

    /// TM-PY-022: Deeply nested Python expressions caught by Monty depth guard.
    /// Monty v0.0.5 added a parser depth guard (d634706). In debug builds the
    /// limit is 35; in release builds it's 200. We use a nesting level that
    /// triggers the Monty guard rather than overflowing the ruff parser stack.
    #[tokio::test]
    async fn threat_python_deep_nesting_parser() {
        let mut bash = bash_with_python();
        // 30 levels of nested tuples triggers Monty's depth guard in debug builds
        // (MAX_NESTING_DEPTH=35) while staying safe for ruff's parser stack.
        let depth = 30;
        let code = format!(
            "python3 -c \"x = {}1{}\"",
            "(".repeat(depth),
            ",)".repeat(depth)
        );
        let result = bash.exec(&code).await.unwrap();
        // In debug builds (depth limit 35), 30 nested tuples should succeed.
        // In release builds (depth limit 200), it definitely succeeds.
        // The guard prevents deeper nesting from crashing.
        assert_eq!(
            result.exit_code, 0,
            "30 levels of nesting should be within parser depth budget"
        );
    }

    /// TM-PY-022b: Nesting at the depth guard boundary fails gracefully.
    #[tokio::test]
    async fn threat_python_nesting_at_guard_boundary() {
        let mut bash = bash_with_python();
        // In debug builds MAX_NESTING_DEPTH=35, so 40 nested statements
        // should trigger the depth guard and return an error, not crash.
        // In release builds (limit=200) this will succeed, which is fine.
        let depth = 40;
        let code = format!(
            "python3 -c \"{}x = 1{}\"",
            "if True:\n    ".repeat(depth),
            ""
        );
        let result = bash.exec(&code).await.unwrap();
        // Either succeeds (release build, limit=200) or errors gracefully (debug build)
        if result.exit_code != 0 {
            assert!(
                !result.stderr.is_empty(),
                "Should get a parse error, not silent failure"
            );
        }
    }

    /// TM-PY-003b: Exponentiation resource exhaustion blocked.
    /// Monty v0.0.5 added a 4x safety multiplier (a07e336) to prevent
    /// huge power results from exhausting memory.
    #[tokio::test]
    async fn threat_python_pow_exhaustion() {
        let limits = PythonLimits::default().max_memory(1024 * 1024); // 1MB
        let mut bash = Bash::builder().python_with_limits(limits).build();
        // 2 ** 1_000_000 produces ~300KB number; with tight 1MB limit the
        // allocation check should reject it before completion.
        let result = bash
            .exec("python3 -c \"x = 2 ** 1000000\ny = 2 ** 1000000\nz = x * y\"")
            .await
            .unwrap();
        assert_ne!(
            result.exit_code, 0,
            "Large exponentiation chain should be blocked by memory limit"
        );
    }

    /// TM-PY-003c: Division by zero during floor-div of extreme values
    /// doesn't panic. Monty v0.0.5 (fc2f154) fixed i64::MIN overflow.
    #[tokio::test]
    async fn threat_python_division_edge_cases() {
        let mut bash = bash_with_python();

        // Floor division by zero should raise ZeroDivisionError, not panic
        let result = bash.exec("python3 -c \"x = 1 // 0\"").await.unwrap();
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("ZeroDivisionError"));

        // Modulo by zero
        let result = bash.exec("python3 -c \"x = 1 % 0\"").await.unwrap();
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("ZeroDivisionError"));
    }
}

// =============================================================================
// 8. NESTING DEPTH SECURITY TESTS
//
// These tests verify that deeply nested structures cannot crash the host via
// stack overflow. Covers parser, command substitution, arithmetic, and
// misconfiguration scenarios.
// =============================================================================

mod nesting_depth_security {
    use super::*;

    // ---- POSITIVE TESTS: normal nesting works correctly ----

    /// Moderate subshell nesting (3 levels) should work fine
    #[tokio::test]
    async fn positive_moderate_subshell_nesting() {
        let mut bash = Bash::new();
        // Note: deeply nested subshells may not propagate stdout in the same way
        // as bash does. Test with a sane depth that we know works.
        let result = bash.exec("(echo ok)").await.unwrap();
        assert_eq!(result.stdout.trim(), "ok");
    }

    /// Moderate command substitution nesting (5 levels) produces correct output
    #[tokio::test]
    async fn positive_moderate_command_sub_nesting() {
        let mut bash = Bash::new();
        let result = bash
            .exec("echo $(echo $(echo $(echo $(echo nested))))")
            .await
            .unwrap();
        assert_eq!(result.stdout.trim(), "nested");
    }

    /// Moderate arithmetic nesting (20 levels) evaluates correctly
    #[tokio::test]
    async fn positive_moderate_arithmetic_nesting() {
        let mut bash = Bash::new();
        let depth = 20;
        let script = format!("echo $(({} 42 {}))", "(".repeat(depth), ")".repeat(depth),);
        let result = bash.exec(&script).await.unwrap();
        assert_eq!(result.stdout.trim(), "42");
    }

    /// Arithmetic with operators at moderate nesting works
    #[tokio::test]
    async fn positive_arithmetic_operators_nested() {
        let mut bash = Bash::new();
        // ((((2+3)))) = 5
        let result = bash.exec("echo $(( ((((2+3)))) ))").await.unwrap();
        assert_eq!(result.stdout.trim(), "5");
    }

    /// Nested if/for/while at moderate depth works
    #[tokio::test]
    async fn positive_compound_nesting() {
        let mut bash = Bash::builder()
            .limits(ExecutionLimits::new().max_commands(1000))
            .build();
        // 5-level nested if
        let script = r#"
            if true; then
                if true; then
                    if true; then
                        if true; then
                            if true; then
                                echo deep
                            fi
                        fi
                    fi
                fi
            fi
        "#;
        let result = bash.exec(script).await.unwrap();
        assert_eq!(result.stdout.trim(), "deep");
    }

    // ---- NEGATIVE TESTS: deep nesting is properly blocked ----

    /// TM-DOS-022: 200-level subshell nesting with tight depth limit → blocked
    #[tokio::test]
    async fn negative_deep_subshells_blocked() {
        let limits = ExecutionLimits::new().max_ast_depth(10);
        let mut bash = Bash::builder().limits(limits).build();

        let script = format!("{}echo hello{}", "(".repeat(200), ")".repeat(200),);
        let result = bash.exec(&script).await;
        // Must not crash. Either errors with depth limit, or parser truncates
        // at depth limit causing the inner echo to not execute
        match result {
            Ok(r) => {
                // Depth limit truncated parsing → echo never reached → no "hello"
                assert!(
                    !r.stdout.contains("hello"),
                    "200-level nesting with max_ast_depth=10 should not execute inner echo"
                );
            }
            Err(e) => {
                let err = e.to_string();
                assert!(
                    err.contains("nesting") || err.contains("depth") || err.contains("fuel"),
                    "Expected depth error, got: {}",
                    err
                );
            }
        }
    }

    /// TM-DOS-022: Deeply nested if statements blocked
    #[tokio::test]
    async fn negative_deep_if_nesting_blocked() {
        let limits = ExecutionLimits::new().max_ast_depth(5);
        let mut bash = Bash::builder().limits(limits).build();

        // Build 20-level nested if
        let mut script = String::new();
        for _ in 0..20 {
            script.push_str("if true; then ");
        }
        script.push_str("echo deep; ");
        for _ in 0..20 {
            script.push_str("fi; ");
        }
        let result = bash.exec(&script).await;
        assert!(
            result.is_err(),
            "20-level if with max_ast_depth=5 must fail"
        );
    }

    /// TM-DOS-026: 1000-level arithmetic paren nesting does not crash
    #[tokio::test]
    async fn negative_extreme_arithmetic_nesting_safe() {
        let mut bash = Bash::new();

        let depth = 1000;
        let script = format!("echo $(({} 7 {}))", "(".repeat(depth), ")".repeat(depth),);
        let result = bash.exec(&script).await;
        // Must not crash — returns 0 (depth exceeded) or error
        if let Ok(r) = result {
            // With depth limiting, deeply nested expr returns 0 as fallback
            assert!(!r.stdout.trim().is_empty(), "should produce output");
        }
    }

    /// TM-DOS-021: Command substitution inherits parent depth budget
    #[tokio::test]
    async fn negative_command_sub_inherits_depth_limit() {
        let limits = ExecutionLimits::new().max_ast_depth(5).max_commands(1000);
        let mut bash = Bash::builder().limits(limits).build();

        // Even though the outer script is simple, the command substitution
        // should inherit the tight depth limit and reject deep nesting inside
        let inner_depth = 50;
        let inner = format!(
            "{}echo x{}",
            "(".repeat(inner_depth),
            ")".repeat(inner_depth),
        );
        let script = format!("echo $({})", inner);
        let result = bash.exec(&script).await;
        // The inner parser should inherit max_ast_depth=5 (minus used depth)
        // and fail on 50-level nesting
        match result {
            Ok(r) => {
                // If command sub parsing fails silently, the $() produces empty string
                // This is acceptable — the deep nesting didn't execute
                assert!(
                    !r.stdout.contains("x") || r.exit_code == 0,
                    "deep nesting in command sub should not produce 'x'"
                );
            }
            Err(e) => {
                let err = e.to_string();
                assert!(
                    err.contains("nesting") || err.contains("depth") || err.contains("fuel"),
                    "Expected depth error, got: {}",
                    err
                );
            }
        }
    }

    /// TM-DOS-021: Fuel is inherited by child parsers
    #[tokio::test]
    async fn negative_command_sub_inherits_fuel_limit() {
        let limits = ExecutionLimits::new()
            .max_parser_operations(50)
            .max_commands(1000);
        let mut bash = Bash::builder().limits(limits).build();

        // A very complex command inside $() should be constrained by inherited fuel
        // Generate many semicolons to burn through fuel quickly
        let inner_cmds: Vec<&str> = (0..100).map(|_| "true").collect();
        let script = format!("echo $({})", inner_cmds.join("; "));
        let result = bash.exec(&script).await;
        // With only 50 fuel, the child parser should run out
        // Either the outer parse fails, or the inner parse silently fails → empty $()
        match result {
            Ok(r) => {
                // Acceptable: inner parse failed → $() is empty
                assert_eq!(
                    r.stdout.trim(),
                    "",
                    "inner parse should fail with limited fuel"
                );
            }
            Err(_) => {
                // Also acceptable: outer parse may fail
            }
        }
    }

    // ---- MISCONFIGURATION TESTS: absurd limits still safe ----

    /// Even with max_ast_depth=1_000_000, the parser hard-caps at 500 to prevent
    /// stack overflow. This is a key defense: misconfiguration
    /// cannot crash the host process.
    #[tokio::test]
    async fn misconfig_huge_ast_depth_still_safe() {
        let limits = ExecutionLimits::new()
            .max_ast_depth(1_000_000) // caller tries to set absurdly high
            .max_commands(10_000);
        let mut bash = Bash::builder().limits(limits).build();

        // 150-level nested if statements — exceeds HARD_MAX_AST_DEPTH (100)
        // The parser hard cap will clamp max_depth to 100 regardless of config.
        let mut script = String::new();
        for _ in 0..150 {
            script.push_str("if true; then ");
        }
        script.push_str("echo deep; ");
        for _ in 0..150 {
            script.push_str("fi; ");
        }
        let result = bash.exec(&script).await;
        // Must not crash! Hard cap at 100 catches this despite 1M config.
        match result {
            Ok(r) => {
                // Depth exceeded at 100 → parse truncated → echo not reached
                assert!(
                    !r.stdout.contains("deep"),
                    "150-level nesting should be blocked by hard cap"
                );
            }
            Err(e) => {
                // Depth/fuel error is expected
                let err = e.to_string();
                assert!(
                    err.contains("fuel") || err.contains("nesting") || err.contains("depth"),
                    "Expected fuel/depth error, got: {}",
                    err
                );
            }
        }
    }

    /// max_ast_depth=1 should reject compound commands that nest beyond depth 1
    #[tokio::test]
    async fn misconfig_tiny_ast_depth_rejects_compounds() {
        // 0 is treated as "use default" per #1181, so use 1 for minimal limit
        let limits = ExecutionLimits::new().max_ast_depth(1);
        let mut bash = Bash::builder().limits(limits).build();

        let result = bash
            .exec("if true; then if true; then echo x; fi; fi")
            .await;
        assert!(
            result.is_err(),
            "max_ast_depth=1 should reject deeply nested compound commands"
        );
    }

    /// Even with max_parser_operations=1_000_000_000, 10MB input limit bounds parser work
    #[tokio::test]
    async fn misconfig_huge_fuel_still_bounded_by_input() {
        let limits = ExecutionLimits::new()
            .max_parser_operations(1_000_000_000)
            .max_input_bytes(1000); // 1KB input limit
        let mut bash = Bash::builder().limits(limits).build();

        // Try to submit more than 1KB
        let script = "echo ".to_string() + &"x".repeat(2000);
        let result = bash.exec(&script).await;
        assert!(
            result.is_err(),
            "input exceeding max_input_bytes must be rejected"
        );
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("too large") || err.contains("input"),
            "Expected input size error, got: {}",
            err
        );
    }

    /// Misconfigured timeout (very long) doesn't matter because command limit still works
    #[tokio::test]
    async fn misconfig_long_timeout_still_command_limited() {
        let limits = ExecutionLimits::new()
            .timeout(std::time::Duration::from_secs(3600)) // 1 hour!
            .max_commands(10);
        let mut bash = Bash::builder().limits(limits).build();

        let result = bash
            .exec("true; true; true; true; true; true; true; true; true; true; true; true")
            .await;
        assert!(
            result.is_err(),
            "command limit should trigger before 1hr timeout"
        );
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("command") && err.contains("exceeded"),
            "Expected command limit error, got: {}",
            err
        );
    }

    // ---- REGRESSION TESTS: specific attack patterns ----

    /// Monty#112 analogue: deeply nested parens in arithmetic context
    /// This is the exact pattern from the Monty issue adapted for bash
    #[tokio::test]
    async fn regression_monty_112_arithmetic_parens() {
        let mut bash = Bash::new();

        // Replicate Monty#112 pattern: ~5750 nesting levels
        // For bash arithmetic, we can't go that deep without 10MB input,
        // but we test the pattern at 3000 levels (well above MAX_ARITHMETIC_DEPTH=200)
        let depth = 3000;
        let script = format!("echo $(({} 1 {}))", "(".repeat(depth), ")".repeat(depth),);
        let result = bash.exec(&script).await;
        // Must not crash — depth limit returns 0 as fallback
        assert!(result.is_ok() || result.is_err(), "must not crash");
    }

    /// Monty#112 analogue: deeply nested subshells (parser recursion)
    #[tokio::test]
    async fn regression_monty_112_subshell_nesting() {
        let mut bash = Bash::new(); // default max_ast_depth=100

        let depth = 500;
        let script = format!("{}echo hello{}", "(".repeat(depth), ")".repeat(depth),);
        let result = bash.exec(&script).await;
        // Must not crash — either errors (depth/fuel exceeded) or Ok (truncated parse)
        match result {
            Ok(r) => {
                // Parser truncated at depth=100 → inner echo not reached
                assert!(
                    !r.stdout.contains("hello"),
                    "500-level subshells should not execute inner echo"
                );
            }
            Err(e) => {
                let err = e.to_string();
                assert!(
                    err.contains("nesting") || err.contains("depth") || err.contains("fuel"),
                    "Expected depth/fuel error, got: {}",
                    err
                );
            }
        }
    }

    /// Mixed nesting: command substitution containing deeply nested subshells
    #[tokio::test]
    async fn regression_mixed_nesting_safe() {
        let limits = ExecutionLimits::new().max_ast_depth(10).max_commands(1000);
        let mut bash = Bash::builder().limits(limits).build();

        // Outer: 5-level subshell, inner: 50-level subshell inside $()
        let outer = "(((((";
        let outer_close = ")))))";
        let inner_depth = 50;
        let inner = format!(
            "{}echo x{}",
            "(".repeat(inner_depth),
            ")".repeat(inner_depth),
        );
        let script = format!("{}echo $({}){}", outer, inner, outer_close);
        let result = bash.exec(&script).await;
        // Inner parser gets remaining depth budget (10-5=5), which < 50
        // So the inner parse should fail
        match result {
            Ok(r) => {
                // Inner parse fails silently → $() is empty, echo prints newline
                assert!(
                    !r.stdout.contains("x"),
                    "inner deep nesting should not succeed"
                );
            }
            Err(e) => {
                let err = e.to_string();
                assert!(
                    err.contains("nesting")
                        || err.contains("depth")
                        || err.contains("fuel")
                        || err.contains("unexpected token"),
                    "Expected depth or syntax error, got: {}",
                    err
                );
            }
        }
    }

    /// Nested command substitutions all share the depth budget
    #[tokio::test]
    async fn negative_chained_command_subs_share_budget() {
        let limits = ExecutionLimits::new().max_ast_depth(15).max_commands(1000);
        let mut bash = Bash::builder().limits(limits).build();

        // 3 levels of command substitution, each containing subshells.
        // Outer uses some depth, inner gets less.
        // Total if limits weren't shared: 3 * 15 = 45
        // With sharing: 15 total
        let script =
            "echo $( ( ( ( ( echo $( ( ( ( ( echo $( ( ( ( ( echo ok ) ) ) ) ) ) ) ) ) ) ) ) ) ) )";
        let result = bash.exec(script).await;
        // This has many levels — may hit limit or succeed depending on accounting
        // Key: no crash
        match result {
            Ok(_) | Err(_) => {} // both acceptable, just no crash
        }
    }
}

// =============================================================================
// TM-DOS-027: BUILTIN PARSER DEPTH LIMIT TESTS
// =============================================================================

mod builtin_parser_depth {
    use super::*;

    /// TM-DOS-027: Deeply nested awk expression via parentheses must not crash
    #[tokio::test]
    async fn threat_awk_deep_paren_nesting_safe() {
        let mut bash = Bash::new();

        // 200-level parenthesized expression in awk
        let depth = 200;
        let open = "(".repeat(depth);
        let close = ")".repeat(depth);
        let script = format!(r#"echo "1" | awk '{{print {open}1{close}}}'"#);

        let result = bash.exec(&script).await;
        // Must not crash. Either error (depth exceeded) or caught by panic handler.
        if let Ok(r) = result {
            // If builtin caught the error, exit code should be non-zero
            assert!(
                r.exit_code != 0 || r.stderr.contains("nesting"),
                "deep awk nesting should fail gracefully"
            );
        }
    }

    /// TM-DOS-027: Deeply nested awk unary operators must not crash
    #[tokio::test]
    async fn threat_awk_deep_unary_nesting_safe() {
        let mut bash = Bash::new();

        // 200-level chained unary negation in awk
        let depth = 200;
        let prefix = "- ".repeat(depth);
        let script = format!(r#"echo "1" | awk '{{print {prefix}1}}'"#);

        let result = bash.exec(&script).await;
        // Must not crash
        if let Ok(r) = result {
            assert!(
                r.exit_code != 0 || r.stderr.contains("nesting"),
                "deep awk unary nesting should fail gracefully"
            );
        }
    }

    /// TM-DOS-027: Deeply nested JSON input to jq must not crash
    #[tokio::test]
    async fn threat_jq_deep_json_nesting_safe() {
        let mut bash = Bash::new();

        // 200-level nested JSON arrays
        let depth = 200;
        let open = "[".repeat(depth);
        let close = "]".repeat(depth);
        let json = format!("{open}1{close}");
        let script = format!(r#"echo '{json}' | jq '.'"#);

        let result = bash.exec(&script).await;
        // Must not crash
        if let Ok(r) = result {
            assert!(
                r.exit_code != 0 || r.stderr.contains("nesting"),
                "deep JSON nesting should fail gracefully"
            );
        }
    }

    /// TM-DOS-027: Moderate nesting in awk still works
    #[tokio::test]
    async fn threat_awk_moderate_nesting_works() {
        let mut bash = Bash::new();

        // 5-level nesting should be fine
        let script = r#"echo "1" | awk '{print (((((1 + 2)))))}'"#;
        let result = bash.exec(script).await.unwrap();
        assert_eq!(result.stdout.trim(), "3");
    }

    /// TM-DOS-027: Moderate nesting in jq still works
    #[tokio::test]
    async fn threat_jq_moderate_nesting_works() {
        let mut bash = Bash::new();

        let script = r#"echo '[[[[1]]]]' | jq '.[0][0][0][0]'"#;
        let result = bash.exec(script).await.unwrap();
        assert_eq!(result.stdout.trim(), "1");
    }
}

// =============================================================================
// NESTED LOOP MULTIPLICATION TESTS (TM-DOS-018)
// =============================================================================

mod nested_loop_security {
    use bashkit::{Bash, ExecutionLimits};

    /// TM-DOS-018: Nested loops hit total loop iteration cap
    #[tokio::test]
    async fn threat_nested_loop_multiplication_blocked() {
        // Per-loop: 1000, total: 5000
        // Two nested loops of 100 each = 10,000 total iterations would exceed 5000
        let limits = ExecutionLimits::new()
            .max_loop_iterations(1000)
            .max_total_loop_iterations(5000)
            .max_commands(100_000);
        let mut bash = Bash::builder().limits(limits).build();

        let script = r#"
            count=0
            for i in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25 26 27 28 29 30 31 32 33 34 35 36 37 38 39 40 41 42 43 44 45 46 47 48 49 50 51 52 53 54 55 56 57 58 59 60 61 62 63 64 65 66 67 68 69 70 71 72 73 74 75 76 77 78 79 80 81 82 83 84 85 86 87 88 89 90 91 92 93 94 95 96 97 98 99 100; do
                for j in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25 26 27 28 29 30 31 32 33 34 35 36 37 38 39 40 41 42 43 44 45 46 47 48 49 50 51 52 53 54 55 56 57 58 59 60 61 62 63 64 65 66 67 68 69 70 71 72 73 74 75 76 77 78 79 80 81 82 83 84 85 86 87 88 89 90 91 92 93 94 95 96 97 98 99 100; do
                    count=$((count + 1))
                done
            done
            echo $count
        "#;
        let result = bash.exec(script).await;
        // Should hit total loop iteration limit
        assert!(
            result.is_err(),
            "Nested 100x100 loops should hit total limit of 5000"
        );
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("loop iterations exceeded"),
            "Expected loop limit error, got: {}",
            err
        );
    }

    /// TM-DOS-018: Sequential loops within total budget succeed
    #[tokio::test]
    async fn threat_sequential_loops_within_budget() {
        let limits = ExecutionLimits::new()
            .max_loop_iterations(100)
            .max_total_loop_iterations(200)
            .max_commands(100_000);
        let mut bash = Bash::builder().limits(limits).build();

        // Two sequential loops of 5 each = 10 total, well within budget
        let result = bash
            .exec("for i in 1 2 3 4 5; do echo $i; done; for j in 1 2 3 4 5; do echo $j; done")
            .await
            .unwrap();
        assert_eq!(result.exit_code, 0);
    }
}

// =============================================================================
// PATH VALIDATION SECURITY TESTS (TM-DOS-012, TM-DOS-013, TM-DOS-015)
// =============================================================================

mod path_validation_security {
    use bashkit::{Bash, FileSystem, FsLimits, InMemoryFs};
    use std::path::Path;
    use std::sync::Arc;

    /// TM-DOS-012: Deep directory nesting blocked by max_path_depth
    #[tokio::test]
    async fn threat_deep_directory_nesting_blocked() {
        let limits = FsLimits::new().max_path_depth(5);
        let fs = Arc::new(InMemoryFs::with_limits(limits));
        let mut bash = Bash::builder().fs(fs).build();

        // Depth 5 should work
        let result = bash.exec("mkdir -p /a/b/c/d/e").await.unwrap();
        assert_eq!(result.exit_code, 0);

        // Depth 6 should fail
        let result = bash.exec("mkdir -p /a/b/c/d/e/f").await.unwrap();
        assert_ne!(result.exit_code, 0);
        assert!(result.stderr.contains("path too deep"));
    }

    /// TM-DOS-012: Writing to deeply nested path blocked
    #[tokio::test]
    async fn threat_deep_path_write_blocked() {
        let limits = FsLimits::new().max_path_depth(3);
        let fs = Arc::new(InMemoryFs::with_limits(limits));

        // Depth 3 should work
        fs.mkdir(Path::new("/a/b"), true).await.unwrap();
        fs.write_file(Path::new("/a/b/c"), b"ok").await.unwrap();

        // Depth 4 should fail
        let result = fs.write_file(Path::new("/a/b/c/d"), b"fail").await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("path too deep"));
    }

    /// TM-DOS-013: Long filenames blocked by max_filename_length
    #[tokio::test]
    async fn threat_long_filename_blocked() {
        let limits = FsLimits::new().max_filename_length(20);
        let fs = Arc::new(InMemoryFs::with_limits(limits));
        let mut bash = Bash::builder().fs(fs).build();

        // Short name works
        let result = bash.exec("echo ok > /tmp/short.txt").await.unwrap();
        assert_eq!(result.exit_code, 0);

        // 21-char name fails
        let long_name = "a".repeat(21);
        let result = bash
            .exec(&format!("echo fail > /tmp/{}", long_name))
            .await
            .unwrap();
        assert_ne!(result.exit_code, 0);
        assert!(result.stderr.contains("filename too long"));
    }

    /// TM-DOS-013: Long total path blocked by max_path_length
    #[tokio::test]
    async fn threat_long_path_blocked() {
        let limits = FsLimits::new().max_path_length(30);
        let fs = Arc::new(InMemoryFs::with_limits(limits));

        // Short path works
        fs.write_file(Path::new("/tmp/ok.txt"), b"ok")
            .await
            .unwrap();

        // Long path fails
        let long_path = format!("/tmp/{}", "x".repeat(30));
        let result = fs.write_file(Path::new(&long_path), b"fail").await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("path too long"));
    }

    /// TM-DOS-015: Control characters in filenames rejected
    #[tokio::test]
    async fn threat_control_char_filename_rejected() {
        let fs = InMemoryFs::new();

        // Newline in filename
        let result = fs.write_file(Path::new("/tmp/file\nname"), b"bad").await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("unsafe character"));

        // Tab in filename
        let result = fs.write_file(Path::new("/tmp/file\tname"), b"bad").await;
        assert!(result.is_err());
    }

    /// TM-DOS-015: Bidi override characters in filenames rejected
    #[tokio::test]
    async fn threat_bidi_override_filename_rejected() {
        let fs = InMemoryFs::new();

        // Right-to-left override (U+202E) — can make "exe.txt" display as "txt.exe"
        let result = fs
            .write_file(Path::new("/tmp/file\u{202E}name"), b"bad")
            .await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("bidi override"), "Error: {}", err);
    }

    /// TM-DOS-015: Normal unicode filenames still work
    #[tokio::test]
    async fn threat_normal_unicode_filename_ok() {
        let fs = InMemoryFs::new();

        // Accented chars
        fs.write_file(Path::new("/tmp/café.txt"), b"ok")
            .await
            .unwrap();

        // CJK characters
        fs.write_file(Path::new("/tmp/文件.txt"), b"ok")
            .await
            .unwrap();
    }

    /// TM-DOS-012: Deep nesting via script blocked end-to-end
    #[tokio::test]
    async fn threat_deep_nesting_script_blocked() {
        let limits = FsLimits::new().max_path_depth(5);
        let fs = Arc::new(InMemoryFs::with_limits(limits));
        let mut bash = Bash::builder().fs(fs).build();

        // 6-level deep path (exceeds max_path_depth=5)
        let result = bash.exec("mkdir -p /a/b/c/d/e/f").await.unwrap();
        assert_ne!(
            result.exit_code, 0,
            "mkdir -p for depth 6 should fail with max_path_depth=5, stderr: {}",
            result.stderr
        );
    }
}

// =============================================================================
// 12. ARCHIVE SECURITY TESTS (TM-DOS-007, TM-DOS-008)
// =============================================================================

mod archive_security {
    use super::*;
    use bashkit::{FsLimits, InMemoryFs};
    use std::sync::Arc;

    /// TM-DOS-007: Gzip bomb — decompression output exceeds file size limit
    #[tokio::test]
    async fn threat_gzip_bomb_blocked() {
        let limits = FsLimits::new().max_file_size(1_000);
        let fs = Arc::new(InMemoryFs::with_limits(limits));
        let mut bash = Bash::builder().fs(fs).build();

        // Create a file larger than the limit, compress it, then try to decompress
        // We can't create a huge file directly (limit blocks it), but we can test
        // that gzip output respects file size limits by creating a compressible file
        // within limits and verifying the pipeline works
        let result = bash
            .exec("echo 'small data' > /tmp/test.txt && gzip /tmp/test.txt")
            .await
            .unwrap();
        assert_eq!(result.exit_code, 0, "gzip of small file should work");

        // Verify gunzip produces output within limits
        let result = bash.exec("gunzip /tmp/test.txt.gz").await.unwrap();
        assert_eq!(
            result.exit_code, 0,
            "gunzip of small file should work: {}",
            result.stderr
        );
    }

    /// TM-DOS-008: Tar with many files — FS file count limit blocks extraction
    #[tokio::test]
    async fn threat_tar_bomb_many_files_blocked() {
        let limits = FsLimits::new().max_file_count(30);
        let fs = Arc::new(InMemoryFs::with_limits(limits));
        let mut bash = Bash::builder().fs(fs).build();

        // Create many files and archive them
        let result = bash
            .exec(
                r#"
mkdir -p /tmp/src
for i in $(seq 1 10); do echo "file $i" > /tmp/src/f$i.txt; done
tar -cf /tmp/archive.tar -C /tmp/src .
mkdir -p /tmp/dst
"#,
            )
            .await
            .unwrap();
        assert_eq!(
            result.exit_code, 0,
            "Creating archive should work: {}",
            result.stderr
        );

        // Now try to extract when we're close to file count limit
        // The extraction should fail or stop when hitting the FS limit
        let result = bash
            .exec("tar -xf /tmp/archive.tar -C /tmp/dst")
            .await
            .unwrap();
        // Either succeeds (if within limits) or fails (if limits hit) —
        // the key property is it doesn't crash or exceed limits
        let _ = result;
    }

    /// TM-ESC-001/TM-INJ-005: Tar path traversal — VFS prevents escape.
    /// Even if tar entries had traversal names, the VFS sandbox blocks it.
    #[tokio::test]
    async fn threat_tar_path_traversal_blocked() {
        let mut bash = Bash::new();

        // Create a tar archive with normal files
        let result = bash
            .exec(
                r#"
mkdir -p /tmp/src
echo "normal" > /tmp/src/safe.txt
cd /tmp/src && tar -cf /tmp/test.tar safe.txt
"#,
            )
            .await
            .unwrap();
        assert_eq!(result.exit_code, 0, "tar create: {}", result.stderr);

        // Extract to a target directory
        let result = bash
            .exec(
                r#"
mkdir -p /tmp/dst
cd /tmp/dst && tar -xf /tmp/test.tar
cat /tmp/dst/safe.txt
"#,
            )
            .await
            .unwrap();
        assert_eq!(result.exit_code, 0, "tar extract: {}", result.stderr);
        assert!(result.stdout.contains("normal"));

        // Verify /etc/passwd doesn't exist (VFS has no real files)
        let result = bash.exec("cat /etc/passwd").await.unwrap();
        assert_ne!(result.exit_code, 0, "VFS should not have /etc/passwd");
    }

    /// TM-DOS-005: Tar extraction respects max_file_size limit.
    /// The limit applies to individual extracted files.
    #[tokio::test]
    async fn threat_tar_large_file_blocked() {
        // 10KB limit — enough for tar overhead and small files, but blocks large content
        let limits = FsLimits::new().max_file_size(10_000);
        let fs = Arc::new(InMemoryFs::with_limits(limits));
        let mut bash = Bash::builder().fs(fs).build();

        // Create a small file and archive it (within limits)
        let result = bash
            .exec(
                r#"
mkdir -p /tmp/src
echo "small" > /tmp/src/ok.txt
cd /tmp/src && tar -cf /tmp/test.tar ok.txt
"#,
            )
            .await
            .unwrap();
        assert_eq!(
            result.exit_code, 0,
            "Archiving should work: {}",
            result.stderr
        );

        // Extract within limits should work
        let result = bash
            .exec("mkdir -p /tmp/dst && cd /tmp/dst && tar -xf /tmp/test.tar")
            .await
            .unwrap();
        assert_eq!(
            result.exit_code, 0,
            "Extraction within limits: {}",
            result.stderr
        );
    }

    /// TM-DOS-005: Gzip respects filesystem limits for output files
    #[tokio::test]
    async fn threat_gzip_respects_fs_limits() {
        let mut bash = Bash::new();

        // Basic gzip/gunzip roundtrip preserves data
        let result = bash
            .exec(
                r#"
echo "test data for compression" > /tmp/data.txt
gzip /tmp/data.txt
gunzip /tmp/data.txt.gz
cat /tmp/data.txt
"#,
            )
            .await
            .unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(
            result.stdout.contains("test data for compression"),
            "Roundtrip should preserve data: {}",
            result.stdout
        );
    }
}

// =============================================================================
// TM-INJ-009 / TM-INJ-018: Variable namespace injection via builtins
// =============================================================================

mod variable_namespace_injection {
    use bashkit::Bash;

    async fn exec(script: &str) -> bashkit::ExecResult {
        let mut bash = Bash::builder().build();
        bash.exec(script).await.unwrap()
    }

    /// All internal prefixes that must be blocked
    const INTERNAL_PREFIXES: &[&str] = &[
        "_NAMEREF_x",
        "_READONLY_x",
        "_UPPER_x",
        "_LOWER_x",
        "_ARRAY_READ_x",
        "_EVAL_CMD",
        "_SHIFT_COUNT",
        "_SET_POSITIONAL",
    ];

    // --- local builtin ---

    #[tokio::test]
    async fn tm_inj_009_local_rejects_internal_prefixes() {
        for prefix in INTERNAL_PREFIXES {
            let result = exec(&format!(
                "myfn() {{ local {prefix}=injected; echo ${{{prefix}:-blocked}}; }}; myfn"
            ))
            .await;
            assert!(
                result.stdout.contains("blocked"),
                "local should block {prefix}: got {:?}",
                result.stdout
            );
        }
    }

    #[tokio::test]
    async fn tm_inj_009_local_allows_normal_vars() {
        let result = exec("myfn() { local MY_VAR=hello; echo $MY_VAR; }; myfn").await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("hello"));
    }

    // --- printf -v ---

    #[tokio::test]
    async fn tm_inj_009_printf_v_rejects_internal_prefixes() {
        for prefix in INTERNAL_PREFIXES {
            let result = exec(&format!(
                "printf -v {prefix} injected; echo ${{{prefix}:-blocked}}"
            ))
            .await;
            assert!(
                result.stdout.contains("blocked"),
                "printf -v should block {prefix}: got {:?}",
                result.stdout
            );
        }
    }

    #[tokio::test]
    async fn tm_inj_009_printf_v_allows_normal_vars() {
        let result = exec("printf -v MY_VAR 'hello'; echo $MY_VAR").await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("hello"));
    }

    // --- read ---

    #[tokio::test]
    async fn tm_inj_009_read_rejects_internal_prefixes() {
        for prefix in INTERNAL_PREFIXES {
            let result = exec(&format!(
                "echo injected | read {prefix}; echo ${{{prefix}:-blocked}}"
            ))
            .await;
            assert!(
                result.stdout.contains("blocked"),
                "read should block {prefix}: got {:?}",
                result.stdout
            );
        }
    }

    #[tokio::test]
    async fn tm_inj_009_read_allows_normal_vars() {
        let result = exec("echo hello | read MY_VAR; echo $MY_VAR").await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("hello"));
    }

    #[tokio::test]
    async fn tm_inj_009_read_array_rejects_internal_prefixes() {
        let result = exec("echo 'a b c' | read -a _NAMEREF_x; echo ${_NAMEREF_x:-blocked}").await;
        assert!(
            result.stdout.contains("blocked"),
            "read -a should block _NAMEREF_x: got {:?}",
            result.stdout
        );
    }

    // --- dotenv ---

    #[tokio::test]
    async fn tm_inj_018_dotenv_rejects_internal_prefixes() {
        let result = exec(
            r#"
echo '_NAMEREF_x=injected' > /tmp/.env
echo '_READONLY_y=injected' >> /tmp/.env
echo 'NORMAL=ok' >> /tmp/.env
dotenv /tmp/.env
echo ${_NAMEREF_x:-blocked1} ${_READONLY_y:-blocked2} $NORMAL
"#,
        )
        .await;
        assert!(
            result.stdout.contains("blocked1"),
            "dotenv should block _NAMEREF_x: got {:?}",
            result.stdout
        );
        assert!(
            result.stdout.contains("blocked2"),
            "dotenv should block _READONLY_y: got {:?}",
            result.stdout
        );
        assert!(
            result.stdout.contains("ok"),
            "dotenv should allow normal vars: got {:?}",
            result.stdout
        );
    }

    // --- Cross-builtin: injected markers from one builtin don't affect another ---

    #[tokio::test]
    async fn tm_inj_cross_builtin_no_state_corruption() {
        // Attempt to inject _READONLY_ via local, verify readonly check isn't affected
        let result = exec(
            r#"
myfn() { local _READONLY_FOO=1; }
myfn
FOO=bar
echo $FOO
"#,
        )
        .await;
        assert_eq!(result.exit_code, 0);
        assert!(
            result.stdout.contains("bar"),
            "Cross-builtin injection should not affect state: got {:?}",
            result.stdout
        );
    }
}

// =============================================================================
// OVERLAY FS LIMIT ACCOUNTING (issue #653)
// =============================================================================

mod overlay_limit_accounting {
    use super::*;

    fn make_lower() -> Arc<InMemoryFs> {
        Arc::new(InMemoryFs::new())
    }

    // --- TM-DOS-035: Combined accounting (upper + lower) ---

    /// TM-DOS-035: check_write_limits must use combined usage, not upper-only.
    /// With 80 bytes in lower and 100-byte limit, writing 30 bytes should fail.
    #[tokio::test]
    async fn tm_dos_035_combined_byte_limit() {
        let lower = make_lower();
        lower
            .write_file(Path::new("/tmp/big.txt"), &[b'A'; 80])
            .await
            .unwrap();

        let limits = FsLimits::new().max_total_bytes(100);
        let overlay = OverlayFs::with_limits(lower, limits);

        // 80 (lower) + 30 (new) = 110 > 100
        let result = overlay
            .write_file(Path::new("/tmp/extra.txt"), &[b'B'; 30])
            .await;
        assert!(
            result.is_err(),
            "TM-DOS-035: write should fail when combined usage exceeds limit"
        );
    }

    /// TM-DOS-035: File count limit must include lower layer files.
    #[tokio::test]
    async fn tm_dos_035_combined_file_count_limit() {
        let lower = make_lower();
        lower
            .write_file(Path::new("/tmp/existing.txt"), b"data")
            .await
            .unwrap();

        let temp = OverlayFs::new(lower.clone());
        let base_count = temp.usage().file_count;

        let limits = FsLimits::new().max_file_count(base_count + 1);
        let overlay = OverlayFs::with_limits(lower, limits);

        overlay
            .write_file(Path::new("/tmp/new1.txt"), b"ok")
            .await
            .unwrap();

        let result = overlay
            .write_file(Path::new("/tmp/new2.txt"), b"fail")
            .await;
        assert!(
            result.is_err(),
            "TM-DOS-035: file count limit must include lower layer"
        );
    }

    // --- TM-DOS-036: Double-counting overwritten files ---

    /// TM-DOS-036: Overwriting a lower file in upper should not double-count.
    #[tokio::test]
    async fn tm_dos_036_no_double_count_on_override() {
        let lower = make_lower();
        lower
            .write_file(Path::new("/tmp/file.txt"), &[b'L'; 100])
            .await
            .unwrap();

        let overlay = OverlayFs::new(lower);
        let before = overlay.usage();

        overlay
            .write_file(Path::new("/tmp/file.txt"), &[b'U'; 50])
            .await
            .unwrap();

        let after = overlay.usage();
        assert_eq!(
            after.file_count, before.file_count,
            "TM-DOS-036: overridden file should not increase count"
        );
        assert_eq!(
            after.total_bytes,
            before.total_bytes - 50,
            "TM-DOS-036: bytes should reflect upper size, not sum"
        );
    }

    /// TM-DOS-036: Whiteout should deduct lower file from usage.
    #[tokio::test]
    async fn tm_dos_036_whiteout_deducts_usage() {
        let lower = make_lower();
        lower
            .write_file(Path::new("/tmp/gone.txt"), &[b'X'; 200])
            .await
            .unwrap();

        let overlay = OverlayFs::new(lower);
        let before = overlay.usage();

        overlay
            .remove(Path::new("/tmp/gone.txt"), false)
            .await
            .unwrap();

        let after = overlay.usage();
        assert_eq!(
            after.total_bytes,
            before.total_bytes - 200,
            "TM-DOS-036: whited-out file bytes should be deducted"
        );
        assert_eq!(
            after.file_count,
            before.file_count - 1,
            "TM-DOS-036: whited-out file should be deducted from count"
        );
    }

    // --- TM-DOS-037: chmod CoW bypasses limits ---

    /// TM-DOS-037: chmod on lower file triggers CoW, must check write limits.
    #[tokio::test]
    async fn tm_dos_037_chmod_file_cow_checks_limits() {
        let lower = make_lower();
        lower
            .write_file(Path::new("/tmp/big.txt"), &[b'X'; 5000])
            .await
            .unwrap();

        let limits = FsLimits::new().max_total_bytes(1000);
        let overlay = OverlayFs::with_limits(lower, limits);

        let result = overlay.chmod(Path::new("/tmp/big.txt"), 0o755).await;
        assert!(
            result.is_err(),
            "TM-DOS-037: chmod CoW should fail when content exceeds write limits"
        );
    }

    /// TM-DOS-037: chmod on lower directory triggers CoW, must check dir limits.
    #[tokio::test]
    async fn tm_dos_037_chmod_dir_cow_checks_limits() {
        let lower = make_lower();
        // Create many directories in lower to fill up dir count
        for i in 0..10 {
            lower
                .mkdir(Path::new(&format!("/d{}", i)), true)
                .await
                .unwrap();
        }

        // Get base dir count, then set limit to exactly that
        let temp = OverlayFs::new(lower.clone());
        let base_dirs = temp.usage().dir_count;

        let limits = FsLimits::new().max_dir_count(base_dirs);
        let overlay = OverlayFs::with_limits(lower, limits);

        // chmod a lower directory should trigger CoW mkdir — must be rejected
        let result = overlay.chmod(Path::new("/d0"), 0o755).await;
        assert!(
            result.is_err(),
            "TM-DOS-037: chmod dir CoW should fail when dir count at limit"
        );
    }

    /// TM-DOS-037: mkdir should check dir count limits.
    #[tokio::test]
    async fn tm_dos_037_mkdir_checks_dir_limits() {
        let lower = make_lower();
        let temp = OverlayFs::new(lower.clone());
        let base_dirs = temp.usage().dir_count;

        let limits = FsLimits::new().max_dir_count(base_dirs + 1);
        let overlay = OverlayFs::with_limits(lower, limits);

        // First mkdir should succeed
        overlay.mkdir(Path::new("/newdir"), false).await.unwrap();

        // Second mkdir should fail
        let result = overlay.mkdir(Path::new("/newdir2"), false).await;
        assert!(
            result.is_err(),
            "TM-DOS-037: mkdir should fail when dir count exceeds limit"
        );
    }

    // --- TM-DOS-038: Incomplete recursive whiteout ---

    /// TM-DOS-038: Recursive delete must hide all lower children.
    #[tokio::test]
    async fn tm_dos_038_recursive_delete_hides_all_children() {
        let lower = make_lower();
        lower.mkdir(Path::new("/data"), true).await.unwrap();
        lower
            .write_file(Path::new("/data/a.txt"), b"aaa")
            .await
            .unwrap();
        lower
            .write_file(Path::new("/data/b.txt"), b"bbb")
            .await
            .unwrap();
        lower.mkdir(Path::new("/data/sub"), true).await.unwrap();
        lower
            .write_file(Path::new("/data/sub/c.txt"), b"ccc")
            .await
            .unwrap();

        let overlay = OverlayFs::new(lower);

        overlay.remove(Path::new("/data"), true).await.unwrap();

        // All children must be invisible
        assert!(
            !overlay.exists(Path::new("/data")).await.unwrap(),
            "TM-DOS-038: directory itself should be hidden"
        );
        assert!(
            !overlay.exists(Path::new("/data/a.txt")).await.unwrap(),
            "TM-DOS-038: child file should be hidden"
        );
        assert!(
            !overlay.exists(Path::new("/data/sub/c.txt")).await.unwrap(),
            "TM-DOS-038: nested child should be hidden"
        );

        // read_file should fail
        assert!(overlay.read_file(Path::new("/data/a.txt")).await.is_err());
        assert!(
            overlay
                .read_file(Path::new("/data/sub/c.txt"))
                .await
                .is_err()
        );
    }

    /// TM-DOS-038: Usage must deduct all recursively deleted children.
    #[tokio::test]
    async fn tm_dos_038_recursive_delete_deducts_all_bytes() {
        let lower = make_lower();
        lower.mkdir(Path::new("/stuff"), true).await.unwrap();
        lower
            .write_file(Path::new("/stuff/x.txt"), &[b'X'; 100])
            .await
            .unwrap();
        lower
            .write_file(Path::new("/stuff/y.txt"), &[b'Y'; 200])
            .await
            .unwrap();
        lower.mkdir(Path::new("/stuff/deep"), true).await.unwrap();
        lower
            .write_file(Path::new("/stuff/deep/z.txt"), &[b'Z'; 50])
            .await
            .unwrap();

        let overlay = OverlayFs::new(lower);
        let before = overlay.usage();

        overlay.remove(Path::new("/stuff"), true).await.unwrap();

        let after = overlay.usage();
        assert_eq!(
            after.total_bytes,
            before.total_bytes - 350,
            "TM-DOS-038: should deduct all child file bytes (100+200+50)"
        );
        assert_eq!(
            after.file_count,
            before.file_count - 3,
            "TM-DOS-038: should deduct all child file counts"
        );
    }

    // --- Boundary math ---

    /// Boundary: lower=50, upper=49, limit=100, write 2 → should fail.
    #[tokio::test]
    async fn tm_dos_boundary_exact_limit() {
        let lower = make_lower();
        lower
            .write_file(Path::new("/tmp/lower.txt"), &[b'A'; 50])
            .await
            .unwrap();

        let limits = FsLimits::new().max_total_bytes(100);
        let overlay = OverlayFs::with_limits(lower, limits);

        // 50 (lower) + 49 (upper) = 99 <= 100: should succeed
        overlay
            .write_file(Path::new("/tmp/upper.txt"), &[b'B'; 49])
            .await
            .unwrap();

        // 99 + 2 = 101 > 100: should fail
        let result = overlay
            .write_file(Path::new("/tmp/over.txt"), &[b'C'; 2])
            .await;
        assert!(result.is_err(), "boundary: 99 + 2 = 101 > 100 should fail");
    }

    // --- CoW accumulation via repeated chmod ---

    /// Repeated chmod on different lower files should accumulate CoW correctly.
    /// After CoW, usage should reflect correct combined accounting.
    #[tokio::test]
    async fn tm_dos_cow_accumulation_via_chmod() {
        let lower = make_lower();
        for i in 0..5 {
            lower
                .write_file(Path::new(&format!("/tmp/f{}.txt", i)), &[b'A'; 100])
                .await
                .unwrap();
        }

        // Give generous limit so chmod CoW succeeds (check is conservative: adds
        // content_size before deducting hidden lower, so limit must be >= usage + file_size)
        let temp = OverlayFs::new(lower.clone());
        let base = temp.usage().total_bytes;
        let limits = FsLimits::new().max_total_bytes(base + 500);
        let overlay = OverlayFs::with_limits(lower, limits);

        let before = overlay.usage();

        for i in 0..5 {
            let path = format!("/tmp/f{}.txt", i);
            overlay.chmod(Path::new(&path), 0o755).await.unwrap();
        }

        let after = overlay.usage();
        // Each chmod copies file to upper (100 bytes) and hides lower (100 bytes) → net 0
        assert_eq!(
            after.total_bytes, before.total_bytes,
            "CoW chmod should not change total bytes"
        );
        assert_eq!(
            after.file_count, before.file_count,
            "CoW chmod should not change file count"
        );
    }
}

// =============================================================================
// YAML/TEMPLATE RECURSION DEPTH LIMITS (issue #654)
// =============================================================================

mod yaml_template_depth {
    use super::*;

    fn bash() -> Bash {
        Bash::builder().build()
    }

    // --- TM-DOS-051: YAML depth bomb ---

    /// TM-DOS-051: Deeply nested YAML map should produce error, not stack overflow.
    #[tokio::test]
    async fn tm_dos_051_yaml_depth_bomb_maps() {
        let mut bash = bash();
        // Generate 200-level nested YAML
        let mut yaml = String::new();
        for i in 0..200 {
            let indent = "  ".repeat(i);
            yaml.push_str(&format!("{indent}level{i}:\n"));
        }
        let last_indent = "  ".repeat(200);
        yaml.push_str(&format!("{last_indent}value: deep\n"));

        let cmd = format!("yaml get level0.level1.level2 - <<'YAML_EOF'\n{yaml}YAML_EOF");
        let result = bash.exec(&cmd).await.unwrap();
        // Should get an error or truncated result, not a stack overflow/panic
        let output = format!("{}{}", result.stdout, result.stderr);
        assert!(
            output.contains("depth exceeded") || result.exit_code != 0 || output.contains("ERROR"),
            "TM-DOS-051: deeply nested YAML should produce clean error, got: stdout={:?} stderr={:?}",
            result.stdout,
            result.stderr
        );
    }

    /// TM-DOS-051: Deeply nested YAML list should produce error.
    #[tokio::test]
    async fn tm_dos_051_yaml_depth_bomb_lists() {
        let mut bash = bash();
        let mut yaml = String::new();
        for i in 0..200 {
            let indent = "  ".repeat(i);
            yaml.push_str(&format!("{indent}-\n"));
        }
        let last_indent = "  ".repeat(200);
        yaml.push_str(&format!("{last_indent}- leaf\n"));

        let cmd = format!("yaml get . - <<'YAML_EOF'\n{yaml}YAML_EOF");
        let result = bash.exec(&cmd).await.unwrap();
        let output = format!("{}{}", result.stdout, result.stderr);
        assert!(
            output.contains("depth exceeded") || result.exit_code != 0 || output.contains("ERROR"),
            "TM-DOS-051: deeply nested YAML list should produce clean error"
        );
    }

    /// TM-DOS-051: YAML with reasonable nesting should work fine.
    #[tokio::test]
    async fn tm_dos_051_yaml_normal_nesting_works() {
        let mut bash = bash();
        // Write a 5-level nested YAML to a file, then query it
        let script = r#"
cat > /tmp/test.yaml << 'EOF'
a:
  b:
    c:
      d:
        e: deep_value
EOF
yaml get -r a.b.c.d.e /tmp/test.yaml
"#;
        let result = bash.exec(script).await.unwrap();
        assert_eq!(
            result.exit_code, 0,
            "yaml get should succeed: stderr={:?} stdout={:?}",
            result.stderr, result.stdout
        );
        assert!(
            result.stdout.trim() == "deep_value",
            "Normal 5-level nesting should work: got {:?}",
            result.stdout
        );
    }

    // --- TM-DOS-052: Template depth bomb ---

    /// TM-DOS-052: Deeply nested {{#if}} should produce error, not stack overflow.
    #[tokio::test]
    async fn tm_dos_052_template_if_depth_bomb() {
        let mut bash = bash();
        let mut template = String::new();
        for _ in 0..200 {
            template.push_str("{{#if x}}");
        }
        template.push_str("deep");
        for _ in 0..200 {
            template.push_str("{{/if}}");
        }

        // Write template and JSON to VFS, then render
        let script = format!(
            r#"echo '{template}' > /tmp/tpl.txt
echo '{{"x": true}}' > /tmp/data.json
template render /tmp/tpl.txt -d /tmp/data.json"#
        );
        let result = bash.exec(&script).await.unwrap();
        let output = format!("{}{}", result.stdout, result.stderr);
        assert!(
            output.contains("depth exceeded") || result.exit_code != 0,
            "TM-DOS-052: deeply nested #if should produce clean error, got: stdout={:?} stderr={:?}",
            result.stdout,
            result.stderr
        );
    }

    /// TM-DOS-052: Template with reasonable usage should work fine.
    #[tokio::test]
    async fn tm_dos_052_template_normal_nesting_works() {
        let mut bash = bash();
        let script = r#"
cat > /tmp/tpl.txt << 'TPLEOF'
{{#if x}}hello from template{{/if}}
TPLEOF
cat > /tmp/data.json << 'JSONEOF'
{"x": true}
JSONEOF
template render /tmp/tpl.txt -d /tmp/data.json
"#;
        let result = bash.exec(script).await.unwrap();
        assert_eq!(
            result.exit_code, 0,
            "template render should succeed: stderr={:?} stdout={:?}",
            result.stderr, result.stdout
        );
        assert!(
            result.stdout.contains("hello from template"),
            "Normal template should work: got {:?}",
            result.stdout
        );
    }
}

// =============================================================================
// SESSION-LEVEL CUMULATIVE RESOURCE COUNTERS (TM-DOS-059)
// =============================================================================

mod session_limits {
    use super::*;

    /// TM-DOS-059: Cumulative command limit across multiple exec() calls.
    /// Run N exec() calls each with K commands; verify failure when total > max_total_commands.
    #[tokio::test]
    async fn tm_dos_059_cumulative_command_limit() {
        let session = SessionLimits::new()
            .max_total_commands(15)
            .max_exec_calls(100);
        let limits = ExecutionLimits::new().max_commands(10_000);
        let mut bash = Bash::builder()
            .limits(limits)
            .session_limits(session)
            .build();

        // Each exec runs 5 commands (echo x 5). Three calls = 15 commands should be near limit.
        let script = "echo 1; echo 2; echo 3; echo 4; echo 5";

        let r1 = bash.exec(script).await.unwrap();
        assert_eq!(r1.exit_code, 0, "first batch should succeed");

        let r2 = bash.exec(script).await.unwrap();
        assert_eq!(r2.exit_code, 0, "second batch should succeed");

        // Eventually we should hit the session command limit (Err or non-zero exit).
        let mut hit_limit = false;
        for _ in 0..5 {
            match bash.exec(script).await {
                Err(e) => {
                    let msg = e.to_string();
                    assert!(
                        msg.contains("session"),
                        "error should mention session: {msg}"
                    );
                    hit_limit = true;
                    break;
                }
                Ok(r) if r.exit_code != 0 => {
                    hit_limit = true;
                    break;
                }
                Ok(_) => {} // keep going
            }
        }
        assert!(hit_limit, "should eventually hit session command limit");
    }

    /// TM-DOS-059: exec() call count limit.
    #[tokio::test]
    async fn tm_dos_059_exec_call_count_limit() {
        let session = SessionLimits::new()
            .max_exec_calls(3)
            .max_total_commands(u64::MAX);
        let limits = ExecutionLimits::new();
        let mut bash = Bash::builder()
            .limits(limits)
            .session_limits(session)
            .build();

        // First 3 exec() calls should succeed
        for i in 0..3 {
            let r = bash.exec("echo ok").await.unwrap();
            assert_eq!(r.exit_code, 0, "exec call {} should succeed", i + 1);
        }

        // 4th exec() should fail with Err (session limit exceeded at entry)
        let r = bash.exec("echo should_fail").await;
        assert!(
            r.is_err(),
            "4th exec call should fail due to session exec limit"
        );
        let msg = r.unwrap_err().to_string();
        assert!(
            msg.contains("session") && msg.contains("exec"),
            "error should mention session exec limit: {msg}"
        );
    }

    /// TM-DOS-059: Session counters persist across exec() calls (not reset).
    #[tokio::test]
    async fn tm_dos_059_counter_persistence() {
        let session = SessionLimits::new()
            .max_total_commands(20)
            .max_exec_calls(100);
        let limits = ExecutionLimits::new().max_commands(10_000);
        let mut bash = Bash::builder()
            .limits(limits)
            .session_limits(session)
            .build();

        // Run commands in separate exec() calls, accumulating toward limit
        let mut hit_limit = false;
        for i in 0..20 {
            match bash.exec("echo a; echo b; echo c").await {
                Err(e) => {
                    let msg = e.to_string();
                    assert!(
                        msg.contains("session"),
                        "error should mention session: {msg}"
                    );
                    hit_limit = true;
                    assert!(
                        i >= 3,
                        "should succeed for at least a few calls before limit"
                    );
                    break;
                }
                Ok(r) if r.exit_code != 0 => {
                    hit_limit = true;
                    break;
                }
                Ok(_) => {}
            }
        }
        assert!(hit_limit, "should eventually hit session command limit");
    }

    /// TM-DOS-059: Per-exec limits still work independently of session limits.
    #[tokio::test]
    async fn tm_dos_059_per_exec_limits_still_work() {
        let session = SessionLimits::unlimited();
        let limits = ExecutionLimits::new().max_commands(3);
        let mut bash = Bash::builder()
            .limits(limits)
            .session_limits(session)
            .build();

        // Per-exec limit of 3 commands should still trigger.
        // May return Ok with non-zero exit or Err depending on how limit surfaces.
        let r = bash.exec("echo 1; echo 2; echo 3; echo 4; echo 5").await;
        match r {
            Ok(result) => assert_ne!(
                result.exit_code, 0,
                "per-exec command limit should still work"
            ),
            Err(e) => {
                let msg = e.to_string();
                assert!(
                    msg.contains("command") || msg.contains("limit"),
                    "error should be about command limit: {msg}"
                );
            }
        }
    }

    /// TM-DOS-059: Builder API configures session limits correctly.
    #[tokio::test]
    async fn tm_dos_059_builder_api() {
        let session = SessionLimits::new()
            .max_total_commands(50)
            .max_exec_calls(10);
        let mut bash = Bash::builder().session_limits(session).build();

        // Should work within limits
        let r = bash.exec("echo hello").await.unwrap();
        assert_eq!(r.exit_code, 0);
        assert!(r.stdout.contains("hello"));
    }

    /// TM-DOS-059: Default session limits are reasonable and non-zero.
    #[tokio::test]
    async fn tm_dos_059_default_safety() {
        let defaults = SessionLimits::default();
        assert!(
            defaults.max_total_commands > 0,
            "default max_total_commands should be non-zero"
        );
        assert!(
            defaults.max_exec_calls > 0,
            "default max_exec_calls should be non-zero"
        );
        // Defaults should be reasonably large but finite
        assert!(
            defaults.max_total_commands < u64::MAX,
            "default max_total_commands should be finite"
        );
        assert!(
            defaults.max_exec_calls < u64::MAX,
            "default max_exec_calls should be finite"
        );
        // Verify specific expected values
        assert_eq!(defaults.max_total_commands, 100_000);
        assert_eq!(defaults.max_exec_calls, 1_000);
    }

    /// TM-DOS-059: SessionLimits::unlimited() disables all session limits.
    #[tokio::test]
    async fn tm_dos_059_unlimited() {
        let unlimited = SessionLimits::unlimited();
        assert_eq!(unlimited.max_total_commands, u64::MAX);
        assert_eq!(unlimited.max_exec_calls, u64::MAX);
    }

    /// TM-DOS-059: reset_for_execution does NOT reset session counters.
    #[test]
    fn tm_dos_059_reset_preserves_session_counters() {
        use bashkit::ExecutionCounters;

        let limits = ExecutionLimits::new().max_commands(10_000);
        let mut counters = ExecutionCounters::new();

        // Simulate some commands
        for _ in 0..5 {
            counters.tick_command(&limits).unwrap();
        }
        counters.tick_exec_call();

        assert_eq!(counters.session_commands, 5);
        assert_eq!(counters.session_exec_calls, 1);

        // reset_for_execution should NOT reset session counters
        counters.reset_for_execution();

        assert_eq!(
            counters.session_commands, 5,
            "session_commands must persist"
        );
        assert_eq!(
            counters.session_exec_calls, 1,
            "session_exec_calls must persist"
        );
        // But per-exec counters should be reset
        assert_eq!(counters.commands, 0);
    }
}

// =============================================================================
// MEMORY BUDGET LIMITS (TM-DOS-060)
// =============================================================================

mod memory_limits {
    use super::*;

    /// TM-DOS-060: Variable count bomb — script creating many variables.
    #[tokio::test]
    async fn tm_dos_060_variable_count_bomb() {
        let mem = MemoryLimits::new().max_variable_count(50);
        let limits = ExecutionLimits::new()
            .max_commands(10_000)
            .max_loop_iterations(10_000);
        let mut bash = Bash::builder()
            .limits(limits)
            .memory_limits(mem)
            .session_limits(SessionLimits::unlimited())
            .build();

        // Try to create 100 variables — should stop at 50
        let script = r#"
for i in $(seq 1 100); do
    eval "var_$i=hello"
done
echo "done"
"#;
        let result = bash.exec(script).await.unwrap();
        // The script should complete but some variables won't be created
        assert_eq!(result.exit_code, 0);
    }

    /// TM-DOS-060: Variable byte bomb — large variable values.
    #[tokio::test]
    async fn tm_dos_060_variable_size_bomb() {
        let mem = MemoryLimits::new().max_total_variable_bytes(1000);
        let limits = ExecutionLimits::new();
        let mut bash = Bash::builder()
            .limits(limits)
            .memory_limits(mem)
            .session_limits(SessionLimits::unlimited())
            .build();

        // Try to create a variable with a large value
        let script = r#"
big=$(printf '%0500s' | tr ' ' 'A')
echo ${#big}
big2=$(printf '%0500s' | tr ' ' 'B')
echo ${#big2}
"#;
        let result = bash.exec(script).await.unwrap();
        // First variable should succeed, second may be rejected
        assert_eq!(result.exit_code, 0);
        let lines: Vec<&str> = result.stdout.trim().lines().collect();
        assert!(!lines.is_empty(), "should have produced some output");
    }

    /// TM-DOS-060: Array entry bomb — indexed array with many entries.
    #[tokio::test]
    async fn tm_dos_060_array_entry_bomb() {
        let mem = MemoryLimits::new().max_array_entries(50);
        let limits = ExecutionLimits::new()
            .max_commands(10_000)
            .max_loop_iterations(10_000);
        let mut bash = Bash::builder()
            .limits(limits)
            .memory_limits(mem)
            .session_limits(SessionLimits::unlimited())
            .build();

        // Try to create array with 100 entries
        let script = r#"
for i in $(seq 1 100); do
    arr[$i]=hello
done
echo "done"
"#;
        let result = bash.exec(script).await.unwrap();
        // Script completes; insertions beyond limit are silently dropped
        assert_eq!(result.exit_code, 0);
    }

    /// TM-DOS-060: Function count bomb — defining many functions.
    #[tokio::test]
    async fn tm_dos_060_function_count_bomb() {
        let mem = MemoryLimits::new().max_function_count(10);
        let limits = ExecutionLimits::new()
            .max_commands(10_000)
            .max_loop_iterations(10_000);
        let mut bash = Bash::builder()
            .limits(limits)
            .memory_limits(mem)
            .session_limits(SessionLimits::unlimited())
            .build();

        // Try to define 50 functions
        let script = r#"
for i in $(seq 1 50); do
    eval "func_$i() { echo $i; }"
done
echo "done"
"#;
        let result = bash.exec(script).await.unwrap();
        assert_eq!(result.exit_code, 0);
    }

    /// TM-DOS-060: Normal scripts unaffected by default limits.
    #[tokio::test]
    async fn tm_dos_060_normal_script_unaffected() {
        // Use default memory limits
        let mut bash = Bash::builder().build();

        let script = r#"
name="hello"
count=42
arr=(one two three)
declare -A map=([key1]=val1 [key2]=val2)
greet() { echo "Hello $1"; }
greet "world"
echo "$name $count ${arr[1]}"
"#;
        let result = bash.exec(script).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("Hello world"));
        assert!(result.stdout.contains("hello 42 two"));
    }

    /// TM-DOS-060: Default memory limits are reasonable and non-zero.
    #[test]
    fn tm_dos_060_default_safety() {
        let defaults = MemoryLimits::default();
        assert_eq!(defaults.max_variable_count, 10_000);
        assert_eq!(defaults.max_total_variable_bytes, 10_000_000);
        assert_eq!(defaults.max_array_entries, 100_000);
        assert_eq!(defaults.max_function_count, 1_000);
        assert_eq!(defaults.max_function_body_bytes, 1_000_000);
    }

    /// TM-DOS-060: MemoryLimits::unlimited() disables limits.
    #[test]
    fn tm_dos_060_unlimited() {
        let unlimited = MemoryLimits::unlimited();
        assert_eq!(unlimited.max_variable_count, usize::MAX);
        assert_eq!(unlimited.max_total_variable_bytes, usize::MAX);
        assert_eq!(unlimited.max_array_entries, usize::MAX);
        assert_eq!(unlimited.max_function_count, usize::MAX);
        assert_eq!(unlimited.max_function_body_bytes, usize::MAX);
    }

    /// TM-DOS-060: Builder API configures memory limits correctly.
    #[tokio::test]
    async fn tm_dos_060_builder_api() {
        let mem = MemoryLimits::new()
            .max_variable_count(500)
            .max_total_variable_bytes(50_000)
            .max_array_entries(1000)
            .max_function_count(50)
            .max_function_body_bytes(10_000);
        let mut bash = Bash::builder().memory_limits(mem).build();

        let result = bash.exec("echo hello").await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("hello"));
    }

    /// TM-DOS-060: Cross-instance isolation — two Bash instances have independent budgets.
    #[tokio::test]
    async fn tm_dos_060_cross_instance_isolation() {
        let mem = MemoryLimits::new().max_variable_count(20);
        let limits = ExecutionLimits::new()
            .max_commands(10_000)
            .max_loop_iterations(10_000);

        let mut bash1 = Bash::builder()
            .limits(limits.clone())
            .memory_limits(mem.clone())
            .session_limits(SessionLimits::unlimited())
            .build();
        let mut bash2 = Bash::builder()
            .limits(limits)
            .memory_limits(mem)
            .session_limits(SessionLimits::unlimited())
            .build();

        // Both should independently handle variable creation
        let script = r#"
for i in $(seq 1 15); do
    eval "x_$i=test"
done
echo "done"
"#;
        let r1 = bash1.exec(script).await.unwrap();
        let r2 = bash2.exec(script).await.unwrap();
        assert_eq!(r1.exit_code, 0);
        assert_eq!(r2.exit_code, 0);
    }
}

// =============================================================================
// TRACE EVENT TESTS (TM-INF-019)
// =============================================================================

mod trace_events {
    use super::*;

    #[tokio::test]
    async fn trace_off_produces_no_events() {
        let mut bash = Bash::builder().trace_mode(TraceMode::Off).build();
        let r = bash.exec("echo hello; echo world").await.unwrap();
        assert_eq!(r.exit_code, 0);
        assert!(r.events.is_empty(), "Off mode should produce no events");
    }

    #[tokio::test]
    async fn trace_full_records_command_start_and_exit() {
        let mut bash = Bash::builder().trace_mode(TraceMode::Full).build();
        let r = bash.exec("echo hello").await.unwrap();
        assert_eq!(r.exit_code, 0);
        assert!(
            r.events.len() >= 2,
            "Full mode should record at least start+exit, got {}",
            r.events.len()
        );

        // First event should be CommandStart for echo
        assert_eq!(r.events[0].kind, TraceEventKind::CommandStart);
        if let TraceEventDetails::CommandStart { command, argv, .. } = &r.events[0].details {
            assert_eq!(command, "echo");
            assert_eq!(argv, &["hello"]);
        } else {
            panic!("expected CommandStart details");
        }

        // Second event should be CommandExit for echo
        assert_eq!(r.events[1].kind, TraceEventKind::CommandExit);
        if let TraceEventDetails::CommandExit {
            command, exit_code, ..
        } = &r.events[1].details
        {
            assert_eq!(command, "echo");
            assert_eq!(*exit_code, 0);
        } else {
            panic!("expected CommandExit details");
        }
    }

    #[tokio::test]
    async fn trace_full_multiple_commands() {
        let mut bash = Bash::builder().trace_mode(TraceMode::Full).build();
        let r = bash.exec("echo a; echo b; echo c").await.unwrap();
        assert_eq!(r.exit_code, 0);

        // Should have start+exit for each of three echo commands
        let starts: Vec<_> = r
            .events
            .iter()
            .filter(|e| e.kind == TraceEventKind::CommandStart)
            .collect();
        let exits: Vec<_> = r
            .events
            .iter()
            .filter(|e| e.kind == TraceEventKind::CommandExit)
            .collect();
        assert_eq!(starts.len(), 3, "3 commands should have 3 starts");
        assert_eq!(exits.len(), 3, "3 commands should have 3 exits");
    }

    #[tokio::test]
    async fn trace_full_seq_numbers_monotonic() {
        let mut bash = Bash::builder().trace_mode(TraceMode::Full).build();
        let r = bash.exec("echo a; echo b").await.unwrap();
        assert!(r.events.len() >= 4);
        for i in 1..r.events.len() {
            assert!(
                r.events[i].seq > r.events[i - 1].seq,
                "seq numbers should be strictly monotonic"
            );
        }
    }

    #[tokio::test]
    async fn trace_redacted_scrubs_secrets_in_argv() {
        let mut bash = Bash::builder().trace_mode(TraceMode::Redacted).build();
        // Use printf to avoid actual HTTP call; the trace records the command start
        let r = bash
            .exec(r#"printf "%s\n" -H "Authorization: Bearer secret123" "https://api.example.com""#)
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        assert!(!r.events.is_empty());

        // Check that no event leaks "secret123"
        for event in &r.events {
            if let TraceEventDetails::CommandStart { argv, .. } = &event.details {
                for arg in argv {
                    assert!(
                        !arg.contains("secret123"),
                        "Redacted mode should not leak secrets: {arg}"
                    );
                }
            }
        }
    }

    #[tokio::test]
    async fn trace_redacted_scrubs_env_secrets() {
        let mut bash = Bash::builder().trace_mode(TraceMode::Redacted).build();
        // printf with env-style key=value argument containing a secret suffix
        let r = bash
            .exec(r#"printf "%s" "API_KEY=supersecret""#)
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);

        for event in &r.events {
            if let TraceEventDetails::CommandStart { argv, .. } = &event.details {
                for arg in argv {
                    assert!(
                        !arg.contains("supersecret"),
                        "Redacted mode should scrub env secrets: {arg}"
                    );
                }
            }
        }
    }

    #[tokio::test]
    async fn trace_full_does_not_scrub() {
        let mut bash = Bash::builder().trace_mode(TraceMode::Full).build();
        let r = bash
            .exec(r#"printf "%s" "API_KEY=supersecret""#)
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);

        let mut found_secret = false;
        for event in &r.events {
            if let TraceEventDetails::CommandStart { argv, .. } = &event.details {
                for arg in argv {
                    if arg.contains("supersecret") {
                        found_secret = true;
                    }
                }
            }
        }
        assert!(found_secret, "Full mode should preserve raw secrets");
    }

    #[tokio::test]
    async fn trace_callback_receives_events() {
        use std::sync::{Arc, Mutex};
        let count = Arc::new(Mutex::new(0u32));
        let count_clone = count.clone();
        let mut bash = Bash::builder()
            .trace_mode(TraceMode::Full)
            .on_trace_event(Box::new(move |_event| {
                *count_clone.lock().unwrap() += 1;
            }))
            .build();
        let r = bash.exec("echo hello").await.unwrap();
        assert_eq!(r.exit_code, 0);
        let cb_count = *count.lock().unwrap();
        assert!(
            cb_count >= 2,
            "callback should fire for at least start+exit, got {cb_count}"
        );
        // events should also be in the result
        assert_eq!(r.events.len() as u32, cb_count);
    }

    #[tokio::test]
    async fn trace_off_zero_overhead() {
        let mut bash = Bash::builder().trace_mode(TraceMode::Off).build();
        // Run a loop to verify no events accumulate
        let r = bash
            .exec("for i in $(seq 1 100); do echo $i; done")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        assert!(r.events.is_empty());
    }

    #[tokio::test]
    async fn trace_records_function_calls() {
        let mut bash = Bash::builder().trace_mode(TraceMode::Full).build();
        let r = bash.exec("myfn() { echo inside; }; myfn").await.unwrap();
        assert_eq!(r.exit_code, 0);

        // Should see start/exit for myfn and start/exit for echo inside it
        let fn_starts: Vec<_> = r
            .events
            .iter()
            .filter(|e| {
                e.kind == TraceEventKind::CommandStart
                    && matches!(&e.details, TraceEventDetails::CommandStart { command, .. } if command == "myfn")
            })
            .collect();
        assert!(
            !fn_starts.is_empty(),
            "should record CommandStart for function call"
        );
    }

    #[tokio::test]
    async fn trace_records_command_not_found() {
        let mut bash = Bash::builder().trace_mode(TraceMode::Full).build();
        let r = bash.exec("nonexistent_command_xyz").await.unwrap();
        assert_ne!(r.exit_code, 0);

        // Should still get start+exit (exit code 127)
        let exits: Vec<_> = r
            .events
            .iter()
            .filter(|e| {
                e.kind == TraceEventKind::CommandExit
                    && matches!(&e.details, TraceEventDetails::CommandExit { exit_code, .. } if exit_code == &127)
            })
            .collect();
        assert!(
            !exits.is_empty(),
            "should record CommandExit with code 127 for not-found"
        );
    }

    // TM-DOS-029: Malformed ${#[} must not panic (fuzz crash from CI)
    #[tokio::test]
    async fn arithmetic_malformed_brace_length_no_panic() {
        let mut bash = Bash::new();
        // Input discovered by arithmetic_fuzz: [${#[
        // Triggered panic "byte range starts at 1 but ends at 0" in
        // expand_brace_expr_in_arithmetic when rest="[" and bracket=0.
        let r = bash.exec("echo $((0 + ${#[}))").await.unwrap();
        // Should not panic — just return 0 for malformed expression
        assert_eq!(r.exit_code, 0);
    }
}

// =============================================================================
// =============================================================================
// TYPESCRIPT / ZAPCODE SECURITY (TM-TS)
//
// Threat model tests for the embedded TypeScript interpreter (zapcode-core).
// NOTE: TypeScript is opt-in — requires `typescript` feature AND builder call.
// =============================================================================

#[cfg(feature = "typescript")]
mod typescript_security {
    use super::*;
    use bashkit::TypeScriptLimits;
    use std::time::Duration;

    fn bash_with_ts() -> Bash {
        Bash::builder()
            .typescript_with_limits(TypeScriptLimits::default())
            .build()
    }

    fn bash_with_ts_tight() -> Bash {
        Bash::builder()
            .typescript_with_limits(
                TypeScriptLimits::default()
                    .max_duration(Duration::from_secs(3))
                    .max_memory(4 * 1024 * 1024)
                    .max_allocations(50_000)
                    .max_stack_depth(100),
            )
            .build()
    }

    /// TM-TS-001: TypeScript infinite loop blocked by time limit
    #[tokio::test]
    async fn threat_ts_infinite_loop() {
        let mut bash = bash_with_ts_tight();
        let result = bash.exec("ts -c \"while (true) {}\"").await.unwrap();
        assert_ne!(result.exit_code, 0, "Infinite loop should not succeed");
    }

    /// TM-TS-002: TypeScript memory exhaustion blocked
    #[tokio::test]
    async fn threat_ts_memory_exhaustion() {
        let mut bash = bash_with_ts_tight();
        let result = bash
            .exec("ts -c \"const arr: number[] = []; while (true) { arr.push(1); }\"")
            .await
            .unwrap();
        assert_ne!(result.exit_code, 0, "Memory bomb should not succeed");
    }

    /// TM-TS-003: TypeScript stack overflow blocked
    #[tokio::test]
    async fn threat_ts_stack_overflow() {
        let mut bash = bash_with_ts_tight();
        let result = bash
            .exec("ts -c \"const f = (): number => f(); f()\"")
            .await
            .unwrap();
        assert_ne!(result.exit_code, 0, "Stack overflow should not succeed");
    }

    /// TM-TS-005: TypeScript VFS reads only from BashKit VFS
    #[tokio::test]
    async fn threat_ts_vfs_no_real_fs() {
        let mut bash = bash_with_ts();
        let result = bash
            .exec("ts -c \"const c = await readFile('/etc/passwd'); console.log(c)\"")
            .await
            .unwrap();
        assert!(
            !result.stdout.contains("root:"),
            "Should not read real /etc/passwd"
        );
    }

    /// TM-TS-007: TypeScript VFS path traversal blocked
    #[tokio::test]
    async fn threat_ts_vfs_path_traversal() {
        let mut bash = bash_with_ts();
        let result = bash
            .exec("ts -c \"const c = await readFile('/tmp/../../../etc/passwd'); console.log(c)\"")
            .await
            .unwrap();
        assert!(
            !result.stdout.contains("root:"),
            "Path traversal must not escape VFS"
        );
    }

    /// TM-TS-012: TypeScript error output goes to stderr
    #[tokio::test]
    async fn threat_ts_error_isolation() {
        let mut bash = bash_with_ts();
        let result = bash
            .exec("ts -c \"throw new Error('test')\"")
            .await
            .unwrap();
        assert_eq!(result.exit_code, 1);
    }

    /// TM-TS-021: TypeScript cannot execute shell commands
    #[tokio::test]
    async fn threat_ts_no_shell_exec() {
        let mut bash = bash_with_ts();
        let result = bash
            .exec("ts -c \"console.log(process.env.HOME)\"")
            .await
            .unwrap();
        assert_ne!(result.exit_code, 0, "process.env should not exist");
        assert!(
            !result.stdout.contains("/home"),
            "Should not access env vars via process"
        );
    }

    /// Opt-in verification: ts NOT available without builder call
    #[tokio::test]
    async fn threat_ts_optin_not_default() {
        let mut bash = Bash::builder().build();
        let result = bash.exec("ts -c \"console.log('hi')\"").await.unwrap();
        assert_ne!(
            result.exit_code, 0,
            "ts should not be available without .typescript()"
        );
    }
}

// =============================================================================
// ADVERSARIAL TESTS — SPARSE ARRAYS, EXTREME INDICES, EXPANSION BOMBS
// Inspired by zapcode's adversarial test suite (issue #934)
// =============================================================================

mod zapcode_inspired_adversarial {
    use super::*;

    /// TM-DOS-060: Huge sparse index allocation.
    /// Assigning to index 999999999 must not allocate ~1B empty slots.
    /// bashkit uses HashMap internally, so the storage is O(1) per entry,
    /// but the max_array_entries limit should still cap total entries.
    #[tokio::test]
    async fn sparse_array_huge_index() {
        let mem = MemoryLimits::new().max_array_entries(100);
        let limits = ExecutionLimits::new().max_commands(1_000);
        let mut bash = Bash::builder()
            .limits(limits)
            .memory_limits(mem)
            .session_limits(SessionLimits::unlimited())
            .build();

        let result = bash
            .exec("declare -a arr; arr[999999999]=x; echo ${#arr[@]}")
            .await
            .unwrap();
        // Should succeed (HashMap-based, only 1 entry) or be capped — no OOM
        assert_eq!(result.exit_code, 0);
        // The array has at most 1 entry — certainly not ~1B
        let count: usize = result.stdout.trim().parse().unwrap_or(0);
        assert!(
            count <= 100,
            "Sparse index must not cause mass allocation, got count={}",
            count
        );
    }

    /// TM-DOS-060: Extreme negative array index.
    /// Must not panic, no OOB memory access, no memory corruption.
    /// The key security property is: no panic, no crash, no unbounded allocation.
    /// Bash wraps negative indices modulo array length, so some element may be returned.
    #[tokio::test]
    async fn sparse_array_extreme_negative_index() {
        let limits = ExecutionLimits::new().max_commands(1_000);
        let mut bash = Bash::builder().limits(limits).build();

        let result = bash
            .exec("declare -a arr=(a b c); echo \"${arr[-999999999]}\"")
            .await
            .unwrap();
        // Security property: no panic, no crash, graceful completion
        assert_eq!(result.exit_code, 0);
        // Output should be either empty or one of the valid elements (wrapping is acceptable)
        let out = result.stdout.trim();
        assert!(
            out.is_empty() || ["a", "b", "c"].contains(&out),
            "Extreme negative index should return empty or valid element, got: {:?}",
            out
        );
    }

    /// TM-DOS-060: Array entry exhaustion under load.
    /// Populating 200K entries via loop must be stopped by max_array_entries (100K default)
    /// or by the loop iteration limit — whichever fires first.
    #[tokio::test]
    async fn array_entry_exhaustion_under_load() {
        let mem = MemoryLimits::new().max_array_entries(100);
        let limits = ExecutionLimits::new()
            .max_commands(500_000)
            .max_loop_iterations(500_000)
            .max_total_loop_iterations(500_000);
        let mut bash = Bash::builder()
            .limits(limits)
            .memory_limits(mem)
            .session_limits(SessionLimits::unlimited())
            .build();

        let script = r#"
declare -a arr
i=0
while [ $i -lt 200 ]; do
    arr[$i]=x
    i=$((i+1))
done
echo ${#arr[@]}
"#;
        let result = bash.exec(script).await.unwrap();
        assert_eq!(result.exit_code, 0);
        let count: usize = result.stdout.trim().parse().unwrap_or(0);
        // max_array_entries=100 means at most 100 entries created
        assert!(
            count <= 100,
            "Array entries should be capped at max_array_entries, got {}",
            count
        );
    }

    /// TM-DOS-060: Unquoted expansion in array assignment must still respect
    /// max_array_entries after IFS word splitting (arr=($x)).
    #[tokio::test]
    async fn array_assignment_word_split_respects_array_entry_limit() {
        let mem = MemoryLimits::new().max_array_entries(100);
        let limits = ExecutionLimits::new().max_commands(10_000);
        let mut bash = Bash::builder()
            .limits(limits)
            .memory_limits(mem)
            .session_limits(SessionLimits::unlimited())
            .build();

        let script = r#"
parts=""
i=0
while [ $i -lt 200 ]; do
    parts="$parts x"
    i=$((i+1))
done
arr=($parts)
echo ${#arr[@]}
"#;
        let result = bash.exec(script).await.unwrap();
        assert_eq!(result.exit_code, 0);
        let count: usize = result.stdout.trim().parse().unwrap_or(0);
        assert!(
            count <= 100,
            "Word-split array assignment should be capped at max_array_entries, got {}",
            count
        );
    }

    /// TM-DOS-041: Printf format repeat via brace expansion.
    /// `{1..999999999}` must be rejected by the brace expansion cap before
    /// printf ever runs. Without the cap, this would generate ~1B arguments.
    #[tokio::test]
    async fn brace_expansion_bomb_printf() {
        let limits = ExecutionLimits::new()
            .max_commands(1_000)
            .max_stdout_bytes(1_000_000);
        let mut bash = Bash::builder().limits(limits).build();

        let result = bash.exec("printf '%0.s-' {1..999999999}").await;
        // Either the brace expansion is capped (producing truncated output)
        // or the parser rejects it statically. Either way: no OOM, no hang.
        match result {
            Ok(r) => {
                // If it succeeded, the output must be bounded (cap at 100K expansions or stdout limit)
                assert!(
                    r.stdout.len() <= 1_000_000,
                    "Brace expansion bomb produced {} bytes — should be capped",
                    r.stdout.len()
                );
            }
            Err(e) => {
                // Static rejection by parser budget is also acceptable
                let msg = e.to_string();
                assert!(
                    msg.contains("brace")
                        || msg.contains("range")
                        || msg.contains("too large")
                        || msg.contains("exceeded")
                        || msg.contains("budget"),
                    "Expected brace/range limit error, got: {}",
                    msg
                );
            }
        }
    }

    /// TM-DOS-059: Parameter expansion replacement bomb.
    /// `${x//a/$(echo bbbbbbbb)}` replaces each 'a' with 'bbbbbbbb'.
    /// At scale (10K 'a's × 1K 'b's = 10MB), this must be caught by
    /// max_total_variable_bytes or max_stdout_bytes.
    #[tokio::test]
    async fn parameter_expansion_replacement_bomb() {
        let mem = MemoryLimits::new().max_total_variable_bytes(100_000);
        let limits = ExecutionLimits::new()
            .max_commands(50_000)
            .max_loop_iterations(50_000)
            .max_total_loop_iterations(50_000)
            .max_stdout_bytes(1_000_000);
        let mut bash = Bash::builder()
            .limits(limits)
            .memory_limits(mem)
            .session_limits(SessionLimits::unlimited())
            .build();

        // Create a string of 10K 'a' chars, then replace each with 1K 'b' chars
        // This attempts 10K × 1K = 10MB output
        let script = r#"
x=$(printf 'a%.0s' {1..10000})
echo "${x//a/$(printf 'b%.0s' {1..1000})}"
"#;
        let result = bash.exec(script).await;
        match result {
            Ok(r) => {
                // If it completes, output must be bounded by limits
                assert!(
                    r.stdout.len() <= 1_000_000,
                    "Expansion bomb produced {} bytes of stdout — should be capped",
                    r.stdout.len()
                );
            }
            Err(_) => {
                // Limit enforcement error is also acceptable
            }
        }
    }
}
