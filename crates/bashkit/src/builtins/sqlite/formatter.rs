//! Output formatting for SQLite query results.
//!
//! Mirrors the `sqlite3` shell modes that LLM agents and humans actually use:
//! list (default), csv, tabs, line, column/box, json, markdown.

use std::fmt::Write;

use turso_core::{Numeric, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum OutputMode {
    /// Default: separator-joined fields, one row per line.
    #[default]
    List,
    /// RFC 4180 CSV with quoted fields.
    Csv,
    /// Tab-separated.
    Tabs,
    /// Each column on its own line as `name = value` blocks.
    Line,
    /// Box-drawing table with column alignment.
    Box,
    /// One JSON object per row, all wrapped in an array.
    Json,
    /// GitHub-flavoured markdown table.
    Markdown,
    /// Same as List but each value padded to the column width (best-effort).
    Column,
}

impl OutputMode {
    pub(super) fn parse(s: &str) -> Option<Self> {
        Some(match s.to_ascii_lowercase().as_str() {
            "list" => Self::List,
            "csv" => Self::Csv,
            "tab" | "tabs" => Self::Tabs,
            "line" | "lines" => Self::Line,
            "box" => Self::Box,
            "column" | "columns" => Self::Column,
            "json" => Self::Json,
            "markdown" | "md" => Self::Markdown,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone)]
pub(super) struct OutputOpts {
    pub mode: OutputMode,
    pub separator: String,
    pub headers: bool,
    pub null_text: String,
}

impl Default for OutputOpts {
    fn default() -> Self {
        Self {
            mode: OutputMode::default(),
            // sqlite3 default is "|"
            separator: "|".to_string(),
            headers: false,
            null_text: String::new(),
        }
    }
}

/// Append a result to stdout, rolling it back if it exceeds the byte budget.
pub(super) fn render_bounded(
    stdout: &mut String,
    cols: &[String],
    rows: &[Vec<Value>],
    opts: &OutputOpts,
    max_bytes: usize,
) -> Result<(), String> {
    let start = stdout.len();
    let mut out = BoundedOutput {
        text: stdout,
        remaining: max_bytes.saturating_sub(start),
        rejected_at: None,
    };
    if render_into(&mut out, cols, rows, opts).is_err() {
        let next = out.rejected_at.unwrap_or(out.text.len());
        out.text.truncate(start);
        return Err(format!(
            "sqlite output exceeds byte cap ({next} > {max_bytes})"
        ));
    }
    Ok(())
}

#[cfg(test)]
fn render(cols: &[String], rows: &[Vec<Value>], opts: &OutputOpts) -> String {
    let mut stdout = String::new();
    render_bounded(&mut stdout, cols, rows, opts, usize::MAX).unwrap();
    stdout
}

struct BoundedOutput<'a> {
    text: &'a mut String,
    remaining: usize,
    rejected_at: Option<usize>,
}

impl BoundedOutput<'_> {
    fn check_bytes(&mut self, bytes: usize) -> std::fmt::Result {
        if bytes > self.remaining {
            self.rejected_at = Some(self.text.len().saturating_add(bytes));
            return Err(std::fmt::Error);
        }
        Ok(())
    }

    fn reserve_bytes(&mut self, bytes: usize) -> std::fmt::Result {
        self.check_bytes(bytes)?;
        self.remaining -= bytes;
        Ok(())
    }

    fn repeat(&mut self, ch: char, count: usize) -> std::fmt::Result {
        let bytes = ch.len_utf8().saturating_mul(count);
        self.reserve_bytes(bytes)?;
        self.text.extend(std::iter::repeat_n(ch, count));
        Ok(())
    }

    fn json_string(&mut self, text: &str) -> std::fmt::Result {
        serde_json::to_writer(self, text).map_err(|_| std::fmt::Error)
    }
}

impl std::fmt::Write for BoundedOutput<'_> {
    fn write_str(&mut self, text: &str) -> std::fmt::Result {
        self.reserve_bytes(text.len())?;
        self.text.push_str(text);
        Ok(())
    }
}

impl std::io::Write for BoundedOutput<'_> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        // serde_json emits complete UTF-8 fragments for strings and ASCII escapes.
        let text = std::str::from_utf8(bytes).map_err(std::io::Error::other)?;
        self.write_str(text).map_err(std::io::Error::other)?;
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn display_value<'a>(
    out: &mut BoundedOutput<'_>,
    value: &'a Value,
    null_text: &'a str,
) -> Result<std::borrow::Cow<'a, str>, std::fmt::Error> {
    Ok(match value {
        Value::Null => null_text.into(),
        Value::Text(text) => text.as_str().into(),
        Value::Blob(bytes) => {
            // Each invalid UTF-8 sequence consumes at most three input bytes
            // and is replaced by a three-byte replacement character.
            out.check_bytes(bytes.len())?;
            String::from_utf8_lossy(bytes)
        }
        Value::Numeric(_) => value.to_string().into(),
    })
}

