// napi macros generate code that triggers some clippy lints
#![allow(clippy::needless_pass_by_value, clippy::trivially_copy_pass_by_ref)]

//! Node.js/TypeScript bindings for the Bashkit sandboxed bash interpreter.
//!
//! Exposes `Bash` (core interpreter), `BashTool` (interpreter + LLM metadata),
//! and `ExecResult` via napi-rs for use from JavaScript/TypeScript.
//!
//! # Safety: `Arc<SharedState>` pattern
//!
//! Both `Bash` and `BashTool` wrap all mutable state in `Arc<SharedState>`.
//! Every `#[napi]` method clones the `Arc` *before* doing any blocking or async
//! work. This prevents CodeQL `rust/access-invalid-pointer` alerts caused by
//! holding a raw-pointer-derived `&self` across `block_on` or `.await` points.

use bashkit::tool::VERSION;
use bashkit::{
    Bash as RustBash, BashTool as RustBashTool, ExecutionLimits, ExtFunctionResult, MontyObject,
    PythonExternalFnHandler, PythonLimits, ScriptedTool as RustScriptedTool, Tool, ToolArgs,
    ToolDef, ToolRequest,
};
use napi_derive::napi;
use std::collections::HashMap;
use std::path::Path;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::Mutex;

// ---------------------------------------------------------------------------
// Shared tokio runtime + concurrency limiter for JS tool callbacks (issue #982).
// A single multi-thread runtime is created lazily and reused for every callback
// invocation, replacing the previous pattern of spawning an unbounded number of
// OS threads each with its own single-threaded runtime. A semaphore caps the
// maximum number of concurrent in-flight callbacks to prevent DoS.
// ---------------------------------------------------------------------------
const MAX_CONCURRENT_TOOL_CALLBACKS: usize = 10;

fn callback_runtime() -> &'static tokio::runtime::Runtime {
    use std::sync::OnceLock;
    static RT: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RT.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("failed to create shared callback runtime")
    })
}

fn callback_semaphore() -> &'static tokio::sync::Semaphore {
    use std::sync::OnceLock;
    static SEM: OnceLock<tokio::sync::Semaphore> = OnceLock::new();
    SEM.get_or_init(|| tokio::sync::Semaphore::new(MAX_CONCURRENT_TOOL_CALLBACKS))
}

// ============================================================================
// MontyObject <-> JSON conversion
// ============================================================================

#[allow(dead_code)]
fn monty_to_json(obj: &MontyObject) -> serde_json::Value {
    match obj {
        MontyObject::None => serde_json::Value::Null,
        MontyObject::Bool(b) => serde_json::Value::Bool(*b),
        MontyObject::Int(i) => serde_json::json!(*i),
        MontyObject::BigInt(b) => serde_json::Value::String(b.to_string()),
        MontyObject::Float(f) => serde_json::json!(*f),
        MontyObject::String(s) | MontyObject::Path(s) => serde_json::Value::String(s.clone()),
        MontyObject::Bytes(b) => serde_json::Value::String(base64_encode(b)),
        MontyObject::Tuple(items) | MontyObject::List(items) => {
            serde_json::Value::Array(items.iter().map(monty_to_json).collect())
        }
        MontyObject::Set(items) | MontyObject::FrozenSet(items) => {
            serde_json::Value::Array(items.iter().map(monty_to_json).collect())
        }
        MontyObject::Dict(pairs) => {
            let mut map = serde_json::Map::new();
            for (k, v) in pairs.clone() {
                let key = match &k {
                    MontyObject::String(s) => s.clone(),
                    other => format!("{}", monty_to_json(other)),
                };
                map.insert(key, monty_to_json(&v));
            }
            serde_json::Value::Object(map)
        }
        MontyObject::NamedTuple {
            field_names,
            values,
            ..
        } => {
            let mut map = serde_json::Map::new();
            for (name, value) in field_names.iter().zip(values.iter()) {
                map.insert(name.clone(), monty_to_json(value));
            }
            serde_json::Value::Object(map)
        }
        other => serde_json::Value::String(other.py_repr()),
    }
}

