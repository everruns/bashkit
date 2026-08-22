use bashkit::{
    Bash, ExecOptions, ExecutionCapabilityError, ExecutionExtensions, ExecutionLimits, ToolArgs,
    ToolCallDecision, ToolCallRequest, ToolDef, ToolRegistry,
};
use serde_json::json;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use std::time::Duration;

fn orders_def() -> ToolDef {
    ToolDef::new("orders.list", "List customer orders")
        .with_category("orders")
        .with_tags(&["read"])
        .with_schema(json!({
            "type": "object",
            "required": ["customer"],
            "additionalProperties": false,
            "properties": { "customer": { "type": "string" } }
        }))
}

#[tokio::test]
async fn tool_callback_context_is_revoked_when_the_request_completes() {
    let retained: Arc<Mutex<Option<ToolArgs>>> = Arc::new(Mutex::new(None));
    let callback_retained = retained.clone();
    let registry = ToolRegistry::builder()
        .async_tool_fn(orders_def(), move |args| {
            let callback_retained = callback_retained.clone();
            async move {
                assert_eq!(args.tenant_id().unwrap().as_deref(), Some("tenant-a"));
                *callback_retained.lock().unwrap() = Some(args);
                Ok("ok".to_string())
            }
        })
        .build();
    let mut bash = Bash::builder().tool_registry(registry.clone()).build();

    let result = bash
        .exec_with_options("orders.list --customer x", request("tenant-a", &registry))
        .await
        .unwrap();
    assert_eq!(result.stdout, "ok");

    let args = retained.lock().unwrap().take().unwrap();
    assert_eq!(args.tenant_id(), Err(ExecutionCapabilityError::Revoked));
    assert_eq!(args.surface(), Err(ExecutionCapabilityError::Revoked));
}

fn request(tenant: &str, registry: &ToolRegistry) -> ExecOptions {
    ExecOptions::new()
        .extensions(ExecutionExtensions::new().with(ToolCallRequest::new(tenant, registry.trace())))
}

#[tokio::test]
async fn one_registry_dispatches_shell_python_and_typescript_with_shared_policy_and_callback() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let callback_calls = calls.clone();
    let policy_calls = Arc::new(AtomicUsize::new(0));
    let policy_count = policy_calls.clone();
    let registry = ToolRegistry::builder()
        .tool_fn(orders_def(), move |args: &ToolArgs| {
            callback_calls.lock().unwrap().push((
                args.tenant_id().unwrap().unwrap_or_default(),
                args.param_str("customer").unwrap_or_default().to_string(),
            ));
            Ok(json!({"customer": args.param_str("customer"), "orders": [1, 2]}).to_string())
        })
        .policy(move |call| {
            policy_count.fetch_add(1, Ordering::SeqCst);
            assert_eq!(call.tenant_id(), Some("tenant-a"));
            ToolCallDecision::Allow
        })
        .build();
    let mut bash = Bash::builder()
        .tool_registry(registry.clone())
        .env("BASHKIT_ALLOW_INPROCESS_PYTHON", "1")
        .build();

    let shell = bash
        .exec_with_options(
            "orders.list --customer shell",
            request("tenant-a", &registry),
        )
        .await
        .unwrap();
    let python = bash
        .exec_with_options(
            r#"python -c 'print(tools.orders.list({"customer": "python"})["customer"])'"#,
            request("tenant-a", &registry),
        )
        .await
        .unwrap();
    let typescript = bash
        .exec_with_options(
            r#"ts -c '(await tools.orders.list({customer: "typescript"})).customer'"#,
            request("tenant-a", &registry),
        )
        .await
        .unwrap();

    assert_eq!(shell.exit_code, 0, "{}", shell.stderr);
    assert_eq!(python.stdout, "python\n", "{}", python.stderr);
    assert_eq!(typescript.stdout, "typescript\n", "{}", typescript.stderr);
    assert_eq!(policy_calls.load(Ordering::SeqCst), 3);
    assert_eq!(calls.lock().unwrap().len(), 3);
}

