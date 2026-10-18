//! Streaming invariants across builtin, descriptor and substitution boundaries.

use bashkit::{
    Bash, Command, CommandContext, CommandStatus, ExecutionBudgetExceeded, ExecutionLimits,
    LimitExceeded, async_trait,
};
use std::sync::Arc;
use tokio::sync::Notify;

struct Chunks;

#[async_trait]
impl Command for Chunks {
    async fn execute(&self, ctx: CommandContext<'_>) -> bashkit::Result<CommandStatus> {
        let count: usize = ctx.args[0].parse().unwrap();
        for _ in 0..count {
            ctx.stdout.write_all(&[b'x'; 1024]).await?;
        }
        ctx.stdout.write_all(b"\n").await?;
        Ok(CommandStatus::success())
    }
}

#[tokio::test]
async fn streaming_builtin_input_consumes_shared_budget() {
    for filter in [
        "cat",
        "grep -q x",
        "grep absent",
        "tr x y",
        "cat -n",
        "tr -s x",
    ] {
        let mut bash = Bash::builder()
            .limits(ExecutionLimits::new().max_aggregate_input_bytes(16_384))
            .command("chunks", Chunks)
            .build();
        let script = format!("chunks 32 | {filter}");
        let result = bash.execute(&script).await;
        assert!(
            matches!(
                result,
                Err(bashkit::Error::ResourceLimit(
                    LimitExceeded::ExecutionBudget(ExecutionBudgetExceeded::InputBytes { .. })
                ))
            ),
            "{script}: {result:?}"
        );
        assert_eq!(
            bash.execute("echo recovered")
                .capture(32)
                .await
                .unwrap()
                .stdout,
            "recovered\n"
        );
    }
}

#[tokio::test]
async fn streaming_records_and_collectors_admit_memory_before_growth() {
    for filter in ["grep -q x", "grep absent", "cat -n", "tr -s x"] {
        let mut bash = Bash::builder()
            .limits(ExecutionLimits::new().max_live_intermediate_bytes(65_536))
            .command("chunks", Chunks)
            .build();
        let script = format!("chunks 128 | {filter}");
        let result = bash.execute(&script).await;
        assert!(
            matches!(
                result,
                Err(bashkit::Error::ResourceLimit(
                    LimitExceeded::ExecutionBudget(ExecutionBudgetExceeded::LiveBytes { .. })
                ))
            ),
            "{script}: {result:?}"
        );
        assert_eq!(
            bash.execute("echo recovered")
                .capture(32)
                .await
                .unwrap()
                .stdout,
            "recovered\n"
        );
    }
    // Total volume may exceed the live-storage cap when no record accumulates.
    let mut bash = Bash::builder()
        .limits(ExecutionLimits::new().max_live_intermediate_bytes(65_536))
        .command("chunks", Chunks)
        .build();
    assert_eq!(bash.execute("chunks 128 | cat").await.unwrap().exit_code, 0);
}

#[tokio::test]
async fn saved_output_descriptors_reach_sinks_and_capture() {
    for (script, stdout, stderr) in [
        ("exec 3>&1 >/out; echo hi >&3", "hi\n", ""),
        ("exec 3>&2 2>/out; echo hi >&3", "", "hi\n"),
        (
            "exec 3>&1 >/out; printf hi >&3; printf file; cat /out >&3",
            "hifile",
            "",
        ),
    ] {
        let mut bash = Bash::new();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let result = bash
            .execute(script)
            .stdout(&mut out)
            .stderr(&mut err)
            .capture(64)
            .await
            .unwrap();
        assert_eq!(result.exit_code, 0, "{script}");
        assert_eq!(out, stdout.as_bytes(), "{script}");
        assert_eq!(err, stderr.as_bytes(), "{script}");
        assert_eq!(result.stdout.as_bytes(), out, "{script}");
        assert_eq!(result.stderr.as_bytes(), err, "{script}");
    }
}

#[tokio::test]
async fn process_substitution_preserves_bytes_in_buffered_and_live_execution() {
    for script in [
        "cat <(printf '\\377\\000A')",
        "cat <(printf '\\377'; printf '\\000A')",
        "printf '\\377\\000A' > >(cat)",
    ] {
        let oracle = std::process::Command::new("bash")
            .args(["--noprofile", "--norc", "-c", script])
            .output()
            .unwrap();
        assert!(oracle.status.success());
        for live in [false, true] {
            let mut bash = Bash::new();
            let result = if live {
                bash.execute(script).capture(64).await
            } else {
                bash.exec(script).await
            }
            .unwrap();
            assert_eq!(
                result.stdout.as_bytes(),
                oracle.stdout,
                "{script}, live={live}"
            );
            assert_eq!(result.stderr.as_bytes(), oracle.stderr);
            assert_eq!(result.exit_code, 0);
        }
    }
}

struct Gated(Arc<Notify>);

struct DelayedOutput {
    started: Arc<Notify>,
    release: Arc<Notify>,
}

#[async_trait]
impl Command for DelayedOutput {
    async fn execute(&self, ctx: CommandContext<'_>) -> bashkit::Result<CommandStatus> {
        ctx.stdout.write_all(b"first").await?;
        self.started.notify_one();
        self.release.notified().await;
        ctx.stdout.write_all(b"last\n").await?;
        ctx.stderr.write_all(b"diagnostic\n").await?;
        Ok(CommandStatus::success())
    }
}

