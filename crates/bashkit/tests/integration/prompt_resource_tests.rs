// Prompt reparses must share runtime limits; each parsed expression is shallow.
use bashkit::{Bash, Error, ExecutionBudgetExceeded, ExecutionLimits, LimitExceeded};

fn assert_depth(result: bashkit::Result<bashkit::ExecResult>, depth: usize) {
    assert!(
        matches!(result, Err(Error::ResourceLimit(LimitExceeded::MaxPromptDepth(d))) if d == depth),
        "expected prompt depth {depth}, got {result:?}"
    );
}

fn prompt_chain(depth: usize, limits: ExecutionLimits) -> Bash {
    let mut bash = Bash::builder().limits(limits).build();
    bash.set_env("p0", "done");
    for i in 1..depth {
        bash.set_env(&format!("p{i}"), &format!("${{p{}@P}}", i - 1));
    }
    bash
}

#[tokio::test]
async fn finite_prompt_chain_respects_depth_limit() {
    // Finite six-level chain: safe to execute against the unfixed interpreter.
    let mut bash = prompt_chain(6, ExecutionLimits::new().max_ast_depth(5));
    let result = bash.exec("echo \"${p5@P}\"").await;
    assert_depth(result, 5);
}

#[tokio::test]
async fn prompt_cycles_stop_and_session_recovers() {
    // Run cycles only with the guard installed; depth tests above are finite.
    for script in [
        r#"x='${x@P}'; echo "${x@P}""#,
        r#"x='${y@P}'; y='${x@P}'; echo "${x@P}""#,
        r#"x='$(echo "${x@P}")'; echo "${x@P}""#,
        r#"x='$(echo "${x@P}" | cat)'; echo "${x@P}""#,
    ] {
        let mut bash = Bash::new();
        assert_depth(bash.exec(script).await, 8);
        assert_eq!(
            bash.exec(r#"x=ok; echo "${x@P}""#).await.unwrap().stdout,
            "ok\n"
        );
    }
}

#[tokio::test]
async fn background_prompt_cycle_is_contained() {
    let mut bash = Bash::new();
    let result = bash
        .exec(r#"x='${x@P}'; echo "${x@P}" & wait"#)
        .await
        .unwrap();
    assert!(
        result
            .stderr
            .contains("maximum prompt expansion depth exceeded (8)")
    );
    assert_eq!(
        bash.exec("echo recovered").await.unwrap().stdout,
        "recovered\n"
    );
}

#[tokio::test]
async fn prompt_depth_boundary_and_siblings() {
    let mut bash = prompt_chain(8, ExecutionLimits::new());
    let result = bash.exec(r#"echo "${p7@P}|${p7@P}""#).await.unwrap();
    assert_eq!(result.stdout, "done|done\n");
    bash.set_env("p8", "${p7@P}");
    assert_depth(bash.exec(r#"echo "${p8@P}""#).await, 8);
    // Raising AST depth cannot remove the native-stack ceiling.
    let mut bash = prompt_chain(9, ExecutionLimits::new().max_ast_depth(usize::MAX));
    assert_depth(bash.exec(r#"echo "${p8@P}""#).await, 8);
}

#[tokio::test]
async fn promptvars_disabled_keeps_cycle_literal() {
    let mut bash = Bash::new();
    let result = bash
        .exec(r#"shopt -u promptvars; x='${x@P}'; echo "${x@P}""#)
        .await
        .unwrap();
    assert_eq!(result.stdout, "${x@P}\n");
}

#[tokio::test]
async fn prompt_reparses_share_work_budget() {
    let mut bash = prompt_chain(6, ExecutionLimits::new().max_work_units(20));
    let result = bash.exec(r#"echo "${p5@P}""#).await;
    assert!(
        matches!(
            result,
            Err(Error::ResourceLimit(LimitExceeded::ExecutionBudget(
                ExecutionBudgetExceeded::WorkUnits { .. }
            )))
        ),
        "expected shared work budget: {result:?}"
    );
}

#[tokio::test]
async fn prompt_siblings_share_input_budget() {
    let mut bash = Bash::builder()
        .limits(ExecutionLimits::new().max_aggregate_input_bytes(80))
        .env("x", "a".repeat(40))
        .build();
    let result = bash.exec(r#"echo "${x@P}${x@P}""#).await;
    assert!(
        matches!(
            result,
            Err(Error::ResourceLimit(LimitExceeded::ExecutionBudget(
                ExecutionBudgetExceeded::InputBytes { .. }
            )))
        ),
        "expected shared input budget: {result:?}"
    );
}
