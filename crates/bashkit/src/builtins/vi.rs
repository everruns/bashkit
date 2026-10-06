//! `vi` builtin: a small modal text editor for the in-process terminal.
//!
//! Only usable inside a [`Terminal`](crate::terminal::Terminal) session: it
//! reads keystrokes from the terminal device in raw mode and draws on the
//! alternate screen. Under plain `exec()` there is no terminal, so it fails
//! like real vi with stdin redirected from a file would be unusable.
//!
//! Important decisions:
//! - A useful subset, not vim: normal/insert/command-line modes, counts,
//!   common motions and operators, undo/redo, `/` search, `:w :q :wq :x :s`.
//!   No visual mode, registers, macros, splits, vimrc or `:!` (no shell escape
//!   from the editor; run commands at the prompt instead).
//! - Lines are `Vec<char>`; one char is one screen cell (tabs expand to the
//!   next multiple of 8). Wide CJK glyphs may misalign the cursor.
//! - Bounded memory: files over `MAX_BUFFER_BYTES` are refused, inserts past
//!   it are dropped, and the undo history is capped by total bytes.
//! - Writes go through the session VFS, so VFS limits and read-only mounts
//!   apply to `:w` like any other write.

use std::path::PathBuf;

use async_trait::async_trait;

use super::{Builtin, Context, resolve_path};
use crate::error::{Error, Result};
use crate::interpreter::ExecResult;
use crate::terminal::{Key, ScreenGuard, Tty, read_key};

// THREAT[TM-DOS-119]: bounded editor memory.
/// Largest buffer the editor will load or grow to.
const MAX_BUFFER_BYTES: usize = 8 * 1024 * 1024;
/// Cap on bytes retained across all undo/redo snapshots.
const MAX_UNDO_BYTES: usize = 16 * 1024 * 1024;
const TAB_STOP: usize = 8;

/// The vi builtin.
pub struct Vi;

#[async_trait]
impl Builtin for Vi {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let mut file: Option<&String> = None;
        for arg in ctx.args {
            if arg == "--help" {
                return Ok(ExecResult::ok(
                    "usage: vi [FILE]\nModal text editor; needs an interactive terminal.\n",
                ));
            }
            if arg.starts_with('-') || arg.starts_with('+') {
                return Ok(ExecResult::err(
                    format!("vi: unsupported option: {arg}\n"),
                    2,
                ));
            }
            if file.is_some() {
                return Ok(ExecResult::err("vi: only one file is supported\n", 2));
            }
            file = Some(arg);
        }

        let Some(tty) = ctx.execution_extension::<Tty>() else {
            return Ok(ExecResult::err(
                "vi: not a terminal (run inside bashkit::terminal::Terminal)\n",
                1,
            ));
        };
        let tty = tty.try_with(Clone::clone).map_err(|_| Error::Cancelled)?;

        let path = file.map(|f| resolve_path(ctx.cwd, f));
        let mut editor = Editor::new(tty.size().rows, tty.size().cols);
        editor.display_name = file.cloned();
        if let Some(path) = &path {
            match ctx.fs.stat(path).await {
                Ok(meta) if meta.file_type.is_dir() => {
                    return Ok(ExecResult::err(
                        format!("vi: {}: is a directory\n", path.display()),
                        1,
                    ));
                }
                Ok(meta) if meta.size as usize > MAX_BUFFER_BYTES => {
                    return Ok(ExecResult::err(
                        format!("vi: {}: file too large\n", path.display()),
                        1,
                    ));
                }
                Ok(_) => {
                    let bytes = ctx.fs.read_file(path).await?;
                    editor.load(&String::from_utf8_lossy(&bytes));
                    editor.status = format!(
                        "\"{}\" {}L, {}B",
                        editor.name(),
                        editor.lines.len(),
                        bytes.len()
                    );
                }
                Err(_) => editor.status = format!("\"{}\" [New]", editor.name()),
            }
        }
        editor.path = path;
        editor.saved_hash = editor.buffer_hash();

        let _screen = ScreenGuard::enter(&tty, true);
        editor.run(&tty, &ctx).await;
        Ok(ExecResult::ok(""))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Normal,
    Insert,
    /// Command line: `:` ex command or `/` search.
    Prompt(char),
}

#[derive(Clone)]
struct Snapshot {
    lines: Vec<Vec<char>>,
    row: usize,
    col: usize,
}

impl Snapshot {
    fn bytes(&self) -> usize {
        self.lines.iter().map(|l| l.len() * 4 + 1).sum()
    }
}

#[derive(Clone)]
struct Yank {
    text: Vec<Vec<char>>,
    linewise: bool,
}

struct Editor {
    lines: Vec<Vec<char>>,
    row: usize,
    col: usize,
    top: usize,
    left: usize,
    rows: u16,
    cols: u16,
    mode: Mode,
    prompt: String,
    status: String,
    path: Option<PathBuf>,
    display_name: Option<String>,
    /// Hash of the buffer as last loaded or written; differs when modified.
    saved_hash: u64,
    count: Option<usize>,
    pending: Option<char>,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    yank: Option<Yank>,
    last_search: Option<String>,
    /// Ex command queued by a normal-mode key (`ZZ`), run by the async loop.
    pending_ex: Option<String>,
    quit: bool,
}

