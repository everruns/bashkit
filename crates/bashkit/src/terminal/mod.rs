//! In-process terminal: keystrokes in, screen text out.
//!
//! [`Terminal`] wraps a [`Bash`](crate::Bash) in a PTY-like device. The host
//! feeds raw input bytes with [`Terminal::send`], drives the session with
//! [`Terminal::run_until_idle`], and reads the result as plain text with
//! [`Terminal::screen_text`] (or as raw bytes with [`Terminal::take_output`]
//! for a real renderer such as xterm.js). Full-screen programs work: the
//! built-in `vi` edits VFS files through it.
//!
//! Behind the `terminal` cargo feature (off by default).
//!
//! # Example
//!
//! ```rust
//! use bashkit::Bash;
//! use bashkit::terminal::{Terminal, TerminalStatus};
//!
//! # #[tokio::main]
//! # async fn main() {
//! let mut term = Terminal::new(Bash::builder());
//! term.send("vi /tmp/note.txt\r");
//! term.run_until_idle().await;
//! term.send("ihello from vi\x1b:wq\r");
//! term.run_until_idle().await;
//!
//! let saved = term.fs().read_file("/tmp/note.txt".as_ref()).await.unwrap();
//! assert_eq!(saved, b"hello from vi\n");
//!
//! term.send("exit 3\r");
//! assert_eq!(term.run_until_idle().await, TerminalStatus::Exited(3));
//! # }
//! ```
//!
//! # Design decisions
//!
//! - Pull-driven, no background task. The shell session is one boxed future
//!   that only runs inside [`Terminal::run_until_idle`]. "Idle" means the
//!   session is blocked reading input and the queue is empty, which is exactly
//!   when a host (human UI or LLM agent) should look at the screen and decide
//!   what to type next. Works the same on multi-thread, current-thread and
//!   single-threaded wasm runtimes.
//! - Screen model is the `vt100` crate (pure Rust, MIT), not libghostty: no
//!   Zig/C toolchain, builds for every bashkit target.
//! - Time blocked on terminal input is excluded from the execution timeout
//!   (TM-DOS-057): typing speed is not sandbox work. All other limits apply.
//! - Cooked mode is a small built-in line discipline (echo, backspace, ^U, ^W,
//!   ^C, ^D). Command stdin is still a value fixed before the command starts
//!   (L-CLI-002), so `read` does not block on the terminal; programs that need
//!   keystrokes (`vi`) read the device directly in raw mode.

mod tty;

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};

pub(crate) use tty::{Tty, TtyEvent};

use crate::fs::FileSystem;
use crate::hooks::{ExitEvent, HookAction};
use crate::time_compat::InputWaitClock;
use crate::{BashBuilder, Error, ExecOptions, ExecutionExtensions};

const DEFAULT_PS1: &str = "$ ";
const DEFAULT_PS2: &str = "> ";
/// Longest line the cooked-mode editor accepts; further input is dropped.
const MAX_LINE_BYTES: usize = 64 * 1024;

/// Terminal dimensions in character cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TerminalSize {
    /// Number of rows (lines).
    pub rows: u16,
    /// Number of columns.
    pub cols: u16,
}

impl Default for TerminalSize {
    fn default() -> Self {
        Self { rows: 24, cols: 80 }
    }
}

impl TerminalSize {
    /// Build a size, clamping each dimension to at least 2.
    pub fn new(rows: u16, cols: u16) -> Self {
        Self {
            rows: rows.max(2),
            cols: cols.max(2),
        }
    }
}

/// Session state reported by [`Terminal::run_until_idle`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalStatus {
    /// Waiting for input. Inspect the screen, then [`Terminal::send`] more.
    Idle,
    /// The shell exited (`exit`, or Ctrl-D on an empty line) with this code.
    Exited(i32),
}

type Session = Pin<Box<dyn Future<Output = i32> + Send>>;

/// An interactive bash session attached to an in-memory terminal.
///
/// See the [module docs](self) for the model and an example.
pub struct Terminal {
    tty: Tty,
    fs: Arc<dyn FileSystem>,
    session: Option<Session>,
    exit_code: Option<i32>,
}

impl Terminal {
    /// Start a session on an 80x24 terminal.
    ///
    /// The builder is used as-is, with the standard file descriptors marked as
    /// terminals (`[ -t 0 ]` is true) and an `on_exit` hook added so `exit`
    /// ends the session.
    pub fn new(builder: BashBuilder) -> Self {
        Self::with_size(builder, TerminalSize::default())
    }

