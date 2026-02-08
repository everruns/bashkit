//! nl builtin command - number lines of files

use async_trait::async_trait;

use super::{Builtin, Context};
use crate::error::Result;
use crate::interpreter::ExecResult;

/// The nl builtin - number lines of files.
///
/// Usage: nl [-b TYPE] [-n FORMAT] [-s SEP] [-i INCR] [-v START] [-w WIDTH] [FILE...]
///
/// Options:
///   -b TYPE    Body numbering type: a (all), t (non-empty, default), n (none)
///   -n FORMAT  Number format: ln (left-justified), rn (right-justified, default), rz (right-justified, zero-padded)
///   -s SEP     Separator string between number and line (default: TAB)
///   -i INCR    Line number increment (default: 1)
///   -v START   Starting line number (default: 1)
///   -w WIDTH   Number width (default: 6)
pub struct Nl;

#[derive(Clone, Copy, PartialEq)]
enum BodyType {
    All,
    NonEmpty,
    None,
}

#[derive(Clone, Copy)]
enum NumberFormat {
    LeftJustified,
    RightJustified,
    RightZero,
}

struct NlOptions {
    body_type: BodyType,
    format: NumberFormat,
    separator: String,
    increment: usize,
    start: usize,
    width: usize,
}

impl Default for NlOptions {
    fn default() -> Self {
        Self {
            body_type: BodyType::NonEmpty,
            format: NumberFormat::RightJustified,
            separator: "\t".to_string(),
            increment: 1,
            start: 1,
            width: 6,
        }
    }
}

fn parse_nl_args(args: &[String]) -> std::result::Result<(NlOptions, Vec<String>), String> {
    let mut opts = NlOptions::default();
    let mut files = Vec::new();
    let mut i = 0;

    while i < args.len() {
        let arg = &args[i];
        if arg == "-b" {
            i += 1;
            if i < args.len() {
                opts.body_type = match args[i].as_str() {
                    "a" => BodyType::All,
                    "t" => BodyType::NonEmpty,
                    "n" => BodyType::None,
                    other => return Err(format!("nl: invalid body numbering style: '{}'", other)),
                };
            }
        } else if let Some(val) = arg.strip_prefix("-b") {
            opts.body_type = match val {
                "a" => BodyType::All,
                "t" => BodyType::NonEmpty,
                "n" => BodyType::None,
                other => return Err(format!("nl: invalid body numbering style: '{}'", other)),
            };
        } else if arg == "-n" {
            i += 1;
            if i < args.len() {
                opts.format = match args[i].as_str() {
                    "ln" => NumberFormat::LeftJustified,
                    "rn" => NumberFormat::RightJustified,
                    "rz" => NumberFormat::RightZero,
                    other => return Err(format!("nl: invalid line numbering format: '{}'", other)),
                };
            }
        } else if let Some(val) = arg.strip_prefix("-n") {
            opts.format = match val {
                "ln" => NumberFormat::LeftJustified,
                "rn" => NumberFormat::RightJustified,
                "rz" => NumberFormat::RightZero,
                other => return Err(format!("nl: invalid line numbering format: '{}'", other)),
            };
        } else if arg == "-s" {
            i += 1;
            if i < args.len() {
                opts.separator = args[i].clone();
            }
        } else if let Some(val) = arg.strip_prefix("-s") {
            opts.separator = val.to_string();
        } else if arg == "-i" {
            i += 1;
            if i < args.len() {
                opts.increment = args[i]
                    .parse()
                    .map_err(|_| format!("nl: invalid line number increment: '{}'", args[i]))?;
            }
        } else if let Some(val) = arg.strip_prefix("-i") {
            opts.increment = val
                .parse()
                .map_err(|_| format!("nl: invalid line number increment: '{}'", val))?;
        } else if arg == "-v" {
            i += 1;
            if i < args.len() {
                opts.start = args[i]
                    .parse()
                    .map_err(|_| format!("nl: invalid starting line number: '{}'", args[i]))?;
            }
        } else if let Some(val) = arg.strip_prefix("-v") {
            opts.start = val
                .parse()
                .map_err(|_| format!("nl: invalid starting line number: '{}'", val))?;
        } else if arg == "-w" {
            i += 1;
            if i < args.len() {
                opts.width = args[i]
                    .parse()
                    .map_err(|_| format!("nl: invalid line number field width: '{}'", args[i]))?;
            }
        } else if let Some(val) = arg.strip_prefix("-w") {
            opts.width = val
                .parse()
                .map_err(|_| format!("nl: invalid line number field width: '{}'", val))?;
        } else if arg == "-" || !arg.starts_with('-') {
            files.push(arg.clone());
        }
        i += 1;
    }

    Ok((opts, files))
}