#[tokio::test]
async fn registry_enforces_schema_denial_sanitization_discovery_and_request_traces() {
    let registry = ToolRegistry::builder()
        .tool_fn(orders_def(), |_args| {
            Err("postgres://secret@internal/orders".into())
        })
        .policy(|call| {
            if call.tenant_id() == Some("denied") {
                ToolCallDecision::Deny
            } else {
                ToolCallDecision::Allow
            }
        })
        .build();
    let allowed = ToolCallRequest::new("allowed", registry.trace());
    let allowed_trace = allowed.trace();
    let mut bash = Bash::builder()
        .tool_registry(registry.clone())
        .env("BASHKIT_ALLOW_INPROCESS_PYTHON", "1")
        .build();

    let schema = bash.exec("orders.list").await.unwrap();
    assert_eq!(schema.exit_code, 2);
    assert!(schema.stderr.contains("required property 'customer'"));

    let denied = bash
        .exec_with_options(
            r#"ts -c 'await tools.orders.list({customer: "x"})'"#,
            request("denied", &registry),
        )
        .await
        .unwrap();
    assert_ne!(denied.exit_code, 0);
    assert!(denied.stderr.contains("denied by policy"));

    let failed = bash
        .exec_with_options(
            "orders.list --customer x",
            ExecOptions::new().extensions(ExecutionExtensions::new().with(allowed)),
        )
        .await
        .unwrap();
    assert_eq!(failed.exit_code, 1);
    assert!(failed.stderr.contains("callback failed"));
    assert!(!failed.stderr.contains("postgres"));

    let discover = bash
        .exec(r#"python -c 'print(tools.discover({"category": "orders"})[0]["name"])'"#)
        .await
        .unwrap();
    assert_eq!(discover.stdout, "orders.list\n", "{}", discover.stderr);

    let trace = allowed_trace.take_invocations();
    assert_eq!(trace.len(), 1);
    assert_eq!(trace[0].name, "orders.list");
}

#[tokio::test]
async fn registry_resolves_local_schema_references_before_callback() {
    let called = Arc::new(AtomicBool::new(false));
    let callback_called = called.clone();
    let definition = ToolDef::new("customers.get", "Get a customer").with_schema(json!({
        "$ref": "#/$defs/Input",
        "$defs": {
            "Input": {
                "type": "object",
                "required": ["customer"],
                "additionalProperties": false,
                "properties": { "customer": { "type": "string" } }
            }
        }
    }));
    let registry = ToolRegistry::builder()
        .tool_fn(definition, move |_args| {
            callback_called.store(true, Ordering::SeqCst);
            Ok("unexpected".into())
        })
        .build();
    let mut bash = Bash::builder().tool_registry(registry).build();

    let result = bash.exec("customers.get --unknown attacker").await.unwrap();

    assert_eq!(result.exit_code, 2);
    assert!(result.stderr.contains("required property 'customer'"));
    assert!(!called.load(Ordering::SeqCst));
}

#[tokio::test]
async fn registry_deadline_cancels_callback_and_tenants_do_not_share_context_or_traces() {
    struct CancelOnDrop(Arc<AtomicBool>);
    impl Drop for CancelOnDrop {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }

    let cancelled = Arc::new(AtomicBool::new(false));
    let callback_cancelled = cancelled.clone();
    let registry = ToolRegistry::builder()
        .async_tool_fn(orders_def(), move |args| {
            let callback_cancelled = callback_cancelled.clone();
            async move {
                let _guard = CancelOnDrop(callback_cancelled);
                tokio::time::sleep(Duration::from_secs(1)).await;
                Ok(args.tenant_id().unwrap().unwrap_or_default())
            }
        })
        .build();
    let limits = ExecutionLimits::new().timeout(Duration::from_millis(25));
    let mut bash = Bash::builder()
        .limits(limits)
        .tool_registry(registry.clone())
        .env("BASHKIT_ALLOW_INPROCESS_PYTHON", "1")
        .build();
    let timeout = bash
        .exec_with_options("orders.list --customer x", request("slow", &registry))
        .await
        .unwrap();
    assert_eq!(timeout.exit_code, 1);
    assert!(cancelled.load(Ordering::SeqCst));

    let seen = Arc::new(Mutex::new(Vec::new()));
    let callback_seen = seen.clone();
    let registry = ToolRegistry::builder()
        .tool_fn(orders_def(), move |args| {
            let tenant = args.tenant_id().unwrap().unwrap_or_default();
            callback_seen.lock().unwrap().push(tenant.clone());
            Ok(tenant)
        })
        .build();
    let a = ToolCallRequest::new("a", registry.trace());
    let a_trace = a.trace();
    let b = ToolCallRequest::new("b", registry.trace());
    let b_trace = b.trace();
    let mut bash = Bash::builder().tool_registry(registry).build();
    let first = bash
        .exec_with_options(
            "orders.list --customer x",
            ExecOptions::new().extensions(ExecutionExtensions::new().with(a)),
        )
        .await
        .unwrap();
    let second = bash
        .exec_with_options(
            "orders.list --customer x",
            ExecOptions::new().extensions(ExecutionExtensions::new().with(b)),
        )
        .await
        .unwrap();
    assert_eq!(
        (first.stdout.text().unwrap(), second.stdout.text().unwrap()),
        ("a", "b")
    );
    assert_eq!(&*seen.lock().unwrap(), &["a", "b"]);
    assert_eq!(a_trace.take_invocations().len(), 1);
    assert_eq!(b_trace.take_invocations().len(), 1);
}
