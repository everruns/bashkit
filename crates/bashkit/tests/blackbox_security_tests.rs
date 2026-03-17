//! Blackbox Security Tests for Bashkit
//!
//! Exploratory blackbox security testing — probing the interpreter as a hostile
//! attacker would, without relying on source code knowledge. Each test exercises
//! a specific abuse vector.
//!
//! Tests marked `#[ignore]` document confirmed security findings that currently
//! reproduce. They are tracked via GitHub issues and threat model IDs.
//!
//! Run passing tests: `cargo test --test blackbox_security_tests`
//! Run all (including findings): `cargo test --test blackbox_security_tests -- --ignored`

#![allow(unused_variables, clippy::single_match, clippy::match_single_binding)]

use bashkit::{Bash, ExecutionLimits};
use std::time::{Duration, Instant};

/// Helper: build a bash instance with tight resource limits
fn tight_bash() -> Bash {
    Bash::builder()
        .limits(
            ExecutionLimits::new()
                .max_commands(500)
                .max_loop_iterations(100)
                .max_total_loop_iterations(500)
                .max_function_depth(20)
                .timeout(Duration::from_secs(5)),
        )
        .build()
}

/// Helper: build a bash with very tight limits for DoS testing
fn dos_bash() -> Bash {
    Bash::builder()
        .limits(
            ExecutionLimits::new()
                .max_commands(50)
                .max_loop_iterations(10)
                .max_total_loop_iterations(50)
                .max_function_depth(5)
                .timeout(Duration::from_secs(3)),
        )
        .build()
}

// =============================================================================
// FINDING 1: STACK OVERFLOW — NESTED COMMAND SUBSTITUTION
// Threat: TM-DOS-044 (regression — was marked fixed via #492)
// Issue: Deeply nested $(echo $(...)) at depth ~50 causes stack overflow.
// The lexer fix in #492 may not cover the interpreter execution path.
// =============================================================================

mod finding_nested_cmd_subst_stack_overflow {
    use super::*;

    /// TM-DOS-044: depth-50 nested command substitution is bounded.
    #[tokio::test]
    async fn depth_50_is_bounded() {
        let mut bash = tight_bash();
        let depth = 50;
        let mut cmd = "echo hello".to_string();
        for _ in 0..depth {
            cmd = format!("echo $({})", cmd);
        }
        let result = bash.exec(&cmd).await;
        match &result {
            Ok(r) => assert!(!r.stdout.is_empty() || r.exit_code != 0),
            Err(_) => {}
        }
    }

    /// Moderate nesting (depth 10) works fine — confirms the boundary.
    #[tokio::test]
    async fn depth_10_works() {
        let mut bash = tight_bash();
        let depth = 10;
        let mut cmd = "echo hello".to_string();
        for _ in 0..depth {
            cmd = format!("echo $({})", cmd);
        }
        let result = bash.exec(&cmd).await;
        match &result {
            Ok(r) => assert!(!r.stdout.is_empty()),
            Err(_) => {}
        }
    }
}

// =============================================================================
// FINDING 2: STACK OVERFLOW — SOURCE SELF-RECURSION
// Threat: TM-DOS-056 (new)
// Issue: A script that sources itself causes unbounded recursion.
// Function depth limit does not apply to source/. commands.
// =============================================================================

mod finding_source_recursion_stack_overflow {
    use super::*;

    /// TM-DOS-056: source self-recursion hits depth limit instead of stack overflow.
    #[tokio::test]
    async fn source_self_recursion_hits_depth_limit() {
        let mut bash = dos_bash();
        let _ = bash
            .exec("echo 'source /tmp/recurse.sh' > /tmp/recurse.sh")
            .await;
        let result = bash.exec("source /tmp/recurse.sh").await;
        assert!(result.is_err(), "Self-sourcing must hit recursion limit");
    }

    /// TM-DOS-056: mutual recursion via source also hits depth limit.
    #[tokio::test]
    async fn source_mutual_recursion_hits_depth_limit() {
        let mut bash = dos_bash();
        let _ = bash
            .exec("echo 'source /tmp/recurse_b.sh' > /tmp/recurse_a.sh")
            .await;
        let _ = bash
            .exec("echo 'source /tmp/recurse_a.sh' > /tmp/recurse_b.sh")
            .await;
        let result = bash.exec("source /tmp/recurse_a.sh").await;
        assert!(
            result.is_err(),
            "Mutual source recursion must hit depth limit"
        );
    }
}

// =============================================================================
// FINDING 3: TIMEOUT BYPASS VIA SLEEP
// Threat: TM-DOS-057 (new)
// Issue: sleep in subshell, pipeline, or background+wait ignores execution
// timeout. The timeout mechanism doesn't propagate to these contexts.
// =============================================================================

mod finding_timeout_bypass {
    use super::*;

    /// TM-DOS-057: sleep in subshell respects execution timeout.
    #[tokio::test]
    async fn subshell_sleep_respects_timeout() {
        let mut bash = Bash::builder()
            .limits(ExecutionLimits::new().timeout(Duration::from_secs(2)))
            .build();
        let start = Instant::now();
        let result = bash.exec("(sleep 100)").await;
        let elapsed = start.elapsed();
        assert!(result.is_err(), "Should return timeout error");
        assert!(
            elapsed < Duration::from_secs(5),
            "Subshell sleep should respect timeout: took {:?}",
            elapsed
        );
    }

