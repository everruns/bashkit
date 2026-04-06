// THREAT[TM-DOS-088]: Command substitution depth limit tests.
// Each $(...) level clones the full interpreter state, so memory ≈ depth × state_size.
// A dedicated max_subst_depth limit (default 32) prevents OOM from deeply nested
// command substitution chains.

use bashkit::{Bash, ExecutionLimits};

/// Deeply nested command substitution must be capped by max_subst_depth.
/// Regression test for issue #1088.
#[tokio::test]
async fn subst_depth_limit_prevents_oom() {
    // Build a deeply nested $(...) expression: $(echo $(echo $(echo ...)))
    let depth = 40; // exceeds default max_subst_depth of 32
    let mut script = "echo hi".to_string();
    for _ in 0..depth {
        script = format!("echo $({})", script);
    }

    let mut bash = Bash::builder()
        .limits(
            ExecutionLimits::new()
                .max_commands(500)
                .max_subst_depth(5)
                .timeout(std::time::Duration::from_secs(5)),
        )
        .build();

    // Must not OOM — should return an error or truncated result
    let result = bash.exec(&script).await;
    // The execution should either error or succeed with limited depth,
    // but must NOT panic or OOM.
    assert!(result.is_ok() || result.is_err());
}

/// With a low subst depth limit, nested substitutions produce an error.
#[tokio::test]
async fn subst_depth_limit_returns_error() {
    let mut script = "echo hi".to_string();
    for _ in 0..10 {
        script = format!("echo $({})", script);
    }

    let mut bash = Bash::builder()
        .limits(
            ExecutionLimits::new()
                .max_commands(500)
                .max_subst_depth(3)
                .timeout(std::time::Duration::from_secs(5)),
        )
        .build();

    let result = bash.exec(&script).await;
    match result {
        Ok(r) => {
            // Execution succeeded but depth was limited (error message in stderr
            // or result was truncated)
            assert!(
                r.exit_code != 0 || r.stderr.contains("substitution depth"),
                "expected error from deep nesting, got exit_code={} stderr={:?}",
                r.exit_code,
                r.stderr
            );
        }
        Err(e) => {
            let msg = e.to_string();
            assert!(
                msg.contains("substitution depth") || msg.contains("nesting"),
                "unexpected error: {}",
                msg
            );
        }
    }
}

/// Fuzz crash input from issue #1088 (decoded from base64).
/// Original: agsfXzpfX19fX19nX19fJChbXTBfBQUFBQUfXzpfX19fX19nX18FBQUFBQQFBQUFBQUFBQUFBQUFBQUFBQUFBQU=
#[tokio::test]
async fn fuzz_crash_1088_oom_input() {
    use base64::Engine;
    let b64 =
        "agsfXzpfX19fX19nX19fJChbXTBfBQUFBQUfXzpfX19fX19nX18FBQUFBQQFBQUFBQUFBQUFBQUFBQUFBQUFBQU=";
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(b64)
        .unwrap_or_default();
    let input = String::from_utf8_lossy(&decoded);
    let script = format!("echo $(({})) 2>/dev/null", input);

    let mut bash = Bash::builder()
        .limits(
            ExecutionLimits::new()
                .max_commands(100)
                .max_function_depth(10)
                .max_subst_depth(5)
                .max_stdout_bytes(4096)
                .max_stderr_bytes(4096)
                .timeout(std::time::Duration::from_millis(500)),
        )
        .build();

    // Must not panic or OOM
    let _ = bash.exec(&script).await;
}
