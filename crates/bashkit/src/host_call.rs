// Host calls deliberately suspend only the live Rust execution future. They
// are not interpreter snapshots and cannot be serialized or resumed elsewhere.
//
// The future is handed to an independent task where the target has a spawner
// (any async runtime on native, `spawn_local` on JS-backed wasm), so the
// execution deadline keeps running while the host sits on a request instead of
// polling `next_event`. Targets without a spawner — a non-JS wasm embedder has
// no executor at all, see knowledge/runtimes/non-js-wasm.md — keep the future
// in the handle and let the caller's poll drive it. Only the *timing* of
// enforcement differs: on every target a timed-out execution drops its session
// and cannot be recovered with `into_bash`.

use std::collections::HashMap;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use tokio::sync::{mpsc, oneshot};

use crate::builtins::{Builtin, Context};
use crate::{Error, ExecOptions, ExecResult, Result, StreamData};

/// Identifier for one suspended host-call request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HostCallId(u64);

/// An event-backed builtin invocation waiting for a host result.
pub struct HostCallRequest {
    id: HostCallId,
    command: String,
    args: Vec<String>,
    env: HashMap<String, String>,
    cwd: PathBuf,
    stdin: Option<StreamData>,
}

impl std::fmt::Debug for HostCallRequest {
    // THREAT[TM-LOG-001]: requests may contain secrets in argv, env, or stdin.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HostCallRequest")
            .field("id", &self.id)
            .field("command", &self.command)
            .field("args_len", &self.args.len())
            .field("env_len", &self.env.len())
            .field("cwd", &self.cwd)
            .field("stdin_len", &self.stdin.as_ref().map(StreamData::len))
            .finish()
    }
}

impl HostCallRequest {
    /// Identifier to pass to [`ExecutionHandle::resume`].
    pub fn id(&self) -> HostCallId {
        self.id
    }

    /// Invoked command name.
    pub fn command(&self) -> &str {
        &self.command
    }

    /// Command arguments, excluding the command name.
    pub fn args(&self) -> &[String] {
        &self.args
    }

    /// Exported environment visible to the command.
    pub fn env(&self) -> &HashMap<String, String> {
        &self.env
    }

    /// Virtual working directory at the call site.
    pub fn cwd(&self) -> &Path {
        &self.cwd
    }

    /// Exact pipeline input supplied to the command.
    pub fn stdin(&self) -> Option<&StreamData> {
        self.stdin.as_ref()
    }
}

/// Observable state transition from a process-local execution.
#[derive(Debug)]
pub enum ExecutionEvent {
    /// Execution reached an event-backed builtin and is parked for its result.
    HostCall(HostCallRequest),
    /// Execution finished normally.
    Complete(ExecResult),
}

struct HostCallEnvelope {
    request: HostCallRequest,
    response: oneshot::Sender<ExecResult>,
}

#[derive(Clone)]
pub(crate) struct HostCallBroker {
    requests: mpsc::Sender<HostCallEnvelope>,
    next_id: Arc<AtomicU64>,
}

impl HostCallBroker {
    async fn call(&self, mut request: HostCallRequest) -> Result<ExecResult> {
        request.id = HostCallId(self.next_id.fetch_add(1, Ordering::Relaxed));
        let (response, response_rx) = oneshot::channel();
        self.requests
            .send(HostCallEnvelope { request, response })
            .await
            .map_err(|_| Error::Execution("host-call execution driver was dropped".to_string()))?;
        response_rx
            .await
            .map_err(|_| Error::Execution("host-call request was abandoned".to_string()))
    }
}

pub(crate) struct HostCallBuiltin {
    command: String,
}

impl HostCallBuiltin {
    pub(crate) fn new(command: String) -> Self {
        Self { command }
    }
}

