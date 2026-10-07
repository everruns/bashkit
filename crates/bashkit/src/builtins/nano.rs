//! `nano` builtin: a small modeless text editor for the in-process terminal.
//!
//! Only usable inside a [`Terminal`](crate::terminal::Terminal) session, like
//! `vi`: it reads keys from the terminal device in raw mode and draws on the
//! alternate screen.
//!
//! Important decisions:
//! - A useful subset of GNU nano: typing inserts, arrows/Home/End/PageUp/
//!   PageDown move, `^O` write out (asks for the name), `^X` exit (asks to
//!   save a modified buffer), `^K`/`^U` cut and paste lines, `^W` search,
//!   `^G` help, `^C` cursor position. No multiple buffers, syntax colors,
//!   undo, mouse, or rc files; unknown keys are ignored.
//! - Same bounds and VFS rules as `vi` (8 MiB buffer, writes through the
//!   session VFS). Tabs render to 8-column stops; one char is one cell.
//! - Exits 0 whether or not the buffer was saved, like GNU nano.

use std::path::PathBuf;

use async_trait::async_trait;

use super::{Builtin, Context, resolve_path};
use crate::error::{Error, Result};
use crate::interpreter::ExecResult;
use crate::terminal::{Key, ScreenGuard, Tty, read_key};

// THREAT[TM-DOS-119]: bounded editor memory, same cap as vi.
const MAX_BUFFER_BYTES: usize = 8 * 1024 * 1024;
/// Cap on the cut buffer (`^K` accumulates lines).
const MAX_CUT_BYTES: usize = 1024 * 1024;
const TAB_STOP: usize = 8;
const HELP_ROW_1: &str = "^G Help  ^O Write Out  ^W Where Is  ^K Cut  ^C Location";
const HELP_ROW_2: &str = "^X Exit  ^U Paste";

/// The nano builtin.
pub struct Nano;

#[async_trait]
impl Builtin for Nano {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let mut file: Option<&String> = None;
        for arg in ctx.args {
            if arg == "--help" {
                return Ok(ExecResult::ok(
                    "usage: nano [FILE]\nModeless text editor; needs an interactive terminal.\n\
                     ^O write out, ^X exit, ^K/^U cut/paste line, ^W search, ^G help.\n",
                ));
            }
            if arg.starts_with('-') || arg.starts_with('+') {
                return Ok(ExecResult::err(
                    format!("nano: unsupported option: {arg}\n"),
                    2,
                ));
            }
            if file.is_some() {
                return Ok(ExecResult::err("nano: only one file is supported\n", 2));
            }
            file = Some(arg);
        }
        let Some(tty) = ctx.execution_extension::<Tty>() else {
            return Ok(ExecResult::err(
                "nano: not a terminal (run inside bashkit::terminal::Terminal)\n",
                1,
            ));
        };
        let tty = tty.try_with(Clone::clone).map_err(|_| Error::Cancelled)?;
        let path = file.map(|f| resolve_path(ctx.cwd, f));
        run_on(&tty, &ctx, path, file.cloned()).await
    }
}

/// Edit `path` (or an unnamed buffer) on `tty` until the user exits.
pub(crate) async fn run_on(
    tty: &Tty,
    ctx: &Context<'_>,
    path: Option<PathBuf>,
    display_name: Option<String>,
) -> Result<ExecResult> {
    let size = tty.size();
    let mut ed = Editor::new(size.rows, size.cols);
    ed.display_name = display_name;
    if let Some(path) = &path {
        match ctx.fs.stat(path).await {
            Ok(meta) if meta.file_type.is_dir() => {
                return Ok(ExecResult::err(
                    format!("nano: {}: is a directory\n", path.display()),
                    1,
                ));
            }
            Ok(meta) if meta.size as usize > MAX_BUFFER_BYTES => {
                return Ok(ExecResult::err(
                    format!("nano: {}: file too large\n", path.display()),
                    1,
                ));
            }
            Ok(_) => {
                let bytes = ctx.fs.read_file(path).await?;
                ed.load(&String::from_utf8_lossy(&bytes));
                ed.status = format!("[ Read {} lines ]", ed.lines.len());
            }
            Err(_) => ed.status = "[ New File ]".into(),
        }
    }
    ed.path = path;
    let _screen = ScreenGuard::enter(tty, true);
    ed.run(tty, ctx).await;
    Ok(ExecResult::ok(""))
}

