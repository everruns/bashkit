// THREAT[TM-DOS-089]: Stack overflow regression tests for nested command substitution.
// Before the Box::pin fix, each $(...) level inlined its async state machine into the
// caller's stack frame, causing overflow at moderate depths. The fix moves the command
// substitution body to the heap via Box::pin, keeping stack usage constant per level.

use bashkit::{Bash, ExecutionLimits};

/// Default max_subst_depth (32) must not cause stack overflow.
/// Before the Box::pin fix, this would SIGABRT at ~20-30 levels.
/// Regression test for issue #1089.
#[tokio::test]
async fn depth_32_no_stack_overflow() {
    let depth = 32;
    let mut cmd = "echo hello".to_string();
    for _ in 0..depth {
        cmd = format!("echo $({})", cmd);
    }

    let mut bash = Bash::builder()
        .limits(
            ExecutionLimits::new()
                .max_commands(500)
                .max_subst_depth(32)
                .timeout(std::time::Duration::from_secs(5)),
        )
        .build();

    // Must not stack-overflow — should return a result (possibly truncated by depth limit)
    let result = bash.exec(&cmd).await;
    if let Ok(r) = result {
        // Depth limit error is also acceptable (Err case)
        assert!(!r.stdout.is_empty() || r.exit_code != 0);
    }
}

/// Deeply nested $() in arithmetic context (the fuzz crash vector).
/// Regression test for the specific #1089 crash input pattern.
#[tokio::test]
async fn nested_subst_in_arithmetic_no_overflow() {
    // Simulates the crash input: nested $( inside $((...))
    let mut inner = "echo 1".to_string();
    for _ in 0..15 {
        inner = format!("echo $({})", inner);
    }
    let script = format!("echo $(($({})))", inner);

    let mut bash = Bash::builder()
        .limits(
            ExecutionLimits::new()
                .max_commands(500)
                .max_subst_depth(20)
                .timeout(std::time::Duration::from_secs(5)),
        )
        .build();

    // Must not panic or SIGABRT
    let _ = bash.exec(&script).await;
}
