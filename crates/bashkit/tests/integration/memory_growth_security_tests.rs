//! TM-DOS-110: in-builtin memory growth must hit a limit, not the allocator (#2444).
//!
//! A script that keeps growing a value inside awk used to allocate until the
//! host aborted with `memory allocation ... failed`. Every growth path must
//! stop at a cap first: a fatal awk error (exit 2), and the shell carries on.
//!
//! DECISION: every assertion here names a *memory* or *output* outcome, so it
//! must not race the execution deadline. Inheriting the 30 s default made the
//! expected diagnostic a function of machine speed: under the nightly
//! AddressSanitizer job (`-Z sanitizer=address`, ~15x slower) the deadline
//! fired first and `jq` reported `execution timed out` where a size cap was
//! expected. Cap tests therefore pin [`SLOW_BUILD_TIMEOUT`] instead. It is
//! generous enough for instrumented builds (ASAN, Miri) and still far below a
//! genuinely unbounded or quadratic regression, which overruns it by orders of
//! magnitude. `non_emitting_loop_stops_at_the_timeout` is the one test that
//! asserts a timeout, and sets its own short deadline.

use bashkit::{Bash, ExecutionLimits, MemoryLimits};

/// A rejected scalar assignment must fail the request, never silently keep the
/// old value and report success to the caller.
#[tokio::test]
async fn scalar_assignment_budget_failure_is_reported_and_session_recovers() {
    let mut bash = Bash::builder()
        .memory_limits(MemoryLimits::new().max_total_variable_bytes(1024))
        .build();
    let error = bash
        .exec("s=x; for i in {1..11}; do s=\"$s$s\"; done; echo done")
        .await
        .expect_err("over-budget scalar assignment must fail execution");
    assert!(
        error.to_string().contains("variable byte limit"),
        "unexpected diagnostic: {error}"
    );
    let recovered = bash.exec("echo recovered").await.unwrap();
    assert_eq!(recovered.exit_code, 0);
    assert_eq!(recovered.stdout, "recovered\n");
}

#[tokio::test]
async fn local_and_exported_scalar_budget_failures_are_reported() {
    for script in [
        "f(){ local s=x; for i in {1..11}; do s=\"$s$s\"; done; }; f",
        "set -a; s=x; for i in {1..11}; do s=\"$s$s\"; done",
    ] {
        let mut bash = Bash::builder()
            .memory_limits(MemoryLimits::new().max_total_variable_bytes(1024))
            .build();
        let error = bash.exec(script).await.expect_err(script);
        assert!(
            error.to_string().contains("byte limit"),
            "{script}: {error}"
        );
    }
}

/// Wall-clock budget for tests whose expected outcome is a memory or output
/// cap. Instrumented builds are slow; the deadline must not preempt the cap.
const SLOW_BUILD_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(600);

/// Default limits with the execution deadline pushed out of the way, so a cap
/// is the only thing that can stop these scripts.
fn capped_limits() -> ExecutionLimits {
    ExecutionLimits::new().timeout(SLOW_BUILD_TIMEOUT)
}

async fn run(script: &str) -> bashkit::ExecResult {
    run_with(capped_limits(), script).await
}

async fn run_with(limits: ExecutionLimits, script: &str) -> bashkit::ExecResult {
    Bash::builder()
        .limits(limits)
        .build()
        .exec(script)
        .await
        .unwrap()
}

const STRING_CAP: &str = "awk: fatal: string size limit (16777216 bytes) exceeded\n";

fn assert_fatal(r: &bashkit::ExecResult, stderr: &str, what: &str) {
    assert_eq!(r.stderr, stderr, "{what}: diagnostic");
    assert!(r.stdout.ends_with("after\n"), "{what}: shell continues");
    assert!(
        r.stdout.contains("rc=2"),
        "{what}: awk exits 2: {}",
        r.stdout
    );
}

mod awk_strings {
    use super::*;

    /// The report in #2444: doubling a string forever.
    #[tokio::test]
    async fn doubling_concat_stops_at_string_cap() {
        let r = run("awk 'BEGIN { s = \"x\"; while (1) s = s s }'; echo rc=$?; echo after").await;
        assert_fatal(&r, STRING_CAP, "s = s s");
    }

    #[tokio::test]
    async fn multi_part_concat_is_checked_before_allocating() {
        let r = run(
            "awk 'BEGIN { s = \"x\"; for (i = 0; i < 22; i++) s = s s; t = s s s s s }'; echo rc=$?; echo after",
        )
        .await;
        assert_fatal(&r, STRING_CAP, "5-way concat of 4 MiB");
    }

