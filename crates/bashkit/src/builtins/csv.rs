//! CSV utilities builtin
//!
//! Non-standard builtin for querying and transforming CSV data.
//!
//! Usage:
//!   csv select name,age data.csv
//!   csv count data.csv
//!   csv headers data.csv
//!   csv filter age = 30 data.csv
//!   csv sort name data.csv
//!   echo "a,b\n1,2" | csv count

use async_trait::async_trait;

use super::{Builtin, Context, resolve_path};
use crate::error::Result;
use crate::interpreter::ExecResult;

/// csv builtin - CSV query and transformation utilities
pub struct Csv;

/// A parsed CSV table: header row (if any) + data rows.
struct CsvTable {
    headers: Option<Vec<String>>,
    rows: Vec<Vec<String>>,
}

/// Parse a single CSV line handling quoted fields.
/// Supports double-quote escaping (RFC 4180 style: "" inside quotes).
fn parse_csv_line(line: &str, delim: char) -> Vec<String> {
    let mut fields = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut chars = line.chars().peekable();

    while let Some(c) = chars.next() {
        if in_quotes {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    // escaped quote
                    chars.next();
                    current.push('"');
                } else {
                    in_quotes = false;
                }
            } else {
                current.push(c);
            }
        } else if c == '"' {
            in_quotes = true;
        } else if c == delim {
            fields.push(current.clone());
            current.clear();
        } else {
            current.push(c);
        }
    }
    fields.push(current);
    fields
}

