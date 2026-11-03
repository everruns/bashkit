//! fmt's repeated indentation/prefix must be admitted before output allocation.
use bashkit::{Bash, Error, ExecOptions, ExecutionLimits, LimitExceeded};
use std::path::Path;

fn amplified_input() -> Vec<u8> {
    format!("{}{}\n", " ".repeat(256), "a ".repeat(128)).into_bytes()
}

#[tokio::test]
async fn fmt_indentation_respects_live_budget_and_session_recovers() {
    for mode in ["-q", "", "-c", "-t", "-s"] {
        for from_file in [false, true] {
            let mut bash = Bash::builder()
                .limits(ExecutionLimits::new().max_live_intermediate_bytes(8192))
                .build();
            let input = amplified_input();
            bash.fs()
                .write_file(Path::new("/input"), &input)
                .await
                .unwrap();
            let result = if from_file {
                bash.exec(&format!("fmt {mode} /input")).await
            } else {
                bash.exec_with_options(&format!("fmt {mode}"), ExecOptions::new().stdin(input))
                    .await
            };
            assert!(
                matches!(
                    result,
                    Err(Error::ResourceLimit(LimitExceeded::ExecutionBudget(_)))
                ),
                "mode={mode} file={from_file}: {result:?}"
            );
            let result = bash.exec("printf 'a b\\n' | fmt").await.unwrap();
            assert_eq!(result.stdout, "a b\n");
            assert_eq!(result.exit_code, 0);
        }
    }
}

#[tokio::test]
async fn fmt_repeated_prefix_and_multiple_files_share_output_budget() {
    for mode in ["-q", ""] {
        let prefix = "#".repeat(256);
        let input = format!("{prefix}{}\n", "a ".repeat(128));
        let mut bash = Bash::builder()
            .limits(ExecutionLimits::new().max_live_intermediate_bytes(8192))
            .build();
        let result = bash
            .exec_with_options(
                &format!("fmt {mode} -p '{prefix}'"),
                ExecOptions::new().stdin(input),
            )
            .await;
        assert!(matches!(
            result,
            Err(Error::ResourceLimit(LimitExceeded::ExecutionBudget(_)))
        ));
    }
    let mut bash = Bash::builder()
        .limits(ExecutionLimits::new().max_live_intermediate_bytes(8192))
        .build();
    let input = format!("{}a\n", " ".repeat(2048));
    bash.fs()
        .write_file(Path::new("/input"), input.as_bytes())
        .await
        .unwrap();
    assert_eq!(
        bash.exec("fmt /input").await.unwrap().stdout.as_bytes(),
        input.as_bytes()
    );
    assert!(matches!(
        bash.exec("fmt /input /input /input").await,
        Err(Error::ResourceLimit(LimitExceeded::ExecutionBudget(_)))
    ));
}

#[tokio::test]
async fn fmt_generated_bytes_consume_work_before_capture() {
    let mut bash = Bash::builder()
        .limits(
            ExecutionLimits::new()
                .max_work_units(4096)
                .max_stdout_bytes(16),
        )
        .build();
    assert_eq!(
        bash.exec("printf 'a b\\n' | fmt").await.unwrap().stdout,
        "a b\n"
    );
    for mode in ["-q", ""] {
        let result = bash
            .exec_with_options(
                &format!("fmt {mode}"),
                ExecOptions::new().stdin(amplified_input()),
            )
            .await;
        assert!(matches!(
            result,
            Err(Error::ResourceLimit(LimitExceeded::ExecutionBudget(_)))
        ));
        assert!(result.unwrap_err().to_string().contains("work units"));
    }
    assert_eq!(
        bash.exec("printf 'recovered\\n' | fmt")
            .await
            .unwrap()
            .stdout,
        "recovered\n"
    );
}

