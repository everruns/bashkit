//! Output formatting for SQLite query results.
//!
//! Mirrors the `sqlite3` shell modes that LLM agents and humans actually use:
//! list (default), csv, tabs, line, column/box, json, markdown.

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

/// Render a single value with the given NULL placeholder. NULL is the only
/// value that depends on `null_text`; all others use turso's `Display`.
fn render_value(v: &Value, null_text: &str) -> String {
    match v {
        Value::Null => null_text.to_string(),
        _ => format!("{v}"),
    }
}

/// CSV-quote per RFC 4180 when the field contains the separator, a quote, CR,
/// or LF. Empty fields are written as-is (sqlite3 matches this).
fn csv_quote(field: &str, sep: &str) -> String {
    let needs_quote =
        field.contains(sep) || field.contains('"') || field.contains('\n') || field.contains('\r');
    if !needs_quote {
        return field.to_string();
    }
    let escaped = field.replace('"', "\"\"");
    format!("\"{escaped}\"")
}

/// JSON-encode a single Value in the same way sqlite3's `json` mode does:
/// numbers are emitted unquoted, NULLs become `null`, blobs become a hex
/// string, and text/everything else is JSON-encoded.
fn json_value(v: &Value) -> String {
    match v {
        Value::Null => "null".to_string(),
        Value::Numeric(Numeric::Integer(i)) => i.to_string(),
        Value::Numeric(Numeric::Float(f)) => {
            let f64v: f64 = (*f).into();
            if f64v.is_finite() {
                // Prefer compact representation matching sqlite3.
                let s = format!("{f64v}");
                // Force a `.0` suffix when serialising whole numbers so we
                // can round-trip type information.
                if s.contains('.') || s.contains('e') || s.contains('E') {
                    s
                } else {
                    format!("{s}.0")
                }
            } else {
                "null".to_string()
            }
        }
        Value::Text(t) => serde_json::to_string(t.as_str()).unwrap_or_else(|_| "\"\"".to_string()),
        Value::Blob(b) => {
            // sqlite3's CLI emits blobs as hex strings in json mode.
            let mut hex = String::with_capacity(b.len() * 2 + 2);
            hex.push('"');
            for byte in b {
                use std::fmt::Write as _;
                let _ = write!(hex, "{byte:02x}");
            }
            hex.push('"');
            hex
        }
    }
}

/// Render a complete result set into the chosen output mode.
///
/// Statements that produce no columns at all (CREATE / INSERT / UPDATE / DELETE)
/// render to the empty string. Statements with columns but zero rows also
/// render to the empty string in row-oriented modes — matching sqlite3's
/// behaviour, which never prints a lonely header row in list mode.
pub(super) fn render(column_names: &[String], rows: &[Vec<Value>], opts: &OutputOpts) -> String {
    if column_names.is_empty() {
        return String::new();
    }
    if rows.is_empty() {
        // json mode is the one exception: an empty array is meaningful.
        if matches!(opts.mode, OutputMode::Json) {
            return "[]\n".to_string();
        }
        return String::new();
    }
    match opts.mode {
        OutputMode::List => render_separated(column_names, rows, &opts.separator, opts),
        OutputMode::Tabs => render_separated(column_names, rows, "\t", opts),
        OutputMode::Csv => render_csv(column_names, rows, &opts.separator, opts),
        OutputMode::Line => render_line(column_names, rows, opts),
        OutputMode::Column => render_column(column_names, rows, opts),
        OutputMode::Box => render_box(column_names, rows, opts),
        OutputMode::Json => render_json(column_names, rows, opts),
        OutputMode::Markdown => render_markdown(column_names, rows, opts),
    }
}

fn render_separated(cols: &[String], rows: &[Vec<Value>], sep: &str, opts: &OutputOpts) -> String {
    let mut out = String::new();
    if opts.headers {
        out.push_str(&cols.join(sep));
        out.push('\n');
    }
    for row in rows {
        let line: Vec<String> = row
            .iter()
            .map(|v| render_value(v, &opts.null_text))
            .collect();
        out.push_str(&line.join(sep));
        out.push('\n');
    }
    out
}

fn render_csv(cols: &[String], rows: &[Vec<Value>], sep: &str, opts: &OutputOpts) -> String {
    // sqlite3 always quotes header in csv mode regardless of headers setting,
    // but only if `.headers on` is set we emit it at all.
    let mut out = String::new();
    if opts.headers {
        let line: Vec<String> = cols.iter().map(|c| csv_quote(c, sep)).collect();
        out.push_str(&line.join(sep));
        out.push('\n');
    }
    for row in rows {
        let line: Vec<String> = row
            .iter()
            .map(|v| {
                let s = render_value(v, &opts.null_text);
                csv_quote(&s, sep)
            })
            .collect();
        out.push_str(&line.join(sep));
        out.push('\n');
    }
    out
}

