//! comm builtin command - compare two sorted files line by line
//!
//! Decision: options follow GNU comm (`-123z`, `--output-delimiter`,
//! `--total`, `--zero-terminated`); `--check-order`/`--nocheck-order` are
//! accepted but order is never checked.

use async_trait::async_trait;

use super::arg_parser::{OptArg, gnu_getopt};
use super::{Builtin, BuiltinHelper, Context, read_text_file};
use crate::error::Result;
use crate::fs::vfs_join;
use crate::interpreter::ExecResult;

/// The comm builtin - compare two sorted files line by line.
///
/// Usage: comm [-123z] [--output-delimiter=STR] [--total] FILE1 FILE2
///
/// Options:
///   -1   Suppress lines unique to FILE1
///   -2   Suppress lines unique to FILE2
///   -3   Suppress lines that appear in both files
///   -z   Line delimiter is NUL, not newline
pub struct Comm;

impl BuiltinHelper for Comm {
    const NAME: &'static str = "comm";
}

struct CommOptions {
    suppress: [bool; 3],
    delimiter: String,
    total: bool,
    zero_terminated: bool,
}

#[allow(clippy::result_large_err)]
fn parse_comm_args(args: &[String]) -> std::result::Result<(CommOptions, Vec<String>), ExecResult> {
    let (parsed, files) = gnu_getopt(
        "comm",
        args,
        "123z",
        &[
            ("check-order", OptArg::No, 'c'),
            ("nocheck-order", OptArg::No, 'C'),
            ("output-delimiter", OptArg::Required, 'o'),
            ("total", OptArg::No, 't'),
            ("zero-terminated", OptArg::No, 'z'),
        ],
        true,
        1,
    )?;
    let mut opts = CommOptions {
        suppress: [false; 3],
        delimiter: "\t".to_string(),
        total: false,
        zero_terminated: false,
    };
    for o in parsed {
        match o.key {
            '1' => opts.suppress[0] = true,
            '2' => opts.suppress[1] = true,
            '3' => opts.suppress[2] = true,
            'z' => opts.zero_terminated = true,
            't' => opts.total = true,
            'o' => {
                let d = o.value.unwrap_or_default();
                // GNU: an empty delimiter means a NUL byte.
                opts.delimiter = if d.is_empty() { "\0".to_string() } else { d };
            }
            _ => {}
        }
    }
    Ok((opts, files))
}

fn records(text: &str, sep: char) -> Vec<String> {
    if text.is_empty() {
        return Vec::new();
    }
    text.strip_suffix(sep)
        .unwrap_or(text)
        .split(sep)
        .map(str::to_string)
        .collect()
}

#[async_trait]
impl Builtin for Comm {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        if let Some(r) = Self::check_help(
            ctx.args,
            "Usage: comm [OPTION]... FILE1 FILE2\nCompare two sorted files line by line.\n\n  -1\t\tsuppress column 1 (lines unique to FILE1)\n  -2\t\tsuppress column 2 (lines unique to FILE2)\n  -3\t\tsuppress column 3 (lines that appear in both files)\n  --output-delimiter=STR\tseparate columns with STR\n  --total\toutput a summary\n  -z, --zero-terminated\tline delimiter is NUL, not newline\n  --help\tdisplay this help and exit\n  --version\toutput version information and exit\n",
            Some("comm (bashkit) 0.1"),
        ) {
            return Ok(r);
        }
        let (opts, files) = match parse_comm_args(ctx.args) {
            Ok(v) => v,
            Err(e) => return Ok(e),
        };

        match files.len() {
            0 => return Ok(Self::err("missing operand", 1)),
            1 => {
                return Ok(Self::err(
                    &format!("missing operand after '{}'", files[0]),
                    1,
                ));
            }
            2 => {}
            _ => return Ok(Self::err(&format!("extra operand '{}'", files[2]), 1)),
        }
        let sep = if opts.zero_terminated { '\0' } else { '\n' };

        let mut inputs: Vec<Vec<String>> = Vec::with_capacity(2);
        for file in &files {
            let text = if file == "-" {
                ctx.stdin.map(ToString::to_string).unwrap_or_default()
            } else {
                let path = if file.starts_with('/') {
                    std::path::PathBuf::from(file)
                } else {
                    vfs_join(ctx.cwd, file)
                };
                match read_text_file(&*ctx.fs, &path, "comm").await {
                    Ok(text) => text,
                    Err(e) => return Ok(e),
                }
            };
            inputs.push(records(&text, sep));
        }
        let (lines1, lines2) = (&inputs[0], &inputs[1]);

        let d = &opts.delimiter;
        let prefixes = [
            String::new(),
            if opts.suppress[0] {
                String::new()
            } else {
                d.clone()
            },
            d.repeat(usize::from(!opts.suppress[0]) + usize::from(!opts.suppress[1])),
        ];
        let mut counts = [0u64; 3];
        let mut output = String::new();
        let mut emit = |col: usize, line: &str, output: &mut String| {
            counts[col] += 1;
            if !opts.suppress[col] {
                output.push_str(&prefixes[col]);
                output.push_str(line);
                output.push(sep);
            }
        };

        let (mut i, mut j) = (0, 0);
        while i < lines1.len() && j < lines2.len() {
            match lines1[i].cmp(&lines2[j]) {
                std::cmp::Ordering::Less => {
                    emit(0, &lines1[i], &mut output);
                    i += 1;
                }
                std::cmp::Ordering::Greater => {
                    emit(1, &lines2[j], &mut output);
                    j += 1;
                }
                std::cmp::Ordering::Equal => {
                    emit(2, &lines1[i], &mut output);
                    i += 1;
                    j += 1;
                }
            }
        }
        for line in &lines1[i..] {
            emit(0, line, &mut output);
        }
        for line in &lines2[j..] {
            emit(1, line, &mut output);
        }

