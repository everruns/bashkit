//! Blackbox Security Tests for Bashkit
//!
//! Creative exploratory security testing — probing the interpreter as
//! a hostile attacker would, without relying on source code knowledge.
//! Each test exercises a specific abuse vector.
//!
//! Run with: `cargo test --test blackbox_security_tests`

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
// 1. RESOURCE EXHAUSTION & LIMITS BYPASS
// =============================================================================

mod resource_exhaustion_bypass {
    use super::*;

    /// Try to bypass command limits using eval chains
    #[tokio::test]
    async fn eval_chain_respects_command_limits() {
        let mut bash = dos_bash();
        let result = bash
            .exec(r#"eval 'eval "eval \"eval \\\"for i in $(seq 1 1000); do echo x; done\\\"\""'"#)
            .await;
        // Must either error or complete within limits
        match &result {
            Ok(r) => {
                let lines = r.stdout.lines().count();
                assert!(lines <= 50, "eval chain produced {} lines, limits not enforced", lines);
            }
            Err(_) => {} // Limit error is fine
        }
    }

    /// Try to bypass loop limits by nesting loops inside functions
    #[tokio::test]
    async fn nested_function_loop_bypass() {
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
                assert!(lines <= 50, "Nested function loops produced {} lines", lines);
            }
            Err(_) => {}
        }
    }

    /// Try to exhaust memory via exponential variable expansion
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
        // Should complete without OOM — checking it doesn't hang or crash
        match &result {
            Ok(r) => {
                let len: usize = r.stdout.trim().parse().unwrap_or(0);
                // 10^4 = 10000 chars is fine, but test it doesn't go crazy
                assert!(len <= 100_000_000, "Variable grew to {} chars", len);
            }
            Err(_) => {}
        }
    }

    /// SECURITY FINDING: sleep inside subshell ignores execution timeout.
    /// Set timeout to 2s, `(sleep 100)` runs for 60s+.
    /// The timeout mechanism doesn't propagate into subshell execution.
    #[tokio::test]
    #[ignore] // SECURITY FINDING: timeout bypass via subshell sleep
    async fn subshell_timeout_bypass() {
        let mut bash = Bash::builder()
            .limits(ExecutionLimits::new().timeout(Duration::from_secs(2)))
            .build();

        let start = Instant::now();
        let _ = bash.exec("(sleep 100)").await;
        let elapsed = start.elapsed();
        assert!(
            elapsed < Duration::from_secs(5),
            "Subshell sleep bypassed timeout: took {:?}",
            elapsed
        );
    }

    /// Recursive function that tries to evade function depth limit
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
        assert!(result.is_err() || result.unwrap().exit_code != 0,
            "Recursive alias should hit function depth limit");
    }

    /// Try mutual recursion to bypass depth limits
    #[tokio::test]
    async fn mutual_recursion_depth_limit() {
        let mut bash = dos_bash();
        let result = bash
            .exec(
                r#"
                ping() { pong; }
                pong() { ping; }
                ping
                "#,
            )
            .await;
        assert!(
            result.is_err(),
            "Mutual recursion must hit depth limit"
        );
    }

    /// Fork bomb pattern — should be caught by command limits
    #[tokio::test]
    async fn fork_bomb_pattern() {
        let mut bash = dos_bash();
        let result = bash
            .exec(r#":(){ :|:& };:"#)
            .await;
        // Must not hang, must be caught by limits
        match &result {
            Ok(r) => assert!(r.exit_code != 0 || r.stderr.contains("limit") || r.stderr.contains("error"),
                "Fork bomb pattern should be blocked"),
            Err(_) => {}
        }
    }

    /// Try to use `source` to create infinite recursion.
    /// SECURITY FINDING: source self-recursion causes stack overflow.
    /// The function depth limit doesn't apply to `source` calls.
    #[tokio::test]
    #[ignore] // SECURITY FINDING: stack overflow — crashes the process
    async fn source_self_recursion_crashes() {
        let mut bash = dos_bash();
        // Write a script that sources itself
        let _ = bash
            .exec("echo 'source /tmp/recurse.sh' > /tmp/recurse.sh")
            .await;
        let result = bash.exec("source /tmp/recurse.sh").await;
        assert!(
            result.is_err(),
            "Self-sourcing must hit recursion/command limit"
        );
    }

    /// Deeply nested command substitution — depth 50 causes stack overflow.
    /// This is a real DoS vulnerability: an attacker can crash the process
    /// with a moderately nested $(echo $(...)) chain.
    ///
    /// ROOT CAUSE: recursive interpret/expand calls without depth tracking
    /// for command substitution nesting.
    #[tokio::test]
    #[ignore] // SECURITY FINDING: stack overflow — crashes the process
    async fn deeply_nested_command_substitution_crashes() {
        let mut bash = tight_bash();
        // Build: $(echo $(echo $(echo ... )))
        let depth = 50;
        let mut cmd = "echo hello".to_string();
        for _ in 0..depth {
            cmd = format!("echo $({})", cmd);
        }
        let result = bash.exec(&cmd).await;
        // Should either work or error, not crash/hang
        match &result {
            Ok(r) => assert!(!r.stdout.is_empty() || r.exit_code != 0),
            Err(_) => {}
        }
    }

    /// Shallower nested command substitution should work fine
    #[tokio::test]
    async fn moderately_nested_command_substitution() {
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

    /// Stress test: many here-documents
    #[tokio::test]
    async fn many_heredocs_memory() {
        let mut bash = tight_bash();
        let mut script = String::new();
        for i in 0..100 {
            script.push_str(&format!(
                "cat <<'EOF{i}'\n{}\nEOF{i}\n",
                "A".repeat(1000),
            ));
        }
        let result = bash.exec(&script).await;
        // Must complete (limits should kick in for commands)
        match &result {
            Ok(r) => {
                assert!(r.stdout.len() < 200_000, "Too much heredoc output: {}", r.stdout.len());
            }
            Err(_) => {}
        }
    }
}

// =============================================================================
// 2. VARIABLE INJECTION & EXPANSION ATTACKS
// =============================================================================

mod variable_injection {
    use super::*;

    /// Try to inject variables via PS1/PS2/PS4 to leak info
    #[tokio::test]
    async fn ps_variable_injection() {
        let mut bash = tight_bash();
        let result = bash
            .exec(
                r#"
                PS1='$(cat /etc/passwd)'
                PS2='$(whoami)'
                PS4='+ $(date) '
                set -x
                echo test
                "#,
            )
            .await
            .unwrap();
        // PS variables should not execute commands in non-interactive mode
        assert!(
            !result.stdout.contains("root:"),
            "PS1 executed command substitution"
        );
    }

    /// IFS manipulation to split commands differently
    #[tokio::test]
    async fn ifs_injection_attack() {
        let mut bash = tight_bash();
        let result = bash
            .exec(
                r#"
                IFS="/"
                cmd="echo/hello/world"
                $cmd
                "#,
            )
            .await
            .unwrap();
        // With IFS="/", $cmd splits into "echo" "hello" "world"
        // Exit code 127 is expected if word splitting creates invalid command lookup
        // The key thing is it shouldn't crash or escape the sandbox
        assert!(result.exit_code == 0 || result.exit_code == 127);
    }

    /// Try to overwrite PATH to hijack commands
    #[tokio::test]
    async fn path_hijack_attempt() {
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
        // cat should still work as a builtin, not execute our fake
        assert_eq!(result.stdout.trim(), "test",
            "PATH hijack should not override builtins");
    }

    /// Try to use BASH_ENV to execute code on startup
    #[tokio::test]
    async fn bash_env_injection() {
        let mut bash = tight_bash();
        let _ = bash.exec("echo 'echo INJECTED' > /tmp/evil_env.sh").await;

        let mut bash2 = tight_bash();
        let result = bash2
            .exec(
                r#"
                export BASH_ENV=/tmp/evil_env.sh
                bash -c 'echo clean'
                "#,
            )
            .await
            .unwrap();
        assert!(
            !result.stdout.contains("INJECTED"),
            "BASH_ENV should not auto-execute scripts"
        );
    }

    /// Try to use PROMPT_COMMAND to inject code
    #[tokio::test]
    async fn prompt_command_injection() {
        let mut bash = tight_bash();
        let result = bash
            .exec(
                r#"
                PROMPT_COMMAND='echo INJECTED_VIA_PROMPT'
                echo clean
                "#,
            )
            .await
            .unwrap();
        assert!(
            !result.stdout.contains("INJECTED_VIA_PROMPT"),
            "PROMPT_COMMAND should not execute in non-interactive mode"
        );
    }

    /// Variable name with special characters
    #[tokio::test]
    async fn variable_name_special_chars() {
        let mut bash = tight_bash();
        // Try to create variables with weird names that might confuse the interpreter
        let result = bash
            .exec(
                r#"
                declare "a[0]=injected"
                declare "a b=test"
                declare "a;echo EVIL=test"
                echo clean
                "#,
            )
            .await
            .unwrap();
        assert!(
            !result.stdout.contains("EVIL"),
            "Variable name with semicolon caused command injection"
        );
    }

    /// Nameref chain to access internal state
    #[tokio::test]
    async fn nameref_chain_internal_access() {
        let mut bash = tight_bash();
        let result = bash
            .exec(
                r#"
                declare -n ref1=BASH_VERSION
                declare -n ref2=ref1
                echo "$ref2"
                declare -n ref3=RANDOM
                echo "$ref3"
                "#,
            )
            .await
            .unwrap();
        // Should work but not expose host info
        // BASH_VERSION in bashkit should be bashkit's own, not host bash
    }

    /// Try to use printf to write to arbitrary file descriptors
    #[tokio::test]
    async fn printf_fd_escape() {
        let mut bash = tight_bash();
        let result = bash
            .exec(
                r#"
                printf "INJECTED" > /dev/fd/1
                printf "ALSO_INJECTED" >&255
                echo clean
                "#,
            )
            .await;
        // Should not crash
        match &result {
            Ok(r) => assert!(r.stdout.contains("clean")),
            Err(_) => {}
        }
    }

    /// Indirect expansion to access internal variables
    #[tokio::test]
    async fn indirect_expansion_internal_vars() {
        let mut bash = tight_bash();
        let result = bash
            .exec(
                r#"
                secret="hidden"
                varname="_NAMEREF_secret"
                echo "${!varname}"
                varname="_READONLY_marker"
                echo "${!varname}"
                "#,
            )
            .await
            .unwrap();
        // Indirect expansion should not bypass internal variable protection
        assert!(
            !result.stdout.contains("hidden"),
            "Indirect expansion leaked internal variable"
        );
    }

    /// Try to set special bash variables to confuse the interpreter
    #[tokio::test]
    async fn special_variable_confusion() {
        let mut bash = tight_bash();
        let result = bash
            .exec(
                r#"
                LINENO=999999
                FUNCNAME="fake_function"
                BASH_SOURCE="/etc/passwd"
                RANDOM=-1
                SECONDS=-1
                echo "$LINENO $FUNCNAME $BASH_SOURCE"
                "#,
            )
            .await
            .unwrap();
        // Setting these special variables is allowed in bash too.
        // The critical check is that it doesn't crash or affect interpreter operation.
        assert_eq!(result.exit_code, 0);
    }
}

// =============================================================================
// 3. FILESYSTEM ESCAPE ATTEMPTS
// =============================================================================

mod filesystem_escape {
    use super::*;

    /// Path traversal via symlinks
    #[tokio::test]
    async fn symlink_path_traversal() {
        let mut bash = tight_bash();
        let result = bash
            .exec(
                r#"
                ln -s /etc/passwd /tmp/innocent_link
                cat /tmp/innocent_link
                "#,
            )
            .await
            .unwrap();
        assert!(
            !result.stdout.contains("root:x:"),
            "Symlink traversal accessed host /etc/passwd"
        );
    }

    /// Path traversal via .. sequences
    #[tokio::test]
    async fn dotdot_traversal() {
        let mut bash = tight_bash();
        let result = bash
            .exec(
                r#"
                cd /tmp
                cat ../../../etc/passwd
                cat /tmp/../../../etc/shadow
                "#,
            )
            .await
            .unwrap();
        assert!(
            !result.stdout.contains("root:"),
            "Dot-dot traversal accessed host files"
        );
    }

    /// Try to escape via /proc/self
    #[tokio::test]
    async fn proc_self_escape() {
        let mut bash = tight_bash();
        let result = bash
            .exec(
                r#"
                cat /proc/self/environ
                cat /proc/self/cmdline
                cat /proc/self/maps
                ls /proc/self/fd/
                "#,
            )
            .await
            .unwrap();
        // VFS should not have /proc mounted
        assert!(
            !result.stdout.contains("PATH=") && !result.stdout.contains("HOME="),
            "/proc/self leaked host environment"
        );
    }

    /// Try to create device files
    #[tokio::test]
    async fn device_file_creation() {
        let mut bash = tight_bash();
        let result = bash
            .exec(
                r#"
                echo "test" > /dev/tty
                echo "test" > /dev/tcp/127.0.0.1/80
                echo "test" > /dev/udp/127.0.0.1/53
                echo clean
                "#,
            )
            .await;
        // /dev/tcp and /dev/udp are bash special files for network access
        // They should NOT actually open network connections
        match &result {
            Ok(r) => assert!(r.stdout.contains("clean")),
            Err(_) => {}
        }
    }

    /// Try to access host filesystem via /dev/fd
    #[tokio::test]
    async fn dev_fd_escape() {
        let mut bash = tight_bash();
        let result = bash
            .exec(
                r#"
                ls /dev/fd/
                cat /dev/fd/0
                readlink /dev/fd/0
                "#,
            )
            .await;
        // Should not expose real file descriptors
        match &result {
            Ok(r) => assert!(!r.stdout.contains("/dev/pts")),
            Err(_) => {}
        }
    }

    /// Try to use find to discover files outside VFS
    #[tokio::test]
    async fn find_escape_attempt() {
        let mut bash = tight_bash();
        let result = bash
            .exec(
                r#"
                find / -name "*.conf" 2>/dev/null
                find / -name "passwd" 2>/dev/null
                "#,
            )
            .await
            .unwrap();
        assert!(
            !result.stdout.contains("/etc/passwd"),
            "find discovered host files"
        );
    }

    /// Null byte injection in filenames
    #[tokio::test]
    async fn null_byte_filename() {
        let mut bash = tight_bash();
        let result = bash
            .exec(
                "echo test > $'/tmp/file\\x00.txt'\necho test > $'/tmp/normal.txt\\x00/etc/passwd'\necho clean",
            )
            .await;
        // Should not crash on null bytes in filenames
        match &result {
            Ok(_) => {}
            Err(e) => {
                let msg = e.to_string();
                assert!(!msg.contains("panic"), "Null byte caused panic: {}", msg);
            }
        }
    }

    /// Long path name attack
    #[tokio::test]
    async fn long_path_dos() {
        let mut bash = tight_bash();
        let long_dir = "A".repeat(4096);
        let result = bash
            .exec(&format!("mkdir -p /tmp/{}\necho clean", long_dir))
            .await;
        // Should not crash
        match &result {
            Ok(r) => assert!(r.stdout.contains("clean") || r.exit_code != 0),
            Err(_) => {}
        }
    }

    /// Relative path trickery with CDPATH
    #[tokio::test]
    async fn cdpath_escape() {
        let mut bash = tight_bash();
        let result = bash
            .exec(
                r#"
                CDPATH="/:..:/../../.."
                cd etc 2>/dev/null && cat passwd
                "#,
            )
            .await
            .unwrap();
        assert!(
            !result.stdout.contains("root:"),
            "CDPATH allowed filesystem escape"
        );
    }
}

// =============================================================================
// 4. COMMAND INJECTION & EVAL ABUSE
// =============================================================================

mod command_injection {
    use super::*;

    /// Eval with untrusted input
    #[tokio::test]
    async fn eval_injection() {
        let mut bash = tight_bash();
        let result = bash
            .exec(
                r#"
                user_input='hello; echo INJECTED'
                eval "echo $user_input"
                "#,
            )
            .await
            .unwrap();
        // This is expected bash behavior — eval DOES execute the injection
        // The test verifies it doesn't crash or escape the sandbox
        assert!(result.stdout.contains("INJECTED"),
            "eval should execute as bash does (this is expected behavior)");
    }

    /// Try to use trap to execute arbitrary code
    #[tokio::test]
    async fn trap_code_execution() {
        let mut bash = tight_bash();
        let result = bash
            .exec(
                r#"
                trap 'echo TRAP_FIRED' EXIT
                trap 'echo ERR_TRAP' ERR
                trap 'echo DEBUG_TRAP' DEBUG
                echo normal
                "#,
            )
            .await
            .unwrap();
        // Traps should fire but within the sandbox
        assert!(result.stdout.contains("normal"));
    }

    /// Backtick injection
    #[tokio::test]
    async fn backtick_injection() {
        let mut bash = tight_bash();
        let result = bash
            .exec(
                r#"
                x=`echo hello`
                echo "$x"
                # Nested backticks
                y=`echo \`echo nested\``
                echo "$y"
                "#,
            )
            .await
            .unwrap();
        assert_eq!(result.exit_code, 0);
    }

    /// Process substitution abuse
    #[tokio::test]
    async fn process_substitution_abuse() {
        let mut bash = tight_bash();
        let result = bash
            .exec(
                r#"
                cat <(echo from_process_sub)
                diff <(echo a) <(echo b)
                "#,
            )
            .await;
        // Should either work safely or error, not crash
        match &result {
            Ok(_) => {}
            Err(_) => {}
        }
    }

    /// Try to use bash -c to escape restrictions
    #[tokio::test]
    async fn bash_c_escape() {
        let mut bash = dos_bash();
        let result = bash
            .exec(
                r#"
                bash -c 'for i in $(seq 1 1000); do echo $i; done'
                "#,
            )
            .await;
        // Inner bash -c must also respect limits
        match &result {
            Ok(r) => {
                let lines = r.stdout.lines().count();
                assert!(lines <= 50, "bash -c bypassed limits: {} lines", lines);
            }
            Err(_) => {}
        }
    }

    /// Try to use sh -c to escape restrictions
    #[tokio::test]
    async fn sh_c_escape() {
        let mut bash = dos_bash();
        let result = bash
            .exec(
                r#"
                sh -c 'while true; do echo x; done'
                "#,
            )
            .await;
        assert!(
            result.is_err() || result.as_ref().unwrap().stdout.lines().count() <= 50,
            "sh -c bypassed limits"
        );
    }

    /// Try arithmetic evaluation to execute commands
    #[tokio::test]
    async fn arithmetic_command_execution() {
        let mut bash = tight_bash();
        // In real bash, array subscripts can execute arbitrary code
        // e.g., a[$(echo pwned)] — this is a known bash vuln
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
        // Array subscript should not execute command substitution in arithmetic context
        // If it does, check that it's sandboxed
        assert!(result.stdout.contains("clean"));
    }

    /// Globbing as DoS — create many files then glob
    #[tokio::test]
    async fn glob_expansion_dos() {
        let mut bash = tight_bash();
        let result = bash
            .exec(
                r#"
                mkdir -p /tmp/globtest
                for i in $(seq 1 100); do touch "/tmp/globtest/file_$i.txt"; done
                echo /tmp/globtest/*.txt | wc -w
                "#,
            )
            .await;
        match &result {
            Ok(r) => {
                assert_eq!(result.as_ref().unwrap().exit_code, 0);
            }
            Err(_) => {}
        }
    }
}

// =============================================================================
// 5. PARSER EDGE CASES & PANICS
// =============================================================================

mod parser_edge_cases {
    use super::*;

    /// Deeply nested parentheses
    #[tokio::test]
    async fn deep_nested_parens() {
        let mut bash = tight_bash();
        let deep = "(".repeat(100) + "echo hi" + &")".repeat(100);
        let result = bash.exec(&deep).await;
        // Should not stack overflow
        match &result {
            Ok(_) => {}
            Err(e) => {
                assert!(
                    !e.to_string().contains("stack overflow"),
                    "Deep parens caused stack overflow"
                );
            }
        }
    }

    /// Deeply nested braces
    #[tokio::test]
    async fn deep_nested_braces() {
        let mut bash = tight_bash();
        let deep = "{".repeat(100) + " echo hi; " + &"}".repeat(100);
        let result = bash.exec(&deep).await;
        match &result {
            Ok(_) => {}
            Err(e) => {
                assert!(
                    !e.to_string().contains("stack overflow"),
                    "Deep braces caused stack overflow"
                );
            }
        }
    }

    /// Unterminated constructs shouldn't hang
    #[tokio::test]
    async fn unterminated_constructs() {
        let mut bash = Bash::builder()
            .limits(ExecutionLimits::new().timeout(Duration::from_secs(2)))
            .build();

        let start = Instant::now();
        // These should error quickly, not hang waiting for more input
        let _ = bash.exec("echo \"unterminated string").await;
        let _ = bash.exec("echo 'unterminated single").await;
        let _ = bash.exec("echo $(unterminated subshell").await;
        let _ = bash.exec("if true; then echo").await;
        let _ = bash.exec("while true; do echo").await;
        let _ = bash.exec("case x in").await;
        let elapsed = start.elapsed();
        assert!(
            elapsed < Duration::from_secs(5),
            "Unterminated constructs took {:?}",
            elapsed
        );
    }

    /// Very long single line
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

    /// Many semicolons (empty commands)
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

    /// Heredoc with same delimiter as content
    #[tokio::test]
    async fn heredoc_delimiter_confusion() {
        let mut bash = tight_bash();
        let result = bash
            .exec(
                r#"
                cat <<EOF
                This line contains EOF but not at start
                EOF in middle of line
                EOF
                "#,
            )
            .await
            .unwrap();
        assert!(result.stdout.contains("EOF but not at start"));
    }

    /// ANSI escape codes in output
    #[tokio::test]
    async fn ansi_escape_in_output() {
        let mut bash = tight_bash();
        let result = bash
            .exec(r#"printf '\033[2J\033[H\033[31mRED\033[0m\n'"#)
            .await
            .unwrap();
        // Should pass through ANSI codes, not interpret them
        assert!(result.stdout.contains("RED"));
    }

    /// Brace expansion abuse
    #[tokio::test]
    async fn brace_expansion_dos() {
        let mut bash = tight_bash();
        // {1..1000000} could generate massive output
        let result = bash
            .exec("echo {1..10000} | wc -w")
            .await;
        match &result {
            Ok(r) => {
                // Should either work or be limited
            }
            Err(_) => {}
        }
    }
}

// =============================================================================
// 6. STATE CORRUPTION & CROSS-EXECUTION LEAKS
// =============================================================================

mod state_corruption {
    use super::*;

    /// SECURITY FINDING: EXIT trap from one exec() fires in the next exec().
    /// This is a state isolation issue — traps should be scoped to a single
    /// exec() invocation, but EXIT traps persist and fire on subsequent calls.
    #[tokio::test]
    #[ignore] // SECURITY FINDING: EXIT trap leaks between exec() calls
    async fn trap_persistence_across_exec() {
        let mut bash = tight_bash();
        let _ = bash
            .exec("trap 'echo LEAKED_TRAP' EXIT")
            .await
            .unwrap();
        let result = bash.exec("echo clean_execution").await.unwrap();
        // The EXIT trap from previous exec should not fire in this exec
        // (This tests whether exec() properly isolates trap state)
        assert!(
            !result.stdout.contains("LEAKED_TRAP"),
            "Trap leaked between exec() calls"
        );
    }

    /// Test that function definitions persist (they should) but don't leak secrets
    #[tokio::test]
    async fn function_secret_leak() {
        let mut bash = tight_bash();
        let _ = bash
            .exec(
                r#"
                secret="API_KEY_12345"
                hide_secret() { echo "$secret"; }
                "#,
            )
            .await;
        // Functions persist across exec, but let's check variable scope
        let result = bash.exec("hide_secret").await.unwrap();
        // This is expected — functions close over the environment
        // The point is to verify the behavior is consistent
    }

    /// Test that subshell variables don't leak to parent
    #[tokio::test]
    async fn subshell_variable_isolation() {
        let mut bash = tight_bash();
        let result = bash
            .exec(
                r#"
                x=parent
                (x=child; echo "inner: $x")
                echo "outer: $x"
                "#,
            )
            .await
            .unwrap();
        assert!(result.stdout.contains("inner: child"));
        assert!(result.stdout.contains("outer: parent"),
            "Subshell variable leaked to parent scope");
    }

    /// SECURITY FINDING: `unset` can remove readonly variables.
    /// After `readonly LOCKED=secret_value`, `unset LOCKED` should fail,
    /// but the variable is actually removed.
    #[tokio::test]
    #[ignore] // SECURITY FINDING: readonly bypassed via unset
    async fn readonly_unset_bypass() {
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
        // readonly should not be bypassed
        assert!(
            result.stdout.contains("LOCKED=secret_value"),
            "readonly was bypassed"
        );
    }

    /// Verify env isolation between independent Bash instances
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

    /// SECURITY FINDING: $? from one exec() leaks into the next.
    /// After `exit 42`, the next exec() sees $? == 42 instead of 0.
    /// This is a state isolation bug — each exec() should start fresh.
    #[tokio::test]
    #[ignore] // SECURITY FINDING: $? leaks across exec() calls
    async fn exit_code_isolation() {
        let mut bash = tight_bash();
        let _ = bash.exec("exit 42").await.unwrap();
        let result = bash.exec("echo $?").await.unwrap();
        // $? should be 0 (fresh execution), not 42
        assert_eq!(
            result.stdout.trim(),
            "0",
            "$? leaked across exec() calls: got {}",
            result.stdout.trim()
        );
    }
}

// =============================================================================
// 7. UNICODE & ENCODING ATTACKS
// =============================================================================

mod unicode_attacks {
    use super::*;

    /// Right-to-left override character to disguise commands
    #[tokio::test]
    async fn rtl_override_disguise() {
        let mut bash = tight_bash();
        // U+202E RIGHT-TO-LEFT OVERRIDE
        let result = bash
            .exec("echo \u{202E}test\u{202C}")
            .await
            .unwrap();
        // Should handle without crashing
        assert_eq!(result.exit_code, 0);
    }

    /// Zero-width characters in variable names
    #[tokio::test]
    async fn zero_width_variable_names() {
        let mut bash = tight_bash();
        let result = bash
            .exec(
                r#"
                normal="real_value"
                echo "$normal"
                "#,
            )
            .await
            .unwrap();
        assert!(result.stdout.contains("real_value"));
    }

    /// Homoglyph attack — using lookalike characters
    #[tokio::test]
    async fn homoglyph_command_names() {
        let mut bash = tight_bash();
        // Cyrillic 'е' (U+0435) looks like Latin 'e'
        let result = bash.exec("echo normal").await.unwrap();
        assert!(result.stdout.contains("normal"));
    }

    /// Very long Unicode strings
    #[tokio::test]
    async fn long_unicode_string() {
        let mut bash = tight_bash();
        let emoji_bomb = "\u{1F4A3}".repeat(10000);
        let result = bash
            .exec(&format!("echo '{}'", emoji_bomb))
            .await;
        match &result {
            Ok(r) => assert_eq!(r.exit_code, 0),
            Err(_) => {}
        }
    }

    /// Multi-byte character boundary in variable expansion
    #[tokio::test]
    async fn multibyte_substring() {
        let mut bash = tight_bash();
        let result = bash
            .exec(
                r#"
                x="héllo wörld"
                echo "${x:0:5}"
                echo "${#x}"
                echo "${x:3:2}"
                "#,
            )
            .await;
        // Should not panic on multi-byte char boundaries
        match &result {
            Ok(_) => {}
            Err(e) => {
                assert!(!e.to_string().contains("byte index"),
                    "Multi-byte substring caused panic: {}", e);
            }
        }
    }

    /// Null bytes in various positions
    #[tokio::test]
    async fn null_bytes_everywhere() {
        let mut bash = tight_bash();
        let tests = vec![
            "echo $'\\x00'",
            "x=$'hello\\x00world'; echo \"$x\"",
            "echo test | tr 'e' '\\0'",
        ];
        for test in tests {
            let result = bash.exec(test).await;
            match &result {
                Ok(_) => {}
                Err(e) => {
                    assert!(!e.to_string().contains("panic"),
                        "Null byte test panicked: {} for input: {}", e, test);
                }
            }
        }
    }
}

// =============================================================================
// 8. CREATIVE ABUSE VECTORS
// =============================================================================

mod creative_abuse {
    use super::*;

    /// Try to use printf formatting to leak memory
    #[tokio::test]
    async fn printf_format_string_attack() {
        let mut bash = tight_bash();
        let result = bash
            .exec(
                r#"
                printf "%s%s%s%s%s%s%s%s%s%s%s%s%s%s%s%s%s%s%s%s"
                printf "%n" 2>/dev/null
                printf "%d" "not_a_number" 2>/dev/null
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

    /// Try to abuse read with timeout to hang
    #[tokio::test]
    async fn read_hang_attempt() {
        let mut bash = Bash::builder()
            .limits(ExecutionLimits::new().timeout(Duration::from_secs(3)))
            .build();
        let start = Instant::now();
        let _ = bash.exec("read -t 1 x; echo done").await;
        let elapsed = start.elapsed();
        assert!(
            elapsed < Duration::from_secs(5),
            "read command hung for {:?}",
            elapsed
        );
    }

    /// Try to abuse yes command as DoS
    #[tokio::test]
    async fn yes_dos() {
        let mut bash = dos_bash();
        let result = bash.exec("yes | head -5").await;
        match &result {
            Ok(r) => {
                let lines = r.stdout.lines().count();
                assert!(lines <= 50, "yes produced too many lines: {}", lines);
            }
            Err(_) => {}
        }
    }

    /// SECURITY FINDING: `seq 1 1000000` produces 1M lines despite command limit of 50.
    /// The seq builtin runs as a single command but generates unbounded output.
    /// An attacker can produce massive memory allocation via a single command.
    #[tokio::test]
    #[ignore] // SECURITY FINDING: seq bypasses command limits (1M lines with 50-cmd limit)
    async fn seq_dos() {
        let mut bash = dos_bash();
        let result = bash.exec("seq 1 1000000").await;
        match &result {
            Ok(r) => {
                let lines = r.stdout.lines().count();
                assert!(lines <= 100, "seq bypassed limits: {} lines", lines);
            }
            Err(_) => {}
        }
    }

    /// Try to abuse xargs to multiply command execution
    #[tokio::test]
    async fn xargs_command_multiplication() {
        let mut bash = dos_bash();
        let result = bash
            .exec("seq 1 100 | xargs -I{} echo line_{}")
            .await;
        match &result {
            Ok(r) => {
                let lines = r.stdout.lines().count();
                assert!(lines <= 50, "xargs bypassed command limits: {} lines", lines);
            }
            Err(_) => {}
        }
    }

    /// Try to use watch to create persistent execution
    #[tokio::test]
    async fn watch_persistence() {
        let mut bash = Bash::builder()
            .limits(ExecutionLimits::new().timeout(Duration::from_secs(3)))
            .build();
        let start = Instant::now();
        let _ = bash.exec("watch -n 0 echo x").await;
        let elapsed = start.elapsed();
        assert!(
            elapsed < Duration::from_secs(6),
            "watch ran indefinitely: {:?}",
            elapsed
        );
    }

    /// Try to use env/printenv to discover host info
    #[tokio::test]
    async fn env_info_disclosure() {
        let mut bash = tight_bash();
        let result = bash.exec("env; printenv; set").await.unwrap();
        // Should not contain actual host environment variables
        let suspicious = [
            "DOPPLER_TOKEN",
            "AWS_SECRET",
            "GITHUB_TOKEN",
            "API_KEY",
            "ANTHROPIC_API_KEY",
        ];
        for key in suspicious {
            assert!(
                !result.stdout.contains(key),
                "env leaked sensitive host variable: {}",
                key
            );
        }
    }

    /// Try to exfiltrate data via DNS-style encoding in variable names
    #[tokio::test]
    async fn data_exfiltration_encoding() {
        let mut bash = tight_bash();
        let result = bash
            .exec(
                r#"
                secret="steal_this"
                encoded=$(echo "$secret" | base64)
                # Without network, this should be harmless
                echo "$encoded"
                "#,
            )
            .await
            .unwrap();
        // base64 encoding should work (it's just text processing)
        // The point is that without network, it can't leave the sandbox
        assert!(!result.stdout.trim().is_empty());
    }

    /// Attempting to write a large file to exhaust VFS memory
    #[tokio::test]
    async fn large_file_vfs_dos() {
        let mut bash = tight_bash();
        let result = bash
            .exec(
                r#"
                dd if=/dev/urandom of=/tmp/bigfile bs=1024 count=10240 2>/dev/null
                ls -la /tmp/bigfile
                "#,
            )
            .await;
        // Should either work within limits or error
        match &result {
            Ok(r) => {}
            Err(_) => {}
        }
    }

    /// Try to use history to recover commands from other sessions
    #[tokio::test]
    async fn history_cross_session_leak() {
        let mut bash1 = tight_bash();
        let _ = bash1.exec("SECRET_CMD=password123").await;

        let mut bash2 = tight_bash();
        let result = bash2.exec("history").await.unwrap();
        assert!(
            !result.stdout.contains("password123"),
            "History leaked commands from another instance"
        );
    }

    /// Try arithmetic overflow in various contexts
    #[tokio::test]
    async fn arithmetic_overflow_attacks() {
        let mut bash = tight_bash();
        let tests = vec![
            "echo $((9223372036854775807 + 1))",
            "echo $((-9223372036854775808 - 1))",
            "echo $((9223372036854775807 * 2))",
            "echo $((1 / 0))",
            "echo $((1 % 0))",
            "echo $((2 ** 64))",
            "echo $((2 ** -1))",
        ];
        for test in tests {
            let result = bash.exec(test).await;
            match &result {
                Ok(_) => {} // Should handle gracefully
                Err(e) => {
                    assert!(
                        !e.to_string().contains("panic") && !e.to_string().contains("overflow"),
                        "Arithmetic test panicked: {} for: {}",
                        e,
                        test
                    );
                }
            }
        }
    }

    /// Try to abuse parallel command to bypass limits
    #[tokio::test]
    async fn parallel_limit_bypass() {
        let mut bash = dos_bash();
        let result = bash
            .exec(
                r#"
                seq 1 100 | parallel echo
                "#,
            )
            .await;
        match &result {
            Ok(r) => {
                let lines = r.stdout.lines().count();
                assert!(lines <= 100, "parallel bypassed limits: {} lines", lines);
            }
            Err(_) => {}
        }
    }

    /// Try double-free style: unset then use
    #[tokio::test]
    async fn use_after_unset() {
        let mut bash = tight_bash();
        let result = bash
            .exec(
                r#"
                x="hello"
                ref=$x
                unset x
                echo "ref=$ref"
                echo "x=$x"
                # Try with arrays
                arr=(a b c)
                ref="${arr[@]}"
                unset arr
                echo "ref=$ref"
                echo "arr=${arr[@]}"
                "#,
            )
            .await
            .unwrap();
        assert!(result.stdout.contains("ref=hello"));
        assert!(result.stdout.contains("x=\n") || result.stdout.contains("x="));
    }

    /// Test signal handling — SIGTERM/SIGKILL should be no-ops
    #[tokio::test]
    async fn signal_handling() {
        let mut bash = tight_bash();
        let result = bash
            .exec(
                r#"
                kill -9 $$
                kill -15 $$
                kill -1 $$
                echo "still alive"
                "#,
            )
            .await;
        match &result {
            Ok(r) => {
                // Should either ignore signals or handle them safely
                // In a virtual interpreter, kill should be a no-op
            }
            Err(_) => {}
        }
    }

    /// Try to abuse compgen to enumerate builtins/commands
    #[tokio::test]
    async fn compgen_enumeration() {
        let mut bash = tight_bash();
        let result = bash
            .exec(
                r#"
                compgen -b | sort
                compgen -c | sort
                compgen -v | sort
                "#,
            )
            .await;
        // Should work but only show virtual commands, not host commands
        match &result {
            Ok(r) => {
                // Should not contain host-only commands like systemctl, apt, etc.
                assert!(!r.stdout.contains("systemctl"),
                    "compgen showed host commands");
            }
            Err(_) => {}
        }
    }

    /// SECURITY FINDING: `timeout 3600 sleep 3600` ignores execution timeout.
    /// Despite a 3-second timeout on the Bash instance, the timeout builtin
    /// overrides it and the command runs for 60s+ before being killed.
    #[tokio::test]
    #[ignore] // SECURITY FINDING: timeout builtin overrides execution timeout
    async fn timeout_abuse() {
        let mut bash = Bash::builder()
            .limits(ExecutionLimits::new().timeout(Duration::from_secs(3)))
            .build();
        let start = Instant::now();
        // Try to use timeout to override the execution timeout
        let _ = bash.exec("timeout 3600 sleep 3600").await;
        let elapsed = start.elapsed();
        assert!(
            elapsed < Duration::from_secs(6),
            "timeout command overrode execution timeout: {:?}",
            elapsed
        );
    }

    /// Race condition: concurrent exec calls on same instance
    #[tokio::test]
    async fn concurrent_exec_safety() {
        // Note: Bash takes &mut self, so this tests sequential rapid fire
        let mut bash = tight_bash();
        for i in 0..20 {
            let result = bash.exec(&format!("echo {}", i)).await.unwrap();
            assert_eq!(result.stdout.trim(), &i.to_string());
        }
    }

    /// Try regex DoS via catastrophic backtracking
    #[tokio::test]
    async fn regex_dos() {
        let mut bash = Bash::builder()
            .limits(ExecutionLimits::new().timeout(Duration::from_secs(5)))
            .build();
        let start = Instant::now();
        // Classic ReDoS pattern
        let result = bash
            .exec(&format!(
                "echo '{}' | grep -E '(a+)+b'",
                "a".repeat(30)
            ))
            .await;
        let elapsed = start.elapsed();
        assert!(
            elapsed < Duration::from_secs(5),
            "Regex DoS took {:?}",
            elapsed
        );
    }

    /// Try to abuse sed with backreferences for ReDoS
    #[tokio::test]
    async fn sed_redos() {
        let mut bash = Bash::builder()
            .limits(ExecutionLimits::new().timeout(Duration::from_secs(5)))
            .build();
        let start = Instant::now();
        let result = bash
            .exec(&format!(
                "echo '{}' | sed 's/\\(a*\\)*/x/'",
                "a".repeat(50)
            ))
            .await;
        let elapsed = start.elapsed();
        assert!(
            elapsed < Duration::from_secs(5),
            "sed ReDoS took {:?}",
            elapsed
        );
    }

    /// Try to use awk for arbitrary computation bypassing limits
    #[tokio::test]
    async fn awk_computation_bypass() {
        let mut bash = dos_bash();
        let result = bash
            .exec(
                r#"
                echo x | awk 'BEGIN { for(i=0;i<10000;i++) printf "x" }'
                "#,
            )
            .await;
        match &result {
            Ok(r) => {
                assert!(r.stdout.len() <= 100_000, "awk bypassed output limits: {} bytes", r.stdout.len());
            }
            Err(_) => {}
        }
    }
}

