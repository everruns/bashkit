use bashkit::{
    Bash, Error, ExecutionLimits, ExecutionProfile, ExecutionProfileName, LimitExceeded,
};
use std::path::Path;
use std::process::Command;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

fn strip_timing(stderr: &str) -> String {
    stderr
        .lines()
        .filter(|line| {
            !line.is_empty()
                && !line.starts_with("real")
                && !line.starts_with("user")
                && !line.starts_with("sys")
        })
        .map(|line| format!("{line}\n"))
        .collect()
}

#[tokio::test]
async fn reserved_word_wraps_pipeline_group_and_function() {
    let mut bash = Bash::new();
    let result = bash
        .exec("set -o pipefail; f() { echo function; echo problem >&2; return 7; }; time { f | cat; }")
        .await
        .unwrap();

    assert_eq!(result.stdout, "function\n");
    assert_eq!(strip_timing(&result.stderr), "problem\n");
    assert_eq!(result.exit_code, 7);
}

#[tokio::test(start_paused = true)]
async fn posix_report_uses_virtual_monotonic_time_without_fake_cpu() {
    let mut bash = Bash::new();
    let result = bash.exec("time -p sleep 1.25").await.unwrap();

    assert_eq!(result.stdout, "");
    assert_eq!(
        result.stderr,
        "real 1.25\nuser unavailable\nsys unavailable\n"
    );
    assert_eq!(result.exit_code, 0);
}

#[tokio::test(start_paused = true)]
async fn gnu_format_reports_truthful_fields_and_bashkit_counters() {
    let mut bash = Bash::new();
    let result = bash
        .exec("time -f 'elapsed=%e status=%x cpu=%U rss=%M commands=%{commands} loops=%{loops} work=%{work_units}' -- sh -c 'for i in 1 2 3; do :; done; exit 4'")
        .await
        .unwrap();

    assert_eq!(result.stdout, "");
    assert_eq!(
        result.stderr,
        "elapsed=0.00 status=4 cpu=unavailable rss=unavailable commands=7 loops=3 work=18\n"
    );
    assert_eq!(result.exit_code, 4);
}

#[tokio::test(start_paused = true)]
async fn verbose_report_contains_only_available_or_explicitly_unavailable_data() {
    let mut bash = Bash::new();
    let result = bash.exec("time -v sleep 0.2").await.unwrap();

    assert!(result.stderr.contains("Elapsed (wall clock) time: 0.20\n"));
    assert!(result.stderr.contains("User CPU time: unavailable\n"));
    assert!(result.stderr.contains("System CPU time: unavailable\n"));
    assert!(
        result
            .stderr
            .contains("Maximum resident set size: unavailable\n")
    );
    assert!(result.stderr.contains("Bashkit commands: 1\n"));
    assert!(result.stderr.contains("Exit status: 0\n"));
}

#[tokio::test(start_paused = true)]
async fn output_and_append_use_vfs_without_redirecting_wrapped_streams() {
    let mut bash = Bash::new();
    let first = bash
        .exec("time -f 'first:%e' -o /tmp/time.txt sh -c 'echo out; echo err >&2; exit 3'")
        .await
        .unwrap();
    assert_eq!(first.stdout, "out\n");
    assert_eq!(first.stderr, "err\n");
    assert_eq!(first.exit_code, 3);

    let second = bash
        .exec("time -f 'second:%x' -a -o /tmp/time.txt true")
        .await
        .unwrap();
    assert_eq!(second.exit_code, 0);
    assert_eq!(
        bash.fs()
            .read_file(Path::new("/tmp/time.txt"))
            .await
            .unwrap(),
        b"first:0.00\nsecond:0\n"
    );
}

#[tokio::test]
async fn output_error_is_deterministic_and_does_not_hide_command_output() {
    let mut bash = Bash::new();
    let result = bash
        .exec("time -o /missing/report.txt sh -c 'echo ran; exit 9'")
        .await
        .unwrap();

    assert_eq!(result.stdout, "ran\n");
    assert_eq!(
        result.stderr,
        "time: cannot write output file '/missing/report.txt'\n"
    );
    assert_eq!(result.exit_code, 1);
}

