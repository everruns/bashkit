//! TM-DOS-109: builtin resource caps must fail loudly (#2446).
//!
//! A builtin that stops at a cap keeps its partial output, names the cap on
//! stderr and exits non-zero. Each cap also still bounds the work: the tests
//! check both the diagnostic and that the cap cannot be bypassed.

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

mod awk_loops {
    use super::*;

    const LOOP_FORMS: &[&str] = &[
        "while (1) n++",
        "do { n++ } while (1)",
        "for (i = 0; 1; i++) n++",
        "for (i = 0; i < 1000; i++) n++",
    ];

    #[tokio::test]
    async fn every_loop_form_aborts_at_the_per_loop_cap() {
        for body in LOOP_FORMS {
            let script = format!("awk 'BEGIN {{ {body}; print n }} END {{ print \"end\" }}'");
            let r = run_with(ExecutionLimits::new().max_loop_iterations(50), &script).await;
            assert_eq!(r.exit_code, 2, "{body}: exit code");
            assert_eq!(r.stdout, "", "{body}: no partial result or END output");
            assert_eq!(
                r.stderr, "awk: fatal: loop iteration limit (50) exceeded\n",
                "{body}: diagnostic"
            );
        }
    }

    #[tokio::test]
    async fn for_in_aborts_at_the_per_loop_cap() {
        let r = run_with(
            ExecutionLimits::new().max_loop_iterations(5),
            "awk 'BEGIN { split(\"a b c d\", x); for (k in x) n++; print n }'",
        )
        .await;
        assert_eq!(r.exit_code, 0, "4 keys fit a cap of 5");
        assert_eq!(r.stdout, "4\n");

        let r = run_with(
            ExecutionLimits::new().max_loop_iterations(3),
            "awk 'BEGIN { split(\"a b c d\", x); for (k in x) n++; print n }'",
        )
        .await;
        assert_eq!(r.exit_code, 2);
        assert!(r.stderr.contains("loop iteration limit (3) exceeded"));
    }

    #[tokio::test]
    async fn loops_under_the_cap_run_to_completion() {
        let r = run("awk 'BEGIN { for (i = 0; i < 9000; i++) n++; print n }'").await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "9000\n");
        assert_eq!(r.stderr, "");
    }

    /// Nested loops, each under the per-loop cap, cannot multiply work past
    /// the whole-program cap.
    #[tokio::test]
    async fn nested_loops_cannot_bypass_the_total_cap() {
        let r = run_with(
            ExecutionLimits::new()
                .max_loop_iterations(1_000)
                .max_total_loop_iterations(5_000),
            "awk 'BEGIN { for (i = 0; i < 999; i++) for (j = 0; j < 999; j++) n++; print n }'",
        )
        .await;
        assert_eq!(r.exit_code, 2);
        assert_eq!(r.stdout, "");
        assert_eq!(
            r.stderr,
            "awk: fatal: total loop iteration limit (5000) exceeded\n"
        );
    }

    /// The cap is per awk run: a later rule or record cannot resume a loop
    /// the fatal error stopped.
    #[tokio::test]
    async fn fatal_stops_remaining_records() {
        let r = run_with(
            ExecutionLimits::new().max_loop_iterations(10),
            "printf '1\\n2\\n3\\n' | awk '{ print \"rec\", $1; while (1) n++ }'",
        )
        .await;
        assert_eq!(r.exit_code, 2);
        assert_eq!(r.stdout, "rec 1\n");
    }

    #[tokio::test]
    async fn recursion_cap_is_fatal() {
        let r =
            run("awk 'function f(n) { return f(n + 1) } BEGIN { f(1); print \"after\" }'").await;
        assert_eq!(r.exit_code, 2);
        assert_eq!(r.stdout, "");
        assert_eq!(
            r.stderr,
            "awk: fatal: function call depth limit (64) exceeded\n"
        );
    }

    #[tokio::test]
    async fn exit_status_reaches_the_shell() {
        let r = run_with(
            ExecutionLimits::new().max_loop_iterations(10),
            "set -e; awk 'BEGIN { while (1) n++ }'; echo unreachable",
        )
        .await;
        assert_ne!(r.exit_code, 0);
        assert!(!r.stdout.contains("unreachable"));
    }
}

