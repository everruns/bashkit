//! Python bindings for Bashkit
//!
//! Exposes the Bash interpreter as a Python class for use in AI agent frameworks.
//! Uses stateful execution - filesystem and variables persist between calls.

use bashkit::{Bash, BashTool as RustBashTool, ExecutionLimits, Tool};
use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3_async_runtimes::tokio::future_into_py;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Result from executing bash commands
#[pyclass]
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
    fn to_dict(&self) -> pyo3::PyResult<pyo3::Py<pyo3::types::PyDict>> {
        Python::with_gil(|py| {
            let dict = pyo3::types::PyDict::new(py);
            dict.set_item("stdout", &self.stdout)?;
            dict.set_item("stderr", &self.stderr)?;
            dict.set_item("exit_code", self.exit_code)?;
            dict.set_item("error", &self.error)?;
            Ok(dict.into())
        })
    }
}

/// Virtual bash interpreter for AI agents
///
/// BashTool provides a safe execution environment for running bash commands
/// with a virtual filesystem. State persists between calls - files created
/// in one call are available in subsequent calls.
///
/// Example:
///     ```python
///     from bashkit_py import BashTool
///
///     tool = BashTool()
///     result = await tool.execute("echo 'Hello, World!'")
///     print(result.stdout)  # Hello, World!
///     ```
#[pyclass]
#[allow(dead_code)]
pub struct BashTool {
    /// Stateful bash interpreter - persists filesystem and variables
    inner: Arc<Mutex<Bash>>,
    username: Option<String>,
    hostname: Option<String>,
    max_commands: Option<u64>,
    max_loop_iterations: Option<u64>,
}

#[pymethods]
impl BashTool {
    /// Create a new BashTool instance
    ///
    /// Args:
    ///     username: Custom username for virtual environment (default: "user")
    ///     hostname: Custom hostname for virtual environment (default: "sandbox")
    ///     max_commands: Maximum commands to execute (default: 10000)
    ///     max_loop_iterations: Maximum loop iterations (default: 100000)
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

    /// Execute bash commands asynchronously
    ///
    /// State persists between calls - files, variables, and functions
    /// created in one call are available in subsequent calls.
    ///
    /// Args:
    ///     commands: Bash commands to execute (like `bash -c "commands"`)
    ///
    /// Returns:
    ///     ExecResult with stdout, stderr, exit_code
    ///
    /// Example:
    ///     ```python
    ///     result = await tool.execute("echo hello && echo world")
    ///     print(result.stdout)  # hello\nworld\n
    ///     ```
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

    /// Execute bash commands synchronously (blocking)
    ///
    /// Note: Prefer `execute()` for async contexts. This method blocks.
    ///
    /// Args:
    ///     commands: Bash commands to execute
    ///
    /// Returns:
    ///     ExecResult with stdout, stderr, exit_code
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

    /// Reset the interpreter state (clear filesystem, variables, functions)
    fn reset(&self) -> PyResult<()> {
        let inner = self.inner.clone();
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| PyRuntimeError::new_err(format!("Failed to create runtime: {}", e)))?;

        rt.block_on(async move {
            let mut bash = inner.lock().await;
            // Create fresh Bash with same settings
            let builder = Bash::builder();
            // Note: We lose settings on reset, could store them
            *bash = builder.build();
            Ok(())
        })
    }

    /// Get the tool name
    #[getter]
    fn name(&self) -> &str {
        "bashkit"
    }

    /// Get short description
    #[getter]
    fn short_description(&self) -> &str {
        "Virtual bash interpreter with virtual filesystem"
    }

    /// Get the full description
    fn description(&self) -> PyResult<String> {
        let tool = RustBashTool::default();
        Ok(tool.description())
    }

    /// Get LLM documentation
    fn help(&self) -> PyResult<String> {
        let tool = RustBashTool::default();
        Ok(tool.help())
    }

    /// Get system prompt for LLMs
    fn system_prompt(&self) -> PyResult<String> {
        let tool = RustBashTool::default();
        Ok(tool.system_prompt())
    }

    /// Get JSON schema for input validation
    fn input_schema(&self) -> PyResult<String> {
        let tool = RustBashTool::default();
        let schema = tool.input_schema();
        serde_json::to_string_pretty(&schema)
            .map_err(|e| PyValueError::new_err(format!("Schema serialization failed: {}", e)))
    }

    /// Get JSON schema for output
    fn output_schema(&self) -> PyResult<String> {
        let tool = RustBashTool::default();
        let schema = tool.output_schema();
        serde_json::to_string_pretty(&schema)
            .map_err(|e| PyValueError::new_err(format!("Schema serialization failed: {}", e)))
    }

    /// Get tool version
    #[getter]
    fn version(&self) -> &str {
        bashkit::tool::VERSION
    }

    fn __repr__(&self) -> String {
        format!(
            "BashTool(username={:?}, hostname={:?})",
            self.username.as_deref().unwrap_or("user"),
            self.hostname.as_deref().unwrap_or("sandbox")
        )
    }
}

/// Create a LangChain-compatible tool from BashTool
///
/// Returns a dict with:
///   - name: Tool name
///   - description: Tool description
///   - args_schema: JSON schema for arguments
///
/// Example:
///     ```python
///     from bashkit_py import create_langchain_tool_spec
///
///     spec = create_langchain_tool_spec()
///     # Use with langchain's StructuredTool.from_function()
///     ```
#[pyfunction]
fn create_langchain_tool_spec() -> PyResult<pyo3::Py<pyo3::types::PyDict>> {
    let tool = RustBashTool::default();

    Python::with_gil(|py| {
        let dict = pyo3::types::PyDict::new(py);
        dict.set_item("name", tool.name())?;
        dict.set_item("description", tool.description())?;

        let schema = tool.input_schema();
        let schema_str = serde_json::to_string(&schema)
            .map_err(|e| PyValueError::new_err(format!("Schema error: {}", e)))?;
        dict.set_item("args_schema", schema_str)?;

        Ok(dict.into())
    })
}

/// Python module definition
#[pymodule]
fn _bashkit(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<BashTool>()?;
    m.add_class::<ExecResult>()?;
    m.add_function(wrap_pyfunction!(create_langchain_tool_spec, m)?)?;
    Ok(())
}
