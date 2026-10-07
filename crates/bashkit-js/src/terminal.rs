//! `Terminal`: interactive in-process terminal (vi, less, a persistent shell)
//! for Node, Bun and Deno.
//!
//! Decisions:
//! - Wraps the core `TerminalTool`, so JS gets both the low-level loop
//!   (`send` / `runUntilIdle` / `screenText`) and the agent-shaped
//!   `call(input, waitMs)` with Vim key notation.
//! - `runUntilIdle` drives the session in short slices (`SLICE`), releasing the
//!   lock between them. The core loop is cancellation-safe, so this changes
//!   nothing for the session, and it lets `send("\x03")` or `screenText()`
//!   from JS get in while a long command runs instead of waiting for it.
//! - Async methods are plain methods that clone the `Arc` and spawn a free
//!   async function (`Env::spawn_future`), not `async fn(&self)`: the pending
//!   promise never dereferences the napi-wrapped object (CodeQL
//!   `rust/access-invalid-pointer`).
//! - Construction options are a small subset of `Bash` options (identity,
//!   cwd, env, size).

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use bashkit::Bash as RustBash;
use bashkit::terminal::{TerminalActivity, TerminalSize, TerminalStatus, TerminalTool};
use napi::Env;
use napi::bindgen_prelude::{Buffer, Either, PromiseRaw};
use napi_derive::napi;
use tokio::sync::Mutex;

const SLICE: Duration = Duration::from_millis(20);

#[napi(object)]
pub struct TerminalOptions {
    pub rows: Option<u32>,
    pub cols: Option<u32>,
    pub username: Option<String>,
    pub hostname: Option<String>,
    pub cwd: Option<String>,
    pub env: Option<HashMap<String, String>>,
}

/// One finished command line.
#[napi(object)]
pub struct TerminalCommandRecord {
    pub command: String,
    /// Combined stdout and stderr with `\n` line endings.
    pub output: String,
    pub exit_code: i32,
    pub output_truncated: bool,
}

/// What the session is doing.
#[napi(object)]
pub struct TerminalActivityInfo {
    /// `"prompt"`, `"continuation"`, `"running"` or `"exited"`.
    pub state: String,
    /// The running command line (state `"running"`).
    pub command: Option<String>,
    /// Shell exit code (state `"exited"`).
    pub exit_code: Option<i32>,
}

fn clamp_dim(v: Option<u32>, default: u16) -> u16 {
    v.map_or(default, |n| n.min(u32::from(u16::MAX)) as u16)
}

/// Interactive bash session on an in-memory terminal.
#[napi]
pub struct Terminal {
    tool: Arc<Mutex<TerminalTool>>,
}

#[napi]
impl Terminal {
    #[napi(constructor)]
    pub fn new(options: Option<TerminalOptions>) -> Self {
        let opts = options.unwrap_or(TerminalOptions {
            rows: None,
            cols: None,
            username: None,
            hostname: None,
            cwd: None,
            env: None,
        });
        let mut builder = RustBash::builder();
        if let Some(u) = opts.username {
            builder = builder.username(u);
        }
        if let Some(h) = opts.hostname {
            builder = builder.hostname(h);
        }
        if let Some(c) = opts.cwd {
            builder = builder.cwd(c);
        }
        for (k, v) in opts.env.unwrap_or_default() {
            builder = builder.env(&k, &v);
        }
        let size = TerminalSize::new(clamp_dim(opts.rows, 24), clamp_dim(opts.cols, 80));
        Self {
            tool: Arc::new(Mutex::new(TerminalTool::with_size(builder, size))),
        }
    }

    /// Clone the Arc before blocking on the lock, so no raw-pointer-derived
    /// `&self` is held while blocked (same pattern as `block_on_with` in lib.rs).
    fn with<R>(&self, f: impl FnOnce(&mut TerminalTool) -> R) -> R {
        let tool = Arc::clone(&self.tool);
        let mut guard = tool.blocking_lock();
        f(&mut guard)
    }

    /// Queue raw input as if typed (`"\r"` Enter, `"\x1b"` Escape,
    /// `"\x03"` Ctrl-C). Returns how many bytes were accepted.
    #[napi]
    pub fn send(&self, data: Either<String, Buffer>) -> u32 {
        let bytes: Vec<u8> = match data {
            Either::A(s) => s.into_bytes(),
            Either::B(b) => b.to_vec(),
        };
        self.with(|t| t.terminal().send(&bytes)) as u32
    }