impl Editor {
    fn new(rows: u16, cols: u16) -> Self {
        Self {
            lines: vec![Vec::new()],
            row: 0,
            col: 0,
            top: 0,
            left: 0,
            rows,
            cols,
            mode: Mode::Normal,
            prompt: String::new(),
            status: String::new(),
            path: None,
            display_name: None,
            saved_hash: 0,
            count: None,
            pending: None,
            undo: Vec::new(),
            redo: Vec::new(),
            yank: None,
            last_search: None,
            pending_ex: None,
            quit: false,
        }
    }

    fn name(&self) -> String {
        self.display_name
            .clone()
            .unwrap_or_else(|| "[No Name]".to_string())
    }

    fn load(&mut self, text: &str) {
        let body = text.strip_suffix('\n').unwrap_or(text);
        self.lines = if text.is_empty() {
            vec![Vec::new()]
        } else {
            body.split('\n')
                .map(|l| l.strip_suffix('\r').unwrap_or(l).chars().collect())
                .collect()
        };
    }

    fn text(&self) -> String {
        if self.lines.len() == 1 && self.lines[0].is_empty() {
            return String::new();
        }
        let mut out = String::new();
        for line in &self.lines {
            out.extend(line.iter());
            out.push('\n');
        }
        out
    }

    fn buffer_hash(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        self.lines.hash(&mut h);
        h.finish()
    }

    fn is_dirty(&self) -> bool {
        self.buffer_hash() != self.saved_hash
    }

    fn buffer_bytes(&self) -> usize {
        self.lines.iter().map(|l| l.len() + 1).sum()
    }

    // ---- main loop -------------------------------------------------------

    async fn run(&mut self, tty: &Tty, ctx: &Context<'_>) {
        self.render(tty);
        while !self.quit {
            let key = match read_key(tty).await {
                Some(Ok(key)) => key,
                Some(Err(())) => {
                    let size = tty.size();
                    self.rows = size.rows;
                    self.cols = size.cols;
                    self.render(tty);
                    continue;
                }
                None => return,
            };
            match self.mode {
                Mode::Normal => self.normal_key(key),
                Mode::Insert => self.insert_key(key),
                Mode::Prompt(kind) => {
                    if let Some(cmd) = self.prompt_key(key) {
                        self.mode = Mode::Normal;
                        if kind == ':' {
                            self.ex_command(&cmd, ctx).await;
                        } else {
                            self.search(Some(cmd), true);
                        }
                    }
                }
            }
            if let Some(cmd) = self.pending_ex.take() {
                self.ex_command(&cmd, ctx).await;
            }
            if !self.quit {
                self.render(tty);
            }
        }
    }

    // ---- rendering -------------------------------------------------------

    fn text_rows(&self) -> usize {
        (self.rows as usize).saturating_sub(1).max(1)
    }

    fn display_col(line: &[char], col: usize) -> usize {
        let mut w = 0;
        for &c in line.iter().take(col) {
            w = if c == '\t' {
                (w / TAB_STOP + 1) * TAB_STOP
            } else {
                w + 1
            };
        }
        w
    }

    fn render(&mut self, tty: &Tty) {
        let text_rows = self.text_rows();
        let cols = self.cols as usize;
        if self.row < self.top {
            self.top = self.row;
        }
        if self.row >= self.top + text_rows {
            self.top = self.row + 1 - text_rows;
        }
        let cursor_x = Self::display_col(&self.lines[self.row], self.col);
        if cursor_x < self.left {
            self.left = cursor_x;
        }
        if cursor_x >= self.left + cols {
            self.left = cursor_x + 1 - cols;
        }

        let mut out = String::from("\x1b[?25l");
        for screen_row in 0..text_rows {
            out.push_str(&format!("\x1b[{};1H\x1b[2K", screen_row + 1));
            match self.lines.get(self.top + screen_row) {
                Some(line) => {
                    let mut x = 0;
                    for &c in line {
                        let (glyph, width) = if c == '\t' {
                            (' ', TAB_STOP - x % TAB_STOP)
                        } else if c.is_control() {
                            ('?', 1)
                        } else {
                            (c, 1)
                        };
                        for _ in 0..width {
                            if x >= self.left && x < self.left + cols {
                                out.push(glyph);
                            }
                            x += 1;
                        }
                        if x >= self.left + cols {
                            break;
                        }
                    }
                }
                None => out.push('~'),
            }
        }
        out.push_str(&format!("\x1b[{};1H\x1b[2K", self.rows));
        let (status, prompt_cursor) = match self.mode {
            Mode::Prompt(kind) => {
                let s = format!("{kind}{}", self.prompt);
                let len = s.chars().count();
                (s, Some(len))
            }
            Mode::Insert if self.status.is_empty() => ("-- INSERT --".to_string(), None),
            _ => (self.status.clone(), None),
        };
        out.extend(status.chars().take(cols.saturating_sub(1)));
        match prompt_cursor {
            Some(x) => out.push_str(&format!("\x1b[{};{}H", self.rows, x.min(cols - 1) + 1)),
            None => out.push_str(&format!(
                "\x1b[{};{}H",
                self.row - self.top + 1,
                cursor_x - self.left + 1
            )),
        }
        out.push_str("\x1b[?25h");
        tty.write(out.as_bytes());
    }

    // ---- editing helpers -------------------------------------------------

    fn checkpoint(&mut self) {
        self.undo.push(Snapshot {
            lines: self.lines.clone(),
            row: self.row,
            col: self.col,
        });
        self.redo.clear();
        let mut total: usize = self.undo.iter().map(Snapshot::bytes).sum();
        while total > MAX_UNDO_BYTES && self.undo.len() > 1 {
            total -= self.undo.remove(0).bytes();
        }
    }

