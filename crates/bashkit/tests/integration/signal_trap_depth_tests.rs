//! Signal handlers share the function/source depth ceiling (TM-DOS-020).
//! Small command budgets keep pre-fix recursion probes safe.

use bashkit::{Bash, ExecutionLimits};

#[tokio::test]
async fn recursive_signal_traps_share_function_depth() {
    for script in [
        "trap 'kill -USR1 $$' USR1; kill -USR1 $$",
        "trap 'kill -USR2 $$' USR1; trap 'kill -USR1 $$' USR2; kill -USR1 $$",
        "f() { kill -USR1 $$; }; trap 'f' USR1; kill -USR1 $$",
        "echo 'kill -USR1 $$' > /handler; trap 'source /handler' USR1; kill -USR1 $$",
    ] {
        for depth in [2, 3] {
            let mut bash = Bash::builder()
                .limits(
                    ExecutionLimits::new()
                        .max_function_depth(depth)
                        .max_commands(12),
                )
                .build();
            let err = bash.exec(script).await.unwrap_err().to_string();
            assert!(err.contains("depth exceeded"), "{script}: {err}");
            assert!(!err.contains("command limit"), "{script}: {err}");
            let next = bash.exec("echo recovered").await.unwrap();
            assert_eq!(next.stdout, "recovered\n");
            assert_eq!(next.exit_code, 0);
        }
    }
}

#[tokio::test]
async fn signal_depth_is_released_after_each_handler() {
    let mut bash = Bash::builder()
        .limits(ExecutionLimits::new().max_function_depth(2))
        .build();
    let result = bash.exec("trap 'echo first; kill -USR2 $$; echo last' USR1; trap 'echo second' USR2; kill -USR1 $$; kill -USR1 $$; echo done").await.unwrap();
    assert_eq!(result.exit_code, 0);
    assert_eq!(
        result.stdout,
        "first\nsecond\nlast\nfirst\nsecond\nlast\ndone\n"
    );
    assert!(result.stderr.is_empty(), "{}", result.stderr);
}

#[tokio::test]
async fn ignored_signal_needs_no_handler_depth() {
    let mut bash = Bash::builder()
        .limits(ExecutionLimits::new().max_function_depth(0))
        .build();
    let result = bash
        .exec("trap '' USR1; kill -USR1 $$; echo ignored")
        .await
        .unwrap();
    assert_eq!(result.exit_code, 0);
    assert_eq!(result.stdout, "ignored\n");
}

#[tokio::test]
async fn signal_handler_command_limit_reaches_host() {
    let mut bash = Bash::builder()
        .limits(ExecutionLimits::new().max_commands(3))
        .build();
    let err = bash
        .exec("trap 'echo first; echo second' USR1; kill -USR1 $$")
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("maximum command count exceeded"), "{err}");
}

#[tokio::test]
async fn signal_handler_parse_error_is_swallowed() {
    // TM-DOS-030 (merged main semantics): an unparsable handler never
    // disturbs the interrupted command; the kill completes and the shell
    // stays usable.
    let mut bash = Bash::builder().build();
    let result = bash
        .exec("trap 'if' USR1; kill -USR1 $$; echo done")
        .await
        .unwrap();
    assert_eq!(result.stdout, "done\n");
    assert_eq!(result.exit_code, 0);
    let next = bash.exec("echo recovered").await.unwrap();
    assert_eq!(next.stdout, "recovered\n");
}

#[tokio::test]
async fn finite_three_level_nesting_succeeds_with_recovery() {
    // Terminates by construction (acyclic signal chain USR1 -> USR2 -> USR3):
    // three nested handler entries are admitted, with exact output.
    let mut bash = Bash::builder().build();
    let result = bash
        .exec("trap 'echo one; kill -USR2 $$' USR1; trap 'echo two; kill -TERM $$' USR2; trap 'echo three' TERM; kill -USR1 $$; echo done")
        .await
        .unwrap();
    assert_eq!(result.stdout, "one\ntwo\nthree\ndone\n");
    assert_eq!(result.exit_code, 0);
    let next = bash.exec("echo recovered").await.unwrap();
    assert_eq!(next.stdout, "recovered\n");
}

#[tokio::test]
async fn shared_depth_zero_admits_no_handlers() {
    // Bounded by construction: the first trap entry already exceeds a zero
    // budget, so no handler body runs and the trip surfaces.
    let mut bash = Bash::builder()
        .limits(ExecutionLimits::new().max_function_depth(0))
        .build();
    let err = bash
        .exec("trap 'echo ENTRY >> /log; kill -USR1 $$' USR1; kill -USR1 $$")
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("depth exceeded"), "{err}");
    let log = bash
        .exec("cat /log 2>/dev/null || echo none")
        .await
        .unwrap();
    assert!(
        log.stdout.contains("none"),
        "no handler may run: {}",
        log.stdout
    );
}

#[tokio::test]
async fn shared_depth_one_admits_exactly_one_handler() {
    // Bounded by construction: each cycle consumes depth; the second entry
    // trips, so exactly one handler body runs.
    let mut bash = Bash::builder()
        .limits(ExecutionLimits::new().max_function_depth(1))
        .build();
    let err = bash
        .exec("trap 'echo ENTRY >> /log; kill -USR1 $$' USR1; kill -USR1 $$")
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("depth exceeded"), "{err}");
    let log = bash.exec("cat /log").await.unwrap();
    assert_eq!(log.stdout.lines().count(), 1, "{}", log.stdout);
}
