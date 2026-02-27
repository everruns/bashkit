//! Python bindings for Bashkit
//!
//! Exposes the Bash interpreter and ScriptedTool as Python classes for use in
//! AI agent frameworks. BashTool provides stateful execution (filesystem persists
//! between calls). ScriptedTool composes Python callbacks as bash builtins for
//! multi-tool orchestration in a single script.

use bashkit::tool::VERSION;
use bashkit::{
    Bash, BashTool as RustBashTool, ExecutionLimits, ScriptedTool as RustScriptedTool, Tool,
    ToolArgs, ToolDef, ToolRequest,
};
use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};
use pyo3_async_runtimes::tokio::future_into_py;
use std::sync::Arc;
use tokio::sync::Mutex;

// ============================================================================
// JSON <-> Python helpers
// ============================================================================

/// Convert serde_json::Value → Py<PyAny>
fn json_to_py(py: Python<'_>, val: &serde_json::Value) -> PyResult<Py<PyAny>> {
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
                .map(|v| json_to_py(py, v))
                .collect::<PyResult<_>>()?;
            Ok(PyList::new(py, &items)?.into_any().unbind())
        }
        serde_json::Value::Object(map) => {
            let dict = PyDict::new(py);
            for (k, v) in map {
                dict.set_item(k, json_to_py(py, v)?)?;
            }
            Ok(dict.into_any().unbind())
        }
    }
}

/// Convert Py<PyAny> → serde_json::Value (for schema dicts)
#[allow(clippy::only_used_in_recursion)]
fn py_to_json(py: Python<'_>, obj: &Bound<'_, pyo3::PyAny>) -> PyResult<serde_json::Value> {
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
            .map(|item| py_to_json(py, &item))
            .collect::<PyResult<_>>()?;
        return Ok(serde_json::Value::Array(arr));
    }
    if let Ok(dict) = obj.cast::<PyDict>() {
        let mut map = serde_json::Map::new();
        for (k, v) in dict.iter() {
            let key: String = k.extract()?;
            map.insert(key, py_to_json(py, &v)?);
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
}

#[pymethods]
impl ExecResult {
    fn __repr__(&self) -> String {
        format!(
            "ExecResult(stdout={:?}, stderr={:?}, exit_code={}, error={:?})",
            self.stdout, self.stderr, self.exit_code, self.error
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
            Ok(dict.into())
        })
    }
}

// ============================================================================
// BashTool — stateful interpreter
// ============================================================================

/// Virtual bash interpreter for AI agents
///
/// BashTool provides a safe execution environment for running bash commands
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
    username: Option<String>,
    hostname: Option<String>,
    max_commands: Option<u64>,
    max_loop_iterations: Option<u64>,
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
            limits = limits.max_commands(mc as usize);
        }
        if let Some(mli) = max_loop_iterations {
            limits = limits.max_loop_iterations(mli as usize);
        }
        builder = builder.limits(limits);

        let bash = builder.build();

        Ok(Self {
            inner: Arc::new(Mutex::new(bash)),
            username,
            hostname,
            max_commands,
            max_loop_iterations,
        })
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
                }),
                Err(e) => Ok(ExecResult {
                    stdout: String::new(),
                    stderr: String::new(),
                    exit_code: 1,
                    error: Some(e.to_string()),
                }),
            }
        })
    }

    fn execute_sync(&self, commands: String) -> PyResult<ExecResult> {
        let inner = self.inner.clone();
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| PyRuntimeError::new_err(format!("Failed to create runtime: {}", e)))?;

        rt.block_on(async move {
            let mut bash = inner.lock().await;
            match bash.exec(&commands).await {
                Ok(result) => Ok(ExecResult {
                    stdout: result.stdout,
                    stderr: result.stderr,
                    exit_code: result.exit_code,
                    error: None,
                }),
                Err(e) => Ok(ExecResult {
                    stdout: String::new(),
                    stderr: String::new(),
                    exit_code: 1,
                    error: Some(e.to_string()),
                }),
            }
        })
    }

    fn reset(&self) -> PyResult<()> {
        let inner = self.inner.clone();
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| PyRuntimeError::new_err(format!("Failed to create runtime: {}", e)))?;

        rt.block_on(async move {
            let mut bash = inner.lock().await;
            let builder = Bash::builder();
            *bash = builder.build();
            Ok(())
        })
    }

    #[getter]
    fn name(&self) -> &str {
        "bashkit"
    }

    #[getter]
    fn short_description(&self) -> &str {
        "Virtual bash interpreter with virtual filesystem"
    }

    fn description(&self) -> PyResult<String> {
        let tool = RustBashTool::default();
        Ok(tool.description())
    }

    fn help(&self) -> PyResult<String> {
        let tool = RustBashTool::default();
        Ok(tool.help())
    }

    fn system_prompt(&self) -> PyResult<String> {
        let tool = RustBashTool::default();
        Ok(tool.system_prompt())
    }

    fn input_schema(&self) -> PyResult<String> {
        let tool = RustBashTool::default();
        let schema = tool.input_schema();
        serde_json::to_string_pretty(&schema)
            .map_err(|e| PyValueError::new_err(format!("Schema serialization failed: {}", e)))
    }

    fn output_schema(&self) -> PyResult<String> {
        let tool = RustBashTool::default();
        let schema = tool.output_schema();
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
                limits = limits.max_commands(mc as usize);
            }
            if let Some(mli) = self.max_loop_iterations {
                limits = limits.max_loop_iterations(mli as usize);
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
    ) -> Self {
        Self {
            name,
            short_desc: short_description,
            tools: Vec::new(),
            env_vars: Vec::new(),
            max_commands,
            max_loop_iterations,
        }
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
        let mut tool = self.build_rust_tool();
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
            })
        })
    }

    /// Execute a bash script synchronously (blocking).
    fn execute_sync(&self, commands: String) -> PyResult<ExecResult> {
        let mut tool = self.build_rust_tool();
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| PyRuntimeError::new_err(format!("Failed to create runtime: {}", e)))?;

        let resp = rt.block_on(async move {
            tool.execute(ToolRequest {
                commands,
                timeout_ms: None,
            })
            .await
        });
        Ok(ExecResult {
            stdout: resp.stdout,
            stderr: resp.stderr,
            exit_code: resp.exit_code,
            error: resp.error,
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

    /// Get the full description.
    fn description(&self) -> String {
        self.build_rust_tool().description()
    }

    /// Get help text (man-page format).
    fn help(&self) -> String {
        self.build_rust_tool().help()
    }

    /// Get system prompt for LLMs (token-efficient).
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
    m.add_class::<BashTool>()?;
    m.add_class::<ScriptedTool>()?;
    m.add_class::<ExecResult>()?;
    m.add_function(wrap_pyfunction!(create_langchain_tool_spec, m)?)?;
    Ok(())
}