    fn restore(&mut self, from_undo: bool) {
        let (src, dst) = if from_undo {
            (&mut self.undo, &mut self.redo)
        } else {
            (&mut self.redo, &mut self.undo)
        };
        let Some(snap) = src.pop() else {
            self.status = if from_undo {
                "Already at oldest change"
            } else {
                "Already at newest change"
            }
            .to_string();
            return;
        };
        dst.push(Snapshot {
            lines: std::mem::take(&mut self.lines),
            row: self.row,
            col: self.col,
        });
        self.lines = snap.lines;
        self.row = snap.row.min(self.lines.len() - 1);
        self.col = snap.col;
        self.clamp_normal();
    }

    fn line(&self) -> &Vec<char> {
        &self.lines[self.row]
    }

    fn clamp_normal(&mut self) {
        let len = self.line().len();
        self.col = self.col.min(len.saturating_sub(1));
    }

    fn take_count(&mut self) -> usize {
        self.count.take().unwrap_or(1).max(1)
    }

    fn first_non_blank(&self) -> usize {
        self.line()
            .iter()
            .position(|c| !c.is_whitespace())
            .unwrap_or(0)
    }

    fn word_forward(&self, mut row: usize, mut col: usize) -> (usize, usize) {
        let class = |c: char| {
            if c.is_whitespace() {
                0
            } else if c.is_alphanumeric() || c == '_' {
                1
            } else {
                2
            }
        };
        let line = &self.lines[row];
        if col < line.len() {
            let start = class(line[col]);
            while col < line.len() && class(line[col]) == start && start != 0 {
                col += 1;
            }
        }
        loop {
            let line = &self.lines[row];
            while col < line.len() && line[col].is_whitespace() {
                col += 1;
            }
            if col < line.len() || row + 1 >= self.lines.len() {
                return (row, col.min(line.len()));
            }
            row += 1;
            col = 0;
            if self.lines[row].is_empty() {
                return (row, 0);
            }
        }
    }

    fn word_backward(&self, mut row: usize, mut col: usize) -> (usize, usize) {
        let class = |c: char| {
            if c.is_alphanumeric() || c == '_' {
                1
            } else {
                2
            }
        };
        loop {
            if col == 0 {
                if row == 0 {
                    return (0, 0);
                }
                row -= 1;
                col = self.lines[row].len();
                if col == 0 {
                    return (row, 0);
                }
            }
            let line = &self.lines[row];
            while col > 0 && line[col - 1].is_whitespace() {
                col -= 1;
            }
            if col == 0 {
                continue;
            }
            let c = class(line[col - 1]);
            while col > 0 && !line[col - 1].is_whitespace() && class(line[col - 1]) == c {
                col -= 1;
            }
            return (row, col);
        }
    }

    fn word_end(&self, mut row: usize, mut col: usize) -> (usize, usize) {
        let class = |c: char| {
            if c.is_alphanumeric() || c == '_' {
                1
            } else {
                2
            }
        };
        col += 1;
        loop {
            let line = &self.lines[row];
            while col < line.len() && line[col].is_whitespace() {
                col += 1;
            }
            if col < line.len() {
                let c = class(line[col]);
                while col + 1 < line.len()
                    && !line[col + 1].is_whitespace()
                    && class(line[col + 1]) == c
                {
                    col += 1;
                }
                return (row, col);
            }
            if row + 1 >= self.lines.len() {
                return (row, line.len().saturating_sub(1));
            }
            row += 1;
            col = 0;
        }
    }

    fn delete_lines(&mut self, n: usize) {
        let end = (self.row + n).min(self.lines.len());
        let removed: Vec<Vec<char>> = self.lines.drain(self.row..end).collect();
        self.yank = Some(Yank {
            text: removed,
            linewise: true,
        });
        if self.lines.is_empty() {
            self.lines.push(Vec::new());
        }
        self.row = self.row.min(self.lines.len() - 1);
        self.col = self.first_non_blank();
    }

    /// Delete `[col, end)` on the current line into the yank register.
    fn delete_range(&mut self, start: usize, end: usize) {
        let end = end.min(self.line().len());
        if start >= end {
            return;
        }
        let removed: Vec<char> = self.lines[self.row].drain(start..end).collect();
        self.yank = Some(Yank {
            text: vec![removed],
            linewise: false,
        });
        self.col = start;
    }

    fn insert_char(&mut self, c: char) {
        if self.buffer_bytes() + c.len_utf8() > MAX_BUFFER_BYTES {
            self.status = "E: buffer size limit reached".to_string();
            return;
        }
        let col = self.col.min(self.line().len());
        self.lines[self.row].insert(col, c);
        self.col = col + 1;
    }

    fn put(&mut self, after: bool) {
        let Some(yank) = self.yank.clone() else {
            self.status = "E353: Nothing in register".to_string();
            return;
        };
        let bytes: usize = yank.text.iter().map(|l| l.len() + 1).sum();
        if self.buffer_bytes() + bytes > MAX_BUFFER_BYTES {
            self.status = "E: buffer size limit reached".to_string();
            return;
        }
        self.checkpoint();
        if yank.linewise {
            let at = if after { self.row + 1 } else { self.row };
            for (i, line) in yank.text.into_iter().enumerate() {
                self.lines.insert(at + i, line);
            }
            self.row = at;
            self.col = self.first_non_blank();
        } else {
            let piece = &yank.text[0];
            let at = if after && !self.line().is_empty() {
                self.col + 1
            } else {
                self.col
            }
            .min(self.line().len());
            for (i, &c) in piece.iter().enumerate() {
                self.lines[self.row].insert(at + i, c);
            }
            self.col = (at + piece.len()).saturating_sub(1);
        }
    }