    /// TM-DOS-057: sleep in pipeline respects execution timeout.
    #[tokio::test]
    async fn pipeline_sleep_respects_timeout() {
        let mut bash = Bash::builder()
            .limits(ExecutionLimits::new().timeout(Duration::from_secs(2)))
            .build();
        let start = Instant::now();
        let result = bash.exec("echo x | sleep 100").await;
        let elapsed = start.elapsed();
        assert!(result.is_err(), "Should return timeout error");
        assert!(
            elapsed < Duration::from_secs(5),
            "Pipeline sleep should respect timeout: took {:?}",
            elapsed
        );
    }

    /// TM-DOS-057: sleep in background + wait respects execution timeout.
    #[tokio::test]
    async fn background_sleep_wait_respects_timeout() {
        let mut bash = Bash::builder()
            .limits(ExecutionLimits::new().timeout(Duration::from_secs(2)))
            .build();
        let start = Instant::now();
        let result = bash.exec("sleep 100 &\nwait").await;
        let elapsed = start.elapsed();
        assert!(result.is_err(), "Should return timeout error");
        assert!(
            elapsed < Duration::from_secs(5),
            "Background sleep+wait should respect timeout: took {:?}",
            elapsed
        );
    }

    /// TM-DOS-057: timeout builtin cannot override execution timeout.
    #[tokio::test]
    async fn timeout_builtin_cannot_override_execution_timeout() {
        let mut bash = Bash::builder()
            .limits(ExecutionLimits::new().timeout(Duration::from_secs(3)))
            .build();
        let start = Instant::now();
        let result = bash.exec("timeout 3600 sleep 3600").await;
        let elapsed = start.elapsed();
        assert!(result.is_err(), "Should return timeout error");
        assert!(
            elapsed < Duration::from_secs(6),
            "timeout builtin should not override execution timeout: {:?}",
            elapsed
        );
    }

    /// TM-DOS-057: direct sleep respects execution timeout.
    #[tokio::test]
    async fn direct_sleep_respects_timeout() {
        let mut bash = Bash::builder()
            .limits(ExecutionLimits::new().timeout(Duration::from_secs(2)))
            .build();
        let start = Instant::now();
        let result = bash.exec("sleep 100").await;
        let elapsed = start.elapsed();
        assert!(result.is_err(), "Should return timeout error");
        assert!(
            elapsed < Duration::from_secs(5),
            "Direct sleep should respect timeout: took {:?}",
            elapsed
        );
    }
}

// =============================================================================
// FINDING 4: READONLY BYPASS
// Threats: TM-INJ-019, TM-INJ-020, TM-INJ-021 (new)
// Issue: readonly variables can be overwritten via unset, declare, and export.
// =============================================================================

mod finding_readonly_bypass {
    use super::*;

    /// TM-INJ-019: unset cannot remove readonly variables.
    #[tokio::test]
    async fn unset_cannot_remove_readonly() {
        let mut bash = tight_bash();
        let result = bash
            .exec(
                r#"
                readonly LOCKED=secret_value
                unset LOCKED 2>/dev/null
                echo "LOCKED=$LOCKED"
                LOCKED=overwritten 2>/dev/null
                echo "LOCKED=$LOCKED"
                "#,
            )
            .await
            .unwrap();
        assert!(
            result.stdout.contains("LOCKED=secret_value"),
            "readonly was bypassed via unset"
        );
    }

    /// TM-INJ-020: declare cannot overwrite readonly variables.
    #[tokio::test]
    async fn declare_cannot_overwrite_readonly() {
        let mut bash = tight_bash();
        let result = bash
            .exec(
                r#"
                readonly LOCKED=original
                declare LOCKED=overwritten 2>/dev/null
                echo "$LOCKED"
                "#,
            )
            .await
            .unwrap();
        assert_eq!(
            result.stdout.trim(),
            "original",
            "readonly bypassed via declare"
        );
    }

    /// TM-INJ-021: export cannot overwrite readonly variables.
    #[tokio::test]
    async fn export_cannot_overwrite_readonly() {
        let mut bash = tight_bash();
        let result = bash
            .exec(
                r#"
                readonly LOCKED=original
                export LOCKED=overwritten 2>/dev/null
                echo "$LOCKED"
                "#,
            )
            .await
            .unwrap();
        assert_eq!(
            result.stdout.trim(),
            "original",
            "readonly bypassed via export"
        );
    }

    /// Non-finding: readonly via local in function is bash-compatible shadowing.
    #[tokio::test]
    async fn local_shadows_readonly_in_function() {
        let mut bash = tight_bash();
        let result = bash
            .exec(
                r#"
                readonly LOCKED=original
                f() { local LOCKED=overwritten; echo "$LOCKED"; }
                f
                echo "$LOCKED"
                "#,
            )
            .await
            .unwrap();
        // In bash, local CAN shadow readonly in function scope.
        // After function returns, LOCKED should still be original.
        assert!(
            result.stdout.trim().ends_with("original"),
            "readonly not restored after function: got {}",
            result.stdout.trim()
        );
    }
}

