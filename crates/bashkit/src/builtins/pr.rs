//! pr builtin - paginate or columnate files for printing
//!
//! Important decisions:
//! - Written from GNU `pr`'s observable behavior (no GNU code; bashkit is
//!   MIT). Page = 5-line header, body, 5-line trailer (66 lines by default);
//!   `-t`, `-T` or a page length of 10 or less drop header and trailer.
//! - Multi-column output (`-COLUMN`, `-m`) matches GNU byte for byte:
//!   column width is `(W - number field - (cols-1) * sep) / cols`, text is
//!   truncated to it, and blank runs of two or more (and all padding) are
//!   written as tabs where they reach a tab stop. Down-filled last pages are
//!   balanced, the first columns taking the extra lines.
//! - Header date is the file's mtime (now for stdin and `-m`) in the
//!   sandbox clock and `TZ`, so it is never host data.
//! - Output is capped at `MAX_OUTPUT`: padding can multiply input size.
//! - Not implemented (L-PR-001): `-e`/`-i` tab conversion options; `-c` and
//!   `-v` are accepted and ignored (no control-character display).

use async_trait::async_trait;

use super::{Builtin, Context, Date, resolve_path};
use crate::error::Result;
use crate::interpreter::ExecResult;

/// The pr builtin. Holds the sandbox clock for header dates.
pub struct Pr {
    clock: Date,
}

impl Pr {
    /// pr whose header dates use `clock` (shared with `date`).
    pub fn with_clock(clock: Date) -> Self {
        Self { clock }
    }
}

const USAGE: &str = "Usage: pr [OPTION]... [FILE]...\nPaginate or columnate FILE(s) for printing.\n\nWith no FILE, or when FILE is -, read standard input.\n\n  +FIRST_PAGE[:LAST_PAGE]  begin [stop] printing with page FIRST_[LAST_]PAGE\n  -COLUMN, --columns=COLUMN  output COLUMN columns, down then across\n  -a, --across              print columns across rather than down\n  -d, --double-space        double space the output\n  -D, --date-format=FORMAT  use FORMAT for the header date\n  -F, -f, --form-feed       use form feeds instead of newlines to separate pages\n  -h, --header=HEADER       use HEADER instead of the file name in the header\n  -J, --join-lines          merge full lines, turn off truncation\n  -l, --length=PAGE_LENGTH  set the page length (default 66 lines)\n  -m, --merge               print all files in parallel, one in each column\n  -n, --number-lines[=SEP[DIGITS]]  number lines (default SEP TAB, DIGITS 5)\n  -N, --first-line-number=NUMBER  start counting with NUMBER\n  -o, --indent=MARGIN       offset each line with MARGIN spaces\n  -s[CHAR], --separator[=CHAR]  separate columns by CHAR (default TAB)\n  -S[STRING], --sep-string[=STRING]  separate columns by STRING\n  -t, --omit-header         omit page headers and trailers\n  -T, --omit-pagination     omit headers, trailers and form feeds\n  -w, --width=PAGE_WIDTH    page width for multi-column output (default 72)\n  -W, --page-width=PAGE_WIDTH  always truncate lines to PAGE_WIDTH\n      --help                display this help and exit\n      --version             output version information and exit\n";

const PAGE_LEN: usize = 66;
const PAGE_WIDTH: usize = 72;
const HEADER_LINES: usize = 5;
const TRAILER_LINES: usize = 5;
/// Bound on `-l`, `-w`, `-COLUMN`, `-o`, `-n DIGITS`, so a hostile option
/// cannot make a tiny input expand without limit.
const MAX_DIM: usize = 10_000;
const TAB: usize = 8;
/// Output cap. Padding can multiply input size (`-w 10000 -2` on empty
/// lines), so stop and fail past this instead of building it (TM-DOS-*).
const MAX_OUTPUT: usize = 16 * 1024 * 1024;

/// A column entry: line text and its line number (with `-n`).
type Cell = Option<(String, Option<i64>)>;

struct Opts {
    columns: usize,
    across: bool,
    double: bool,
    date_format: String,
    form_feed: bool,
    header: Option<String>,
    join: bool,
    page_len: usize,
    merge: bool,
    number: Option<(char, usize)>,
    first_number: i64,
    offset: usize,
    /// `-s`: separator without padding unless `-w` is set.
    sep_char: Option<String>,
    /// `-S`: separator string, padding kept.
    sep_string: Option<String>,
    omit_header: bool,
    width: Option<usize>,
    page_width: Option<usize>,
    first_page: usize,
    last_page: Option<usize>,
}