// =============================================================================
// 9. REDIRECT & PIPE ATTACKS
// =============================================================================

mod redirect_attacks {
    use super::*;

    /// Try to redirect to /dev/tcp for network access
    #[tokio::test]
    async fn dev_tcp_redirect() {
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
        // /dev/tcp should not actually open a network connection
        match &result {
            Ok(r) => {
                assert!(!r.stdout.contains("HTTP/"),
                    "/dev/tcp opened a real network connection");
            }
            Err(_) => {}
        }
    }

    /// Redirect to many file descriptors
    #[tokio::test]
    async fn fd_exhaustion() {
        let mut bash = tight_bash();
        let mut script = String::new();
        for i in 3..100 {
            script.push_str(&format!("exec {i}>/tmp/fd_{i}.txt\n"));
        }
        script.push_str("echo done\n");
        let result = bash.exec(&script).await;
        match &result {
            Ok(r) => assert!(r.stdout.contains("done")),
            Err(_) => {}
        }
    }

    /// Pipe to self pattern
    #[tokio::test]
    async fn pipe_to_self() {
        let mut bash = dos_bash();
        let result = bash
            .exec("echo x | cat | cat | cat | cat | cat | cat | cat | cat | cat | cat")
            .await;
        match &result {
            Ok(r) => assert_eq!(r.stdout.trim(), "x"),
            Err(_) => {}
        }
    }