#[allow(dead_code)]
fn json_to_monty(val: &serde_json::Value) -> MontyObject {
    match val {
        serde_json::Value::Null => MontyObject::None,
        serde_json::Value::Bool(b) => MontyObject::Bool(*b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                MontyObject::Int(i)
            } else if let Some(f) = n.as_f64() {
                MontyObject::Float(f)
            } else {
                MontyObject::None
            }
        }
        serde_json::Value::String(s) => MontyObject::String(s.clone()),
        serde_json::Value::Array(arr) => MontyObject::List(arr.iter().map(json_to_monty).collect()),
        serde_json::Value::Object(map) => {
            let pairs: Vec<(MontyObject, MontyObject)> = map
                .iter()
                .map(|(k, v)| (MontyObject::String(k.clone()), json_to_monty(v)))
                .collect();
            MontyObject::dict(pairs)
        }
    }
}

#[allow(dead_code)]
fn base64_encode(data: &[u8]) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = chunk.get(1).copied().unwrap_or(0) as u32;
        let b2 = chunk.get(2).copied().unwrap_or(0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        result.push(CHARS[(n >> 18 & 63) as usize] as char);
        result.push(CHARS[(n >> 12 & 63) as usize] as char);
        if chunk.len() > 1 {
            result.push(CHARS[(n >> 6 & 63) as usize] as char);
        } else {
            result.push('=');
        }
        if chunk.len() > 2 {
            result.push(CHARS[(n & 63) as usize] as char);
        } else {
            result.push('=');
        }
    }
    result
}

// ============================================================================
// ExecResult
// ============================================================================

/// Result from executing bash commands.
#[napi(object)]
#[derive(Clone)]
pub struct ExecResult {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    pub error: Option<String>,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
    pub final_env: Option<HashMap<String, String>>,
    /// True if exit_code is 0.
    pub success: bool,
}

// ============================================================================
// BashOptions
// ============================================================================

/// Options for creating a Bash or BashTool instance.
#[napi(object)]
pub struct BashOptions {
    pub username: Option<String>,
    pub hostname: Option<String>,
    pub max_commands: Option<u32>,
    pub max_loop_iterations: Option<u32>,
    /// Files to mount in the virtual filesystem.
    /// Keys are absolute paths, values are file content strings.
    pub files: Option<HashMap<String, String>>,
    /// Enable embedded Python execution (`python`/`python3` builtins).
    pub python: Option<bool>,
    /// Names of external functions callable from embedded Python code.
    pub external_functions: Option<Vec<String>>,
}

fn default_opts() -> BashOptions {
    BashOptions {
        username: None,
        hostname: None,
        max_commands: None,
        max_loop_iterations: None,
        files: None,
        python: None,
        external_functions: None,
    }
}

// ============================================================================
// SharedState — all mutable state behind Arc to avoid raw pointer issues
// ============================================================================

struct SharedState {
    inner: Mutex<RustBash>,
    rt: Mutex<tokio::runtime::Runtime>,
    cancelled: Arc<AtomicBool>,
    username: Option<String>,
    hostname: Option<String>,
    max_commands: Option<u32>,
    max_loop_iterations: Option<u32>,
    python: bool,
    external_functions: Vec<String>,
    external_handler: Option<ExternalHandlerArc>,
}

/// Wrapper for the external handler that can be stored and cloned.
type ExternalHandlerArc = Arc<
    dyn Fn(
            String,
            Vec<MontyObject>,
            Vec<(MontyObject, MontyObject)>,
        ) -> Pin<Box<dyn std::future::Future<Output = ExtFunctionResult> + Send>>
        + Send
        + Sync,
>;

/// Clone `Arc<SharedState>`, then use the runtime to block on a future that
/// captures only the cloned Arc. This avoids holding raw `&self` across
/// `block_on` boundaries.
fn block_on_with<Fut, T>(state: &Arc<SharedState>, f: impl FnOnce(Arc<SharedState>) -> Fut) -> T
where
    Fut: std::future::Future<Output = T>,
{
    let s = state.clone();
    let rt_guard = s.rt.blocking_lock();
    let s2 = state.clone();
    rt_guard.block_on(f(s2))
}

// ============================================================================
// Bash — core interpreter
// ============================================================================

/// Core bash interpreter with virtual filesystem.
///
/// State persists between calls — files created in one `execute()` are
/// available in subsequent calls.
#[napi]
pub struct Bash {
    state: Arc<SharedState>,
}

