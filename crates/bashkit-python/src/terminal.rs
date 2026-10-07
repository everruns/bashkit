//! `bashkit.Terminal`: interactive in-process terminal (vi, less, a persistent
//! shell) for Python hosts and LLM agents.
//!
//! Decisions:
//! - One class wraps the core `TerminalTool`, so Python gets both the
//!   low-level loop (`send` / `run_until_idle` / `screen_text`) and the
//!   agent-shaped `call(input=..., wait_ms=...)` with Vim key notation.
//! - Sync API on a per-instance current-thread runtime with the GIL released,
//!   like `Bash.execute_sync`; works the same on the Pyodide wheel. Waiting is
//!   bounded by the caller's `timeout` so a long command never wedges Python.
//! - Constructor options are a small subset of `Bash(...)` (identity, cwd,
//!   env, size). Richer setup can come later without changing this shape.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

use bashkit::Bash;
use bashkit::terminal::{TerminalActivity, TerminalSize, TerminalStatus, TerminalTool};
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyDict, PyList};

use crate::{PyFileSystem, PyRuntime, json_to_py, make_runtime};

/// Interactive bash session on an in-memory terminal.
///
/// Type keys, run until the shell needs input, read the screen as text.
/// Full-screen programs (`vi`, `less`, `more`) work.
#[pyclass(name = "Terminal", module = "bashkit")]
pub(crate) struct PyTerminal {
    tool: Mutex<TerminalTool>,
    rt: PyRuntime,
}

impl PyTerminal {
    fn with_tool<R>(&self, f: impl FnOnce(&mut TerminalTool) -> R) -> R {
        let mut guard = self.tool.lock().unwrap_or_else(|p| p.into_inner());
        f(&mut guard)
    }
}

fn activity_dict<'py>(py: Python<'py>, activity: TerminalActivity) -> PyResult<Bound<'py, PyDict>> {
    let d = PyDict::new(py);
    match activity {
        TerminalActivity::Starting | TerminalActivity::Prompt => d.set_item("state", "prompt")?,
        TerminalActivity::ContinuationPrompt => d.set_item("state", "continuation")?,
        TerminalActivity::Running { command } => {
            d.set_item("state", "running")?;
            d.set_item("command", command)?;
        }
        TerminalActivity::Exited(code) => {
            d.set_item("state", "exited")?;
            d.set_item("exit_code", code)?;
        }
    }
    Ok(d)
}

#[pymethods]
impl PyTerminal {
    #[new]
    #[pyo3(signature = (rows=24, cols=80, username=None, hostname=None, cwd=None, env=None))]
    fn new(
        rows: u16,
        cols: u16,
        username: Option<String>,
        hostname: Option<String>,
        cwd: Option<String>,
        env: Option<HashMap<String, String>>,
    ) -> PyResult<Self> {
        let env = env.unwrap_or_default();
        // Each `session` passed to `call` gets a shell built the same way.
        let factory = move || {
            let mut builder = Bash::builder();
            if let Some(u) = &username {
                builder = builder.username(u.clone());
            }
            if let Some(h) = &hostname {
                builder = builder.hostname(h.clone());
            }
            if let Some(c) = &cwd {
                builder = builder.cwd(c.clone());
            }
            for (k, v) in &env {
                builder = builder.env(k, v);
            }
            builder
        };
        let rt = make_runtime()?;
        // The session future is created inside the runtime so nothing in it
        // can observe a missing tokio context.
        let tool = {
            let _enter = rt.enter();
            TerminalTool::with_sessions(TerminalSize::new(rows, cols), Box::new(factory))
        };
        Ok(Self {
            tool: Mutex::new(tool),
            rt,
        })
    }

    /// Queue raw input as if typed (`"\r"` is Enter, `"\x1b"` Escape).
    /// Returns how many bytes were accepted.
    fn send(&self, data: &Bound<'_, PyAny>) -> PyResult<usize> {
        let bytes: Vec<u8> = if let Ok(s) = data.extract::<String>() {
            s.into_bytes()
        } else if let Ok(b) = data.extract::<Vec<u8>>() {
            b
        } else {
            return Err(PyValueError::new_err("send() takes str or bytes"));
        };
        Ok(self.with_tool(|t| t.terminal().send(&bytes)))
    }