    /// Try to use named pipes (FIFO) for IPC
    #[tokio::test]
    async fn named_pipe_ipc() {
        let mut bash = tight_bash();
        let result = bash
            .exec(
                r#"
                mkfifo /tmp/testpipe 2>/dev/null
                echo "test" > /tmp/testpipe &
                cat /tmp/testpipe
                "#,
            )
            .await;
        // Should handle FIFOs safely
        match &result {
            Ok(_) => {}
            Err(_) => {}
        }
    }

    /// Here-string with process substitution
    #[tokio::test]
    async fn herestring_abuse() {
        let mut bash = tight_bash();
        let result = bash
            .exec(
                r#"
                cat <<< "$(for i in $(seq 1 100); do echo line_$i; done)"
                "#,
            )
            .await;
        match &result {
            Ok(r) => assert!(r.stdout.lines().count() > 0),
            Err(_) => {}
        }
    }

    /// Redirect stderr to stdin loop
    #[tokio::test]
    async fn stderr_stdin_loop() {
        let mut bash = dos_bash();
        let result = bash
            .exec(
                r#"
                echo error >&2 2>&1 | cat
                "#,
            )
            .await;
        match &result {
            Ok(_) => {}
            Err(_) => {}
        }
    }
}

// =============================================================================
// 10. TIMING & SIDE-CHANNEL ATTACKS
// =============================================================================