mod awk_getline {
    use super::*;

    #[tokio::test]
    async fn open_file_cap_is_fatal_and_close_releases_it() {
        let script = "for i in $(seq 0 104); do echo \"l$i\" > /tmp/g$i; done
awk 'BEGIN { for (i = 0; i < 105; i++) { f = \"/tmp/g\" i; if ((getline x < f) > 0) ok++ } print ok }'
echo \"rc=$?\"
awk 'BEGIN { for (i = 0; i < 105; i++) { f = \"/tmp/g\" i; if ((getline x < f) > 0) ok++; close(f) } print ok }'";
        let r = run(script).await;
        assert_eq!(r.stdout, "rc=2\n105\n");
        assert!(
            r.stderr
                .contains("awk: fatal: getline open file limit (100) exceeded")
        );
    }

    #[tokio::test]
    async fn missing_file_is_still_minus_one() {
        let r = run("awk 'BEGIN { print (getline x < \"/tmp/nope\") }'").await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "-1\n");
    }
}

mod seq_cap {
    use super::*;

    #[tokio::test]
    async fn over_the_line_cap_reports_and_fails() {
        let r = run("seq 200000 | wc -l; echo \"${PIPESTATUS[0]}\"").await;
        assert_eq!(
            r.stdout.split_whitespace().collect::<Vec<_>>(),
            ["100000", "1"]
        );
        assert!(
            r.stderr
                .contains("seq: line limit (100000) exceeded; output truncated")
        );
    }

    #[tokio::test]
    async fn exactly_the_cap_is_not_an_error() {
        let r = run("seq 100000 | tail -n 1").await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "100000\n");
        assert_eq!(r.stderr, "");
    }

    #[tokio::test]
    async fn byte_cap_reports_and_fails() {
        let r = run("seq -s '..........' 1 99999 > /dev/null; echo $?").await;
        assert_eq!(r.stdout, "1\n");
        assert!(
            r.stderr
                .contains("seq: output byte limit (1048576) exceeded")
        );
    }

    #[tokio::test]
    async fn pipefail_sees_the_truncation() {
        let r = run("set -o pipefail; seq 200000 | wc -l > /dev/null; echo $?").await;
        assert_eq!(r.stdout, "1\n");
    }
}

mod yes_cap {
    use super::*;

    #[tokio::test]
    async fn yes_reports_its_cap() {
        let r = run("yes | wc -l; echo \"${PIPESTATUS[0]}\"").await;
        assert_eq!(
            r.stdout.split_whitespace().collect::<Vec<_>>(),
            ["10000", "1"]
        );
        assert!(
            r.stderr
                .contains("yes: output limit (10000 lines) exceeded")
        );
    }

    #[tokio::test]
    async fn yes_into_head_keeps_the_pipeline_status() {
        let r = run("yes | head -n 3; echo \"rc=$?\"").await;
        assert_eq!(r.stdout, "y\ny\ny\nrc=0\n");
    }
}

mod numfmt_cap {
    use super::*;

    #[tokio::test]
    async fn numfmt_output_cap_reports_and_fails() {
        let r = run("seq 1 90000 | numfmt --padding=20 > /dev/null; echo $?").await;
        assert_eq!(r.stdout, "1\n");
        assert!(
            r.stderr
                .contains("numfmt: output byte limit (1048576) exceeded")
        );
    }
}

mod history_cap {
    use super::*;

    #[tokio::test]
    async fn history_listing_cap_reports_and_fails() {
        let mut bash = Bash::builder()
            .limits(ExecutionLimits::new().max_history_output_bytes(40))
            .build();
        for i in 0..10 {
            bash.exec(&format!("echo command-number-{i}"))
                .await
                .unwrap();
        }
        let r = bash.exec("history").await.unwrap();
        assert_eq!(r.exit_code, 1);
        assert!(r.stdout.len() <= 40);
        assert!(
            r.stderr
                .contains("history: output limit (40 bytes) exceeded")
        );
    }
}
