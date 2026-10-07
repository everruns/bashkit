//! fold builtin command - wrap lines at specified width
//!
//! Decision: a port of GNU fold's column state machine. Backspace moves one
//! column left, CR resets to column 0, TAB advances to the next multiple of
//! 8, and a character that alone overflows an empty line is kept on it. Each
//! char counts one column (no wide-char widths); `-b` counts UTF-8 bytes.

use async_trait::async_trait;

use super::arg_parser::{OptArg, gnu_getopt};
use super::{Builtin, BuiltinHelper, Context, read_text_file, resolve_path};
use crate::error::Result;
use crate::interpreter::ExecResult;

/// The fold builtin command.
///
/// Usage: fold [-w width] [-s] [-b] [FILE...]
///
/// Options:
///   -w width  Wrap at width columns (default 80)
///   -s        Break at spaces (word boundary)
///   -b        Count bytes instead of columns
pub struct Fold;

impl BuiltinHelper for Fold {
    const NAME: &'static str = "fold";
}

const TAB_WIDTH: usize = 8;

fn adjust_column(column: usize, c: char, count_bytes: bool) -> usize {
    if count_bytes {
        return column + c.len_utf8();
    }
    match c {
        '\x08' => column.saturating_sub(1),
        '\r' => 0,
        '\t' => column + TAB_WIDTH - column % TAB_WIDTH,
        _ => column + 1,
    }
}

fn fold_text(text: &str, width: usize, break_spaces: bool, count_bytes: bool, out: &mut String) {
    let mut line: Vec<char> = Vec::new();
    let mut column = 0usize;
    for c in text.chars() {
        if c == '\n' {
            out.extend(line.drain(..));
            out.push('\n');
            column = 0;
            continue;
        }
        loop {
            column = adjust_column(column, c, count_bytes);
            if column <= width {
                line.push(c);
                break;
            }
            if break_spaces && let Some(blank) = line.iter().rposition(|&b| b == ' ' || b == '\t') {
                let rest = line.split_off(blank + 1);
                out.extend(line.drain(..));
                out.push('\n');
                line = rest;
                column = line
                    .iter()
                    .fold(0, |col, &ch| adjust_column(col, ch, count_bytes));
                continue;
            }
            if line.is_empty() {
                line.push(c);
                break;
            }
            out.extend(line.drain(..));
            out.push('\n');
            column = 0;
        }
    }
    out.extend(line);
}

/// Rewrite the obsolete `-NUM` form into `-wNUM`.
fn normalize_args(args: &[String]) -> Vec<String> {
    let mut out = Vec::with_capacity(args.len());
    let mut done = false;
    for a in args {
        if !done && a == "--" {
            done = true;
        }
        let digits = a.strip_prefix('-').filter(|d| !d.is_empty());
        match digits {
            Some(d) if !done && d.bytes().all(|b| b.is_ascii_digit()) => {
                out.push(format!("-w{d}"));
            }
            _ => out.push(a.clone()),
        }
    }
    out
}

#[async_trait]
impl Builtin for Fold {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        if let Some(r) = Self::check_help(
            ctx.args,
            "Usage: fold [OPTION]... [FILE]...\nWrap each input line to fit in specified width.\n\n  -b, --bytes\tcount bytes rather than columns\n  -s, --spaces\tbreak at spaces\n  -w, --width=WIDTH\tuse WIDTH columns instead of 80\n  --help\tdisplay this help and exit\n  --version\toutput version information and exit\n",
            Some("fold (bashkit) 0.1"),
        ) {
            return Ok(r);
        }
        let args = normalize_args(ctx.args);
        let (opts, mut files) = match gnu_getopt(
            "fold",
            &args,
            "bsw:",
            &[
                ("bytes", OptArg::No, 'b'),
                ("spaces", OptArg::No, 's'),
                ("width", OptArg::Required, 'w'),
            ],
            true,
            1,
        ) {
            Ok(v) => v,
            Err(e) => return Ok(e),
        };
        let mut width: usize = 80;
        let mut break_spaces = false;
        let mut count_bytes = false;
        for o in opts {
            match o.key {
                'b' => count_bytes = true,
                's' => break_spaces = true,
                _ => {
                    let val = o.value.unwrap_or_default();
                    width = match val.parse::<usize>() {
                        Ok(w) if w > 0 => w,
                        _ => {
                            return Ok(Self::err(format!("invalid number of columns: '{val}'"), 1));
                        }
                    };
                }
            }
        }
        if files.is_empty() {
            files.push("-".to_string());
        }