/// Usage error (GNU adds the `--help` hint).
fn err(msg: impl std::fmt::Display) -> ExecResult {
    ExecResult::err(
        format!("pr: {msg}\nTry 'pr --help' for more information.\n"),
        1,
    )
}

/// Bad option value (GNU prints no hint).
fn bad_value(msg: impl std::fmt::Display) -> ExecResult {
    ExecResult::err(format!("pr: {msg}\n"), 1)
}

const LINES: &str = "'-l PAGE_LENGTH' invalid number of lines";
const COLUMNS: &str = "invalid number of columns";
const WIDTH: &str = "'-w PAGE_WIDTH' invalid number of characters";
const PAGE_WIDTH_MSG: &str = "'-W PAGE_WIDTH' invalid number of characters";
const OFFSET: &str = "'-o MARGIN' invalid line offset";
const FIRST_NUMBER: &str = "'-N NUMBER' invalid starting line number";

#[allow(clippy::result_large_err)]
/// Parse a count in `min..=MAX_DIM`, with GNU's two message shapes.
fn parse_num(what: &str, v: &str, min: usize) -> std::result::Result<usize, ExecResult> {
    let numeric = v
        .strip_prefix('-')
        .unwrap_or(v)
        .bytes()
        .all(|b| b.is_ascii_digit())
        && !v.trim_start_matches('-').is_empty();
    match v.parse::<usize>() {
        Ok(n) if (min..=MAX_DIM).contains(&n) => Ok(n),
        _ if numeric => Err(bad_value(format_args!(
            "{what}: '{v}': Numerical result out of range"
        ))),
        _ => Err(bad_value(format_args!("{what}: '{v}'"))),
    }
}

#[allow(clippy::result_large_err)]
/// `-n[SEP[DIGITS]]`: a non-digit first char is the separator.
fn parse_number_spec(spec: &str) -> std::result::Result<(char, usize), ExecResult> {
    let mut chars = spec.chars();
    let (sep, rest) = match chars.clone().next() {
        Some(c) if !c.is_ascii_digit() => {
            chars.next();
            (c, chars.as_str())
        }
        _ => ('\t', spec),
    };
    let digits = if rest.is_empty() {
        5
    } else {
        match rest.parse::<usize>() {
            Ok(d) if (1..=MAX_DIM).contains(&d) => d,
            _ => {
                let bad = rest.trim_start_matches(|c: char| c.is_ascii_digit());
                let bad = if bad.is_empty() { rest } else { bad };
                return Err(err(format_args!(
                    "'-n' extra characters or invalid number in the argument: '{bad}'"
                )));
            }
        }
    };
    Ok((sep, digits))
}