    /// Start a session on a terminal of the given size.
    pub fn with_size(builder: BashBuilder, size: TerminalSize) -> Self {
        let size = TerminalSize::new(size.rows, size.cols);
        let exit = Arc::new(ExitState::default());
        let exit_hook = Arc::clone(&exit);
        let mut bash = builder
            .tty(0, true)
            .tty(1, true)
            .tty(2, true)
            .on_exit(Box::new(move |event: ExitEvent| {
                exit_hook.code.store(event.code, Ordering::SeqCst);
                exit_hook.requested.store(true, Ordering::SeqCst);
                HookAction::Continue(event)
            }))
            .build();
        let clock = InputWaitClock::default();
        bash.input_wait_clock = Some(clock.clone());
        let tty = Tty::new(size, clock, bash.cancellation_token());
        let fs = bash.fs();
        let session: Session = Box::pin(shell_loop(bash, tty.clone(), exit));
        Self {
            tty,
            fs,
            session: Some(session),
            exit_code: None,
        }
    }

    /// Queue raw input bytes, as if typed. Returns how many were accepted
    /// (input past a 1 MiB unread backlog is rejected).
    ///
    /// Use `\r` for Enter, `\x1b` for Escape, `\x03` for Ctrl-C, `\x04` for
    /// Ctrl-D, and `\x1b[A`..`\x1b[D` for the arrow keys. Nothing runs until
    /// [`run_until_idle`](Self::run_until_idle).
    pub fn send(&self, input: impl AsRef<[u8]>) -> usize {
        self.tty.send(input.as_ref())
    }

    /// Run the session until it needs more input or exits.
    ///
    /// Cancellation-safe: dropping the future (e.g. under
    /// `tokio::time::timeout`) leaves the session where it was.
    pub async fn run_until_idle(&mut self) -> TerminalStatus {
        let Some(session) = self.session.as_mut() else {
            return TerminalStatus::Exited(self.exit_code.unwrap_or(0));
        };
        loop {
            if self.tty.is_idle() {
                return TerminalStatus::Idle;
            }
            tokio::select! {
                biased;
                code = session.as_mut() => {
                    self.session = None;
                    self.exit_code = Some(code);
                    return TerminalStatus::Exited(code);
                }
                _ = self.tty.idle_signal() => {}
            }
        }
    }

    /// The visible screen as plain text: one line per row, trailing blanks
    /// and trailing empty rows trimmed. This is what a person looking at the terminal would read.
    pub fn screen_text(&self) -> String {
        let contents = self.tty.with_screen(|s| s.contents());
        let mut text = contents
            .lines()
            .map(str::trim_end)
            .collect::<Vec<_>>()
            .join("\n");
        text.truncate(text.trim_end().len());
        text
    }

    /// Cursor position as `(row, col)`, zero-based.
    pub fn cursor(&self) -> (u16, u16) {
        self.tty.with_screen(|s| s.cursor_position())
    }

    /// Whether a full-screen program (such as `vi`) has switched to the
    /// alternate screen.
    pub fn is_alternate_screen(&self) -> bool {
        self.tty.with_screen(|s| s.alternate_screen())
    }

    /// Drain raw output bytes (with escape sequences) produced since the last
    /// call, for a host-side renderer such as xterm.js. Retains at most the
    /// newest 4 MiB between calls.
    pub fn take_output(&self) -> Vec<u8> {
        self.tty.take_output()
    }

    /// Change the terminal size. Running full-screen programs redraw;
    /// `COLUMNS`/`LINES` update before the next command.
    pub fn resize(&self, size: TerminalSize) {
        self.tty.resize(TerminalSize::new(size.rows, size.cols));
    }

    /// Current terminal size.
    pub fn size(&self) -> TerminalSize {
        self.tty.size()
    }

    /// The session's virtual filesystem, to read what commands and `vi` wrote.
    pub fn fs(&self) -> Arc<dyn FileSystem> {
        Arc::clone(&self.fs)
    }

    /// Exit code once the shell has exited.
    pub fn exit_code(&self) -> Option<i32> {
        self.exit_code
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        self.tty.close();
    }
}

#[derive(Default)]
struct ExitState {
    requested: AtomicBool,
    code: AtomicI32,
}