#[napi]
impl Bash {
    #[napi(constructor)]
    pub fn new(options: Option<BashOptions>) -> napi::Result<Self> {
        let opts = options.unwrap_or_else(default_opts);
        let py = opts.python.unwrap_or(false);
        let ext_fns = opts.external_functions.clone().unwrap_or_default();

        let bash = build_bash(
            opts.username.as_deref(),
            opts.hostname.as_deref(),
            opts.max_commands,
            opts.max_loop_iterations,
            opts.files.as_ref(),
            py,
            &ext_fns,
            None,
        );
        let cancelled = bash.cancellation_token();

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| napi::Error::from_reason(format!("Failed to create runtime: {e}")))?;

        Ok(Self {
            state: Arc::new(SharedState {
                inner: Mutex::new(bash),
                rt: Mutex::new(rt),
                cancelled,
                username: opts.username,
                hostname: opts.hostname,
                max_commands: opts.max_commands,
                max_loop_iterations: opts.max_loop_iterations,
                python: py,
                external_functions: ext_fns,
                external_handler: None,
            }),
        })
    }

    /// Execute bash commands synchronously.
    #[napi]
    pub fn execute_sync(&self, commands: String) -> napi::Result<ExecResult> {
        self.state.cancelled.store(false, Ordering::Relaxed);
        block_on_with(&self.state, |s| async move {
            let mut bash = s.inner.lock().await;
            match bash.exec(&commands).await {
                Ok(result) => Ok(ExecResult {
                    stdout: result.stdout,
                    stderr: result.stderr,
                    exit_code: result.exit_code,
                    error: None,
                    stdout_truncated: result.stdout_truncated,
                    stderr_truncated: result.stderr_truncated,
                    final_env: result.final_env,
                    success: result.exit_code == 0,
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
                        success: false,
                    })
                }
            }
        })
    }

    /// Execute bash commands asynchronously, returning a Promise.
    #[napi]
    pub async fn execute(&self, commands: String) -> napi::Result<ExecResult> {
        let s = self.state.clone();
        let mut bash = s.inner.lock().await;
        match bash.exec(&commands).await {
            Ok(result) => Ok(ExecResult {
                stdout: result.stdout,
                stderr: result.stderr,
                exit_code: result.exit_code,
                error: None,
                stdout_truncated: result.stdout_truncated,
                stderr_truncated: result.stderr_truncated,
                final_env: result.final_env,
                success: result.exit_code == 0,
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
                    success: false,
                })
            }
        }
    }

    /// Cancel the currently running execution.
    ///
    /// Safe to call from any thread. Execution will abort at the next
    /// command boundary.
    #[napi]
    pub fn cancel(&self) {
        self.state.cancelled.store(true, Ordering::Relaxed);
    }

    /// Reset interpreter to fresh state, preserving configuration.
    #[napi]
    pub fn reset(&self) -> napi::Result<()> {
        block_on_with(&self.state, |s| async move {
            let mut bash = s.inner.lock().await;
            let new_bash = build_bash(
                s.username.as_deref(),
                s.hostname.as_deref(),
                s.max_commands,
                s.max_loop_iterations,
                None,
                s.python,
                &s.external_functions,
                s.external_handler.as_ref(),
            );
            *bash = new_bash;
            Ok(())
        })
    }

    // ========================================================================
    // Snapshot / Resume
    // ========================================================================

    /// Serialize interpreter state (shell variables, VFS contents, counters) to bytes.
    ///
    /// Returns a `Buffer` (Uint8Array) that can be persisted and used with
    /// `Bash.fromSnapshot()` to restore the session later.
    #[napi]
    pub fn snapshot(&self) -> napi::Result<napi::bindgen_prelude::Buffer> {
        block_on_with(&self.state, |s| async move {
            let bash = s.inner.lock().await;
            let bytes = bash
                .snapshot()
                .map_err(|e| napi::Error::from_reason(e.to_string()))?;
            Ok(napi::bindgen_prelude::Buffer::from(bytes))
        })
    }

    /// Restore interpreter state from a snapshot previously created with `snapshot()`.
    #[napi]
    pub fn restore_snapshot(&self, data: napi::bindgen_prelude::Buffer) -> napi::Result<()> {
        block_on_with(&self.state, |s| async move {
            let mut bash = s.inner.lock().await;
            bash.restore_snapshot(&data)
                .map_err(|e| napi::Error::from_reason(e.to_string()))
        })
    }

    /// Create a new Bash instance from a snapshot.
    ///
    /// Accepts optional `BashOptions` to re-apply execution limits.
    /// Without options, safe defaults are used (not unlimited).
    #[napi(factory)]
    pub fn from_snapshot(
        data: napi::bindgen_prelude::Buffer,
        options: Option<BashOptions>,
    ) -> napi::Result<Self> {
        let opts = options.unwrap_or_else(default_opts);

        // Build a configured Bash instance with proper limits, then restore snapshot state
        let mut bash = build_bash(
            opts.username.as_deref(),
            opts.hostname.as_deref(),
            opts.max_commands,
            opts.max_loop_iterations,
            opts.files.as_ref(),
            opts.python.unwrap_or(false),
            &opts.external_functions.clone().unwrap_or_default(),
            None,
        );
        // restore_snapshot preserves the instance's limits while restoring shell state
        bash.restore_snapshot(&data)
            .map_err(|e| napi::Error::from_reason(e.to_string()))?;

        let cancelled = bash.cancellation_token();
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| napi::Error::from_reason(format!("Failed to create runtime: {e}")))?;
        Ok(Self {
            state: Arc::new(SharedState {
                inner: Mutex::new(bash),
                rt: tokio::sync::Mutex::new(rt),
                cancelled,
                username: opts.username,
                hostname: opts.hostname,
                max_commands: opts.max_commands,
                max_loop_iterations: opts.max_loop_iterations,
                python: opts.python.unwrap_or(false),
                external_functions: opts.external_functions.unwrap_or_default(),
                external_handler: None,
            }),
        })
    }

    // ========================================================================
    // VFS — direct filesystem access
    // ========================================================================

    /// Read a file from the virtual filesystem. Returns contents as a UTF-8 string.
    #[napi]
    pub fn read_file(&self, path: String) -> napi::Result<String> {
        block_on_with(&self.state, |s| async move {
            let bash = s.inner.lock().await;
            let bytes = bash
                .fs()
                .read_file(Path::new(&path))
                .await
                .map_err(|e| napi::Error::from_reason(e.to_string()))?;
            String::from_utf8(bytes)
                .map_err(|e| napi::Error::from_reason(format!("Invalid UTF-8: {e}")))
        })
    }

    /// Write a string to a file in the virtual filesystem.
    /// Creates the file if it doesn't exist, replaces contents if it does.
    #[napi]
    pub fn write_file(&self, path: String, content: String) -> napi::Result<()> {
        block_on_with(&self.state, |s| async move {
            let bash = s.inner.lock().await;
            bash.fs()
                .write_file(Path::new(&path), content.as_bytes())
                .await
                .map_err(|e| napi::Error::from_reason(e.to_string()))
        })
    }

    /// Create a directory. If recursive is true, creates parent directories as needed.
    #[napi]
    pub fn mkdir(&self, path: String, recursive: Option<bool>) -> napi::Result<()> {
        block_on_with(&self.state, |s| async move {
            let bash = s.inner.lock().await;
            bash.fs()
                .mkdir(Path::new(&path), recursive.unwrap_or(false))
                .await
                .map_err(|e| napi::Error::from_reason(e.to_string()))
        })
    }

    /// Check if a path exists in the virtual filesystem.
    #[napi]
    pub fn exists(&self, path: String) -> napi::Result<bool> {
        block_on_with(&self.state, |s| async move {
            let bash = s.inner.lock().await;
            bash.fs()
                .exists(Path::new(&path))
                .await
                .map_err(|e| napi::Error::from_reason(e.to_string()))
        })
    }

    /// Remove a file or directory. If recursive is true, removes directory contents.
    #[napi]
    pub fn remove(&self, path: String, recursive: Option<bool>) -> napi::Result<()> {
        block_on_with(&self.state, |s| async move {
            let bash = s.inner.lock().await;
            bash.fs()
                .remove(Path::new(&path), recursive.unwrap_or(false))
                .await
                .map_err(|e| napi::Error::from_reason(e.to_string()))
        })
    }

    /// List entries in a directory. Returns entry names.
    #[napi]
    pub fn read_dir(&self, path: String) -> napi::Result<Vec<String>> {
        block_on_with(&self.state, |s| async move {
            let bash = s.inner.lock().await;
            let entries = bash
                .fs()
                .read_dir(Path::new(&path))
                .await
                .map_err(|e| napi::Error::from_reason(e.to_string()))?;
            Ok(entries.into_iter().map(|e| e.name.clone()).collect())
        })
    }
}

