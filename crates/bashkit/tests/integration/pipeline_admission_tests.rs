//! Ordinary two-stage compatibility under producer admission; no stress inputs.
use bashkit::{Bash, ExecOptions, ExecutionLimits};

#[tokio::test]
async fn two_stage_pipeline_preserves_exports_status_and_isolation() {
    let script = "export DEMO=parent; { export DEMO=child; printf '%s\\n' \"$DEMO\"; } | cat; printf '%s %s\\n' \"$DEMO\" \"${PIPESTATUS[*]}\"";
    let mut bash = Bash::builder()
        .limits(ExecutionLimits::new().max_background_jobs(1))
        .build();
    let result = bash.exec(script).await.unwrap();
    assert_eq!(result.stdout, "child\nparent 0 0\n");
    assert!(result.stderr.is_empty());
    assert_eq!(result.exit_code, 0);
    let oracle = std::process::Command::new("bash")
        .args(["--noprofile", "--norc", "-c", script])
        .env_clear()
        .output()
        .unwrap();
    assert!(oracle.status.success());
    assert_eq!(oracle.stdout, result.stdout.as_bytes());
    assert_eq!(oracle.stderr, result.stderr.as_bytes());
}

#[tokio::test]
async fn sequential_two_stage_pipeline_accepts_zero_job_slots() {
    let mut bash = Bash::builder()
        .concurrent_jobs(false)
        .limits(ExecutionLimits::new().max_background_jobs(0))
        .build();
    let result = bash.exec("{ printf 'ok\\n'; } | cat").await.unwrap();
    assert_eq!(result.stdout, "ok\n");
    assert_eq!(result.exit_code, 0);
}

#[tokio::test]
async fn two_stage_lastpipe_retains_final_stage_assignment() {
    let mut bash = Bash::builder()
        .limits(ExecutionLimits::new().max_background_jobs(1))
        .build();
    let result = bash.exec("shopt -s lastpipe; { printf 'ok\\n'; } | read -r value; printf '%s %s\\n' \"$value\" \"${PIPESTATUS[*]}\"").await.unwrap();
    assert_eq!(result.stdout, "ok 0 0\n");
    assert!(result.stderr.is_empty());
}

#[tokio::test]
async fn two_stage_pipeline_preserves_supplied_stdin_and_session_reuse() {
    let mut bash = Bash::builder()
        .limits(ExecutionLimits::new().max_background_jobs(1))
        .build();
    let result = bash
        .exec_with_options("{ cat; } | cat", ExecOptions::new().stdin("input\n"))
        .await
        .unwrap();
    assert_eq!(result.stdout, "input\n");
    assert_eq!(result.exit_code, 0);
    assert!(result.stderr.is_empty());
    let next = bash.exec("printf after").await.unwrap();
    assert_eq!(next.stdout, "after");
}