#[allow(clippy::result_large_err)]
fn parse_args(args: &[String]) -> std::result::Result<(Opts, Vec<String>), ExecResult> {
    let mut o = Opts {
        columns: 1,
        across: false,
        double: false,
        date_format: "%Y-%m-%d %H:%M".to_string(),
        form_feed: false,
        header: None,
        join: false,
        page_len: PAGE_LEN,
        merge: false,
        number: None,
        first_number: 1,
        offset: 0,
        sep_char: None,
        sep_string: None,
        omit_header: false,
        width: None,
        page_width: None,
        first_page: 1,
        last_page: None,
    };
    let mut files = Vec::new();
    let mut i = 0;
    let mut only_files = false;
    while i < args.len() {
        let a = args[i].as_str();
        i += 1;
        // `+0` is not a page number; GNU reads it as a file name.
        if only_files || a == "-" || a == "+0" || !(a.starts_with('-') || a.starts_with('+')) {
            files.push(a.to_string());
            continue;
        }
        if a == "--" {
            only_files = true;
            continue;
        }
        if let Some(pages) = a.strip_prefix('+') {
            let (first, last) = match pages.split_once(':') {
                Some((f, l)) => (f, Some(l)),
                None => (pages, None),
            };
            let page = |v: &str| {
                v.parse::<usize>()
                    .ok()
                    .filter(|n| *n >= 1)
                    .ok_or_else(|| bad_value(format_args!("invalid + argument '{pages}'")))
            };
            o.first_page = page(first)?;
            if let Some(l) = last {
                o.last_page = Some(page(l)?);
            }
            continue;
        }
        if let Some(long) = a.strip_prefix("--") {
            let (name, inline) = match long.split_once('=') {
                Some((n, v)) => (n, Some(v.to_string())),
                None => (long, None),
            };
            let mut required = || -> std::result::Result<String, ExecResult> {
                if let Some(v) = inline.clone() {
                    return Ok(v);
                }
                let v = args
                    .get(i)
                    .cloned()
                    .ok_or_else(|| err(format_args!("option '--{name}' requires an argument")))?;
                i += 1;
                Ok(v)
            };
            match name {
                "columns" => o.columns = parse_num(COLUMNS, &required()?, 1)?,
                "across" => o.across = true,
                "double-space" => o.double = true,
                "date-format" => o.date_format = required()?,
                "form-feed" => o.form_feed = true,
                "header" => o.header = Some(required()?),
                "join-lines" => o.join = true,
                "length" => o.page_len = parse_num(LINES, &required()?, 0)?,
                "merge" => o.merge = true,
                "number-lines" => {
                    o.number = Some(parse_number_spec(inline.as_deref().unwrap_or(""))?)
                }
                "first-line-number" => {
                    let v = required()?;
                    o.first_number = v
                        .parse()
                        .map_err(|_| bad_value(format_args!("{FIRST_NUMBER}: '{v}'")))?;
                }
                "indent" => o.offset = parse_num(OFFSET, &required()?, 0)?,
                "separator" => o.sep_char = Some(inline.clone().unwrap_or_else(|| "\t".into())),
                "sep-string" => o.sep_string = Some(inline.clone().unwrap_or_default()),
                "omit-header" => o.omit_header = true,
                "omit-pagination" => {
                    o.omit_header = true;
                    o.form_feed = false;
                }
                "width" => o.width = Some(parse_num(WIDTH, &required()?, 1)?),
                "page-width" => o.page_width = Some(parse_num(PAGE_WIDTH_MSG, &required()?, 1)?),
                "no-file-warnings" => {}
                _ => return Err(err(format_args!("unrecognized option '{a}'"))),
            }
            continue;
        }
        // Short options. `-COLUMN` digits first.
        let body = &a[1..];
        if body.bytes().all(|b| b.is_ascii_digit()) {
            o.columns = parse_num(COLUMNS, body, 1)?;
            continue;
        }
        let bytes = body.as_bytes();
        let mut j = 0;
        while j < bytes.len() {
            let f = bytes[j] as char;
            let rest = &body[j + 1..];
            // Value taken from the rest of this word, or the next word.
            let value = |i: &mut usize| -> std::result::Result<String, ExecResult> {
                if !rest.is_empty() {
                    return Ok(rest.to_string());
                }
                let v = args
                    .get(*i)
                    .cloned()
                    .ok_or_else(|| err(format_args!("option requires an argument -- '{f}'")))?;
                *i += 1;
                Ok(v)
            };
            match f {
                'a' => o.across = true,
                'd' => o.double = true,
                'F' | 'f' => o.form_feed = true,
                'J' => o.join = true,
                'm' => o.merge = true,
                't' => o.omit_header = true,
                'T' => {
                    o.omit_header = true;
                    o.form_feed = false;
                }
                'r' | 'c' | 'v' => {}
                // Optional-argument options consume the rest of the word.
                'n' => {
                    o.number = Some(parse_number_spec(rest)?);
                    break;
                }
                's' => {
                    o.sep_char = Some(if rest.is_empty() {
                        "\t".into()
                    } else {
                        rest.into()
                    });
                    break;
                }
                'S' => {
                    o.sep_string = Some(rest.to_string());
                    break;
                }
                'D' => {
                    o.date_format = value(&mut i)?;
                    break;
                }
                'h' => {
                    o.header = Some(value(&mut i)?);
                    break;
                }
                'l' => {
                    o.page_len = parse_num(LINES, &value(&mut i)?, 0)?;
                    break;
                }
                'N' => {
                    let v = value(&mut i)?;
                    o.first_number = v
                        .parse()
                        .map_err(|_| bad_value(format_args!("{FIRST_NUMBER}: '{v}'")))?;
                    break;
                }
                'o' => {
                    o.offset = parse_num(OFFSET, &value(&mut i)?, 0)?;
                    break;
                }
                'w' => {
                    o.width = Some(parse_num(WIDTH, &value(&mut i)?, 1)?);
                    break;
                }
                'W' => {
                    o.page_width = Some(parse_num(PAGE_WIDTH_MSG, &value(&mut i)?, 1)?);
                    break;
                }
                c if c.is_ascii_digit() => {
                    let digits: String =
                        body[j..].chars().take_while(char::is_ascii_digit).collect();
                    o.columns = parse_num(COLUMNS, &digits, 1)?;
                    j += digits.len();
                    continue;
                }
                _ => return Err(err(format_args!("invalid option -- '{f}'"))),
            }
            j += 1;
        }
    }
    if files.is_empty() {
        files.push("-".to_string());
    }
    if o.merge {
        o.columns = files.len();
    }
    Ok((o, files))
}

