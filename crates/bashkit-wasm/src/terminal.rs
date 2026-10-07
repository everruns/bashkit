//! `Terminal`: an interactive bash session for browser terminal renderers.
//!
//! Wraps `bashkit::terminal::Terminal` (cargo feature `terminal`, off by
//! default). The host forwards keystrokes with `send`, awaits `runUntilIdle`,
//! and writes `takeOutput()` bytes into a renderer such as xterm.js.
//!
//! Decisions:
//! - `runUntilIdle` polls the core future once per wake and drops it between
//!   polls, releasing the `RefCell` borrow each time. The core call is
//!   cancellation-safe (the shell session lives inside the terminal, not in
//!   that future), so this is equivalent to awaiting it, and it lets `send`,
//!   `resize` and `takeOutput` run while a command is still in flight (e.g.
//!   Ctrl-C during `sleep`).
//! - Custom builtins run in async mode only: the terminal never drives a
//!   session synchronously.

use std::cell::RefCell;
use std::future::Future;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::task::Poll;

use bashkit::terminal::{Terminal as CoreTerminal, TerminalSize, TerminalStatus};
use futures_util::future::poll_fn;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::future_to_promise;

use crate::{FileSystem, core_builder, parse_options, seed_files, set_property};

/// Interactive bash session on an in-memory terminal.
#[wasm_bindgen]
pub struct Terminal {
    inner: Rc<RefCell<CoreTerminal>>,
}

#[wasm_bindgen]
impl Terminal {
    /// Start a session. `options` takes every `BashOptions` field plus `rows`
    /// and `cols` (default 24x80).
    #[wasm_bindgen(constructor)]
    pub fn new(options: JsValue) -> Result<Terminal, JsError> {
        let config = parse_options(&options)?;
        let dim = |key: &str, default: u16| -> u16 {
            js_sys::Reflect::get(&options, &JsValue::from_str(key))
                .ok()
                .and_then(|v| v.as_f64())
                .map_or(default, |n| n.clamp(2.0, f64::from(u16::MAX)) as u16)
        };
        let size = if options.is_object() {
            TerminalSize::new(dim("rows", 24), dim("cols", 80))
        } else {
            TerminalSize::default()
        };
        // Never set: terminal sessions always run on the async path.
        let sync_flag = Arc::new(AtomicBool::new(false));
        let term = CoreTerminal::with_size(core_builder(&config, &sync_flag), size);
        seed_files(&term.fs(), &config.files)?;
        Ok(Terminal {
            inner: Rc::new(RefCell::new(term)),
        })
    }

    /// Queue keystrokes (as xterm.js `onData` delivers them). Returns how many
    /// bytes were accepted. Nothing runs until `runUntilIdle`.
    pub fn send(&self, input: String) -> Result<usize, JsError> {
        Ok(self.borrow()?.send(input.as_bytes()))
    }

    /// Queue raw input bytes.
    #[wasm_bindgen(js_name = sendBytes)]
    pub fn send_bytes(&self, input: &[u8]) -> Result<usize, JsError> {
        Ok(self.borrow()?.send(input))
    }

    /// Run until the session waits for input or exits. Resolves to
    /// `{ status: "idle" }` or `{ status: "exited", exitCode }`.
    #[wasm_bindgen(js_name = runUntilIdle)]
    pub fn run_until_idle(&self) -> js_sys::Promise {
        let inner = Rc::clone(&self.inner);
        future_to_promise(async move {
            let status = poll_fn(|cx| {
                let Ok(mut term) = inner.try_borrow_mut() else {
                    // Only reachable from a builtin callback re-entering the
                    // terminal mid-poll; try again on the next tick.
                    cx.waker().wake_by_ref();
                    return Poll::Pending;
                };
                let fut = term.run_until_idle();
                futures_util::pin_mut!(fut);
                fut.poll(cx)
            })
            .await;
            let out = js_sys::Object::new();
            match status {
                TerminalStatus::Idle => set_property(&out, "status", &"idle".into())?,
                TerminalStatus::Exited(code) => {
                    set_property(&out, "status", &"exited".into())?;
                    set_property(&out, "exitCode", &code.into())?;
                }
            }
            Ok(out.into())
        })
    }

    /// Drain raw output bytes (with escape sequences) since the last call.
    #[wasm_bindgen(js_name = takeOutput)]
    pub fn take_output(&self) -> Result<js_sys::Uint8Array, JsError> {
        Ok(self.borrow()?.take_output().as_slice().into())
    }

    /// The visible screen as plain text.
    #[wasm_bindgen(js_name = screenText)]
    pub fn screen_text(&self) -> Result<String, JsError> {
        Ok(self.borrow()?.screen_text())
    }

    /// Change the size; full-screen programs redraw.
    pub fn resize(&self, rows: u16, cols: u16) -> Result<(), JsError> {
        self.borrow()?.resize(TerminalSize::new(rows, cols));
        Ok(())
    }

    /// Current number of rows.
    #[wasm_bindgen(getter)]
    pub fn rows(&self) -> Result<u16, JsError> {
        Ok(self.borrow()?.size().rows)
    }

    /// Current number of columns.
    #[wasm_bindgen(getter)]
    pub fn cols(&self) -> Result<u16, JsError> {
        Ok(self.borrow()?.size().cols)
    }

    /// Whether a full-screen program (`vi`, `less`) is on the alternate screen.
    #[wasm_bindgen(js_name = isAlternateScreen)]
    pub fn is_alternate_screen(&self) -> Result<bool, JsError> {
        Ok(self.borrow()?.is_alternate_screen())
    }

    /// Exit code once the shell has exited, else `undefined`.
    #[wasm_bindgen(getter, js_name = exitCode)]
    pub fn exit_code(&self) -> Result<Option<i32>, JsError> {
        Ok(self.borrow()?.exit_code())
    }

    /// The session's virtual filesystem.
    pub fn fs(&self) -> Result<FileSystem, JsError> {
        Ok(FileSystem::new(self.borrow()?.fs()))
    }
}

impl Terminal {
    fn borrow(&self) -> Result<std::cell::Ref<'_, CoreTerminal>, JsError> {
        self.inner
            .try_borrow()
            .map_err(|_| JsError::new("terminal is busy (reentrant call from a builtin)"))
    }
}