    /// Run until the session needs input or exits. Resolves `"idle"`,
    /// `"exited"`, or `"timeout"` when `timeoutMs` passes first (the command
    /// keeps running state; call again or send Ctrl-C).
    #[napi(ts_return_type = "Promise<'idle' | 'exited' | 'timeout'>")]
    pub fn run_until_idle<'env>(
        &self,
        env: &'env Env,
        timeout_ms: Option<u32>,
    ) -> napi::Result<PromiseRaw<'env, String>> {
        env.spawn_future(run_until_idle(Arc::clone(&self.tool), timeout_ms))
    }

    /// Agent step: tool arguments as a JSON string in, result JSON out; the
    /// JS wrapper builds and parses them (see `call`).
    #[napi(js_name = "__callJson", ts_return_type = "Promise<string>")]
    pub fn call_json<'env>(
        &self,
        env: &'env Env,
        args_json: String,
    ) -> napi::Result<PromiseRaw<'env, String>> {
        env.spawn_future(call_json(Arc::clone(&self.tool), args_json))
    }

    /// Visible screen as plain text.
    #[napi]
    pub fn screen_text(&self) -> String {
        self.with(|t| t.terminal().screen_text())
    }

    /// Screen plus up to 1000 lines of scrollback.
    #[napi]
    pub fn history_text(&self) -> String {
        self.with(|t| t.terminal().history_text())
    }

    /// Raw output bytes (with escape sequences) since the last call, for a
    /// renderer such as xterm.js.
    #[napi]
    pub fn take_output(&self) -> Buffer {
        Buffer::from(self.with(|t| t.terminal().take_output()))
    }

    /// Commands that finished since the last call.
    #[napi]
    pub fn take_transcript(&self) -> Vec<TerminalCommandRecord> {
        self.with(|t| t.terminal().take_transcript())
            .into_iter()
            .map(|r| TerminalCommandRecord {
                command: r.command,
                output: r.output,
                exit_code: r.exit_code,
                output_truncated: r.output_truncated,
            })
            .collect()
    }

    /// What the session is doing.
    #[napi]
    pub fn activity(&self) -> TerminalActivityInfo {
        let (state, command, exit_code) = match self.with(|t| t.terminal().activity()) {
            TerminalActivity::Starting | TerminalActivity::Prompt => ("prompt", None, None),
            TerminalActivity::ContinuationPrompt => ("continuation", None, None),
            TerminalActivity::Running { command } => ("running", Some(command), None),
            TerminalActivity::Exited(code) => ("exited", None, Some(code)),
        };
        TerminalActivityInfo {
            state: state.into(),
            command,
            exit_code,
        }
    }

    /// Cursor position as `[row, col]`, zero-based.
    #[napi(ts_return_type = "[number, number]")]
    pub fn cursor(&self) -> Vec<u32> {
        let (r, c) = self.with(|t| t.terminal().cursor());
        vec![u32::from(r), u32::from(c)]
    }

    /// True while a full-screen program such as `vi` is open.
    #[napi]
    pub fn is_alternate_screen(&self) -> bool {
        self.with(|t| t.terminal().is_alternate_screen())
    }

    /// Resize the terminal; a running `vi` redraws.
    #[napi]
    pub fn resize(&self, rows: u32, cols: u32) {
        let size = TerminalSize::new(clamp_dim(Some(rows), 24), clamp_dim(Some(cols), 80));
        self.with(|t| t.terminal().resize(size));
    }

    /// `[rows, cols]`.
    #[napi(ts_return_type = "[number, number]")]
    pub fn size(&self) -> Vec<u32> {
        let s = self.with(|t| t.terminal().size());
        vec![u32::from(s.rows), u32::from(s.cols)]
    }

    /// Shell exit code once the session has exited.
    #[napi(getter)]
    pub fn exit_code(&self) -> Option<i32> {
        self.with(|t| t.terminal().exit_code())
    }

    /// Read a file from the session's virtual filesystem.
    #[napi(ts_return_type = "Promise<Buffer>")]
    pub fn read_file<'env>(
        &self,
        env: &'env Env,
        path: String,
    ) -> napi::Result<PromiseRaw<'env, Buffer>> {
        env.spawn_future(read_file(Arc::clone(&self.tool), path))
    }

    /// OpenAI-compatible function definition for `call`, as JSON.
    #[napi(js_name = "__toolDefinitionJson")]
    pub fn tool_definition_json(&self) -> String {
        self.with(|t| t.tool_definition()).to_string()
    }

    /// Terse usage guide for a system prompt.
    #[napi]
    pub fn system_prompt(&self) -> String {
        self.with(|t| t.system_prompt())
    }
}

// Async work runs in free functions that own a cloned `Arc`: the spawned
// future never touches the napi-wrapped `Terminal`, so it stays valid even if
// JS drops the object while the promise is pending.

async fn run_until_idle(
    tool: Arc<Mutex<TerminalTool>>,
    timeout_ms: Option<u32>,
) -> napi::Result<String> {
    let deadline =
        timeout_ms.map(|ms| tokio::time::Instant::now() + Duration::from_millis(u64::from(ms)));
    loop {
        let mut slice = SLICE;
        if let Some(d) = deadline {
            let left = d.saturating_duration_since(tokio::time::Instant::now());
            if left.is_zero() {
                return Ok("timeout".into());
            }
            slice = slice.min(left);
        }
        let mut guard = tool.lock().await;
        match tokio::time::timeout(slice, guard.terminal_mut().run_until_idle()).await {
            Ok(TerminalStatus::Idle) => return Ok("idle".into()),
            Ok(TerminalStatus::Exited(_)) => return Ok("exited".into()),
            Err(_) => {}
        }
        drop(guard);
        tokio::task::yield_now().await;
    }
}

async fn call_json(tool: Arc<Mutex<TerminalTool>>, args_json: String) -> napi::Result<String> {
    let args: serde_json::Value = serde_json::from_str(&args_json)
        .map_err(|e| napi::Error::from_reason(format!("invalid call arguments: {e}")))?;
    let mut guard = tool.lock().await;
    let out = guard
        .call(args)
        .await
        .map_err(|e| napi::Error::from_reason(e.to_string()))?;
    Ok(out.to_string())
}

async fn read_file(tool: Arc<Mutex<TerminalTool>>, path: String) -> napi::Result<Buffer> {
    let fs = tool.lock().await.terminal().fs();
    fs.read_file(std::path::Path::new(&path))
        .await
        .map(Buffer::from)
        .map_err(|e| napi::Error::from_reason(e.to_string()))
}
