//! Shared terminal device: input queue, screen model, mode flags.
//!
//! One [`Tty`] is shared by the [`Terminal`](super::Terminal) host handle, the
//! shell loop that owns the `Bash`, and any builtin (`vi`) that reaches it via
//! the per-execution extension bag. All state sits behind one mutex; nothing
//! holds the lock across an `.await`.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use tokio::sync::Notify;

use super::TerminalSize;
use crate::time_compat::InputWaitClock;

// THREAT[TM-DOS-119]: bound every buffer a host or script can grow.
/// Cap on queued, unread input. Bytes past it are rejected at `send`.
pub(crate) const MAX_PENDING_INPUT: usize = 1024 * 1024;
/// Cap on raw output retained for `take_output`. Oldest bytes drop first; the
/// screen model still sees every byte.
pub(crate) const MAX_PENDING_OUTPUT: usize = 4 * 1024 * 1024;

/// What a blocking terminal read returned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TtyEvent {
    Byte(u8),
    /// The host resized the terminal; full-screen programs should redraw.
    Resize,
    /// The host closed the terminal; no more input will arrive.
    Closed,
}

struct TtyState {
    input: VecDeque<u8>,
    /// A reader is blocked on an empty queue: the session is idle.
    waiting: bool,
    /// Raw mode: bytes go to the reader untouched (no Ctrl-C interrupt).
    raw: bool,
    /// A shell command is running (Ctrl-C should cancel it).
    foreground: bool,
    resized: bool,
    closed: bool,
    screen: vt100::Parser,
    output: Vec<u8>,
}

pub(crate) struct TtyShared {
    state: Mutex<TtyState>,
    input_ready: Notify,
    idle: Notify,
    clock: InputWaitClock,
    cancel: Arc<AtomicBool>,
}

/// Cloneable handle to the shared terminal device.
#[derive(Clone)]
pub(crate) struct Tty(Arc<TtyShared>);

impl Tty {
    pub(crate) fn new(size: TerminalSize, clock: InputWaitClock, cancel: Arc<AtomicBool>) -> Self {
        Self(Arc::new(TtyShared {
            state: Mutex::new(TtyState {
                input: VecDeque::new(),
                waiting: false,
                raw: false,
                foreground: false,
                resized: false,
                closed: false,
                screen: vt100::Parser::new(size.rows, size.cols, 0),
                output: Vec::new(),
            }),
            input_ready: Notify::new(),
            idle: Notify::new(),
            clock,
            cancel,
        }))
    }

    fn lock(&self) -> MutexGuard<'_, TtyState> {
        self.0.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    // ---- host side -------------------------------------------------------

    /// Queue input bytes. Returns how many were accepted.
    ///
    /// In cooked mode while a command runs, Ctrl-C (0x03) cancels the command
    /// instead of being queued, like a real line discipline's ISIG.
    pub(crate) fn send(&self, bytes: &[u8]) -> usize {
        let mut accepted = 0;
        {
            let mut st = self.lock();
            for &b in bytes {
                if b == 0x03 && st.foreground && !st.raw {
                    self.0.cancel.store(true, Ordering::Relaxed);
                    accepted += 1;
                    continue;
                }
                if st.input.len() >= MAX_PENDING_INPUT {
                    break;
                }
                st.input.push_back(b);
                accepted += 1;
            }
            if !st.input.is_empty() {
                st.waiting = false;
            }
        }
        self.0.input_ready.notify_one();
        accepted
    }

    pub(crate) fn resize(&self, size: TerminalSize) {
        {
            let mut st = self.lock();
            st.screen.screen_mut().set_size(size.rows, size.cols);
            st.resized = true;
            st.waiting = false;
        }
        self.0.input_ready.notify_one();
    }

    pub(crate) fn close(&self) {
        {
            let mut st = self.lock();
            st.closed = true;
            st.waiting = false;
        }
        self.0.input_ready.notify_one();
    }

    /// True when a reader is blocked waiting for input that is not there.
    pub(crate) fn is_idle(&self) -> bool {
        let st = self.lock();
        st.waiting && st.input.is_empty() && !st.resized
    }

    pub(crate) async fn idle_signal(&self) {
        self.0.idle.notified().await;
    }

    pub(crate) fn take_output(&self) -> Vec<u8> {
        std::mem::take(&mut self.lock().output)
    }

    pub(crate) fn with_screen<R>(&self, f: impl FnOnce(&vt100::Screen) -> R) -> R {
        f(self.lock().screen.screen())
    }

    // ---- device side -----------------------------------------------------

    pub(crate) fn size(&self) -> TerminalSize {
        let (rows, cols) = self.lock().screen.screen().size();
        TerminalSize { rows, cols }
    }

    pub(crate) fn set_raw(&self, raw: bool) {
        self.lock().raw = raw;
    }

    pub(crate) fn set_foreground(&self, foreground: bool) {
        self.lock().foreground = foreground;
    }

    /// Write bytes to the terminal exactly as given (raw output).
    pub(crate) fn write(&self, bytes: &[u8]) {
        let mut st = self.lock();
        st.screen.process(bytes);
        st.output.extend_from_slice(bytes);
        let len = st.output.len();
        if len > MAX_PENDING_OUTPUT {
            st.output.drain(..len - MAX_PENDING_OUTPUT);
        }
    }

    /// Write with output post-processing (ONLCR): `\n` becomes `\r\n`.
    pub(crate) fn write_cooked(&self, bytes: &[u8]) {
        if !bytes.contains(&b'\n') {
            self.write(bytes);
            return;
        }
        let mut out = Vec::with_capacity(bytes.len() + bytes.len() / 8);
        let mut prev = 0u8;
        for &b in bytes {
            if b == b'\n' && prev != b'\r' {
                out.push(b'\r');
            }
            out.push(b);
            prev = b;
        }
        self.write(&out);
    }

    /// Next queued byte without blocking or consuming it.
    pub(crate) fn peek_byte(&self) -> Option<u8> {
        self.lock().input.front().copied()
    }

    /// Next queued byte without blocking.
    pub(crate) fn try_read_byte(&self) -> Option<u8> {
        self.lock().input.pop_front()
    }

    /// Block until input, a resize, or close. Time spent blocked here does not
    /// count against the execution timeout.
    // THREAT[TM-DOS-120]: only this wait is excluded from the deadline.
    pub(crate) async fn read_event(&self) -> TtyEvent {
        loop {
            {
                let mut st = self.lock();
                if st.resized {
                    st.resized = false;
                    return TtyEvent::Resize;
                }
                if let Some(b) = st.input.pop_front() {
                    st.waiting = false;
                    return TtyEvent::Byte(b);
                }
                if st.closed {
                    return TtyEvent::Closed;
                }
                st.waiting = true;
            }
            self.0.idle.notify_one();
            let _wait = self.0.clock.pause();
            self.0.input_ready.notified().await;
        }
    }
}