/// What a bottom-line prompt is asking for.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Ask {
    /// File name for `^O` (or `^X` then Y); `exit` quits after writing.
    FileName {
        exit: bool,
    },
    /// "Save modified buffer?" from `^X`.
    SaveOnExit,
    Search,
}

struct Editor {
    lines: Vec<Vec<char>>,
    row: usize,
    col: usize,
    top: usize,
    left: usize,
    rows: u16,
    cols: u16,
    path: Option<PathBuf>,
    display_name: Option<String>,
    modified: bool,
    status: String,
    cut: Vec<Vec<char>>,
    /// Consecutive `^K` presses append to the cut buffer.
    last_was_cut: bool,
    ask: Option<(Ask, String)>,
    last_search: String,
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
            path: None,
            display_name: None,
            modified: false,
            status: String::new(),
            cut: Vec::new(),
            last_was_cut: false,
            ask: None,
            last_search: String::new(),
            quit: false,
        }
    }

    fn load(&mut self, text: &str) {
        let text = text.strip_suffix('\n').unwrap_or(text);
        self.lines = text.split('\n').map(|l| l.chars().collect()).collect();
        if self.lines.is_empty() {
            self.lines.push(Vec::new());
        }
    }

    fn text(&self) -> String {
        let mut out = String::new();
        for line in &self.lines {
            out.extend(line.iter());
            out.push('\n');
        }
        out
    }

    fn bytes(&self) -> usize {
        self.lines.iter().map(|l| l.len() + 1).sum()
    }

    fn name(&self) -> String {
        self.display_name
            .clone()
            .unwrap_or_else(|| "New Buffer".into())
    }

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
            if self.ask.is_some() {
                self.ask_key(key, ctx).await;
            } else {
                self.edit_key(key);
            }
            if !self.quit {
                self.render(tty);
            }
        }
    }

    fn edit_key(&mut self, key: Key) {
        let was_cut = std::mem::take(&mut self.last_was_cut);
        match key {
            Key::Char(c) => self.insert(c),
            Key::Ctrl(b'i') => self.insert('\t'),
            Key::Enter => self.newline(),
            Key::Backspace => self.backspace(),
            Key::Delete | Key::Ctrl(b'd') => self.delete(),
            Key::Left | Key::Ctrl(b'b') => self.left(),
            Key::Right | Key::Ctrl(b'f') => self.right(),
            Key::Up | Key::Ctrl(b'p') => self.vertical(-1),
            Key::Down | Key::Ctrl(b'n') => self.vertical(1),
            Key::Home | Key::Ctrl(b'a') => self.col = 0,
            Key::End | Key::Ctrl(b'e') => self.col = self.lines[self.row].len(),
            Key::PageUp | Key::Ctrl(b'y') => self.vertical(-(self.text_rows() as isize)),
            Key::PageDown | Key::Ctrl(b'v') => self.vertical(self.text_rows() as isize),
            Key::Ctrl(b'k') => self.cut_line(was_cut),
            Key::Ctrl(b'u') => self.paste(),
            Key::Ctrl(b'o') => {
                self.ask = Some((Ask::FileName { exit: false }, self.default_name()));
            }
            Key::Ctrl(b'x') => {
                if self.modified {
                    self.ask = Some((Ask::SaveOnExit, String::new()));
                } else {
                    self.quit = true;
                }
            }
            Key::Ctrl(b'w') => self.ask = Some((Ask::Search, String::new())),
            Key::Ctrl(b'g') => {
                self.status = "^O save  ^X exit  ^K cut line  ^U paste  ^W search  \
                               arrows move"
                    .into();
            }
            Key::Ctrl(b'c') => {
                self.status = format!(
                    "[ line {}/{}, col {}/{} ]",
                    self.row + 1,
                    self.lines.len(),
                    self.col + 1,
                    self.lines[self.row].len() + 1
                );
            }
            _ => {}
        }
    }

    fn default_name(&self) -> String {
        self.display_name.clone().unwrap_or_default()
    }

    async fn ask_key(&mut self, key: Key, ctx: &Context<'_>) {
        let Some((ask, mut input)) = self.ask.take() else {
            return;
        };
        if ask == Ask::SaveOnExit {
            match key {
                Key::Char('y' | 'Y') => {
                    self.ask = Some((Ask::FileName { exit: true }, self.default_name()));
                }
                Key::Char('n' | 'N') => self.quit = true,
                Key::Ctrl(b'c') => self.status = "[ Cancelled ]".into(),
                _ => self.ask = Some((ask, input)),
            }
            return;
        }
        match key {
            Key::Enter => match ask {
                Ask::FileName { exit } => {
                    if input.is_empty() {
                        self.status = "[ Cancelled ]".into();
                    } else if self.write(&input, ctx).await && exit {
                        self.quit = true;
                    }
                }
                Ask::Search => {
                    if !input.is_empty() {
                        self.last_search = input;
                    }
                    self.search();
                }
                Ask::SaveOnExit => {}
            },
            Key::Ctrl(b'c') => self.status = "[ Cancelled ]".into(),
            Key::Backspace => {
                input.pop();
                self.ask = Some((ask, input));
            }
            Key::Char(c) if input.len() < 4096 => {
                input.push(c);
                self.ask = Some((ask, input));
            }
            _ => self.ask = Some((ask, input)),
        }
    }

    async fn write(&mut self, name: &str, ctx: &Context<'_>) -> bool {
        let path = resolve_path(ctx.cwd, name);
        let text = self.text();
        match ctx.fs.write_file(&path, text.as_bytes()).await {
            Ok(()) => {
                self.path = Some(path);
                self.display_name = Some(name.to_string());
                self.modified = false;
                self.status = format!("[ Wrote {} lines ]", self.lines.len());
                true
            }
            Err(e) => {
                self.status = format!("[ Error writing {name}: {e} ]");
                false
            }
        }
    }

    fn search(&mut self) {
        let needle: Vec<char> = self.last_search.chars().collect();
        if needle.is_empty() {
            return;
        }
        let n = self.lines.len();
        for step in 0..=n {
            let r = (self.row + step) % n;
            let from = if step == 0 { self.col + 1 } else { 0 };
            let line = &self.lines[r];
            if let Some(pos) = (from..line.len().saturating_sub(needle.len() - 1))
                .find(|&i| line[i..].starts_with(&needle))
            {
                self.row = r;
                self.col = pos;
                if step == n {
                    self.status = "[ This is the only occurrence ]".into();
                }
                return;
            }
        }
        self.status = format!("[ \"{}\" not found ]", self.last_search);
    }

    // ---- editing ---------------------------------------------------------

    fn room(&mut self, extra: usize) -> bool {
        if self.bytes() + extra > MAX_BUFFER_BYTES {
            self.status = "[ Buffer is full ]".into();
            return false;
        }
        true
    }

    fn insert(&mut self, c: char) {
        if !self.room(c.len_utf8()) {
            return;
        }
        self.lines[self.row].insert(self.col, c);
        self.col += 1;
        self.modified = true;
    }

    fn newline(&mut self) {
        if !self.room(1) {
            return;
        }
        let rest = self.lines[self.row].split_off(self.col);
        self.lines.insert(self.row + 1, rest);
        self.row += 1;
        self.col = 0;
        self.modified = true;
    }

    fn backspace(&mut self) {
        if self.col > 0 {
            self.col -= 1;
            self.lines[self.row].remove(self.col);
            self.modified = true;
        } else if self.row > 0 {
            let line = self.lines.remove(self.row);
            self.row -= 1;
            self.col = self.lines[self.row].len();
            self.lines[self.row].extend(line);
            self.modified = true;
        }
    }

    fn delete(&mut self) {
        if self.col < self.lines[self.row].len() {
            self.lines[self.row].remove(self.col);
            self.modified = true;
        } else if self.row + 1 < self.lines.len() {
            let next = self.lines.remove(self.row + 1);
            self.lines[self.row].extend(next);
            self.modified = true;
        }
    }

    fn left(&mut self) {
        if self.col > 0 {
            self.col -= 1;
        } else if self.row > 0 {
            self.row -= 1;
            self.col = self.lines[self.row].len();
        }
    }

    fn right(&mut self) {
        if self.col < self.lines[self.row].len() {
            self.col += 1;
        } else if self.row + 1 < self.lines.len() {
            self.row += 1;
            self.col = 0;
        }
    }

    fn vertical(&mut self, delta: isize) {
        let max = self.lines.len() as isize - 1;
        self.row = (self.row as isize + delta).clamp(0, max) as usize;
        self.col = self.col.min(self.lines[self.row].len());
    }

    fn cut_line(&mut self, append: bool) {
        if !append {
            self.cut.clear();
        }
        let cut_bytes: usize = self.cut.iter().map(|l| l.len() + 1).sum();
        if cut_bytes + self.lines[self.row].len() < MAX_CUT_BYTES {
            let line = if self.lines.len() == 1 {
                std::mem::take(&mut self.lines[0])
            } else {
                self.lines.remove(self.row)
            };
            self.cut.push(line);
            self.row = self.row.min(self.lines.len() - 1);
            self.col = 0;
            self.modified = true;
        }
        self.last_was_cut = true;
    }

    fn paste(&mut self) {
        let extra: usize = self.cut.iter().map(|l| l.len() + 1).sum();
        if self.cut.is_empty() || !self.room(extra) {
            return;
        }
        for (i, line) in self.cut.clone().into_iter().enumerate() {
            self.lines.insert(self.row + i, line);
        }
        self.row += self.cut.len();
        self.col = 0;
        self.modified = true;
    }

    // ---- rendering -------------------------------------------------------

    /// Rows for text: everything but the title, status and two help rows.
    fn text_rows(&self) -> usize {
        usize::from(self.rows).saturating_sub(4).max(1)
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
        let cols = usize::from(self.cols);
        let text_rows = self.text_rows();
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

        let mut out = String::from("\x1b[?25l\x1b[H");
        // Title bar, reverse video.
        let title = format!(
            "  bashkit nano  {}{}",
            self.name(),
            if self.modified { "  Modified" } else { "" }
        );
        out.push_str("\x1b[7m");
        out.extend(format!("{title:<cols$}").chars().take(cols));
        out.push_str("\x1b[0m\r\n");
        for i in 0..text_rows {
            out.push_str("\x1b[2K");
            if let Some(line) = self.lines.get(self.top + i) {
                out.push_str(&visible(line, self.left, cols));
            }
            out.push_str("\r\n");
        }
        out.push_str("\x1b[2K");
        match &self.ask {
            Some((ask, input)) => {
                let label = match ask {
                    Ask::FileName { .. } => "File Name to Write: ",
                    Ask::SaveOnExit => "Save modified buffer?  Y Yes  N No  ^C Cancel",
                    Ask::Search => "Search: ",
                };
                out.push_str("\x1b[7m");
                out.extend(format!("{label}{input}").chars().take(cols));
                out.push_str("\x1b[0m");
            }
            None => out.extend(self.status.chars().take(cols)),
        }
        out.push_str("\r\n\x1b[2K");
        out.extend(HELP_ROW_1.chars().take(cols));
        out.push_str("\r\n\x1b[2K");
        out.extend(HELP_ROW_2.chars().take(cols));
        let (cy, cx) = match &self.ask {
            Some((Ask::SaveOnExit, _)) | None => {
                (self.row - self.top + 2, cursor_x - self.left + 1)
            }
            Some((ask, input)) => {
                let label = if *ask == Ask::Search {
                    "Search: "
                } else {
                    "File Name to Write: "
                };
                (
                    text_rows + 2,
                    (label.chars().count() + input.chars().count() + 1).min(cols),
                )
            }
        };
        out.push_str(&format!("\x1b[{cy};{cx}H\x1b[?25h"));
        tty.write(out.as_bytes());
        if self.ask.is_none() {
            self.status.clear();
        }
    }
}

