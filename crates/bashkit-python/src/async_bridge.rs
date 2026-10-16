//! Drain native result deliveries before CPython destroys worker thread states.
//!
//! `call_soon_threadsafe` can wake the caller before its native invocation has
//! released all Python references. The caller can then exit while that worker
//! is still attached. Retain the upstream bridge's cancellation/context/panic
//! behavior, but track its blocking deliveries through their complete teardown.

use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Condvar, Mutex, Weak};

use pyo3::prelude::*;
use pyo3_async_runtimes::{TaskLocals, generic};

static DELIVERIES: (Mutex<usize>, Condvar) = (Mutex::new(0), Condvar::new());
static PRIVATE_LOOPS: Mutex<(bool, Vec<Weak<super::PyPrivateAsyncLoop>>)> =
    Mutex::new((false, Vec::new()));

pub(super) fn register_private_loop(private_loop: &Arc<super::PyPrivateAsyncLoop>) {
    let mut registry = PRIVATE_LOOPS.lock().expect("private loops registry lock");
    if registry.0 {
        private_loop.closing.store(true, Ordering::Release);
    } else {
        registry.1.retain(|entry| entry.strong_count() > 0);
        registry.1.push(Arc::downgrade(private_loop));
    }
}

struct DeliveryGuard;

impl Drop for DeliveryGuard {
    fn drop(&mut self) {
        let mut active = DELIVERIES.0.lock().expect("async deliveries lock");
        *active -= 1;
        DELIVERIES.1.notify_all();
    }
}

struct CallbackRuntime;

impl generic::Runtime for CallbackRuntime {
    type JoinError = tokio::task::JoinError;
    type JoinHandle = tokio::task::JoinHandle<()>;

    fn spawn<F>(future: F) -> Self::JoinHandle
    where
        F: Future<Output = ()> + Send + 'static,
    {
        pyo3_async_runtimes::tokio::get_runtime().spawn(future)
    }

    fn spawn_blocking<F>(deliver: F) -> Self::JoinHandle
    where
        F: FnOnce() + Send + 'static,
    {
        let guard = {
            let mut active = DELIVERIES.0.lock().expect("async deliveries lock");
            if super::interpreter_at_exit() {
                None
            } else {
                *active += 1;
                Some(DeliveryGuard)
            }
        };
        pyo3_async_runtimes::tokio::get_runtime().spawn_blocking(move || {
            let Some(_guard) = guard else {
                // Drop unattached: PyO3 defers decrefs rather than touching a
                // finalizing interpreter. No Python call may begin after exit.
                drop(deliver);
                return;
            };
            deliver();
        })
    }
}

tokio::task_local! {
    static LOCALS: TaskLocals;
}

impl generic::ContextExt for CallbackRuntime {
    fn scope<F, R>(locals: TaskLocals, future: F) -> Pin<Box<dyn Future<Output = R> + Send>>
    where
        F: Future<Output = R> + Send + 'static,
    {
        Box::pin(LOCALS.scope(locals, future))
    }

    fn get_task_locals() -> Option<TaskLocals> {
        LOCALS.try_with(Clone::clone).ok()
    }
}

pub(super) fn future_into_py<F, T>(py: Python<'_>, future: F) -> PyResult<Bound<'_, PyAny>>
where
    F: Future<Output = PyResult<T>> + Send + 'static,
    T: for<'py> IntoPyObject<'py> + Send + 'static,
{
    generic::future_into_py::<CallbackRuntime, _, _>(py, future)
}

pub(super) fn get_current_locals(py: Python<'_>) -> PyResult<TaskLocals> {
    generic::get_current_locals::<CallbackRuntime>(py)
}

// THREAT[TM-PY-030]: hold the same enrollment lock while closing the boundary,
// then release the GIL so already-enrolled deliveries can finish their Python
// cleanup. Future deliveries cannot attach after the boundary is closed.
pub(super) fn shutdown(py: Python<'_>) {
    // Private-loop callbacks may still be unwinding from timeout cancellation.
    // Cancel and join them while Python remains fully attachable, then close
    // the result-delivery boundary. New private loops cannot start meanwhile.
    let private_loops = {
        let mut registry = PRIVATE_LOOPS.lock().expect("private loops registry lock");
        registry.0 = true;
        registry
            .1
            .drain(..)
            .filter_map(|entry| entry.upgrade())
            .collect::<Vec<_>>()
    };
    for private_loop in &private_loops {
        private_loop.closing.store(true, Ordering::Release);
        private_loop.cancel_inflight();
    }
    for private_loop in private_loops {
        private_loop.shutdown();
    }
    let active = DELIVERIES.0.lock().expect("async deliveries lock");
    super::INTERPRETER_AT_EXIT.store(true, Ordering::Release);
    drop(active);
    py.detach(|| {
        let active = DELIVERIES.0.lock().expect("async deliveries lock");
        drop(
            DELIVERIES
                .1
                .wait_while(active, |active| *active > 0)
                .expect("async deliveries lock"),
        );
    });
}