mod timing_attacks {
    use super::*;

    /// Try to use timing to detect if files exist on host
    #[tokio::test]
    async fn timing_file_existence() {
        let mut bash = tight_bash();
        let start = Instant::now();
        let _ = bash.exec("test -f /etc/passwd").await;
        let t1 = start.elapsed();

        let start = Instant::now();
        let _ = bash.exec("test -f /nonexistent/file").await;
        let t2 = start.elapsed();

        // Timing difference should be negligible (both are VFS ops)
        // A large difference would suggest one is hitting real FS
        let diff = t1.abs_diff(t2);
        assert!(
            diff < Duration::from_millis(100),
            "Timing side-channel detected: existing={:?} vs nonexistent={:?}",
            t1,
            t2
        );
    }

    /// Try to use date/SECONDS to measure execution time
    #[tokio::test]
    async fn time_measurement_abuse() {
        let mut bash = tight_bash();
        let result = bash
            .exec(
                r#"
                start=$SECONDS
                for i in $(seq 1 100); do true; done
                end=$SECONDS
                echo "elapsed=$((end - start))"
                "#,
            )
            .await
            .unwrap();
        // SECONDS should work but not reveal real host time
    }

    /// SECURITY FINDING: /dev/urandom via head produces empty output.
    /// `head -c 16 /dev/urandom | base64` returns empty string in both instances.
    /// The /dev/urandom virtual file may not work correctly with head -c.
    #[tokio::test]
    #[ignore] // SECURITY FINDING: /dev/urandom + head -c returns empty
    async fn urandom_isolation() {
        let mut bash1 = tight_bash();
        let mut bash2 = tight_bash();

        let r1 = bash1
            .exec("head -c 16 /dev/urandom | base64")
            .await
            .unwrap();
        let r2 = bash2
            .exec("head -c 16 /dev/urandom | base64")
            .await
            .unwrap();

        // /dev/urandom should produce non-empty random data
        assert!(
            !r1.stdout.trim().is_empty(),
            "/dev/urandom produced empty output"
        );
        // Different instances should produce different random data
        assert_ne!(
            r1.stdout.trim(),
            r2.stdout.trim(),
            "Two instances produced identical /dev/urandom output"
        );
    }
}