    // ---- normal mode -----------------------------------------------------

    fn normal_key(&mut self, key: Key) {
        self.status.clear();
        if let Some(op) = self.pending.take() {
            self.operator(op, key);
            return;
        }
        if let Key::Char(c @ '1'..='9') = key {
            self.count = Some(self.count.unwrap_or(0) * 10 + c.to_digit(10).unwrap_or(0) as usize);
            return;
        }
        if let (Key::Char('0'), Some(n)) = (key, self.count) {
            self.count = Some(n.saturating_mul(10));
            return;
        }
        let had_count = self.count.is_some();
        let n = self.take_count();
        match key {
            Key::Char('h') | Key::Left | Key::Backspace => self.col = self.col.saturating_sub(n),
            Key::Char('l') | Key::Right | Key::Char(' ') => {
                self.col += n;
                self.clamp_normal();
            }
            Key::Char('j') | Key::Down | Key::Ctrl(b'n') => {
                self.row = (self.row + n).min(self.lines.len() - 1);
                self.clamp_normal();
            }
            Key::Char('k') | Key::Up | Key::Ctrl(b'p') => {
                self.row = self.row.saturating_sub(n);
                self.clamp_normal();
            }
            Key::Enter | Key::Char('+') => {
                self.row = (self.row + n).min(self.lines.len() - 1);
                self.col = self.first_non_blank();
            }
            Key::Char('-') => {
                self.row = self.row.saturating_sub(n);
                self.col = self.first_non_blank();
            }
            Key::Char('0') | Key::Home => self.col = 0,
            Key::Char('^') => self.col = self.first_non_blank(),
            Key::Char('$') | Key::End => self.col = self.line().len().saturating_sub(1),
            Key::Char('w') => {
                for _ in 0..n {
                    (self.row, self.col) = self.word_forward(self.row, self.col);
                }
                self.clamp_normal();
            }
            Key::Char('b') => {
                for _ in 0..n {
                    (self.row, self.col) = self.word_backward(self.row, self.col);
                }
            }
            Key::Char('e') => {
                for _ in 0..n {
                    (self.row, self.col) = self.word_end(self.row, self.col);
                }
            }
            Key::Char('G') => {
                self.row = if had_count {
                    n.min(self.lines.len()) - 1
                } else {
                    self.lines.len() - 1
                };
                self.col = self.first_non_blank();
            }
            Key::Char('x') | Key::Delete if !self.line().is_empty() => {
                self.checkpoint();
                self.delete_range(self.col, self.col + n);
                self.clamp_normal();
            }
            Key::Char('X') if self.col > 0 => {
                self.checkpoint();
                let start = self.col.saturating_sub(n);
                self.delete_range(start, self.col);
            }
            Key::Char('D') => {
                self.checkpoint();
                self.delete_range(self.col, usize::MAX);
                self.clamp_normal();
            }
            Key::Char('C') => {
                self.checkpoint();
                self.delete_range(self.col, usize::MAX);
                self.mode = Mode::Insert;
            }
            Key::Char('s') => {
                self.checkpoint();
                self.delete_range(self.col, self.col + n);
                self.mode = Mode::Insert;
            }
            Key::Char('S') => {
                self.checkpoint();
                self.lines[self.row].clear();
                self.col = 0;
                self.mode = Mode::Insert;
            }
            Key::Char('J') if self.row + 1 < self.lines.len() => {
                self.checkpoint();
                for _ in 0..n.max(1) {
                    if self.row + 1 >= self.lines.len() {
                        break;
                    }
                    let next = self.lines.remove(self.row + 1);
                    let trimmed: Vec<char> =
                        next.into_iter().skip_while(|c| c.is_whitespace()).collect();
                    let line = &mut self.lines[self.row];
                    while line.last().is_some_and(|c| c.is_whitespace()) {
                        line.pop();
                    }
                    self.col = line.len();
                    if !trimmed.is_empty() && !line.is_empty() {
                        line.push(' ');
                    }
                    line.extend(trimmed);
                }
            }
            Key::Char('p') => {
                for _ in 0..n {
                    self.put(true);
                }
            }
            Key::Char('P') => {
                for _ in 0..n {
                    self.put(false);
                }
            }
            Key::Char('Y') => self.yank_lines(n),
            Key::Char('i') => self.begin_insert(self.col),
            Key::Char('a') => self.begin_insert((self.col + 1).min(self.line().len())),
            Key::Char('I') => self.begin_insert(self.first_non_blank()),
            Key::Char('A') => self.begin_insert(self.line().len()),
            Key::Char('o') | Key::Char('O') => {
                self.checkpoint();
                let at = if key == Key::Char('o') {
                    self.row + 1
                } else {
                    self.row
                };
                self.lines.insert(at, Vec::new());
                self.row = at;
                self.col = 0;
                self.mode = Mode::Insert;
            }
            Key::Char('u') => {
                for _ in 0..n {
                    self.restore(true);
                }
            }
            Key::Ctrl(b'r') => {
                for _ in 0..n {
                    self.restore(false);
                }
            }
            Key::Char('n') => self.search(None, true),
            Key::Char('N') => self.search(None, false),
            Key::Char(c @ (':' | '/')) => {
                self.prompt.clear();
                self.mode = Mode::Prompt(c);
            }
            Key::Char(c @ ('d' | 'c' | 'y' | 'g' | 'r' | 'Z')) => {
                self.pending = Some(c);
                if had_count {
                    self.count = Some(n);
                }
            }
            Key::Ctrl(b'g') => {
                self.status = format!(
                    "\"{}\"{} line {} of {}",
                    self.name(),
                    if self.is_dirty() { " [Modified]" } else { "" },
                    self.row + 1,
                    self.lines.len()
                );
            }
            Key::Ctrl(b'c') => {
                self.status = "Type  :q!  and press <Enter> to abandon changes".into()
            }
            _ => {}
        }
    }