        if opts.total {
            output.push_str(&format!(
                "{}{d}{}{d}{}{d}total{sep}",
                counts[0], counts[1], counts[2]
            ));
        }

        Ok(ExecResult::ok(output))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Arc;

    use crate::fs::{FileSystem, InMemoryFs};

    async fn run_comm(args: &[&str], stdin: Option<&str>, files: &[(&str, &[u8])]) -> ExecResult {
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
            stdin: crate::builtins::test_stream_opt(stdin),
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
        };

        Comm.execute(ctx).await.unwrap()
    }

    #[tokio::test]
    async fn test_comm_basic() {
        let result = run_comm(
            &["/a.txt", "/b.txt"],
            None,
            &[("/a.txt", b"a\nb\nc\n"), ("/b.txt", b"b\nc\nd\n")],
        )
        .await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "a\n\t\tb\n\t\tc\n\td\n");
    }

    #[tokio::test]
    async fn test_comm_suppress_1() {
        let result = run_comm(
            &["-1", "/a.txt", "/b.txt"],
            None,
            &[("/a.txt", b"a\nb\nc\n"), ("/b.txt", b"b\nc\nd\n")],
        )
        .await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "\tb\n\tc\nd\n");
    }

    #[tokio::test]
    async fn test_comm_suppress_2() {
        let result = run_comm(
            &["-2", "/a.txt", "/b.txt"],
            None,
            &[("/a.txt", b"a\nb\nc\n"), ("/b.txt", b"b\nc\nd\n")],
        )
        .await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "a\n\tb\n\tc\n");
    }

    #[tokio::test]
    async fn test_comm_suppress_3() {
        let result = run_comm(
            &["-3", "/a.txt", "/b.txt"],
            None,
            &[("/a.txt", b"a\nb\nc\n"), ("/b.txt", b"b\nc\nd\n")],
        )
        .await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "a\n\td\n");
    }

    #[tokio::test]
    async fn test_comm_suppress_12() {
        // Show only common lines
        let result = run_comm(
            &["-12", "/a.txt", "/b.txt"],
            None,
            &[("/a.txt", b"a\nb\nc\n"), ("/b.txt", b"b\nc\nd\n")],
        )
        .await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "b\nc\n");
    }

    #[tokio::test]
    async fn test_comm_suppress_13() {
        // Show only lines unique to file2
        let result = run_comm(
            &["-13", "/a.txt", "/b.txt"],
            None,
            &[("/a.txt", b"a\nb\nc\n"), ("/b.txt", b"b\nc\nd\n")],
        )
        .await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "d\n");
    }

    #[tokio::test]
    async fn test_comm_suppress_23() {
        // Show only lines unique to file1
        let result = run_comm(
            &["-23", "/a.txt", "/b.txt"],
            None,
            &[("/a.txt", b"a\nb\nc\n"), ("/b.txt", b"b\nc\nd\n")],
        )
        .await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "a\n");
    }

    #[tokio::test]
    async fn test_comm_identical_files() {
        let result = run_comm(
            &["/a.txt", "/b.txt"],
            None,
            &[("/a.txt", b"a\nb\nc\n"), ("/b.txt", b"a\nb\nc\n")],
        )
        .await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "\t\ta\n\t\tb\n\t\tc\n");
    }

    #[tokio::test]
    async fn test_comm_no_common() {
        let result = run_comm(
            &["/a.txt", "/b.txt"],
            None,
            &[("/a.txt", b"a\nc\n"), ("/b.txt", b"b\nd\n")],
        )
        .await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "a\n\tb\nc\n\td\n");
    }

    #[tokio::test]
    async fn test_comm_empty_file() {
        let result = run_comm(
            &["/a.txt", "/b.txt"],
            None,
            &[("/a.txt", b"a\nb\n"), ("/b.txt", b"")],
        )
        .await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "a\nb\n");
    }

    #[tokio::test]
    async fn test_comm_missing_operand() {
        let result = run_comm(&["/a.txt"], None, &[("/a.txt", b"a\n")]).await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("missing operand"));
    }

    #[tokio::test]
    async fn test_comm_invalid_option() {
        let result = run_comm(
            &["-Q", "/a.txt", "/b.txt"],
            None,
            &[("/a.txt", b"a\n"), ("/b.txt", b"b\n")],
        )
        .await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("invalid option -- 'Q'"));
    }

    #[tokio::test]
    async fn test_comm_file_not_found() {
        let result = run_comm(&["/a.txt", "/b.txt"], None, &[("/a.txt", b"a\n")]).await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("comm:"));
    }

    #[tokio::test]
    async fn test_comm_zero_terminated() {
        let result = run_comm(
            &["-z", "/a", "/b"],
            None,
            &[("/a", b"a\0b\0"), ("/b", b"b\0c\0")],
        )
        .await;
        assert_eq!(result.stdout, "a\0\t\tb\0\tc\0");
    }

    #[tokio::test]
    async fn test_comm_output_delimiter_and_total() {
        let result = run_comm(
            &["--output-delimiter=|", "--total", "/a", "/b"],
            None,
            &[("/a", b"a\nb\n"), ("/b", b"b\nc\n")],
        )
        .await;
        assert_eq!(result.stdout, "a\n||b\n|c\n1|1|1|total\n");
    }

    #[tokio::test]
    async fn test_comm_extra_operand() {
        let result = run_comm(&["/a", "/b", "/c"], None, &[]).await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("extra operand '/c'"));
    }
}
