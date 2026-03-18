//! Bashkit Python package
//!
//! Primary interface: `Bash` — the core interpreter with virtual filesystem.
//! Convenience wrapper: `BashTool` — adds contract metadata (`description`,
//! `help`, `system_prompt`, JSON schemas) on top of the core interpreter.
//! Orchestration: `ScriptedTool` — composes Python callbacks as bash builtins.

use bashkit::tool::VERSION;
use bashkit::{
    Bash, BashTool as RustBashTool, ExcType, ExecutionLimits, ExtFunctionResult, MontyException,
    MontyObject, PythonExternalFnHandler, PythonLimits, ScriptedTool as RustScriptedTool, Tool,
    ToolArgs, ToolDef, ToolRequest,
};
use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyDict, PyFloat, PyFrozenSet, PyInt, PyList, PySet, PyTuple};
use pyo3_async_runtimes::tokio::future_into_py;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::Mutex;

// ============================================================================
// JSON <-> Python helpers
// ============================================================================

/// Convert serde_json::Value → Py<PyAny>
const MAX_NESTING_DEPTH: usize = 64;

fn json_to_py(py: Python<'_>, val: &serde_json::Value) -> PyResult<Py<PyAny>> {
    json_to_py_inner(py, val, 0)
}

fn json_to_py_inner(py: Python<'_>, val: &serde_json::Value, depth: usize) -> PyResult<Py<PyAny>> {
    if depth > MAX_NESTING_DEPTH {
        return Err(pyo3::exceptions::PyValueError::new_err(
            "JSON nesting depth exceeds maximum of 64",
        ));
    }
    match val {
        serde_json::Value::Null => Ok(py.None()),
        serde_json::Value::Bool(b) => Ok(b.into_pyobject(py)?.to_owned().into_any().unbind()),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(i.into_pyobject(py)?.into_any().unbind())
            } else if let Some(f) = n.as_f64() {
                Ok(f.into_pyobject(py)?.into_any().unbind())
            } else {
                Ok(py.None())
            }
        }
        serde_json::Value::String(s) => Ok(s.into_pyobject(py)?.into_any().unbind()),
        serde_json::Value::Array(arr) => {
            let items: Vec<Py<PyAny>> = arr
                .iter()
                .map(|v| json_to_py_inner(py, v, depth + 1))
                .collect::<PyResult<_>>()?;
            Ok(PyList::new(py, &items)?.into_any().unbind())
        }
        serde_json::Value::Object(map) => {
            let dict = PyDict::new(py);
            for (k, v) in map {
                dict.set_item(k, json_to_py_inner(py, v, depth + 1)?)?;
            }
            Ok(dict.into_any().unbind())
        }
    }
}

/// Convert Py<PyAny> → serde_json::Value (for schema dicts)
fn py_to_json(py: Python<'_>, obj: &Bound<'_, pyo3::PyAny>) -> PyResult<serde_json::Value> {
    py_to_json_inner(py, obj, 0)
}

#[allow(clippy::only_used_in_recursion)]
fn py_to_json_inner(
    py: Python<'_>,
    obj: &Bound<'_, pyo3::PyAny>,
    depth: usize,
) -> PyResult<serde_json::Value> {
    if depth > MAX_NESTING_DEPTH {
        return Err(pyo3::exceptions::PyValueError::new_err(
            "Python object nesting depth exceeds maximum of 64",
        ));
    }
    if obj.is_none() {
        return Ok(serde_json::Value::Null);
    }
    if let Ok(b) = obj.extract::<bool>() {
        return Ok(serde_json::Value::Bool(b));
    }
    if let Ok(i) = obj.extract::<i64>() {
        return Ok(serde_json::json!(i));
    }
    if let Ok(f) = obj.extract::<f64>() {
        return Ok(serde_json::json!(f));
    }
    if let Ok(s) = obj.extract::<String>() {
        return Ok(serde_json::Value::String(s));
    }
    if let Ok(list) = obj.cast::<PyList>() {
        let arr: Vec<serde_json::Value> = list
            .iter()
            .map(|item| py_to_json_inner(py, &item, depth + 1))
            .collect::<PyResult<_>>()?;
        return Ok(serde_json::Value::Array(arr));
    }
    if let Ok(dict) = obj.cast::<PyDict>() {
        let mut map = serde_json::Map::new();
        for (k, v) in dict.iter() {
            let key: String = k.extract()?;
            map.insert(key, py_to_json_inner(py, &v, depth + 1)?);
        }
        return Ok(serde_json::Value::Object(map));
    }
    // Fallback: str()
    let s = obj.str()?.extract::<String>()?;
    Ok(serde_json::Value::String(s))
}

// ============================================================================
// ExecResult
// ============================================================================

/// Result from executing bash commands
#[pyclass(from_py_object)]
#[derive(Clone)]
pub struct ExecResult {
    #[pyo3(get)]
    pub stdout: String,
    #[pyo3(get)]
    pub stderr: String,
    #[pyo3(get)]
    pub exit_code: i32,
    #[pyo3(get)]
    pub error: Option<String>,
    #[pyo3(get)]
    pub stdout_truncated: bool,
    #[pyo3(get)]
    pub stderr_truncated: bool,
    #[pyo3(get)]
    pub final_env: Option<std::collections::HashMap<String, String>>,
}