#[crate::async_trait]
impl Builtin for HostCallBuiltin {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let Some(broker) = ctx.execution_extension::<HostCallBroker>() else {
            return Ok(ExecResult::err(
                format!(
                    "{}: host-call builtin requires Bash::start_execution\n",
                    self.command
                ),
                1,
            ));
        };
        let broker_value = broker
            .try_with(Clone::clone)
            .map_err(|_| Error::Cancelled)?;
        broker
            .run(broker_value.call(HostCallRequest {
                id: HostCallId(0),
                command: self.command.clone(),
                args: ctx.args.to_vec(),
                env: ctx.env.clone(),
                cwd: ctx.cwd.clone(),
                stdin: ctx.stdin.cloned(),
            }))
            .await
            .map_err(|_| Error::Cancelled)?
    }
}

type ExecutionFuture = Pin<Box<dyn Future<Output = ()> + Send>>;

/// What the execution reported when it finished. The session is `None` when
/// the deadline fired: a timed-out run drops the interpreter instead of
/// retaining it for a host that may never come back for it.
type ExecutionCompletion = (Option<Box<crate::Bash>>, Result<ExecResult>);

enum Driver {
    /// Built but not handed anywhere yet; the first `next_event` places it.
    Unstarted(ExecutionFuture),
    /// No task spawner on this target: the caller's `next_event` poll is the
    /// only thing that can advance execution.
    Inline(ExecutionFuture),
    /// Running on an independent task, which keeps the deadline armed even
    /// while the host is parked on a request.
    Spawned,
    /// Reported its result; the handle is spent.
    Finished,
}

/// Drives one process-local execution across event-backed builtin calls.
///
/// The handle controls the [`crate::Bash`] instance until execution completes.
/// Recover it with [`ExecutionHandle::into_bash`] after normal completion. A
/// timeout, or dropping a suspended handle, discards the session rather than
/// exposing partially unwound interpreter state.
pub struct ExecutionHandle {
    driver: Driver,
    completion: oneshot::Receiver<ExecutionCompletion>,
    abort: futures_util::future::AbortHandle,
    requests: mpsc::Receiver<HostCallEnvelope>,
    pending: HashMap<HostCallId, oneshot::Sender<ExecResult>>,
    completed_bash: Option<Box<crate::Bash>>,
}

impl std::fmt::Debug for ExecutionHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExecutionHandle")
            .field("active", &!matches!(self.driver, Driver::Finished))
            .field("pending_calls", &self.pending.len())
            .field("completed", &self.completed_bash.is_some())
            .finish()
    }
}

impl ExecutionHandle {
    pub(crate) fn new(bash: crate::Bash, script: String, mut options: ExecOptions) -> Self {
        // THREAT[TM-DOS-098]: one bounded slot prevents script-driven request
        // accumulation; the independently driven future keeps the execution
        // deadline running while the host holds a request.
        let (requests, request_rx) = mpsc::channel(1);
        let (completion_tx, completion) = oneshot::channel();
        let (abort, abort_registration) = futures_util::future::AbortHandle::new_pair();
        let broker = HostCallBroker {
            requests,
            next_id: Arc::new(AtomicU64::new(1)),
        };
        let _ = options.extensions.insert(broker);
        let mut bash = Box::new(bash);
        let execution = async move {
            let result = bash.exec_with_options(&script, options).await;
            // A run that hit its deadline releases the interpreter here rather
            // than parking it in the handle: bounding retention is the point,
            // and a partially unwound session is not worth reusing.
            let timed_out = matches!(
                result,
                Err(Error::ResourceLimit(crate::LimitExceeded::Timeout(_)))
            );
            let _ = completion_tx.send((if timed_out { None } else { Some(bash) }, result));
        };
        // Abortable so that dropping the handle stops a spawned driver
        // promptly instead of letting it run the script out in the background.
        let future = Box::pin(async move {
            let _ = futures_util::future::Abortable::new(execution, abort_registration).await;
        });
        Self {
            driver: Driver::Unstarted(future),
            completion,
            abort,
            requests: request_rx,
            pending: HashMap::new(),
            completed_bash: None,
        }
    }

