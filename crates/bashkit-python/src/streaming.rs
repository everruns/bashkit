//! Native Python live command I/O. Each operation stays under the originating
//! Rust execution budget; Python never blocks the interpreter while awaiting it.

use super::*;
use bashkit::{Command, CommandContext, CommandInput, CommandOutput, CommandStatus};
use pyo3::exceptions::PyStopAsyncIteration;

#[pyclass(name = "CommandStatus", module = "bashkit", get_all, from_py_object)]
#[derive(Clone)]
pub(crate) struct PyCommandStatus {
    pub exit_code: i32,
}

#[pymethods]
impl PyCommandStatus {
    #[new]
    #[pyo3(signature = (exit_code=0))]
    fn new(exit_code: i32) -> Self {
        Self { exit_code }
    }
}

#[pyclass(name = "CommandInput", module = "bashkit", skip_from_py_object)]
pub(crate) struct PyCommandInput {
    input: CommandInput,
}

#[pymethods]
impl PyCommandInput {
    fn __aiter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }
    fn __anext__<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let input = self.input.clone();
        future_into_py(py, async move {
            let chunk = input
                .read_chunk()
                .await
                .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
            match chunk {
                Some(bytes) => {
                    Python::attach(|py| Ok(PyBytes::new(py, &bytes).into_any().unbind()))
                }
                None => Err(PyStopAsyncIteration::new_err(())),
            }
        })
    }
    fn read_chunk<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let input = self.input.clone();
        future_into_py(py, async move {
            let bytes = input
                .read_chunk()
                .await
                .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
            Python::attach(|py| {
                Ok(bytes.map_or_else(|| py.None(), |b| PyBytes::new(py, &b).into_any().unbind()))
            })
        })
    }
}

#[pyclass(name = "CommandOutput", module = "bashkit", skip_from_py_object)]
pub(crate) struct PyCommandOutput {
    output: CommandOutput,
}

#[pymethods]
impl PyCommandOutput {
    fn write<'py>(&self, py: Python<'py>, bytes: Vec<u8>) -> PyResult<Bound<'py, PyAny>> {
        let output = self.output.clone();
        future_into_py(py, async move {
            output.write_all(&bytes).await.map_err(|e| match e {
                bashkit::Error::Io(e) if e.kind() == std::io::ErrorKind::BrokenPipe => {
                    pyo3::exceptions::PyBrokenPipeError::new_err("pipe reader closed")
                }
                e => PyRuntimeError::new_err(e.to_string()),
            })?;
            Ok(())
        })
    }
}

#[pyclass(
    name = "CommandContext",
    module = "bashkit",
    get_all,
    skip_from_py_object
)]
pub(crate) struct PyCommandContext {
    name: String,
    argv: Vec<String>,
    env: HashMap<String, String>,
    cwd: String,
    fs: Py<PyFileSystem>,
    stdin: Py<PyCommandInput>,
    stdout: Py<PyCommandOutput>,
    stderr: Py<PyCommandOutput>,
}

struct PyCommand {
    name: String,
    callback: Py<PyAny>,
    rt: PyRuntime,
}

#[async_trait]
impl Command for PyCommand {
    async fn execute(&self, ctx: CommandContext<'_>) -> bashkit::Result<CommandStatus> {
        let session = ctx
            .execution_extension::<Arc<PyCallbackSession>>()
            .ok_or_else(|| bashkit::Error::Execution("missing Python command session".into()))?;
        let callback_session = session
            .try_with(Clone::clone)
            .map_err(|_| bashkit::Error::Cancelled)?;
        let arg = Python::attach(|py| -> PyResult<Py<PyAny>> {
            Ok(Py::new(
                py,
                PyCommandContext {
                    name: self.name.clone(),
                    argv: ctx.args.to_vec(),
                    env: ctx.env.clone(),
                    cwd: ctx.cwd.to_string_lossy().into_owned(),
                    fs: Py::new(
                        py,
                        PyFileSystem::from_static(ctx.fs.clone(), self.rt.clone()),
                    )?,
                    stdin: Py::new(
                        py,
                        PyCommandInput {
                            input: ctx.stdin.clone(),
                        },
                    )?,
                    stdout: Py::new(
                        py,
                        PyCommandOutput {
                            output: ctx.stdout.clone(),
                        },
                    )?,
                    stderr: Py::new(
                        py,
                        PyCommandOutput {
                            output: ctx.stderr.clone(),
                        },
                    )?,
                },
            )?
            .into_any())
        })
        .map_err(|e| bashkit::Error::Execution(e.to_string()))?;
        let result = session
            .run(call_python_callback_async(
                callback_session,
                &self.name,
                &self.callback,
                vec![arg],
            ))
            .await
            .map_err(|_| bashkit::Error::Cancelled)?;
        match result {
            Ok(value) => Python::attach(|py| {
                let status = value
                    .bind(py)
                    .extract::<PyRef<'_, PyCommandStatus>>()
                    .map(|s| s.exit_code)
                    .or_else(|_| value.bind(py).extract::<i32>());
                status.map(CommandStatus::with_code).map_err(|_| {
                    bashkit::Error::Execution("command must return CommandStatus or int".into())
                })
            }),
            Err(message) => {
                ctx.stderr.write_all(message.as_bytes()).await?;
                Ok(CommandStatus::with_code(1))
            }
        }
    }
}

