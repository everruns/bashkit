// Request context for host functions called from embedded Python/TypeScript.
//
// Important decision: host-function handlers (`PythonExternalFnHandler`,
// `TypeScriptExternalFnHandler`) keep their signatures. The interpreter VMs
// suspend inside a Tokio task-local scope instead, and handlers read the
// active request with `RuntimeCallContext::current()`. Embedders that build
// their own tool layers (e.g. a ToolDef registry outside core) get tenant
// extensions, the aggregate budget and the wall-clock deadline without core
// knowing about tools.

use std::sync::Arc;
use std::time::Duration;

use crate::builtins::{Context, ExecutionDeadline, ExecutionExtensions};
use crate::{ExecutionBudget, ExecutionCapability};

/// Per-request context visible to host functions invoked from embedded
/// Python or TypeScript code.
///
/// Obtain it inside a handler with [`RuntimeCallContext::current`]. Outside a
/// runtime call (or on a runtime started without a request) every accessor
/// returns `None`.
#[derive(Clone, Default)]
pub struct RuntimeCallContext {
    extensions: Option<Arc<ExecutionExtensions>>,
}

impl RuntimeCallContext {
    pub(crate) fn from_context(ctx: &Context<'_>) -> Self {
        Self {
            extensions: ctx.execution_extensions(),
        }
    }

    /// Context of the runtime call currently executing on this task.
    pub fn current() -> Self {
        RUNTIME_CALL_CONTEXT
            .try_with(Clone::clone)
            .unwrap_or_default()
    }

    /// Look up a typed, revocable per-execution extension, if present.
    pub fn execution_extension<T>(&self) -> Option<ExecutionCapability<T>>
    where
        T: Send + Sync + 'static,
    {
        self.extensions.as_ref()?.get::<T>()
    }

    /// Bind a host value to the current execution lease.
    pub fn execution_capability<T>(&self, value: T) -> Option<ExecutionCapability<T>>
    where
        T: Send + Sync + 'static,
    {
        self.extensions.as_ref()?.capability(value)
    }

    /// Aggregate budget shared by this request's commands and runtimes.
    pub fn execution_budget(&self) -> Option<ExecutionCapability<ExecutionBudget>> {
        self.execution_extension::<ExecutionBudget>()
    }

    /// Remaining wall-clock budget of the current `exec*` call.
    pub fn remaining_deadline(&self) -> Option<Duration> {
        self.execution_extension::<ExecutionDeadline>()?
            .try_with(ExecutionDeadline::remaining)
            .ok()
    }
}

tokio::task_local! {
    static RUNTIME_CALL_CONTEXT: RuntimeCallContext;
}

/// Run an embedded-runtime future with `ctx` as its [`RuntimeCallContext`].
pub(crate) async fn scope<F: std::future::Future>(ctx: &Context<'_>, future: F) -> F::Output {
    RUNTIME_CALL_CONTEXT
        .scope(RuntimeCallContext::from_context(ctx), future)
        .await
}
