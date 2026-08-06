//! Adversarial contract for request-owned async execution boundaries.

#[cfg(any(
    feature = "scripted_tool",
    feature = "http_client",
    feature = "python",
    feature = "typescript",
    feature = "sqlite"
))]
use bashkit::{Bash, Error};
#[cfg(any(
    feature = "scripted_tool",
    feature = "http_client",
    feature = "python",
    feature = "typescript",
    feature = "sqlite"
))]
use std::sync::atomic::Ordering;
#[cfg(any(
    feature = "scripted_tool",
    feature = "http_client",
    feature = "python",
    feature = "typescript"
))]
use std::sync::{Arc, atomic::AtomicBool};
#[cfg(any(
    feature = "scripted_tool",
    feature = "http_client",
    feature = "python",
    feature = "typescript"
))]
use tokio::sync::Notify;

#[cfg(any(
    feature = "scripted_tool",
    feature = "http_client",
    feature = "python",
    feature = "typescript"
))]
struct ReleaseProbe(Arc<AtomicBool>);

#[cfg(any(
    feature = "scripted_tool",
    feature = "http_client",
    feature = "python",
    feature = "typescript"
))]
impl Drop for ReleaseProbe {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

#[cfg(any(
    feature = "scripted_tool",
    feature = "http_client",
    feature = "python",
    feature = "typescript"
))]
async fn assert_cancel_drops_boundary(
    mut bash: Bash,
    script: &str,
    started: Arc<Notify>,
    released: Arc<AtomicBool>,
) {
    let cancellation = bash.cancellation_token();
    let cancel = cancellation.clone();
    let canceller = tokio::spawn(async move {
        started.notified().await;
        cancel.store(true, Ordering::SeqCst);
    });

    let result = bash.exec(script).await;
    canceller.await.unwrap();
    assert!(matches!(result, Err(Error::Cancelled)), "{result:?}");
    assert!(released.load(Ordering::SeqCst), "boundary resource leaked");

    cancellation.store(false, Ordering::SeqCst);
    assert_eq!(
        bash.exec("echo reusable").await.unwrap().stdout,
        "reusable\n"
    );
}

#[cfg(feature = "scripted_tool")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn runtime_tool_callback_obeys_request_lifecycle() {
    use bashkit::{ToolArgs, ToolDef, ToolRegistry};

    let started = Arc::new(Notify::new());
    let released = Arc::new(AtomicBool::new(false));
    let callback_started = started.clone();
    let callback_released = released.clone();
    let registry = ToolRegistry::builder()
        .async_tool_fn(
            ToolDef::new("pending_tool", "wait forever"),
            move |_args: ToolArgs| {
                let started = callback_started.clone();
                let released = callback_released.clone();
                async move {
                    let _probe = ReleaseProbe(released);
                    started.notify_one();
                    std::future::pending::<std::result::Result<String, String>>().await
                }
            },
        )
        .build();
    let bash = Bash::builder().tool_registry(registry).build();

    assert_cancel_drops_boundary(bash, "pending_tool", started, released).await;
}

#[cfg(feature = "http_client")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn http_transport_obeys_request_lifecycle() {
    use bashkit::{
        HttpResponse, HttpTransport, HttpTransportError, HttpTransportRequest, NetworkAllowlist,
    };

    struct PendingTransport {
        started: Arc<Notify>,
        released: Arc<AtomicBool>,
    }

    #[bashkit::async_trait]
    impl HttpTransport for PendingTransport {
        async fn execute(
            &self,
            _request: HttpTransportRequest,
        ) -> std::result::Result<HttpResponse, HttpTransportError> {
            let _probe = ReleaseProbe(self.released.clone());
            self.started.notify_one();
            std::future::pending().await
        }
    }

    let started = Arc::new(Notify::new());
    let released = Arc::new(AtomicBool::new(false));
    let bash = Bash::builder()
        .network(NetworkAllowlist::allow_all())
        .http_transport(Arc::new(PendingTransport {
            started: started.clone(),
            released: released.clone(),
        }))
        .build();

    assert_cancel_drops_boundary(bash, "curl https://example.com", started, released).await;
}

#[cfg(feature = "python")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn embedded_python_callback_obeys_request_lifecycle() {
    use bashkit::{ExtFunctionResult, PythonExternalFnHandler, PythonLimits};

    let started = Arc::new(Notify::new());
    let released = Arc::new(AtomicBool::new(false));
    let callback_started = started.clone();
    let callback_released = released.clone();
    let handler: PythonExternalFnHandler = Arc::new(move |_, _, _| {
        let started = callback_started.clone();
        let released = callback_released.clone();
        Box::pin(async move {
            let _probe = ReleaseProbe(released);
            started.notify_one();
            std::future::pending::<ExtFunctionResult>().await
        })
    });
    let bash = Bash::builder()
        .python_with_external_handler(PythonLimits::default(), vec!["pending".into()], handler)
        .env("BASHKIT_ALLOW_INPROCESS_PYTHON", "1")
        .build();

    assert_cancel_drops_boundary(bash, "python -c 'pending()'", started, released).await;
}

#[cfg(feature = "typescript")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn embedded_typescript_callback_obeys_request_lifecycle() {
    use bashkit::{TypeScriptExternalFnHandler, TypeScriptLimits, ZapcodeValue};

    let started = Arc::new(Notify::new());
    let released = Arc::new(AtomicBool::new(false));
    let callback_started = started.clone();
    let callback_released = released.clone();
    let handler: TypeScriptExternalFnHandler = Arc::new(move |_, _| {
        let started = callback_started.clone();
        let released = callback_released.clone();
        Box::pin(async move {
            let _probe = ReleaseProbe(released);
            started.notify_one();
            std::future::pending::<std::result::Result<ZapcodeValue, String>>().await
        })
    });
    let bash = Bash::builder()
        .typescript_with_external_handler(
            TypeScriptLimits::default(),
            vec!["pending".into()],
            handler,
        )
        .build();

    assert_cancel_drops_boundary(bash, "ts -c 'await pending()'", started, released).await;
}

#[cfg(feature = "sqlite")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sqlite_vm_obeys_request_lifecycle() {
    use bashkit::SqliteLimits;

    let mut bash = Bash::builder()
        .sqlite_with_limits(SqliteLimits::default().max_statements(25_000))
        .env("BASHKIT_ALLOW_INPROCESS_SQLITE", "1")
        .build();
    let cancellation = bash.cancellation_token();
    let cancel = cancellation.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        cancel.store(true, Ordering::SeqCst);
    });

    // A statement flood stays inside the supported SQL subset and guarantees
    // many SQLite VM checkpoints without depending on scheduler timing.
    let sql = "SELECT 1;".repeat(20_000);
    let result = bash.exec(&format!("sqlite :memory: '{sql}'")).await;
    assert!(matches!(result, Err(Error::Cancelled)), "{result:?}");
    cancellation.store(false, Ordering::SeqCst);
    assert_eq!(
        bash.exec("echo reusable").await.unwrap().stdout,
        "reusable\n"
    );
}
