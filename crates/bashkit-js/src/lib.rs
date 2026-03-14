// napi macros generate code that triggers some clippy lints
#![allow(clippy::needless_pass_by_value, clippy::trivially_copy_pass_by_ref)]

//! Node.js/TypeScript bindings for the Bashkit sandboxed bash interpreter.
//!
//! Exposes `Bash` (core interpreter), `BashTool` (interpreter + LLM metadata),
//! and `ExecResult` via napi-rs for use from JavaScript/TypeScript.
//!
//! # Safety: handle-based registry pattern
//!
//! napi-rs stores `#[napi]` structs behind raw pointers that are dereferenced
//! on every method call.  CodeQL flags transitive pointer chains reachable from
//! these raw pointers (`rust/access-invalid-pointer`).
//!
//! To eliminate this, both `Bash` and `BashTool` store only a plain `u64`
//! handle.  All real state lives in a global `REGISTRY` keyed by handle.
//! Methods look up the `Arc<SharedState>` from the registry, so the napi raw
//! pointer never transitively reaches heap-allocated interpreter state.

use bashkit::tool::VERSION;
use bashkit::{Bash as RustBash, BashTool as RustBashTool, ExecutionLimits, Tool};
use napi_derive::napi;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, RwLock};
use tokio::sync::Mutex;

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
}

fn default_opts() -> BashOptions {
    BashOptions {
        username: None,
        hostname: None,
        max_commands: None,
        max_loop_iterations: None,
        files: None,
    }
}

// ============================================================================
// Handle-based registry — keeps all interpreter state outside the napi raw
// pointer, avoiding CodeQL rust/access-invalid-pointer on transitive chains.
// ============================================================================

static NEXT_HANDLE: AtomicU64 = AtomicU64::new(1);
static REGISTRY: LazyLock<RwLock<HashMap<u64, Arc<SharedState>>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));

fn register(state: Arc<SharedState>) -> u64 {
    let handle = NEXT_HANDLE.fetch_add(1, Ordering::Relaxed);
    REGISTRY.write().unwrap().insert(handle, state);
    handle
}

fn lookup(handle: u64) -> napi::Result<Arc<SharedState>> {
    REGISTRY
        .read()
        .unwrap()
        .get(&handle)
        .cloned()
        .ok_or_else(|| napi::Error::from_reason("Interpreter instance has been disposed"))
}

fn unregister(handle: u64) {
    REGISTRY.write().unwrap().remove(&handle);
}

// ============================================================================
// Shared interpreter state
// ============================================================================

struct SharedState {
    interpreter: Arc<Mutex<RustBash>>,
    cancelled: Arc<AtomicBool>,
    username: Option<String>,
    hostname: Option<String>,
    max_commands: Option<u32>,
    max_loop_iterations: Option<u32>,
}

impl SharedState {
    fn new(opts: BashOptions) -> napi::Result<Arc<Self>> {
        let bash = build_bash(
            opts.username.as_deref(),
            opts.hostname.as_deref(),
            opts.max_commands,
            opts.max_loop_iterations,
            opts.files.as_ref(),
        );
        let cancelled = bash.cancellation_token();

        Ok(Arc::new(Self {
            interpreter: Arc::new(Mutex::new(bash)),
            cancelled,
            username: opts.username,
            hostname: opts.hostname,
            max_commands: opts.max_commands,
            max_loop_iterations: opts.max_loop_iterations,
        }))
    }

    fn execute_sync(&self, commands: &str) -> napi::Result<ExecResult> {
        self.cancelled.store(false, Ordering::Relaxed);
        let interpreter = self.interpreter.clone();
        let commands = commands.to_owned();
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| napi::Error::from_reason(format!("Failed to create runtime: {e}")))?;
        rt.block_on(async move {
            let mut bash = interpreter.lock().await;
            exec_to_result(&mut bash, &commands).await
        })
    }

    async fn execute_async(&self, commands: &str) -> napi::Result<ExecResult> {
        let interpreter = self.interpreter.clone();
        let mut bash = interpreter.lock().await;
        exec_to_result(&mut bash, commands).await
    }

    fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }

    fn reset(&self) -> napi::Result<()> {
        let interpreter = self.interpreter.clone();
        let username = self.username.clone();
        let hostname = self.hostname.clone();
        let max_commands = self.max_commands;
        let max_loop_iterations = self.max_loop_iterations;

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| napi::Error::from_reason(format!("Failed to create runtime: {e}")))?;
        rt.block_on(async move {
            let mut bash = interpreter.lock().await;
            let new_bash = build_bash(
                username.as_deref(),
                hostname.as_deref(),
                max_commands,
                max_loop_iterations,
                None,
            );
            *bash = new_bash;
            Ok(())
        })
    }
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
    handle: u64,
}