#[pymethods]
impl ExecResult {
    fn __repr__(&self) -> String {
        format!(
            "ExecResult(stdout={:?}, stderr={:?}, exit_code={}, error={:?}, stdout_truncated={}, stderr_truncated={}, final_env={:?})",
            self.stdout,
            self.stderr,
            self.exit_code,
            self.error,
            self.stdout_truncated,
            self.stderr_truncated,
            self.final_env
        )
    }

    fn __str__(&self) -> String {
        if self.exit_code == 0 {
            self.stdout.clone()
        } else {
            format!("Error ({}): {}", self.exit_code, self.stderr)
        }
    }

    /// Check if command succeeded
    #[getter]
    fn success(&self) -> bool {
        self.exit_code == 0
    }

    /// Return output as dict
    fn to_dict(&self) -> pyo3::PyResult<pyo3::Py<PyDict>> {
        Python::attach(|py| {
            let dict = PyDict::new(py);
            dict.set_item("stdout", &self.stdout)?;
            dict.set_item("stderr", &self.stderr)?;
            dict.set_item("exit_code", self.exit_code)?;
            dict.set_item("error", &self.error)?;
            dict.set_item("stdout_truncated", self.stdout_truncated)?;
            dict.set_item("stderr_truncated", self.stderr_truncated)?;
            dict.set_item("final_env", &self.final_env)?;
            Ok(dict.into())
        })
    }
}

// ============================================================================
// Bash — core interpreter
// ============================================================================

/// Build a `PythonExternalFnHandler` from a Python async callable.
///
/// The handler converts MontyObject args/kwargs to Python objects, calls the
/// async handler coroutine, awaits it, and converts the result back.
fn make_external_handler(py_handler: Py<PyAny>) -> PythonExternalFnHandler {
    Arc::new(move |fn_name, args, kwargs| {
        let py_handler = Python::attach(|py| py_handler.clone_ref(py));
        Box::pin(async move {
            let fut = Python::attach(|py| {
                let py_args = args
                    .iter()
                    .map(|o| monty_to_py(py, o))
                    .collect::<PyResult<Vec<_>>>()?;
                let py_args_list = PyList::new(py, &py_args)?;
                let py_kwargs = PyDict::new(py);
                for (k, v) in &kwargs {
                    py_kwargs.set_item(monty_to_py(py, k)?, monty_to_py(py, v)?)?;
                }
                let coro = py_handler.call1(py, (fn_name, py_args_list, py_kwargs))?;
                pyo3_async_runtimes::tokio::into_future(coro.into_bound(py))
            });
            match fut {
                Err(e) => ExtFunctionResult::Error(MontyException::new(
                    ExcType::RuntimeError,
                    Some(e.to_string()),
                )),
                Ok(awaitable) => match awaitable.await {
                    Err(e) => ExtFunctionResult::Error(MontyException::new(
                        ExcType::RuntimeError,
                        Some(e.to_string()),
                    )),
                    Ok(py_result) => {
                        Python::attach(|py| match py_to_monty(py, py_result.bind(py)) {
                            Ok(v) => ExtFunctionResult::Return(v),
                            Err(e) => ExtFunctionResult::Error(MontyException::new(
                                ExcType::RuntimeError,
                                Some(e.to_string()),
                            )),
                        })
                    }
                },
            }
        })
    })
}

/// Apply python/external_handler configuration to a `BashBuilder`.
///
/// Centralises the logic shared between `new()` and `reset()`.
fn apply_python_config(
    mut builder: bashkit::BashBuilder,
    python: bool,
    fn_names: Vec<String>,
    handler: Option<Py<PyAny>>,
) -> bashkit::BashBuilder {
    // By construction, handler.is_some() implies python=true (validated in new()).
    match (python, handler) {
        (true, Some(h)) => {
            builder = builder.python_with_external_handler(
                PythonLimits::default(),
                fn_names,
                make_external_handler(h),
            );
        }
        (true, None) => {
            builder = builder.python();
        }
        (false, _) => {}
    }
    builder
}

/// Core bash interpreter with virtual filesystem.
///
/// State persists between calls — files created in one `execute()` are
/// available in subsequent calls. This is the primary interface.
///
/// Example:
///     ```python
///     from bashkit import Bash
///
///     bash = Bash()
///     result = await bash.execute("echo 'Hello, World!'")
///     print(result.stdout)  # Hello, World!
///     ```
#[pyclass(name = "Bash")]
#[allow(dead_code)]
pub struct PyBash {
    inner: Arc<Mutex<Bash>>,
    /// Shared tokio runtime — reused across all sync calls to avoid
    /// per-call OS thread/fd exhaustion (issue #414).
    rt: tokio::runtime::Runtime,
    cancelled: Arc<AtomicBool>,
    username: Option<String>,
    hostname: Option<String>,
    /// Whether Monty Python execution is enabled (`python`/`python3` builtins).
    python: bool,
    /// External function names callable from Monty code via the handler.
    external_functions: Vec<String>,
    /// Async Python callable invoked when Monty calls an external function.
    external_handler: Option<Py<PyAny>>,
    max_commands: Option<u64>,
    max_loop_iterations: Option<u64>,
}