// ============================================================================
// BashTool — interpreter + tool-contract metadata
// ============================================================================

/// Bash interpreter with tool-contract metadata (`description`, `help`,
/// `system_prompt`, schemas).
///
/// Use this when integrating with AI frameworks that need tool definitions.
#[napi]
pub struct BashTool {
    state: Arc<SharedState>,
}

impl BashTool {
    fn build_rust_tool(state: &SharedState) -> RustBashTool {
        let mut builder = RustBashTool::builder();

        if let Some(ref username) = state.username {
            builder = builder.username(username);
        }
        if let Some(ref hostname) = state.hostname {
            builder = builder.hostname(hostname);
        }

        let mut limits = ExecutionLimits::new();
        if let Some(mc) = state.max_commands {
            limits = limits.max_commands(mc as usize);
        }
        if let Some(mli) = state.max_loop_iterations {
            limits = limits.max_loop_iterations(mli as usize);
        }

        builder.limits(limits).build()
    }
}

#[napi]
impl BashTool {
    #[napi(constructor)]
    pub fn new(options: Option<BashOptions>) -> napi::Result<Self> {
        let opts = options.unwrap_or_else(default_opts);
        let py = opts.python.unwrap_or(false);
        let ext_fns = opts.external_functions.clone().unwrap_or_default();

        let bash = build_bash(
            opts.username.as_deref(),
            opts.hostname.as_deref(),
            opts.max_commands,
            opts.max_loop_iterations,
            opts.files.as_ref(),
            py,
            &ext_fns,
            None,
        );
        let cancelled = bash.cancellation_token();

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| napi::Error::from_reason(format!("Failed to create runtime: {e}")))?;

