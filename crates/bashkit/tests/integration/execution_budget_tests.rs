#[cfg(feature = "jq")]
use bashkit::ExecOptions;
use bashkit::{
    Bash, Builtin, BuiltinContext, Error, ExecResult, ExecutionBudget, ExecutionLimits,
    LimitExceeded, SessionLimits, async_trait,
};
use std::path::Path;
use std::sync::{Arc, Mutex};

fn assert_budget_exhausted(result: bashkit::Result<bashkit::ExecResult>) {
    assert!(
        matches!(
            result,
            Err(Error::ResourceLimit(LimitExceeded::ExecutionBudget(_)))
        ),
        "expected shared execution budget exhaustion, got {result:?}"
    );
}

#[tokio::test]
/// TM-DOS-111: the `$(<file)` fast path is still an executed command.
async fn command_substitution_file_read_respects_command_limit() {
    let limits = ExecutionLimits::new().max_commands(1);
    let mut bash = Bash::builder().limits(limits).build();
    bash.fs()
        .write_file(Path::new("/f"), b"data")
        .await
        .unwrap();

    let result = bash.exec(": \"$(</f)\"").await;
    assert!(
        matches!(
            result,
            Err(Error::ResourceLimit(LimitExceeded::MaxCommands(1)))
        ),
        "expected command limit exhaustion, got {result:?}"
    );
}

#[tokio::test]
/// TM-DOS-111: shortcut commands count toward the cumulative session limit.
async fn command_substitution_file_read_respects_session_command_limit() {
    let mut bash = Bash::builder()
        .limits(ExecutionLimits::new().max_commands(100))
        .session_limits(
            SessionLimits::new()
                .max_total_commands(1)
                .max_exec_calls(100),
        )
        .build();
    bash.fs()
        .write_file(Path::new("/f"), b"data")
        .await
        .unwrap();

    let error = bash.exec(": \"$(</f)\"").await.unwrap_err().to_string();
    assert!(error.contains("session command limit"), "{error}");
}

#[tokio::test]
/// TM-DOS-111: the shortcut consumes the same shared work unit as a command.
async fn command_substitution_file_read_respects_work_limit() {
    let limits = ExecutionLimits::new()
        .max_commands(100)
        .max_work_units(1)
        .max_aggregate_input_bytes(1_000);
    let mut bash = Bash::builder().limits(limits).build();
    bash.fs()
        .write_file(Path::new("/f"), b"data")
        .await
        .unwrap();

    assert_budget_exhausted(bash.exec(": \"$(</f)\"").await);
}

#[tokio::test]
/// TM-DOS-111: file-backed substitution text is a live intermediate.
async fn command_substitution_file_read_respects_live_byte_limit() {
    let limits = ExecutionLimits::new()
        .max_commands(100)
        .max_work_units(10_000)
        .max_live_intermediate_bytes(4);
    let mut bash = Bash::builder().limits(limits).build();
    bash.fs()
        .write_file(Path::new("/f"), b"12345")
        .await
        .unwrap();

    assert_budget_exhausted(bash.exec(": \"$(</f)\"").await);
}

#[tokio::test]
/// TM-DOS-111: sibling substitutions cannot each reuse the full live budget.
async fn command_substitution_file_reads_share_live_byte_limit() {
    let limits = ExecutionLimits::new()
        .max_commands(100)
        .max_work_units(10_000)
        .max_live_intermediate_bytes(10);
    let mut bash = Bash::builder().limits(limits).build();
    bash.fs().write_file(Path::new("/f"), b"123").await.unwrap();

    assert_budget_exhausted(bash.exec(": \"$(</f)$(</f)$(</f)\"").await);
}

#[tokio::test]
/// TM-DOS-096: nested parsers/interpreters must share aggregate work.
async fn nested_command_substitutions_cannot_refresh_work_budget() {
    let limits = ExecutionLimits::new()
        .max_commands(100)
        .max_work_units(6)
        .max_aggregate_input_bytes(1_000);
    let mut bash = Bash::builder().limits(limits).build();

    assert_budget_exhausted(bash.exec("echo $(echo $(echo $(echo nested)))").await);
}