#[tokio::test]
async fn malformed_options_and_formats_never_run_the_wrapped_command() {
    let mut bash = Bash::new();
    let bad_option = bash.exec("time --wat echo leaked").await.unwrap();
    assert_eq!(bad_option.stdout, "");
    assert_eq!(bad_option.stderr, "time: unrecognized option '--wat'\n");
    assert_eq!(bad_option.exit_code, 2);

    let bad_format = bash.exec("time -f '%Q' echo leaked").await.unwrap();
    assert_eq!(bad_format.stdout, "");
    assert_eq!(bad_format.stderr, "time: unsupported format field '%Q'\n");
    assert_eq!(bad_format.exit_code, 2);
}

#[tokio::test(start_paused = true)]
async fn hardened_profile_coarsens_elapsed_time() {
    let mut bash = Bash::builder()
        .profile(ExecutionProfile::named(ExecutionProfileName::Hardened))
        .build();
    let result = bash.exec("time -f '%e' sleep 0.149").await.unwrap();

    assert_eq!(result.stderr, "0.10\n");
}

#[tokio::test]
async fn time_does_not_refresh_resource_budget() {
    let limits = ExecutionLimits::new().max_commands(100).max_work_units(4);
    let mut bash = Bash::builder().limits(limits).build();
    let result = bash.exec("time echo $(echo nested)").await;

    assert!(matches!(
        result,
        Err(Error::ResourceLimit(LimitExceeded::ExecutionBudget(_)))
    ));
}

#[tokio::test(start_paused = true)]
async fn wrapped_timeout_status_and_elapsed_time_are_reported() {
    let mut bash = Bash::new();
    let result = bash
        .exec("time -f 'elapsed=%e status=%x' timeout 0.1 sleep 5")
        .await
        .unwrap();

    assert_eq!(result.stderr, "elapsed=0.10 status=124\n");
    assert_eq!(result.exit_code, 124);
}

#[tokio::test]
async fn output_file_does_not_bypass_report_size_limit() {
    let limits = ExecutionLimits::new().max_stderr_bytes(32);
    let mut bash = Bash::builder().limits(limits).build();
    bash.fs()
        .write_file(Path::new("/tmp/report"), b"sentinel\n")
        .await
        .unwrap();

    let result = bash
        .exec("time -o /tmp/report -f '0123456789012345678901234567890123456789' true")
        .await
        .unwrap();
    assert_eq!(result.exit_code, 1);
    assert_eq!(
        bash.fs().read_file(Path::new("/tmp/report")).await.unwrap(),
        b"sentinel\n"
    );
}

#[tokio::test]
async fn streaming_emits_wrapped_output_and_the_final_report_once() {
    let chunks = Arc::new(Mutex::new((String::new(), String::new())));
    let sink = Arc::clone(&chunks);
    let mut bash = Bash::new();
    let result = bash
        .exec_streaming(
            "time -f 'status=%x' sh -c 'echo out; echo err >&2; exit 3'",
            Box::new(move |stdout, stderr| {
                let mut chunks = sink.lock().unwrap();
                chunks.0.push_str(&stdout.to_string());
                chunks.1.push_str(&stderr.to_string());
            }),
        )
        .await
        .unwrap();

    assert_eq!(result.exit_code, 3);
    let chunks = chunks.lock().unwrap();
    assert_eq!(chunks.0, "out\n");
    assert_eq!(chunks.1, "err\nstatus=3\n");
}

#[tokio::test]
async fn time_obeys_pre_cancelled_request_and_writes_no_report() {
    let mut bash = Bash::new();
    bash.cancellation_token().store(true, Ordering::Relaxed);
    let result = bash.exec("time -o /tmp/report true").await;

    assert_eq!(result.unwrap_err().to_string(), "execution cancelled");
    assert!(!bash.fs().exists(Path::new("/tmp/report")).await.unwrap());
}

#[tokio::test]
async fn time_errexit_uses_wrapped_status() {
    let mut bash = Bash::new();
    let result = bash.exec("set -e; time false; echo leaked").await.unwrap();

    assert_eq!(result.stdout, "");
    assert_eq!(result.exit_code, 1);
}