        Ok(Self {
            state: Arc::new(SharedState {
                inner: Mutex::new(bash),
                rt: Mutex::new(rt),
                cancelled,
                username: opts.username,
                hostname: opts.hostname,
                max_commands: opts.max_commands,
                max_loop_iterations: opts.max_loop_iterations,
                python: py,
                external_functions: ext_fns,
                external_handler: None,
            }),
        })
    }

    /// Execute bash commands synchronously.
    #[napi]
    pub fn execute_sync(&self, commands: String) -> napi::Result<ExecResult> {
        self.state.cancelled.store(false, Ordering::Relaxed);
        block_on_with(&self.state, |s| async move {
            let mut bash = s.inner.lock().await;
            match bash.exec(&commands).await {
                Ok(result) => Ok(ExecResult {
                    stdout: result.stdout,
                    stderr: result.stderr,
                    exit_code: result.exit_code,
                    error: None,
                    stdout_truncated: result.stdout_truncated,
                    stderr_truncated: result.stderr_truncated,
                    final_env: result.final_env,
                    success: result.exit_code == 0,
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
                        success: false,
                    })
                }
            }
        })
    }

    /// Execute bash commands asynchronously, returning a Promise.
    #[napi]
    pub async fn execute(&self, commands: String) -> napi::Result<ExecResult> {
        let s = self.state.clone();
        let mut bash = s.inner.lock().await;
        match bash.exec(&commands).await {
            Ok(result) => Ok(ExecResult {
                stdout: result.stdout,
                stderr: result.stderr,
                exit_code: result.exit_code,
                error: None,
                stdout_truncated: result.stdout_truncated,
                stderr_truncated: result.stderr_truncated,
                final_env: result.final_env,
                success: result.exit_code == 0,
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
                    success: false,
                })
            }
        }
    }

    /// Cancel the currently running execution.
    #[napi]
    pub fn cancel(&self) {
        self.state.cancelled.store(true, Ordering::Relaxed);
    }

    /// Reset interpreter to fresh state, preserving configuration.
    #[napi]
    pub fn reset(&self) -> napi::Result<()> {
        block_on_with(&self.state, |s| async move {
            let mut bash = s.inner.lock().await;
            let new_bash = build_bash(
                s.username.as_deref(),
                s.hostname.as_deref(),
                s.max_commands,
                s.max_loop_iterations,
                None,
                s.python,
                &s.external_functions,
                s.external_handler.as_ref(),
            );
            *bash = new_bash;
            Ok(())
        })
    }

    /// Get tool name.
    #[napi(getter)]
    pub fn name(&self) -> &str {
        "bashkit"
    }

    /// Get short description.
    #[napi(getter)]
    pub fn short_description(&self) -> &str {
        "Run bash commands in an isolated virtual filesystem"
    }

    /// Get token-efficient tool description.
    #[napi]
    pub fn description(&self) -> String {
        Self::build_rust_tool(&self.state).description().to_string()
    }

    /// Get help as a Markdown document.
    #[napi]
    pub fn help(&self) -> String {
        Self::build_rust_tool(&self.state).help()
    }

    /// Get compact system-prompt text for orchestration.
    #[napi]
    pub fn system_prompt(&self) -> String {
        Self::build_rust_tool(&self.state).system_prompt()
    }

    /// Get JSON input schema as string.
    #[napi]
    pub fn input_schema(&self) -> napi::Result<String> {
        let schema = Self::build_rust_tool(&self.state).input_schema();
        serde_json::to_string_pretty(&schema)
            .map_err(|e| napi::Error::from_reason(format!("Schema serialization failed: {e}")))
    }

    /// Get JSON output schema as string.
    #[napi]
    pub fn output_schema(&self) -> napi::Result<String> {
        let schema = Self::build_rust_tool(&self.state).output_schema();
        serde_json::to_string_pretty(&schema)
            .map_err(|e| napi::Error::from_reason(format!("Schema serialization failed: {e}")))
    }

    /// Get tool version.
    #[napi(getter)]
    pub fn version(&self) -> &str {
        VERSION
    }

    // ========================================================================
    // VFS — direct filesystem access (no shell command composition)
    // ========================================================================

    /// Read a file from the virtual filesystem. Returns contents as a UTF-8 string.
    #[napi]
    pub fn read_file(&self, path: String) -> napi::Result<String> {
        block_on_with(&self.state, |s| async move {
            let bash = s.inner.lock().await;
            let bytes = bash
                .fs()
                .read_file(Path::new(&path))
                .await
                .map_err(|e| napi::Error::from_reason(e.to_string()))?;
            String::from_utf8(bytes)
                .map_err(|e| napi::Error::from_reason(format!("Invalid UTF-8: {e}")))
        })
    }

    /// Write a string to a file in the virtual filesystem.
    #[napi]
    pub fn write_file(&self, path: String, content: String) -> napi::Result<()> {
        block_on_with(&self.state, |s| async move {
            let bash = s.inner.lock().await;
            bash.fs()
                .write_file(Path::new(&path), content.as_bytes())
                .await
                .map_err(|e| napi::Error::from_reason(e.to_string()))
        })
    }

    /// Create a directory. If recursive is true, creates parent directories as needed.
    #[napi]
    pub fn mkdir(&self, path: String, recursive: Option<bool>) -> napi::Result<()> {
        block_on_with(&self.state, |s| async move {
            let bash = s.inner.lock().await;
            bash.fs()
                .mkdir(Path::new(&path), recursive.unwrap_or(false))
                .await
                .map_err(|e| napi::Error::from_reason(e.to_string()))
        })
    }

    /// Check if a path exists in the virtual filesystem.
    #[napi]
    pub fn exists(&self, path: String) -> napi::Result<bool> {
        block_on_with(&self.state, |s| async move {
            let bash = s.inner.lock().await;
            bash.fs()
                .exists(Path::new(&path))
                .await
                .map_err(|e| napi::Error::from_reason(e.to_string()))
        })
    }

    /// Remove a file or directory. If recursive is true, removes directory contents.
    #[napi]
    pub fn remove(&self, path: String, recursive: Option<bool>) -> napi::Result<()> {
        block_on_with(&self.state, |s| async move {
            let bash = s.inner.lock().await;
            bash.fs()
                .remove(Path::new(&path), recursive.unwrap_or(false))
                .await
                .map_err(|e| napi::Error::from_reason(e.to_string()))
        })
    }

    /// List entries in a directory. Returns entry names.
    #[napi]
    pub fn read_dir(&self, path: String) -> napi::Result<Vec<String>> {
        block_on_with(&self.state, |s| async move {
            let bash = s.inner.lock().await;
            let entries = bash
                .fs()
                .read_dir(Path::new(&path))
                .await
                .map_err(|e| napi::Error::from_reason(e.to_string()))?;
            Ok(entries.into_iter().map(|e| e.name.clone()).collect())
        })
    }
}