#[tokio::test]
/// TM-DOS-096: changing builtin subsystems in a pipeline cannot refresh input.
async fn mixed_pipeline_consumers_share_aggregate_input_budget() {
    let limits = ExecutionLimits::new()
        .max_commands(100)
        .max_work_units(10_000)
        .max_aggregate_input_bytes(80);
    let mut bash = Bash::builder().limits(limits).build();

    assert_budget_exhausted(
        bash.exec("printf 12345678901234567890 | cat | awk '{print}' | rg 1")
            .await,
    );
}

#[tokio::test]
/// TM-DOS-116: awk close/reopen cycles must not recycle getline input budget.
async fn awk_getline_close_cannot_recycle_input_budget() {
    use bashkit::FileSystem;
    use bashkit::InMemoryFs;
    use std::path::Path;

    let fs = Arc::new(InMemoryFs::new());
    // Exactly the per-file cap: reopening it used to make each 10 MB load
    // disappear from accounting as soon as close() dropped the cache entry.
    fs.write_file(Path::new("/tmp/data"), &vec![b'x'; 10_000_000])
        .await
        .unwrap();
    let limits = ExecutionLimits::new()
        .max_commands(100)
        .max_work_units(30_000_000)
        .max_aggregate_input_bytes(15_000_000);
    let mut bash = Bash::builder().fs(fs).limits(limits).build();

    assert_budget_exhausted(
        bash.exec(
            r#"awk 'BEGIN { for (i = 0; i < 2; i++) { getline x < "/tmp/data"; close("/tmp/data") } }'"#,
        )
        .await,
    );
}

#[tokio::test]
/// TM-DOS-096: the first exhaustion poisons all later descendants.
async fn a_poisoned_budget_stops_later_pipeline_stages() {
    let limits = ExecutionLimits::new()
        .max_commands(100)
        .max_work_units(8)
        .max_aggregate_input_bytes(10_000);
    let mut bash = Bash::builder().limits(limits).build();

    let result = bash
        .exec("printf first | awk '{print}'; echo must-not-run")
        .await;
    assert_budget_exhausted(result);
}

#[tokio::test]
async fn separate_host_requests_receive_fresh_budgets() {
    let limits = ExecutionLimits::new()
        .max_work_units(8)
        .max_aggregate_input_bytes(1_000);
    let mut bash = Bash::builder().limits(limits).build();

    assert_eq!(bash.exec("true").await.unwrap().exit_code, 0);
    assert_eq!(bash.exec("true").await.unwrap().exit_code, 0);
}

#[derive(Clone)]
struct CaptureBoundary(Arc<Mutex<Option<ExecutionBudget>>>);

#[async_trait]
impl Builtin for CaptureBoundary {
    async fn execute(&self, ctx: BuiltinContext<'_>) -> bashkit::Result<ExecResult> {
        *self.0.lock().unwrap() = ctx
            .execution_budget()
            .and_then(|budget| budget.try_with(Clone::clone).ok());
        Ok(ExecResult::ok("captured\n"))
    }
}

#[tokio::test]
/// TM-ISO-027: a request boundary closes even when a host retains a clone.
async fn completed_request_rejects_late_work() {
    let captured = Arc::new(Mutex::new(None));
    let mut bash = Bash::builder()
        .builtin("capture", Box::new(CaptureBoundary(captured.clone())))
        .build();

    assert_eq!(bash.exec("capture").await.unwrap().stdout, "captured\n");
    let old = captured.lock().unwrap().take().unwrap();
    assert_eq!(
        old.check().unwrap_err().to_string(),
        "execution budget exhausted: request closed"
    );
    assert_eq!(
        old.consume_work(1).unwrap_err().to_string(),
        "execution budget exhausted: request closed"
    );

    assert_eq!(bash.exec("echo reused").await.unwrap().stdout, "reused\n");
    assert_eq!(
        old.check().unwrap_err().to_string(),
        "execution budget exhausted: request closed"
    );
}

