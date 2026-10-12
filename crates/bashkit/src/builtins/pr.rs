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
//!   Inputs retain timestamps; long date formats are rendered one stream at
//!   a time, so empty files do not retain duplicate date headers.
//! - Every output/row append checks `MAX_OUTPUT` before growth. Owning
//!   buffers lease capacity from the shared budget; cells borrow input text.
//!   Pagination checks work/deadlines even for skipped pages (TM-DOS-137).
//! - Not implemented (L-PR-001): `-e`/`-i` tab conversion options; `-c` and
//!   `-v` are accepted and ignored (no control-character display).

use async_trait::async_trait;

use super::{Builtin, Context, Date, resolve_path};
use crate::error::Result;
use crate::interpreter::ExecResult;
use crate::limits::{BudgetedString, BudgetedVec, ExecutionBudget, LimitExceeded};

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
type Cell<'a> = Option<(&'a str, Option<i64>)>;

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
struct Input<'a> {
    lines: BudgetedVec<BudgetedString>,
    title: &'a str,
    // THREAT[TM-DOS-137]: Do not retain a long rendered -D header per file.
    date: Option<i64>,
}

// THREAT[TM-DOS-137]: admit all pagination bytes, including headers, before
// allocation. BudgetedString charges capacity and polls cancellation/deadlines.
fn append(out: &mut BudgetedString, text: &str) -> Result<()> {
    if out
        .len()
        .checked_add(text.len())
        .is_none_or(|len| len > MAX_OUTPUT)
    {
        return Err(LimitExceeded::Memory(format!("pr: output exceeds {MAX_OUTPUT} bytes")).into());
    }
    out.try_push_str_with_limit(text, MAX_OUTPUT)?;
    Ok(())
}

fn checked_rows_bytes(total: usize, line_len: usize) -> Result<usize> {
    total
        .checked_add(line_len)
        .and_then(|len| len.checked_add(1))
        .filter(|len| *len <= MAX_OUTPUT)
        .ok_or_else(|| {
            LimitExceeded::Memory(format!("pr: output exceeds {MAX_OUTPUT} bytes")).into()
        })
}

fn push(out: &mut BudgetedString, ch: char) -> Result<()> {
    append(out, ch.encode_utf8(&mut [0; 4]))
}

fn repeat(out: &mut BudgetedString, ch: char, count: usize) -> Result<()> {
    for _ in 0..count {
        push(out, ch)?;
    }
    Ok(())
}

fn work(budget: Option<&ExecutionBudget>, units: usize) -> Result<()> {
    if let Some(budget) = budget {
        budget.consume_work(units as u64)?;
    }
    Ok(())
}

/// Expand tabs to spaces, columns counted from `start`.
fn expand_tabs(s: &str, start: usize, budget: Option<&ExecutionBudget>) -> Result<BudgetedString> {
    let mut out = BudgetedString::new(budget)?;
    let mut col = start;
    for ch in s.chars() {
        if ch == '\t' {
            let next = (col / TAB + 1) * TAB;
            repeat(&mut out, ' ', next - col)?;
            col = next;
        } else {
            push(&mut out, ch)?;
            col += 1;
        }
    }
    Ok(out)
}

/// Line writer tracking the output column, writing blank runs as tabs
/// where they reach a tab stop (GNU multi-column output).
struct Row {
    out: BudgetedString,
    col: usize,
}

impl Row {
    fn new(offset: usize, budget: Option<&ExecutionBudget>) -> Result<Self> {
        let mut out = BudgetedString::new(budget)?;
        repeat(&mut out, ' ', offset)?;
        Ok(Row { out, col: offset })
    }

    /// Blanks up to column `target`, as tabs then spaces.
    fn pad_to(&mut self, target: usize) -> Result<()> {
        while self.col < target {
            let next = (self.col / TAB + 1) * TAB;
            if next <= target {
                // Even a single blank up to a tab stop is written as a tab.
                push(&mut self.out, '\t')?;
                self.col = next;
            } else {
                push(&mut self.out, ' ')?;
                self.col += 1;
            }
        }
        Ok(())
    }

