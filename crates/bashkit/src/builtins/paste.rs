//! paste builtin command - merge lines of files
//!
//! Decision: every `-` operand reads the one shared stdin stream, so
//! `paste - -` takes lines in turns and `paste -s - -` gives the second `-`
//! nothing, as GNU paste does. `\0` in a delimiter list means "no delimiter".

use async_trait::async_trait;

use super::arg_parser::{OptArg, gnu_getopt};
use super::{Builtin, Context, read_text_file};
use crate::error::Result;
use crate::fs::vfs_join;
use crate::interpreter::ExecResult;

/// The paste builtin - merge lines of files.
///
/// Usage: paste [-s] [-z] [-d LIST] [FILE...]
///
/// Options:
///   -d LIST    Use LIST instead of TAB as delimiters (cycled)
///   -s         Paste one file at a time (serial mode)
///   -z         Line delimiter is NUL, not newline
pub struct Paste;

struct PasteOptions {
    delimiters: Vec<String>,
    serial: bool,
    zero_terminated: bool,
}

#[allow(clippy::result_large_err)]
fn parse_paste_args(
    args: &[String],
) -> std::result::Result<(PasteOptions, Vec<String>), ExecResult> {
    let (opts, files) = gnu_getopt(
        "paste",
        args,
        "d:sz",
        &[
            ("delimiters", OptArg::Required, 'd'),
            ("serial", OptArg::No, 's'),
            ("zero-terminated", OptArg::No, 'z'),
        ],
        true,
        1,
    )?;
    let mut out = PasteOptions {
        delimiters: vec!["\t".to_string()],
        serial: false,
        zero_terminated: false,
    };
    for o in opts {
        match o.key {
            'd' => out.delimiters = parse_delim_spec(&o.value.unwrap_or_default()),
            's' => out.serial = true,
            _ => out.zero_terminated = true,
        }
    }
    if out.delimiters.is_empty() {
        out.delimiters = vec![String::new()];
    }
    Ok((out, files))
}

fn parse_delim_spec(spec: &str) -> Vec<String> {
    let mut delims = Vec::new();
    let mut chars = spec.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            delims.push(match chars.next() {
                Some('n') => "\n".to_string(),
                Some('t') => "\t".to_string(),
                Some('\\') => "\\".to_string(),
                // `\0` is the empty delimiter.
                Some('0') => String::new(),
                Some(other) => other.to_string(),
                None => "\\".to_string(),
            });
        } else {
            delims.push(c.to_string());
        }
    }
    delims
}

/// Split into records, keeping a final unterminated one.
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

/// An input: its own lines, or the shared stdin cursor.
enum Source {
    Lines(Vec<String>, usize),
    Stdin,
}

struct Inputs {
    sources: Vec<Source>,
    stdin: Vec<String>,
    stdin_pos: usize,
}

impl Inputs {
    fn next(&mut self, idx: usize) -> Option<String> {
        match &mut self.sources[idx] {
            Source::Lines(lines, pos) => {
                let line = lines.get(*pos).cloned();
                *pos += 1;
                line
            }
            Source::Stdin => {
                let line = self.stdin.get(self.stdin_pos).cloned();
                self.stdin_pos += 1;
                line
            }
        }
    }
}

#[async_trait]
impl Builtin for Paste {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        if let Some(r) = super::check_help_version(
            ctx.args,
            "Usage: paste [OPTION]... [FILE]...\nMerge lines of files.\n\n  -d, --delimiters=LIST\treuse characters from LIST instead of TABs\n  -s, --serial\tpaste one file at a time instead of in parallel\n  -z, --zero-terminated\tline delimiter is NUL, not newline\n  --help\tdisplay this help and exit\n  --version\toutput version information and exit\n",
            Some("paste (bashkit) 0.1"),
        ) {
            return Ok(r);
        }
        let (opts, mut files) = match parse_paste_args(ctx.args) {
            Ok(parsed) => parsed,
            Err(e) => return Ok(e),
        };
        if files.is_empty() {
            files.push("-".to_string());
        }
        let sep = if opts.zero_terminated { '\0' } else { '\n' };

        let mut inputs = Inputs {
            sources: Vec::new(),
            stdin: records(ctx.stdin.map(|s| &**s).unwrap_or(""), sep),
            stdin_pos: 0,
        };
        for file in &files {
            if file == "-" {
                inputs.sources.push(Source::Stdin);
                continue;
            }
            let path = if file.starts_with('/') {
                std::path::PathBuf::from(file)
            } else {
                vfs_join(ctx.cwd, file)
            };
            let text = match read_text_file(&*ctx.fs, &path, "paste").await {
                Ok(t) => t,
                Err(e) => return Ok(e),
            };
            inputs.sources.push(Source::Lines(records(&text, sep), 0));
        }

        let delims = &opts.delimiters;
        let n = inputs.sources.len();
        let mut output = String::new();

        if opts.serial {
            // Serial mode: each file becomes one line.
            for idx in 0..n {
                let mut k = 0;
                let mut first = true;
                while let Some(line) = inputs.next(idx) {
                    if !first {
                        output.push_str(&delims[k % delims.len()]);
                        k += 1;
                    }
                    first = false;
                    output.push_str(&line);
                }
                output.push(sep);
            }
        } else {
            // Parallel mode: merge corresponding lines until all are spent.
            loop {
                let mut line = String::new();
                let mut any = false;
                for idx in 0..n {
                    if let Some(l) = inputs.next(idx) {
                        any = true;
                        line.push_str(&l);
                    }
                    if idx + 1 < n {
                        line.push_str(&delims[idx % delims.len()]);
                    }
                }
                if !any {
                    break;
                }
                output.push_str(&line);
                output.push(sep);
            }
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

    async fn run_paste(args: &[&str], stdin: Option<&str>) -> ExecResult {
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
            stdin: crate::builtins::test_stream_opt(stdin),
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
        };

        Paste.execute(ctx).await.unwrap()
    }