#[tokio::test]
/// TM-ISO-027: independently executing shells never share lifecycle state.
async fn concurrent_requests_have_independent_boundaries() {
    let left = Arc::new(Mutex::new(None));
    let right = Arc::new(Mutex::new(None));
    let mut left_bash = Bash::builder()
        .builtin("capture", Box::new(CaptureBoundary(left.clone())))
        .build();
    let mut right_bash = Bash::builder()
        .builtin("capture", Box::new(CaptureBoundary(right.clone())))
        .build();

    let (left_result, right_result) = tokio::join!(
        left_bash.exec("capture; echo left"),
        right_bash.exec("capture; echo right")
    );
    assert_eq!(left_result.unwrap().stdout, "captured\nleft\n");
    assert_eq!(right_result.unwrap().stdout, "captured\nright\n");
    let left = left.lock().unwrap().take().unwrap();
    let right = right.lock().unwrap().take().unwrap();
    assert_eq!(
        left.check().unwrap_err().to_string(),
        "execution budget exhausted: request closed"
    );
    assert_eq!(
        right.check().unwrap_err().to_string(),
        "execution budget exhausted: request closed"
    );
}

#[cfg(feature = "jq")]
#[tokio::test]
/// TM-DOS-096: jq generator work is charged to the host request.
async fn jq_generator_consumes_shared_work_budget() {
    let limits = ExecutionLimits::new()
        .max_work_units(500)
        .max_aggregate_input_bytes(100_000);
    let mut bash = Bash::builder().limits(limits).build();

    assert_budget_exhausted(bash.exec("jq -n 'range(0; 10000)'").await);
}

#[cfg(feature = "jq")]
#[tokio::test]
/// TM-DOS-100: jq must reserve normalized input before control expansion.
async fn jq_control_normalization_respects_live_memory_budget() {
    let limits = ExecutionLimits::new()
        .max_work_units(10_000)
        .max_aggregate_input_bytes(10_000)
        .max_live_intermediate_bytes(8);
    let mut bash = Bash::builder().limits(limits).build();

    assert_budget_exhausted(
        bash.exec_with_options("jq -c .", ExecOptions::new().stdin("\"a\n\""))
            .await,
    );
}

#[cfg(feature = "jq")]
#[tokio::test]
/// TM-DOS-110: stream preprocessing must obey the intermediate limit even for empty.
async fn jq_stream_long_key_respects_memory_limit() {
    let mut bash = Bash::builder()
        .limits(ExecutionLimits::new().max_live_intermediate_bytes(32_768))
        .build();
    let input = format!("{{\"{}\":[null,null]}}", "k".repeat(65_536));
    bash.fs()
        .write_file(Path::new("/input.json"), input.as_bytes())
        .await
        .unwrap();
    let result = bash.exec("jq --stream empty /input.json").await.unwrap();
    assert_eq!(result.exit_code, 5);
    assert!(
        result
            .stderr
            .contains("value size limit (32768 bytes) exceeded"),
        "{}",
        result.stderr
    );
    assert!(result.stdout.is_empty());
    assert_eq!(
        bash.exec("echo recovered").await.unwrap().stdout,
        "recovered\n"
    );
}

#[cfg(feature = "jq")]
#[tokio::test]
/// TM-DOS-110: slurping events retains them and must meter their bodies.
async fn jq_stream_slurp_respects_memory_limit() {
    let mut bash = Bash::builder()
        .limits(ExecutionLimits::new().max_live_intermediate_bytes(65_536))
        .build();
    let input = format!(
        "{{\"{}\":[{}]}}",
        "k".repeat(1024),
        vec!["null"; 2000].join(",")
    );
    let result = bash
        .exec_with_options("jq --stream --slurp empty", ExecOptions::new().stdin(input))
        .await
        .unwrap();
    assert_eq!(result.exit_code, 5);
    assert!(
        result
            .stderr
            .contains("value size limit (65536 bytes) exceeded"),
        "{}",
        result.stderr
    );
    assert!(result.stdout.is_empty());
}