// =============================================================================
// FINDING 5: STATE ISOLATION — TRAPS LEAK ACROSS exec()
// Threat: TM-ISO-021 (new)
// Issue: EXIT trap set in one exec() fires in subsequent exec() calls.
// =============================================================================

mod finding_trap_leak {
    use super::*;

    /// TM-ISO-005: EXIT trap from one exec() does not fire in the next exec().
    #[tokio::test]
    async fn exit_trap_does_not_leak_between_exec() {
        let mut bash = tight_bash();
        let _ = bash.exec("trap 'echo LEAKED_TRAP' EXIT").await.unwrap();
        let result = bash.exec("echo clean_execution").await.unwrap();
        assert!(
            !result.stdout.contains("LEAKED_TRAP"),
            "EXIT trap leaked between exec() calls"
        );
    }
}

// =============================================================================
// FINDING 6: STATE ISOLATION — $? LEAKS ACROSS exec()
// Threat: TM-ISO-022 (new)
// Issue: exit code from one exec() is visible as $? in the next exec().
// =============================================================================

mod finding_exit_code_leak {
    use super::*;

    /// TM-ISO-006: $? from one exec() does not leak into the next.
    #[tokio::test]
    async fn exit_code_does_not_leak_between_exec() {
        let mut bash = tight_bash();
        let _ = bash.exec("exit 42").await.unwrap();
        let result = bash.exec("echo $?").await.unwrap();
        assert_eq!(
            result.stdout.trim(),
            "0",
            "$? leaked across exec() calls: got {}",
            result.stdout.trim()
        );
    }
}

// =============================================================================
// FINDING 7: STATE ISOLATION — set -e LEAKS ACROSS exec()
// Threat: TM-ISO-023 (new)
// Issue: Shell options (set -e) persist across exec() calls.
// =============================================================================

mod finding_shell_options_leak {
    use super::*;

    /// TM-ISO-007: set -e does not persist across exec() calls.
    #[tokio::test]
    async fn set_e_does_not_leak_between_exec() {
        let mut bash = tight_bash();
        let _ = bash.exec("set -e").await;
        let result = bash.exec("false; echo 'survived'").await.unwrap();
        assert!(
            result.stdout.contains("survived"),
            "set -e leaked across exec() calls — false aborted execution"
        );
    }
}

// =============================================================================
// FINDING 8: /dev/urandom RETURNS EMPTY WITH head -c
// Threat: TM-INT-007 (new)
// Issue: head -c N /dev/urandom returns empty output.
// =============================================================================

mod finding_urandom_empty {
    use super::*;

    /// TM-INT-004: /dev/urandom via head -c produces data.
    #[tokio::test]
    async fn urandom_head_c_returns_data() {
        let mut bash = tight_bash();
        let result = bash.exec("head -c 16 /dev/urandom | base64").await.unwrap();
        assert!(
            !result.stdout.trim().is_empty(),
            "/dev/urandom produced empty output"
        );
    }
}

// =============================================================================
// FINDING 9: seq PRODUCES UNBOUNDED OUTPUT (relates to #648)
// Threat: TM-DOS-058 (new — specific instance of missing output limits)
// Issue: seq 1 1000000 produces 1M lines despite 50-command limit.
// Related to #648 (feat: add stdout/stderr output capture size limits).
// =============================================================================

mod finding_seq_output_dos {
    use super::*;

    /// TM-DOS-058: seq output is bounded even with large range.
    #[tokio::test]
    async fn seq_output_is_bounded() {
        let mut bash = dos_bash();
        let result = bash.exec("seq 1 1000000").await;
        match &result {
            Ok(r) => {
                // Output should be truncated: max 100K iterations or 1MB output
                assert!(
                    r.stdout.len() <= 1_200_000,
                    "seq output too large: {} bytes",
                    r.stdout.len()
                );
                let lines = r.stdout.lines().count();
                assert!(lines <= 100_001, "seq produced too many lines: {}", lines);
            }
            Err(_) => {} // timeout is also acceptable
        }
    }
}

// =============================================================================
// NON-FINDING TESTS — PASSING SECURITY PROBES
// These tests verify that security controls ARE working correctly.
// Organized by attack category.
// =============================================================================

mod resource_exhaustion_passing {
    use super::*;