    fn yank_lines(&mut self, n: usize) {
        let end = (self.row + n).min(self.lines.len());
        self.yank = Some(Yank {
            text: self.lines[self.row..end].to_vec(),
            linewise: true,
        });
    }

    fn begin_insert(&mut self, col: usize) {
        self.checkpoint();
        self.col = col;
        self.mode = Mode::Insert;
    }

    /// Second key of a two-key command (`dd`, `dw`, `cw`, `yy`, `gg`, `rX`, `ZZ`).
    fn operator(&mut self, op: char, key: Key) {
        let n = self.take_count();
        let Key::Char(c) = key else {
            return;
        };
        match (op, c) {
            ('g', 'g') => {
                self.row = 0;
                self.col = self.first_non_blank();
            }
            ('Z', 'Z') => self.pending_ex = Some("x".to_string()),
            ('Z', 'Q') => self.quit = true,
            ('r', ch) => {
                let len = self.line().len();
                if self.col + n <= len {
                    self.checkpoint();
                    for i in 0..n {
                        self.lines[self.row][self.col + i] = ch;
                    }
                    self.col += n - 1;
                }
            }
            ('d', 'd') => {
                self.checkpoint();
                self.delete_lines(n);
            }
            ('c', 'c') => {
                self.checkpoint();
                self.lines[self.row].clear();
                self.col = 0;
                self.mode = Mode::Insert;
            }
            ('y', 'y') => self.yank_lines(n),
            (op @ ('d' | 'c' | 'y'), motion @ ('w' | 'e' | '$' | '0' | 'b')) => {
                let (start, end) = match motion {
                    '$' => (self.col, self.line().len()),
                    '0' => (0, self.col),
                    'b' => {
                        let mut col = self.col;
                        for _ in 0..n {
                            col = self.word_backward(self.row, col).1;
                        }
                        (col, self.col)
                    }
                    _ => {
                        // `cw` behaves like `ce` (vi compatibility).
                        let mut col = self.col;
                        for _ in 0..n {
                            let (r, c2) = if motion == 'e' || op == 'c' {
                                let (r, c2) = self.word_end(self.row, col);
                                (r, c2 + 1)
                            } else {
                                self.word_forward(self.row, col)
                            };
                            col = if r == self.row { c2 } else { self.line().len() };
                        }
                        (self.col, col)
                    }
                };
                if op == 'y' {
                    let end = end.min(self.line().len());
                    if start < end {
                        self.yank = Some(Yank {
                            text: vec![self.line()[start..end].to_vec()],
                            linewise: false,
                        });
                    }
                    return;
                }
                self.checkpoint();
                self.delete_range(start, end);
                if op == 'c' {
                    self.mode = Mode::Insert;
                } else {
                    self.clamp_normal();
                }
            }
            _ => {}
        }
    }

    // ---- insert mode -----------------------------------------------------

    fn insert_key(&mut self, key: Key) {
        self.status.clear();
        match key {
            Key::Esc => {
                self.mode = Mode::Normal;
                self.col = self.col.saturating_sub(1);
                self.clamp_normal();
            }
            Key::Enter => {
                if self.buffer_bytes() + 1 > MAX_BUFFER_BYTES {
                    self.status = "E: buffer size limit reached".to_string();
                    return;
                }
                let col = self.col.min(self.line().len());
                let rest = self.lines[self.row].split_off(col);
                self.lines.insert(self.row + 1, rest);
                self.row += 1;
                self.col = 0;
            }
            Key::Backspace => {
                if self.col > 0 {
                    self.col -= 1;
                    self.lines[self.row].remove(self.col);
                } else if self.row > 0 {
                    let line = self.lines.remove(self.row);
                    self.row -= 1;
                    self.col = self.line().len();
                    self.lines[self.row].extend(line);
                }
            }
            Key::Delete if self.col < self.line().len() => {
                self.lines[self.row].remove(self.col);
            }
            Key::Left => self.col = self.col.saturating_sub(1),
            Key::Right => self.col = (self.col + 1).min(self.line().len()),
            Key::Up => {
                self.row = self.row.saturating_sub(1);
                self.col = self.col.min(self.line().len());
            }
            Key::Down => {
                self.row = (self.row + 1).min(self.lines.len() - 1);
                self.col = self.col.min(self.line().len());
            }
            Key::Home => self.col = 0,
            Key::End => self.col = self.line().len(),
            Key::Ctrl(b'w') => {
                let (_, start) = self.word_backward(self.row, self.col);
                self.lines[self.row].drain(start..self.col);
                self.col = start;
            }
            Key::Ctrl(b'i') => self.insert_char('\t'),
            Key::Char(c) => self.insert_char(c),
            _ => {}
        }
    }

    // ---- command line ----------------------------------------------------

