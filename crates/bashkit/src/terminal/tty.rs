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
/// Cap on recent command output kept for `TerminalTool`'s `wait_for` match.
pub(crate) const MAX_OUTPUT_TAP: usize = 256 * 1024;
/// Lines of primary-screen history kept for `history_text`.
pub(crate) const SCROLLBACK_LINES: usize = 1000;

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
    /// A command is reading a typed line (`read`, `select`).
    reading_line: bool,
    resized: bool,
    closed: bool,
    screen: vt100::Parser,
    output: Vec<u8>,
    /// Recent command stdout/stderr (no echo or prompts), for `wait_for`.
    tap: Vec<u8>,
    /// Total bytes ever appended to `tap`; marks index into it.
    tap_total: u64,
}

pub(crate) struct TtyShared {
    state: Mutex<TtyState>,
    input_ready: Notify,
    idle: Notify,
    clock: InputWaitClock,
    cancel: Arc<AtomicBool>,
    /// Fired with `cancel` so `exec` drops the running command mid-way.
    interrupt: Arc<Notify>,
}

/// Cloneable handle to the shared terminal device.
#[derive(Clone)]
pub(crate) struct Tty(Arc<TtyShared>);

impl Tty {
    pub(crate) fn new(
        size: TerminalSize,
        clock: InputWaitClock,
        cancel: Arc<AtomicBool>,
        interrupt: Arc<Notify>,
    ) -> Self {
        Self(Arc::new(TtyShared {
            state: Mutex::new(TtyState {
                input: VecDeque::new(),
                waiting: false,
                raw: false,
                foreground: false,
                reading_line: false,
                resized: false,
                closed: false,
                screen: vt100::Parser::new(size.rows, size.cols, SCROLLBACK_LINES),
                output: Vec::new(),
                tap: Vec::new(),
                tap_total: 0,
            }),
            input_ready: Notify::new(),
            idle: Notify::new(),
            clock,
            cancel,
            interrupt,
        }))
    }

    fn lock(&self) -> MutexGuard<'_, TtyState> {
        self.0.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    // ---- host side -------------------------------------------------------

    /// Queue input bytes. Returns how many were accepted.
    ///
    /// In cooked mode while a command runs, Ctrl-C (0x03) interrupts the
    /// command instead of being queued, like a real line discipline's ISIG.
    pub(crate) fn send(&self, bytes: &[u8]) -> usize {
        let mut accepted = 0;
        {
            let mut st = self.lock();
            for &b in bytes {
                if b == 0x03 && st.foreground && !st.raw {
                    self.0.cancel.store(true, Ordering::Relaxed);
                    self.0.interrupt.notify_one();
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

    /// Position in the command-output stream; pass to `output_since`.
    pub(crate) fn output_mark(&self) -> u64 {
        self.lock().tap_total
    }

    /// Command output written after `mark`, at most `MAX_OUTPUT_TAP` bytes
    /// (the newest ones).
    pub(crate) fn output_since(&self, mark: u64) -> Vec<u8> {
        let st = self.lock();
        let new = st.tap_total.saturating_sub(mark);
        let keep = usize::try_from(new).unwrap_or(usize::MAX).min(st.tap.len());
        st.tap[st.tap.len() - keep..].to_vec()
    }

    pub(crate) fn with_screen<R>(&self, f: impl FnOnce(&vt100::Screen) -> R) -> R {
        f(self.lock().screen.screen())
    }

    /// Every retained row of the current screen buffer, oldest first:
    /// scrollback history, then the visible rows. One string per row.
    pub(crate) fn history_rows(&self) -> Vec<String> {
        let mut st = self.lock();
        let screen = st.screen.screen_mut();
        let (rows, cols) = screen.size();
        let rows = usize::from(rows);
        screen.set_scrollback(usize::MAX);
        let total = screen.scrollback();
        let mut out = Vec::with_capacity(total + rows);
        // At offset `off` the view starts `off` rows into history; take the
        // history part of each page until the live screen is reached.
        let mut off = total;
        while off > 0 {
            screen.set_scrollback(off);
            let take = off.min(rows);
            out.extend(screen.rows(0, cols).take(take));
            off -= take;
        }
        screen.set_scrollback(0);
        out.extend(screen.rows(0, cols));
        out
    }

    // ---- device side -----------------------------------------------------

    pub(crate) fn size(&self) -> TerminalSize {
        let (rows, cols) = self.lock().screen.screen().size();
        TerminalSize { rows, cols }
    }

    pub(crate) fn set_raw(&self, raw: bool) {
        self.lock().raw = raw;
    }

    /// Mark that a command is reading a typed line until the guard drops.
    pub(crate) fn reading_line(&self) -> ReadingGuard {
        self.lock().reading_line = true;
        ReadingGuard(self.clone())
    }

    pub(crate) fn is_reading_line(&self) -> bool {
        self.lock().reading_line
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

    /// Record command stdout/stderr for `output_since`. Does not draw.
    pub(crate) fn tap_output(&self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        let mut st = self.lock();
        st.tap.extend_from_slice(bytes);
        st.tap_total += bytes.len() as u64;
        let len = st.tap.len();
        if len > MAX_OUTPUT_TAP {
            st.tap.drain(..len - MAX_OUTPUT_TAP);
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
            // A reader dropped mid-wait (`read -t` deadline, Ctrl-C) must not
            // leave the session looking idle while the command carries on.
            let _waiting = WaitingGuard(self);
            self.0.input_ready.notified().await;
        }
    }
}

/// Clears the reading-line mark on drop, including when Ctrl-C drops the
/// running command mid-read.
pub(crate) struct ReadingGuard(Tty);

impl Drop for ReadingGuard {
    fn drop(&mut self) {
        self.0.lock().reading_line = false;
    }
}

struct WaitingGuard<'a>(&'a Tty);

impl Drop for WaitingGuard<'_> {
    fn drop(&mut self) {
        self.0.lock().waiting = false;
    }
}