// ============================================================================
// ScriptedTool — multi-tool orchestration via bash scripts
// ============================================================================

/// Options for creating a ScriptedTool instance.
#[napi(object)]
pub struct ScriptedToolOptions {
    pub name: String,
    pub short_description: Option<String>,
    pub max_commands: Option<u32>,
    pub max_loop_iterations: Option<u32>,
}

/// Threadsafe callback: data=(String,), return=String, CalleeHandled=false.
/// The tuple matches the JS function signature `(request: string) => string`.
type ToolTsfn = napi::threadsafe_function::ThreadsafeFunction<
    (String,),
    String,
    (String,),
    napi::Status,
    false,
>;

/// Entry for a registered JS tool callback.
///
/// Stores a threadsafe function that receives a JSON-serialized request string
/// `{"params": {...}, "stdin": "..." | null}` and returns a string result.
struct JsToolEntry {
    name: String,
    description: String,
    schema: serde_json::Value,
    /// Wrapped in Arc so we can share references with Rust ScriptedTool callbacks
    /// (ThreadsafeFunction doesn't implement Clone).
    callback: Arc<ToolTsfn>,
}

/// Compose JS callbacks as bash builtins for multi-tool orchestration.
///
/// Each registered tool becomes a bash builtin command. An LLM (or user) writes
/// a single bash script that pipes, loops, and branches across all tools.
#[napi]
pub struct ScriptedTool {
    name: String,
    short_desc: Option<String>,
    tools: Vec<JsToolEntry>,
    env_vars: Vec<(String, String)>,
    rt: Mutex<tokio::runtime::Runtime>,
    max_commands: Option<u32>,
    max_loop_iterations: Option<u32>,
}

