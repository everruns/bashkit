//! Interactive `less` / `more` for the in-process terminal.
//!
//! Important decisions:
//! - Interactive only when a [`Terminal`](crate::terminal::Terminal) hands the
//!   command its device (the `Tty` execution extension). Plain `exec()`,
//!   `BashTool` and the CLI never install one, so there `less`/`more` stay
//!   cat-like and never wait for keys. This keeps non-terminal callers safe.
//! - Content is read up front (files from the VFS, or pipeline stdin), so
//!   memory is bounded by the same VFS/stdin limits as `cat`.
//! - Control characters in content render in caret notation (`^[`), like
//!   `less` without `-R`, so a file cannot drive the host terminal with
//!   escape sequences.
//! - `less` uses the alternate screen; `more` scrolls on the normal screen
//!   and leaves what it showed behind, like the real tools.

use async_trait::async_trait;

use super::{Builtin, Context, resolve_path};
use crate::error::{Error, Result};
use crate::interpreter::ExecResult;
use crate::terminal::{Key, ScreenGuard, Tty, read_key};

const TAB_STOP: usize = 8;

/// The `more` builtin. Pages inside a terminal session, behaves like `cat`
/// everywhere else.
pub struct More;

#[async_trait]
impl Builtin for More {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        if ctx.args.iter().any(|a| a == "--help") {
            return Ok(ExecResult::ok(
                "Usage: more [FILE]...\nPage through text one screenful at a time.\n\nOutside a terminal session, more behaves like cat.\n",
            ));
        }
        match terminal(&ctx)? {
            Some(tty) => page(&ctx, &tty, Pager::More).await,
            None => super::Less.execute(ctx).await,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Pager {
    Less,
    More,
}

/// The terminal device, if this command runs inside a `Terminal` session.
pub(crate) fn terminal(ctx: &Context<'_>) -> Result<Option<Tty>> {
    match ctx.execution_extension::<Tty>() {
        Some(cap) => Ok(Some(
            cap.try_with(Clone::clone).map_err(|_| Error::Cancelled)?,
        )),
        None => Ok(None),
    }
}

/// Page `ctx`'s files (or stdin) on the terminal.
pub(crate) async fn page(ctx: &Context<'_>, tty: &Tty, kind: Pager) -> Result<ExecResult> {
    let name = if kind == Pager::Less { "less" } else { "more" };
    let quit_if_one_screen = kind == Pager::More
        || ctx
            .args
            .iter()
            .any(|a| a.starts_with('-') && !a.starts_with("--") && a.contains('F'));
    let files: Vec<&String> = ctx.args.iter().filter(|a| !a.starts_with('-')).collect();

    let mut text = String::new();
    let mut title = String::new();
    if files.is_empty() {
        match ctx.stdin_text_lossy() {
            Some(stdin) => text.push_str(&stdin),
            None => {
                return Ok(ExecResult::err(format!("{name}: missing filename\n"), 1));
            }
        }
    } else {
        for file in &files {
            let path = resolve_path(ctx.cwd, file);
            match ctx.fs.stat(&path).await {
                Ok(meta) if meta.file_type.is_dir() => {
                    return Ok(ExecResult::err(format!("{file}: Is a directory\n"), 1));
                }
                Ok(_) => {}
                Err(_) => {
                    return Ok(ExecResult::err(
                        format!("{file}: No such file or directory\n"),
                        1,
                    ));
                }
            }
            let bytes = ctx.fs.read_file(&path).await?;
            if files.len() > 1 {
                text.push_str(&format!("::::::::::::::\n{file}\n::::::::::::::\n"));
            }
            text.push_str(&String::from_utf8_lossy(&bytes));
            if !text.is_empty() && !text.ends_with('\n') {
                text.push('\n');
            }
        }
        title = files[0].to_string();
    }
    page_text(tty, &text, title, kind, quit_if_one_screen).await
}

/// Page `text` on the terminal (used by `less`, `more` and `man`).
pub(crate) async fn page_text(
    tty: &Tty,
    text: &str,
    title: String,
    kind: Pager,
    quit_if_one_screen: bool,
) -> Result<ExecResult> {
    let mut view = View::new(text, title, tty.size().cols as usize);
    let page_rows = (tty.size().rows as usize).saturating_sub(1).max(1);
    if quit_if_one_screen && view.rows.len() <= page_rows {
        for row in &view.rows {
            tty.write(row.as_bytes());
            tty.write(b"\r\n");
        }
        return Ok(ExecResult::ok(""));
    }

    match kind {
        Pager::Less => {
            let _screen = ScreenGuard::enter(tty, true);
            view.run_less(tty).await;
        }
        Pager::More => {
            let _screen = ScreenGuard::enter(tty, false);
            view.run_more(tty).await;
        }
    }
    Ok(ExecResult::ok(""))
}

/// Expand tabs, render control chars in caret notation, wrap at `cols`.
fn wrap_lines(lines: &[String], cols: usize) -> (Vec<String>, Vec<usize>) {
    let mut rows = Vec::new();
    let mut row_line = Vec::new();
    for (idx, line) in lines.iter().enumerate() {
        let mut row = String::new();
        let mut width = 0;
        let mut push = |s: &str, w: usize, row: &mut String, width: &mut usize| {
            if *width + w > cols && *width > 0 {
                rows.push(std::mem::take(row));
                row_line.push(idx);
                *width = 0;
            }
            row.push_str(s);
            *width += w;
        };
        for c in line.chars() {
            match c {
                '\t' => {
                    let n = TAB_STOP - width % TAB_STOP;
                    push(&" ".repeat(n), n, &mut row, &mut width);
                }
                c if (c as u32) < 0x20 || c == '\x7f' => {
                    let caret = format!("^{}", ((c as u8) ^ 0x40) as char);
                    push(&caret, 2, &mut row, &mut width);
                }
                c if c.is_control() => push("?", 1, &mut row, &mut width),
                c => {
                    let mut buf = [0u8; 4];
                    push(c.encode_utf8(&mut buf), 1, &mut row, &mut width);
                }
            }
        }
        rows.push(row);
        row_line.push(idx);
    }
    (rows, row_line)
}

struct View {
    lines: Vec<String>,
    /// Display rows after wrapping, with the logical line each came from.
    rows: Vec<String>,
    row_line: Vec<usize>,
    cols: usize,
    top: usize,
    title: String,
    message: Option<String>,
    last_search: Option<(String, bool)>,
}

impl View {
    fn new(text: &str, title: String, cols: usize) -> Self {
        let body = text.strip_suffix('\n').unwrap_or(text);
        let lines = if text.is_empty() {
            Vec::new()
        } else {
            body.split('\n')
                .map(|l| l.strip_suffix('\r').unwrap_or(l).to_string())
                .collect()
        };
        let mut view = Self {
            lines,
            rows: Vec::new(),
            row_line: Vec::new(),
            cols: cols.max(1),
            top: 0,
            title,
            message: None,
            last_search: None,
        };
        view.wrap();
        view
    }

    fn wrap(&mut self) {
        (self.rows, self.row_line) = wrap_lines(&self.lines, self.cols);
    }

    fn resize(&mut self, tty: &Tty) {
        let line = self.row_line.get(self.top).copied().unwrap_or(0);
        self.cols = (tty.size().cols as usize).max(1);
        self.wrap();
        self.top = self.row_line.iter().position(|&l| l == line).unwrap_or(0);
    }

    fn page_rows(tty: &Tty) -> usize {
        (tty.size().rows as usize).saturating_sub(1).max(1)
    }

    fn max_top(&self, page: usize) -> usize {
        self.rows.len().saturating_sub(page)
    }

    // ---- less ------------------------------------------------------------

    fn render_less(&mut self, tty: &Tty, first: bool) {
        let page = Self::page_rows(tty);
        let mut out = String::from("\x1b[?25l");
        for r in 0..page {
            out.push_str(&format!("\x1b[{};1H\x1b[2K", r + 1));
            match self.rows.get(self.top + r) {
                Some(row) => out.push_str(row),
                None => out.push('~'),
            }
        }
        out.push_str(&format!("\x1b[{};1H\x1b[2K", page + 1));
        let at_end = self.top + page >= self.rows.len();
        match self.message.take() {
            Some(msg) => out.push_str(&format!("\x1b[7m{msg}\x1b[0m")),
            None if at_end => out.push_str("\x1b[7m(END)\x1b[0m"),
            None if first && !self.title.is_empty() => {
                out.push_str(&format!("\x1b[7m{}\x1b[0m", self.title))
            }
            None => out.push(':'),
        }
        out.push_str("\x1b[?25h");
        tty.write(out.as_bytes());
    }

    async fn run_less(&mut self, tty: &Tty) {
        self.render_less(tty, true);
        loop {
            let Some(key) = read_key(tty).await else {
                return;
            };
            let page = Self::page_rows(tty);
            let max_top = self.max_top(page);
            match key {
                Err(()) => self.resize(tty),
                Ok(Key::Char('q' | 'Q')) => return,
                Ok(Key::Char(' ' | 'f') | Key::Ctrl(b'f' | b'v') | Key::PageDown) => {
                    self.top = (self.top + page).min(max_top);
                }
                Ok(Key::Char('b') | Key::Ctrl(b'b') | Key::PageUp) => {
                    self.top = self.top.saturating_sub(page);
                }
                Ok(Key::Char('j' | 'e') | Key::Enter | Key::Down | Key::Ctrl(b'n' | b'e')) => {
                    self.top = (self.top + 1).min(max_top);
                }
                Ok(Key::Char('k' | 'y') | Key::Up | Key::Ctrl(b'p' | b'y')) => {
                    self.top = self.top.saturating_sub(1);
                }
                Ok(Key::Char('d') | Key::Ctrl(b'd')) => {
                    self.top = (self.top + page / 2).min(max_top);
                }
                Ok(Key::Char('u') | Key::Ctrl(b'u')) => {
                    self.top = self.top.saturating_sub(page / 2);
                }
                Ok(Key::Char('g' | '<') | Key::Home) => self.top = 0,
                Ok(Key::Char('G' | '>') | Key::End) => self.top = max_top,
                Ok(Key::Char(c @ ('/' | '?'))) => {
                    let Some(pattern) = self.prompt(tty, c, page).await else {
                        return;
                    };
                    if !pattern.is_empty() {
                        self.last_search = Some((pattern, c == '/'));
                    }
                    self.search(false, page);
                }
                Ok(Key::Char('n')) => self.search(false, page),
                Ok(Key::Char('N')) => self.search(true, page),
                _ => {}
            }
            self.render_less(tty, false);
        }
    }

    /// Read a `/` or `?` search line on the status row. `None` if the
    /// terminal closed.
    async fn prompt(&mut self, tty: &Tty, lead: char, page: usize) -> Option<String> {
        let mut input = String::new();
        loop {
            tty.write(format!("\x1b[{};1H\x1b[2K{lead}{input}", page + 1).as_bytes());
            match read_key(tty).await? {
                Ok(Key::Enter) => return Some(input),
                Ok(Key::Esc | Key::Ctrl(b'c')) => return Some(String::new()),
                Ok(Key::Backspace) if input.pop().is_none() => return Some(String::new()),
                Ok(Key::Char(c)) if input.len() < 1024 => input.push(c),
                _ => {}
            }
        }
    }

    /// Move to the next match of the last search (`reverse` flips direction).
    fn search(&mut self, reverse: bool, page: usize) {
        let Some((pattern, forward)) = self.last_search.clone() else {
            self.message = Some("No previous regular expression".into());
            return;
        };
        let re = match regex::RegexBuilder::new(&pattern)
            .size_limit(1 << 20)
            .build()
        {
            Ok(re) => re,
            Err(_) => {
                self.message = Some(format!("Invalid pattern: {pattern}"));
                return;
            }
        };
        let forward = forward != reverse;
        let current = self.row_line.get(self.top).copied().unwrap_or(0);
        let hit = if forward {
            (current + 1..self.lines.len()).find(|&l| re.is_match(&self.lines[l]))
        } else {
            (0..current).rev().find(|&l| re.is_match(&self.lines[l]))
        };
        match hit {
            Some(line) => {
                let row = self.row_line.iter().position(|&l| l == line).unwrap_or(0);
                self.top = row.min(self.max_top(page));
            }
            None => self.message = Some("Pattern not found".into()),
        }
    }

    // ---- more ------------------------------------------------------------

    async fn run_more(&mut self, tty: &Tty) {
        let mut shown = 0;
        let mut step = Self::page_rows(tty);
        loop {
            let end = (shown + step).min(self.rows.len());
            for row in &self.rows[shown..end] {
                tty.write(row.as_bytes());
                tty.write(b"\r\n");
            }
            shown = end;
            if shown >= self.rows.len() {
                return;
            }
            let pct = shown * 100 / self.rows.len().max(1);
            tty.write(format!("\x1b[7m--More--({pct}%)\x1b[0m").as_bytes());
            let next = loop {
                match read_key(tty).await {
                    None | Some(Ok(Key::Char('q' | 'Q'))) => {
                        tty.write(b"\r\x1b[K");
                        return;
                    }
                    Some(Ok(Key::Char(' ' | 'f') | Key::PageDown)) => {
                        break Self::page_rows(tty);
                    }
                    Some(Ok(Key::Enter | Key::Char('j') | Key::Down)) => break 1,
                    Some(Ok(Key::Char('d') | Key::Ctrl(b'd'))) => {
                        break (Self::page_rows(tty) / 2).max(1);
                    }
                    _ => {}
                }
            };
            tty.write(b"\r\x1b[K");
            step = next;
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::Bash;
    use crate::terminal::{Terminal, TerminalSize, TerminalStatus};

    async fn session(rows: u16) -> Terminal {
        let mut term = Terminal::with_size(Bash::builder(), TerminalSize::new(rows, 40));
        term.run_until_idle().await;
        let body: String = (1..=50).map(|i| format!("line{i}\n")).collect();
        term.fs()
            .write_file("/tmp/long.txt".as_ref(), body.as_bytes())
            .await
            .unwrap();
        term
    }

    async fn keys(term: &mut Terminal, input: &str) -> String {
        term.send(input);
        assert_eq!(term.run_until_idle().await, TerminalStatus::Idle);
        term.screen_text()
    }

    #[tokio::test]
    async fn less_pages_and_quits() {
        let mut term = session(10).await;
        let screen = keys(&mut term, "less /tmp/long.txt\r").await;
        assert!(term.is_alternate_screen());
        assert!(screen.starts_with("line1\n"), "{screen}");
        assert!(screen.ends_with("/tmp/long.txt"), "{screen}");
        let screen = keys(&mut term, " ").await;
        assert!(screen.starts_with("line10\n"), "{screen}");
        assert!(screen.ends_with(':'), "{screen}");
        let screen = keys(&mut term, "G").await;
        assert!(screen.contains("line50\n(END)"), "{screen}");
        let screen = keys(&mut term, "gj").await;
        assert!(screen.starts_with("line2\n"), "{screen}");
        let screen = keys(&mut term, "q").await;
        assert!(!term.is_alternate_screen());
        assert!(screen.ends_with("$ less /tmp/long.txt\n$"), "{screen}");
    }

    #[tokio::test]
    async fn less_searches() {
        let mut term = session(10).await;
        keys(&mut term, "less /tmp/long.txt\r").await;
        let screen = keys(&mut term, "/line3[0-9]\r").await;
        assert!(screen.starts_with("line30\n"), "{screen}");
        let screen = keys(&mut term, "n").await;
        assert!(screen.starts_with("line31\n"), "{screen}");
        let screen = keys(&mut term, "N").await;
        assert!(screen.starts_with("line30\n"), "{screen}");
        let screen = keys(&mut term, "/nomatch\r").await;
        assert!(screen.ends_with("Pattern not found"), "{screen}");
        keys(&mut term, "q").await;
    }

    #[tokio::test]
    async fn less_reads_pipeline_stdin() {
        let mut term = session(10).await;
        let screen = keys(&mut term, "seq 1 100 | less\r").await;
        assert!(screen.starts_with("1\n2\n"), "{screen}");
        keys(&mut term, "q").await;
    }

    #[tokio::test]
    async fn less_dash_f_prints_short_input() {
        let mut term = session(10).await;
        let screen = keys(&mut term, "echo short | less -F; echo done\r").await;
        assert!(!term.is_alternate_screen());
        assert!(screen.contains("\nshort\ndone"), "{screen}");
    }

    #[tokio::test]
    async fn control_characters_are_escaped() {
        let mut term = session(10).await;
        let screen = keys(&mut term, "printf 'a\\033[2Jb\\n' | less\r").await;
        assert!(screen.starts_with("a^[[2Jb"), "{screen}");
        keys(&mut term, "q").await;
    }

    #[tokio::test]
    async fn more_scrolls_on_normal_screen() {
        let mut term = session(10).await;
        let screen = keys(&mut term, "more /tmp/long.txt\r").await;
        assert!(!term.is_alternate_screen());
        assert!(screen.ends_with("line9\n--More--(18%)"), "{screen}");
        let screen = keys(&mut term, "\r").await;
        assert!(screen.ends_with("line10\n--More--(20%)"), "{screen}");
        let screen = keys(&mut term, "q").await;
        assert!(screen.ends_with("line10\n$"), "{screen}");
    }

    #[tokio::test]
    async fn more_runs_to_end_and_returns_to_prompt() {
        let mut term = session(30).await;
        let screen = keys(&mut term, "more /tmp/long.txt\r").await;
        assert!(screen.contains("--More--"), "{screen}");
        let screen = keys(&mut term, " ").await;
        assert!(screen.ends_with("line50\n$"), "{screen}");
    }

    #[tokio::test]
    async fn more_prints_short_files_directly() {
        let mut term = session(10).await;
        let screen = keys(&mut term, "echo hi | more; echo after\r").await;
        assert!(screen.contains("\nhi\nafter"), "{screen}");
    }

    #[tokio::test]
    async fn pagers_report_missing_files() {
        let mut term = session(10).await;
        let screen = keys(&mut term, "less /nope; echo rc=$?\r").await;
        assert!(screen.contains("No such file or directory"), "{screen}");
        assert!(screen.contains("rc=1"), "{screen}");
        let screen = keys(&mut term, "less; echo rc=$?\r").await;
        assert!(screen.contains("missing filename"), "{screen}");
    }

    /// Outside a terminal session the pagers must never wait for input.
    #[tokio::test]
    async fn pagers_are_cat_like_without_terminal() {
        let mut bash = Bash::new();
        bash.exec("seq 1 500 > /tmp/n").await.unwrap();
        for cmd in [
            "less /tmp/n",
            "more /tmp/n",
            "seq 1 500 | less",
            "seq 1 500 | more",
        ] {
            let r = tokio::time::timeout(std::time::Duration::from_secs(5), bash.exec(cmd))
                .await
                .unwrap_or_else(|_| panic!("{cmd} blocked"))
                .unwrap();
            assert_eq!(r.exit_code, 0, "{cmd}");
            assert_eq!(r.stdout.lines().count(), 500, "{cmd}");
        }
        let r = bash.exec("vi /tmp/n").await.unwrap();
        assert_eq!(r.exit_code, 1);
    }
}