#[pymethods]
impl PyBash {
    #[new]
    #[pyo3(signature = (
        username=None,
        hostname=None,
        max_commands=None,
        max_loop_iterations=None,
        python=false,
        external_functions=None,
        external_handler=None,
    ))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        py: Python<'_>,
        username: Option<String>,
        hostname: Option<String>,
        max_commands: Option<u64>,
        max_loop_iterations: Option<u64>,
        python: bool,
        external_functions: Option<Vec<String>>,
        external_handler: Option<Py<PyAny>>,
    ) -> PyResult<Self> {
        let mut builder = Bash::builder();

        if let Some(ref u) = username {
            builder = builder.username(u);
        }
        if let Some(ref h) = hostname {
            builder = builder.hostname(h);
        }

        let mut limits = ExecutionLimits::new();
        if let Some(mc) = max_commands {
            limits = limits.max_commands(usize::try_from(mc).unwrap_or(usize::MAX));
        }
        if let Some(mli) = max_loop_iterations {
            limits = limits.max_loop_iterations(usize::try_from(mli).unwrap_or(usize::MAX));
        }
        builder = builder.limits(limits);

        let fn_names = external_functions.clone().unwrap_or_default();
        if !fn_names.is_empty() && external_handler.is_none() {
            return Err(PyValueError::new_err(
                "external_functions requires external_handler — the list has no effect without a handler",
            ));
        }
        if external_handler.is_some() && !python {
            return Err(PyValueError::new_err(
                "external_handler requires python=True",
            ));
        }
        if external_handler
            .as_ref()
            .is_some_and(|h| !h.bind(py).is_callable())
        {
            return Err(PyValueError::new_err("external_handler must be callable"));
        }
        if let Some(ref handler) = external_handler {
            // Check both the object itself and its __call__ method to support
            // objects with `async def __call__` (matching the ExternalHandler Protocol),
            // decorated coroutines, and similar async callables that return False
            // from iscoroutinefunction(obj) but True for iscoroutinefunction(obj.__call__).
            let inspect = py.import("inspect")?;
            let is_coro_fn = inspect.getattr("iscoroutinefunction")?;
            let bound = handler.bind(py);
            let is_coro = is_coro_fn.call1((bound,))?.extract::<bool>()?
                || bound
                    .getattr("__call__")
                    .ok()
                    .and_then(|c| is_coro_fn.call1((c,)).ok())
                    .and_then(|r| r.extract::<bool>().ok())
                    .unwrap_or(false);
            if !is_coro {
                return Err(PyValueError::new_err(
                    "external_handler must be an async callable (coroutine function)",
                ));
            }
        }
        let handler_for_build = external_handler.as_ref().map(|h| h.clone_ref(py));
        builder = apply_python_config(builder, python, fn_names, handler_for_build);

        let bash = builder.build();
        let cancelled = bash.cancellation_token();

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| PyRuntimeError::new_err(format!("Failed to create runtime: {e}")))?;

        Ok(Self {
            inner: Arc::new(Mutex::new(bash)),
            rt,
            cancelled,
            username,
            hostname,
            python,
            external_functions: external_functions.unwrap_or_default(),
            external_handler,
            max_commands,
            max_loop_iterations,
        })
    }

    /// Cancel the currently running execution.
    ///
    /// Safe to call from any thread. Execution will abort at the next
    /// command boundary.
    fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }

    /// Execute commands asynchronously.
    fn execute<'py>(&self, py: Python<'py>, commands: String) -> PyResult<Bound<'py, PyAny>> {
        let inner = self.inner.clone();
        future_into_py(py, async move {
            let mut bash = inner.lock().await;
            match bash.exec(&commands).await {
                Ok(result) => Ok(ExecResult {
                    stdout: result.stdout,
                    stderr: result.stderr,
                    exit_code: result.exit_code,
                    error: None,
                    stdout_truncated: result.stdout_truncated,
                    stderr_truncated: result.stderr_truncated,
                    final_env: result.final_env,
                }),
                Err(e) => {
                    let msg = e.to_string();
                    Ok(ExecResult {
                        stdout: String::new(),
                        stderr: msg.clone(),
                        exit_code: 1,
                        error: Some(msg),
                        stdout_truncated: false,
                        stderr_truncated: false,
                        final_env: None,
                    })
                }
            }
        })
    }

    /// Execute commands synchronously (blocking).
    ///
    /// Not supported when `external_handler` is configured: the handler is an async
    /// Python coroutine that requires a running event loop, which is unavailable in
    /// sync context. Use `execute()` (async) instead.
    ///
    /// Releases GIL before blocking on tokio to prevent deadlock with callbacks.
    fn execute_sync(&self, py: Python<'_>, commands: String) -> PyResult<ExecResult> {
        if self.external_handler.is_some() {
            return Err(PyRuntimeError::new_err(
                "execute_sync is not supported when external_handler is configured — use execute() (async) instead, e.g. asyncio.run(bash.execute(...))",
            ));
        }
        let inner = self.inner.clone();

        py.detach(|| {
            self.rt.block_on(async move {
                let mut bash = inner.lock().await;
                match bash.exec(&commands).await {
                    Ok(result) => Ok(ExecResult {
                        stdout: result.stdout,
                        stderr: result.stderr,
                        exit_code: result.exit_code,
                        error: None,
                        stdout_truncated: result.stdout_truncated,
                        stderr_truncated: result.stderr_truncated,
                        final_env: result.final_env,
                    }),
                    Err(e) => {
                        let msg = e.to_string();
                        Ok(ExecResult {
                            stdout: String::new(),
                            stderr: msg.clone(),
                            exit_code: 1,
                            error: Some(msg),
                            stdout_truncated: false,
                            stderr_truncated: false,
                            final_env: None,
                        })
                    }
                }
            })
        })
    }

    /// Reset interpreter to fresh state, preserving all configuration including
    /// python mode and external function handler.
    /// Releases GIL before blocking on tokio to prevent deadlock.
    fn reset(&self, py: Python<'_>) -> PyResult<()> {
        let inner = self.inner.clone();
        // THREAT[TM-PY-026]: Rebuild with same config to preserve DoS protections.
        let username = self.username.clone();
        let hostname = self.hostname.clone();
        let max_commands = self.max_commands;
        let max_loop_iterations = self.max_loop_iterations;
        let python = self.python;
        let external_functions = self.external_functions.clone();
        // Clone handler ref while still holding the GIL (before py.detach).
        let handler_clone = self.external_handler.as_ref().map(|h| h.clone_ref(py));
        let cancelled = self.cancelled.clone();

        py.detach(|| {
            self.rt.block_on(async move {
                let mut bash = inner.lock().await;
                let mut builder = Bash::builder();
                if let Some(ref u) = username {
                    builder = builder.username(u);
                }
                if let Some(ref h) = hostname {
                    builder = builder.hostname(h);
                }
                let mut limits = ExecutionLimits::new();
                if let Some(mc) = max_commands {
                    limits = limits.max_commands(usize::try_from(mc).unwrap_or(usize::MAX));
                }
                if let Some(mli) = max_loop_iterations {
                    limits = limits.max_loop_iterations(usize::try_from(mli).unwrap_or(usize::MAX));
                }
                builder = builder.limits(limits);
                builder = apply_python_config(builder, python, external_functions, handler_clone);
                *bash = builder.build();
                // Reset the shared cancellation flag so cancel() doesn't immediately
                // abort executions that started after reset.
                cancelled.store(false, std::sync::atomic::Ordering::Relaxed);
                Ok(())
            })
        })
    }

    fn __repr__(&self) -> String {
        format!(
            "Bash(username={:?}, hostname={:?})",
            self.username.as_deref().unwrap_or("user"),
            self.hostname.as_deref().unwrap_or("sandbox")
        )
    }
}

