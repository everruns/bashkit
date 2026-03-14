// napi macros generate code that triggers some clippy lints
#![allow(clippy::needless_pass_by_value, clippy::trivially_copy_pass_by_ref)]

//! Node.js/TypeScript bindings for the Bashkit sandboxed bash interpreter.
//!
//! Exposes `Bash` (core interpreter), `BashTool` (interpreter + LLM metadata),
//! and `ExecResult` via napi-rs for use from JavaScript/TypeScript.

use bashkit::tool::VERSION;
use bashkit::{Bash as RustBash, BashTool as RustBashTool, ExecutionLimits, Tool};
use napi_derive::napi;
use std::collections::HashMap;
use std::sync::Arc;
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
// Bash — core interpreter
// ============================================================================

/// Core bash interpreter with virtual filesystem.
///
/// State persists between calls — files created in one `execute()` are
/// available in subsequent calls.
#[napi]
pub struct Bash {
    inner: Arc<Mutex<RustBash>>,
    rt: tokio::runtime::Runtime,
    username: Option<String>,
    hostname: Option<String>,
    max_commands: Option<u32>,
    max_loop_iterations: Option<u32>,
}

#[napi]
impl Bash {
    #[napi(constructor)]
    pub fn new(options: Option<BashOptions>) -> napi::Result<Self> {
        let opts = options.unwrap_or_else(default_opts);

        let bash = build_bash(
            opts.username.as_deref(),
            opts.hostname.as_deref(),
            opts.max_commands,
            opts.max_loop_iterations,
            opts.files.as_ref(),
        );

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| napi::Error::from_reason(format!("Failed to create runtime: {e}")))?;

        Ok(Self {
            inner: Arc::new(Mutex::new(bash)),
            rt,
            username: opts.username,
            hostname: opts.hostname,
            max_commands: opts.max_commands,
            max_loop_iterations: opts.max_loop_iterations,
        })
    }

    /// Execute bash commands synchronously.
    #[napi]
    pub fn execute_sync(&self, commands: String) -> napi::Result<ExecResult> {
        let inner = self.inner.clone();
        self.rt.block_on(async move {
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

    /// Execute bash commands asynchronously, returning a Promise.
    #[napi]
    pub async fn execute(&self, commands: String) -> napi::Result<ExecResult> {
        let inner = self.inner.clone();
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
    }

    /// Reset interpreter to fresh state, preserving configuration.
    #[napi]
    pub fn reset(&self) -> napi::Result<()> {
        let inner = self.inner.clone();
        let username = self.username.clone();
        let hostname = self.hostname.clone();
        let max_commands = self.max_commands;
        let max_loop_iterations = self.max_loop_iterations;

        self.rt.block_on(async move {
            let mut bash = inner.lock().await;
            *bash = build_bash(
                username.as_deref(),
                hostname.as_deref(),
                max_commands,
                max_loop_iterations,
                None,
            );
            Ok(())
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
    inner: Arc<Mutex<RustBash>>,
    rt: tokio::runtime::Runtime,
    username: Option<String>,
    hostname: Option<String>,
    max_commands: Option<u32>,
    max_loop_iterations: Option<u32>,
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
            limits = limits.max_commands(mc as usize);
        }
        if let Some(mli) = self.max_loop_iterations {
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

        let bash = build_bash(
            opts.username.as_deref(),
            opts.hostname.as_deref(),
            opts.max_commands,
            opts.max_loop_iterations,
            opts.files.as_ref(),
        );

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| napi::Error::from_reason(format!("Failed to create runtime: {e}")))?;

        Ok(Self {
            inner: Arc::new(Mutex::new(bash)),
            rt,
            username: opts.username,
            hostname: opts.hostname,
            max_commands: opts.max_commands,
            max_loop_iterations: opts.max_loop_iterations,
        })
    }

    /// Execute bash commands synchronously.
    #[napi]
    pub fn execute_sync(&self, commands: String) -> napi::Result<ExecResult> {
        let inner = self.inner.clone();
        self.rt.block_on(async move {
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

    /// Execute bash commands asynchronously, returning a Promise.
    #[napi]
    pub async fn execute(&self, commands: String) -> napi::Result<ExecResult> {
        let inner = self.inner.clone();
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
    }

    /// Reset interpreter to fresh state, preserving configuration.
    #[napi]
    pub fn reset(&self) -> napi::Result<()> {
        let inner = self.inner.clone();
        let username = self.username.clone();
        let hostname = self.hostname.clone();
        let max_commands = self.max_commands;
        let max_loop_iterations = self.max_loop_iterations;

        self.rt.block_on(async move {
            let mut bash = inner.lock().await;
            *bash = build_bash(
                username.as_deref(),
                hostname.as_deref(),
                max_commands,
                max_loop_iterations,
                None,
            );
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
        let schema = self.build_rust_tool().input_schema();
        serde_json::to_string_pretty(&schema)
            .map_err(|e| napi::Error::from_reason(format!("Schema serialization failed: {e}")))
    }

    /// Get JSON output schema as string.
    #[napi]
    pub fn output_schema(&self) -> napi::Result<String> {
        let schema = self.build_rust_tool().output_schema();
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

/// Get the bashkit version string.
#[napi]
pub fn get_version() -> &'static str {
    VERSION
}
