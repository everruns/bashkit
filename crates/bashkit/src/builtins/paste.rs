//! paste builtin command - merge lines of files

use async_trait::async_trait;

use super::{Builtin, Context, read_text_file};
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

fn parse_paste_args(args: &[String]) -> std::result::Result<(PasteOptions, Vec<String>), String> {
    let mut opts = PasteOptions {
        delimiters: vec!['\t'],
        serial: false,
    };
    let mut files = Vec::new();
    let mut p = super::arg_parser::ArgParser::new(args);

    while !p.is_done() {
        if let Some(val) = p.flag_value("-d", "paste")? {
            opts.delimiters = parse_delim_spec(val);
        } else if p.flag("-s") {
            opts.serial = true;
        } else if try_parse_combined_flags(&mut p, &mut opts)? {
            // handled combined flags like -sd, or -sd ,
        } else if p.is_flag() && p.current() != Some("--") {
            // Reject unknown options instead of treating them as files.
            return Err(invalid_option_msg("paste", p.current().unwrap_or_default()));
        } else if let Some(arg) = p.positional() {
            files.push(arg.to_string());
        }
    }

    if opts.delimiters.is_empty() {
        opts.delimiters = vec!['\t'];
    }

    Ok((opts, files))
}

/// Parse combined short flags like `-sd,` where `s` is a boolean flag
/// and `d` takes the rest of the string as its value.
fn try_parse_combined_flags(
    p: &mut super::arg_parser::ArgParser<'_>,
    opts: &mut PasteOptions,
) -> std::result::Result<bool, String> {
    let arg = match p.current() {
        Some(a) if a.starts_with('-') && !a.starts_with("--") && a.len() > 2 => a,
        _ => return Ok(false),
    };

    let chars: Vec<char> = arg[1..].chars().collect();
    let mut i = 0;
    let mut serial = false;
    let mut delimiters = None;

    while i < chars.len() {
        match chars[i] {
            's' => {
                serial = true;
                i += 1;
            }
            'd' => {
                // 'd' consumes the attached remainder, or the next argv when trailing.
                let rest: String = chars[i + 1..].iter().collect();
                if rest.is_empty() {
                    let Some(next) = p.rest().get(1) else {
                        return Err("paste: -d requires an argument".to_string());
                    };
                    delimiters = Some(parse_delim_spec(next));
                    p.advance();
                } else {
                    delimiters = Some(parse_delim_spec(&rest));
                }
                i = chars.len(); // consumed everything
            }
            _ => return Ok(false), // unknown flag char, bail out
        }
    }

    // All chars parsed successfully — apply and advance
    if serial {
        opts.serial = true;
    }
    if let Some(d) = delimiters {
        opts.delimiters = d;
    }
    p.advance();
    Ok(true)
}

/// Build an unrecognized-option message (no trailing newline; the caller
/// appends it via `format!("{msg}\n")`). Mirrors `super::invalid_option`.
fn invalid_option_msg(cmd: &str, arg: &str) -> String {
    if let Some(long) = arg.strip_prefix("--") {
        format!("{cmd}: unrecognized option '--{long}'")
    } else {
        let ch = arg
            .strip_prefix('-')
            .and_then(|s| s.chars().next())
            .unwrap_or('-');
        format!("{cmd}: invalid option -- '{ch}'")
    }
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
        if let Some(r) = super::check_help_version(
            ctx.args,
            "Usage: paste [OPTION]... [FILE]...\nMerge lines of files.\n\n  -d LIST\tuse LIST as delimiters\n  -s\t\tpaste one file at a time\n  --help\tdisplay this help and exit\n  --version\toutput version information and exit\n",
            Some("paste (bashkit) 0.1"),
        ) {
            return Ok(r);
        }
        let (opts, files) = match parse_paste_args(ctx.args) {
            Ok(parsed) => parsed,
            Err(msg) => return Ok(ExecResult::err(format!("{msg}\n"), 1)),
        };

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

                    let text = match read_text_file(&*ctx.fs, &path, "paste").await {
                        Ok(t) => t,
                        Err(e) => return Ok(e),
                    };
                    sources.push(text.lines().map(|l| l.to_string()).collect());
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
            stdin,
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
        assert!(result.stderr.contains("paste: -d requires an argument"));
    }

    #[tokio::test]
    async fn test_paste_missing_delimiter_errors() {
        let result = run_paste(&["-d"], Some("a\nb\nc\n")).await;
        assert_eq!(result.exit_code, 1);
        assert_eq!(result.stdout, "");
        assert!(result.stderr.contains("paste: -d requires an argument"));
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