// ============================================================================
// BashTool — interpreter + tool-contract metadata
// ============================================================================

/// Bash interpreter with tool-contract metadata (`description`, `help`,
/// `system_prompt`, schemas).
///
/// Extends `Bash` with methods required by LLM tool-use protocols.
/// Use this when integrating with LangChain, PydanticAI, or similar frameworks.
///
/// Example:
///     ```python
///     from bashkit import BashTool
///
///     tool = BashTool()
///     print(tool.input_schema())  # JSON schema for LLM
///     result = await tool.execute("echo 'Hello!'")
///     ```
/// with a virtual filesystem. State persists between calls - files created
/// in one call are available in subsequent calls.
///
/// Example:
///     ```python
///     from bashkit import BashTool
///
///     tool = BashTool()
///     result = await tool.execute("echo 'Hello, World!'")
///     print(result.stdout)  # Hello, World!
///     ```
#[pyclass]
#[allow(dead_code)]
pub struct BashTool {
    inner: Arc<Mutex<Bash>>,
    /// Shared tokio runtime — reused across all sync calls to avoid
    /// per-call OS thread/fd exhaustion (issue #414).
    rt: tokio::runtime::Runtime,
    cancelled: Arc<AtomicBool>,
    username: Option<String>,
    hostname: Option<String>,
    max_commands: Option<u64>,
    max_loop_iterations: Option<u64>,
}

impl BashTool {
    fn build_rust_tool(&self) -> RustBashTool {
        let mut builder = RustBashTool::builder();

        if let Some(ref username) = self.username {
            builder = builder.username(username);
        }
        if let Some(ref hostname) = self.hostname {
            builder = builder.hostname(hostname);
        }

        let mut limits = ExecutionLimits::new();
        if let Some(mc) = self.max_commands {
            limits = limits.max_commands(usize::try_from(mc).unwrap_or(usize::MAX));
        }
        if let Some(mli) = self.max_loop_iterations {
            limits = limits.max_loop_iterations(usize::try_from(mli).unwrap_or(usize::MAX));
        }

        builder.limits(limits).build()
    }
}

#[pymethods]
impl BashTool {
    #[new]
    #[pyo3(signature = (username=None, hostname=None, max_commands=None, max_loop_iterations=None))]
    fn new(
        username: Option<String>,
        hostname: Option<String>,
        max_commands: Option<u64>,
        max_loop_iterations: Option<u64>,
    ) -> PyResult<Self> {
        let mut builder = Bash::builder();

        if let Some(ref u) = username {
            builder = builder.username(u);
        }
        if let Some(ref h) = hostname {
            builder = builder.hostname(h);
        }

        let mut limits = ExecutionLimits::new();
        if let Some(mc) = max_commands {
            limits = limits.max_commands(usize::try_from(mc).unwrap_or(usize::MAX));
        }
        if let Some(mli) = max_loop_iterations {
            limits = limits.max_loop_iterations(usize::try_from(mli).unwrap_or(usize::MAX));
        }
        builder = builder.limits(limits);

        let bash = builder.build();
        let cancelled = bash.cancellation_token();

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| PyRuntimeError::new_err(format!("Failed to create runtime: {e}")))?;

