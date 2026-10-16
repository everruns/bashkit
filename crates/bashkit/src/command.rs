//! Streaming host commands. I/O is request-scoped and byte-native; completion
//! never carries output. Buffered shell utilities use the interpreter's adapter.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use crate::interpreter::pipe::{PIPE_CAPACITY, Pipe};
use crate::{
    Builtin, BuiltinContext, Error, ExecResult, ExecutionBudget, FileSystem, Result, StreamData,
};
use async_trait::async_trait;

/// Completion of a host command, separate from its output destinations.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CommandStatus {
    /// Shell exit status. Nonzero status is an ordinary command failure.
    pub exit_code: i32,
}

impl CommandStatus {
    /// Successful completion.
    pub const fn success() -> Self {
        Self { exit_code: 0 }
    }
    /// Complete with an explicit shell status.
    pub const fn with_code(exit_code: i32) -> Self {
        Self { exit_code }
    }
}

/// Implement a live command registered with [`crate::BashBuilder::command`].
#[async_trait]
pub trait Command: Send + Sync {
    /// Consume input and write output before returning completion.
    async fn execute(&self, ctx: CommandContext<'_>) -> Result<CommandStatus>;
}

/// Capabilities available for one command invocation.
pub struct CommandContext<'a> {
    /// Registered command name.
    pub name: &'a str,
    /// Expanded arguments, excluding the command name.
    pub args: &'a [String],
    /// Effective exported environment, including prefix assignments.
    pub env: &'a HashMap<String, String>,
    /// Current virtual working directory.
    pub cwd: &'a Path,
    /// Request-scoped virtual filesystem.
    pub fs: Arc<dyn FileSystem>,
    /// Incremental standard input. An empty stream is EOF, not absent input.
    pub stdin: CommandInput,
    /// Standard output after shell routing.
    pub stdout: CommandOutput,
    /// Standard error after shell routing.
    pub stderr: CommandOutput,
    extensions: Arc<crate::ExecutionExtensions>,
}

impl CommandContext<'_> {
    /// A revocable request-scoped host extension.
    pub fn execution_extension<T: Send + Sync + 'static>(
        &self,
    ) -> Option<crate::ExecutionCapability<T>> {
        self.extensions.get::<T>()
    }
}

struct InputState {
    buffered: StreamData,
    offset: usize,
    pipe: Option<Arc<Pipe>>,
    _storage: crate::ExecutionBudgetLease,
}

/// A clone shares the input cursor; every operation checks its request budget.
#[derive(Clone)]
pub struct CommandInput {
    state: Arc<tokio::sync::Mutex<InputState>>,
    budget: ExecutionBudget,
}

impl CommandInput {
    /// Read at most one pipe-sized chunk. `None` means EOF.
    pub async fn read_chunk(&self) -> Result<Option<Vec<u8>>> {
        self.budget.check()?;
        let bytes = self
            .budget
            .run(async {
                let mut state = self.state.lock().await;
                if state.offset < state.buffered.len() {
                    let end = (state.offset + PIPE_CAPACITY).min(state.buffered.len());
                    let bytes = state.buffered.as_bytes()[state.offset..end].to_vec();
                    state.offset = end;
                    bytes
                } else if let Some(pipe) = &state.pipe {
                    pipe.read_some().await
                } else {
                    Vec::new()
                }
            })
            .await?;
        self.budget.consume_input(bytes.len())?;
        self.budget
            .consume_work(1 + bytes.len().div_ceil(1024) as u64)?;
        Ok((!bytes.is_empty()).then_some(bytes))
    }
}

struct Capture {
    bytes: crate::limits::BudgetedVec<u8>,
    truncated: bool,
    limit: usize,
}

impl Capture {
    fn new(budget: &ExecutionBudget, limit: usize) -> Result<Arc<Mutex<Self>>> {
        Ok(Arc::new(Mutex::new(Self {
            bytes: crate::limits::BudgetedVec::new(Some(budget))?,
            truncated: false,
            limit,
        })))
    }
}

