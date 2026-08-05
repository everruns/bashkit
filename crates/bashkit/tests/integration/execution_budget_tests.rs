use bashkit::{Bash, Error, ExecutionLimits, LimitExceeded};

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