#[cfg(feature = "jq")]
#[tokio::test]
/// Shared keys and lazy events keep many leaves below a small live limit.
async fn jq_stream_many_leaves_fit_small_memory_limit() {
    let mut bash = Bash::builder()
        .limits(ExecutionLimits::new().max_live_intermediate_bytes(65_536))
        .build();
    let input = format!(
        "{{\"{}\":[{}]}}",
        "k".repeat(1024),
        vec!["null"; 2000].join(",")
    );
    let result = bash
        .exec_with_options(
            "jq -nc --stream 'reduce inputs as $event (0; . + 1)'",
            ExecOptions::new().stdin(input),
        )
        .await
        .unwrap();
    assert_eq!(result.exit_code, 0, "{}", result.stderr);
    assert_eq!(result.stdout, "2002\n");
}

#[cfg(feature = "jq")]
#[tokio::test]
/// Non-emitting filters must charge traversal, beyond input normalization.
async fn jq_stream_empty_consumes_shared_work_budget() {
    let input = format!("[{}]", vec!["null"; 1000].join(","));
    let mut bash = Bash::builder()
        .limits(ExecutionLimits::new().max_work_units(input.len() as u64 + 500))
        .build();
    assert_budget_exhausted(
        bash.exec_with_options("jq --stream empty", ExecOptions::new().stdin(input))
            .await,
    );
}

#[cfg(feature = "python")]
#[tokio::test]
/// TM-DOS-096: separate Python entries cannot each claim a fresh VM allowance.
async fn repeated_python_entries_share_runtime_admission_budget() {
    let limits = ExecutionLimits::new()
        .max_work_units(1_500_000)
        .max_aggregate_input_bytes(100_000);
    let mut bash = Bash::builder()
        .limits(limits)
        .python()
        .env("BASHKIT_ALLOW_INPROCESS_PYTHON", "1")
        .build();

    assert_budget_exhausted(
        bash.exec("python -c 'print(1)'; python -c 'print(2)'")
            .await,
    );
}

#[cfg(feature = "typescript")]
#[tokio::test]
/// TM-DOS-096: separate TypeScript entries cannot refresh allocation fuel.
async fn repeated_typescript_entries_share_runtime_admission_budget() {
    let limits = ExecutionLimits::new()
        .max_work_units(1_500_000)
        .max_aggregate_input_bytes(100_000);
    let mut bash = Bash::builder().limits(limits).typescript().build();

    assert_budget_exhausted(bash.exec("ts -c '1 + 1'; ts -c '2 + 2'").await);
}

#[cfg(feature = "sqlite")]
#[tokio::test]
/// TM-DOS-096: SQLite VM steps consume the request's remaining work.
async fn sqlite_steps_consume_shared_work_budget() {
    let limits = ExecutionLimits::new()
        .max_work_units(600)
        .max_aggregate_input_bytes(100_000);
    let mut bash = Bash::builder()
        .limits(limits)
        .sqlite()
        .env("BASHKIT_ALLOW_INPROCESS_SQLITE", "1")
        .build();

    let sql = "SELECT 1;".repeat(400);
    assert_budget_exhausted(bash.exec(&format!("sqlite :memory: '{sql}'")).await);
}

#[cfg(feature = "sqlite")]
#[tokio::test]
/// TM-SQL-014: work done inside a single `Statement::step()` is charged too.
///
/// The rows of this recursive CTE are swallowed by the aggregate, so the query
/// never returns from its first step. Without the VM progress handler the
/// request would spin forever with the budget untouched.
async fn unbounded_recursive_cte_consumes_shared_work_budget() {
    let limits = ExecutionLimits::new()
        .max_work_units(2_000)
        .max_aggregate_input_bytes(100_000);
    let mut bash = Bash::builder()
        .limits(limits)
        .sqlite()
        .env("BASHKIT_ALLOW_INPROCESS_SQLITE", "1")
        .build();

    assert_budget_exhausted(
        bash.exec(
            "sqlite :memory: 'WITH RECURSIVE r(n) AS (SELECT 1 UNION ALL SELECT n + 1 FROM r) \
             SELECT count(*) FROM r;'",
        )
        .await,
    );
}

