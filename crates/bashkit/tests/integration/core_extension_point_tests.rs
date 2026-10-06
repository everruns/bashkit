//! Public extension points that let tool layers live outside core:
//! `RuntimeCallContext` for embedded-runtime host functions, `ShellFeatures`
//! and `builtin_filter` for restricted shells, and
//! `BuiltinContext::remaining_deadline()`.

#[cfg(any(feature = "python", feature = "typescript"))]
use bashkit::ExecutionExtensions;
use bashkit::{
    Bash, Builtin, BuiltinContext, ExecResult, ExecutionLimits, ShellFeatures, async_trait,
};
use std::time::Duration;

#[cfg(any(feature = "python", feature = "typescript"))]
#[derive(Clone)]
struct Tenant(&'static str);

#[cfg(any(feature = "python", feature = "typescript"))]
fn current_tenant() -> String {
    bashkit::RuntimeCallContext::current()
        .execution_extension::<Tenant>()
        .and_then(|tenant| tenant.try_with(|tenant| tenant.0.to_string()).ok())
        .unwrap_or_else(|| "none".to_string())
}

#[cfg(any(feature = "python", feature = "typescript"))]
#[test]
fn runtime_call_context_is_empty_outside_a_runtime_call() {
    let ctx = bashkit::RuntimeCallContext::current();
    assert!(ctx.execution_extension::<Tenant>().is_none());
    assert!(ctx.execution_budget().is_none());
    assert!(ctx.remaining_deadline().is_none());
    assert!(ctx.execution_capability(()).is_none());
}

#[cfg(feature = "python")]
fn python_bash() -> Bash {
    use bashkit::{ExtFunctionResult, MontyObject, PythonExternalFnHandler, PythonLimits};
    use std::sync::Arc;

    let handler: PythonExternalFnHandler = Arc::new(|_, _, _| {
        Box::pin(async move { ExtFunctionResult::Return(MontyObject::String(current_tenant())) })
    });
    Bash::builder()
        .python_with_external_handler(PythonLimits::default(), vec!["tenant".into()], handler)
        .env("BASHKIT_ALLOW_INPROCESS_PYTHON", "1")
        .build()
}

#[cfg(feature = "python")]
#[tokio::test]
async fn python_host_function_sees_request_extensions() {
    let mut bash = python_bash();
    let result = bash
        .exec_with_extensions(
            "python -c 'print(tenant())'",
            ExecutionExtensions::new().with(Tenant("acme")),
        )
        .await
        .unwrap();
    assert_eq!(result.stdout, "acme\n", "stderr: {}", result.stderr);
}

#[cfg(feature = "python")]
#[tokio::test]
async fn python_host_function_without_request_extension_sees_none() {
    let mut bash = python_bash();
    let result = bash.exec("python -c 'print(tenant())'").await.unwrap();
    assert_eq!(result.stdout, "none\n", "stderr: {}", result.stderr);
}

#[cfg(feature = "python")]
#[tokio::test]
async fn python_host_function_context_does_not_leak_across_requests() {
    let mut bash = python_bash();
    let first = bash
        .exec_with_extensions(
            "python -c 'print(tenant())'",
            ExecutionExtensions::new().with(Tenant("acme")),
        )
        .await
        .unwrap();
    assert_eq!(first.stdout, "acme\n");
    let second = bash.exec("python -c 'print(tenant())'").await.unwrap();
    assert_eq!(second.stdout, "none\n");
}

#[cfg(feature = "typescript")]
#[tokio::test]
async fn typescript_host_function_sees_request_extensions() {
    use bashkit::{TypeScriptExternalFnHandler, TypeScriptLimits, ZapcodeValue};
    use std::sync::Arc;

    let handler: TypeScriptExternalFnHandler =
        Arc::new(|_, _| Box::pin(async move { Ok(ZapcodeValue::String(current_tenant().into())) }));
    let mut bash = Bash::builder()
        .typescript_with_external_handler(
            TypeScriptLimits::default(),
            vec!["tenant".into()],
            handler,
        )
        .build();
    let result = bash
        .exec_with_extensions(
            "ts -c 'await tenant()'",
            ExecutionExtensions::new().with(Tenant("acme")),
        )
        .await
        .unwrap();
    assert_eq!(result.stdout, "acme\n", "stderr: {}", result.stderr);

    let result = bash.exec("ts -c 'await tenant()'").await.unwrap();
    assert_eq!(result.stdout, "none\n", "stderr: {}", result.stderr);
}

struct Upper;

#[async_trait]
impl Builtin for Upper {
    async fn execute(&self, ctx: BuiltinContext<'_>) -> bashkit::Result<ExecResult> {
        let input = ctx.stdin_text_lossy().unwrap_or_default().to_uppercase();
        Ok(ExecResult::ok(input))
    }
}

#[tokio::test]
async fn shell_features_default_to_full_shell() {
    assert_eq!(ShellFeatures::default(), ShellFeatures::all());
    let mut bash = Bash::builder().build();
    let result = bash
        .exec("echo hi > /tmp/out; cat /tmp/out; cat <(echo sub)")
        .await
        .unwrap();
    assert_eq!(result.stdout, "hi\nsub\n");
}

#[tokio::test]
async fn disabled_file_redirects_reject_files_but_keep_dev_null() {
    let mut bash = Bash::builder()
        .shell_features(ShellFeatures::all().file_redirects(false))
        .build();

    let write = bash.exec("echo secret > /tmp/out").await.unwrap();
    assert_ne!(write.exit_code, 0);
    assert!(write.stderr.contains("filesystem redirection disabled"));

    let read = bash.exec("cat < /etc/passwd").await.unwrap();
    assert_ne!(read.exit_code, 0);

    let null = bash.exec("echo quiet > /dev/null; echo ok").await.unwrap();
    assert_eq!(null.stdout, "ok\n");

    // Other features stay on.
    let sub = bash.exec("cat <(echo sub)").await.unwrap();
    assert_eq!(sub.stdout, "sub\n");
}

#[tokio::test]
async fn disabled_process_substitution_is_rejected() {
    let mut bash = Bash::builder()
        .shell_features(ShellFeatures::all().process_substitution(false))
        .build();
    // Expansion-time rejection surfaces as an execution error.
    let err = match bash.exec("cat <(echo sub)").await {
        Ok(result) => panic!("expected rejection, got {result:?}"),
        Err(err) => err.to_string(),
    };
    assert!(err.contains("process substitution disabled"), "{err}");
}

#[tokio::test]
async fn disabled_script_execution_rejects_paths_and_nested_shells() {
    let mut bash = Bash::builder()
        .shell_features(ShellFeatures::all().script_execution(false))
        .mount_text("/work/x.sh", "echo ran\n")
        .build();
    for script in [
        "/work/x.sh",
        "source /work/x.sh",
        ". /work/x.sh",
        "bash /work/x.sh",
        "sh -c 'echo ran'",
    ] {
        let result = bash.exec(script).await.unwrap();
        assert_eq!(result.exit_code, 127, "{script}: {}", result.stderr);
        assert!(!result.stdout.contains("ran"), "{script}");
    }
    // Files themselves are still readable: only execution is off.
    let read = bash.exec("cat /work/x.sh").await.unwrap();
    assert_eq!(read.stdout, "echo ran\n");
}

#[tokio::test]
async fn builtin_filter_drops_defaults_but_keeps_custom_builtins() {
    let mut bash = Bash::builder()
        .builtin_filter(|name| matches!(name, "echo" | "sort"))
        .builtin("upper", Box::new(Upper))
        .build();
    let result = bash
        .exec("for x in a b; do echo $x; done | upper | sort -r")
        .await
        .unwrap();
    assert_eq!(result.stdout, "B\nA\n");

    let missing = bash.exec("cat /etc/passwd").await.unwrap();
    assert_eq!(missing.exit_code, 127);
}

struct Deadline;

#[async_trait]
impl Builtin for Deadline {
    async fn execute(&self, ctx: BuiltinContext<'_>) -> bashkit::Result<ExecResult> {
        let out = match ctx.remaining_deadline() {
            Some(remaining) if remaining <= Duration::from_secs(30) => "bounded\n",
            Some(_) => "too-long\n",
            None => "none\n",
        };
        Ok(ExecResult::ok(out.to_string()))
    }
}

#[tokio::test]
async fn builtin_sees_remaining_deadline() {
    let mut bash = Bash::builder()
        .limits(ExecutionLimits::new().timeout(Duration::from_secs(30)))
        .builtin("deadline", Box::new(Deadline))
        .build();
    let result = bash.exec("deadline").await.unwrap();
    assert_eq!(result.stdout, "bounded\n");
}

#[tokio::test]
async fn builtin_run_budgeted_returns_future_output() {
    struct Budgeted;

    #[async_trait]
    impl Builtin for Budgeted {
        async fn execute(&self, ctx: BuiltinContext<'_>) -> bashkit::Result<ExecResult> {
            let value = ctx.run_budgeted(async { 41 + 1 }).await?;
            Ok(ExecResult::ok(format!("{value}\n")))
        }
    }

    let mut bash = Bash::builder()
        .builtin("budgeted", Box::new(Budgeted))
        .build();
    let result = bash.exec("budgeted").await.unwrap();
    assert_eq!(result.stdout, "42\n");
}