pub(crate) struct FileDestination {
    fs: Arc<dyn FileSystem>,
    path: PathBuf,
    lock: tokio::sync::Mutex<()>,
    _storage: crate::ExecutionBudgetLease,
}

#[derive(Clone)]
pub(crate) enum CommandRoute {
    Stdout,
    Stderr,
    File(Arc<FileDestination>),
    Pipe(Arc<Pipe>),
    Discard,
    Closed,
}

impl CommandRoute {
    pub(crate) fn file(
        fs: Arc<dyn FileSystem>,
        path: PathBuf,
        budget: &ExecutionBudget,
    ) -> Result<Self> {
        let storage =
            budget.lease_bytes(path.as_os_str().len() + std::mem::size_of::<FileDestination>())?;
        Ok(Self::File(Arc::new(FileDestination {
            fs,
            path,
            lock: tokio::sync::Mutex::new(()),
            _storage: storage,
        })))
    }
}

#[derive(Clone)]
enum Destination {
    File(Arc<FileDestination>),
    Discard,
    Closed,
    Pipe(Arc<Pipe>),
    Capture(Arc<Mutex<Capture>>),
}

/// An async output destination. Clones share ordering and request lifetime.
#[derive(Clone)]
pub struct CommandOutput {
    destination: Destination,
    budget: ExecutionBudget,
}

impl CommandOutput {
    /// Write all bytes, suspending while a pipe is full. A reader that exits
    /// causes `Error::Io` with `BrokenPipe`; handlers may handle that error.
    pub async fn write_all(&self, bytes: &[u8]) -> Result<()> {
        self.budget.check()?;
        self.budget
            .consume_work(1 + bytes.len().div_ceil(1024) as u64)?;
        match &self.destination {
            Destination::File(file) => {
                self.budget
                    .run(async {
                        let _lock = file.lock.lock().await;
                        for chunk in bytes.chunks(PIPE_CAPACITY) {
                            file.fs.append_file(&file.path, chunk).await?;
                        }
                        Ok::<_, Error>(())
                    })
                    .await??;
            }
            Destination::Discard => {}
            Destination::Closed => return Err(std::io::Error::from_raw_os_error(9).into()),
            Destination::Pipe(pipe) => self.budget.run(pipe.write_bounded(bytes)).await??,
            Destination::Capture(capture) => {
                let mut capture = capture.lock().unwrap_or_else(|e| e.into_inner());
                let n = bytes
                    .len()
                    .min(capture.limit.saturating_sub(capture.bytes.len()));
                let limit = capture.limit;
                capture
                    .bytes
                    .try_extend_from_slice_with_limit(&bytes[..n], limit)?;
                capture.truncated |= n < bytes.len();
            }
        }
        self.budget.check()?;
        Ok(())
    }
}

// Failure to report an ordinary I/O error through a closed stderr must not
// abort the shell. Request-budget failures remain fatal during diagnostics.
async fn write_diagnostic(output: &CommandOutput, bytes: &[u8]) -> Result<()> {
    match output.write_all(bytes).await {
        Err(Error::Io(_)) => Ok(()),
        result => result,
    }
}

pub(crate) struct CommandAdapter {
    pub name: String,
    pub command: Arc<dyn Command>,
}

#[async_trait]
impl Builtin for CommandAdapter {
    fn streams_io(&self) -> bool {
        true
    }