    #[tokio::test]
    async fn gsub_amplification_stops_at_string_cap() {
        // 10k matches, each replaced by a 10k string: 100 MB.
        let r = run(
            "awk 'BEGIN { s = sprintf(\"%10000s\", \"\"); gsub(/ /, s, s) }'; echo rc=$?; echo after",
        )
        .await;
        assert_fatal(&r, STRING_CAP, "gsub");
    }

    #[tokio::test]
    async fn gensub_amplification_stops_at_string_cap() {
        let r = run(
            "awk 'BEGIN { s = sprintf(\"%10000s\", \"\"); t = gensub(/ /, s, \"g\", s) }'; echo rc=$?; echo after",
        )
        .await;
        assert_fatal(&r, STRING_CAP, "gensub");
    }

    #[tokio::test]
    async fn sprintf_amplification_stops_at_string_cap() {
        // Five 4 MiB conversions render 20 MiB.
        let r = run(
            "awk 'BEGIN { s = \"x\"; for (i = 0; i < 22; i++) s = s s; x = sprintf(\"%s%s%s%s%s\", s, s, s, s, s) }'; echo rc=$?; echo after",
        )
        .await;
        assert_fatal(&r, STRING_CAP, "sprintf");
    }

    #[tokio::test]
    async fn record_rebuild_stops_at_string_cap() {
        let r = run(
            "echo a | awk '{ OFS = sprintf(\"%10000s\", \"\"); $5000 = 1 }'; echo rc=$?; echo after",
        )
        .await;
        assert_fatal(&r, STRING_CAP, "$N rebuild with wide OFS");
    }

    #[tokio::test]
    async fn huge_field_index_stops_at_field_cap() {
        let r = run("echo a | awk '{ $1000000000 = 1 }'; echo rc=$?; echo after").await;
        assert_fatal(
            &r,
            "awk: fatal: field index limit (100000) exceeded\n",
            "$1e9 = 1",
        );
    }
}

mod awk_state {
    use super::*;

    fn memory_cap(limit: u64) -> String {
        format!("awk: fatal: memory limit ({limit} bytes) exceeded\n")
    }

    #[tokio::test]
    async fn array_of_large_values_stops_at_memory_cap() {
        // 9000 x 10 KB = 90 MB of array values, over the 32 MB default.
        let r = run(
            "awk 'BEGIN { s = sprintf(\"%10000s\", \"\"); for (i = 0; i < 9000; i++) a[i] = s }'; echo rc=$?; echo after",
        )
        .await;
        assert_fatal(&r, &memory_cap(32_000_000), "array growth");
    }

    #[tokio::test]
    async fn split_into_many_elements_stops_at_memory_cap() {
        // A 2 MiB string of 1M words: per-element overhead dominates.
        let r = run(
            "awk 'BEGIN { s = \"a \"; for (i = 0; i < 20; i++) s = s s; n = split(s, a) }'; echo rc=$?; echo after",
        )
        .await;
        assert_fatal(&r, &memory_cap(32_000_000), "split");
    }