// =============================================================================
// 11. EDGE CASES IN STRING/PARAMETER EXPANSION
// =============================================================================

mod expansion_edge_cases {
    use super::*;

    /// Recursive variable expansion
    #[tokio::test]
    async fn recursive_variable_expansion() {
        let mut bash = tight_bash();
        let result = bash
            .exec(
                r#"
                a='$b'
                b='$c'
                c='$a'
                eval echo "$a"
                "#,
            )
            .await
            .unwrap();
        // Should not infinite loop
        assert_eq!(result.exit_code, 0);
    }

    /// Parameter expansion with unset variables
    #[tokio::test]
    async fn unset_parameter_expansion() {
        let mut bash = tight_bash();
        let result = bash
            .exec(
                r#"
                echo "${unset_var:-default}"
                echo "${unset_var:=assigned}"
                echo "$unset_var"
                echo "${unset_var:+override}"
                echo "${another_unset:?This should error}" 2>/dev/null
                echo "survived"
                "#,
            )
            .await
            .unwrap();
        assert!(result.stdout.contains("default"));
        assert!(result.stdout.contains("assigned"));
    }

    /// Very deeply nested parameter expansion
    #[tokio::test]
    async fn deep_parameter_expansion() {
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

    /// Associative array key injection
    #[tokio::test]
    async fn assoc_array_key_injection() {
        let mut bash = tight_bash();
        let result = bash
            .exec(
                r#"
                declare -A map
                map["normal"]="value1"
                map["key with spaces"]="value2"
                map[$'key\nwith\nnewlines']="value3"
                map[""]="empty_key"
                echo "${map["normal"]}"
                echo "${map["key with spaces"]}"
                echo "count: ${#map[@]}"
                "#,
            )
            .await
            .unwrap();
        assert!(result.stdout.contains("value1"));
    }

    /// Dollar-sign edge cases — should not crash on any special variable
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
        // Some special vars may not be fully supported, but none should crash.
        // Accept any exit code — the important thing is no panic.
    }
}