#[tokio::test]
async fn cdpath_candidate_respects_live_byte_limit() {
    let limits = ExecutionLimits::new().max_live_intermediate_bytes(256);
    let mut bash = Bash::builder()
        .env("CDPATH", ":".repeat(256))
        .limits(limits)
        .build();
    // Bounded reproduction: the old collector allocates only about 25 KiB.
    assert_budget_exhausted(bash.exec(&format!("cd {}", "x".repeat(64))).await);
    assert_eq!(bash.exec("pwd").await.unwrap().exit_code, 0);
}

#[tokio::test]
async fn cdpath_search_respects_work_limit() {
    let limits = ExecutionLimits::new().max_work_units(64);
    let mut bash = Bash::builder()
        .env("CDPATH", ":".repeat(256))
        .limits(limits)
        .build();
    assert_budget_exhausted(bash.exec("cd missing").await);
}

#[tokio::test]
async fn cdpath_script_assignment_cannot_bypass_live_byte_limit() {
    let limits = ExecutionLimits::new().max_live_intermediate_bytes(1_024);
    let mut bash = Bash::builder().limits(limits).build();
    let script = format!("CDPATH={}\ncd {}", ":".repeat(256), "x".repeat(64));
    assert_budget_exhausted(bash.exec(&script).await);
}

#[tokio::test]
async fn cdpath_releases_failed_candidate_workspace() {
    let limits = ExecutionLimits::new().max_live_intermediate_bytes(1_024);
    let mut bash = Bash::builder()
        .env("CDPATH", ":".repeat(256))
        .limits(limits)
        .build();
    let result = bash.exec("cd missing").await.unwrap();
    assert_eq!(result.exit_code, 1);
    assert!(result.stderr.contains("No such file or directory"));
}

#[tokio::test]
async fn cdpath_stops_at_first_hit_under_small_budgets() {
    let limits = ExecutionLimits::new()
        .max_live_intermediate_bytes(1_024)
        .max_work_units(64);
    let mut bash = Bash::builder()
        .env("CDPATH", format!("/tmp:{}", "missing:".repeat(1_024)))
        .limits(limits)
        .build();
    bash.fs()
        .mkdir(Path::new("/tmp/target"), false)
        .await
        .unwrap();
    let result = bash.exec("cd target; pwd").await.unwrap();
    assert_eq!(result.exit_code, 0);
    assert_eq!(result.stdout, "/tmp/target\n/tmp/target\n");
}

#[tokio::test]
async fn cdpath_search_yields_to_cancellation() {
    use std::sync::atomic::Ordering;
    let mut bash = Bash::builder().env("CDPATH", ":".repeat(1_024)).build();
    let token = bash.cancellation_token();
    let cancel = async {
        for _ in 0..3 {
            tokio::task::yield_now().await;
        }
        token.store(true, Ordering::Relaxed);
    };
    let (result, ()) = tokio::join!(bash.exec("cd missing"), cancel);
    assert!(
        result.is_err(),
        "search completed without observing cancellation"
    );
    assert!(result.unwrap_err().to_string().contains("cancelled"));
}

#[tokio::test]
async fn indexed_array_pending_values_share_live_byte_limit() {
    for assignment in [
        "a=([0]=$v [0]=$v [0]=$v [0]=$v)",
        "declare -a a=([0]=$v [0]=$v [0]=$v [0]=$v)",
        "a+=([0]=$v [0]=$v [0]=$v [0]=$v)",
        "a=(\"$v\" \"$v\" \"$v\" \"$v\")",
        "a=(\"$v\"{1..4})",
        "a=([0]=$v$v$v)",
    ] {
        let mut bash = Bash::builder()
            .limits(ExecutionLimits::new().max_live_intermediate_bytes(8_192))
            .build();
        bash.exec(&format!("v='{}'; a=(original)", "x".repeat(4_096)))
            .await
            .unwrap();
        assert_budget_exhausted(bash.exec(assignment).await);
        assert_eq!(
            bash.exec("echo ${a[0]}").await.unwrap().stdout,
            "original\n"
        );
        assert_eq!(bash.exec("echo reused").await.unwrap().stdout, "reused\n");
    }
}