    /// A cell's text (tabs already expanded). Runs of two or more blanks
    /// become tabs where they reach a stop; a single blank stays a space;
    /// trailing blanks are left to the next padding run.
    fn cell(&mut self, text: &str) -> Result<()> {
        let mut run = 0;
        for ch in text.chars() {
            if ch == ' ' {
                run += 1;
                continue;
            }
            if run == 1 {
                push(&mut self.out, ' ')?;
                self.col += 1;
            } else if run > 1 {
                let target = self.col + run;
                self.pad_to(target)?;
            }
            run = 0;
            push(&mut self.out, ch)?;
            self.col += 1;
        }
        Ok(())
    }

    /// Text written as is (separators, single-column lines).
    fn raw(&mut self, text: &str) -> Result<()> {
        append(&mut self.out, text)?;
        self.col += text.chars().count();
        Ok(())
    }
}

struct Layout<'a> {
    o: &'a Opts,
    /// Per-column text width; None means no truncation and no padding.
    col_width: Option<usize>,
    sep: &'a str,
    /// `-m -n`: width of the line number field ahead of the first column
    /// (a TAB separator counts up to the next tab stop).
    lead: usize,
}

impl Layout<'_> {
    fn number(&self, n: i64, budget: Option<&ExecutionBudget>) -> Result<BudgetedString> {
        let mut out = BudgetedString::new(budget)?;
        if let Some((sep, digits)) = self.o.number {
            let text = n.to_string();
            let start = text.len().saturating_sub(digits);
            repeat(&mut out, ' ', digits.saturating_sub(text.len()))?;
            append(&mut out, &text[start..])?;
            push(&mut out, sep)?;
        }
        Ok(out)
    }

    /// Render one row of cells (None = absent cell, Some("") = empty cell).
    fn render(
        &self,
        cells: &[Cell<'_>],
        budget: Option<&ExecutionBudget>,
    ) -> Result<BudgetedString> {
        let o = self.o;
        work(
            budget,
            1 + cells
                .iter()
                .flatten()
                .map(|(s, _)| s.len() / 1024)
                .sum::<usize>(),
        )?;
        let mut row = Row::new(o.offset, budget)?;
        if o.columns <= 1 {
            if let Some(Some((text, n))) = cells.first() {
                if let Some(n) = n {
                    row.raw(&self.number(*n, budget)?)?;
                }
                let mut line = *text;
                if let Some(w) = o.page_width {
                    let room = w.saturating_sub(row.col - o.offset);
                    let end = line.char_indices().nth(room).map_or(line.len(), |(i, _)| i);
                    line = &line[..end];
                }
                row.raw(line)?;
            }
            return Ok(row.out);
        }
        let last = cells.iter().rposition(Option::is_some);
        let Some(last) = last else {
            return Ok(row.out);
        };
        for (i, cell) in cells.iter().enumerate().take(last + 1) {
            if i > 0 {
                match self.col_width {
                    Some(_) => {
                        // Separator: a blank (part of the padding run) or the
                        // -S/-s string after padding to the column width.
                        let start = o.offset + self.col_offset(i);
                        if self.sep == " " {
                            row.pad_to(start)?;
                        } else {
                            row.pad_to(start - self.sep_len())?;
                            row.raw(self.sep)?;
                        }
                    }
                    None => row.raw(self.sep)?,
                }
            }
            let Some((text, n)) = cell else {
                continue;
            };
            let col_start = row.col;
            let mut content = BudgetedString::new(budget)?;
            if let Some(n) = n {
                // GNU expands the number separator within its own field,
                // even when joined columns have no fixed text width.
                let number = self.number(*n, budget)?;
                append(&mut content, &expand_tabs(&number, 0, budget)?)?;
            }
            append(&mut content, text)?;
            let rel = col_start - o.offset - self.col_offset(i);
            let expanded = expand_tabs(&content, rel, budget)?;
            let end = if let Some(w) = self.col_width {
                let w = if i == 0 { w + self.lead } else { w };
                expanded
                    .char_indices()
                    .nth(w.saturating_sub(rel))
                    .map_or(expanded.len(), |(i, _)| i)
            } else {
                expanded.len()
            };
            row.cell(&expanded[..end])?;
        }
        Ok(row.out)
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
    fn header_line(
        o: &Opts,
        date: &str,
        title: &str,
        page: usize,
        out: &mut BudgetedString,
    ) -> Result<()> {
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
        repeat(out, ' ', o.offset)?;
        append(out, date)?;
        repeat(out, ' ', lpad)?;
        append(out, title)?;
        repeat(out, ' ', rpad)?;
        append(out, &right)
    }
}

/// Write pages for one stream of rows.
fn emit_pages(
    o: &Opts,
    rows: &[BudgetedString],
    rows_per_page: usize,
    title: &str,
    date: &str,
    out: &mut BudgetedString,
    budget: Option<&ExecutionBudget>,
) -> Result<()> {
    let headers = !o.omit_header;
    let body_len = if headers {
        o.page_len - HEADER_LINES - TRAILER_LINES
    } else {
        o.page_len
    };
    for (idx, page_rows) in rows.chunks(rows_per_page.max(1)).enumerate() {
        work(budget, 1)?;
        let page = idx + 1;
        if page < o.first_page || o.last_page.is_some_and(|l| page > l) {
            continue;
        }
        if headers {
            work(budget, 1 + title.len() / 1024 + date.len() / 1024)?;
            repeat(out, ' ', o.offset)?;
            append(out, "\n\n")?;
            Pr::header_line(o, date, title, page, out)?;
            append(out, "\n\n\n")?;
        }
        let mut used = 0;
        for (k, r) in page_rows.iter().enumerate() {
            append(out, r)?;
            push(out, '\n')?;
            used += 1;
            // Multi-column pages put no blank line after the last row.
            if o.double && (o.columns <= 1 || k + 1 < page_rows.len()) {
                push(out, '\n')?;
                used += 1;
            }
        }
        if headers {
            if o.form_feed {
                push(out, '\x0c')?;
            } else {
                for _ in used..body_len {
                    push(out, '\n')?;
                }
                for _ in 0..TRAILER_LINES {
                    push(out, '\n')?;
                }
            }
        }
    }
    Ok(())
}

fn layout_for(o: &Opts) -> Layout<'_> {
    let multi = o.columns > 1;
    let width = o.page_width.or(o.width).unwrap_or(PAGE_WIDTH);
    // `-s` without `-w`, or `-J`: no truncation, no padding.
    let unpadded = o.join || (o.sep_char.is_some() && o.width.is_none() && o.page_width.is_none());
    let sep = if let Some(s) = &o.sep_string {
        s.as_str()
    } else if let Some(s) = &o.sep_char {
        s.chars()
            .next()
            .map(|c| &s[..c.len_utf8()])
            .unwrap_or_default()
    } else if o.join {
        "\t"
    } else {
        " "
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
fn file_rows<'a>(
    o: &Opts,
    lines: &'a [BudgetedString],
    rows_per_page: usize,
    budget: Option<&ExecutionBudget>,
) -> Result<BudgetedVec<BudgetedVec<Cell<'a>>>> {
    let numbered = o.number.is_some();
    let cell = |k: usize| -> Cell<'a> {
        lines
            .get(k)
            .map(|l| (l.as_ref(), numbered.then_some(o.first_number + k as i64)))
    };
    let cols = o.columns.max(1);
    let mut rows = BudgetedVec::new(budget)?;
    if cols == 1 {
        for k in 0..lines.len() {
            work(budget, 1)?;
            let mut row = BudgetedVec::new(budget)?;
            row.try_push(cell(k))?;
            rows.try_push(row)?;
        }
        return Ok(rows);
    }
    let per_page = rows_per_page.max(1) * cols;
    let mut start = 0;
    while start < lines.len() {
        work(budget, 1)?;
        let n = (lines.len() - start).min(per_page);
        if o.across {
            let nrows = n.div_ceil(cols);
            for r in 0..nrows {
                let row = (0..cols)
                    .map(|c| {
                        let k = r * cols + c;
                        (k < n).then(|| cell(start + k)).flatten()
                    })
                    .try_fold(BudgetedVec::new(budget)?, |mut row, cell| -> Result<_> {
                        work(budget, 1)?;
                        row.try_push(cell)?;
                        Ok(row)
                    })?;
                rows.try_push(row)?;
            }
        } else {
            // Full page: each column holds rows_per_page lines. Last page:
            // balanced, the first columns take the extra lines.
            let base = n / cols;
            let rem = n % cols;
            let nrows = base + usize::from(rem > 0);
            for r in 0..nrows {
                let row = (0..cols)
                    .map(|c| {
                        (r < base + usize::from(c < rem))
                            .then(|| cell(start + c * base + c.min(rem) + r))
                            .flatten()
                    })
                    .try_fold(BudgetedVec::new(budget)?, |mut row, cell| -> Result<_> {
                        work(budget, 1)?;
                        row.try_push(cell)?;
                        Ok(row)
                    })?;
                rows.try_push(row)?;
            }
        }
        start += n;
    }
    Ok(rows)
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
        let budget = ctx
            .execution_budget()
            .map(|budget| {
                budget
                    .try_with(Clone::clone)
                    .map_err(|_| crate::Error::Cancelled)
            })
            .transpose()?;
        let budget = budget.as_ref();
        let tz = ctx.env.get("TZ");
        let now = if o.omit_header {
            String::new()
        } else {
            self.clock
                .strftime(tz, None, &o.date_format)
                .unwrap_or_default()
        };

        let mut stderr = String::new();
        let mut exit_code = 0;
        let mut inputs = BudgetedVec::new(budget)?;
        for file in &files {
            let mut _bytes_lease = None;
            let mut _capacity_lease = None;
            let mut reserved_size = 0;
            let (bytes, title, date) =
                if file == "-" {
                    let data = std::borrow::Cow::Borrowed(ctx.stdin_bytes().unwrap_or_default());
                    (data, "", None)
                } else {
                    let path = resolve_path(ctx.cwd, file);
                    if let Ok(meta) = ctx.fs.stat(&path).await {
                        let size = usize::try_from(meta.size)
                            .map_err(|_| LimitExceeded::Memory("pr: input size overflow".into()))?;
                        _bytes_lease = ctx.lease_budget_bytes(size)?;
                        reserved_size = size;
                    }
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
                    let date =
                        ctx.fs.stat(&path).await.ok().map(|meta| {
                            crate::time_compat::to_chrono_utc(meta.modified).timestamp()
                        });
                    _capacity_lease =
                        ctx.lease_budget_bytes(data.capacity().saturating_sub(reserved_size))?;
                    (std::borrow::Cow::Owned(data), file.as_str(), date)
                };
            ctx.consume_budget_input(bytes.len())?;

            ctx.consume_budget_work(1 + (bytes.len() / 1024) as u64)?;
            // Lossy UTF-8 can require three bytes per invalid input byte.
            let _decode_lease = if std::str::from_utf8(&bytes).is_err() {
                ctx.lease_budget_bytes(
                    bytes
                        .len()
                        .checked_mul(3)
                        .ok_or_else(|| LimitExceeded::Memory("pr: input size overflow".into()))?,
                )?
            } else {
                None
            };
            let text = String::from_utf8_lossy(&bytes);
            let mut lines = BudgetedVec::new(budget)?;
            for line in text.split('\n') {
                work(budget, 1)?;
                let mut owned = BudgetedString::new(budget)?;
                append(&mut owned, line)?;
                lines.try_push(owned)?;
            }
            if text.ends_with('\n') || text.is_empty() {
                lines.pop();
            }
            inputs.try_push(Input { lines, title, date })?;
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
        let mut out = BudgetedString::new(budget)?;

        if o.merge {
            let header = o.header.as_deref().unwrap_or_default();
            let longest = inputs.iter().map(|i| i.lines.len()).max().unwrap_or(0);
            let mut rows = BudgetedVec::new(budget)?;
            let mut rows_bytes = 0;
            for k in 0..longest {
                let number = o.number.is_some().then_some(o.first_number + k as i64);
                let cells = inputs
                    .iter()
                    .enumerate()
                    .map(|(c, inp)| {
                        let text = inp.lines.get(k).map(|s| s.as_ref()).unwrap_or_default();
                        Some((text, if c == 0 { number } else { None }))
                    })
                    .try_fold(BudgetedVec::new(budget)?, |mut cells, cell| -> Result<_> {
                        cells.try_push(cell)?;
                        Ok(cells)
                    })?;
                let line = layout.render(&cells, budget)?;
                rows_bytes = checked_rows_bytes(rows_bytes, line.len())?;
                rows.try_push(line)?;
            }
            emit_pages(&o, &rows, rows_per_page, header, &now, &mut out, budget)?;
        } else {
            for inp in inputs.iter() {
                let title = o.header.as_deref().unwrap_or(inp.title);
                let cells = file_rows(&o, &inp.lines, rows_per_page, budget)?;
                let mut rows = BudgetedVec::new(budget)?;
                let mut rows_bytes = 0;
                for c in cells.iter() {
                    let line = layout.render(c, budget)?;
                    rows_bytes = checked_rows_bytes(rows_bytes, line.len())?;
                    rows.try_push(line)?;
                }
                if rows.is_empty() {
                    continue;
                }
                // At most one file's formatted date is live while emitting.
                let date = if o.omit_header {
                    std::borrow::Cow::Borrowed("")
                } else if let Some(secs) = inp.date {
                    std::borrow::Cow::Owned(
                        self.clock
                            .strftime(tz, Some(secs), &o.date_format)
                            .unwrap_or_default(),
                    )
                } else {
                    std::borrow::Cow::Borrowed(now.as_str())
                };
                emit_pages(&o, &rows, rows_per_page, title, &date, &mut out, budget)?;
            }
        }

        ctx.consume_budget_work(1 + (out.len() / 1024) as u64)?;
        let mut result = ExecResult::with_code(out.into_inner(), exit_code);
        result.stderr = stderr.into();
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn joined_number_fields_use_local_tab_stops() {
        assert_eq!(
            rows(&["-2", "-J", "-n"], "a\nb\nc\nd\n"),
            "    1\ta\t    3\t  c\n    2\tb\t    4\t  d\n"
        );
    }

    fn rows(args: &[&str], input: &str) -> String {
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let Ok((o, _)) = parse_args(&args) else {
            panic!("valid args");
        };
        let lines: Vec<BudgetedString> = input
            .lines()
            .map(|s| BudgetedString::try_from_string(s.to_string(), None).unwrap())
            .collect();
        let layout = layout_for(&o);
        file_rows(&o, &lines, 66, None)
            .unwrap()
            .iter()
            .map(|c| layout.render(c, None).unwrap().into_inner() + "\n")
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
    #[test]
    fn pagination_cap_is_checked_before_append_at_utf8_boundary() {
        use std::sync::{Arc, atomic::AtomicBool};
        let budget = ExecutionBudget::new(
            &crate::ExecutionLimits::new().max_live_intermediate_bytes(MAX_OUTPUT as u64),
            Arc::new(AtomicBool::new(false)),
        );
        let mut out = BudgetedString::new(Some(&budget)).unwrap();
        append(&mut out, &"x".repeat(MAX_OUTPUT - 1)).unwrap();
        assert!(append(&mut out, "é").is_err());
        assert_eq!(out.len(), MAX_OUTPUT - 1);
        push(&mut out, 'x').unwrap();
        append(&mut out, "").unwrap();
        assert_eq!(out.len(), MAX_OUTPUT);
        assert!(push(&mut out, '\n').is_err());
        assert_eq!(out.len(), MAX_OUTPUT);
        assert!(checked_rows_bytes(usize::MAX, 1).is_err());
    }

    #[test]
    fn pagination_checks_cancelled_budget_even_for_skipped_pages() {
        use std::sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        };
        let cancel = Arc::new(AtomicBool::new(false));
        let budget = ExecutionBudget::new(&crate::ExecutionLimits::new(), cancel.clone());
        let args = vec!["+2".to_string()];
        let (o, _) = parse_args(&args).unwrap();
        let rows = vec![BudgetedString::new(None).unwrap()];
        let mut out = BudgetedString::new(Some(&budget)).unwrap();
        cancel.store(true, Ordering::Relaxed);
        assert!(emit_pages(&o, &rows, 1, "title", "date", &mut out, Some(&budget)).is_err());
        assert!(out.is_empty());
    }
}