fn format_number(num: usize, format: NumberFormat, width: usize) -> String {
    match format {
        NumberFormat::LeftJustified => format!("{:<width$}", num, width = width),
        NumberFormat::RightJustified => format!("{:>width$}", num, width = width),
        NumberFormat::RightZero => format!("{:0>width$}", num, width = width),
    }
}

fn number_lines(text: &str, opts: &NlOptions, line_num: &mut usize) -> String {
    let mut output = String::new();

    for line in text.lines() {
        let should_number = match opts.body_type {
            BodyType::All => true,
            BodyType::NonEmpty => !line.is_empty(),
            BodyType::None => false,
        };

        if should_number {
            output.push_str(&format_number(*line_num, opts.format, opts.width));
            output.push_str(&opts.separator);
            output.push_str(line);
            output.push('\n');
            *line_num += opts.increment;
        } else {
            // No number, just indent with spaces
            output.push_str(&" ".repeat(opts.width));
            output.push_str(&opts.separator);
            output.push_str(line);
            output.push('\n');
        }
    }

    output
}

#[async_trait]
impl Builtin for Nl {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let (opts, files) = match parse_nl_args(ctx.args) {
            Ok(v) => v,
            Err(e) => return Ok(ExecResult::err(format!("{}\n", e), 1)),
        };

        let mut output = String::new();
        let mut line_num = opts.start;