fn display_width(value: &Value, null_text: &str) -> usize {
    match value {
        Value::Null => null_text.chars().count(),
        Value::Text(text) => text.as_str().chars().count(),
        Value::Blob(bytes) => bytes
            .utf8_chunks()
            .map(|chunk| chunk.valid().chars().count() + usize::from(!chunk.invalid().is_empty()))
            .sum(),
        Value::Numeric(_) => value.to_string().chars().count(),
    }
}

fn write_field(
    out: &mut BoundedOutput<'_>,
    field: &str,
    separator: &str,
    csv: bool,
) -> std::fmt::Result {
    if !csv || !(field.contains(separator) || field.contains(['"', '\n', '\r'])) {
        return out.write_str(field);
    }
    out.write_char('"')?;
    for (index, part) in field.split('"').enumerate() {
        if index > 0 {
            out.write_str("\"\"")?;
        }
        out.write_str(part)?;
    }
    out.write_char('"')
}

fn render_into(
    out: &mut BoundedOutput<'_>,
    cols: &[String],
    rows: &[Vec<Value>],
    opts: &OutputOpts,
) -> std::fmt::Result {
    if cols.is_empty() || (rows.is_empty() && opts.mode != OutputMode::Json) {
        return Ok(());
    }
    match opts.mode {
        OutputMode::List | OutputMode::Tabs | OutputMode::Csv => {
            let sep = if matches!(opts.mode, OutputMode::Tabs) {
                "\t"
            } else {
                &opts.separator
            };
            let csv = matches!(opts.mode, OutputMode::Csv);
            if opts.headers {
                for (index, col) in cols.iter().enumerate() {
                    if index > 0 {
                        out.write_str(sep)?;
                    }
                    write_field(out, col, sep, csv)?;
                }
                out.write_char('\n')?;
            }
            for row in rows {
                for (index, value) in row.iter().enumerate() {
                    if index > 0 {
                        out.write_str(sep)?;
                    }
                    let field = display_value(out, value, &opts.null_text)?;
                    write_field(out, &field, sep, csv)?;
                }
                out.write_char('\n')?;
            }
            Ok(())
        }
        OutputMode::Line => {
            let width = cols.iter().map(String::len).max().unwrap_or_default();
            for (index, row) in rows.iter().enumerate() {
                if index > 0 {
                    out.write_char('\n')?;
                }
                for (col, value) in cols.iter().zip(row) {
                    out.repeat(' ', width - col.chars().count())?;
                    out.write_str(col)?;
                    out.write_str(" = ")?;
                    let field = display_value(out, value, &opts.null_text)?;
                    out.write_str(&field)?;
                    out.write_char('\n')?;
                }
            }
            Ok(())
        }
        OutputMode::Column | OutputMode::Box | OutputMode::Markdown => {
            render_table(out, cols, rows, opts)
        }
        OutputMode::Json => render_json(out, cols, rows),
    }
}

fn render_table(
    out: &mut BoundedOutput<'_>,
    cols: &[String],
    rows: &[Vec<Value>],
    opts: &OutputOpts,
) -> std::fmt::Result {
    let mut widths: Vec<_> = cols.iter().map(|col| col.chars().count()).collect();
    for row in rows {
        for (value, width) in row.iter().zip(&mut widths) {
            *width = (*width).max(display_width(value, &opts.null_text));
        }
    }
    let (edge, divider, padding) = match opts.mode {
        OutputMode::Box => ("│", "│", " "),
        OutputMode::Markdown => ("|", "|", " "),
        _ => ("", "  ", ""),
    };
    if matches!(opts.mode, OutputMode::Box) {
        write_border(out, &widths, ('┌', '┬', '┐'), '─', 2)?;
    }
    if opts.headers || !matches!(opts.mode, OutputMode::Column) {
        write_table_row(
            out,
            cols.iter().map(|col| TableCell::Header(col)),
            &widths,
            edge,
            divider,
            padding,
        )?;
        match opts.mode {
            OutputMode::Box => write_border(out, &widths, ('├', '┼', '┤'), '─', 2)?,
            OutputMode::Markdown => write_border(out, &widths, ('|', '|', '|'), '-', 2)?,
            _ => {
                for (index, width) in widths.iter().enumerate() {
                    if index > 0 {
                        out.write_str(divider)?;
                    }
                    out.repeat('-', *width)?;
                }
                out.write_char('\n')?;
            }
        }
    }
    for row in rows {
        write_table_row(
            out,
            row.iter()
                .map(|value| TableCell::Value(value, &opts.null_text)),
            &widths,
            edge,
            divider,
            padding,
        )?;
    }
    if matches!(opts.mode, OutputMode::Box) {
        write_border(out, &widths, ('└', '┴', '┘'), '─', 2)?;
    }
    Ok(())
}