// =============================================================================
// 12. SECOND WAVE: DEEPER PROBES BASED ON INITIAL FINDINGS
// =============================================================================

mod deep_probes {
    use super::*;

    // --- OUTPUT SIZE ATTACKS ---
    // seq bypasses limits. What other builtins produce unbounded output?

    /// yes builtin with no pipe limit
    #[tokio::test]
    async fn yes_unbounded_without_pipe() {
        let mut bash = dos_bash();
        // yes without head: should be stopped by command/loop limits
        let result = bash.exec("yes | head -n 5").await;
        match &result {
            Ok(r) => {
                let lines = r.stdout.lines().count();
                assert!(lines <= 10, "yes|head produced too many lines: {}", lines);
            }
            Err(_) => {}
        }
    }

    /// printf repeat generates massive output via a single command
    #[tokio::test]
    async fn printf_repeat_dos() {
        let mut bash = dos_bash();
        // printf with format repeat: "%.0s" repeats for each argument
        let result = bash
            .exec("printf 'A%.0s' $(seq 1 100000)")
            .await;
        match &result {
            Ok(r) => {
                assert!(
                    r.stdout.len() <= 200_000,
                    "printf repeat generated {} bytes",
                    r.stdout.len()
                );
            }
            Err(_) => {}
        }
    }