#[napi]
impl ScriptedTool {
    #[napi(constructor)]
    pub fn new(options: ScriptedToolOptions) -> napi::Result<Self> {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| napi::Error::from_reason(format!("Failed to create runtime: {e}")))?;

        Ok(Self {
            name: options.name,
            short_desc: options.short_description,
            tools: Vec::new(),
            env_vars: Vec::new(),
            rt: Mutex::new(rt),
            max_commands: options.max_commands,
            max_loop_iterations: options.max_loop_iterations,
        })
    }

    /// Register a tool command.
    ///
    /// The callback receives a JSON string `{"params": {...}, "stdin": "..." | null}`
    /// and must return a string result.
    #[napi(
        ts_args_type = "name: string, description: string, callback: (request: string) => string, schema?: string"
    )]
    pub fn add_tool(
        &mut self,
        name: String,
        description: String,
        callback: napi::bindgen_prelude::Function<(String,), String>,
        schema: Option<String>,
    ) -> napi::Result<()> {
        let tsfn: ToolTsfn = callback.build_threadsafe_function::<(String,)>().build()?;

        let schema_val = match schema {
            Some(s) => serde_json::from_str(&s)
                .map_err(|e| napi::Error::from_reason(format!("Invalid schema JSON: {e}")))?,
            None => serde_json::Value::Object(Default::default()),
        };

        self.tools.push(JsToolEntry {
            name,
            description,
            schema: schema_val,
            callback: Arc::new(tsfn),
        });
        Ok(())
    }

    /// Add an environment variable visible inside scripts.
    #[napi]
    pub fn env(&mut self, key: String, value: String) {
        self.env_vars.push((key, value));
    }

    /// Execute a bash script synchronously.
    #[napi]
    pub fn execute_sync(&self, commands: String) -> napi::Result<ExecResult> {
        let tool = self.build_rust_tool();
        let rt_guard = self.rt.blocking_lock();
        let resp = rt_guard.block_on(async move {
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
            stdout_truncated: resp.stdout_truncated,
            stderr_truncated: resp.stderr_truncated,
            final_env: resp.final_env,
            success: resp.exit_code == 0,
        })
    }

    /// Execute a bash script asynchronously, returning a Promise.
    #[napi]
    pub async fn execute(&self, commands: String) -> napi::Result<ExecResult> {
        let tool = self.build_rust_tool();
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
            success: resp.exit_code == 0,
        })
    }

    /// Get tool name.
    #[napi(getter)]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Get short description.
    #[napi(getter)]
    pub fn short_description(&self) -> String {
        self.short_desc
            .clone()
            .unwrap_or_else(|| format!("ScriptedTool: {}", self.name))
    }

    /// Number of registered tools.
    #[napi]
    pub fn tool_count(&self) -> u32 {
        self.tools.len() as u32
    }

    /// Get token-efficient tool description.
    #[napi]
    pub fn description(&self) -> String {
        self.build_rust_tool().description().to_string()
    }

    /// Get help as a Markdown document.
    #[napi]
    pub fn help(&self) -> String {
        self.build_rust_tool().help()
    }

    /// Get compact system-prompt text for orchestration.
    #[napi]
    pub fn system_prompt(&self) -> String {
        self.build_rust_tool().system_prompt()
    }

    /// Get JSON input schema as string.
    #[napi]
    pub fn input_schema(&self) -> napi::Result<String> {
        let tool = self.build_rust_tool();
        let schema = tool.input_schema();
        serde_json::to_string_pretty(&schema)
            .map_err(|e| napi::Error::from_reason(format!("Schema serialization failed: {e}")))
    }

    /// Get JSON output schema as string.
    #[napi]
    pub fn output_schema(&self) -> napi::Result<String> {
        let tool = self.build_rust_tool();
        let schema = tool.output_schema();
        serde_json::to_string_pretty(&schema)
            .map_err(|e| napi::Error::from_reason(format!("Schema serialization failed: {e}")))
    }

    /// Get tool version.
    #[napi(getter)]
    pub fn version(&self) -> &str {
        VERSION
    }
}

