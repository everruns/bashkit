//! One execution builder, with explicit capture and async byte sinks. Engine
//! and output pumps run on the caller's executor; no threads are required.

use crate::interpreter::pipe::{PIPE_CAPACITY, Pipe, ReadEnd, WriteEnd};
use crate::{Bash, ExecOptions, ExecResult, ExecutionBudget, ExecutionLimits, Result};
use std::future::{Future, IntoFuture};
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncWrite, AsyncWriteExt};

pub(crate) struct ExecutionOutput {
    pub stdout: Arc<Pipe>,
    pub stderr: Arc<Pipe>,
    capture: Option<usize>,
    admitted: Mutex<Option<OutputAdmission>>,
}

struct OutputAdmission {
    stdout_output_limit: usize,
    stderr_output_limit: usize,
    stdout_limit: usize,
    stderr_limit: usize,
    budget: ExecutionBudget,
    _storage: crate::ExecutionBudgetLease,
    // The request ends after its final sink writes, not after engine polling.
    _completion: crate::limits::ExecutionBudgetCompletionGuard,
}

impl ExecutionOutput {
    fn new(capture: Option<usize>) -> Arc<Self> {
        Arc::new(Self {
            stdout: Pipe::new(),
            stderr: Pipe::new(),
            capture,
            admitted: Mutex::new(None),
        })
    }

    pub(crate) fn install(&self, budget: &ExecutionBudget, limits: &ExecutionLimits) -> Result<()> {
        let completion = budget.completion_guard();
        let out = self.capture.unwrap_or(0).min(limits.max_stdout_bytes);
        let err = self.capture.unwrap_or(0).min(limits.max_stderr_bytes);
        let bytes = out
            .checked_add(err)
            .and_then(|n| n.checked_add(2 * PIPE_CAPACITY))
            .ok_or_else(|| crate::Error::Execution("capture size overflow".into()))?;
        let lease = budget.lease_bytes(bytes)?;
        *self.admitted.lock().unwrap_or_else(|e| e.into_inner()) = Some(OutputAdmission {
            stdout_output_limit: limits.max_stdout_bytes,
            stderr_output_limit: limits.max_stderr_bytes,
            stdout_limit: out,
            stderr_limit: err,
            budget: budget.clone(),
            _storage: lease,
            _completion: completion,
        });
        Ok(())
    }

    fn capture_limit(&self, stderr: bool) -> usize {
        self.admitted
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .map_or(0, |a| {
                if stderr {
                    a.stderr_limit
                } else {
                    a.stdout_limit
                }
            })
    }

    fn output_limit(&self, stderr: bool) -> usize {
        self.admitted
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .map_or(0, |a| {
                if stderr {
                    a.stderr_output_limit
                } else {
                    a.stdout_output_limit
                }
            })
    }

    fn budget(&self) -> Option<ExecutionBudget> {
        self.admitted
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .map(|a| a.budget.clone())
    }
}

/// A single execution. With no capture or writer, output is drained and discarded.
/// Use [`capture`](Self::capture) for a bounded final value, writers for live
/// delivery, or both. Awaiting the builder runs it exactly once.
#[must_use = "an execution does nothing until awaited"]
pub struct Execution<'a> {
    bash: &'a mut Bash,
    script: &'a str,
    capture: Option<usize>,
    stdout: Option<Sink<'a>>,
    stderr: Option<Sink<'a>>,
    options: ExecOptions,
    cancel: Option<Arc<std::sync::atomic::AtomicBool>>,
}

impl<'a> Execution<'a> {
    pub(crate) fn new(bash: &'a mut Bash, script: &'a str) -> Self {
        Self {
            bash,
            script,
            capture: None,
            stdout: None,
            stderr: None,
            options: ExecOptions::new(),
            cancel: None,
        }
    }
    /// Capture at most this many bytes per stream, within sandbox output limits.
    pub fn capture(mut self, max_bytes: usize) -> Self {
        self.capture = Some(max_bytes);
        self
    }
    /// Deliver stdout through an async byte writer; awaiting writes backpressures.
    pub fn stdout(mut self, writer: &'a mut (dyn AsyncWrite + Unpin + Send)) -> Self {
        self.stdout = Some(Sink::Writer(writer));
        self
    }
    /// Deliver stderr through an async byte writer.
    pub fn stderr(mut self, writer: &'a mut (dyn AsyncWrite + Unpin + Send)) -> Self {
        self.stderr = Some(Sink::Writer(writer));
        self
    }
    /// Deliver stdout to an async callback. Chunks are bounded owned bytes.
    pub fn stdout_fn<F, Fut>(mut self, mut callback: F) -> Self
    where
        F: FnMut(Vec<u8>) -> Fut + Send + 'a,
        Fut: Future<Output = Result<()>> + Send + 'a,
    {
        self.stdout = Some(Sink::Callback(Box::new(move |bytes| {
            Box::pin(callback(bytes))
        })));
        self
    }
    /// Deliver stderr to an async callback.
    pub fn stderr_fn<F, Fut>(mut self, mut callback: F) -> Self
    where
        F: FnMut(Vec<u8>) -> Fut + Send + 'a,
        Fut: Future<Output = Result<()>> + Send + 'a,
    {
        self.stderr = Some(Sink::Callback(Box::new(move |bytes| {
            Box::pin(callback(bytes))
        })));
        self
    }
    /// Cancel when an external atomic flag is set. Existing session cancellation
    /// remains available through `Bash::cancellation_token`.
    pub fn cancel_with(mut self, token: Arc<std::sync::atomic::AtomicBool>) -> Self {
        self.cancel = Some(token);
        self
    }

    /// Invocation parameters, input and request-scoped extensions.
    pub fn options(mut self, options: ExecOptions) -> Self {
        self.options = options;
        self
    }