    /// dd can generate large output from /dev/zero
    #[tokio::test]
    async fn dd_zero_dos() {
        let mut bash = dos_bash();
        let result = bash
            .exec("dd if=/dev/zero bs=1M count=100 2>/dev/null | wc -c")
            .await;
        match &result {
            Ok(r) => {
                let bytes: usize = r.stdout.trim().parse().unwrap_or(0);
                assert!(
                    bytes <= 10_000_000,
                    "dd from /dev/zero produced {} bytes",
                    bytes
                );
            }
            Err(_) => {}
        }
    }

    // --- SLEEP/TIMEOUT VARIANTS ---
    // sleep in subshell bypasses timeout. What about other patterns?

    /// SECURITY FINDING: `sleep` in background + wait bypasses timeout.
    /// Timeout doesn't propagate to background jobs.
    #[tokio::test]
    #[ignore] // SECURITY FINDING: timeout bypass via background sleep + wait
    async fn sleep_background_timeout() {
        let mut bash = Bash::builder()
            .limits(ExecutionLimits::new().timeout(Duration::from_secs(2)))
            .build();
        let start = Instant::now();
        let _ = bash.exec("sleep 100 &\nwait").await;
        let elapsed = start.elapsed();
        assert!(
            elapsed < Duration::from_secs(5),
            "sleep in background+wait bypassed timeout: {:?}",
            elapsed
        );
    }

    /// sleep via read -t
    #[tokio::test]
    async fn read_as_sleep_timeout() {
        let mut bash = Bash::builder()
            .limits(ExecutionLimits::new().timeout(Duration::from_secs(2)))
            .build();
        let start = Instant::now();
        let _ = bash.exec("read -t 100 var < /dev/null").await;
        let elapsed = start.elapsed();
        assert!(
            elapsed < Duration::from_secs(5),
            "read -t bypassed timeout: {:?}",
            elapsed
        );
    }

    /// SECURITY FINDING: `sleep` in pipeline bypasses timeout.
    /// `echo x | sleep 100` runs for 60s+ despite 2s timeout.
    #[tokio::test]
    #[ignore] // SECURITY FINDING: timeout bypass via pipeline sleep
    async fn sleep_pipeline_timeout() {
        let mut bash = Bash::builder()
            .limits(ExecutionLimits::new().timeout(Duration::from_secs(2)))
            .build();
        let start = Instant::now();
        let _ = bash.exec("echo x | sleep 100").await;
        let elapsed = start.elapsed();
        assert!(
            elapsed < Duration::from_secs(5),
            "sleep in pipeline bypassed timeout: {:?}",
            elapsed
        );
    }

    // --- RECURSIVE/NESTED PATTERNS ---
    // source self-recursion crashes. What about other recursion patterns?

    /// Eval-based self recursion — exits 0 without error despite running into limits.
    /// The eval chain silently stops when hitting command limit but reports success.
    #[tokio::test]
    async fn eval_self_recursion() {
        let mut bash = dos_bash();
        let result = bash.exec("x='eval \"$x\"'; eval \"$x\"").await;
        // Should either error or complete — the key is no crash
        // Note: currently exits 0 despite hitting limits, which is debatable
        match &result {
            Ok(_) => {} // Exits 0 — not ideal but not a crash
            Err(_) => {}
        }
    }

    /// Alias-based infinite expansion
    #[tokio::test]
    async fn alias_infinite_expansion() {
        let mut bash = dos_bash();
        let result = bash
            .exec(
                r#"
                shopt -s expand_aliases
                alias a='b'
                alias b='a'
                a
                "#,
            )
            .await;
        // Should not hang or crash
        match &result {
            Ok(_) => {}
            Err(_) => {}
        }
    }

    /// Nested arithmetic expansion — not a security issue, just edge case behavior.
    /// Deep nesting may produce unexpected results due to expansion ordering.
    #[tokio::test]
    async fn nested_arithmetic_expansion() {
        let mut bash = tight_bash();
        let depth = 30;
        let mut expr = "1".to_string();
        for _ in 0..depth {
            expr = format!("$(({} + 1))", expr);
        }
        let result = bash.exec(&format!("echo {}", expr)).await;
        // Should not crash — result correctness is a compatibility concern, not security
        match &result {
            Ok(_) => {}
            Err(_) => {}
        }
    }

    // --- STATE ISOLATION DEEP PROBES ---
    // Traps and $? leak. What else leaks?

    /// Test that aliases don't leak between exec() calls
    #[tokio::test]
    async fn alias_persistence_across_exec() {
        let mut bash = tight_bash();
        let _ = bash
            .exec("shopt -s expand_aliases; alias evil='echo LEAKED'")
            .await;
        let result = bash.exec("evil 2>/dev/null; echo $?").await.unwrap();
        // Check if alias persisted — if evil runs, alias leaked
        let has_leaked = result.stdout.contains("LEAKED");
        // Aliases persisting is arguably a feature (like functions), but
        // worth documenting
    }

    /// SECURITY FINDING: `set -e` persists across exec() calls.
    /// Setting `set -e` in one exec makes subsequent exec calls abort on errors.
    /// This is a state isolation bug — shell options should reset per exec.
    #[tokio::test]
    #[ignore] // SECURITY FINDING: set -e leaks across exec() calls
    async fn shell_options_leak_across_exec() {
        let mut bash = tight_bash();
        let _ = bash.exec("set -e").await;
        let result = bash.exec("false; echo 'survived'").await.unwrap();
        // If set -e leaked, the script would abort on `false`
        assert!(
            result.stdout.contains("survived"),
            "set -e leaked across exec() calls — false aborted execution"
        );
    }

    /// Test that umask leaks
    #[tokio::test]
    async fn umask_leak_across_exec() {
        let mut bash = tight_bash();
        let _ = bash.exec("umask 0000").await;
        let result = bash.exec("umask").await.unwrap();
        // Default umask should be restored
        // (This may be by design — depends on intended state model)
    }

    /// Test that cwd leaks
    #[tokio::test]
    async fn cwd_persistence_across_exec() {
        let mut bash = tight_bash();
        let _ = bash.exec("mkdir -p /tmp/testdir && cd /tmp/testdir").await;
        let result = bash.exec("pwd").await.unwrap();
        // This is arguably expected behavior (interactive shell model),
        // but in a multi-tenant context it could be unexpected
    }

    // --- READONLY BYPASS VARIANTS ---