#[tokio::test(start_paused = true)]
async fn nested_time_and_compound_redirection_keep_report_boundaries() {
    let mut bash = Bash::new();
    let nested = bash
        .exec("time -f 'outer:%x' time -f 'inner:%x' sleep 0.1")
        .await
        .unwrap();
    assert_eq!(nested.stderr, "inner:0\nouter:0\n");

    let redirected = bash
        .exec("time -f 'report:%x' { echo visible; echo wrapped >&2; } 2>/tmp/all")
        .await
        .unwrap();
    assert_eq!(redirected.stdout, "visible\n");
    assert_eq!(redirected.stderr, "report:0\n");
    assert_eq!(
        bash.fs().read_file(Path::new("/tmp/all")).await.unwrap(),
        b"wrapped\n"
    );

    let outer = bash
        .exec("{ time -f 'outer:%x' true; } 2>/tmp/outer")
        .await
        .unwrap();
    assert_eq!(outer.stderr, "");
    assert_eq!(
        bash.fs().read_file(Path::new("/tmp/outer")).await.unwrap(),
        b"outer:0\n"
    );
}

#[tokio::test]
async fn invalid_format_preserves_existing_output_file_atomically() {
    let mut bash = Bash::new();
    bash.fs()
        .write_file(Path::new("/tmp/report"), b"sentinel\n")
        .await
        .unwrap();

    let result = bash
        .exec("time -o /tmp/report -f '%Q' echo must-not-run")
        .await
        .unwrap();
    assert_eq!(result.exit_code, 2);
    assert_eq!(result.stdout, "");
    assert_eq!(
        bash.fs().read_file(Path::new("/tmp/report")).await.unwrap(),
        b"sentinel\n"
    );

    let valid = bash
        .exec("time -o /tmp/report -f 'replacement:%x' true")
        .await
        .unwrap();
    assert_eq!(valid.exit_code, 0);
    assert_eq!(
        bash.fs().read_file(Path::new("/tmp/report")).await.unwrap(),
        b"replacement:0\n"
    );
}

#[tokio::test]
async fn failed_atomic_replace_cleans_temporary_report() {
    let mut bash = Bash::new();
    bash.fs()
        .mkdir(Path::new("/tmp/report-dir"), false)
        .await
        .unwrap();

    let result = bash
        .exec("time -o /tmp/report-dir -f '%x' true")
        .await
        .unwrap();
    assert_eq!(result.exit_code, 1);
    assert_eq!(
        result.stderr,
        "time: cannot write output file '/tmp/report-dir'\n"
    );
    assert!(
        bash.fs()
            .stat(Path::new("/tmp/report-dir"))
            .await
            .unwrap()
            .file_type
            .is_dir()
    );
    assert!(
        bash.fs()
            .read_dir(Path::new("/tmp"))
            .await
            .unwrap()
            .iter()
            .all(|entry| !entry.name.starts_with(".bashkit-time-"))
    );
}

#[tokio::test]
async fn reserved_word_matches_real_bash_stream_and_status_semantics() {
    let script = "set -o pipefail; f() { echo out; echo err >&2; return 6; }; time f | cat";
    let host = Command::new("bash")
        .args(["--noprofile", "--norc", "-c", script])
        .output()
        .expect("real bash must be available for compatibility tests");

    let mut bash = Bash::new();
    let result = bash.exec(script).await.unwrap();
    assert_eq!(result.stdout.as_bytes(), host.stdout);
    assert_eq!(strip_timing(&result.stderr), "err\n");
    assert!(String::from_utf8_lossy(&host.stderr).contains("err\n"));
    assert_eq!(result.exit_code, host.status.code().unwrap());
}

#[tokio::test]
async fn gnu_time_matches_wrapped_stream_and_status_when_available() {
    let version = Command::new("/usr/bin/time").arg("--version").output();
    let Ok(version) = version else { return };
    if !String::from_utf8_lossy(&version.stdout).contains("GNU time") {
        return;
    }
    let host = Command::new("/usr/bin/time")
        .args([
            "-f",
            "status=%x",
            "sh",
            "-c",
            "echo out; echo err >&2; exit 6",
        ])
        .output()
        .unwrap();

    let mut bash = Bash::new();
    let result = bash
        .exec("time -f 'status=%x' -- sh -c 'echo out; echo err >&2; exit 6'")
        .await
        .unwrap();
    assert_eq!(result.stdout.as_bytes(), host.stdout);
    assert_eq!(result.stderr.as_bytes(), host.stderr);
    assert_eq!(result.exit_code, host.status.code().unwrap());
}