/// One input: its lines and the header title/date.
struct Input {
    lines: Vec<String>,
    title: String,
    date: String,
}

/// Expand tabs to spaces, columns counted from `start`.
fn expand_tabs(s: &str, start: usize) -> String {
    let mut out = String::with_capacity(s.len());
    let mut col = start;
    for ch in s.chars() {
        if ch == '\t' {
            let next = (col / TAB + 1) * TAB;
            out.extend(std::iter::repeat_n(' ', next - col));
            col = next;
        } else {
            out.push(ch);
            col += 1;
        }
    }
    out
}

/// Line writer tracking the output column, writing blank runs as tabs
/// where they reach a tab stop (GNU multi-column output).
struct Row {
    out: String,
    col: usize,
}

impl Row {
    fn new(offset: usize) -> Self {
        Row {
            out: " ".repeat(offset),
            col: offset,
        }
    }

    /// Blanks up to column `target`, as tabs then spaces.
    fn pad_to(&mut self, target: usize) {
        while self.col < target {
            let next = (self.col / TAB + 1) * TAB;
            if next <= target {
                // Even a single blank up to a tab stop is written as a tab.
                self.out.push('\t');
                self.col = next;
            } else {
                self.out.push(' ');
                self.col += 1;
            }
        }
    }

    /// A cell's text (tabs already expanded). Runs of two or more blanks
    /// become tabs where they reach a stop; a single blank stays a space;
    /// trailing blanks are left to the next padding run.
    fn cell(&mut self, text: &str) {
        let mut run = 0;
        for ch in text.chars() {
            if ch == ' ' {
                run += 1;
                continue;
            }
            if run == 1 {
                self.out.push(' ');
                self.col += 1;
            } else if run > 1 {
                let target = self.col + run;
                self.pad_to(target);
            }
            run = 0;
            self.out.push(ch);
            self.col += 1;
        }
    }

    /// Text written as is (separators, single-column lines).
    fn raw(&mut self, text: &str) {
        self.out.push_str(text);
        self.col += text.chars().count();
    }
}

struct Layout<'a> {
    o: &'a Opts,
    /// Per-column text width; None means no truncation and no padding.
    col_width: Option<usize>,
    sep: String,
    /// `-m -n`: width of the line number field ahead of the first column
    /// (a TAB separator counts up to the next tab stop).
    lead: usize,
}