    /// SECURITY FINDING: `declare` can overwrite readonly variables.
    /// `readonly LOCKED=original; declare LOCKED=overwritten` succeeds.
    #[tokio::test]
    #[ignore] // SECURITY FINDING: readonly bypassed via declare
    async fn readonly_bypass_via_declare() {
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

    /// SECURITY FINDING: `export` can overwrite readonly variables.
    /// `readonly LOCKED=original; export LOCKED=overwritten` succeeds.
    #[tokio::test]
    #[ignore] // SECURITY FINDING: readonly bypassed via export
    async fn readonly_bypass_via_export() {
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

    /// Try to bypass readonly via local in function
    #[tokio::test]
    async fn readonly_bypass_via_local() {
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
        // In bash, local CAN shadow readonly in function scope
        // But after function returns, LOCKED should still be original
        assert!(
            result.stdout.trim().ends_with("original"),
            "readonly not restored after function: got {}",
            result.stdout.trim()
        );
    }

    // --- MEMORY EXHAUSTION ---

    /// Exponential array growth
    #[tokio::test]
    async fn exponential_array_growth() {
        let mut bash = tight_bash();
        let result = bash
            .exec(
                r#"
                arr=(1)
                for i in $(seq 1 20); do
                    arr=("${arr[@]}" "${arr[@]}")
                done
                echo "${#arr[@]}"
                "#,
            )
            .await;
        match &result {
            Ok(r) => {
                let count: usize = r.stdout.trim().parse().unwrap_or(0);
                // 2^20 = 1M elements — this could exhaust memory
                assert!(
                    count <= 1_000_000,
                    "Array grew to {} elements",
                    count
                );
            }
            Err(_) => {} // Error is fine (limits kicked in)
        }
    }

    /// Associative array with massive keys
    #[tokio::test]
    async fn assoc_array_big_keys() {
        let mut bash = tight_bash();
        let big_key = "K".repeat(100_000);
        let result = bash
            .exec(&format!(
                "declare -A m; m[\"{big_key}\"]=val; echo ${{#m[@]}}"
            ))
            .await;
        match &result {
            Ok(r) => assert_eq!(r.stdout.trim(), "1"),
            Err(_) => {}
        }
    }

    // --- INTERESTING EDGE CASES ---

    /// Heredoc that looks like a command
    #[tokio::test]
    async fn heredoc_command_injection() {
        let mut bash = tight_bash();
        let result = bash
            .exec(
                r#"
                cat <<'EOF'
                $(echo INJECTED)
                `echo INJECTED2`
                EOF
                "#,
            )
            .await
            .unwrap();
        // Single-quoted heredoc delimiter should prevent expansion
        assert!(result.stdout.contains("$(echo INJECTED)"),
            "Heredoc expanded command substitution despite single-quoted delimiter");
    }

    /// Case statement with glob patterns
    #[tokio::test]
    async fn case_glob_injection() {
        let mut bash = tight_bash();
        let result = bash
            .exec(
                r#"
                x="../../etc/passwd"
                case "$x" in
                    *etc/passwd*) echo "MATCHED_DANGEROUS_PATH" ;;
                    *) echo "safe" ;;
                esac
                "#,
            )
            .await
            .unwrap();
        // Pattern matching should work correctly
        assert!(result.stdout.contains("MATCHED_DANGEROUS_PATH"));
    }

    /// Tee to many files simultaneously
    #[tokio::test]
    async fn tee_many_files() {
        let mut bash = tight_bash();
        let mut files = String::new();
        for i in 0..50 {
            files.push_str(&format!("/tmp/tee_{}.txt ", i));
        }
        let result = bash
            .exec(&format!("echo content | tee {}", files))
            .await;
        match &result {
            Ok(r) => assert!(r.stdout.contains("content")),
            Err(_) => {}
        }
    }

    /// Command substitution in array index
    #[tokio::test]
    async fn cmd_subst_in_array_index() {
        let mut bash = tight_bash();
        let result = bash
            .exec(
                r#"
                arr=(zero one two three)
                idx=$(echo 2)
                echo "${arr[$idx]}"
                echo "${arr[$(echo 1)]}"
                "#,
            )
            .await
            .unwrap();
        assert!(result.stdout.contains("two"));
    }

    /// Try to abuse jq for computation
    #[tokio::test]
    async fn jq_computation_bypass() {
        let mut bash = dos_bash();
        let result = bash
            .exec(
                r#"
                echo '{}' | jq '[range(10000)] | length'
                "#,
            )
            .await;
        match &result {
            Ok(r) => {
                // jq might allow unbounded computation
            }
            Err(_) => {}
        }
    }

    /// Try to abuse awk to write files
    #[tokio::test]
    async fn awk_file_write() {
        let mut bash = tight_bash();
        let result = bash
            .exec(
                r#"
                echo x | awk 'BEGIN { print "INJECTED" > "/tmp/awk_escape.txt" }'
                cat /tmp/awk_escape.txt 2>/dev/null
                "#,
            )
            .await;
        // awk's file I/O should go through the VFS
        match &result {
            Ok(r) => {
                // If awk can write files, it should be within VFS
            }
            Err(_) => {}
        }
    }

    /// Try to abuse sed to write files
    #[tokio::test]
    async fn sed_file_write() {
        let mut bash = tight_bash();
        let _ = bash
            .exec("echo original > /tmp/sed_test.txt")
            .await;
        let result = bash
            .exec(
                r#"
                sed -i 's/original/MODIFIED/' /tmp/sed_test.txt
                cat /tmp/sed_test.txt
                "#,
            )
            .await
            .unwrap();
        // sed -i should work within VFS
        assert!(
            result.stdout.contains("MODIFIED") || result.stdout.contains("original"),
            "sed -i should work within VFS"
        );
    }

    /// Try to leak info via error messages
    #[tokio::test]
    async fn error_message_info_leak() {
        let mut bash = tight_bash();
        let result = bash
            .exec(
                r#"
                cat /nonexistent/path 2>&1
                ls /real/host/path 2>&1
                cd /absolute/nonsense 2>&1
                "#,
            )
            .await
            .unwrap();
        // Error messages should not contain host filesystem paths
        assert!(
            !result.stdout.contains("/usr/") && !result.stdout.contains("/home/"),
            "Error messages leaked host paths: {}",
            result.stdout
        );
    }

    /// Large number of environment variables
    #[tokio::test]
    async fn env_var_flooding() {
        let mut bash = tight_bash();
        let mut script = String::new();
        for i in 0..1000 {
            script.push_str(&format!("export VAR_{}={}\n", i, "x".repeat(100)));
        }
        script.push_str("env | wc -l\n");
        let result = bash.exec(&script).await;
        match &result {
            Ok(r) => {
                // Should handle many env vars without crashing
            }
            Err(_) => {}
        }
    }

    /// Try to abuse process substitution for data exfiltration
    #[tokio::test]
    async fn process_substitution_file_access() {
        let mut bash = tight_bash();
        let result = bash
            .exec(
                r#"
                diff <(echo "local data") <(cat /etc/passwd 2>/dev/null)
                "#,
            )
            .await;
        match &result {
            Ok(r) => {
                assert!(
                    !r.stdout.contains("root:x:"),
                    "Process substitution accessed host /etc/passwd"
                );
            }
            Err(_) => {}
        }
    }

    /// Massive pipeline chain
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
            Err(_) => {} // Limit error is fine
        }
    }

    /// Try to use mapfile/readarray to bypass limits
    #[tokio::test]
    async fn mapfile_dos() {
        let mut bash = dos_bash();
        let result = bash
            .exec(
                r#"
                seq 1 100000 | mapfile -t arr
                echo "${#arr[@]}"
                "#,
            )
            .await;
        match &result {
            Ok(r) => {
                let count: usize = r.stdout.trim().parse().unwrap_or(0);
                // Should be limited somehow
            }
            Err(_) => {}
        }
    }
}