    /// Edit the `:`/`/` line; returns the finished command on Enter.
    fn prompt_key(&mut self, key: Key) -> Option<String> {
        match key {
            Key::Enter => return Some(std::mem::take(&mut self.prompt)),
            Key::Esc | Key::Ctrl(b'c') => self.mode = Mode::Normal,
            Key::Backspace if self.prompt.pop().is_none() => self.mode = Mode::Normal,
            Key::Char(c) if self.prompt.len() < 4096 => self.prompt.push(c),
            _ => {}
        }
        None
    }

    fn search(&mut self, pattern: Option<String>, forward: bool) {
        if let Some(p) = pattern.filter(|p| !p.is_empty()) {
            self.last_search = Some(p);
        }
        let Some(pattern) = self.last_search.clone() else {
            self.status = "E35: No previous regular expression".into();
            return;
        };
        let re = match regex::RegexBuilder::new(&pattern)
            .size_limit(1 << 20)
            .build()
        {
            Ok(re) => re,
            Err(_) => {
                self.status = format!("E486: Invalid pattern: {pattern}");
                return;
            }
        };
        let total = self.lines.len();
        let matches_in = |line: &[char]| -> Vec<usize> {
            let s: String = line.iter().collect();
            re.find_iter(&s)
                .map(|m| s[..m.start()].chars().count())
                .collect()
        };
        for step in 0..=total {
            let row = if forward {
                (self.row + step) % total
            } else {
                (self.row + total - step % total) % total
            };
            let hits = matches_in(&self.lines[row]);
            let hit = if forward {
                hits.into_iter().find(|&c| step > 0 || c > self.col)
            } else {
                hits.into_iter().rev().find(|&c| step > 0 || c < self.col)
            };
            if let Some(col) = hit {
                self.row = row;
                self.col = col;
                return;
            }
        }
        self.status = format!("E486: Pattern not found: {pattern}");
    }

    async fn ex_command(&mut self, cmd: &str, ctx: &Context<'_>) {
        let cmd = cmd.trim();
        if let Ok(line) = cmd.parse::<usize>() {
            self.row = line.clamp(1, self.lines.len()) - 1;
            self.col = self.first_non_blank();
            return;
        }
        if cmd == "$" {
            self.row = self.lines.len() - 1;
            self.col = self.first_non_blank();
            return;
        }
        if let Some(sub) = cmd.strip_prefix("%s").or_else(|| cmd.strip_prefix("s"))
            && sub.starts_with(|c: char| !c.is_alphanumeric() && !c.is_whitespace())
        {
            self.substitute(sub, cmd.starts_with('%'));
            return;
        }
        let (name, arg) = match cmd.find(' ') {
            Some(i) => (&cmd[..i], cmd[i + 1..].trim()),
            None => (cmd, ""),
        };
        let arg = (!arg.is_empty()).then_some(arg);
        match name {
            "" => {}
            "q" | "quit" => {
                if self.is_dirty() {
                    self.status = "E37: No write since last change (add ! to override)".into();
                } else {
                    self.quit = true;
                }
            }
            "q!" | "quit!" | "cq" => self.quit = true,
            "w" | "w!" | "write" => {
                self.write(arg, ctx).await;
            }
            "wq" | "wq!" | "x" | "x!" | "xit" | "exit" => {
                let skip_write =
                    (name.starts_with('x') || name == "exit") && !self.is_dirty() && arg.is_none();
                if skip_write || self.write(arg, ctx).await {
                    self.quit = true;
                }
            }
            _ => self.status = format!("E492: Not an editor command: {cmd}"),
        }
    }

    fn substitute(&mut self, spec: &str, all_lines: bool) {
        let mut chars = spec.chars();
        let Some(delim) = chars.next() else {
            return;
        };
        let rest: String = chars.collect();
        let parts: Vec<&str> = rest.splitn(3, delim).collect();
        let (pattern, replacement, flags) = match parts.as_slice() {
            [p, r, f] => (*p, *r, *f),
            [p, r] => (*p, *r, ""),
            [p] => (*p, "", ""),
            _ => return,
        };
        let pattern = if pattern.is_empty() {
            match &self.last_search {
                Some(p) => p.clone(),
                None => {
                    self.status = "E35: No previous regular expression".into();
                    return;
                }
            }
        } else {
            pattern.to_string()
        };
        let re = match regex::RegexBuilder::new(&pattern)
            .size_limit(1 << 20)
            .build()
        {
            Ok(re) => re,
            Err(_) => {
                self.status = format!("E486: Invalid pattern: {pattern}");
                return;
            }
        };
        // vi uses `&` for the whole match; regex uses `$0`. Escape `$` first.
        let replacement = replacement
            .replace('$', "$$")
            .replace("\\&", "\u{0}")
            .replace('&', "${0}")
            .replace('\u{0}', "&");
        let global = flags.contains('g');
        let rows: Vec<usize> = if all_lines {
            (0..self.lines.len()).collect()
        } else {
            vec![self.row]
        };
        let mut changed = Vec::new();
        for &row in &rows {
            let s: String = self.lines[row].iter().collect();
            let out = if global {
                re.replace_all(&s, replacement.as_str())
            } else {
                re.replace(&s, replacement.as_str())
            };
            if out != s {
                changed.push((row, out.chars().collect::<Vec<char>>()));
            }
        }
        if changed.is_empty() {
            self.status = format!("E486: Pattern not found: {pattern}");
            return;
        }
        let growth: usize = changed.iter().map(|(_, l)| l.len()).sum();
        if self.buffer_bytes() + growth > MAX_BUFFER_BYTES {
            self.status = "E: buffer size limit reached".to_string();
            return;
        }
        self.checkpoint();
        let last = changed.last().map(|(r, _)| *r).unwrap_or(self.row);
        for (row, line) in changed {
            self.lines[row] = line;
        }
        self.row = last;
        self.col = self.first_non_blank();
        self.last_search = Some(pattern);
    }