#[tokio::test]
async fn indexed_array_pending_keys_share_live_byte_limit() {
    let mut bash = Bash::builder()
        .limits(ExecutionLimits::new().max_live_intermediate_bytes(8_192))
        .build();
    // Leading zeroes keep every subscript valid and small.
    bash.exec(&format!("k='{}'", "0".repeat(4_096)))
        .await
        .unwrap();
    assert_budget_exhausted(bash.exec("a=([$k]=x [$k]=y [$k]=z)").await);
}

#[tokio::test]
async fn indexed_array_pending_storage_is_released_between_assignments() {
    let mut bash = Bash::builder()
        .limits(ExecutionLimits::new().max_live_intermediate_bytes(8_192))
        .build();
    let result = bash
        .exec(&format!(
            "v='{}'; a=([0]=$v); a=([0]=$v); a=([0]=$v); echo ${{#a[0]}}",
            "x".repeat(4_096)
        ))
        .await
        .unwrap();
    assert_eq!(result.stdout, "4096\n");
}

#[tokio::test]
async fn indexed_array_budget_preserves_expansion_order() {
    let script = "a=(old); a=([0]=1+2 [a[0]]=$a [0]+=x); printf '%s\\n' \"${a[0]}\" \"${a[3]}\"; a+=([0]=new [1]=$a); printf '%s\\n' \"${a[1]}\"";
    let expected = std::process::Command::new("bash")
        .args(["--noprofile", "--norc", "-c", script])
        .output()
        .unwrap();
    assert!(expected.status.success());
    let mut bash = Bash::builder()
        .limits(ExecutionLimits::new().max_live_intermediate_bytes(8_192))
        .build();
    let result = bash.exec(script).await.unwrap();
    assert_eq!(result.exit_code, 0);
    assert_eq!(result.stdout.as_bytes(), expected.stdout);
    assert_eq!(result.stderr.as_bytes(), expected.stderr);
}

#[tokio::test]
async fn indexed_array_empty_pending_items_still_charge_container_storage() {
    let mut bash = Bash::builder()
        .limits(ExecutionLimits::new().max_live_intermediate_bytes(256))
        .build();
    let script = format!("a=({})", "[0]='' ".repeat(64));
    assert_budget_exhausted(bash.exec(&script).await);
}

#[tokio::test]
async fn indexed_array_duplicate_subscripts_remain_valid_at_entry_limit() {
    let mut bash = Bash::builder()
        .memory_limits(bashkit::MemoryLimits::new().max_array_entries(1))
        .limits(ExecutionLimits::new().max_live_intermediate_bytes(8_192))
        .build();
    let result = bash
        .exec(&format!(
            "v='{}'; a=([0]=$v [0]=$v [0]=$v); echo ${{#a[@]}} ${{#a[0]}}",
            "x".repeat(1_024)
        ))
        .await
        .unwrap();
    assert_eq!(result.stdout, "1 1024\n");
}

#[tokio::test]
async fn indexed_array_substitutions_release_consumed_output() {
    let mut bash = Bash::builder()
        .limits(ExecutionLimits::new().max_live_intermediate_bytes(7_168))
        .build();
    bash.exec(&format!("v='{}'", "x".repeat(1_024)))
        .await
        .unwrap();
    let result = bash
        .exec("a=([0]=$(printf %s \"$v\")$(printf %s \"$v\")$(printf %s \"$v\")); echo ${#a[0]}")
        .await
        .unwrap();
    assert_eq!(result.stdout, "3072\n");
}