impl ScriptedTool {
    fn build_rust_tool(&self) -> RustScriptedTool {
        let mut builder = RustScriptedTool::builder(&self.name);

        if let Some(ref desc) = self.short_desc {
            builder = builder.short_description(desc);
        }

        for entry in &self.tools {
            let tsfn = entry.callback.clone();
            let tool_name = entry.name.clone();

            let callback = move |args: &ToolArgs| -> Result<String, String> {
                // Serialize params + stdin as JSON for the JS callback
                let request = serde_json::json!({
                    "params": args.params,
                    "stdin": args.stdin.as_deref(),
                });
                let request_str = serde_json::to_string(&request).map_err(|e| e.to_string())?;

                // Dispatch the TSFN call on the shared callback runtime with a
                // concurrency semaphore to prevent unbounded thread/task creation
                // (see issue #982).
                let tsfn_clone = tsfn.clone();
                let tool_name_clone = tool_name.clone();
                let rt = callback_runtime();
                let sem = callback_semaphore();
                let (tx, rx) = std::sync::mpsc::channel();
                rt.spawn(async move {
                    let result = match sem.acquire().await {
                        Ok(_permit) => tsfn_clone
                            .call_async((request_str,))
                            .await
                            .map_err(|e| format!("{}: {}", tool_name_clone, e)),
                        Err(e) => Err(format!("{}: semaphore error: {}", tool_name_clone, e)),
                    };
                    let _ = tx.send(result);
                });
                rx.recv()
                    .map_err(|_| format!("{}: callback channel closed", tool_name))?
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

// ============================================================================
// Helpers
// ============================================================================

#[allow(clippy::too_many_arguments)]
fn build_bash(
    username: Option<&str>,
    hostname: Option<&str>,
    max_commands: Option<u32>,
    max_loop_iterations: Option<u32>,
    files: Option<&HashMap<String, String>>,
    python: bool,
    external_functions: &[String],
    external_handler: Option<&ExternalHandlerArc>,
) -> RustBash {
    let mut builder = RustBash::builder();

    if let Some(u) = username {
        builder = builder.username(u);
    }
    if let Some(h) = hostname {
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

    // Mount files into the virtual filesystem
    if let Some(files) = files {
        for (path, content) in files {
            builder = builder.mount_text(path, content);
        }
    }

    // Enable Python/Monty
    if python {
        if let Some(handler) = external_handler {
            let h = handler.clone();
            let fn_names = external_functions.to_vec();
            let python_handler: PythonExternalFnHandler = Arc::new(move |name, args, kwargs| {
                let h = h.clone();
                Box::pin(async move { h(name, args, kwargs).await })
            });
            builder = builder.python_with_external_handler(
                PythonLimits::default(),
                fn_names,
                python_handler,
            );
        } else {
            builder = builder.python();
        }
    }

    builder.build()
}

/// Get the bashkit version string.
#[napi]
pub fn get_version() -> &'static str {
    VERSION
}