#[tokio::test]
async fn fmt_passthrough_lines_use_the_same_budget() {
    let mut bash = Bash::builder()
        .limits(ExecutionLimits::new().max_live_intermediate_bytes(8192))
        .build();
    // Prefix mismatch bypasses line breaking, but must still use the sink.
    let result = bash
        .exec_with_options(
            "fmt -p '#'",
            ExecOptions::new().stdin(format!("{}\n", "x".repeat(8192))),
        )
        .await;
    assert!(matches!(
        result,
        Err(Error::ResourceLimit(LimitExceeded::ExecutionBudget(_)))
    ));
    let good = bash
        .exec_with_options("fmt -p '#'", ExecOptions::new().stdin("raw\n\n# a b\n"))
        .await
        .unwrap();
    assert_eq!(good.stdout, "raw\n\n# a b\n");
}

#[tokio::test]
async fn fmt_below_budget_preserves_indent_through_pipe_and_redirect() {
    let mut bash = Bash::builder()
        .limits(
            ExecutionLimits::new()
                .max_stdout_bytes(16)
                .max_live_intermediate_bytes(256_000),
        )
        .build();
    let input = amplified_input();
    bash.fs()
        .write_file(Path::new("/input"), &input)
        .await
        .unwrap();
    let expected = format!("{}a\n", " ".repeat(256)).repeat(128);
    for mode in ["-q", ""] {
        let result = bash
            .exec(&format!("fmt {mode} /input | wc -c"))
            .await
            .unwrap();
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout.trim(), expected.len().to_string());
        let result = bash
            .exec(&format!("fmt {mode} /input > /output"))
            .await
            .unwrap();
        assert_eq!(result.exit_code, 0);
        assert_eq!(
            bash.fs().read_file(Path::new("/output")).await.unwrap(),
            expected.as_bytes()
        );
    }
}

#[tokio::test]
async fn fmt_missing_file_reports_error_and_formats_remaining_file() {
    let mut bash = Bash::new();
    bash.fs()
        .write_file(Path::new("/input"), b"a b\n")
        .await
        .unwrap();
    let result = bash.exec("fmt /missing /input").await.unwrap();
    assert_eq!(result.exit_code, 1);
    assert_eq!(result.stdout, "a b\n");
    assert_eq!(
        result.stderr,
        "fmt: cannot open '/missing' for reading: No such file or directory\n"
    );
    bashkit::testing::assert_no_leak(&result, "fmt missing file", &[]);
}

#[tokio::test]
async fn fmt_finite_shell_input_matches_real_bash() {
    use std::process::Command;
    // One short line per paragraph: GNU/BSD fmt and the vendored optimal-fit
    // engine agree here; multi-line break choices are covered by L-FMT-001.
    let script = "printf '  alpha beta\\n\\n  gamma delta\\n' | fmt -w 40";
    let expected = Command::new("bash")
        .args(["--noprofile", "--norc", "-c", script])
        .env("LC_ALL", "C")
        .output()
        .unwrap();
    assert!(expected.status.success());
    let mut bash = Bash::new();
    let actual = bash.exec(script).await.unwrap();
    assert_eq!(actual.stdout.as_bytes(), expected.stdout);
    assert_eq!(actual.stderr.as_bytes(), expected.stderr);
    assert_eq!(actual.exit_code, expected.status.code().unwrap());
}

#[tokio::test]
async fn fmt_api_cancellation_leaves_session_reusable() {
    use std::sync::atomic::Ordering;
    use std::time::Duration;
    let mut bash = Bash::new();
    let token = bash.cancellation_token();
    let cancel = token.clone();
    // Even without cancellation this finite payload emits only ~8 MiB.
    let input = format!("{}{}\n", " ".repeat(256), "a ".repeat(32_768));
    let worker = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(1));
        cancel.store(true, Ordering::Relaxed);
    });
    let result = bash
        .exec_with_options("fmt -q", ExecOptions::new().stdin(input))
        .await;
    worker.join().unwrap();
    assert!(result.unwrap_err().to_string().contains("cancelled"));
    token.store(false, Ordering::Relaxed);
    assert_eq!(
        bash.exec("printf 'a b\\n' | fmt").await.unwrap().stdout,
        "a b\n"
    );
}