    /// Write the buffer; returns whether it succeeded.
    async fn write(&mut self, arg: Option<&str>, ctx: &Context<'_>) -> bool {
        let path = match (arg, &self.path) {
            (Some(name), _) => {
                let p = resolve_path(ctx.cwd, name);
                if self.path.is_none() {
                    self.path = Some(p.clone());
                    self.display_name = Some(name.to_string());
                }
                p
            }
            (None, Some(p)) => p.clone(),
            (None, None) => {
                self.status = "E32: No file name".into();
                return false;
            }
        };
        let text = self.text();
        match ctx.fs.write_file(&path, text.as_bytes()).await {
            Ok(()) => {
                if Some(&path) == self.path.as_ref() {
                    self.saved_hash = self.buffer_hash();
                }
                self.status = format!(
                    "\"{}\" {}L, {}B written",
                    arg.map(str::to_string).unwrap_or_else(|| self.name()),
                    text.matches('\n').count(),
                    text.len()
                );
                true
            }
            Err(e) => {
                self.status = format!("E212: Can't open file for writing: {e}");
                false
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::Bash;
    use crate::terminal::{Terminal, TerminalStatus};

    async fn session() -> Terminal {
        let mut term = Terminal::new(Bash::builder());
        term.run_until_idle().await;
        term
    }

    async fn keys(term: &mut Terminal, input: &str) {
        term.send(input);
        assert_eq!(term.run_until_idle().await, TerminalStatus::Idle);
    }

    async fn file(term: &Terminal, path: &str) -> String {
        String::from_utf8(term.fs().read_file(path.as_ref()).await.unwrap()).unwrap()
    }

    async fn edit(initial: Option<&str>, input: &str) -> String {
        let mut term = session().await;
        if let Some(text) = initial {
            term.fs()
                .write_file("/tmp/f.txt".as_ref(), text.as_bytes())
                .await
                .unwrap();
        }
        keys(&mut term, "vi /tmp/f.txt\r").await;
        keys(&mut term, input).await;
        assert!(
            !term.is_alternate_screen(),
            "vi still open: {}",
            term.screen_text()
        );
        file(&term, "/tmp/f.txt").await
    }

    #[tokio::test]
    async fn new_file_insert_and_write_quit() {
        assert_eq!(edit(None, "ihello\rworld\x1b:wq\r").await, "hello\nworld\n");
    }

    #[tokio::test]
    async fn screen_shows_buffer_and_status() {
        let mut term = session().await;
        term.fs()
            .write_file("/tmp/a.txt".as_ref(), b"one\ntwo\n")
            .await
            .unwrap();
        keys(&mut term, "vi /tmp/a.txt\r").await;
        assert!(term.is_alternate_screen());
        let text = term.screen_text();
        assert!(text.starts_with("one\ntwo\n~\n"), "{text}");
        assert!(text.ends_with("\"/tmp/a.txt\" 2L, 8B"), "{text}");
        keys(&mut term, "i").await;
        assert!(term.screen_text().ends_with("-- INSERT --"));
        keys(&mut term, "\x1b:q\r").await;
        // Leaving restores the shell screen.
        assert!(
            term.screen_text().contains("$ vi /tmp/a.txt"),
            "{}",
            term.screen_text()
        );
    }

    #[tokio::test]
    async fn dirty_quit_is_refused_until_forced() {
        let mut term = session().await;
        keys(&mut term, "vi /tmp/x.txt\r").await;
        keys(&mut term, "iabc\x1b:q\r").await;
        assert!(term.is_alternate_screen());
        assert!(term.screen_text().contains("E37"));
        keys(&mut term, ":q!\r").await;
        assert!(!term.is_alternate_screen());
        assert!(term.fs().read_file("/tmp/x.txt".as_ref()).await.is_err());
    }

    #[tokio::test]
    async fn delete_yank_put_lines() {
        let out = edit(Some("a\nb\nc\n"), "ddp:wq\r").await;
        assert_eq!(out, "b\na\nc\n");
        let out = edit(Some("a\nb\nc\n"), "2yyGp:wq\r").await;
        assert_eq!(out, "a\nb\nc\na\nb\n");
        let out = edit(Some("a\nb\nc\n"), "2dd:wq\r").await;
        assert_eq!(out, "c\n");
    }

    #[tokio::test]
    async fn word_motions_and_operators() {
        assert_eq!(edit(Some("foo bar baz\n"), "wdw:wq\r").await, "foo baz\n");
        assert_eq!(
            edit(Some("foo bar baz\n"), "wcwqux\x1b:wq\r").await,
            "foo qux baz\n"
        );
        assert_eq!(edit(Some("foo bar baz\n"), "wD:wq\r").await, "foo \n");
        assert_eq!(edit(Some("foo bar\n"), "$xbx:wq\r").await, "foo a\n");
        assert_eq!(edit(Some("abc\n"), "rZ:wq\r").await, "Zbc\n");
    }

    #[tokio::test]
    async fn open_line_append_and_join() {
        assert_eq!(edit(Some("a\nc\n"), "ob\x1b:wq\r").await, "a\nb\nc\n");
        assert_eq!(edit(Some("a\nc\n"), "jOb\x1b:wq\r").await, "a\nb\nc\n");
        assert_eq!(edit(Some("ab\n"), "A!\x1bI>\x1b:wq\r").await, ">ab!\n");
        assert_eq!(edit(Some("a\n  b\n"), "J:wq\r").await, "a b\n");
    }

    #[tokio::test]
    async fn undo_and_redo() {
        assert_eq!(edit(Some("abc\n"), "xxu:wq\r").await, "bc\n");
        assert_eq!(edit(Some("abc\n"), "xxuu\x12:wq\r").await, "bc\n");
        assert_eq!(edit(Some("x\n"), "ione two\x1bu:wq\r").await, "x\n");
    }

    #[tokio::test]
    async fn search_and_goto() {
        assert_eq!(
            edit(Some("a\nneedle\nb\nneedle\n"), "/needle\rnx:wq\r").await,
            "a\nneedle\nb\needle\n"
        );
        assert_eq!(edit(Some("1\n2\n3\n"), ":2\rdd:wq\r").await, "1\n3\n");
        assert_eq!(edit(Some("1\n2\n3\n"), "Gdd:wq\r").await, "1\n2\n");
        assert_eq!(edit(Some("1\n2\n3\n"), "G2Gdd:wq\r").await, "1\n3\n");
        assert_eq!(edit(Some("1\n2\n3\n"), "Gggdd:wq\r").await, "2\n3\n");
    }

    #[tokio::test]
    async fn substitute_current_line_and_global() {
        assert_eq!(edit(Some("aa\naa\n"), ":s/a/b/\r:wq\r").await, "ba\naa\n");
        assert_eq!(edit(Some("aa\naa\n"), ":%s/a/b/g\r:wq\r").await, "bb\nbb\n");
        assert_eq!(edit(Some("x\n"), ":s/x/<&>/\r:wq\r").await, "<x>\n");
        assert_eq!(edit(Some("x\n"), ":s/x/$1/\r:wq\r").await, "$1\n");
    }

    #[tokio::test]
    async fn arrow_keys_and_backspace_in_insert_mode() {
        assert_eq!(
            edit(Some("ac\n"), "a\x1b[Cb\x7f\x1b[Db\x1b:wq\r").await,
            "abc\n"
        );
    }

    #[tokio::test]
    async fn zz_writes_and_quits() {
        assert_eq!(edit(Some("a\n"), "ib\x1bZZ").await, "ba\n");
    }

    #[tokio::test]
    async fn write_as_and_unicode() {
        let mut term = session().await;
        keys(&mut term, "vi\r").await;
        keys(&mut term, "iпривіт ✓\x1b:w /tmp/u.txt\r:q\r").await;
        assert!(!term.is_alternate_screen());
        assert_eq!(file(&term, "/tmp/u.txt").await, "привіт ✓\n");
    }

    #[tokio::test]
    async fn result_is_visible_to_later_commands() {
        let mut term = session().await;
        keys(&mut term, "cd /tmp && vi notes.md\r").await;
        keys(&mut term, "i# Title\rbody\x1b:wq\r").await;
        keys(&mut term, "cat notes.md | wc -l\r").await;
        assert!(
            term.screen_text().ends_with("2\n$"),
            "{}",
            term.screen_text()
        );
    }

    #[tokio::test]
    async fn long_files_scroll() {
        let body: String = (1..=100).map(|i| format!("line{i}\n")).collect();
        let mut term = session().await;
        term.fs()
            .write_file("/tmp/long.txt".as_ref(), body.as_bytes())
            .await
            .unwrap();
        keys(&mut term, "vi /tmp/long.txt\r").await;
        keys(&mut term, "G").await;
        let text = term.screen_text();
        assert!(text.contains("line100"), "{text}");
        assert!(!text.contains("line1\n"), "{text}");
        keys(&mut term, ":q\r").await;
    }

    #[tokio::test]
    async fn without_terminal_fails_cleanly() {
        let mut bash = Bash::new();
        let r = bash.exec("vi /tmp/x").await.unwrap();
        assert_eq!(r.exit_code, 1);
        assert!(r.stderr.contains("not a terminal"));
    }

    #[tokio::test]
    async fn directory_and_bad_options_are_rejected() {
        let mut term = session().await;
        keys(&mut term, "vi /tmp; echo rc=$?\r").await;
        assert!(term.screen_text().contains("is a directory"));
        assert!(term.screen_text().contains("rc=1"));
        keys(&mut term, "vi -R x; echo rc=$?\r").await;
        assert!(term.screen_text().contains("rc=2"));
    }

    #[tokio::test]
    async fn readonly_vfs_write_error_keeps_editor_open() {
        let mut term = Terminal::new(Bash::builder().readonly_filesystem(true));
        term.run_until_idle().await;
        keys(&mut term, "vi /tmp/r.txt\r").await;
        keys(&mut term, "ix\x1b:w\r").await;
        assert!(term.is_alternate_screen());
        assert!(
            term.screen_text().contains("E212"),
            "{}",
            term.screen_text()
        );
        keys(&mut term, ":q!\r").await;
    }

    #[tokio::test]
    async fn buffer_cap_stops_inserts() {
        let mut ed = super::Editor::new(24, 80);
        ed.lines = vec![vec!['a'; super::MAX_BUFFER_BYTES - 1]];
        ed.insert_char('b');
        assert!(ed.status.contains("limit"));
        assert_eq!(ed.buffer_bytes(), super::MAX_BUFFER_BYTES);
    }
}