impl Layout<'_> {
    fn number(&self, n: i64) -> String {
        match self.o.number {
            Some((sep, digits)) => {
                let s = n.to_string();
                let s = if s.len() > digits {
                    s[s.len() - digits..].to_string()
                } else {
                    format!("{s:>digits$}")
                };
                format!("{s}{sep}")
            }
            None => String::new(),
        }
    }

    /// Render one row of cells (None = absent cell, Some("") = empty cell).
    fn render(&self, cells: &[Cell]) -> String {
        let o = self.o;
        let mut row = Row::new(o.offset);
        if o.columns <= 1 {
            if let Some(Some((text, n))) = cells.first() {
                if let Some(n) = n {
                    row.raw(&self.number(*n));
                }
                let mut line = text.clone();
                if let Some(w) = o.page_width {
                    let room = w.saturating_sub(row.col - o.offset);
                    line = line.chars().take(room).collect();
                }
                row.raw(&line);
            }
            return row.out;
        }
        let last = cells.iter().rposition(Option::is_some);
        let Some(last) = last else {
            return row.out;
        };
        for (i, cell) in cells.iter().enumerate().take(last + 1) {
            if i > 0 {
                match self.col_width {
                    Some(_) => {
                        // Separator: a blank (part of the padding run) or the
                        // -S/-s string after padding to the column width.
                        let start = o.offset + self.col_offset(i);
                        if self.sep == " " {
                            row.pad_to(start);
                        } else {
                            row.pad_to(start - self.sep_len());
                            row.raw(&self.sep);
                        }
                    }
                    None => row.raw(&self.sep),
                }
            }
            let Some((text, n)) = cell else {
                continue;
            };
            let col_start = row.col;
            let mut content = String::new();
            if let Some(n) = n {
                content.push_str(&self.number(*n));
            }
            content.push_str(text);
            let rel = col_start - o.offset - self.col_offset(i);
            let mut expanded = expand_tabs(&content, rel);
            if let Some(w) = self.col_width {
                let w = if i == 0 { w + self.lead } else { w };
                expanded = expanded.chars().take(w.saturating_sub(rel)).collect();
            }
            row.cell(&expanded);
        }
        row.out
    }

    fn sep_len(&self) -> usize {
        self.sep.chars().count()
    }

    /// Absolute start of column `i` relative to the offset, for tab expansion.
    fn col_offset(&self, i: usize) -> usize {
        match self.col_width {
            Some(_) if i == 0 => 0,
            Some(w) => self.lead + i * (w + self.sep_len()),
            None => 0,
        }
    }
}

impl Pr {
    fn header_line(o: &Opts, date: &str, title: &str, page: usize) -> String {
        let width = o.page_width.or(o.width).unwrap_or(PAGE_WIDTH);
        let right = format!("Page {page}");
        let used = date.chars().count() + right.len();
        let tlen = title.chars().count();
        let avail = width.saturating_sub(used);
        let (lpad, rpad) = if tlen + 2 <= avail {
            let l = (avail - tlen) / 2;
            (l, avail - tlen - l)
        } else {
            (1, 1)
        };
        format!(
            "{}{date}{}{title}{}{right}",
            " ".repeat(o.offset),
            " ".repeat(lpad),
            " ".repeat(rpad)
        )
    }
}

/// Write pages for one stream of rows.
fn emit_pages(
    o: &Opts,
    rows: Vec<String>,
    rows_per_page: usize,
    title: &str,
    date: &str,
    out: &mut String,
) {
    let headers = !o.omit_header;
    let body_len = if headers {
        o.page_len - HEADER_LINES - TRAILER_LINES
    } else {
        o.page_len
    };
    let pages: Vec<&[String]> = if rows.is_empty() {
        Vec::new()
    } else {
        rows.chunks(rows_per_page.max(1)).collect()
    };
    for (idx, page_rows) in pages.iter().enumerate() {
        let page = idx + 1;
        if page < o.first_page || o.last_page.is_some_and(|l| page > l) {
            continue;
        }
        if headers {
            out.push_str(&" ".repeat(o.offset));
            out.push_str("\n\n");
            out.push_str(&Pr::header_line(o, date, title, page));
            out.push_str("\n\n\n");
        }
        let mut used = 0;
        for (k, r) in page_rows.iter().enumerate() {
            out.push_str(r);
            out.push('\n');
            used += 1;
            // Multi-column pages put no blank line after the last row.
            if o.double && (o.columns <= 1 || k + 1 < page_rows.len()) {
                out.push('\n');
                used += 1;
            }
        }
        if headers {
            if o.form_feed {
                out.push('\x0c');
            } else {
                for _ in used..body_len {
                    out.push('\n');
                }
                for _ in 0..TRAILER_LINES {
                    out.push('\n');
                }
            }
        }
    }
}

fn layout_for(o: &Opts) -> Layout<'_> {
    let multi = o.columns > 1;
    let width = o.page_width.or(o.width).unwrap_or(PAGE_WIDTH);
    // `-s` without `-w`, or `-J`: no truncation, no padding.
    let unpadded = o.join || (o.sep_char.is_some() && o.width.is_none() && o.page_width.is_none());
    let sep = if let Some(s) = &o.sep_string {
        s.clone()
    } else if let Some(s) = &o.sep_char {
        s.chars().next().map(String::from).unwrap_or_default()
    } else if o.join {
        "\t".to_string()
    } else {
        " ".to_string()
    };
    let lead = match o.number {
        Some(('\t', digits)) if o.merge => (digits / TAB + 1) * TAB,
        Some((_, digits)) if o.merge => digits + 1,
        _ => 0,
    };
    let col_width = if multi && !unpadded {
        let sep_total = (o.columns - 1) * sep.chars().count();
        Some(width.saturating_sub(sep_total + lead) / o.columns)
    } else {
        None
    };
    Layout {
        o,
        col_width,
        sep,
        lead,
    }
}