    async fn run_paste_with_fs(
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
            stdin: crate::builtins::test_stream_opt(stdin),
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
        };

        Paste.execute(ctx).await.unwrap()
    }

    #[tokio::test]
    async fn test_paste_stdin() {
        let result = run_paste(&[], Some("a\nb\nc\n")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "a\nb\nc\n");
    }

    #[tokio::test]
    async fn test_paste_two_files() {
        let result = run_paste_with_fs(
            &["/a.txt", "/b.txt"],
            None,
            &[("/a.txt", b"1\n2\n3\n"), ("/b.txt", b"a\nb\nc\n")],
        )
        .await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "1\ta\n2\tb\n3\tc\n");
    }

    #[tokio::test]
    async fn test_paste_uneven_files() {
        let result = run_paste_with_fs(
            &["/a.txt", "/b.txt"],
            None,
            &[("/a.txt", b"1\n2\n3\n"), ("/b.txt", b"a\nb\n")],
        )
        .await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "1\ta\n2\tb\n3\t\n");
    }

    #[tokio::test]
    async fn test_paste_custom_delimiter() {
        let result = run_paste_with_fs(
            &["-d", ",", "/a.txt", "/b.txt"],
            None,
            &[("/a.txt", b"1\n2\n"), ("/b.txt", b"a\nb\n")],
        )
        .await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "1,a\n2,b\n");
    }

    #[tokio::test]
    async fn test_paste_serial() {
        let result = run_paste_with_fs(
            &["-s", "/a.txt", "/b.txt"],
            None,
            &[("/a.txt", b"1\n2\n3\n"), ("/b.txt", b"a\nb\nc\n")],
        )
        .await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "1\t2\t3\na\tb\tc\n");
    }

    #[tokio::test]
    async fn test_paste_serial_custom_delim() {
        let result = run_paste_with_fs(
            &["-s", "-d", ",", "/a.txt"],
            None,
            &[("/a.txt", b"x\ny\nz\n")],
        )
        .await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "x,y,z\n");
    }

    #[tokio::test]
    async fn test_paste_cycling_delimiters() {
        let result = run_paste_with_fs(
            &["-d", ",:", "/a.txt", "/b.txt", "/c.txt"],
            None,
            &[
                ("/a.txt", b"1\n2\n"),
                ("/b.txt", b"a\nb\n"),
                ("/c.txt", b"x\ny\n"),
            ],
        )
        .await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "1,a:x\n2,b:y\n");
    }

    #[tokio::test]
    async fn test_paste_empty_input() {
        let result = run_paste(&[], Some("")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "");
    }

    #[tokio::test]
    async fn test_paste_file_not_found() {
        let result = run_paste(&["/nonexistent"], None).await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("paste:"));
    }

    #[tokio::test]
    async fn test_paste_combined_sd_comma() {
        let result = run_paste(&["-sd,"], Some("a\nb\nc\n")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "a,b,c\n");
    }

    #[tokio::test]
    async fn test_paste_combined_sd_colon() {
        let result = run_paste(&["-sd:"], Some("x\ny\nz\n")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "x:y:z\n");
    }

    #[tokio::test]
    async fn test_paste_combined_sd_consumes_next_delimiter() {
        let result = run_paste(&["-sd", ","], Some("a\nb\nc\n")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "a,b,c\n");
    }

    #[tokio::test]
    async fn test_paste_combined_sd_missing_delimiter_errors() {
        let result = run_paste(&["-sd"], Some("a\nb\nc\n")).await;
        assert_eq!(result.exit_code, 1);
        assert_eq!(result.stdout, "");
        assert!(result.stderr.contains("option requires an argument -- 'd'"));
    }

    #[tokio::test]
    async fn test_paste_missing_delimiter_errors() {
        let result = run_paste(&["-d"], Some("a\nb\nc\n")).await;
        assert_eq!(result.exit_code, 1);
        assert_eq!(result.stdout, "");
        assert!(result.stderr.contains("option requires an argument -- 'd'"));
    }

    #[tokio::test]
    async fn test_paste_dashes_share_stdin() {
        let result = run_paste(&["-", "-"], Some("1\n2\n3\n")).await;
        assert_eq!(result.stdout, "1\t2\n3\t\n");
        let result = run_paste(&["-s", "-", "-"], Some("1\n2\n")).await;
        assert_eq!(result.stdout, "1\t2\n\n");
    }

    #[tokio::test]
    async fn test_paste_stdin_dash() {
        let result =
            run_paste_with_fs(&["-", "/b.txt"], Some("1\n2\n"), &[("/b.txt", b"a\nb\n")]).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "1\ta\n2\tb\n");
    }

    #[tokio::test]
    async fn test_paste_backslash_n_delimiter() {
        let result = run_paste_with_fs(
            &["-d", "\\n", "-s", "/a.txt"],
            None,
            &[("/a.txt", b"x\ny\nz\n")],
        )
        .await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "x\ny\nz\n");
    }

    #[tokio::test]
    async fn test_paste_rejects_unknown_option() {
        let result = run_paste(&["-Q"], Some("a\n")).await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("invalid option -- 'Q'"));
    }

    #[tokio::test]
    async fn test_paste_three_files() {
        let result = run_paste_with_fs(
            &["/a.txt", "/b.txt", "/c.txt"],
            None,
            &[
                ("/a.txt", b"1\n2\n"),
                ("/b.txt", b"a\nb\n"),
                ("/c.txt", b"X\nY\n"),
            ],
        )
        .await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "1\ta\tX\n2\tb\tY\n");
    }
}