        Ok(Self {
            inner: Arc::new(Mutex::new(bash)),
            rt,
            cancelled,
            username,
            hostname,
            max_commands,
            max_loop_iterations,
        })
    }

    /// Cancel the currently running execution.
    fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }

    fn execute<'py>(&self, py: Python<'py>, commands: String) -> PyResult<Bound<'py, PyAny>> {
        let inner = self.inner.clone();
        future_into_py(py, async move {
            let mut bash = inner.lock().await;
            match bash.exec(&commands).await {
                Ok(result) => Ok(ExecResult {
                    stdout: result.stdout,
                    stderr: result.stderr,
                    exit_code: result.exit_code,
                    error: None,
                    stdout_truncated: result.stdout_truncated,
                    stderr_truncated: result.stderr_truncated,
                    final_env: result.final_env,
                }),
                Err(e) => {
                    let msg = e.to_string();
                    Ok(ExecResult {
                        stdout: String::new(),
                        stderr: msg.clone(),
                        exit_code: 1,
                        error: Some(msg),
                        stdout_truncated: false,
                        stderr_truncated: false,
                        final_env: None,
                    })
                }
            }
        })
    }

    /// Releases GIL before blocking on tokio to prevent deadlock with callbacks.
    fn execute_sync(&self, py: Python<'_>, commands: String) -> PyResult<ExecResult> {
        let inner = self.inner.clone();

        py.detach(|| {
            self.rt.block_on(async move {
                let mut bash = inner.lock().await;
                match bash.exec(&commands).await {
                    Ok(result) => Ok(ExecResult {
                        stdout: result.stdout,
                        stderr: result.stderr,
                        exit_code: result.exit_code,
                        error: None,
                        stdout_truncated: result.stdout_truncated,
                        stderr_truncated: result.stderr_truncated,
                        final_env: result.final_env,
                    }),
                    Err(e) => {
                        let msg = e.to_string();
                        Ok(ExecResult {
                            stdout: String::new(),
                            stderr: msg.clone(),
                            exit_code: 1,
                            error: Some(msg),
                            stdout_truncated: false,
                            stderr_truncated: false,
                            final_env: None,
                        })
                    }
                }
            })
        })
    }

    /// Releases GIL before blocking on tokio to prevent deadlock.
    /// THREAT[TM-PY-028]: Rebuild with same config to preserve security limits.
    fn reset(&self, py: Python<'_>) -> PyResult<()> {
        let inner = self.inner.clone();
        let username = self.username.clone();
        let hostname = self.hostname.clone();
        let max_commands = self.max_commands;
        let max_loop_iterations = self.max_loop_iterations;

        py.detach(|| {
            self.rt.block_on(async move {
                let mut bash = inner.lock().await;
                let mut builder = Bash::builder();
                if let Some(ref u) = username {
                    builder = builder.username(u);
                }
                if let Some(ref h) = hostname {
                    builder = builder.hostname(h);
                }
                let mut limits = ExecutionLimits::new();
                if let Some(mc) = max_commands {
                    limits = limits.max_commands(usize::try_from(mc).unwrap_or(usize::MAX));
                }
                if let Some(mli) = max_loop_iterations {
                    limits = limits.max_loop_iterations(usize::try_from(mli).unwrap_or(usize::MAX));
                }
                builder = builder.limits(limits);
                *bash = builder.build();
                Ok(())
            })
        })
    }

    #[getter]
    fn name(&self) -> &str {
        "bashkit"
    }

    #[getter]
    fn short_description(&self) -> &str {
        "Run bash commands in an isolated virtual filesystem"
    }

    fn description(&self) -> PyResult<String> {
        Ok(self.build_rust_tool().description().to_string())
    }

    fn help(&self) -> PyResult<String> {
        Ok(self.build_rust_tool().help())
    }

    fn system_prompt(&self) -> PyResult<String> {
        Ok(self.build_rust_tool().system_prompt())
    }

    fn input_schema(&self) -> PyResult<String> {
        let schema = self.build_rust_tool().input_schema();
        serde_json::to_string_pretty(&schema)
            .map_err(|e| PyValueError::new_err(format!("Schema serialization failed: {}", e)))
    }

    fn output_schema(&self) -> PyResult<String> {
        let schema = self.build_rust_tool().output_schema();
        serde_json::to_string_pretty(&schema)
            .map_err(|e| PyValueError::new_err(format!("Schema serialization failed: {}", e)))
    }

    #[getter]
    fn version(&self) -> &str {
        VERSION
    }

    fn __repr__(&self) -> String {
        format!(
            "BashTool(username={:?}, hostname={:?})",
            self.username.as_deref().unwrap_or("user"),
            self.hostname.as_deref().unwrap_or("sandbox")
        )
    }
}

// ============================================================================
// ScriptedTool — multi-tool orchestration via bash scripts
// ============================================================================

/// Entry for a registered Python tool callback
struct PyToolEntry {
    name: String,
    description: String,
    schema: serde_json::Value,
    callback: Py<PyAny>,
}

