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
//! use bashkit::terminal::{Terminal, TerminalActivity, TerminalStatus};
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
//! // Exact per-command results, without scraping the screen.
//! term.send("ls /nope\r");
//! term.run_until_idle().await;
//! let record = term.take_transcript().pop().unwrap();
//! assert_eq!(record.command, "ls /nope");
//! assert_ne!(record.exit_code, 0);
//! assert_eq!(term.activity(), TerminalActivity::Prompt);
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
//! - Agents get three views of the session: the screen ([`Terminal::screen_text`]),
//!   the screen plus scrollback ([`Terminal::history_text`]), and a structured
//!   per-command log ([`Terminal::take_transcript`]) with exact output and exit
//!   codes, so they do not have to scrape prompts out of screen text.
//!   [`Terminal::activity`] says whether the shell is at a prompt or which
//!   command line is running.
//! - Cooked mode is a small built-in line editor (echo, cursor keys, Up/Down
//!   history, backspace/delete, ^U, ^K, ^W, ^C, ^D). Command stdin is still a
//!   value fixed before the command starts (L-CLI-002), so `read` does not block on the terminal; programs that need
//!   keystrokes (`vi`) read the device directly in raw mode.

mod keys;
mod tool;
mod tty;

use std::collections::VecDeque;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

pub(crate) use keys::{Key, ScreenGuard, read_key};
pub use tool::{MAIN_SESSION, SessionFactory, TERMINAL_TOOL_NAME, TerminalTool, TerminalToolError};
pub(crate) use tty::{Tty, TtyEvent};

use crate::fs::FileSystem;
use crate::hooks::{ExitEvent, HookAction};
use crate::time_compat::InputWaitClock;
use crate::{BashBuilder, Error, ExecOptions, ExecutionExtensions};

const DEFAULT_PS1: &str = "$ ";
const DEFAULT_PS2: &str = "> ";
/// Longest line the cooked-mode editor accepts; further input is dropped.
const MAX_LINE_BYTES: usize = 64 * 1024;
// THREAT[TM-DOS-119]: the transcript is bounded like every other buffer.
/// Output kept per transcript record; the rest is dropped and flagged.
const MAX_RECORD_OUTPUT: usize = 64 * 1024;
/// Total bytes (commands + outputs) kept across untaken transcript records.
/// Oldest records drop first.
const MAX_TRANSCRIPT_BYTES: usize = 1024 * 1024;

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

/// What the session is doing, from [`Terminal::activity`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TerminalActivity {
    /// Session not started yet (before the first `run_until_idle`).
    Starting,
    /// The shell prompt (`PS1`) is waiting for a command line.
    Prompt,
    /// A command is incomplete and the `PS2` prompt waits for more lines.
    ContinuationPrompt,
    /// A command line is running, for example `vi notes.txt` waiting for keys.
    Running {
        /// The full command line, including continuation lines.
        command: String,
    },
    /// The shell exited with this code.
    Exited(i32),
}

/// One finished command line, from [`Terminal::take_transcript`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandRecord {
    /// The command line as entered (multi-line input joined with `\n`).
    pub command: String,
    /// Combined stdout and stderr, in the order it was written, with plain
    /// `\n` line endings. Full-screen programs (`vi`, `less`) draw directly to
    /// the terminal and are not captured here.
    pub output: String,
    /// Output past 64 KiB was dropped from `output`.
    pub output_truncated: bool,
    /// Exit status (`130` when interrupted with Ctrl-C, `2` for a syntax error).
    pub exit_code: i32,
}

/// Session facts shared between the host handle and the shell loop.
#[derive(Default)]
struct SessionLog {
    activity: Option<TerminalActivity>,
    records: VecDeque<CommandRecord>,
    record_bytes: usize,
}

#[derive(Clone, Default)]
struct SharedLog(Arc<Mutex<SessionLog>>);

impl SharedLog {
    fn lock(&self) -> std::sync::MutexGuard<'_, SessionLog> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn set_activity(&self, activity: TerminalActivity) {
        self.lock().activity = Some(activity);
    }

    fn push(&self, record: CommandRecord) {
        let mut log = self.lock();
        log.record_bytes += record.command.len() + record.output.len();
        log.records.push_back(record);
        while log.record_bytes > MAX_TRANSCRIPT_BYTES {
            let Some(old) = log.records.pop_front() else {
                break;
            };
            log.record_bytes -= old.command.len() + old.output.len();
        }
    }
}

/// Output being collected for the running command's transcript record.
#[derive(Default)]
struct OutputCapture {
    text: String,
    truncated: bool,
}