/// The part of `line` from display column `left`, `cols` wide, with tabs
/// expanded and control chars in caret notation.
fn visible(line: &[char], left: usize, cols: usize) -> String {
    let mut cells = String::new();
    let mut width = 0;
    for &c in line {
        let mut cell = String::new();
        if c == '\t' {
            let next = (width / TAB_STOP + 1) * TAB_STOP;
            cell.extend(std::iter::repeat_n(' ', next - width));
        } else if (c as u32) < 0x20 || c == '\x7f' {
            cell.push('^');
            cell.push(((c as u8) ^ 0x40) as char);
        } else {
            cell.push(c);
        }
        for ch in cell.chars() {
            if width >= left && width < left + cols {
                cells.push(ch);
            }
            width += 1;
        }
        if width >= left + cols {
            break;
        }
    }
    cells
}

#[cfg(test)]
mod tests {
    use crate::Bash;
    use crate::terminal::Terminal;

    async fn run(term: &mut Terminal, input: &str) {
        term.send(input);
        term.run_until_idle().await;
    }

    async fn read(term: &Terminal, path: &str) -> String {
        let bytes = term.fs().read_file(path.as_ref()).await.unwrap();
        String::from_utf8(bytes).unwrap()
    }

    #[tokio::test]
    async fn edits_saves_and_exits() {
        let mut term = Terminal::new(Bash::builder());
        run(&mut term, "printf 'one\\ntwo\\n' > /tmp/f; nano /tmp/f\r").await;
        assert!(term.is_alternate_screen());
        let text = term.screen_text();
        assert!(text.contains("bashkit nano  /tmp/f"), "{text}");
        assert!(text.contains("^X Exit"), "{text}");
        // Type at the start of line 1, go to line 2's end, add a line.
        run(&mut term, "> \x1b[B\x1b[F\rthree").await;
        assert!(term.screen_text().contains("Modified"));
        // ^O asks for the name (pre-filled), Enter writes.
        run(&mut term, "\x0f\r").await;
        assert_eq!(read(&term, "/tmp/f").await, "> one\ntwo\nthree\n");
        run(&mut term, "\x18").await;
        assert!(!term.is_alternate_screen());
    }