pub(crate) fn register(
    py: Python<'_>,
    name: String,
    callback: Py<PyAny>,
    rt: &PyRuntime,
    entries: &StdMutex<Vec<PyCustomBuiltinEntry>>,
    registry: &BuiltinRegistry,
) -> PyResult<()> {
    let entry = build_py_custom_builtin_entry(py, name.clone(), callback)?;
    if !entry.is_async {
        return Err(PyTypeError::new_err(
            "streaming commands must be async callables",
        ));
    }
    let command = PyCommand {
        name: name.clone(),
        callback: entry.callback.clone_ref(py),
        rt: rt.clone(),
    };
    let mut entries = entries
        .lock()
        .map_err(|_| PyRuntimeError::new_err("command registry lock poisoned"))?;
    entries.retain(|e| e.name != name);
    entries.push(entry);
    registry.insert_command(name, command);
    Ok(())
}

pub(crate) async fn execute(
    bash: &mut Bash,
    script: &str,
    capture: Option<usize>,
    stdout: Option<Py<PyAny>>,
    stderr: Option<Py<PyAny>>,
    session: Arc<PyCallbackSession>,
) -> PyResult<ExecResult> {
    session.check_cancelled()?;
    let mut extensions = ExecutionExtensions::new();
    let _ = extensions.insert(session.clone());
    let mut execution = bash
        .execute(script)
        .options(bashkit::ExecOptions::new().extensions(extensions));
    if let Some(limit) = capture {
        execution = execution.capture(limit);
    }
    if let Some(callback) = stdout {
        let session = session.clone();
        execution = execution.stdout_fn(move |bytes| {
            let callback = Python::attach(|py| callback.clone_ref(py));
            let session = session.clone();
            async move { deliver(session, callback, bytes).await }
        });
    }
    if let Some(callback) = stderr {
        let session = session.clone();
        execution = execution.stderr_fn(move |bytes| {
            let callback = Python::attach(|py| callback.clone_ref(py));
            let session = session.clone();
            async move { deliver(session, callback, bytes).await }
        });
    }
    Ok(py_exec_result_from_bash_result(execution.await))
}

async fn deliver(
    session: Arc<PyCallbackSession>,
    callback: Py<PyAny>,
    bytes: Vec<u8>,
) -> bashkit::Result<()> {
    let args = Python::attach(|py| vec![PyBytes::new(py, &bytes).into_any().unbind()]);
    call_python_callback_async(session, "output sink", &callback, args)
        .await
        .map(|_| ())
        .map_err(bashkit::Error::Execution)
}

pub(crate) fn validate_sink(py: Python<'_>, callback: Option<&Py<PyAny>>) -> PyResult<()> {
    if let Some(callback) = callback
        && !is_coroutine_callable(py, callback.bind(py))?
    {
        return Err(PyTypeError::new_err(
            "output sinks must be async callables receiving bytes",
        ));
    }
    Ok(())
}

pub(crate) fn add_classes(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyCommandStatus>()?;
    m.add_class::<PyCommandContext>()?;
    m.add_class::<PyCommandInput>()?;
    m.add_class::<PyCommandOutput>()?;
    Ok(())
}