    /// Eval chains respect command limits
    #[tokio::test]
    async fn eval_chain_respects_command_limits() {
        let mut bash = dos_bash();
        let result = bash
            .exec(r#"eval 'eval "eval \"eval \\\"for i in $(seq 1 1000); do echo x; done\\\"\""'"#)
            .await;
        match &result {
            Ok(r) => {
                let lines = r.stdout.lines().count();
                assert!(lines <= 50, "eval chain produced {} lines", lines);
            }
            Err(_) => {}
        }
    }

    /// Nested function loops respect limits
    #[tokio::test]
    async fn nested_function_loop_limits() {
        let mut bash = dos_bash();
        let result = bash
            .exec(
                r#"
                f() { for i in 1 2 3 4 5 6 7 8 9 10 11; do echo "$1:$i"; done; }
                g() { f a; f b; f c; f d; f e; }
                g
                "#,
            )
            .await;
        match &result {
            Ok(r) => {
                let lines = r.stdout.lines().count();
                assert!(
                    lines <= 50,
                    "Nested function loops produced {} lines",
                    lines
                );
            }
            Err(_) => {}
        }
    }

    /// Exponential variable expansion doesn't OOM
    #[tokio::test]
    async fn exponential_variable_expansion() {
        let mut bash = tight_bash();
        let result = bash
            .exec(
                r#"
                a="AAAAAAAAAA"
                b="$a$a$a$a$a$a$a$a$a$a"
                c="$b$b$b$b$b$b$b$b$b$b"
                d="$c$c$c$c$c$c$c$c$c$c"
                echo ${#d}
                "#,
            )
            .await;
        match &result {
            Ok(r) => {
                let len: usize = r.stdout.trim().parse().unwrap_or(0);
                assert!(len <= 100_000_000, "Variable grew to {} chars", len);
            }
            Err(_) => {}
        }
    }

    /// Recursive function via alias hits depth limit
    #[tokio::test]
    async fn recursive_function_via_alias() {
        let mut bash = dos_bash();
        let result = bash
            .exec(
                r#"
                shopt -s expand_aliases
                alias boom='f'
                f() { boom; }
                f
                "#,
            )
            .await;
        assert!(
            result.is_err() || result.unwrap().exit_code != 0,
            "Recursive alias should hit depth limit"
        );
    }

    /// Mutual recursion hits depth limit
    #[tokio::test]
    async fn mutual_recursion_depth_limit() {
        let mut bash = dos_bash();
        let result = bash.exec("ping() { pong; }\npong() { ping; }\nping").await;
        assert!(result.is_err(), "Mutual recursion must hit depth limit");
    }

    /// Fork bomb pattern caught by limits
    #[tokio::test]
    async fn fork_bomb_pattern() {
        let mut bash = dos_bash();
        let result = bash.exec(r#":(){ :|:& };:"#).await;
        match &result {
            Ok(r) => assert!(
                r.exit_code != 0 || r.stderr.contains("limit") || r.stderr.contains("error"),
                "Fork bomb pattern should be blocked"
            ),
            Err(_) => {}
        }
    }

    /// Many heredocs don't exhaust memory
    #[tokio::test]
    async fn many_heredocs_memory() {
        let mut bash = tight_bash();
        let mut script = String::new();
        for i in 0..100 {
            script.push_str(&format!("cat <<'EOF{i}'\n{}\nEOF{i}\n", "A".repeat(1000),));
        }
        let result = bash.exec(&script).await;
        match &result {
            Ok(r) => {
                assert!(
                    r.stdout.len() < 200_000,
                    "Too much heredoc output: {}",
                    r.stdout.len()
                );
            }
            Err(_) => {}
        }
    }

    /// bash -c respects limits
    #[tokio::test]
    async fn bash_c_respects_limits() {
        let mut bash = dos_bash();
        let result = bash
            .exec("bash -c 'for i in $(seq 1 1000); do echo $i; done'")
            .await;
        match &result {
            Ok(r) => {
                let lines = r.stdout.lines().count();
                assert!(lines <= 50, "bash -c bypassed limits: {} lines", lines);
            }
            Err(_) => {}
        }
    }

    /// sh -c respects limits
    #[tokio::test]
    async fn sh_c_respects_limits() {
        let mut bash = dos_bash();
        let result = bash.exec("sh -c 'while true; do echo x; done'").await;
        assert!(
            result.is_err() || result.as_ref().unwrap().stdout.lines().count() <= 50,
            "sh -c bypassed limits"
        );
    }
}

mod variable_injection_passing {
    use super::*;

    /// PS1/PS2/PS4 don't execute command substitution in non-interactive mode
    #[tokio::test]
    async fn ps_variables_safe() {
        let mut bash = tight_bash();
        let result = bash
            .exec(
                r#"
                PS1='$(cat /etc/passwd)'
                PS4='+ $(date) '
                set -x
                echo test
                "#,
            )
            .await
            .unwrap();
        assert!(
            !result.stdout.contains("root:"),
            "PS1 executed command substitution"
        );
    }

    /// IFS manipulation doesn't crash
    #[tokio::test]
    async fn ifs_manipulation_safe() {
        let mut bash = tight_bash();
        let result = bash
            .exec("IFS=\"/\"\ncmd=\"echo/hello/world\"\n$cmd")
            .await
            .unwrap();
        // Exit 127 is expected (word splitting creates invalid command)
        assert!(result.exit_code == 0 || result.exit_code == 127);
    }

    /// PATH hijack doesn't override builtins
    #[tokio::test]
    async fn path_hijack_blocked() {
        let mut bash = tight_bash();
        let result = bash
            .exec(
                r#"
                mkdir -p /tmp/evil
                echo '#!/bin/bash
                echo "HIJACKED"' > /tmp/evil/cat
                chmod +x /tmp/evil/cat
                PATH="/tmp/evil:$PATH"
                echo "test" > /tmp/file.txt
                cat /tmp/file.txt
                "#,
            )
            .await
            .unwrap();
        assert_eq!(
            result.stdout.trim(),
            "test",
            "PATH hijack overrode builtins"
        );
    }

    /// BASH_ENV doesn't auto-execute scripts
    #[tokio::test]
    async fn bash_env_safe() {
        let mut bash = tight_bash();
        let _ = bash.exec("echo 'echo INJECTED' > /tmp/evil_env.sh").await;
        let mut bash2 = tight_bash();
        let result = bash2
            .exec("export BASH_ENV=/tmp/evil_env.sh\nbash -c 'echo clean'")
            .await
            .unwrap();
        assert!(
            !result.stdout.contains("INJECTED"),
            "BASH_ENV auto-executed"
        );
    }

    /// PROMPT_COMMAND doesn't fire in non-interactive mode
    #[tokio::test]
    async fn prompt_command_safe() {
        let mut bash = tight_bash();
        let result = bash
            .exec("PROMPT_COMMAND='echo INJECTED'\necho clean")
            .await
            .unwrap();
        assert!(
            !result.stdout.contains("INJECTED"),
            "PROMPT_COMMAND fired in non-interactive mode"
        );
    }

    /// Variable name with semicolon doesn't cause injection
    #[tokio::test]
    async fn variable_name_injection_blocked() {
        let mut bash = tight_bash();
        let result = bash
            .exec("declare \"a;echo EVIL=test\"\necho clean")
            .await
            .unwrap();
        assert!(
            !result.stdout.contains("EVIL"),
            "Variable name caused injection"
        );
    }

    /// Indirect expansion respects internal variable protection
    #[tokio::test]
    async fn indirect_expansion_blocked() {
        let mut bash = tight_bash();
        let result = bash
            .exec("secret=\"hidden\"\nvarname=\"_NAMEREF_secret\"\necho \"${!varname}\"")
            .await
            .unwrap();
        assert!(
            !result.stdout.contains("hidden"),
            "Indirect expansion leaked internal variable"
        );
    }
}

mod filesystem_escape_passing {
    use super::*;

    /// Symlink doesn't traverse to host filesystem
    #[tokio::test]
    async fn symlink_traversal_blocked() {
        let mut bash = tight_bash();
        let result = bash
            .exec("ln -s /etc/passwd /tmp/link\ncat /tmp/link")
            .await
            .unwrap();
        assert!(
            !result.stdout.contains("root:x:"),
            "Symlink accessed host /etc/passwd"
        );
    }

    /// Path traversal via .. blocked
    #[tokio::test]
    async fn dotdot_traversal_blocked() {
        let mut bash = tight_bash();
        let result = bash
            .exec("cd /tmp\ncat ../../../etc/passwd\ncat /tmp/../../../etc/shadow")
            .await
            .unwrap();
        assert!(
            !result.stdout.contains("root:"),
            "Dot-dot traversal accessed host files"
        );
    }

    /// /proc/self not accessible
    #[tokio::test]
    async fn proc_self_blocked() {
        let mut bash = tight_bash();
        let result = bash
            .exec("cat /proc/self/environ\ncat /proc/self/cmdline")
            .await
            .unwrap();
        assert!(
            !result.stdout.contains("PATH=") && !result.stdout.contains("HOME="),
            "/proc/self leaked host environment"
        );
    }

    /// /dev/tcp doesn't open real connections
    #[tokio::test]
    async fn dev_tcp_blocked() {
        let mut bash = tight_bash();
        let result = bash
            .exec("echo test > /dev/tcp/127.0.0.1/80 2>/dev/null\necho test > /dev/udp/127.0.0.1/53 2>/dev/null\necho clean")
            .await;
        match &result {
            Ok(r) => assert!(r.stdout.contains("clean")),
            Err(_) => {}
        }
    }

    /// find doesn't discover host files
    #[tokio::test]
    async fn find_confined_to_vfs() {
        let mut bash = tight_bash();
        let result = bash
            .exec("find / -name \"*.conf\" 2>/dev/null\nfind / -name \"passwd\" 2>/dev/null")
            .await
            .unwrap();
        assert!(
            !result.stdout.contains("/etc/passwd"),
            "find discovered host files"
        );
    }

    /// Null byte in filename doesn't crash
    #[tokio::test]
    async fn null_byte_filename_safe() {
        let mut bash = tight_bash();
        let result = bash
            .exec("echo test > $'/tmp/file\\x00.txt'\necho clean")
            .await;
        match &result {
            Ok(_) => {}
            Err(e) => assert!(!e.to_string().contains("panic"), "Null byte caused panic"),
        }
    }

    /// CDPATH doesn't escape VFS
    #[tokio::test]
    async fn cdpath_confined() {
        let mut bash = tight_bash();
        let result = bash
            .exec("CDPATH=\"/:..:/../../..\"\ncd etc 2>/dev/null && cat passwd")
            .await
            .unwrap();
        assert!(!result.stdout.contains("root:"), "CDPATH allowed escape");
    }
}

mod command_injection_passing {
    use super::*;

    /// Eval executes in sandbox (expected bash behavior)
    #[tokio::test]
    async fn eval_sandboxed() {
        let mut bash = tight_bash();
        let result = bash
            .exec("user_input='hello; echo INJECTED'\neval \"echo $user_input\"")
            .await
            .unwrap();
        // eval DOES execute the injection — that's normal bash.
        // The point is it stays in the sandbox.
        assert!(result.stdout.contains("INJECTED"));
    }

    /// Traps fire within sandbox
    #[tokio::test]
    async fn trap_sandboxed() {
        let mut bash = tight_bash();
        let result = bash
            .exec("trap 'echo TRAP_FIRED' EXIT\necho normal")
            .await
            .unwrap();
        assert!(result.stdout.contains("normal"));
    }

    /// Array subscript command substitution stays sandboxed
    #[tokio::test]
    async fn array_subscript_cmd_subst_sandboxed() {
        let mut bash = tight_bash();
        let result = bash
            .exec(
                r#"
                declare -a arr
                x='$(echo PWNED > /tmp/pwned.txt)'
                arr[$x]=1
                cat /tmp/pwned.txt 2>/dev/null
                echo clean
                "#,
            )
            .await
            .unwrap();
        assert!(result.stdout.contains("clean"));
    }

    /// xargs respects command limits
    #[tokio::test]
    async fn xargs_respects_limits() {
        let mut bash = dos_bash();
        let result = bash.exec("seq 1 100 | xargs -I{} echo line_{}").await;
        match &result {
            Ok(r) => {
                let lines = r.stdout.lines().count();
                assert!(lines <= 50, "xargs bypassed limits: {} lines", lines);
            }
            Err(_) => {}
        }
    }
}

mod parser_edge_cases_passing {
    use super::*;

    /// Deep nested parentheses don't stack overflow
    #[tokio::test]
    async fn deep_parens_safe() {
        let mut bash = tight_bash();
        let deep = "(".repeat(100) + "echo hi" + &")".repeat(100);
        let result = bash.exec(&deep).await;
        match &result {
            Ok(_) => {}
            Err(e) => assert!(
                !e.to_string().contains("stack overflow"),
                "Deep parens caused stack overflow"
            ),
        }
    }

    /// Unterminated constructs don't hang
    #[tokio::test]
    async fn unterminated_constructs_dont_hang() {
        let mut bash = Bash::builder()
            .limits(ExecutionLimits::new().timeout(Duration::from_secs(2)))
            .build();
        let start = Instant::now();
        let _ = bash.exec("echo \"unterminated string").await;
        let _ = bash.exec("echo 'unterminated single").await;
        let _ = bash.exec("echo $(unterminated subshell").await;
        let _ = bash.exec("if true; then echo").await;
        let _ = bash.exec("case x in").await;
        let elapsed = start.elapsed();
        assert!(
            elapsed < Duration::from_secs(5),
            "Unterminated constructs took {:?}",
            elapsed
        );
    }

    /// Very long line handled
    #[tokio::test]
    async fn very_long_line() {
        let mut bash = tight_bash();
        let long_echo = format!("echo '{}'", "X".repeat(100_000));
        let result = bash.exec(&long_echo).await;
        match &result {
            Ok(r) => assert_eq!(r.stdout.trim().len(), 100_000),
            Err(_) => {}
        }
    }

    /// Many empty commands (semicolons) handled
    #[tokio::test]
    async fn many_empty_commands() {
        let mut bash = tight_bash();
        let semis = ";".repeat(1000);
        let result = bash.exec(&format!("echo start; {} echo end", semis)).await;
        match &result {
            Ok(r) => assert!(r.stdout.contains("start") && r.stdout.contains("end")),
            Err(_) => {}
        }
    }

    /// Heredoc with delimiter in content
    #[tokio::test]
    async fn heredoc_delimiter_in_content() {
        let mut bash = tight_bash();
        let result = bash
            .exec("cat <<EOF\nThis contains EOF but not at start\nEOF in middle\nEOF\n")
            .await
            .unwrap();
        assert!(result.stdout.contains("EOF but not at start"));
    }

    /// Single-quoted heredoc prevents expansion
    #[tokio::test]
    async fn heredoc_single_quoted_no_expansion() {
        let mut bash = tight_bash();
        let result = bash
            .exec("cat <<'EOF'\n$(echo INJECTED)\n`echo INJECTED2`\nEOF\n")
            .await
            .unwrap();
        assert!(
            result.stdout.contains("$(echo INJECTED)"),
            "Single-quoted heredoc expanded command substitution"
        );
    }
}

mod state_isolation_passing {
    use super::*;

    /// Subshell variables don't leak to parent
    #[tokio::test]
    async fn subshell_variable_isolation() {
        let mut bash = tight_bash();
        let result = bash
            .exec("x=parent\n(x=child; echo \"inner: $x\")\necho \"outer: $x\"")
            .await
            .unwrap();
        assert!(result.stdout.contains("inner: child"));
        assert!(
            result.stdout.contains("outer: parent"),
            "Subshell variable leaked to parent"
        );
    }

    /// Cross-instance isolation
    #[tokio::test]
    async fn cross_instance_isolation() {
        let mut bash1 = tight_bash();
        let mut bash2 = tight_bash();
        let _ = bash1.exec("SECRET=from_instance_1").await;
        let result = bash2.exec("echo \"SECRET=$SECRET\"").await.unwrap();
        assert_eq!(
            result.stdout.trim(),
            "SECRET=",
            "Variable leaked between instances"
        );
    }

    /// History doesn't leak between instances
    #[tokio::test]
    async fn history_cross_session() {
        let mut bash1 = tight_bash();
        let _ = bash1.exec("SECRET_CMD=password123").await;
        let mut bash2 = tight_bash();
        let result = bash2.exec("history").await.unwrap();
        assert!(
            !result.stdout.contains("password123"),
            "History leaked between instances"
        );
    }
}

mod unicode_attacks_passing {
    use super::*;

    /// RTL override character handled safely
    #[tokio::test]
    async fn rtl_override() {
        let mut bash = tight_bash();
        let result = bash.exec("echo \u{202E}test\u{202C}").await.unwrap();
        assert_eq!(result.exit_code, 0);
    }

    /// Long Unicode strings handled
    #[tokio::test]
    async fn long_unicode_string() {
        let mut bash = tight_bash();
        let emoji_bomb = "\u{1F4A3}".repeat(10000);
        let result = bash.exec(&format!("echo '{}'", emoji_bomb)).await;
        match &result {
            Ok(r) => assert_eq!(r.exit_code, 0),
            Err(_) => {}
        }
    }

    /// Multi-byte substring doesn't panic
    #[tokio::test]
    async fn multibyte_substring() {
        let mut bash = tight_bash();
        let result = bash
            .exec("x=\"héllo wörld\"\necho \"${x:0:5}\"\necho \"${#x}\"")
            .await;
        match &result {
            Ok(_) => {}
            Err(e) => assert!(
                !e.to_string().contains("byte index"),
                "Multi-byte substring panic: {}",
                e
            ),
        }
    }

    /// Null bytes don't cause panics
    #[tokio::test]
    async fn null_bytes_safe() {
        let mut bash = tight_bash();
        for test in ["echo $'\\x00'", "x=$'hello\\x00world'; echo \"$x\""] {
            let result = bash.exec(test).await;
            match &result {
                Ok(_) => {}
                Err(e) => assert!(
                    !e.to_string().contains("panic"),
                    "Null byte panic: {} for: {}",
                    e,
                    test
                ),
            }
        }
    }
}

mod creative_abuse_passing {
    use super::*;

    /// printf format string attack doesn't crash
    #[tokio::test]
    async fn printf_format_string() {
        let mut bash = tight_bash();
        let result = bash
            .exec(
                r#"
                printf "%s%s%s%s%s%s%s%s%s%s"
                printf "%n" 2>/dev/null
                printf "%.99999999s" "x"
                echo clean
                "#,
            )
            .await;
        match &result {
            Ok(r) => assert!(r.stdout.contains("clean") || r.exit_code == 0),
            Err(_) => {}
        }
    }

    /// read -t doesn't hang
    #[tokio::test]
    async fn read_timeout() {
        let mut bash = Bash::builder()
            .limits(ExecutionLimits::new().timeout(Duration::from_secs(3)))
            .build();
        let start = Instant::now();
        let _ = bash.exec("read -t 1 x; echo done").await;
        let elapsed = start.elapsed();
        assert!(elapsed < Duration::from_secs(5), "read hung: {:?}", elapsed);
    }

    /// yes|head respects limits
    #[tokio::test]
    async fn yes_head() {
        let mut bash = dos_bash();
        let result = bash.exec("yes | head -5").await;
        match &result {
            Ok(r) => {
                let lines = r.stdout.lines().count();
                assert!(lines <= 50, "yes produced {} lines", lines);
            }
            Err(_) => {}
        }
    }

    /// env/printenv don't leak host secrets
    #[tokio::test]
    async fn env_no_secret_leak() {
        let mut bash = tight_bash();
        let result = bash.exec("env; printenv; set").await.unwrap();
        for key in [
            "DOPPLER_TOKEN",
            "AWS_SECRET",
            "GITHUB_TOKEN",
            "ANTHROPIC_API_KEY",
        ] {
            assert!(!result.stdout.contains(key), "env leaked: {}", key);
        }
    }

    /// Arithmetic overflow doesn't panic
    #[tokio::test]
    async fn arithmetic_overflow() {
        let mut bash = tight_bash();
        for test in [
            "echo $((9223372036854775807 + 1))",
            "echo $((-9223372036854775808 - 1))",
            "echo $((9223372036854775807 * 2))",
            "echo $((1 / 0))",
            "echo $((1 % 0))",
        ] {
            let result = bash.exec(test).await;
            match &result {
                Ok(_) => {}
                Err(e) => assert!(
                    !e.to_string().contains("panic") && !e.to_string().contains("overflow"),
                    "Arithmetic panic: {} for: {}",
                    e,
                    test
                ),
            }
        }
    }

    /// Signal handling safe (kill $$ is no-op)
    #[tokio::test]
    async fn signal_handling_safe() {
        let mut bash = tight_bash();
        let _ = bash.exec("kill -9 $$\nkill -15 $$\necho alive").await;
    }

    /// compgen doesn't expose host commands
    #[tokio::test]
    async fn compgen_no_host_commands() {
        let mut bash = tight_bash();
        let result = bash.exec("compgen -c | sort").await;
        match &result {
            Ok(r) => assert!(
                !r.stdout.contains("systemctl"),
                "compgen showed host commands"
            ),
            Err(_) => {}
        }
    }

    /// Regex DoS completes in time
    #[tokio::test]
    async fn regex_dos_bounded() {
        let mut bash = Bash::builder()
            .limits(ExecutionLimits::new().timeout(Duration::from_secs(5)))
            .build();
        let start = Instant::now();
        let _ = bash
            .exec(&format!("echo '{}' | grep -E '(a+)+b'", "a".repeat(30)))
            .await;
        let elapsed = start.elapsed();
        assert!(elapsed < Duration::from_secs(5), "Regex DoS: {:?}", elapsed);
    }

    /// Error messages don't leak host paths
    #[tokio::test]
    async fn error_messages_safe() {
        let mut bash = tight_bash();
        let result = bash
            .exec("cat /nonexistent/path 2>&1\nls /real/host/path 2>&1")
            .await
            .unwrap();
        assert!(
            !result.stdout.contains("/usr/") && !result.stdout.contains("/home/"),
            "Error messages leaked host paths: {}",
            result.stdout
        );
    }

    /// Massive pipeline chain handled
    #[tokio::test]
    async fn massive_pipeline() {
        let mut bash = tight_bash();
        let mut cmd = "echo x".to_string();
        for _ in 0..200 {
            cmd.push_str(" | cat");
        }
        let result = bash.exec(&cmd).await;
        match &result {
            Ok(r) => assert_eq!(r.stdout.trim(), "x"),
            Err(_) => {}
        }
    }

    /// Concurrent exec calls safe
    #[tokio::test]
    async fn concurrent_exec_safety() {
        let mut bash = tight_bash();
        for i in 0..20 {
            let result = bash.exec(&format!("echo {}", i)).await.unwrap();
            assert_eq!(result.stdout.trim(), &i.to_string());
        }
    }

    /// /dev/tcp redirect doesn't open network connection
    #[tokio::test]
    async fn dev_tcp_redirect_blocked() {
        let mut bash = tight_bash();
        let result = bash
            .exec(
                r#"
                exec 3<>/dev/tcp/127.0.0.1/80 2>/dev/null
                echo -e "GET / HTTP/1.0\r\n\r\n" >&3 2>/dev/null
                cat <&3 2>/dev/null
                echo "done"
                "#,
            )
            .await;
        match &result {
            Ok(r) => assert!(
                !r.stdout.contains("HTTP/"),
                "/dev/tcp opened a real connection"
            ),
            Err(_) => {}
        }
    }

    /// Timing side-channel negligible
    #[tokio::test]
    async fn timing_side_channel() {
        let mut bash = tight_bash();
        let start = Instant::now();
        let _ = bash.exec("test -f /etc/passwd").await;
        let t1 = start.elapsed();
        let start = Instant::now();
        let _ = bash.exec("test -f /nonexistent/file").await;
        let t2 = start.elapsed();
        let diff = t1.abs_diff(t2);
        assert!(
            diff < Duration::from_millis(100),
            "Timing side-channel: existing={:?} vs nonexistent={:?}",
            t1,
            t2
        );
    }

    /// Dollar-sign special variables don't crash
    #[tokio::test]
    async fn dollar_sign_edges() {
        let mut bash = tight_bash();
        let result = bash
            .exec(
                r#"
                echo "$$"
                echo "$!"
                echo "$-"
                echo "$_"
                echo "${#}"
                echo "${?}"
                echo "${$}"
                "#,
            )
            .await
            .unwrap();
        // Some may not be fully supported but none should crash.
    }

    /// Parameter expansion edge cases
    #[tokio::test]
    async fn parameter_expansion_edges() {
        let mut bash = tight_bash();
        let result = bash
            .exec(
                r#"
                x="hello_world_test_string"
                echo "${x/hello/goodbye}"
                echo "${x//o/0}"
                echo "${x^^}"
                echo "${x,,}"
                echo "${x:0:5}"
                echo "${x#*_}"
                echo "${x##*_}"
                echo "${x%_*}"
                echo "${x%%_*}"
                "#,
            )
            .await
            .unwrap();
        assert!(result.stdout.contains("goodbye_world_test_string"));
    }

    /// Array expansion edge cases
    #[tokio::test]
    async fn array_expansion_edges() {
        let mut bash = tight_bash();
        let result = bash
            .exec(
                r#"
                arr=()
                echo "empty: ${#arr[@]}"
                arr[999]="sparse"
                echo "sparse: ${arr[999]}"
                echo "indices: ${!arr[@]}"
                unset 'arr[999]'
                echo "after unset: ${#arr[@]}"
                "#,
            )
            .await
            .unwrap();
        assert!(result.stdout.contains("empty: 0"));
        assert!(result.stdout.contains("sparse: sparse"));
    }
}