enum TableCell<'a> {
    Header(&'a str),
    Value(&'a Value, &'a str),
}

fn write_table_row<'a>(
    out: &mut BoundedOutput<'_>,
    cells: impl Iterator<Item = TableCell<'a>>,
    widths: &[usize],
    edge: &str,
    divider: &str,
    padding: &str,
) -> std::fmt::Result {
    out.write_str(edge)?;
    for (index, (cell, width)) in cells.zip(widths).enumerate() {
        if index > 0 {
            out.write_str(divider)?;
        }
        out.write_str(padding)?;
        let cell = match cell {
            TableCell::Header(col) => col.into(),
            TableCell::Value(value, null_text) => display_value(out, value, null_text)?,
        };
        out.write_str(&cell)?;
        out.repeat(' ', width - cell.chars().count())?;
        out.write_str(padding)?;
    }
    out.write_str(edge)?;
    out.write_char('\n')
}

fn write_border(
    out: &mut BoundedOutput<'_>,
    widths: &[usize],
    (left, middle, right): (char, char, char),
    fill: char,
    padding: usize,
) -> std::fmt::Result {
    out.write_char(left)?;
    for (index, width) in widths.iter().enumerate() {
        if index > 0 {
            out.write_char(middle)?;
        }
        out.repeat(fill, width.saturating_add(padding))?;
    }
    out.write_char(right)?;
    out.write_char('\n')
}