async fn shell_loop(mut bash: crate::Bash, tty: Tty, exit: Arc<ExitState>) -> i32 {
    let cancel = bash.cancellation_token();
    let mut last_exit = 0;
    if !bash.shell_state_view().env.contains_key("TERM") {
        bash.set_env("TERM", "xterm-256color");
    }
    loop {
        tty.write_cooked(prompt(&bash, "PS1", DEFAULT_PS1).as_bytes());
        let mut input = match read_line(&tty).await {
            LineRead::Line(line) => line,
            LineRead::Interrupt => continue,
            LineRead::Eof => {
                tty.write(b"exit\r\n");
                return last_exit;
            }
        };
        if input.trim().is_empty() {
            continue;
        }

        last_exit = loop {
            let size = tty.size();
            bash.set_env("COLUMNS", &size.cols.to_string());
            bash.set_env("LINES", &size.rows.to_string());
            cancel.store(false, Ordering::Relaxed);
            tty.set_foreground(true);
            let out_tty = tty.clone();
            let options = ExecOptions::new()
                .streaming(Box::new(move |stdout, stderr| {
                    out_tty.write_cooked(stdout.as_bytes());
                    out_tty.write_cooked(stderr.as_bytes());
                }))
                .extensions(ExecutionExtensions::new().with(tty.clone()));
            let result = bash.exec_with_options(&input, options).await;
            tty.set_foreground(false);
            cancel.store(false, Ordering::Relaxed);

            match result {
                Ok(r) => break r.exit_code,
                Err(Error::Cancelled) => {
                    tty.write(b"^C\r\n");
                    break 130;
                }
                Err(e) => {
                    let msg = e.to_string();
                    if !is_incomplete_input(&msg) {
                        tty.write_cooked(format!("bash: {msg}\n").as_bytes());
                        break 2;
                    }
                    tty.write_cooked(prompt(&bash, "PS2", DEFAULT_PS2).as_bytes());
                    match read_line(&tty).await {
                        LineRead::Line(next) => {
                            input.push('\n');
                            input.push_str(&next);
                        }
                        LineRead::Interrupt => break 130,
                        LineRead::Eof => return last_exit,
                    }
                }
            }
        };

        if exit.requested.load(Ordering::SeqCst) {
            return exit.code.load(Ordering::SeqCst);
        }
    }
}

/// Parse errors that mean "keep reading lines" rather than "report error".
/// Same heuristics as the CLI REPL.
fn is_incomplete_input(msg: &str) -> bool {
    let lower = msg.to_lowercase();
    lower.contains("unterminated")
        || lower.contains("unexpected end of input")
        || lower.contains("unexpected eof")
        || lower.contains("syntax error: empty")
        || lower.contains("expected 'fi'")
        || lower.contains("expected 'done'")
        || lower.contains("expected 'esac'")
        || lower.contains("expected '}' to close brace group")
}