/// Compose Python callbacks as bash builtins for multi-tool orchestration.
///
/// Each registered tool becomes a bash builtin command. An LLM (or user) writes
/// a single bash script that pipes, loops, and branches across all tools.
///
/// Python callbacks receive `(params: dict, stdin: str | None)` and return a
/// string. Raise an exception to signal failure.
///
/// Example:
///     ```python
///     from bashkit import ScriptedTool
///
///     def get_user(params, stdin=None):
///         return '{"id": 1, "name": "Alice"}'
///
///     tool = ScriptedTool("api")
///     tool.add_tool("get_user", "Fetch user by ID",
///         callback=get_user,
///         schema={"type": "object", "properties": {"id": {"type": "integer"}}})
///
///     result = tool.execute_sync("get_user --id 1 | jq -r '.name'")
///     print(result.stdout)  # Alice
///     ```
#[pyclass]
pub struct ScriptedTool {
    name: String,
    short_desc: Option<String>,
    tools: Vec<PyToolEntry>,
    env_vars: Vec<(String, String)>,
    /// Shared tokio runtime — reused across all sync calls to avoid
    /// per-call OS thread/fd exhaustion (issue #414).
    rt: tokio::runtime::Runtime,
    max_commands: Option<u64>,
    max_loop_iterations: Option<u64>,
}

impl ScriptedTool {
    /// Build a Rust ScriptedTool from stored Python config.
    /// Each Python callback is wrapped via `Python::attach`.
    fn build_rust_tool(&self) -> RustScriptedTool {
        let mut builder = RustScriptedTool::builder(&self.name);

        if let Some(ref desc) = self.short_desc {
            builder = builder.short_description(desc);
        }

        for entry in &self.tools {
            let py_cb = Python::attach(|py| entry.callback.clone_ref(py));
            let tool_name = entry.name.clone();

            let callback = move |args: &ToolArgs| -> Result<String, String> {
                Python::attach(|py| {
                    let params = json_to_py(py, &args.params).map_err(|e: PyErr| e.to_string())?;
                    let stdin_arg = args.stdin.as_deref().map(|s| s.to_string());

                    let result = py_cb
                        .call1(py, (params, stdin_arg))
                        .map_err(|e| format!("{}: {}", tool_name, e))?;
                    result
                        .extract::<String>(py)
                        .map_err(|e| format!("{}: callback must return str, got {}", tool_name, e))
                })
            };

            builder = builder.tool(
                ToolDef::new(&entry.name, &entry.description).with_schema(entry.schema.clone()),
                callback,
            );
        }

        for (k, v) in &self.env_vars {
            builder = builder.env(k, v);
        }

        if self.max_commands.is_some() || self.max_loop_iterations.is_some() {
            let mut limits = ExecutionLimits::new();
            if let Some(mc) = self.max_commands {
                limits = limits.max_commands(usize::try_from(mc).unwrap_or(usize::MAX));
            }
            if let Some(mli) = self.max_loop_iterations {
                limits = limits.max_loop_iterations(usize::try_from(mli).unwrap_or(usize::MAX));
            }
            builder = builder.limits(limits);
        }

        builder.build()
    }
}

#[pymethods]
impl ScriptedTool {
    /// Create a new ScriptedTool.
    ///
    /// Args:
    ///     name: Tool name (used in system prompt and docs)
    ///     short_description: One-line description
    ///     max_commands: Max commands per execute call
    ///     max_loop_iterations: Max loop iterations per execute call
    #[new]
    #[pyo3(signature = (name, short_description=None, max_commands=None, max_loop_iterations=None))]
    fn new(
        name: String,
        short_description: Option<String>,
        max_commands: Option<u64>,
        max_loop_iterations: Option<u64>,
    ) -> PyResult<Self> {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| PyRuntimeError::new_err(format!("Failed to create runtime: {e}")))?;

