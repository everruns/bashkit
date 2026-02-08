//! paste builtin command - merge lines of files

use async_trait::async_trait;

use super::{Builtin, Context};
use crate::error::Result;
use crate::interpreter::ExecResult;

/// The paste builtin - merge lines of files.
///
/// Usage: paste [-d DELIM] [-s] [FILE...]
///
/// Options:
///   -d DELIM   Use DELIM instead of TAB as delimiter (cycles through chars)
///   -s         Paste one file at a time (serial mode)
pub struct Paste;

struct PasteOptions {
    delimiters: Vec<char>,
    serial: bool,
}

fn parse_paste_args(args: &[String]) -> (PasteOptions, Vec<String>) {
    let mut opts = PasteOptions {
        delimiters: vec!['\t'],
        serial: false,
    };
    let mut files = Vec::new();
    let mut i = 0;

    while i < args.len() {
        let arg = &args[i];
        if arg == "-d" {
            i += 1;
            if i < args.len() {
                opts.delimiters = parse_delim_spec(&args[i]);
            }
        } else if let Some(d) = arg.strip_prefix("-d") {
            opts.delimiters = parse_delim_spec(d);
        } else if arg == "-s" {
            opts.serial = true;
        } else if arg == "-" || !arg.starts_with('-') {
            files.push(arg.clone());
        }
        i += 1;
    }

    if opts.delimiters.is_empty() {
        opts.delimiters = vec!['\t'];
    }

    (opts, files)
}

fn parse_delim_spec(spec: &str) -> Vec<char> {
    let mut delims = Vec::new();
    let mut chars = spec.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => delims.push('\n'),
                Some('t') => delims.push('\t'),
                Some('\\') => delims.push('\\'),
                Some('0') => delims.push('\0'),
                Some(other) => delims.push(other),
                None => delims.push('\\'),
            }
        } else {
            delims.push(c);
        }
    }
    delims
}

#[async_trait]
impl Builtin for Paste {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let (opts, files) = parse_paste_args(ctx.args);

        // Collect input sources
        let mut sources: Vec<Vec<String>> = Vec::new();

        if files.is_empty() {
            // Read from stdin
            if let Some(stdin) = ctx.stdin {
                sources.push(stdin.lines().map(|l| l.to_string()).collect());
            }
        } else {
            for file in &files {
                if file == "-" {
                    let lines = ctx
                        .stdin
                        .map(|s| s.lines().map(|l| l.to_string()).collect())
                        .unwrap_or_default();
                    sources.push(lines);
                } else {
                    let path = if file.starts_with('/') {
                        std::path::PathBuf::from(file)
                    } else {
                        ctx.cwd.join(file)
                    };

                    match ctx.fs.read_file(&path).await {
                        Ok(content) => {
                            let text = String::from_utf8_lossy(&content);
                            sources.push(text.lines().map(|l| l.to_string()).collect());
                        }
                        Err(e) => {
                            return Ok(ExecResult::err(format!("paste: {}: {}\n", file, e), 1));
                        }
                    }
                }
            }
        }

        let mut output = String::new();

        if opts.serial {
            // Serial mode: each file becomes one line
            for source in &sources {
                for (j, line) in source.iter().enumerate() {
                    if j > 0 {
                        let delim = opts.delimiters[(j - 1) % opts.delimiters.len()];
                        output.push(delim);
                    }
                    output.push_str(line);
                }
                output.push('\n');
            }
        } else {
            // Parallel mode: merge corresponding lines
            let max_lines = sources.iter().map(|s| s.len()).max().unwrap_or(0);
            for i in 0..max_lines {
                for (j, source) in sources.iter().enumerate() {
                    if j > 0 {
                        let delim = opts.delimiters[(j - 1) % opts.delimiters.len()];
                        output.push(delim);
                    }
                    if let Some(line) = source.get(i) {
                        output.push_str(line);
                    }
                }
                output.push('\n');
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
            stdin,
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
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
            stdin,
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
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