impl Drop for Bash {
    fn drop(&mut self) {
        unregister(self.handle);
    }
}

#[napi]
impl Bash {
    #[napi(constructor)]
    pub fn new(options: Option<BashOptions>) -> napi::Result<Self> {
        let opts = options.unwrap_or_else(default_opts);
        let state = SharedState::new(opts)?;
        Ok(Self {
            handle: register(state),
        })
    }

    /// Execute bash commands synchronously.
    #[napi]
    pub fn execute_sync(&self, commands: String) -> napi::Result<ExecResult> {
        lookup(self.handle)?.execute_sync(&commands)
    }

    /// Execute bash commands asynchronously, returning a Promise.
    #[napi]
    pub async fn execute(&self, commands: String) -> napi::Result<ExecResult> {
        lookup(self.handle)?.execute_async(&commands).await
    }

    /// Cancel the currently running execution.
    ///
    /// Safe to call from any thread. Execution will abort at the next
    /// command boundary.
    #[napi]
    pub fn cancel(&self) {
        if let Ok(state) = lookup(self.handle) {
            state.cancel();
        }
    }

    /// Reset interpreter to fresh state, preserving configuration.
    #[napi]
    pub fn reset(&self) -> napi::Result<()> {
        lookup(self.handle)?.reset()
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
    handle: u64,
}

impl Drop for BashTool {
    fn drop(&mut self) {
        unregister(self.handle);
    }
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
        let state = SharedState::new(opts)?;
        Ok(Self {
            handle: register(state),
        })
    }

    /// Execute bash commands synchronously.
    #[napi]
    pub fn execute_sync(&self, commands: String) -> napi::Result<ExecResult> {
        lookup(self.handle)?.execute_sync(&commands)
    }

    /// Execute bash commands asynchronously, returning a Promise.
    #[napi]
    pub async fn execute(&self, commands: String) -> napi::Result<ExecResult> {
        lookup(self.handle)?.execute_async(&commands).await
    }

    /// Cancel the currently running execution.
    #[napi]
    pub fn cancel(&self) {
        if let Ok(state) = lookup(self.handle) {
            state.cancel();
        }
    }

    /// Reset interpreter to fresh state, preserving configuration.
    #[napi]
    pub fn reset(&self) -> napi::Result<()> {
        lookup(self.handle)?.reset()
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
    pub fn description(&self) -> napi::Result<String> {
        let state = lookup(self.handle)?;
        Ok(Self::build_rust_tool(&state).description().to_string())
    }

    /// Get help as a Markdown document.
    #[napi]
    pub fn help(&self) -> napi::Result<String> {
        let state = lookup(self.handle)?;
        Ok(Self::build_rust_tool(&state).help())
    }

    /// Get compact system-prompt text for orchestration.
    #[napi]
    pub fn system_prompt(&self) -> napi::Result<String> {
        let state = lookup(self.handle)?;
        Ok(Self::build_rust_tool(&state).system_prompt())
    }

    /// Get JSON input schema as string.
    #[napi]
    pub fn input_schema(&self) -> napi::Result<String> {
        let state = lookup(self.handle)?;
        let schema = Self::build_rust_tool(&state).input_schema();
        serde_json::to_string_pretty(&schema)
            .map_err(|e| napi::Error::from_reason(format!("Schema serialization failed: {e}")))
    }

    /// Get JSON output schema as string.
    #[napi]
    pub fn output_schema(&self) -> napi::Result<String> {
        let state = lookup(self.handle)?;
        let schema = Self::build_rust_tool(&state).output_schema();
        serde_json::to_string_pretty(&schema)
            .map_err(|e| napi::Error::from_reason(format!("Schema serialization failed: {e}")))
    }

    /// Get tool version.
    #[napi(getter)]
    pub fn version(&self) -> &str {
        VERSION
    }
}

// ============================================================================
// Helpers
// ============================================================================

fn build_bash(
    username: Option<&str>,
    hostname: Option<&str>,
    max_commands: Option<u32>,
    max_loop_iterations: Option<u32>,
    files: Option<&HashMap<String, String>>,
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

    builder.build()
}

async fn exec_to_result(bash: &mut RustBash, commands: &str) -> napi::Result<ExecResult> {
    match bash.exec(commands).await {
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
}

/// Get the bashkit version string.
#[napi]
pub fn get_version() -> &'static str {
    VERSION
}