fn render_line(cols: &[String], rows: &[Vec<Value>], opts: &OutputOpts) -> String {
    let mut out = String::new();
    let max_name = cols.iter().map(|c| c.len()).max().unwrap_or(0);
    for (i, row) in rows.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        for (col, v) in cols.iter().zip(row.iter()) {
            let _ = std::fmt::Write::write_fmt(
                &mut out,
                format_args!(
                    "{col:>width$} = {val}\n",
                    col = col,
                    width = max_name,
                    val = render_value(v, &opts.null_text)
                ),
            );
        }
    }
    out
}

fn col_widths(cols: &[String], rows: &[Vec<Value>], opts: &OutputOpts) -> Vec<usize> {
    let mut widths: Vec<usize> = cols.iter().map(|c| c.chars().count()).collect();
    for row in rows {
        for (i, v) in row.iter().enumerate() {
            let s = render_value(v, &opts.null_text);
            if let Some(w) = widths.get_mut(i) {
                *w = (*w).max(s.chars().count());
            }
        }
    }
    widths
}

fn render_column(cols: &[String], rows: &[Vec<Value>], opts: &OutputOpts) -> String {
    let widths = col_widths(cols, rows, opts);
    let mut out = String::new();
    if opts.headers {
        let pieces: Vec<String> = cols
            .iter()
            .zip(&widths)
            .map(|(c, w)| format!("{c:<w$}", c = c, w = *w))
            .collect();
        out.push_str(&pieces.join("  "));
        out.push('\n');
        let dashes: Vec<String> = widths.iter().map(|w| "-".repeat(*w)).collect();
        out.push_str(&dashes.join("  "));
        out.push('\n');
    }
    for row in rows {
        let pieces: Vec<String> = row
            .iter()
            .zip(&widths)
            .map(|(v, w)| {
                let s = render_value(v, &opts.null_text);
                format!("{s:<w$}", s = s, w = *w)
            })
            .collect();
        out.push_str(&pieces.join("  "));
        out.push('\n');
    }
    out
}

fn render_box(cols: &[String], rows: &[Vec<Value>], opts: &OutputOpts) -> String {
    let widths = col_widths(cols, rows, opts);
    let mut out = String::new();
    let top = widths
        .iter()
        .map(|w| "─".repeat(w + 2))
        .collect::<Vec<_>>()
        .join("┬");
    out.push_str(&format!("┌{top}┐\n"));
    let header_pieces: Vec<String> = cols
        .iter()
        .zip(&widths)
        .map(|(c, w)| format!(" {c:<w$} ", c = c, w = *w))
        .collect();
    out.push_str(&format!("│{}│\n", header_pieces.join("│")));
    let mid = widths
        .iter()
        .map(|w| "─".repeat(w + 2))
        .collect::<Vec<_>>()
        .join("┼");
    out.push_str(&format!("├{mid}┤\n"));
    for row in rows {
        let pieces: Vec<String> = row
            .iter()
            .zip(&widths)
            .map(|(v, w)| {
                let s = render_value(v, &opts.null_text);
                format!(" {s:<w$} ", s = s, w = *w)
            })
            .collect();
        out.push_str(&format!("│{}│\n", pieces.join("│")));
    }
    let bot = widths
        .iter()
        .map(|w| "─".repeat(w + 2))
        .collect::<Vec<_>>()
        .join("┴");
    out.push_str(&format!("└{bot}┘\n"));
    out
}

fn render_json(cols: &[String], rows: &[Vec<Value>], _opts: &OutputOpts) -> String {
    let mut out = String::from("[");
    for (i, row) in rows.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push('{');
        for (j, (c, v)) in cols.iter().zip(row.iter()).enumerate() {
            if j > 0 {
                out.push(',');
            }
            let key = serde_json::to_string(c).unwrap_or_else(|_| "\"\"".to_string());
            out.push_str(&key);
            out.push(':');
            out.push_str(&json_value(v));
        }
        out.push('}');
    }
    out.push_str("]\n");
    out
}

fn render_markdown(cols: &[String], rows: &[Vec<Value>], opts: &OutputOpts) -> String {
    let widths = col_widths(cols, rows, opts);
    let mut out = String::new();
    let header_pieces: Vec<String> = cols
        .iter()
        .zip(&widths)
        .map(|(c, w)| format!(" {c:<w$} ", c = c, w = *w))
        .collect();
    out.push_str(&format!("|{}|\n", header_pieces.join("|")));
    let sep_pieces: Vec<String> = widths.iter().map(|w| "-".repeat(w + 2)).collect();
    out.push_str(&format!("|{}|\n", sep_pieces.join("|")));
    for row in rows {
        let pieces: Vec<String> = row
            .iter()
            .zip(&widths)
            .map(|(v, w)| {
                let s = render_value(v, &opts.null_text);
                format!(" {s:<w$} ", s = s, w = *w)
            })
            .collect();
        out.push_str(&format!("|{}|\n", pieces.join("|")));
    }
    out
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
}