        let mut output = String::new();
        for file in &files {
            if file == "-" {
                let stdin = ctx.stdin.map(ToString::to_string).unwrap_or_default();
                fold_text(&stdin, width, break_spaces, count_bytes, &mut output);
                continue;
            }
            let path = resolve_path(ctx.cwd, file);
            match read_text_file(ctx.fs.as_ref(), &path, "fold").await {
                Ok(text) => fold_text(&text, width, break_spaces, count_bytes, &mut output),
                Err(_) => {
                    return Ok(Self::err_path(file, "No such file or directory", 1));
                }
            }
        }

        Ok(ExecResult::ok(output))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::InMemoryFs;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Arc;

    async fn run_fold(args: &[&str], stdin: Option<&str>) -> ExecResult {
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
            stdin: crate::builtins::test_stream_opt(stdin),
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
        };
        Fold.execute(ctx).await.expect("fold failed")
    }

    #[tokio::test]
    async fn test_fold_default_width() {
        let long = "a".repeat(100);
        let result = run_fold(&[], Some(&long)).await;
        assert_eq!(result.exit_code, 0);
        let lines: Vec<&str> = result.stdout.lines().collect();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].len(), 80);
        assert_eq!(lines[1].len(), 20);
    }

    #[tokio::test]
    async fn test_fold_custom_width() {
        let result = run_fold(&["-w", "10"], Some("abcdefghijklmno")).await;
        assert_eq!(result.exit_code, 0);
        let lines: Vec<&str> = result.stdout.lines().collect();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0], "abcdefghij");
        assert_eq!(lines[1], "klmno");
    }

    #[tokio::test]
    async fn test_fold_break_at_spaces() {
        let result = run_fold(&["-w", "15", "-s"], Some("hello world this is a test")).await;
        assert_eq!(result.exit_code, 0);
        // Should break at spaces, not mid-word
        for line in result.stdout.lines() {
            assert!(line.len() <= 15, "Line too long: '{}'", line);
        }
    }

    #[tokio::test]
    async fn test_fold_short_line_unchanged() {
        let result = run_fold(&["-w", "80"], Some("short line\n")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "short line\n");
    }

    #[tokio::test]
    async fn test_fold_unknown_option() {
        let result = run_fold(&["-Q"], Some("hello")).await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("invalid option -- 'Q'"));
    }

    #[tokio::test]
    async fn test_fold_empty_input() {
        let result = run_fold(&[], Some("")).await;
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn test_fold_tab_counts_to_next_stop() {
        let result = run_fold(&["-w", "4"], Some("\tab\n")).await;
        assert_eq!(result.stdout, "\t\nab\n");
    }

    #[tokio::test]
    async fn test_fold_backspace_moves_left() {
        let result = run_fold(&["-w", "3"], Some("\x08abcde\n")).await;
        assert_eq!(result.stdout, "\x08abc\nde\n");
    }

    #[tokio::test]
    async fn test_fold_spaces_breaks_after_blank() {
        let result = run_fold(&["-s", "-w", "6"], Some("ab cd efgh\n")).await;
        assert_eq!(result.stdout, "ab cd \nefgh\n");
    }

    #[tokio::test]
    async fn test_fold_obsolete_width() {
        let result = run_fold(&["-3"], Some("abcdef\n")).await;
        assert_eq!(result.stdout, "abc\ndef\n");
    }

    #[tokio::test]
    async fn test_fold_invalid_width() {
        let result = run_fold(&["-w", "0"], Some("a\n")).await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("invalid number of columns: '0'"));
    }
}