        Ok(Self {
            name,
            short_desc: short_description,
            tools: Vec::new(),
            env_vars: Vec::new(),
            rt,
            max_commands,
            max_loop_iterations,
        })
    }

    /// Register a tool command.
    ///
    /// The callback signature is: `callback(params: dict, stdin: str | None) -> str`
    ///
    /// `params` contains `--key value` flags parsed from the bash command line,
    /// with types coerced per the schema (integers, booleans, etc.).
    ///
    /// Args:
    ///     name: Command name (becomes a bash builtin)
    ///     description: Human-readable description
    ///     callback: Python callable `(params, stdin) -> str`
    ///     schema: Optional JSON Schema dict for input parameters
    #[pyo3(signature = (name, description, callback, schema=None))]
    fn add_tool(
        &mut self,
        py: Python<'_>,
        name: String,
        description: String,
        callback: Py<PyAny>,
        schema: Option<Bound<'_, pyo3::PyAny>>,
    ) -> PyResult<()> {
        let schema_val = match schema {
            Some(ref s) => py_to_json(py, s)?,
            None => serde_json::Value::Object(Default::default()),
        };
        self.tools.push(PyToolEntry {
            name,
            description,
            schema: schema_val,
            callback,
        });
        Ok(())
    }

    /// Add an environment variable visible inside scripts.
    fn env(&mut self, key: String, value: String) {
        self.env_vars.push((key, value));
    }

    /// Execute a bash script asynchronously.
    fn execute<'py>(&self, py: Python<'py>, commands: String) -> PyResult<Bound<'py, PyAny>> {
        let tool = self.build_rust_tool();
        future_into_py(py, async move {
            let resp = tool
                .execute(ToolRequest {
                    commands,
                    timeout_ms: None,
                })
                .await;
            Ok(ExecResult {
                stdout: resp.stdout,
                stderr: resp.stderr,
                exit_code: resp.exit_code,
                error: resp.error,
                stdout_truncated: resp.stdout_truncated,
                stderr_truncated: resp.stderr_truncated,
                final_env: resp.final_env,
            })
        })
    }

    /// Execute a bash script synchronously (blocking).
    /// Releases GIL before blocking on tokio to prevent deadlock with callbacks.
    fn execute_sync(&self, py: Python<'_>, commands: String) -> PyResult<ExecResult> {
        let tool = self.build_rust_tool();

        let resp = py.detach(|| {
            self.rt.block_on(async move {
                tool.execute(ToolRequest {
                    commands,
                    timeout_ms: None,
                })
                .await
            })
        });
        Ok(ExecResult {
            stdout: resp.stdout,
            stderr: resp.stderr,
            exit_code: resp.exit_code,
            error: resp.error,
            stdout_truncated: resp.stdout_truncated,
            stderr_truncated: resp.stderr_truncated,
            final_env: resp.final_env,
        })
    }

    /// Get the tool name.
    #[getter(name)]
    fn name_prop(&self) -> &str {
        &self.name
    }

    /// Get the short description.
    #[getter]
    fn short_description(&self) -> String {
        self.short_desc
            .clone()
            .unwrap_or_else(|| format!("ScriptedTool: {}", self.name))
    }

    /// Number of registered tools.
    fn tool_count(&self) -> usize {
        self.tools.len()
    }

    /// Get the token-efficient description.
    fn description(&self) -> String {
        self.build_rust_tool().description().to_string()
    }

    /// Get help as a Markdown document.
    fn help(&self) -> String {
        self.build_rust_tool().help()
    }

    /// Get compact system-prompt text for orchestration.
    fn system_prompt(&self) -> String {
        self.build_rust_tool().system_prompt()
    }

    /// Get JSON input schema.
    fn input_schema(&self) -> PyResult<String> {
        let tool = self.build_rust_tool();
        let schema = tool.input_schema();
        serde_json::to_string_pretty(&schema)
            .map_err(|e| PyValueError::new_err(format!("Schema serialization failed: {}", e)))
    }

    /// Get JSON output schema.
    fn output_schema(&self) -> PyResult<String> {
        let tool = self.build_rust_tool();
        let schema = tool.output_schema();
        serde_json::to_string_pretty(&schema)
            .map_err(|e| PyValueError::new_err(format!("Schema serialization failed: {}", e)))
    }

    /// Get tool version.
    #[getter]
    fn version(&self) -> &str {
        VERSION
    }

    fn __repr__(&self) -> String {
        format!(
            "ScriptedTool(name={:?}, tools={})",
            self.name,
            self.tools.len()
        )
    }
}

// ============================================================================
// Module-level functions
// ============================================================================

/// Create a LangChain-compatible tool spec from BashTool.
#[pyfunction]
fn create_langchain_tool_spec() -> PyResult<pyo3::Py<PyDict>> {
    let tool = RustBashTool::default();

    Python::attach(|py| {
        let dict = PyDict::new(py);
        dict.set_item("name", tool.name())?;
        dict.set_item("description", tool.description())?;

        let schema = tool.input_schema();
        let schema_str = serde_json::to_string(&schema)
            .map_err(|e| PyValueError::new_err(format!("Schema error: {}", e)))?;
        dict.set_item("args_schema", schema_str)?;

        Ok(dict.into())
    })
}

// ============================================================================
// Python module
// ============================================================================

#[pymodule]
fn _bashkit(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyBash>()?;
    m.add_class::<BashTool>()?;
    m.add_class::<ScriptedTool>()?;
    m.add_class::<ExecResult>()?;
    m.add_function(wrap_pyfunction!(create_langchain_tool_spec, m)?)?;
    Ok(())
}

// ============================================================================
// MontyObject <-> Python conversion helpers
// ============================================================================