    /// Run until the session needs input or exits. Returns `"idle"`,
    /// `"exited"`, or `"timeout"` if `timeout` seconds passed first (the
    /// command keeps its state; call again or send Ctrl-C).
    #[pyo3(signature = (timeout=None))]
    fn run_until_idle(&self, py: Python<'_>, timeout: Option<f64>) -> PyResult<&'static str> {
        if let Some(t) = timeout
            && !(t.is_finite() && t >= 0.0)
        {
            return Err(PyValueError::new_err(
                "timeout must be a non-negative number",
            ));
        }
        py.detach(|| {
            self.with_tool(|tool| {
                self.rt.block_on(async {
                    let term = tool.terminal_mut();
                    let status = match timeout {
                        Some(t) => {
                            match tokio::time::timeout(
                                Duration::from_secs_f64(t),
                                term.run_until_idle(),
                            )
                            .await
                            {
                                Ok(s) => s,
                                Err(_) => return Ok("timeout"),
                            }
                        }
                        None => term.run_until_idle().await,
                    };
                    Ok(match status {
                        TerminalStatus::Idle => "idle",
                        TerminalStatus::Exited(_) => "exited",
                    })
                })
            })
        })
    }

    /// Agent-style step: type `input` (Vim key notation such as
    /// `"ihello<Esc>:wq<Enter>"`), wait up to `wait_ms`, and return a dict
    /// with `screen`, `activity`, `commands` and more. `wait_for` (regex)
    /// returns early once command output matches; `screen` is `"full"`,
    /// `"changes"` or `"none"`; `session` names a terminal tab (created on
    /// first use, sharing files) and `close=True` closes it. Argument names
    /// match `tool_definition()`, so `call(**tool_args)` forwards a model's
    /// call.
    #[pyo3(signature = (input="", wait_ms=None, wait_for=None, screen=None, session=None, close=None))]
    #[allow(clippy::too_many_arguments)]
    fn call(
        &self,
        py: Python<'_>,
        input: &str,
        wait_ms: Option<u64>,
        wait_for: Option<&str>,
        screen: Option<&str>,
        session: Option<&str>,
        close: Option<bool>,
    ) -> PyResult<Py<PyAny>> {
        let mut args = serde_json::json!({ "input": input });
        if let Some(ms) = wait_ms {
            args["wait_ms"] = ms.into();
        }
        if let Some(p) = wait_for {
            args["wait_for"] = p.into();
        }
        if let Some(m) = screen {
            args["screen"] = m.into();
        }
        if let Some(s) = session {
            args["session"] = s.into();
        }
        if let Some(c) = close {
            args["close"] = c.into();
        }
        let out = py.detach(|| self.with_tool(|tool| self.rt.block_on(tool.call(args))));
        let value = out.map_err(|e| PyValueError::new_err(e.to_string()))?;
        json_to_py(py, &value)
    }

    /// Visible screen as plain text.
    fn screen_text(&self) -> String {
        self.with_tool(|t| t.terminal().screen_text())
    }

    /// Screen plus up to 1000 lines of scrollback.
    fn history_text(&self) -> String {
        self.with_tool(|t| t.terminal().history_text())
    }

    /// Raw output bytes (with escape sequences) since the last call.
    fn take_output<'py>(&self, py: Python<'py>) -> Bound<'py, PyBytes> {
        let out = self.with_tool(|t| t.terminal().take_output());
        PyBytes::new(py, &out)
    }

    /// Commands finished since the last call: list of dicts with `command`,
    /// `output`, `exit_code`, `output_truncated`.
    fn take_transcript<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyList>> {
        let records = self.with_tool(|t| t.terminal().take_transcript());
        let list = PyList::empty(py);
        for r in records {
            let d = PyDict::new(py);
            d.set_item("command", r.command)?;
            d.set_item("output", r.output)?;
            d.set_item("exit_code", r.exit_code)?;
            d.set_item("output_truncated", r.output_truncated)?;
            list.append(d)?;
        }
        Ok(list)
    }

    /// `{"state": "prompt" | "continuation" | "running" | "exited", ...}`
    /// with `command` while running and `exit_code` once exited.
    fn activity<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let activity = self.with_tool(|t| t.terminal().activity());
        activity_dict(py, activity)
    }

    /// Cursor position as `(row, col)`, zero-based.
    fn cursor(&self) -> (u16, u16) {
        self.with_tool(|t| t.terminal().cursor())
    }

    /// True while a full-screen program such as `vi` is open.
    fn is_alternate_screen(&self) -> bool {
        self.with_tool(|t| t.terminal().is_alternate_screen())
    }

    /// While a command (`read`, `select`) waits for a typed line, the
    /// question on the cursor line; otherwise `None`.
    fn input_prompt(&self) -> Option<String> {
        self.with_tool(|t| t.terminal().input_prompt())
    }

    /// Resize the terminal; a running `vi` redraws.
    fn resize(&self, rows: u16, cols: u16) {
        self.with_tool(|t| t.terminal().resize(TerminalSize::new(rows, cols)));
    }

    /// `(rows, cols)`.
    fn size(&self) -> (u16, u16) {
        let s = self.with_tool(|t| t.terminal().size());
        (s.rows, s.cols)
    }

    /// Shell exit code once the session has exited, else `None`.
    #[getter]
    fn exit_code(&self) -> Option<i32> {
        self.with_tool(|t| t.terminal().exit_code())
    }

    /// The session's virtual filesystem (read what `vi` saved).
    fn fs(&self) -> PyFileSystem {
        let fs = self.with_tool(|t| t.terminal().fs());
        PyFileSystem::from_static(fs, self.rt.clone())
    }

    /// OpenAI-compatible function definition for `call`.
    fn tool_definition(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let def = self.with_tool(|t| t.tool_definition());
        json_to_py(py, &def)
    }

    /// Terse usage guide for a system prompt.
    fn system_prompt(&self) -> String {
        self.with_tool(|t| t.system_prompt())
    }

    fn __repr__(&self) -> PyResult<String> {
        let (size, activity) = self.with_tool(|t| (t.terminal().size(), t.terminal().activity()));
        let state = match activity {
            TerminalActivity::Starting | TerminalActivity::Prompt => "prompt".to_string(),
            TerminalActivity::ContinuationPrompt => "continuation".to_string(),
            TerminalActivity::Running { .. } => "running".to_string(),
            TerminalActivity::Exited(c) => format!("exited({c})"),
        };
        Ok(format!("Terminal({}x{}, {state})", size.cols, size.rows))
    }
}