/// Rows of cells for one file in down/across order, page by page.
fn file_rows(o: &Opts, lines: &[String], rows_per_page: usize) -> Vec<Vec<Cell>> {
    let numbered = o.number.is_some();
    let cell = |k: usize| -> Cell {
        lines
            .get(k)
            .map(|l| (l.clone(), numbered.then_some(o.first_number + k as i64)))
    };
    let cols = o.columns.max(1);
    let mut rows = Vec::new();
    if cols == 1 {
        for k in 0..lines.len() {
            rows.push(vec![cell(k)]);
        }
        return rows;
    }
    let per_page = rows_per_page.max(1) * cols;
    let mut start = 0;
    while start < lines.len() {
        let n = (lines.len() - start).min(per_page);
        if o.across {
            let nrows = n.div_ceil(cols);
            for r in 0..nrows {
                let row = (0..cols)
                    .map(|c| {
                        let k = r * cols + c;
                        (k < n).then(|| cell(start + k)).flatten()
                    })
                    .collect();
                rows.push(row);
            }
        } else {
            // Full page: each column holds rows_per_page lines. Last page:
            // balanced, the first columns take the extra lines.
            let counts: Vec<usize> = if n == per_page {
                vec![rows_per_page; cols]
            } else {
                let base = n / cols;
                let rem = n % cols;
                (0..cols).map(|c| base + usize::from(c < rem)).collect()
            };
            let nrows = counts.iter().copied().max().unwrap_or(0);
            let mut col_start = Vec::with_capacity(cols);
            let mut acc = 0;
            for c in &counts {
                col_start.push(acc);
                acc += c;
            }
            for r in 0..nrows {
                let row = (0..cols)
                    .map(|c| {
                        (r < counts[c])
                            .then(|| cell(start + col_start[c] + r))
                            .flatten()
                    })
                    .collect();
                rows.push(row);
            }
        }
        start += n;
    }
    rows
}

#[async_trait]
impl Builtin for Pr {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        if ctx.args.iter().any(|a| a == "--help") {
            return Ok(ExecResult::ok(USAGE.to_string()));
        }
        if ctx.args.iter().any(|a| a == "--version") {
            return Ok(ExecResult::ok("pr (bashkit) 0.1\n".to_string()));
        }
        let (mut o, files) = match parse_args(ctx.args) {
            Ok(v) => v,
            Err(e) => return Ok(e),
        };
        if o.page_len <= HEADER_LINES + TRAILER_LINES {
            o.omit_header = true;
        }
        let tz = ctx.env.get("TZ");
        let now = self
            .clock
            .strftime(tz, None, &o.date_format)
            .unwrap_or_default();

        let mut stderr = String::new();
        let mut exit_code = 0;
        let mut inputs = Vec::new();
        for file in &files {
            let (bytes, title, date) = if file == "-" {
                let data = ctx.stdin_bytes().map(<[u8]>::to_vec).unwrap_or_default();
                (data, String::new(), now.clone())
            } else {
                let path = resolve_path(ctx.cwd, file);
                let data = match ctx.fs.read_file(&path).await {
                    Ok(d) => d,
                    Err(e) => {
                        stderr.push_str(&format!(
                            "pr: {file}: {}\n",
                            crate::error::io_error_reason(&e)
                        ));
                        exit_code = 1;
                        continue;
                    }
                };
                let date = match ctx.fs.stat(&path).await {
                    Ok(meta) => {
                        let secs = crate::time_compat::to_chrono_utc(meta.modified).timestamp();
                        self.clock
                            .strftime(tz, Some(secs), &o.date_format)
                            .unwrap_or_default()
                    }
                    Err(_) => now.clone(),
                };
                (data, file.clone(), date)
            };
            ctx.consume_budget_work(1 + (bytes.len() / 1024) as u64)?;
            let text = String::from_utf8_lossy(&bytes);
            let mut lines: Vec<String> = text.split('\n').map(str::to_string).collect();
            if text.ends_with('\n') || text.is_empty() {
                lines.pop();
            }
            inputs.push(Input { lines, title, date });
        }

