// THREAT[TM-DOS-089]: Stack overflow regression tests for nested command substitution.
// Before the Box::pin fix, each $(...) level inlined its async state machine into the
// caller's stack frame, causing overflow at moderate depths. The fix moves the command
// substitution body to the heap via Box::pin, keeping stack usage constant per level.

use bashkit::{Bash, ExecutionLimits};

/// Live output routing must preserve the same substitution depth guarantee.
#[tokio::test]
async fn depth_32_live_execution_no_stack_overflow() {
    let mut script = "echo hello".to_string();
    for _ in 0..32 {
        script = format!("echo $({script})");
    }
    let mut bash = Bash::builder()
        .limits(
            ExecutionLimits::new()
                .max_commands(500)
                .max_subst_depth(32)
                .timeout(std::time::Duration::from_secs(5)),
        )
        .build();
    if let Ok(result) = bash.execute(&script).capture(64).await {
        assert!(!result.stdout.is_empty() || result.exit_code != 0);
    }
}

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

/// THREAT[TM-DOS-125]: a script that re-runs itself through `sh`/`bash`
/// recursed until the process stack overflowed (taking every tenant in the
/// process with it). Child shells now count against the function depth.
#[tokio::test]
async fn recursive_child_shell_is_bounded() {
    let mut bash = Bash::new();
    let r = bash
        .exec("echo 'sh /tmp/s.sh' > /tmp/s.sh; sh /tmp/s.sh; echo rc=$?")
        .await;
    let r = r.unwrap();
    assert!(r.stdout.contains("rc="), "stdout: {}", r.stdout);
    assert!(
        r.stderr.contains("maximum nesting depth exceeded"),
        "stderr: {}",
        r.stderr
    );

    let r = bash
        .exec("f() { bash -c 'f() { bash -c f; }; f'; }; f; echo done")
        .await
        .unwrap();
    assert!(r.stdout.contains("done"), "stdout: {}", r.stdout);
}

/// THREAT[TM-DOS-141]: `eval` re-enters the interpreter on the native stack.
/// A self-referencing eval used to recurse until the stack overflowed and
/// the whole process aborted. It now counts against the function depth.
#[tokio::test]
async fn recursive_eval_is_bounded() {
    let mut bash = Bash::new();
    let r = bash.exec("x='eval \"$x\"'; eval \"$x\"; echo rc=$?").await;
    // Either a clean execution error or a non-zero status: never an abort.
    match r {
        Ok(r) => assert!(
            r.stderr.contains("maximum nesting depth exceeded"),
            "stderr: {}",
            r.stderr
        ),
        Err(e) => assert!(
            e.to_string().contains("maximum nesting depth exceeded"),
            "error: {e}"
        ),
    }

    // Ordinary nested eval keeps working.
    let r = bash
        .exec("eval 'eval \"eval \\\"echo deep\\\"\"'")
        .await
        .unwrap();
    assert_eq!(r.stdout, "deep\n");
}

/// THREAT[TM-DOS-141]: `compgen -C` runs its command text in the current
/// shell; a command that calls itself through `compgen -C` overflowed the
/// stack like recursive eval.
#[tokio::test]
async fn recursive_compgen_command_is_bounded() {
    let mut bash = Bash::new();
    let r = bash
        .exec("x='compgen -C \"$x\" a'; compgen -C \"$x\" a; echo after")
        .await;
    assert_bounded(r, "after");
}

/// The outcome of a capped recursion: the script either continues past it
/// (`marker` in stdout) or `exec` fails with the depth error. Never an abort,
/// and never some other error.
fn assert_bounded(r: bashkit::Result<bashkit::ExecResult>, marker: &str) {
    match r {
        Ok(r) => assert!(
            r.stdout.contains(marker),
            "stdout: {} stderr: {}",
            r.stdout,
            r.stderr
        ),
        Err(e) => assert!(
            e.to_string().contains("maximum nesting depth exceeded")
                || e.to_string().contains("function depth"),
            "error: {e}"
        ),
    }
}

/// THREAT[TM-DOS-125]: a script that runs itself by path (`./s.sh` or via
/// `$PATH`) is a child shell and gets the same cap as `sh s.sh`; it used to
/// overflow the stack and abort the process.
#[tokio::test]
async fn recursive_path_script_is_bounded() {
    let mut bash = Bash::new();
    let r = bash
        .exec("cd /tmp; printf './s.sh\\n' > s.sh; chmod +x s.sh; ./s.sh; echo rc=$?")
        .await
        .unwrap();
    assert!(r.stdout.contains("rc="), "stdout: {}", r.stdout);
    assert!(
        r.stderr.contains("maximum nesting depth exceeded"),
        "stderr: {}",
        r.stderr
    );

    let r = bash
        .exec(
            "mkdir -p /tmp/bin; printf 'p.sh\\n' > /tmp/bin/p.sh; chmod +x /tmp/bin/p.sh; \
             PATH=/tmp/bin:$PATH; p.sh; echo rc=$?",
        )
        .await
        .unwrap();
    assert!(r.stdout.contains("rc="), "stdout: {}", r.stdout);

    // A short legitimate chain of path-invoked scripts still runs.
    let r = bash
        .exec(
            "cd /tmp; printf 'echo leaf\\n' > c.sh; printf './c.sh\\n' > b.sh; \
             printf './b.sh\\n' > a.sh; chmod +x a.sh b.sh c.sh; ./a.sh",
        )
        .await
        .unwrap();
    assert_eq!(r.stdout, "leaf\n");
}

/// THREAT[TM-DOS-142]: per-kind depth caps bound how many levels exist, but
/// their product (nested compounds inside a recursing function, eval inside
/// functions, a path script that evals itself) used to exhaust a 2 MiB stack
/// and abort the process. Interpreter polls now grow the stack on demand.
#[tokio::test]
async fn nested_compounds_inside_recursion_do_not_overflow() {
    let ifs_open = "if true; then ".repeat(10);
    let ifs_close = "fi; ".repeat(10);
    let script = format!(
        "f(){{ {ifs_open} if (( d < 15 )); then d=$((d+1)); f; fi; {ifs_close} }}; d=0; f; echo d=$d"
    );
    let mut bash = Bash::new();
    let r = bash.exec(&script).await.unwrap();
    assert!(r.stdout.contains("d=15"), "stdout: {}", r.stdout);

    let braces_open = "{ ".repeat(10);
    let braces_close = "} ".repeat(10);
    let script = format!(
        "g(){{ {braces_open} if (( d < 30 )); then d=$((d+1)); echo $(g); else echo leaf; fi; {braces_close} }}; d=0; g; echo after"
    );
    assert_bounded(bash.exec(&script).await, "after");

    assert_bounded(bash.exec("f(){ eval f; }; f; echo rc=$?").await, "rc=");

    let r = bash
        .exec("cd /tmp; printf 'eval ./m.sh\\n' > m.sh; chmod +x m.sh; ./m.sh; echo rc=$?")
        .await
        .unwrap();
    assert!(r.stdout.contains("rc="), "stdout: {}", r.stdout);
}