    async fn run(self) -> Result<ExecResult> {
        if !self.bash.interpreter.hooks().after_tool.is_empty()
            || !self.bash.interpreter.hooks().after_exec.is_empty()
        {
            return Err(crate::Error::Execution("live execution cannot apply whole-output after_tool/after_exec interceptors; use buffered execution".into()));
        }
        if self.options.output_callback.is_some() {
            return Err(crate::Error::Execution(
                "use stdout/stderr byte sinks instead of an ExecOptions output observer".into(),
            ));
        }
        let timeout = self.bash.interpreter.limits().timeout;
        let session_cancel = self.bash.cancellation_token();
        let output = ExecutionOutput::new(self.capture);
        let mut options = self.options;
        // The observer acts as the routing-scope marker already honored by
        // substitutions, redirects and upstream pipeline stages.
        options.output_callback = Some(Box::new(|_, _| {}));
        options.stream_output = Some(output.clone());
        let _ = options.extensions.insert(output.clone());
        // Keep driver combinators small on the stack. The interpreter's
        // recursive poll path is tested on a two-MiB native thread stack.
        let engine = Box::pin(async {
            let _stdout = WriteEnd(output.stdout.clone());
            let _stderr = WriteEnd(output.stderr.clone());
            // A shell failure still closes producer ends and drains accepted
            // output. A sink failure, by contrast, immediately drops the engine.
            Ok::<_, crate::Error>(Box::pin(self.bash.exec_with_options(self.script, options)).await)
        });
        let driven = Box::pin(async {
            tokio::try_join!(
                engine,
                Box::pin(pump(output.clone(), false, self.stdout)),
                Box::pin(pump(output.clone(), true, self.stderr)),
            )
        });
        let completed = async {
            let externally_driven = async {
                if let Some(cancel) = self.cancel {
                    tokio::select! {
                        result = driven => result,
                        () = wait_for_cancel(cancel) => Err(crate::Error::Cancelled),
                    }
                } else {
                    driven.await
                }
            };
            tokio::select! {
                result = externally_driven => result,
                () = wait_for_cancel(session_cancel) => Err(crate::Error::Cancelled),
            }
        };
        let (result, (stdout, stdout_truncated), (stderr, stderr_truncated)) =
            crate::time_compat::timeout(timeout, completed)
                .await
                .map_err(|_| {
                    crate::Error::ResourceLimit(crate::LimitExceeded::Timeout(timeout))
                })??;
        let mut result = result?;
        result.stdout = stdout.into();
        result.stderr = stderr.into();
        result.stdout_truncated |= stdout_truncated;
        result.stderr_truncated |= stderr_truncated;
        Ok(result)
    }
}

impl<'a> IntoFuture for Execution<'a> {
    type Output = Result<ExecResult>;
    type IntoFuture = Pin<Box<dyn Future<Output = Self::Output> + Send + 'a>>;
    fn into_future(self) -> Self::IntoFuture {
        Box::pin(self.run())
    }
}

type SinkFuture<'a> = Pin<Box<dyn Future<Output = Result<()>> + Send + 'a>>;
enum Sink<'a> {
    Writer(&'a mut (dyn AsyncWrite + Unpin + Send)),
    Callback(Box<dyn FnMut(Vec<u8>) -> SinkFuture<'a> + Send + 'a>),
}

async fn pump(
    output: Arc<ExecutionOutput>,
    stderr: bool,
    mut sink: Option<Sink<'_>>,
) -> Result<(Vec<u8>, bool)> {
    let pipe = if stderr {
        output.stderr.clone()
    } else {
        output.stdout.clone()
    };
    let _reader = ReadEnd(pipe.clone());
    let mut captured = Vec::new();
    let mut truncated = false;
    let mut delivered = 0usize;
    loop {
        let mut chunk = pipe.read_some().await;
        if chunk.is_empty() {
            break;
        }
        // Sandbox output caps apply to byte delivery as well as capture. A
        // smaller explicit capture limit never limits the sink itself.
        let accepted = chunk
            .len()
            .min(output.output_limit(stderr).saturating_sub(delivered));
        truncated |= accepted < chunk.len();
        chunk.truncate(accepted);
        delivered += accepted;
        let limit = output.capture_limit(stderr);
        if captured.capacity() == 0 && limit > 0 {
            captured = Vec::with_capacity(limit);
        }
        let n = chunk.len().min(limit.saturating_sub(captured.len()));
        captured.extend_from_slice(&chunk[..n]);
        truncated |= output.capture.is_some() && n < chunk.len();
        if !chunk.is_empty()
            && let Some(sink) = &mut sink
        {
            let budget = output.budget().ok_or(crate::Error::Cancelled)?;
            match sink {
                Sink::Writer(writer) => budget.run(writer.write_all(&chunk)).await??,
                Sink::Callback(callback) => budget.run(callback(chunk)).await??,
            }
        }
    }
    if let Some(Sink::Writer(writer)) = &mut sink
        && let Some(budget) = output.budget()
    {
        budget.run(writer.flush()).await??;
    }
    Ok((captured, truncated))
}

async fn wait_for_cancel(token: Arc<std::sync::atomic::AtomicBool>) {
    loop {
        if token.load(std::sync::atomic::Ordering::Relaxed) {
            return;
        }
        yield_once().await;
        crate::time_compat::sleep(std::time::Duration::from_millis(10)).await;
    }
}

pub(crate) async fn yield_once() {
    let mut yielded = false;
    std::future::poll_fn(move |cx| {
        if yielded {
            std::task::Poll::Ready(())
        } else {
            yielded = true;
            cx.waker().wake_by_ref();
            std::task::Poll::Pending
        }
    })
    .await
}