fn render_json(
    out: &mut BoundedOutput<'_>,
    cols: &[String],
    rows: &[Vec<Value>],
) -> std::fmt::Result {
    out.write_char('[')?;
    for (index, row) in rows.iter().enumerate() {
        if index > 0 {
            out.write_char(',')?;
        }
        out.write_char('{')?;
        for (index, (col, value)) in cols.iter().zip(row).enumerate() {
            if index > 0 {
                out.write_char(',')?;
            }
            out.json_string(col)?;
            out.write_char(':')?;
            match value {
                Value::Null => out.write_str("null")?,
                Value::Numeric(Numeric::Integer(i)) => write!(out, "{i}")?,
                Value::Numeric(Numeric::Float(f)) => {
                    let value: f64 = (*f).into();
                    if value.is_finite() {
                        let text = value.to_string();
                        out.write_str(&text)?;
                        if !text.contains(['.', 'e', 'E']) {
                            out.write_str(".0")?;
                        }
                    } else {
                        out.write_str("null")?;
                    }
                }
                Value::Text(text) => out.json_string(text.as_str())?,
                Value::Blob(bytes) => {
                    out.write_char('"')?;
                    for byte in bytes {
                        write!(out, "{byte:02x}")?;
                    }
                    out.write_char('"')?;
                }
            }
        }
        out.write_char('}')?;
    }
    out.write_str("]\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use turso_core::Value;

    fn v_int(i: i64) -> Value {
        Value::from_i64(i)
    }

    fn v_float(f: f64) -> Value {
        Value::from_f64(f)
    }

    fn v_text(s: &str) -> Value {
        Value::from_text(s.to_string())
    }

    #[test]
    fn parse_modes() {
        assert_eq!(OutputMode::parse("csv"), Some(OutputMode::Csv));
        assert_eq!(OutputMode::parse("CSV"), Some(OutputMode::Csv));
        assert_eq!(OutputMode::parse("md"), Some(OutputMode::Markdown));
        assert_eq!(OutputMode::parse("not-a-mode"), None);
    }

    #[test]
    fn list_default_separator() {
        let cols = vec!["a".to_string(), "b".to_string()];
        let rows = vec![vec![v_int(1), v_text("x")]];
        let out = render(&cols, &rows, &OutputOpts::default());
        assert_eq!(out, "1|x\n");
    }

    #[test]
    fn list_with_headers() {
        let cols = vec!["a".to_string(), "b".to_string()];
        let rows = vec![vec![v_int(1), v_text("x")]];
        let opts = OutputOpts {
            headers: true,
            ..Default::default()
        };
        let out = render(&cols, &rows, &opts);
        assert_eq!(out, "a|b\n1|x\n");
    }

    #[test]
    fn csv_quotes_special_fields() {
        let cols = vec!["a".to_string(), "b".to_string()];
        let rows = vec![vec![v_text("hello,world"), v_text("she said \"hi\"")]];
        let opts = OutputOpts {
            mode: OutputMode::Csv,
            separator: ",".to_string(),
            ..Default::default()
        };
        let out = render(&cols, &rows, &opts);
        assert_eq!(out, "\"hello,world\",\"she said \"\"hi\"\"\"\n");
    }

    #[test]
    fn line_mode_aligns_keys() {
        let cols = vec!["short".to_string(), "longer".to_string()];
        let rows = vec![vec![v_int(1), v_int(2)]];
        let out = render(
            &cols,
            &rows,
            &OutputOpts {
                mode: OutputMode::Line,
                ..Default::default()
            },
        );
        // both keys padded to width 6 (max of "longer")
        assert!(out.contains(" short = 1\n"));
        assert!(out.contains("longer = 2\n"));
    }

    #[test]
    fn box_mode_renders_borders() {
        let cols = vec!["x".to_string()];
        let rows = vec![vec![v_int(1)]];
        let out = render(
            &cols,
            &rows,
            &OutputOpts {
                mode: OutputMode::Box,
                ..Default::default()
            },
        );
        assert!(out.contains('┌'));
        assert!(out.contains('└'));
        assert!(out.contains('│'));
    }

    #[test]
    fn json_mode_round_trips_via_serde() {
        let cols = vec![
            "i".to_string(),
            "f".to_string(),
            "s".to_string(),
            "n".to_string(),
        ];
        let rows = vec![vec![v_int(42), v_float(1.5), v_text("hi"), Value::Null]];
        let out = render(
            &cols,
            &rows,
            &OutputOpts {
                mode: OutputMode::Json,
                ..Default::default()
            },
        );
        let parsed: serde_json::Value = serde_json::from_str(out.trim()).unwrap();
        assert_eq!(parsed[0]["i"], 42);
        assert_eq!(parsed[0]["s"], "hi");
        assert!(parsed[0]["n"].is_null());
    }

    #[test]
    fn json_mode_blob_is_hex() {
        let cols = vec!["b".to_string()];
        let rows = vec![vec![Value::Blob(vec![0x00, 0xff, 0x10])]];
        let out = render(
            &cols,
            &rows,
            &OutputOpts {
                mode: OutputMode::Json,
                ..Default::default()
            },
        );
        assert!(out.contains("\"00ff10\""));
    }

    #[test]
    fn markdown_mode_has_separator_row() {
        let cols = vec!["x".to_string(), "y".to_string()];
        let rows = vec![vec![v_int(1), v_text("a")]];
        let out = render(
            &cols,
            &rows,
            &OutputOpts {
                mode: OutputMode::Markdown,
                ..Default::default()
            },
        );
        let lines: Vec<&str> = out.lines().collect();
        assert!(lines[0].starts_with("| x"));
        assert!(lines[1].contains("---"));
        assert!(lines[2].contains("| 1"));
    }

    #[test]
    fn null_uses_configured_text() {
        let cols = vec!["x".to_string()];
        let rows = vec![vec![Value::Null]];
        let opts = OutputOpts {
            null_text: "<NULL>".to_string(),
            ..Default::default()
        };
        assert_eq!(render(&cols, &rows, &opts), "<NULL>\n");
    }

    #[test]
    fn empty_result_with_headers_returns_empty_in_list_mode() {
        // Match sqlite3 list mode: when there are no rows, emit nothing,
        // even with `.headers on`.
        let cols = vec!["x".to_string()];
        let rows: Vec<Vec<Value>> = vec![];
        let opts = OutputOpts {
            headers: true,
            ..Default::default()
        };
        assert_eq!(render(&cols, &rows, &opts), "");
    }
    #[test]
    fn rendered_byte_limit_is_cumulative_in_every_mode() {
        let cols = vec!["é".to_string(), "label".to_string()];
        let rows = vec![
            vec![v_text("short"), v_text("\"quoted\"")],
            vec![v_text("a longer value"), Value::Null],
        ];
        for mode in [
            "list", "tabs", "csv", "line", "column", "box", "markdown", "json",
        ] {
            let opts = OutputOpts {
                mode: OutputMode::parse(mode).unwrap(),
                headers: true,
                ..OutputOpts::default()
            };
            let expected = render(&cols, &rows, &opts);
            let mut stdout = String::from("prior");
            render_bounded(&mut stdout, &cols, &rows, &opts, expected.len() + 5).unwrap();
            assert_eq!(stdout, format!("prior{expected}"), "{mode}");

            let mut stdout = String::from("prior");
            let err =
                render_bounded(&mut stdout, &cols, &rows, &opts, expected.len() + 4).unwrap_err();
            assert!(err.contains("output exceeds byte cap"), "{mode}: {err}");
            assert_eq!(stdout, "prior", "{mode}");
        }
    }
}