        let headers = !o.omit_header;
        let body_len = if headers {
            o.page_len - HEADER_LINES - TRAILER_LINES
        } else {
            o.page_len
        };
        let rows_per_page = if o.double {
            body_len.div_ceil(2)
        } else {
            body_len
        }
        .max(1);
        let layout = layout_for(&o);
        let mut out = String::new();

        if o.merge {
            let header = o.header.clone().unwrap_or_default();
            let longest = inputs.iter().map(|i| i.lines.len()).max().unwrap_or(0);
            let mut rows = Vec::with_capacity(longest);
            let mut rows_bytes = 0;
            for k in 0..longest {
                let number = o.number.is_some().then_some(o.first_number + k as i64);
                let cells: Vec<Cell> = inputs
                    .iter()
                    .enumerate()
                    .map(|(c, inp)| {
                        let text = inp.lines.get(k).cloned().unwrap_or_default();
                        Some((text, if c == 0 { number } else { None }))
                    })
                    .collect();
                let line = layout.render(&cells);
                rows_bytes += line.len() + 1;
                rows.push(line);
                if rows_bytes > MAX_OUTPUT {
                    break;
                }
            }
            emit_pages(&o, rows, rows_per_page, &header, &now, &mut out);
        } else {
            for inp in &inputs {
                let title = o.header.clone().unwrap_or_else(|| inp.title.clone());
                let cells = file_rows(&o, &inp.lines, rows_per_page);
                let mut rows = Vec::with_capacity(cells.len());
                let mut rows_bytes = 0;
                for c in &cells {
                    let line = layout.render(c);
                    rows_bytes += line.len() + 1;
                    rows.push(line);
                    if out.len() + rows_bytes > MAX_OUTPUT {
                        break;
                    }
                }
                emit_pages(&o, rows, rows_per_page, &title, &inp.date, &mut out);
                if out.len() > MAX_OUTPUT {
                    break;
                }
            }
        }

        if out.len() > MAX_OUTPUT {
            return Ok(ExecResult::err(
                format!("pr: output exceeds {MAX_OUTPUT} bytes\n"),
                1,
            ));
        }
        ctx.consume_budget_work(1 + (out.len() / 1024) as u64)?;
        let mut result = ExecResult::with_code(out, exit_code);
        result.stderr = stderr.into();
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(args: &[&str], input: &str) -> String {
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let Ok((o, _)) = parse_args(&args) else {
            panic!("valid args");
        };
        let lines: Vec<String> = input.lines().map(str::to_string).collect();
        let layout = layout_for(&o);
        file_rows(&o, &lines, 66)
            .iter()
            .map(|c| layout.render(c) + "\n")
            .collect()
    }

    #[test]
    fn two_columns_pad_with_tabs() {
        assert_eq!(rows(&["-2"], "1\n2\n3\n"), "1\t\t\t\t    3\n2\n");
    }

    #[test]
    fn three_columns_balance_first_columns() {
        assert_eq!(
            rows(&["-3"], "1\n2\n3\n4\n5\n6\n7\n"),
            "1\t\t\t4\t\t\t6\n2\t\t\t5\t\t\t7\n3\n"
        );
    }

    #[test]
    fn across_fills_rows() {
        assert_eq!(rows(&["-3", "-a"], "1\n2\n3\n4\n"), "1\t\t\t2\t\t\t3\n4\n");
    }

    #[test]
    fn separator_char_without_width_has_no_padding() {
        assert_eq!(rows(&["-2", "-s,"], "1\n2\n"), "1,2\n");
    }

    #[test]
    fn numbering_single_column() {
        assert_eq!(rows(&["-n"], "a\nb\n"), "    1\ta\n    2\tb\n");
        assert_eq!(rows(&["-n:3"], "a\n"), "  1:a\n");
    }

    #[test]
    fn bad_options_fail() {
        let args = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert!(parse_args(&args(&["-l", "x"])).is_err());
        assert!(parse_args(&args(&["-w"])).is_err());
        assert!(parse_args(&args(&["-z"])).is_err());
        assert!(parse_args(&args(&["-l", "99999999"])).is_err());
    }
}