    #[tokio::test]
    async fn recursion_frames_count_toward_memory_cap() {
        // Each frame shadows a 640 KB local; 60 frames hold ~38 MB.
        let r = run(
            "awk 'function f(n, s) { s = big; if (n < 60) f(n + 1) }
                  BEGIN { big = sprintf(\"%10000s\", \"\"); for (i = 0; i < 6; i++) big = big big; f(0) }'; echo rc=$?; echo after",
        )
        .await;
        assert_fatal(&r, &memory_cap(32_000_000), "recursion frames");
    }

    #[tokio::test]
    async fn memory_cap_follows_host_live_bytes_limit() {
        let limits = capped_limits().max_live_intermediate_bytes(1_000_000);
        let r = run_with(
            limits,
            "awk 'BEGIN { s = sprintf(\"%10000s\", \"\"); for (i = 0; i < 200; i++) a[i] = s }'; echo rc=$?; echo after",
        )
        .await;
        assert_fatal(&r, &memory_cap(1_000_000), "host limit");
    }

    #[tokio::test]
    async fn deleting_releases_memory() {
        // 20 rounds of 2.5 MB each fit when every round is deleted.
        let r = run(
            "awk 'BEGIN { s = sprintf(\"%10000s\", \"\"); for (r = 0; r < 20; r++) { for (i = 0; i < 250; i++) a[i] = s; delete a } print \"ok\" }'",
        )
        .await;
        assert_eq!(r.stderr, "");
        assert_eq!(r.stdout, "ok\n");
    }

    #[tokio::test]
    async fn ordinary_array_workload_still_fits() {
        let r = run("seq 100000 | awk '{ a[NR] = $0 } END { print a[1], a[100000] }'").await;
        assert_eq!(r.stderr, "");
        assert_eq!(r.stdout, "1 100000\n");
        assert_eq!(r.exit_code, 0);
    }
}

#[cfg(feature = "jq")]
mod jq {
    use super::*;

    fn size_error(limit: u64) -> String {
        format!("jq: error: value size limit ({limit} bytes) exceeded\n")
    }

    async fn assert_jq_capped(filter: &str) {
        let script = format!("jq -n '{filter}'; echo rc=$?; echo after");
        let r = run(&script).await;
        assert_eq!(r.stderr, size_error(32_000_000), "{filter}: diagnostic");
        assert_eq!(
            r.stdout, "rc=5\nafter\n",
            "{filter}: exit 5, shell continues"
        );
    }

    /// The report in #2444, and every other way a value can grow inside one
    /// evaluation without emitting anything.
    #[tokio::test]
    async fn growth_inside_one_evaluation_stops_at_the_limit() {
        for filter in [
            r#""x" | until(false; . + .)"#,
            r#""x" | until(false; "\(.)\(.)")"#,
            r#""x" * 1000000000000"#,
            r#"[range(1e12)]"#,
            r#"[1] | until(false; . + .)"#,
            r#"{"a": 1} | until(false; . + {(tostring): .})"#,
            r#"reduce range(40) as $i ("x"; tojson)"#,
            r#"[range(100000) | "x" * 1000]"#,
            r#"[range(200) | [range(100000)]]"#,
        ] {
            assert_jq_capped(filter).await;
        }
    }

    #[tokio::test]
    async fn limit_follows_host_live_bytes_limit() {
        let limits = capped_limits().max_live_intermediate_bytes(100_000);
        let r = run_with(limits, "jq -n '\"x\" * 200000 | length'").await;
        assert_eq!(r.stderr, size_error(100_000));
        assert_eq!(r.exit_code, 5);
    }

    #[tokio::test]
    async fn shared_structure_cannot_expand_on_output() {
        // 2^40 leaves in memory as 40 shared nodes.
        let r = run("jq -nc 'reduce range(40) as $i (1; [., .])'").await;
        assert_eq!(r.stderr, "jq: output limit exceeded (1048576 bytes)\n");
        assert_eq!(r.exit_code, 5);
    }

    /// A loop that never emits used to spin (and grow evaluator state)
    /// until the host died; value operations now poll the deadline.
    #[tokio::test]
    async fn non_emitting_loop_stops_at_the_timeout() {
        let limits = ExecutionLimits::new().timeout(std::time::Duration::from_millis(300));
        for filter in [
            "until(false; .)",
            "0 | until(false; . + 1)",
            "last(range(1e15))",
        ] {
            let script = format!("jq -n '{filter}'; echo rc=$?");
            let r = run_with(limits.clone(), &script).await;
            assert_eq!(r.stderr, "jq: execution timed out\n", "{filter}");
            assert_eq!(r.stdout, "rc=5\n", "{filter}");
        }
    }

    #[tokio::test]
    async fn catching_the_error_does_not_reset_the_limit() {
        let r = run(r#"jq -n '"x" | until(false; try (. + .) catch "y")'"#).await;
        assert_eq!(r.stderr, size_error(32_000_000));
        assert_eq!(r.exit_code, 5);
    }

    #[tokio::test]
    async fn released_memory_is_reusable() {
        // 30 rounds of a 10 MB string: only one is alive at a time.
        let r = run(r#"jq -n 'reduce range(30) as $i (0; . + ("x" * 10000000 | length))'"#).await;
        assert_eq!(r.stderr, "");
        assert_eq!(r.stdout, "300000000\n");
    }

    /// `join` appends in place; metering must not turn it quadratic (300k
    /// joins took over 30 s when every append copied the string).
    #[tokio::test]
    async fn ordinary_workloads_still_fit() {
        let r = run(
            "jq -n '[range(300000)] | map(tostring) | join(\",\") | length'; \
             jq -n 'reduce range(50000) as $i ({}; .[$i|tostring] = $i) | length'",
        )
        .await;
        assert_eq!(r.stderr, "");
        assert_eq!(r.stdout, "1988889\n50000\n");
    }
}