        if files.is_empty() {
            // Read from stdin
            if let Some(stdin) = ctx.stdin {
                output.push_str(&number_lines(stdin, &opts, &mut line_num));
            }
        } else {
            for file in &files {
                if file == "-" {
                    if let Some(stdin) = ctx.stdin {
                        output.push_str(&number_lines(stdin, &opts, &mut line_num));
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
                            output.push_str(&number_lines(&text, &opts, &mut line_num));
                        }
                        Err(e) => {
                            return Ok(ExecResult::err(format!("nl: {}: {}\n", file, e), 1));
                        }
                    }
                }
            }
        }

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

    use crate::fs::{FileSystem, InMemoryFs};

    async fn run_nl(args: &[&str], stdin: Option<&str>) -> ExecResult {
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

        Nl.execute(ctx).await.unwrap()
    }

    async fn run_nl_with_fs(
        args: &[&str],
        stdin: Option<&str>,
        files: &[(&str, &[u8])],
    ) -> ExecResult {
        let fs = Arc::new(InMemoryFs::new());
        for (path, content) in files {
            fs.write_file(std::path::Path::new(path), content)
                .await
                .unwrap();
        }
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

        Nl.execute(ctx).await.unwrap()
    }

    #[tokio::test]
    async fn test_nl_basic() {
        let result = run_nl(&[], Some("hello\nworld\n")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "     1\thello\n     2\tworld\n");
    }

    #[tokio::test]
    async fn test_nl_default_skips_empty() {
        let result = run_nl(&[], Some("hello\n\nworld\n")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "     1\thello\n      \t\n     2\tworld\n");
    }

    #[tokio::test]
    async fn test_nl_all_lines() {
        let result = run_nl(&["-b", "a"], Some("hello\n\nworld\n")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "     1\thello\n     2\t\n     3\tworld\n");
    }

    #[tokio::test]
    async fn test_nl_no_numbering() {
        let result = run_nl(&["-b", "n"], Some("hello\nworld\n")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "      \thello\n      \tworld\n");
    }

    #[tokio::test]
    async fn test_nl_left_justified() {
        let result = run_nl(&["-n", "ln"], Some("hello\nworld\n")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "1     \thello\n2     \tworld\n");
    }

    #[tokio::test]
    async fn test_nl_right_zero() {
        let result = run_nl(&["-n", "rz"], Some("hello\nworld\n")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "000001\thello\n000002\tworld\n");
    }

    #[tokio::test]
    async fn test_nl_custom_separator() {
        let result = run_nl(&["-s", ": "], Some("hello\nworld\n")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "     1: hello\n     2: world\n");
    }

    #[tokio::test]
    async fn test_nl_custom_increment() {
        let result = run_nl(&["-i", "2"], Some("a\nb\nc\n")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "     1\ta\n     3\tb\n     5\tc\n");
    }

    #[tokio::test]
    async fn test_nl_custom_start() {
        let result = run_nl(&["-v", "10"], Some("a\nb\nc\n")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "    10\ta\n    11\tb\n    12\tc\n");
    }

    #[tokio::test]
    async fn test_nl_custom_width() {
        let result = run_nl(&["-w", "3"], Some("a\nb\n")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "  1\ta\n  2\tb\n");
    }

    #[tokio::test]
    async fn test_nl_combined_options() {
        let result = run_nl(
            &[
                "-b", "a", "-n", "rz", "-w", "4", "-s", " ", "-v", "5", "-i", "3",
            ],
            Some("x\n\ny\n"),
        )
        .await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "0005 x\n0008 \n0011 y\n");
    }

    #[tokio::test]
    async fn test_nl_empty_input() {
        let result = run_nl(&[], Some("")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "");
    }

    #[tokio::test]
    async fn test_nl_no_stdin() {
        let result = run_nl(&[], None).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "");
    }

    #[tokio::test]
    async fn test_nl_from_file() {
        let result =
            run_nl_with_fs(&["/test.txt"], None, &[("/test.txt", b"one\ntwo\nthree\n")]).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "     1\tone\n     2\ttwo\n     3\tthree\n");
    }

    #[tokio::test]
    async fn test_nl_file_not_found() {
        let result = run_nl(&["/nonexistent"], None).await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("nl:"));
    }

    #[tokio::test]
    async fn test_nl_invalid_body_type() {
        let result = run_nl(&["-b", "x"], Some("test\n")).await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("invalid body numbering style"));
    }

    #[tokio::test]
    async fn test_nl_invalid_format() {
        let result = run_nl(&["-n", "xx"], Some("test\n")).await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("invalid line numbering format"));
    }

    #[tokio::test]
    async fn test_nl_single_line() {
        let result = run_nl(&[], Some("hello\n")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "     1\thello\n");
    }

    #[tokio::test]
    async fn test_nl_stdin_dash() {
        let result = run_nl(&["-"], Some("hello\nworld\n")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "     1\thello\n     2\tworld\n");
    }

    #[tokio::test]
    async fn test_nl_multiple_files() {
        let result = run_nl_with_fs(
            &["/a.txt", "/b.txt"],
            None,
            &[("/a.txt", b"one\ntwo\n"), ("/b.txt", b"three\nfour\n")],
        )
        .await;
        assert_eq!(result.exit_code, 0);
        // Line numbers continue across files
        assert_eq!(
            result.stdout,
            "     1\tone\n     2\ttwo\n     3\tthree\n     4\tfour\n"
        );
    }

    #[tokio::test]
    async fn test_nl_attached_args() {
        // Test -ba, -nrz, -w4 (attached value form)
        let result = run_nl(&["-ba", "-nrz", "-w4"], Some("x\ny\n")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "0001\tx\n0002\ty\n");
    }
}
