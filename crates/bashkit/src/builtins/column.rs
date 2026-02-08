//! column builtin command - columnate lists

use async_trait::async_trait;

use super::{Builtin, Context};
use crate::error::Result;
use crate::interpreter::ExecResult;

/// The column builtin - format input into columns.
///
/// Usage: column [-t] [-s SEP] [-o SEP] [FILE...]
///
/// Options:
///   -t         Create a table (determine columns from input)
///   -s SEP     Specify input delimiter for -t mode (default: whitespace)
///   -o SEP     Specify output delimiter for -t mode (default: two spaces)
pub struct Column;

struct ColumnOptions {
    table: bool,
    input_sep: Option<String>,
    output_sep: String,
}

fn parse_column_args(args: &[String]) -> (ColumnOptions, Vec<String>) {
    let mut opts = ColumnOptions {
        table: false,
        input_sep: None,
        output_sep: "  ".to_string(),
    };
    let mut files = Vec::new();
    let mut i = 0;

    while i < args.len() {
        let arg = &args[i];
        if arg == "-t" {
            opts.table = true;
        } else if arg == "-s" {
            i += 1;
            if i < args.len() {
                opts.input_sep = Some(args[i].clone());
            }
        } else if let Some(s) = arg.strip_prefix("-s") {
            opts.input_sep = Some(s.to_string());
        } else if arg == "-o" {
            i += 1;
            if i < args.len() {
                opts.output_sep = args[i].clone();
            }
        } else if let Some(o) = arg.strip_prefix("-o") {
            opts.output_sep = o.to_string();
        } else if !arg.starts_with('-') {
            files.push(arg.clone());
        }
        i += 1;
    }

    (opts, files)
}

fn format_table(text: &str, opts: &ColumnOptions) -> String {
    let lines: Vec<&str> = text.lines().collect();
    if lines.is_empty() {
        return String::new();
    }

    // Split each line into fields
    let rows: Vec<Vec<&str>> = lines
        .iter()
        .map(|line| {
            if let Some(ref sep) = opts.input_sep {
                line.split(sep.as_str()).collect()
            } else {
                line.split_whitespace().collect()
            }
        })
        .collect();

    // Determine max columns and column widths
    let max_cols = rows.iter().map(|r| r.len()).max().unwrap_or(0);
    let mut widths = vec![0usize; max_cols];

    for row in &rows {
        for (j, field) in row.iter().enumerate() {
            widths[j] = widths[j].max(field.len());
        }
    }

    // Format each row
    let mut output = String::new();
    for row in &rows {
        for (j, field) in row.iter().enumerate() {
            if j > 0 {
                output.push_str(&opts.output_sep);
            }
            if j < row.len() - 1 {
                // Left-align, pad to column width (except last column)
                output.push_str(&format!("{:<width$}", field, width = widths[j]));
            } else {
                // Last column: no padding
                output.push_str(field);
            }
        }
        output.push('\n');
    }

    output
}

fn format_columns(text: &str) -> String {
    // Simple passthrough when -t is not specified
    // Real `column` fills columns, but for sandbox, passthrough is reasonable
    let mut output = String::new();
    for line in text.lines() {
        output.push_str(line);
        output.push('\n');
    }
    output
}

#[async_trait]
impl Builtin for Column {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let (opts, files) = parse_column_args(ctx.args);

        // Collect all input
        let mut input = String::new();

        if files.is_empty() {
            if let Some(stdin) = ctx.stdin {
                input.push_str(stdin);
            }
        } else {
            for file in &files {
                if file == "-" {
                    if let Some(stdin) = ctx.stdin {
                        input.push_str(stdin);
                    }
                } else {
                    let path = if file.starts_with('/') {
                        std::path::PathBuf::from(file)
                    } else {
                        ctx.cwd.join(file)
                    };

                    match ctx.fs.read_file(&path).await {
                        Ok(content) => {
                            let text = String::from_utf8_lossy(&content);
                            input.push_str(&text);
                        }
                        Err(e) => {
                            return Ok(ExecResult::err(format!("column: {}: {}\n", file, e), 1));
                        }
                    }
                }
            }
        }

        let output = if opts.table {
            format_table(&input, &opts)
        } else {
            format_columns(&input)
        };

        Ok(ExecResult::ok(output))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Arc;

    use crate::fs::InMemoryFs;

    async fn run_column(args: &[&str], stdin: Option<&str>) -> ExecResult {
        let fs = Arc::new(InMemoryFs::new());
        let mut variables = HashMap::new();
        let env = HashMap::new();
        let mut cwd = PathBuf::from("/");

        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
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

        Column.execute(ctx).await.unwrap()
    }

    #[tokio::test]
    async fn test_column_table_basic() {
        let result = run_column(&["-t"], Some("a b c\nfoo bar baz\n")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "a    b    c\nfoo  bar  baz\n");
    }

    #[tokio::test]
    async fn test_column_table_custom_input_sep() {
        let result = run_column(&["-t", "-s", ","], Some("a,b,c\nfoo,bar,baz\n")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "a    b    c\nfoo  bar  baz\n");
    }

    #[tokio::test]
    async fn test_column_table_custom_output_sep() {
        let result = run_column(&["-t", "-o", " | "], Some("a b c\nfoo bar baz\n")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "a   | b   | c\nfoo | bar | baz\n");
    }

    #[tokio::test]
    async fn test_column_table_uneven_rows() {
        let result = run_column(&["-t"], Some("a b c\nx y\n")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "a  b  c\nx  y\n");
    }

    #[tokio::test]
    async fn test_column_passthrough() {
        let result = run_column(&[], Some("hello\nworld\n")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "hello\nworld\n");
    }

    #[tokio::test]
    async fn test_column_empty_input() {
        let result = run_column(&["-t"], Some("")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "");
    }

    #[tokio::test]
    async fn test_column_single_column() {
        let result = run_column(&["-t"], Some("alpha\nbeta\ngamma\n")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "alpha\nbeta\ngamma\n");
    }

    #[tokio::test]
    async fn test_column_colon_delimiter() {
        let result = run_column(
            &["-t", "-s", ":"],
            Some("root:0:root\nnobody:65534:nobody\n"),
        )
        .await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(
            result.stdout,
            "root    0      root\nnobody  65534  nobody\n"
        );
    }

    #[tokio::test]
    async fn test_column_no_stdin() {
        let result = run_column(&["-t"], None).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "");
    }

    #[tokio::test]
    async fn test_column_file_not_found() {
        let result = run_column(&["/nonexistent"], None).await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("column:"));
    }
}