/// Expand a prompt variable (`\u \h \w \W \$ \n \e \\`; `\[ \]` dropped).
fn prompt(bash: &crate::Bash, var: &str, default: &str) -> String {
    let state = bash.shell_state_view();
    let Some(raw) = state.variables.get(var).or_else(|| state.env.get(var)) else {
        return default.to_string();
    };
    let env = |k: &str, d: &'static str| state.env.get(k).cloned().unwrap_or_else(|| d.into());
    let cwd = state.cwd.display().to_string();
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('u') => out.push_str(&env("USER", "user")),
            Some('h') => {
                let host = env("HOSTNAME", "bashkit");
                out.push_str(host.split('.').next().unwrap_or(&host));
            }
            Some('H') => out.push_str(&env("HOSTNAME", "bashkit")),
            Some('w') => {
                let home = env("HOME", "");
                match cwd.strip_prefix(home.as_str()) {
                    Some(rest) if !home.is_empty() => {
                        out.push('~');
                        out.push_str(rest);
                    }
                    _ => out.push_str(&cwd),
                }
            }
            Some('W') => out.push_str(cwd.rsplit('/').find(|s| !s.is_empty()).unwrap_or("/")),
            Some('$') => out.push(if env("EUID", "1000") == "0" { '#' } else { '$' }),
            Some('n') => out.push('\n'),
            Some('e') => out.push('\x1b'),
            Some('[') | Some(']') => {}
            Some('\\') => out.push('\\'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

enum LineRead {
    Line(String),
    Interrupt,
    Eof,
}

/// Cooked-mode line editor: echo, backspace, ^U, ^W, ^C, ^D.
async fn read_line(tty: &Tty) -> LineRead {
    let mut line = String::new();
    let mut pending: Vec<u8> = Vec::new();
    loop {
        let b = match tty.read_event().await {
            TtyEvent::Byte(b) => b,
            TtyEvent::Resize => continue,
            TtyEvent::Closed => return LineRead::Eof,
        };
        match b {
            b'\r' | b'\n' => {
                tty.write(b"\r\n");
                return LineRead::Line(line);
            }
            0x03 => {
                tty.write(b"^C\r\n");
                return LineRead::Interrupt;
            }
            0x04 if line.is_empty() => return LineRead::Eof,
            0x7f | 0x08 => {
                if line.pop().is_some() {
                    tty.write(b"\x08 \x08");
                }
            }
            0x15 => {
                for _ in line.drain(..) {
                    tty.write(b"\x08 \x08");
                }
            }
            0x17 => {
                while line.ends_with(' ') {
                    line.pop();
                    tty.write(b"\x08 \x08");
                }
                while !line.is_empty() && !line.ends_with(' ') {
                    line.pop();
                    tty.write(b"\x08 \x08");
                }
            }
            0x1b => skip_escape_sequence(tty),
            b'\t' => {
                if line.len() < MAX_LINE_BYTES {
                    line.push('\t');
                    tty.write(b"\t");
                }
            }
            b if b < 0x20 => {}
            b => {
                pending.push(b);
                match std::str::from_utf8(&pending) {
                    Ok(s) => {
                        if line.len() + s.len() <= MAX_LINE_BYTES {
                            line.push_str(s);
                            tty.write(s.as_bytes());
                        }
                        pending.clear();
                    }
                    Err(e) if e.error_len().is_some() || pending.len() >= 4 => pending.clear(),
                    Err(_) => {}
                }
            }
        }
    }
}

/// Discard a CSI/SS3 sequence (arrow keys etc.) the line editor ignores.
fn skip_escape_sequence(tty: &Tty) {
    if !matches!(tty.peek_byte(), Some(b'[') | Some(b'O')) {
        return;
    }
    tty.try_read_byte();
    for _ in 0..16 {
        match tty.try_read_byte() {
            Some(b) if (0x40..=0x7e).contains(&b) => return,
            Some(_) => {}
            None => return,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Bash;

    async fn run(term: &mut Terminal, input: &str) -> TerminalStatus {
        term.send(input);
        term.run_until_idle().await
    }

    #[tokio::test]
    async fn prompt_and_command_output_render_on_screen() {
        let mut term = Terminal::new(Bash::builder());
        assert_eq!(term.run_until_idle().await, TerminalStatus::Idle);
        assert_eq!(term.screen_text(), "$");
        assert_eq!(term.cursor(), (0, 2));
        run(&mut term, "echo hello\r").await;
        assert_eq!(term.screen_text(), "$ echo hello\nhello\n$");
    }

    #[tokio::test]
    async fn state_persists_between_lines() {
        let mut term = Terminal::new(Bash::builder());
        run(&mut term, "x=42; cd /tmp\r").await;
        run(&mut term, "echo $x $PWD\r").await;
        assert!(term.screen_text().contains("42 /tmp"));
    }

    #[tokio::test]
    async fn exit_ends_session_with_code() {
        let mut term = Terminal::new(Bash::builder());
        assert_eq!(run(&mut term, "exit 7\r").await, TerminalStatus::Exited(7));
        assert_eq!(term.exit_code(), Some(7));
        // Further driving is a no-op.
        assert_eq!(term.run_until_idle().await, TerminalStatus::Exited(7));
    }

    #[tokio::test]
    async fn ctrl_d_on_empty_line_exits() {
        let mut term = Terminal::new(Bash::builder());
        run(&mut term, "false\r").await;
        assert_eq!(run(&mut term, "\x04").await, TerminalStatus::Exited(1));
    }

    #[tokio::test]
    async fn backspace_and_kill_line_edit_input() {
        let mut term = Terminal::new(Bash::builder());
        run(&mut term, "echo abcX\x7f\r").await;
        run(&mut term, "garbage\x15echo ok\r").await;
        let text = term.screen_text();
        assert!(text.contains("\nabc\n"), "{text}");
        assert!(text.contains("\nok\n"), "{text}");
        assert!(!text.contains("garbage"), "{text}");
    }

    #[tokio::test]
    async fn ctrl_c_discards_line() {
        let mut term = Terminal::new(Bash::builder());
        run(&mut term, "echo nope\x03").await;
        run(&mut term, "echo yes\r").await;
        let text = term.screen_text();
        assert!(text.contains("^C"), "{text}");
        assert!(!text.contains("\nnope"), "{text}");
        assert!(text.contains("\nyes"), "{text}");
    }

    #[tokio::test]
    async fn multiline_input_uses_ps2() {
        let mut term = Terminal::new(Bash::builder());
        run(&mut term, "for i in 1 2; do\r").await;
        assert!(
            term.screen_text().ends_with("\n>"),
            "{}",
            term.screen_text()
        );
        run(&mut term, "echo n$i; done\r").await;
        let text = term.screen_text();
        assert!(text.contains("n1\nn2"), "{text}");
    }

    #[tokio::test]
    async fn syntax_error_is_reported_and_session_continues() {
        let mut term = Terminal::new(Bash::builder());
        run(&mut term, "echo )\r").await;
        run(&mut term, "echo still-here\r").await;
        let text = term.screen_text();
        assert!(text.contains("bash: "), "{text}");
        assert!(text.contains("still-here"), "{text}");
    }

    #[tokio::test]
    async fn stdin_is_a_terminal_and_size_is_exported() {
        let mut term = Terminal::with_size(Bash::builder(), TerminalSize::new(30, 100));
        run(&mut term, "[ -t 0 ] && echo tty $COLUMNS $LINES\r").await;
        assert!(term.screen_text().contains("tty 100 30"));
        term.resize(TerminalSize::new(20, 60));
        run(&mut term, "echo $COLUMNS $LINES\r").await;
        assert!(term.screen_text().contains("60 20"));
    }

    #[tokio::test]
    async fn custom_ps1_expands() {
        let mut term = Terminal::new(Bash::builder());
        run(&mut term, "cd /tmp; PS1='[\\W]\\$ '\r").await;
        assert!(
            term.screen_text().ends_with("[tmp]$"),
            "{}",
            term.screen_text()
        );
    }

    #[tokio::test]
    async fn raw_output_is_drainable() {
        let mut term = Terminal::new(Bash::builder());
        run(&mut term, "printf 'a\\nb\\n'\r").await;
        let out = String::from_utf8(term.take_output()).unwrap();
        assert!(out.contains("a\r\nb\r\n"), "{out:?}");
        assert!(term.take_output().is_empty());
    }

    #[tokio::test]
    async fn input_backlog_is_capped() {
        let term = Terminal::new(Bash::builder());
        let big = vec![b'a'; tty::MAX_PENDING_INPUT + 10];
        assert_eq!(term.send(&big), tty::MAX_PENDING_INPUT);
    }

    #[tokio::test]
    async fn waiting_for_input_does_not_count_toward_timeout() {
        let limits = crate::ExecutionLimits::new().timeout(std::time::Duration::from_millis(200));
        let mut term = Terminal::new(Bash::builder().limits(limits));
        run(&mut term, "vi /tmp/t.txt\r").await;
        tokio::time::sleep(std::time::Duration::from_millis(400)).await;
        run(&mut term, "ihi\x1b:wq\r").await;
        assert_eq!(
            term.fs().read_file("/tmp/t.txt".as_ref()).await.unwrap(),
            b"hi\n"
        );
        assert!(
            !term.screen_text().contains("timed out"),
            "{}",
            term.screen_text()
        );
    }

    #[tokio::test]
    async fn busy_work_still_times_out() {
        let limits = crate::ExecutionLimits::new().timeout(std::time::Duration::from_millis(100));
        let mut term = Terminal::new(Bash::builder().limits(limits));
        run(&mut term, "sleep 5; echo after\r").await;
        let text = term.screen_text();
        assert!(!text.contains("\nafter"), "{text}");
        assert!(text.ends_with('$'), "{text}");
    }

    #[tokio::test]
    async fn ctrl_c_cancels_at_next_command_boundary() {
        let mut term = Terminal::new(Bash::builder());
        term.send("sleep 0.3; echo after\r");
        let pending =
            tokio::time::timeout(std::time::Duration::from_millis(50), term.run_until_idle()).await;
        assert!(pending.is_err(), "sleep should still be running");
        assert_eq!(run(&mut term, "\x03").await, TerminalStatus::Idle);
        let text = term.screen_text();
        assert!(!text.contains("\nafter"), "{text}");
        assert!(text.ends_with("^C\n$"), "{text}");
    }

    #[test]
    fn prompt_escape_parsing_falls_back_on_unknown() {
        let bash = Bash::builder().env("PS1", "a\\qb\\").build();
        assert_eq!(prompt(&bash, "PS1", "$ "), "a\\qb\\");
    }
}