    async fn execute(&self, ctx: BuiltinContext<'_>) -> Result<ExecResult> {
        let budget = ctx
            .execution_budget()
            .ok_or_else(|| Error::Internal("missing command budget".into()))?
            .try_with(Clone::clone)
            .map_err(|_| Error::Cancelled)?;
        let limits = ctx
            .shell
            .as_ref()
            .map(|s| s.limits())
            .cloned()
            .unwrap_or_default();
        let out_capture = Capture::new(
            &budget,
            if ctx.shell.as_ref().is_some_and(|s| s.stdout_pipe.is_some()) {
                0
            } else {
                limits.max_stdout_bytes
            },
        )?;
        let err_capture = Capture::new(&budget, limits.max_stderr_bytes)?;
        let stdout = ctx
            .shell
            .as_ref()
            .and_then(|s| s.stdout_pipe.clone())
            .map(Destination::Pipe)
            .unwrap_or_else(|| Destination::Capture(out_capture.clone()));
        let stderr = ctx
            .shell
            .as_ref()
            .and_then(|s| s.stderr_pipe.clone())
            .map(Destination::Pipe)
            .unwrap_or_else(|| Destination::Capture(err_capture.clone()));
        let resolve = |route: &CommandRoute| match route {
            CommandRoute::Stdout => stdout.clone(),
            CommandRoute::Stderr => stderr.clone(),
            CommandRoute::File(file) => Destination::File(file.clone()),
            CommandRoute::Pipe(pipe) => Destination::Pipe(pipe.clone()),
            CommandRoute::Discard => Destination::Discard,
            CommandRoute::Closed => Destination::Closed,
        };
        let (stdout, stderr) = match ctx.shell.as_ref().and_then(|s| s.command_routes.as_ref()) {
            Some([out, err]) => (resolve(out), resolve(err)),
            None => (stdout, stderr),
        };
        let input_pipe = ctx.shell.as_ref().and_then(|s| s.stdin_pipe.clone());
        let input_storage = budget.lease_bytes(ctx.stdin.map_or(0, StreamData::len))?;
        let buffered = ctx.stdin.cloned().unwrap_or_default();
        let input = CommandInput {
            budget: budget.clone(),
            state: Arc::new(tokio::sync::Mutex::new(InputState {
                buffered,
                offset: 0,
                pipe: input_pipe,
                _storage: input_storage,
            })),
        };
        let error_output = CommandOutput {
            destination: stderr.clone(),
            budget: budget.clone(),
        };
        let result = self.command.execute(CommandContext {
            name: &self.name,
            args: ctx.args,
            env: ctx.env,
            cwd: ctx.cwd,
            fs: ctx.fs.clone(),
            stdin: input,
            stdout: CommandOutput {
                destination: stdout,
                budget: budget.clone(),
            },
            stderr: CommandOutput {
                destination: stderr,
                budget: budget.clone(),
            },
            extensions: ctx
                .shell
                .as_ref()
                .map(|s| s.execution_extensions.clone())
                .unwrap_or_else(|| Arc::new(crate::ExecutionExtensions::new())),
        });
        use futures_util::FutureExt;
        let status = match budget
            .run(std::panic::AssertUnwindSafe(result).catch_unwind())
            .await?
        {
            Ok(Ok(status)) => status,
            Ok(Err(Error::Io(e))) if e.kind() == std::io::ErrorKind::BrokenPipe => {
                CommandStatus::with_code(141)
            }
            Ok(Err(Error::Io(e))) => {
                let message = format!(
                    "{}: write error: {}\n",
                    self.name,
                    crate::error::io_error_reason(&Error::Io(e))
                );
                write_diagnostic(&error_output, message.as_bytes()).await?;
                CommandStatus::with_code(1)
            }
            Ok(Err(e)) => return Err(e),
            Err(_) => {
                write_diagnostic(&error_output, b"command failed unexpectedly\n").await?;
                CommandStatus::with_code(1)
            }
        };
        let mut out = out_capture.lock().unwrap_or_else(|e| e.into_inner());
        let mut err = err_capture.lock().unwrap_or_else(|e| e.into_inner());
        Ok(ExecResult {
            stdout: std::mem::replace(&mut out.bytes, crate::limits::BudgetedVec::new(None)?)
                .into_parts()
                .0
                .into(),
            stderr: std::mem::replace(&mut err.bytes, crate::limits::BudgetedVec::new(None)?)
                .into_parts()
                .0
                .into(),
            stdout_truncated: out.truncated,
            stderr_truncated: err.truncated,
            exit_code: status.exit_code,
            ..Default::default()
        })
    }
}
