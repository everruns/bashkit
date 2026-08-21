// Host calls use an independently driven live Rust future so execution
// deadlines remain active while the host is parked. They are not interpreter
// snapshots and cannot be serialized or resumed elsewhere.

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
type ExecutionCompletion = (Option<Box<crate::Bash>>, Result<ExecResult>);

/// Drives one process-local execution across event-backed builtin calls.
///
/// The handle controls the driver that owns the [`crate::Bash`] instance until
/// execution completes. Recover it with [`ExecutionHandle::into_bash`] after
/// normal completion. Timeout or handle drop discards the session rather than
/// exposing partially unwound interpreter state.
pub struct ExecutionHandle {
    unstarted: Option<ExecutionFuture>,
    completion: oneshot::Receiver<ExecutionCompletion>,
    abort: futures_util::future::AbortHandle,
    requests: mpsc::Receiver<HostCallEnvelope>,
    pending: HashMap<HostCallId, oneshot::Sender<ExecResult>>,
    completed_bash: Option<Box<crate::Bash>>,
    finished: bool,
}

impl std::fmt::Debug for ExecutionHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExecutionHandle")
            .field("active", &!self.finished)
            .field("pending_calls", &self.pending.len())
            .field("completed", &self.completed_bash.is_some())
            .finish()
    }
}

impl ExecutionHandle {
    pub(crate) fn new(bash: crate::Bash, script: String, mut options: ExecOptions) -> Self {
        // THREAT[TM-DOS-098]: the bounded request slot prevents accumulation;
        // the independently driven future enforces the deadline while parked.
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
            (bash, result)
        };
        let future = Box::pin(async move {
            let (bash, result) = execution.await;
            let timed_out = matches!(
                result,
                Err(Error::ResourceLimit(crate::LimitExceeded::Timeout(_)))
            );
            let completion = (if timed_out { None } else { Some(bash) }, result);
            let _ = completion_tx.send(completion);
        });
        let future = Box::pin(async move {
            let _ = futures_util::future::Abortable::new(future, abort_registration).await;
        });
        Self {
            unstarted: Some(future),
            completion,
            abort,
            requests: request_rx,
            pending: HashMap::new(),
            completed_bash: None,
            finished: false,
        }
    }

    /// Run until the next host call or normal completion.
    pub async fn next_event(&mut self) -> Result<ExecutionEvent> {
        enum Next {
            Request(Option<HostCallEnvelope>),
            Complete(ExecutionCompletion),
        }

        if let Some(future) = self.unstarted.take() {
            spawn_execution(future);
        }
        if self.finished {
            return Err(Error::Execution(
                "execution handle has already completed".to_string(),
            ));
        }
        let next = tokio::select! {
            request = self.requests.recv() => Next::Request(request),
            result = &mut self.completion => Next::Complete(result.map_err(|_| {
                Error::Execution("host-call execution driver was dropped".to_string())
            })?),
        };
        match next {
            Next::Request(Some(envelope)) => {
                let id = envelope.request.id;
                self.pending.insert(id, envelope.response);
                Ok(ExecutionEvent::HostCall(envelope.request))
            }
            Next::Request(None) => {
                let (bash, result) = (&mut self.completion).await.map_err(|_| {
                    Error::Execution("host-call execution driver was dropped".to_string())
                })?;
                self.pending.clear();
                self.completed_bash = bash;
                self.finished = true;
                result.map(ExecutionEvent::Complete)
            }
            Next::Complete((bash, result)) => {
                self.pending.clear();
                self.completed_bash = bash;
                self.finished = true;
                result.map(ExecutionEvent::Complete)
            }
        }
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
    /// Returns the unchanged handle when execution is still active.
    pub fn into_bash(mut self) -> std::result::Result<crate::Bash, Self> {
        match self.completed_bash.take() {
            Some(bash) => Ok(*bash),
            None => Err(self),
        }
    }
}

impl Drop for ExecutionHandle {
    fn drop(&mut self) {
        self.abort.abort();
    }
}

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
fn spawn_execution(future: ExecutionFuture) {
    tokio::spawn(async move {
        let _ = future.await;
    });
}

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
fn spawn_execution(future: ExecutionFuture) {
    wasm_bindgen_futures::spawn_local(async move {
        let _ = future.await;
    });
}
