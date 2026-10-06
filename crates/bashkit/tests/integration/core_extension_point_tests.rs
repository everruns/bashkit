//! Public extension points that let tool layers live outside core:
//! `RuntimeCallContext` for embedded-runtime host functions, `code_mode()`,
//! and `BuiltinContext::remaining_deadline()`.

#[cfg(any(feature = "python", feature = "typescript"))]
use bashkit::ExecutionExtensions;
use bashkit::{Bash, Builtin, BuiltinContext, ExecResult, ExecutionLimits, async_trait};
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
async fn code_mode_keeps_logic_and_custom_builtins() {
    let mut bash = Bash::builder()
        .code_mode()
        .builtin("upper", Box::new(Upper))
        .build();
    let result = bash
        .exec("for x in a b; do echo $x; done | upper | sort -r")
        .await
        .unwrap();
    assert_eq!(result.stdout, "B\nA\n");
    assert_eq!(result.exit_code, 0);
}

#[tokio::test]
async fn code_mode_rejects_filesystem_access() {
    let mut bash = Bash::builder().code_mode().build();

    let redirect = bash.exec("echo secret > /tmp/out").await.unwrap();
    assert_ne!(redirect.exit_code, 0, "file redirect must fail");

    let read = bash.exec("cat /etc/passwd").await.unwrap();
    assert_ne!(read.exit_code, 0, "cat is not a code-mode builtin");

    let write = bash.exec("mkdir /work").await.unwrap();
    assert_ne!(write.exit_code, 0, "mkdir is not a code-mode builtin");
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