    /// Run until the next host call or normal completion.
    pub async fn next_event(&mut self) -> Result<ExecutionEvent> {
        enum Next {
            Request(HostCallEnvelope),
            /// The broker was dropped: execution is finishing but has not
            /// reported yet.
            BrokerClosed,
            /// The inline future ran to completion; its result is already
            /// sitting in `completion`.
            InlineDone,
            Complete(std::result::Result<ExecutionCompletion, oneshot::error::RecvError>),
        }

        self.start_driver();

        let next = match &mut self.driver {
            Driver::Finished => {
                return Err(Error::Execution(
                    "execution handle has already completed".to_string(),
                ));
            }
            // `start_driver` just turned any `Unstarted` into one of the two
            // below; polling it here would still be correct if it somehow did
            // not, which is why this arm answers for both.
            Driver::Inline(future) | Driver::Unstarted(future) => tokio::select! {
                request = self.requests.recv() => match request {
                    Some(envelope) => Next::Request(envelope),
                    None => Next::BrokerClosed,
                },
                () = future.as_mut() => Next::InlineDone,
            },
            Driver::Spawned => tokio::select! {
                request = self.requests.recv() => match request {
                    Some(envelope) => Next::Request(envelope),
                    None => Next::BrokerClosed,
                },
                result = &mut self.completion => Next::Complete(result),
            },
        };

        match next {
            Next::Request(envelope) => {
                let id = envelope.request.id;
                self.pending.insert(id, envelope.response);
                Ok(ExecutionEvent::HostCall(envelope.request))
            }
            Next::Complete(completion) => self.settle(completion),
            // The future is spent; make sure nothing polls it again.
            Next::InlineDone => {
                self.driver = Driver::Finished;
                let completion = (&mut self.completion).await;
                self.settle(completion)
            }
            Next::BrokerClosed => {
                if let Driver::Inline(future) | Driver::Unstarted(future) = &mut self.driver {
                    future.as_mut().await;
                }
                let completion = (&mut self.completion).await;
                self.settle(completion)
            }
        }
    }

    /// Place the execution future on a task spawner, or keep it for inline
    /// polling when the target has none. Idempotent.
    fn start_driver(&mut self) {
        if !matches!(self.driver, Driver::Unstarted(_)) {
            return;
        }
        // `Spawned` is a placeholder while the future is moved out; it stands
        // only if `spawn_execution` actually took ownership.
        if let Driver::Unstarted(future) = std::mem::replace(&mut self.driver, Driver::Spawned)
            && let Some(future) = spawn_execution(future)
        {
            self.driver = Driver::Inline(future);
        }
    }

    fn settle(
        &mut self,
        completion: std::result::Result<ExecutionCompletion, oneshot::error::RecvError>,
    ) -> Result<ExecutionEvent> {
        self.driver = Driver::Finished;
        self.pending.clear();
        let (bash, result) = completion
            .map_err(|_| Error::Execution("host-call execution driver was dropped".to_string()))?;
        self.completed_bash = bash;
        result.map(ExecutionEvent::Complete)
    }

    /// Supply the shell result for a suspended host-call request.
    pub fn resume(&mut self, id: HostCallId, result: ExecResult) -> Result<()> {
        let response = self
            .pending
            .remove(&id)
            .ok_or_else(|| Error::Execution(format!("unknown host-call request {}", id.0)))?;
        response.send(result).map_err(|_| {
            Error::Execution(format!("host-call request {} is no longer active", id.0))
        })
    }

    /// Recover the session after a completion event or execution error.
    ///
    /// Returns the unchanged handle when execution is still active, and when
    /// the execution hit its wall-clock deadline — a timed-out run drops its
    /// session instead of retaining it.
    pub fn into_bash(mut self) -> std::result::Result<crate::Bash, Self> {
        match self.completed_bash.take() {
            Some(bash) => Ok(*bash),
            None => Err(self),
        }
    }
}

impl Drop for ExecutionHandle {
    fn drop(&mut self) {
        // Only a spawned driver outlives the handle; aborting is a no-op for
        // an inline one, which is dropped with the rest of these fields.
        self.abort.abort();
    }
}