#[tokio::test]
async fn background_completion_drains_to_live_sinks_and_capture_once() {
    let started = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let mut bash = Bash::builder()
        .command(
            "delayed",
            DelayedOutput {
                started: started.clone(),
                release: release.clone(),
            },
        )
        .build();
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    let execution = async {
        bash.execute("delayed &")
            .stdout(&mut stdout)
            .stderr(&mut stderr)
            .capture(64)
            .await
    };
    let unblock = async {
        started.notified().await;
        release.notify_one();
    };
    let (result, ()) = tokio::time::timeout(std::time::Duration::from_secs(2), async {
        tokio::join!(execution, unblock)
    })
    .await
    .unwrap();
    let result = result.unwrap();
    assert_eq!(result.exit_code, 0);
    assert_eq!(stdout, b"firstlast\n");
    assert_eq!(stderr, b"diagnostic\n");
    assert_eq!(result.stdout.as_bytes(), stdout);
    assert_eq!(result.stderr.as_bytes(), stderr);
}

#[async_trait]
impl Command for Gated {
    async fn execute(&self, ctx: CommandContext<'_>) -> bashkit::Result<CommandStatus> {
        ctx.stdout.write_all(b"first\n").await?;
        self.0.notified().await;
        ctx.stdout.write_all(b"last\n").await?;
        Ok(CommandStatus::success())
    }
}

#[tokio::test]
async fn descriptor_redirects_keep_final_filters_live() {
    for filter in [
        "cat 2>&1",
        "grep first 2>&1",
        "tr a-z A-Z 2>&1",
        "cat 2>/errors",
        "cat 3>&1 >&3",
    ] {
        let release = Arc::new(Notify::new());
        let sink_release = release.clone();
        let mut bash = Bash::builder().command("gated", Gated(release)).build();
        let script = format!("gated | {filter}");
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            bash.execute(&script).capture(64).stdout_fn(move |bytes| {
                assert!(!bytes.is_empty());
                sink_release.notify_one();
                async { Ok(()) }
            }),
        )
        .await
        .expect("redirect buffered output until gated producer completion")
        .unwrap();
        assert_eq!(result.exit_code, 0, "{script}");
        assert!(result.stderr.is_empty(), "{script}");
    }
}

#[tokio::test]
async fn redirected_streaming_utilities_match_bash() {
    for script in [
        "printf 'a\\nb\\n' | cat 2>&1",
        "printf 'a\\nb\\n' | cat 1>&2",
        "printf 'a\\nb\\n' | cat >out 2>&1; cat out",
        "printf 'a\\nb\\n' | cat -n >out; cat out",
        "printf 'a\\nb\\n' | grep -c a >out; cat out",
        "printf 'a\\nb\\n' | tr a-z A-Z 3>&1 >&3",
        "seq 3 >out; cat out",
        "printf 'a\\nb\\n' | cat 3>&1 1>&2 2>&3",
    ] {
        let scratch = tempfile::tempdir().unwrap();
        let oracle = std::process::Command::new("bash")
            .args(["--noprofile", "--norc", "-c", script])
            .current_dir(scratch.path())
            .output()
            .unwrap();
        let mut bash = Bash::new();
        let result = bash.execute(script).capture(1024).await.unwrap();
        assert_eq!(result.stdout.as_bytes(), oracle.stdout, "{script}");
        assert_eq!(result.stderr.as_bytes(), oracle.stderr, "{script}");
        assert_eq!(result.exit_code, oracle.status.code().unwrap(), "{script}");
    }
}

#[tokio::test]
async fn saved_descriptor_delivery_respects_output_caps_and_sink_failure() {
    let script = "exec 3>&1 >/out; printf abcdef >&3";
    let mut bash = Bash::builder()
        .limits(ExecutionLimits::new().max_stdout_bytes(3))
        .build();
    let mut delivered = Vec::new();
    let result = bash
        .execute(script)
        .stdout(&mut delivered)
        .capture(2)
        .await
        .unwrap();
    assert_eq!(delivered, b"abc");
    assert_eq!(result.stdout.as_bytes(), b"ab");
    assert!(result.stdout_truncated);
    let mut bash = Bash::new();
    let result = bash
        .execute(script)
        .stdout_fn(|_| async { Err(std::io::Error::other("sink refused output").into()) })
        .await;
    assert!(matches!(result, Err(bashkit::Error::Io(_))));
    assert_eq!(
        bash.execute("exec 1>&3; echo recovered")
            .capture(32)
            .await
            .unwrap()
            .stdout,
        "recovered\n"
    );
}

#[tokio::test]
async fn streaming_write_errors_remain_command_statuses() {
    let mut bash = Bash::new();
    let result = bash
        .execute("printf x | cat 1>&-; echo status=$?")
        .capture(1024)
        .await
        .unwrap();
    assert_eq!(result.stdout, "status=1\n");
    assert!(result.stderr.contains("Bad file descriptor"));
    let result = bash
        .execute("printf '' | cat 1>&-")
        .capture(64)
        .await
        .unwrap();
    assert_eq!(result.exit_code, 0);
    assert!(result.stderr.is_empty());
    let result = bash.execute("seq 0 1>&-").capture(64).await.unwrap();
    assert_eq!(result.exit_code, 0);
    assert!(result.stderr.is_empty());
}