/// Serialize a row back to CSV with proper quoting.
fn format_csv_row(fields: &[String], delim: char) -> String {
    fields
        .iter()
        .map(|f| {
            if f.contains(delim) || f.contains('"') || f.contains('\n') {
                format!("\"{}\"", f.replace('"', "\"\""))
            } else {
                f.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(&delim.to_string())
}

/// Parse CSV content into a table.
fn parse_csv(content: &str, delim: char, has_header: bool) -> CsvTable {
    let lines: Vec<&str> = content.lines().filter(|l| !l.is_empty()).collect();
    if lines.is_empty() {
        return CsvTable {
            headers: if has_header { Some(Vec::new()) } else { None },
            rows: Vec::new(),
        };
    }

    let (headers, data_start) = if has_header {
        (Some(parse_csv_line(lines[0], delim)), 1)
    } else {
        (None, 0)
    };

    let rows = lines[data_start..]
        .iter()
        .map(|l| parse_csv_line(l, delim))
        .collect();

    CsvTable { headers, rows }
}

/// Resolve a column specifier to a 0-based index.
/// Accepts numeric (1-based) or header name.
fn resolve_column(col: &str, headers: &Option<Vec<String>>) -> Option<usize> {
    if let Ok(n) = col.parse::<usize>() {
        if n == 0 {
            return None;
        }
        return Some(n - 1);
    }
    if let Some(hdrs) = headers {
        hdrs.iter().position(|h| h == col)
    } else {
        None
    }
}

/// Read input from file arg or stdin.
async fn read_input<'a>(
    ctx: &Context<'a>,
    file_arg: Option<&str>,
) -> std::result::Result<String, ExecResult> {
    if let Some(path_str) = file_arg {
        let path = resolve_path(ctx.cwd, path_str);
        match ctx.fs.read_file(&path).await {
            Ok(bytes) => Ok(String::from_utf8_lossy(&bytes).into_owned()),
            Err(_) => Err(ExecResult::err(
                format!("csv: cannot read '{}'\n", path_str),
                1,
            )),
        }
    } else if let Some(stdin) = ctx.stdin {
        Ok(stdin.to_string())
    } else {
        Err(ExecResult::err("csv: no input\n".to_string(), 1))
    }
}

#[async_trait]
impl Builtin for Csv {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        if ctx.args.is_empty() {
            return Ok(ExecResult::err(
                "csv: usage: csv <subcommand> [options] [file]\nSubcommands: select, count, headers, filter, sort\n".to_string(),
                1,
            ));
        }

        // Parse global options before subcommand dispatch.
        // We scan for -d DELIM and --no-header, collecting remaining args.
        let mut delim = ',';
        let mut has_header = true;
        let mut remaining: Vec<String> = Vec::new();
        let mut i = 0;
        while i < ctx.args.len() {
            match ctx.args[i].as_str() {
                "-d" => {
                    i += 1;
                    if i < ctx.args.len() {
                        let d = &ctx.args[i];
                        if d.len() == 1 {
                            delim = d.chars().next().unwrap_or(',');
                        } else if d == "\\t" || d == "tab" {
                            delim = '\t';
                        } else {
                            return Ok(ExecResult::err(
                                "csv: delimiter must be a single character\n".to_string(),
                                1,
                            ));
                        }
                    } else {
                        return Ok(ExecResult::err(
                            "csv: -d requires an argument\n".to_string(),
                            1,
                        ));
                    }
                }
                "--no-header" => {
                    has_header = false;
                }
                _ => {
                    remaining.push(ctx.args[i].clone());
                }
            }
            i += 1;
        }

        if remaining.is_empty() {
            return Ok(ExecResult::err("csv: missing subcommand\n".to_string(), 1));
        }

        let subcmd = remaining[0].as_str();
        let rest = &remaining[1..];

        match subcmd {
            "headers" => {
                let file_arg = rest.first().map(|s| s.as_str());
                let content = match read_input(&ctx, file_arg).await {
                    Ok(c) => c,
                    Err(e) => return Ok(e),
                };
                let table = parse_csv(&content, delim, has_header);
                if let Some(hdrs) = &table.headers {
                    let out = hdrs
                        .iter()
                        .enumerate()
                        .map(|(i, h)| format!("{}: {}", i + 1, h))
                        .collect::<Vec<_>>()
                        .join("\n");
                    Ok(ExecResult::ok(format!("{out}\n")))
                } else {
                    Ok(ExecResult::err(
                        "csv: no headers (use without --no-header)\n".to_string(),
                        1,
                    ))
                }
            }
            "count" => {
                let file_arg = rest.first().map(|s| s.as_str());
                let content = match read_input(&ctx, file_arg).await {
                    Ok(c) => c,
                    Err(e) => return Ok(e),
                };
                let table = parse_csv(&content, delim, has_header);
                Ok(ExecResult::ok(format!("{}\n", table.rows.len())))
            }
            "select" => {
                if rest.is_empty() {
                    return Ok(ExecResult::err(
                        "csv: select requires column specifiers\n".to_string(),
                        1,
                    ));
                }
                let col_spec = &rest[0];
                let file_arg = rest.get(1).map(|s| s.as_str());
                let content = match read_input(&ctx, file_arg).await {
                    Ok(c) => c,
                    Err(e) => return Ok(e),
                };
                let table = parse_csv(&content, delim, has_header);

                let col_names: Vec<&str> = col_spec.split(',').collect();
                let indices: Vec<usize> = col_names
                    .iter()
                    .filter_map(|c| resolve_column(c.trim(), &table.headers))
                    .collect();

                if indices.is_empty() {
                    return Ok(ExecResult::err("csv: no matching columns\n".to_string(), 1));
                }

                let mut out = String::new();
                // Output header row if present
                if let Some(hdrs) = &table.headers {
                    let selected: Vec<String> = indices
                        .iter()
                        .filter_map(|&i| hdrs.get(i).cloned())
                        .collect();
                    out.push_str(&format_csv_row(&selected, delim));
                    out.push('\n');
                }
                for row in &table.rows {
                    let selected: Vec<String> = indices
                        .iter()
                        .map(|&i| row.get(i).cloned().unwrap_or_default())
                        .collect();
                    out.push_str(&format_csv_row(&selected, delim));
                    out.push('\n');
                }
                Ok(ExecResult::ok(out))
            }
            "filter" => {
                // csv filter COLUMN OP VALUE [FILE]
                if rest.len() < 3 {
                    return Ok(ExecResult::err(
                        "csv: filter requires COLUMN OP VALUE\n".to_string(),
                        1,
                    ));
                }
                let col_name = &rest[0];
                let op = &rest[1];
                let value = &rest[2];
                let file_arg = rest.get(3).map(|s| s.as_str());

                let content = match read_input(&ctx, file_arg).await {
                    Ok(c) => c,
                    Err(e) => return Ok(e),
                };
                let table = parse_csv(&content, delim, has_header);

                let col_idx = match resolve_column(col_name, &table.headers) {
                    Some(i) => i,
                    None => {
                        return Ok(ExecResult::err(
                            format!("csv: unknown column '{}'\n", col_name),
                            1,
                        ));
                    }
                };

                let filtered: Vec<&Vec<String>> = table
                    .rows
                    .iter()
                    .filter(|row| {
                        let cell = row.get(col_idx).map(|s| s.as_str()).unwrap_or("");
                        match op.as_str() {
                            "=" | "==" => cell == value.as_str(),
                            "!=" => cell != value.as_str(),
                            "contains" => cell.contains(value.as_str()),
                            _ => false,
                        }
                    })
                    .collect();

                let mut out = String::new();
                if let Some(hdrs) = &table.headers {
                    out.push_str(&format_csv_row(hdrs, delim));
                    out.push('\n');
                }
                for row in &filtered {
                    out.push_str(&format_csv_row(row, delim));
                    out.push('\n');
                }
                Ok(ExecResult::ok(out))
            }
            "sort" => {
                if rest.is_empty() {
                    return Ok(ExecResult::err(
                        "csv: sort requires a column specifier\n".to_string(),
                        1,
                    ));
                }
                let col_name = &rest[0];
                let file_arg = rest.get(1).map(|s| s.as_str());

                let content = match read_input(&ctx, file_arg).await {
                    Ok(c) => c,
                    Err(e) => return Ok(e),
                };
                let table = parse_csv(&content, delim, has_header);

                let col_idx = match resolve_column(col_name, &table.headers) {
                    Some(i) => i,
                    None => {
                        return Ok(ExecResult::err(
                            format!("csv: unknown column '{}'\n", col_name),
                            1,
                        ));
                    }
                };

                let mut sorted_rows = table.rows.clone();
                sorted_rows.sort_by(|a, b| {
                    let va = a.get(col_idx).map(|s| s.as_str()).unwrap_or("");
                    let vb = b.get(col_idx).map(|s| s.as_str()).unwrap_or("");
                    // Try numeric sort first, fall back to string
                    match (va.parse::<f64>(), vb.parse::<f64>()) {
                        (Ok(na), Ok(nb)) => {
                            na.partial_cmp(&nb).unwrap_or(std::cmp::Ordering::Equal)
                        }
                        _ => va.cmp(vb),
                    }
                });

                let mut out = String::new();
                if let Some(hdrs) = &table.headers {
                    out.push_str(&format_csv_row(hdrs, delim));
                    out.push('\n');
                }
                for row in &sorted_rows {
                    out.push_str(&format_csv_row(row, delim));
                    out.push('\n');
                }
                Ok(ExecResult::ok(out))
            }
            _ => Ok(ExecResult::err(
                format!("csv: unknown subcommand '{}'\n", subcmd),
                1,
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Arc;

    use crate::fs::InMemoryFs;

    async fn run(args: &[&str], stdin: Option<&str>) -> ExecResult {
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let env = HashMap::new();
        let mut variables = HashMap::new();
        let mut cwd = PathBuf::from("/");
        let fs = Arc::new(InMemoryFs::new()) as Arc<dyn crate::fs::FileSystem>;
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs,
            stdin,
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
        };
        Csv.execute(ctx).await.unwrap()
    }

    async fn run_with_file(args: &[&str], filename: &str, content: &str) -> ExecResult {
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let env = HashMap::new();
        let mut variables = HashMap::new();
        let mut cwd = PathBuf::from("/");
        let fs = Arc::new(InMemoryFs::new()) as Arc<dyn crate::fs::FileSystem>;
        fs.write_file(&PathBuf::from(filename), content.as_bytes())
            .await
            .unwrap();
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs,
            stdin: None,
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
        };
        Csv.execute(ctx).await.unwrap()
    }

    #[tokio::test]
    async fn test_no_args() {
        let r = run(&[], None).await;
        assert_eq!(r.exit_code, 1);
        assert!(r.stderr.contains("usage"));
    }

    #[tokio::test]
    async fn test_unknown_subcommand() {
        let r = run(&["bogus"], Some("a,b\n1,2\n")).await;
        assert_eq!(r.exit_code, 1);
        assert!(r.stderr.contains("unknown subcommand"));
    }

    #[tokio::test]
    async fn test_count() {
        let r = run(&["count"], Some("name,age\nalice,30\nbob,25\n")).await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout.trim(), "2");
    }

    #[tokio::test]
    async fn test_headers() {
        let r = run(&["headers"], Some("name,age,city\nalice,30,NYC\n")).await;
        assert_eq!(r.exit_code, 0);
        assert!(r.stdout.contains("1: name"));
        assert!(r.stdout.contains("2: age"));
        assert!(r.stdout.contains("3: city"));
    }

    #[tokio::test]
    async fn test_select_by_name() {
        let r = run(&["select", "name"], Some("name,age\nalice,30\nbob,25\n")).await;
        assert_eq!(r.exit_code, 0);
        assert!(r.stdout.contains("name\n"));
        assert!(r.stdout.contains("alice\n"));
        assert!(r.stdout.contains("bob\n"));
    }

    #[tokio::test]
    async fn test_select_by_index() {
        let r = run(&["select", "2"], Some("name,age\nalice,30\nbob,25\n")).await;
        assert_eq!(r.exit_code, 0);
        assert!(r.stdout.contains("age\n"));
        assert!(r.stdout.contains("30\n"));
    }

    #[tokio::test]
    async fn test_filter_equals() {
        let r = run(
            &["filter", "name", "=", "alice"],
            Some("name,age\nalice,30\nbob,25\n"),
        )
        .await;
        assert_eq!(r.exit_code, 0);
        assert!(r.stdout.contains("alice"));
        assert!(!r.stdout.contains("bob"));
    }

    #[tokio::test]
    async fn test_filter_contains() {
        let r = run(
            &["filter", "name", "contains", "li"],
            Some("name,age\nalice,30\nbob,25\n"),
        )
        .await;
        assert_eq!(r.exit_code, 0);
        assert!(r.stdout.contains("alice"));
        assert!(!r.stdout.contains("bob"));
    }

    #[tokio::test]
    async fn test_filter_not_equals() {
        let r = run(
            &["filter", "name", "!=", "alice"],
            Some("name,age\nalice,30\nbob,25\n"),
        )
        .await;
        assert_eq!(r.exit_code, 0);
        assert!(!r.stdout.contains("alice"));
        assert!(r.stdout.contains("bob"));
    }

    #[tokio::test]
    async fn test_sort_string() {
        let r = run(
            &["sort", "name"],
            Some("name,age\ncharlie,20\nalice,30\nbob,25\n"),
        )
        .await;
        assert_eq!(r.exit_code, 0);
        let lines: Vec<&str> = r.stdout.lines().collect();
        assert_eq!(lines[0], "name,age");
        assert!(lines[1].starts_with("alice"));
        assert!(lines[2].starts_with("bob"));
        assert!(lines[3].starts_with("charlie"));
    }

    #[tokio::test]
    async fn test_sort_numeric() {
        let r = run(
            &["sort", "age"],
            Some("name,age\ncharlie,20\nalice,30\nbob,25\n"),
        )
        .await;
        assert_eq!(r.exit_code, 0);
        let lines: Vec<&str> = r.stdout.lines().collect();
        assert_eq!(lines[1], "charlie,20");
        assert_eq!(lines[2], "bob,25");
        assert_eq!(lines[3], "alice,30");
    }

    #[tokio::test]
    async fn test_quoted_fields() {
        let input = "name,bio\nalice,\"likes, commas\"\nbob,\"says \"\"hi\"\"\"\n";
        let r = run(&["select", "bio"], Some(input)).await;
        assert_eq!(r.exit_code, 0);
        assert!(r.stdout.contains("\"likes, commas\""));
        assert!(r.stdout.contains("\"says \"\"hi\"\"\""));
    }

    #[tokio::test]
    async fn test_custom_delimiter() {
        let r = run(
            &["-d", "\t", "count"],
            Some("name\tage\nalice\t30\nbob\t25\n"),
        )
        .await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout.trim(), "2");
    }

    #[tokio::test]
    async fn test_no_header_mode() {
        let r = run(&["--no-header", "count"], Some("alice,30\nbob,25\n")).await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout.trim(), "2");
    }

    #[tokio::test]
    async fn test_read_from_file() {
        let r = run_with_file(
            &["count", "/data.csv"],
            "/data.csv",
            "name,age\nalice,30\nbob,25\n",
        )
        .await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout.trim(), "2");
    }

    #[tokio::test]
    async fn test_filter_unknown_column() {
        let r = run(
            &["filter", "nonexistent", "=", "x"],
            Some("name,age\nalice,30\n"),
        )
        .await;
        assert_eq!(r.exit_code, 1);
        assert!(r.stderr.contains("unknown column"));
    }

    #[tokio::test]
    async fn test_select_no_columns_arg() {
        let r = run(&["select"], Some("a,b\n1,2\n")).await;
        assert_eq!(r.exit_code, 1);
    }

    #[tokio::test]
    async fn test_no_input() {
        let r = run(&["count"], None).await;
        assert_eq!(r.exit_code, 1);
        assert!(r.stderr.contains("no input"));
    }
}