/// Hand the execution to an independent task so its deadline keeps running
/// while the host is parked. Returns the future back when this target has no
/// task spawner, leaving `next_event` as its only driver.
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
fn spawn_execution(future: ExecutionFuture) -> Option<ExecutionFuture> {
    // Not every embedder polls `next_event` from inside a Tokio runtime, and
    // `start_execution` must not start panicking on the ones that don't.
    match tokio::runtime::Handle::try_current() {
        Ok(handle) => {
            handle.spawn(future);
            None
        }
        Err(_) => Some(future),
    }
}

#[cfg(all(target_arch = "wasm32", target_os = "unknown", feature = "wasm_js"))]
fn spawn_execution(future: ExecutionFuture) -> Option<ExecutionFuture> {
    wasm_bindgen_futures::spawn_local(future);
    None
}

// A non-JS wasm embedder has no JS event loop and no executor to spawn onto,
// so there is nothing that could drive a background task. The deadline is
// still enforced, just on the host's next poll. See
// knowledge/runtimes/non-js-wasm.md.
#[cfg(all(
    target_arch = "wasm32",
    target_os = "unknown",
    not(feature = "wasm_js")
))]
fn spawn_execution(future: ExecutionFuture) -> Option<ExecutionFuture> {
    Some(future)
}

// Targets without a task spawner take `Driver::Inline`, and CI only ever
// *builds* those (the non-JS wasm job). These exercise that path natively so
// it is covered by a running test rather than a compile check.
#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::ExecutionLimits;

    fn force_inline_driver(handle: &mut ExecutionHandle) {
        // `Spawned` is a placeholder while the future moves; it never stands.
        if let Driver::Unstarted(future) = std::mem::replace(&mut handle.driver, Driver::Spawned) {
            handle.driver = Driver::Inline(future);
        }
    }

    #[tokio::test]
    async fn inline_driver_suspends_and_resumes_a_host_call() {
        let bash = crate::Bash::builder().host_call_builtin("lookup").build();
        let mut execution = bash.start_execution("lookup alice");
        force_inline_driver(&mut execution);

        let request = match execution.next_event().await.unwrap() {
            ExecutionEvent::HostCall(request) => request,
            ExecutionEvent::Complete(_) => panic!("execution completed before its host call"),
        };
        assert_eq!(request.args(), &["alice"]);
        execution
            .resume(request.id(), ExecResult::ok("ok\n"))
            .unwrap();

        let result = match execution.next_event().await.unwrap() {
            ExecutionEvent::Complete(result) => result,
            ExecutionEvent::HostCall(_) => panic!("unexpected second host call"),
        };
        assert_eq!(result.stdout, "ok\n");
        let _bash = execution.into_bash().unwrap();
    }

    #[tokio::test]
    async fn inline_driver_completes_a_script_without_host_calls() {
        let bash = crate::Bash::builder().host_call_builtin("lookup").build();
        let mut execution = bash.start_execution("echo hi");
        force_inline_driver(&mut execution);

        let result = match execution.next_event().await.unwrap() {
            ExecutionEvent::Complete(result) => result,
            ExecutionEvent::HostCall(_) => panic!("unexpected host call"),
        };
        assert_eq!(result.stdout, "hi\n");
        assert!(execution.next_event().await.is_err());
    }

    /// THREAT[TM-DOS-098]: without a spawner the deadline cannot fire on its
    /// own, but it is still enforced the moment the host comes back, and the
    /// timed-out session is unrecoverable on every target.
    #[tokio::test(start_paused = true)]
    async fn inline_driver_enforces_the_deadline_on_the_next_poll() {
        let bash = crate::Bash::builder()
            .host_call_builtin("lookup")
            .limits(ExecutionLimits::new().timeout(Duration::from_secs(2)))
            .build();
        let mut execution = bash.start_execution("lookup alice");
        force_inline_driver(&mut execution);

        assert!(matches!(
            execution.next_event().await.unwrap(),
            ExecutionEvent::HostCall(_)
        ));
        tokio::time::advance(Duration::from_secs(3)).await;

        let error = execution.next_event().await.unwrap_err();
        assert!(error.to_string().contains("timeout"), "unexpected: {error}");
        assert!(execution.into_bash().is_err());
    }
}