    #[tokio::test]
    async fn exit_asks_to_save_and_cut_paste_moves_lines() {
        let mut term = Terminal::new(Bash::builder());
        run(
            &mut term,
            "printf 'a\\nb\\nc\\nd\\n' > /tmp/g; nano /tmp/g; echo rc=$?\r",
        )
        .await;
        // ^K cuts "a" and "b" (consecutive cuts append), move down, ^U pastes.
        run(&mut term, "\x0b\x0b\x1b[B\x15").await;
        run(&mut term, "\x18").await;
        assert!(term.screen_text().contains("Save modified buffer?"));
        run(&mut term, "y\r").await;
        assert!(!term.is_alternate_screen());
        assert!(
            term.screen_text().ends_with("rc=0\n$"),
            "{}",
            term.screen_text()
        );
        assert_eq!(read(&term, "/tmp/g").await, "c\na\nb\nd\n");
    }

    #[tokio::test]
    async fn exit_without_saving_and_new_file() {
        let mut term = Terminal::new(Bash::builder());
        run(&mut term, "nano\r").await;
        run(&mut term, "draft\x18n").await;
        assert!(!term.is_alternate_screen());
        // New named file: ^X, Y, name prompt is pre-filled.
        run(&mut term, "nano /tmp/new.txt\r").await;
        assert!(term.screen_text().contains("[ New File ]"));
        run(&mut term, "hi\x18y\r").await;
        assert_eq!(read(&term, "/tmp/new.txt").await, "hi\n");
    }

    #[tokio::test]
    async fn search_and_position() {
        let mut term = Terminal::new(Bash::builder());
        run(&mut term, "printf 'x\\nfind me\\n' > /tmp/s; nano /tmp/s\r").await;
        run(&mut term, "\x17me\r\x03").await;
        assert!(
            term.screen_text().contains("[ line 2/2, col 6/8 ]"),
            "{}",
            term.screen_text()
        );
    }

    #[tokio::test]
    async fn needs_a_terminal() {
        let mut bash = Bash::new();
        let r = bash.exec("nano /tmp/x").await.unwrap();
        assert_eq!(r.exit_code, 1);
        assert!(r.stderr.contains("not a terminal"));
    }
}