impl OutputCapture {
    fn append(&mut self, chunk: &str) {
        let room = MAX_RECORD_OUTPUT.saturating_sub(self.text.len());
        if chunk.len() <= room {
            self.text.push_str(chunk);
            return;
        }
        let mut end = room;
        while !chunk.is_char_boundary(end) {
            end -= 1;
        }
        self.text.push_str(&chunk[..end]);
        self.truncated = true;
    }
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
    log: SharedLog,
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
        let interrupt = Arc::new(tokio::sync::Notify::new());
        bash.interrupt = Some(Arc::clone(&interrupt));
        let tty = Tty::new(size, clock, bash.cancellation_token(), interrupt);
        let fs = bash.fs();
        let log = SharedLog::default();
        let session: Session = Box::pin(shell_loop(bash, tty.clone(), exit, log.clone()));
        Self {
            tty,
            fs,
            session: Some(session),
            exit_code: None,
            log,
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
                // Poll once more so a reader with a deadline (`read -t`)
                // whose time is up gets to finish instead of looking idle
                // forever.
                tokio::select! {
                    biased;
                    code = session.as_mut() => {
                        self.session = None;
                        self.exit_code = Some(code);
                        return TerminalStatus::Exited(code);
                    }
                    () = std::future::ready(()) => {}
                }
                if self.tty.is_idle() {
                    return TerminalStatus::Idle;
                }
                continue;
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

    /// The visible screen, one string per row (trailing blanks trimmed,
    /// every row present). Row `i` is screen line `i`.
    pub fn screen_rows(&self) -> Vec<String> {
        self.tty.with_screen(|s| {
            let (_, cols) = s.size();
            s.rows(0, cols).map(|r| r.trim_end().to_string()).collect()
        })
    }

    pub(crate) fn tty(&self) -> &Tty {
        &self.tty
    }

    /// The screen plus up to 1000 lines of scrollback above it, as plain text:
    /// one line per row, oldest first, trailing blanks and trailing empty rows
    /// trimmed. While a full-screen program is open this covers its alternate
    /// screen, which has no scrollback.
    pub fn history_text(&self) -> String {
        let rows = self.tty.history_rows();
        let mut text = rows
            .iter()
            .map(|r| r.trim_end())
            .collect::<Vec<_>>()
            .join("\n");
        text.truncate(text.trim_end().len());
        text
    }

    /// Drain the commands that finished since the last call, oldest first,
    /// with their exact output and exit codes. Keeps at most 1 MiB of
    /// untaken records (oldest drop first).
    pub fn take_transcript(&self) -> Vec<CommandRecord> {
        let mut log = self.log.lock();
        log.record_bytes = 0;
        log.records.drain(..).collect()
    }

    /// What the session is doing right now: at a prompt, running a command
    /// (such as an open `vi`), or exited.
    pub fn activity(&self) -> TerminalActivity {
        if let Some(code) = self.exit_code {
            return TerminalActivity::Exited(code);
        }
        self.log
            .lock()
            .activity
            .clone()
            .unwrap_or(TerminalActivity::Starting)
    }

    /// While a command waits for a typed line (`read`, `select`), the text
    /// left of the cursor on its row: the question being asked, such as
    /// `Continue? [y/N] ` from `read -p` or an `echo -n`. `None` when no
    /// command is waiting for a line (at the shell prompt, inside `vi`, or
    /// while a command is still working).
    pub fn input_prompt(&self) -> Option<String> {
        if !self.tty.is_reading_line() || !self.tty.is_idle() {
            return None;
        }
        Some(self.tty.with_screen(|s| {
            let (row, col) = s.cursor_position();
            let (_, cols) = s.size();
            let text = s.rows(0, cols).nth(usize::from(row)).unwrap_or_default();
            text.chars().take(usize::from(col)).collect::<String>()
        }))
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

async fn shell_loop(mut bash: crate::Bash, tty: Tty, exit: Arc<ExitState>, log: SharedLog) -> i32 {
    let cancel = bash.cancellation_token();
    let mut last_exit = 0;
    if !bash.shell_state_view().env.contains_key("TERM") {
        bash.set_env("TERM", "xterm-256color");
    }
    // Programs that open an editor (`git commit`) read $EDITOR; vi is the
    // one bashkit ships.
    if !bash.shell_state_view().env.contains_key("EDITOR") {
        bash.set_env("EDITOR", "vi");
    }
    let mut history = History::default();
    loop {
        log.set_activity(TerminalActivity::Prompt);
        tty.write_cooked(prompt(&bash, "PS1", DEFAULT_PS1).as_bytes());
        let mut input = match read_line(&tty, &history).await {
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
        history.push(&input);

        let capture = Arc::new(Mutex::new(OutputCapture::default()));
        last_exit = loop {
            log.set_activity(TerminalActivity::Running {
                command: input.clone(),
            });
            let size = tty.size();
            bash.set_env("COLUMNS", &size.cols.to_string());
            bash.set_env("LINES", &size.rows.to_string());
            cancel.store(false, Ordering::Relaxed);
            tty.set_foreground(true);
            let out_tty = tty.clone();
            let out_capture = Arc::clone(&capture);
            let options = ExecOptions::new()
                .streaming(Box::new(move |stdout, stderr| {
                    out_tty.write_cooked(stdout.as_bytes());
                    out_tty.write_cooked(stderr.as_bytes());
                    out_tty.tap_output(stdout.as_bytes());
                    out_tty.tap_output(stderr.as_bytes());
                    let mut cap = out_capture.lock().unwrap_or_else(PoisonError::into_inner);
                    cap.append(stdout);
                    cap.append(stderr);
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
                        let line = format!("bash: {msg}\n");
                        tty.write_cooked(line.as_bytes());
                        capture
                            .lock()
                            .unwrap_or_else(PoisonError::into_inner)
                            .append(&line);
                        break 2;
                    }
                    log.set_activity(TerminalActivity::ContinuationPrompt);
                    tty.write_cooked(prompt(&bash, "PS2", DEFAULT_PS2).as_bytes());
                    match read_line(&tty, &history).await {
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

        bash.carry_exit_code(last_exit);
        let exited = exit.requested.load(Ordering::SeqCst);
        let cap = std::mem::take(&mut *capture.lock().unwrap_or_else(PoisonError::into_inner));
        log.push(CommandRecord {
            command: input,
            output: cap.text,
            output_truncated: cap.truncated,
            exit_code: if exited {
                exit.code.load(Ordering::SeqCst)
            } else {
                last_exit
            },
        });
        if exited {
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

pub(crate) enum LineRead {
    Line(String),
    Interrupt,
    Eof,
}

/// How a command (`read`, `select`) wants one line of typed input.
#[derive(Clone, Copy, Default)]
pub(crate) struct InputOptions {
    /// Do not echo typed characters (`read -s`).
    pub(crate) silent: bool,
    /// Return after this many characters, without waiting for Enter
    /// (`read -n N`).
    pub(crate) nchars: Option<usize>,
}

/// Read one line typed on the terminal for a running command, like a
/// program reading a cooked-mode tty. While it waits, the session reports the
/// line as an input prompt ([`Terminal::input_prompt`]).
pub(crate) async fn read_input(tty: &Tty, opts: InputOptions) -> LineRead {
    let _reading = tty.reading_line();
    if !opts.silent && opts.nchars.is_none() {
        return read_line(tty, &History::default()).await;
    }
    let mut line = String::new();
    let mut pending: Vec<u8> = Vec::new();
    loop {
        if opts.nchars.is_some_and(|n| line.chars().count() >= n) {
            return LineRead::Line(line);
        }
        let b = match tty.read_event().await {
            TtyEvent::Byte(b) => b,
            TtyEvent::Resize => continue,
            TtyEvent::Closed => return LineRead::Eof,
        };
        match b {
            b'\r' | b'\n' => {
                if !opts.silent {
                    tty.write(b"\r\n");
                }
                return LineRead::Line(line);
            }
            0x03 => return LineRead::Interrupt,
            0x04 if line.is_empty() => return LineRead::Eof,
            0x7f | 0x08 => {
                if line.pop().is_some() && !opts.silent {
                    tty.write(b"\x08 \x08");
                }
            }
            b if b < 0x20 && b != b'\t' => {}
            b => {
                pending.push(b);
                match std::str::from_utf8(&pending) {
                    Ok(s) => {
                        if line.len() + s.len() <= MAX_LINE_BYTES {
                            line.push_str(s);
                            if !opts.silent {
                                tty.write(s.as_bytes());
                            }
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

/// Most recent command lines kept for Up/Down recall.
const MAX_HISTORY: usize = 500;

/// Command-line history for the cooked-mode editor (oldest first).
#[derive(Default)]
struct History(std::collections::VecDeque<String>);

impl History {
    fn push(&mut self, line: &str) {
        if self.0.back().is_some_and(|last| last == line) {
            return;
        }
        if self.0.len() == MAX_HISTORY {
            self.0.pop_front();
        }
        self.0.push_back(line.to_string());
    }
}

/// Keys the editor understands from CSI/SS3 escape sequences.
enum EscKey {
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    Delete,
    Other,
}

/// Line being edited plus the cursor, as a char index.
struct LineEditor<'a> {
    tty: &'a Tty,
    chars: Vec<char>,
    cursor: usize,
    bytes: usize,
}

impl<'a> LineEditor<'a> {
    fn new(tty: &'a Tty) -> Self {
        Self {
            tty,
            chars: Vec::new(),
            cursor: 0,
            bytes: 0,
        }
    }

    fn text(&self) -> String {
        self.chars.iter().collect()
    }

    /// Redraw only what changed: step back to where the old and new text
    /// first differ, write the new tail, erase leftovers, then step back to
    /// the new cursor. Assumes one cell per char.
    fn set(&mut self, chars: Vec<char>, cursor: usize) {
        let same = self
            .chars
            .iter()
            .zip(&chars)
            .take_while(|(a, b)| a == b)
            .count()
            .min(self.cursor);
        let mut out = String::new();
        if self.cursor > same {
            out.push_str(&format!("\x1b[{}D", self.cursor - same));
        }
        out.extend(chars[same..].iter());
        out.push_str("\x1b[J");
        let back = chars.len().saturating_sub(cursor);
        if back > 0 {
            out.push_str(&format!("\x1b[{back}D"));
        }
        self.tty.write(out.as_bytes());
        self.bytes = chars.iter().map(|c| c.len_utf8()).sum();
        self.chars = chars;
        self.cursor = cursor;
    }

    fn move_to(&mut self, cursor: usize) {
        let cursor = cursor.min(self.chars.len());
        if cursor < self.cursor {
            self.tty
                .write(format!("\x1b[{}D", self.cursor - cursor).as_bytes());
        } else if cursor > self.cursor {
            self.tty
                .write(format!("\x1b[{}C", cursor - self.cursor).as_bytes());
        }
        self.cursor = cursor;
    }

    fn insert(&mut self, s: &str) {
        if self.bytes + s.len() > MAX_LINE_BYTES {
            return;
        }
        if self.cursor == self.chars.len() {
            // Fast path: typing at the end just echoes.
            self.chars.extend(s.chars());
            self.cursor = self.chars.len();
            self.bytes += s.len();
            self.tty.write(s.as_bytes());
            return;
        }
        let mut chars = self.chars.clone();
        let added: Vec<char> = s.chars().collect();
        let cursor = self.cursor + added.len();
        chars.splice(self.cursor..self.cursor, added);
        self.set(chars, cursor);
    }

    /// Delete `range` (char indices) and leave the cursor at its start.
    fn delete(&mut self, range: std::ops::Range<usize>) {
        if range.is_empty() {
            return;
        }
        let mut chars = self.chars.clone();
        let start = range.start;
        chars.drain(range);
        self.set(chars, start);
    }

    fn replace(&mut self, line: &str) {
        let chars: Vec<char> = line.chars().collect();
        let len = chars.len();
        self.set(chars, len);
    }
}

/// Cooked-mode line editor: echo, cursor movement (Left/Right, Home/End,
/// ^A/^E), Up/Down history, Backspace/Delete, ^U, ^K, ^W, ^C, ^D.
async fn read_line(tty: &Tty, history: &History) -> LineRead {
    let mut ed = LineEditor::new(tty);
    // Index into `history` while browsing; `history.0.len()` is the draft.
    let mut hist_pos = history.0.len();
    let mut draft = String::new();
    let mut pending: Vec<u8> = Vec::new();
    loop {
        let b = match tty.read_event().await {
            TtyEvent::Byte(b) => b,
            TtyEvent::Resize => continue,
            TtyEvent::Closed => return LineRead::Eof,
        };
        match b {
            b'\r' | b'\n' => {
                ed.move_to(ed.chars.len());
                tty.write(b"\r\n");
                return LineRead::Line(ed.text());
            }
            0x03 => {
                ed.move_to(ed.chars.len());
                tty.write(b"^C\r\n");
                return LineRead::Interrupt;
            }
            0x04 if ed.chars.is_empty() => return LineRead::Eof,
            0x04 => ed.delete(ed.cursor..(ed.cursor + 1).min(ed.chars.len())),
            0x7f | 0x08 if ed.cursor > 0 => ed.delete(ed.cursor - 1..ed.cursor),
            0x01 => ed.move_to(0),
            0x05 => ed.move_to(ed.chars.len()),
            0x02 => ed.move_to(ed.cursor.saturating_sub(1)),
            0x06 => ed.move_to(ed.cursor + 1),
            0x0b => ed.delete(ed.cursor..ed.chars.len()),
            0x15 => ed.delete(0..ed.cursor),
            0x17 => {
                let mut start = ed.cursor;
                while start > 0 && ed.chars[start - 1] == ' ' {
                    start -= 1;
                }
                while start > 0 && ed.chars[start - 1] != ' ' {
                    start -= 1;
                }
                ed.delete(start..ed.cursor);
            }
            0x1b => match read_escape(tty) {
                EscKey::Left => ed.move_to(ed.cursor.saturating_sub(1)),
                EscKey::Right => ed.move_to(ed.cursor + 1),
                EscKey::Home => ed.move_to(0),
                EscKey::End => ed.move_to(ed.chars.len()),
                EscKey::Delete => ed.delete(ed.cursor..(ed.cursor + 1).min(ed.chars.len())),
                EscKey::Up if hist_pos > 0 => {
                    if hist_pos == history.0.len() {
                        draft = ed.text();
                    }
                    hist_pos -= 1;
                    ed.replace(&history.0[hist_pos]);
                }
                EscKey::Down if hist_pos < history.0.len() => {
                    hist_pos += 1;
                    match history.0.get(hist_pos) {
                        Some(line) => ed.replace(line),
                        None => ed.replace(&draft),
                    }
                }
                _ => {}
            },
            b'\t' => ed.insert("\t"),
            b if b < 0x20 || b == 0x7f => {}
            b => {
                pending.push(b);
                match std::str::from_utf8(&pending) {
                    Ok(s) => {
                        ed.insert(s);
                        pending.clear();
                    }
                    Err(e) if e.error_len().is_some() || pending.len() >= 4 => pending.clear(),
                    Err(_) => {}
                }
            }
        }
    }
}

/// Read the rest of a CSI/SS3 sequence after ESC and classify it. Unknown
/// sequences are consumed and ignored.
fn read_escape(tty: &Tty) -> EscKey {
    let intro = match tty.peek_byte() {
        Some(b @ (b'[' | b'O')) => b,
        _ => return EscKey::Other,
    };
    tty.try_read_byte();
    let mut params = Vec::new();
    for _ in 0..16 {
        match tty.try_read_byte() {
            Some(b) if (0x40..=0x7e).contains(&b) => {
                return match (intro, params.as_slice(), b) {
                    (_, [], b'A') => EscKey::Up,
                    (_, [], b'B') => EscKey::Down,
                    (_, [], b'C') => EscKey::Right,
                    (_, [], b'D') => EscKey::Left,
                    (_, [], b'H') | (b'[', b"1" | b"7", b'~') => EscKey::Home,
                    (_, [], b'F') | (b'[', b"4" | b"8", b'~') => EscKey::End,
                    (b'[', b"3", b'~') => EscKey::Delete,
                    _ => EscKey::Other,
                };
            }
            Some(b) => params.push(b),
            None => return EscKey::Other,
        }
    }
    EscKey::Other
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Bash;

    async fn run(term: &mut Terminal, input: &str) -> TerminalStatus {
        term.send(input);
        term.run_until_idle().await
    }

    #[cfg(feature = "git")]
    async fn git_repo_term() -> Terminal {
        let mut term = Terminal::new(Bash::builder().git(crate::GitConfig::new()));
        run(
            &mut term,
            "mkdir /r && cd /r && git init -q && echo x > f && git add f\r",
        )
        .await;
        term
    }

    #[cfg(feature = "git")]
    #[tokio::test]
    async fn git_commit_without_m_opens_editor() {
        let mut term = git_repo_term().await;
        run(&mut term, "git commit\r").await;
        assert!(term.is_alternate_screen());
        assert!(
            term.screen_text()
                .contains("# Please enter the commit message")
        );
        run(&mut term, "Ofirst line\x1b:wq\r").await;
        assert!(!term.is_alternate_screen(), "{}", term.screen_text());
        run(&mut term, "git log --oneline\r").await;
        assert!(
            term.screen_text().contains(" first line"),
            "{}",
            term.screen_text()
        );
    }

    #[cfg(feature = "git")]
    #[tokio::test]
    async fn git_commit_editor_abort_paths() {
        let mut term = git_repo_term().await;
        // Template only: empty message aborts.
        run(&mut term, "git commit; echo st=$?\r").await;
        run(&mut term, ":wq\r").await;
        let text = term.screen_text();
        assert!(
            text.contains("Aborting commit due to empty commit message."),
            "{text}"
        );
        assert!(text.contains("st=1"), "{text}");
        // :cq aborts even with text typed.
        run(&mut term, "git commit; echo st=$?\r").await;
        run(&mut term, "Omsg\x1b:cq\r").await;
        assert!(term.screen_text().contains("problem with the editor"));
        // An editor bashkit lacks is reported, not run.
        run(&mut term, "EDITOR=emacs git commit; echo st=$?\r").await;
        assert!(!term.is_alternate_screen());
        assert!(
            term.screen_text()
                .contains("editor 'emacs' is not available")
        );
        run(&mut term, "echo $EDITOR\r").await;
        assert!(
            term.screen_text().ends_with("vi\n$"),
            "{}",
            term.screen_text()
        );
    }

    #[cfg(feature = "git")]
    #[tokio::test]
    async fn git_commit_with_nano_editor() {
        let mut term = git_repo_term().await;
        run(&mut term, "EDITOR=nano git commit\r").await;
        assert!(
            term.screen_text().contains("bashkit nano"),
            "{}",
            term.screen_text()
        );
        // Message on the first (empty) line, then ^X, Y, Enter.
        run(&mut term, "via nano\x18y\r").await;
        assert!(!term.is_alternate_screen());
        run(&mut term, "git log --oneline\r").await;
        assert!(
            term.screen_text().contains(" via nano"),
            "{}",
            term.screen_text()
        );
    }

    #[tokio::test]
    async fn vi_cq_exits_nonzero() {
        let mut term = Terminal::new(Bash::builder());
        run(&mut term, "vi /tmp/a; echo st=$?\r").await;
        run(&mut term, ":cq\r").await;
        assert!(
            term.screen_text().contains("st=1"),
            "{}",
            term.screen_text()
        );
    }

    #[tokio::test]
    async fn read_waits_for_a_typed_line() {
        let mut term = Terminal::new(Bash::builder());
        run(&mut term, "read -p 'Name? ' n; echo \"hi $n\"\r").await;
        assert_eq!(term.input_prompt().as_deref(), Some("Name? "));
        assert!(matches!(term.activity(), TerminalActivity::Running { .. }));
        run(&mut term, "Ann Lee\r").await;
        assert_eq!(term.input_prompt(), None);
        let text = term.screen_text();
        assert!(text.ends_with("Name? Ann Lee\nhi Ann Lee\n$"), "{text}");
        // An echo -n question is the prompt too, and the answer is not
        // treated as a shell command afterwards.
        run(&mut term, "echo -n 'Continue? [y/N] '; read a; echo a=$a\r").await;
        assert_eq!(term.input_prompt().as_deref(), Some("Continue? [y/N] "));
        run(&mut term, "y\r").await;
        assert!(
            term.screen_text().ends_with("a=y\n$"),
            "{}",
            term.screen_text()
        );
    }

    #[tokio::test]
    async fn read_flags_on_terminal() {
        let mut term = Terminal::new(Bash::builder());
        run(&mut term, "read -s -p 'Password: ' pw; echo len=${#pw}\r").await;
        run(&mut term, "hunter2\r").await;
        let text = term.screen_text();
        assert!(!text.contains("hunter2"), "{text}");
        assert!(text.ends_with("Password:\nlen=7\n$"), "{text}");
        run(&mut term, "read -n 1 k; echo; echo k=$k\r").await;
        run(&mut term, "y").await;
        assert!(
            term.screen_text().ends_with("k=y\n$"),
            "{}",
            term.screen_text()
        );
        // The read is idle (waiting) until its -t deadline passes.
        run(&mut term, "read -t 0.05 t; echo rc=$?\r").await;
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        term.run_until_idle().await;
        assert!(
            term.screen_text().ends_with("rc=142\n$"),
            "{}",
            term.screen_text()
        );
        // Ctrl-D on an empty line is EOF.
        run(&mut term, "read e; echo rc=$?\r").await;
        run(&mut term, "\x04").await;
        assert!(
            term.screen_text().ends_with("rc=1\n$"),
            "{}",
            term.screen_text()
        );
        // Ctrl-C while waiting interrupts the command.
        run(&mut term, "read c; echo after\r").await;
        run(&mut term, "\x03").await;
        assert!(
            term.screen_text().ends_with("^C\n$"),
            "{}",
            term.screen_text()
        );
        assert_eq!(term.input_prompt(), None);
        // Piped stdin still wins over the terminal.
        run(&mut term, "echo piped | { read p; echo p=$p; }\r").await;
        assert!(
            term.screen_text().ends_with("p=piped\n$"),
            "{}",
            term.screen_text()
        );
    }

    #[tokio::test]
    async fn select_reads_choice_from_terminal() {
        let mut term = Terminal::new(Bash::builder());
        run(
            &mut term,
            "select c in red blue; do echo picked=$c; break; done\r",
        )
        .await;
        let text = term.screen_text();
        assert!(text.ends_with("1) red\n2) blue\n#?"), "{text}");
        assert_eq!(term.input_prompt().as_deref(), Some("#? "));
        run(&mut term, "2\r").await;
        assert!(
            term.screen_text().ends_with("#? 2\npicked=blue\n$"),
            "{}",
            term.screen_text()
        );
    }

    #[tokio::test]
    async fn watch_refreshes_until_ctrl_c() {
        let mut term = Terminal::new(Bash::builder());
        term.send("printf '0\\033[2J\\n' > /tmp/n; watch -n 0.1 'cat /tmp/n'\r");
        let _ = tokio::time::timeout(std::time::Duration::from_millis(150), term.run_until_idle())
            .await;
        assert!(term.is_alternate_screen());
        let text = term.screen_text();
        // Escape sequences in the output show as text, not terminal control.
        assert!(
            text.starts_with("Every 0.1s: cat /tmp/n\n\n0^[[2J"),
            "{text}"
        );
        // Change the file behind watch's back via the VFS; next tick shows it.
        term.fs()
            .write_file("/tmp/n".as_ref(), b"7\n")
            .await
            .unwrap();
        let _ = tokio::time::timeout(std::time::Duration::from_millis(300), term.run_until_idle())
            .await;
        assert!(
            term.screen_text().ends_with("\n7"),
            "{}",
            term.screen_text()
        );
        assert_eq!(run(&mut term, "\x03").await, TerminalStatus::Idle);
        assert!(!term.is_alternate_screen());
        assert!(
            term.screen_text().ends_with("^C\n$"),
            "{}",
            term.screen_text()
        );
    }

    #[tokio::test]
    async fn watch_chgexit_returns_on_change() {
        let mut term = Terminal::new(Bash::builder());
        term.send("echo a > /tmp/g; watch -g -n 0.1 cat /tmp/g; echo changed\r");
        let _ = tokio::time::timeout(std::time::Duration::from_millis(150), term.run_until_idle())
            .await;
        term.fs()
            .write_file("/tmp/g".as_ref(), b"b\n")
            .await
            .unwrap();
        let status =
            tokio::time::timeout(std::time::Duration::from_secs(5), term.run_until_idle()).await;
        assert_eq!(status, Ok(TerminalStatus::Idle));
        assert!(!term.is_alternate_screen());
        assert!(
            term.screen_text().ends_with("changed\n$"),
            "{}",
            term.screen_text()
        );
    }

    #[tokio::test]
    async fn man_opens_in_less() {
        let mut term = Terminal::new(Bash::builder());
        run(&mut term, "man grep\r").await;
        assert!(term.is_alternate_screen());
        assert!(
            term.screen_text().starts_with("GREP(1)"),
            "{}",
            term.screen_text()
        );
        run(&mut term, "q").await;
        assert!(!term.is_alternate_screen());
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
    async fn arrow_keys_edit_mid_line() {
        let mut term = Terminal::new(Bash::builder());
        // Left x5, insert; Home + Delete x2 + insert; Ctrl-E, Ctrl-A, Ctrl-K.
        run(
            &mut term,
            "echo world\x1b[D\x1b[D\x1b[D\x1b[D\x1b[Dhello \r",
        )
        .await;
        run(&mut term, "XXcho hi\x1b[H\x1b[3~\x1b[3~e\x1b[F!\r").await;
        run(
            &mut term,
            "echo keep junk\x1b[D\x1b[D\x1b[D\x1b[D\x1b[D\x0b\x01\x05\r",
        )
        .await;
        let text = term.screen_text();
        assert!(text.contains("$ echo hello world\nhello world\n"), "{text}");
        assert!(text.contains("$ echo hi!\nhi!\n"), "{text}");
        assert!(text.contains("\nkeep\n$"), "{text}");
    }

    #[tokio::test]
    async fn up_down_recall_history() {
        let mut term = Terminal::new(Bash::builder());
        run(&mut term, "echo one\r").await;
        run(&mut term, "echo two\r").await;
        // Up twice reaches "echo one"; Down returns to "echo two"; edit it.
        run(&mut term, "\x1b[A\x1b[A\x1b[B\x7fZ\r").await;
        // Up past the oldest entry stays there; Down past the newest restores
        // the draft.
        run(
            &mut term,
            "draft\x1b[A\x1b[A\x1b[A\x1b[A\x1b[B\x1b[B\x1b[B\x1b[B\x15echo end\r",
        )
        .await;
        let text = term.screen_text();
        assert!(text.contains("$ echo twZ\ntwZ\n"), "{text}");
        assert!(text.contains("$ echo end\nend\n$"), "{text}");
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
    async fn ctrl_c_interrupts_a_running_command() {
        let mut term = Terminal::new(Bash::builder());
        term.send("sleep 30; echo after\r");
        let pending =
            tokio::time::timeout(std::time::Duration::from_millis(50), term.run_until_idle()).await;
        assert!(pending.is_err(), "sleep should still be running");
        let started = std::time::Instant::now(); // std-time-ok: native-only test
        assert_eq!(run(&mut term, "\x03").await, TerminalStatus::Idle);
        assert!(
            started.elapsed() < std::time::Duration::from_secs(5),
            "Ctrl-C must not wait for sleep to finish"
        );
        let text = term.screen_text();
        assert!(!text.contains("\nafter"), "{text}");
        assert!(text.ends_with("^C\n$"), "{text}");
        run(&mut term, "echo status=$?\r").await;
        assert!(
            term.screen_text().contains("\nstatus=130"),
            "{}",
            term.screen_text()
        );
        // The session keeps working after the interrupt.
        run(&mut term, "f() { echo in-f; }; f\r").await;
        assert!(
            term.screen_text().contains("\nin-f"),
            "{}",
            term.screen_text()
        );
    }

    #[tokio::test]
    async fn exit_status_carries_across_lines() {
        let mut term = Terminal::new(Bash::builder());
        run(&mut term, "false\r").await;
        run(&mut term, "echo st=$?\r").await;
        assert!(
            term.screen_text().contains("\nst=1"),
            "{}",
            term.screen_text()
        );
        run(&mut term, "echo st=$?\r").await;
        assert!(
            term.screen_text().contains("\nst=0"),
            "{}",
            term.screen_text()
        );
    }

    #[tokio::test]
    async fn ctrl_c_interrupts_a_busy_loop() {
        let mut term = Terminal::new(Bash::builder());
        term.send("while true; do sleep 0.01; done\r");
        let _ =
            tokio::time::timeout(std::time::Duration::from_millis(50), term.run_until_idle()).await;
        assert_eq!(run(&mut term, "\x03").await, TerminalStatus::Idle);
        assert!(
            term.screen_text().ends_with("^C\n$"),
            "{}",
            term.screen_text()
        );
    }

    #[tokio::test]
    async fn transcript_records_commands_output_and_exit_codes() {
        let mut term = Terminal::new(Bash::builder());
        run(&mut term, "echo hi; echo err >&2\r").await;
        run(&mut term, "false\r").await;
        run(&mut term, "for i in 1 2; do\r").await;
        run(&mut term, "echo n$i; done\r").await;
        run(&mut term, "echo )\r").await;
        let records = term.take_transcript();
        let summary: Vec<_> = records
            .iter()
            .map(|r| (r.command.as_str(), r.output.as_str(), r.exit_code))
            .collect();
        assert_eq!(summary[0], ("echo hi; echo err >&2", "hi\nerr\n", 0));
        assert_eq!(summary[1], ("false", "", 1));
        assert_eq!(
            summary[2],
            ("for i in 1 2; do\necho n$i; done", "n1\nn2\n", 0)
        );
        assert_eq!(summary[3].2, 2);
        assert!(summary[3].1.starts_with("bash: "), "{:?}", summary[3]);
        assert_eq!(records.len(), 4);
        assert!(term.take_transcript().is_empty(), "drained");
    }

    #[tokio::test]
    async fn transcript_marks_interrupt_and_exit() {
        let mut term = Terminal::new(Bash::builder());
        term.send("sleep 0.3; echo after\r");
        let _ =
            tokio::time::timeout(std::time::Duration::from_millis(50), term.run_until_idle()).await;
        run(&mut term, "\x03").await;
        run(&mut term, "exit 4\r").await;
        let records = term.take_transcript();
        assert_eq!(records[0].exit_code, 130);
        assert_eq!(records[1].command, "exit 4");
        assert_eq!(records[1].exit_code, 4);
    }

    #[tokio::test]
    async fn transcript_output_is_capped_per_record() {
        let mut term = Terminal::new(Bash::builder());
        run(&mut term, "seq 1 30000\r").await;
        let record = term.take_transcript().remove(0);
        assert!(record.output_truncated);
        assert_eq!(record.output.len(), MAX_RECORD_OUTPUT);
        assert!(record.output.starts_with("1\n2\n"));
    }

    #[test]
    fn transcript_total_size_is_bounded() {
        let log = SharedLog::default();
        for i in 0..40 {
            log.push(CommandRecord {
                command: format!("cmd{i}"),
                output: "x".repeat(MAX_RECORD_OUTPUT),
                output_truncated: false,
                exit_code: 0,
            });
        }
        let log = log.lock();
        assert!(log.record_bytes <= MAX_TRANSCRIPT_BYTES);
        assert_eq!(log.records.back().unwrap().command, "cmd39");
        assert!(log.records.front().unwrap().command != "cmd0");
    }

    #[tokio::test]
    async fn activity_tracks_prompt_program_and_exit() {
        let mut term = Terminal::new(Bash::builder());
        assert_eq!(term.activity(), TerminalActivity::Starting);
        term.run_until_idle().await;
        assert_eq!(term.activity(), TerminalActivity::Prompt);
        run(&mut term, "vi /tmp/a.txt\r").await;
        assert_eq!(
            term.activity(),
            TerminalActivity::Running {
                command: "vi /tmp/a.txt".into()
            }
        );
        run(&mut term, ":q\r").await;
        assert_eq!(term.activity(), TerminalActivity::Prompt);
        run(&mut term, "if true; then\r").await;
        assert_eq!(term.activity(), TerminalActivity::ContinuationPrompt);
        run(&mut term, "echo in; fi\r").await;
        run(&mut term, "exit 3\r").await;
        assert_eq!(term.activity(), TerminalActivity::Exited(3));
    }

    #[tokio::test]
    async fn history_text_keeps_lines_scrolled_off_screen() {
        let mut term = Terminal::with_size(Bash::builder(), TerminalSize::new(5, 40));
        run(&mut term, "seq 1 50\r").await;
        let screen = term.screen_text();
        assert!(!screen.contains("\n10\n"), "{screen}");
        let history = term.history_text();
        assert!(history.starts_with("$ seq 1 50\n1\n2\n"), "{history}");
        assert!(history.ends_with("49\n50\n$"), "{history}");
        assert_eq!(history.lines().count(), 52);
        // Reading history leaves the live view untouched.
        assert_eq!(term.screen_text(), screen);
        run(&mut term, "echo next\r").await;
        assert!(term.screen_text().ends_with("next\n$"));
    }

    #[tokio::test]
    async fn history_is_bounded() {
        let mut term = Terminal::with_size(Bash::builder(), TerminalSize::new(5, 20));
        run(&mut term, "seq 1 3000\r").await;
        let history = term.history_text();
        assert_eq!(history.lines().count(), tty::SCROLLBACK_LINES + 5);
        assert!(history.ends_with("3000\n$"), "{history}");
    }

    #[test]
    fn prompt_escape_parsing_falls_back_on_unknown() {
        let bash = Bash::builder().env("PS1", "a\\qb\\").build();
        assert_eq!(prompt(&bash, "PS1", "$ "), "a\\qb\\");
    }
}
