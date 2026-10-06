//! A registry callback is a request-owned async boundary: cancelling the
//! request must drop the pending callback and leave the shell reusable.

use bashkit::{Bash, Error};
use bashkit_scripted_tool::{ToolArgs, ToolDef, ToolRegistry};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::Notify;

struct ReleaseProbe(Arc<AtomicBool>);

impl Drop for ReleaseProbe {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn runtime_tool_callback_obeys_request_lifecycle() {
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
    let mut bash = registry.install(Bash::builder()).build();

    let cancellation = bash.cancellation_token();
    let cancel = cancellation.clone();
    let canceller = tokio::spawn(async move {
        started.notified().await;
        cancel.store(true, Ordering::SeqCst);
    });

    let result = bash.exec("pending_tool").await;
    canceller.await.unwrap();
    assert!(matches!(result, Err(Error::Cancelled)), "{result:?}");
    assert!(released.load(Ordering::SeqCst), "boundary resource leaked");

    cancellation.store(false, Ordering::SeqCst);
    assert_eq!(
        bash.exec("echo reusable").await.unwrap().stdout,
        "reusable\n"
    );
}
