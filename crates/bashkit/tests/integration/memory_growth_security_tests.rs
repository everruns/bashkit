//! TM-DOS-110: in-builtin memory growth must hit a limit, not the allocator (#2444).
//!
//! A script that keeps growing a value inside awk used to allocate until the
//! host aborted with `memory allocation ... failed`. Every growth path must
//! stop at a cap first: a fatal awk error (exit 2), and the shell carries on.

use bashkit::{Bash, ExecutionLimits};

async fn run(script: &str) -> bashkit::ExecResult {
    Bash::new().exec(script).await.unwrap()
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
        let limits = ExecutionLimits::new().max_live_intermediate_bytes(1_000_000);
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