fn monty_to_py(py: Python<'_>, obj: &MontyObject) -> PyResult<Py<PyAny>> {
    match obj {
        MontyObject::None => Ok(py.None()),
        MontyObject::Bool(b) => Ok(b.into_pyobject(py)?.to_owned().into_any().unbind()),
        MontyObject::Int(i) => Ok(i.into_pyobject(py)?.into_any().unbind()),
        // BigInt: convert to Python int via its decimal string representation.
        MontyObject::BigInt(b) => {
            let int_str = b.to_string();
            let py_int = py.import("builtins")?.getattr("int")?.call1((int_str,))?;
            Ok(py_int.into_any().unbind())
        }
        MontyObject::Float(f) => Ok(f.into_pyobject(py)?.into_any().unbind()),
        MontyObject::String(s) => Ok(s.into_pyobject(py)?.into_any().unbind()),
        // Known limitation: Path becomes a plain Python str, not pathlib.Path.
        MontyObject::Path(s) => Ok(s.into_pyobject(py)?.into_any().unbind()),
        MontyObject::Bytes(b) => Ok(b.as_slice().into_pyobject(py)?.into_any().unbind()),
        MontyObject::Tuple(items) => {
            let py_items = items
                .iter()
                .map(|v| monty_to_py(py, v))
                .collect::<PyResult<Vec<_>>>()?;
            Ok(PyTuple::new(py, &py_items)?.into_any().unbind())
        }
        MontyObject::List(items) => {
            let py_items = items
                .iter()
                .map(|v| monty_to_py(py, v))
                .collect::<PyResult<Vec<_>>>()?;
            Ok(PyList::new(py, &py_items)?.into_any().unbind())
        }
        MontyObject::Set(items) => {
            let py_items = items
                .iter()
                .map(|v| monty_to_py(py, v))
                .collect::<PyResult<Vec<_>>>()?;
            Ok(PySet::new(py, &py_items)?.into_any().unbind())
        }
        MontyObject::FrozenSet(items) => {
            let py_items = items
                .iter()
                .map(|v| monty_to_py(py, v))
                .collect::<PyResult<Vec<_>>>()?;
            Ok(PyFrozenSet::new(py, &py_items)?.into_any().unbind())
        }
        // NamedTuple: convert to dict mapping field names to values, preserving field names.
        MontyObject::NamedTuple {
            field_names,
            values,
            ..
        } => {
            let dict = PyDict::new(py);
            for (name, value) in field_names.iter().zip(values.iter()) {
                dict.set_item(name, monty_to_py(py, value)?)?;
            }
            Ok(dict.into_any().unbind())
        }
        MontyObject::Dict(dict_pairs) => {
            let dict = PyDict::new(py);
            // DictPairs only implements IntoIterator (consuming), so clone is required
            // to iterate without moving out of the match guard.
            for (k, v) in dict_pairs.clone() {
                dict.set_item(monty_to_py(py, &k)?, monty_to_py(py, &v)?)?;
            }
            Ok(dict.into_any().unbind())
        }
        // All other variants (Exception, Type, Function, etc.) — repr as string.
        other => Ok(other.py_repr().into_pyobject(py)?.into_any().unbind()),
    }
}

// `py` is used directly in `is_instance_of`, `import`, and `cast` calls — not only
// forwarded in recursive calls — so clippy's "only used in recursion" is a false positive.
#[allow(clippy::only_used_in_recursion)]
fn py_to_monty(py: Python<'_>, obj: &Bound<'_, PyAny>) -> PyResult<MontyObject> {
    if obj.is_none() {
        return Ok(MontyObject::None);
    }
    // bool must come before int — bool is a subtype of int in Python
    if let Ok(b) = obj.extract::<bool>() {
        return Ok(MontyObject::Bool(b));
    }
    if let Ok(i) = obj.extract::<i64>() {
        return Ok(MontyObject::Int(i));
    }
    // Large Python int that overflows i64: convert via decimal string → BigInt.
    if obj.is_instance_of::<PyInt>() {
        let s = obj.str()?.extract::<String>()?;
        let b = s.parse::<num_bigint::BigInt>().map_err(|e| {
            PyValueError::new_err(format!("failed to parse Python int as BigInt: {e}"))
        })?;
        return Ok(MontyObject::BigInt(b));
    }
    // Guard f64 with an isinstance check so large Python ints (which widen to f64)
    // are not incorrectly classified as floats.
    if obj.is_instance_of::<PyFloat>()
        && let Ok(f) = obj.extract::<f64>()
    {
        return Ok(MontyObject::Float(f));
    }
    if let Ok(s) = obj.extract::<String>() {
        return Ok(MontyObject::String(s));
    }
    // Guard bytes with isinstance to avoid ambiguity with str-like objects.
    if obj.is_instance_of::<PyBytes>()
        && let Ok(b) = obj.extract::<Vec<u8>>()
    {
        return Ok(MontyObject::Bytes(b));
    }
    if let Ok(tuple) = obj.cast::<PyTuple>() {
        let items = tuple
            .iter()
            .map(|v| py_to_monty(py, &v))
            .collect::<PyResult<Vec<_>>>()?;
        return Ok(MontyObject::Tuple(items));
    }
    if let Ok(list) = obj.cast::<PyList>() {
        let items = list
            .iter()
            .map(|v| py_to_monty(py, &v))
            .collect::<PyResult<Vec<_>>>()?;
        return Ok(MontyObject::List(items));
    }
    if let Ok(dict) = obj.cast::<PyDict>() {
        let pairs: Vec<(MontyObject, MontyObject)> = dict
            .iter()
            .map(|(k, v)| Ok((py_to_monty(py, &k)?, py_to_monty(py, &v)?)))
            .collect::<PyResult<Vec<_>>>()?;
        return Ok(MontyObject::dict(pairs));
    }
    if let Ok(set) = obj.cast::<PySet>() {
        let items = set
            .iter()
            .map(|v| py_to_monty(py, &v))
            .collect::<PyResult<Vec<_>>>()?;
        return Ok(MontyObject::Set(items));
    }
    if let Ok(fset) = obj.cast::<PyFrozenSet>() {
        let items = fset
            .iter()
            .map(|v| py_to_monty(py, &v))
            .collect::<PyResult<Vec<_>>>()?;
        return Ok(MontyObject::FrozenSet(items));
    }
    // Fallback: convert to string via __str__
    Ok(MontyObject::String(obj.str()?.extract::<String>()?))
}
