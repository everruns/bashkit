//! rg - Simplified ripgrep builtin
//!
//! Recursive file search by default, similar to grep but with rg-style defaults.
//!
//! Usage:
//!   rg PATTERN [PATH...]
//!   rg -i PATTERN file          # case insensitive
//!   rg -n PATTERN file          # show line numbers (default)
//!   rg -c PATTERN file          # count matches
//!   rg -l PATTERN file          # files with matches
//!   rg -v PATTERN file          # invert match
//!   rg -w PATTERN file          # word boundary
//!   rg -F PATTERN file          # fixed strings (literal)
//!   rg -m NUM PATTERN file      # max count per file
//!   rg --no-filename PATTERN    # suppress filename
//!   rg --color never PATTERN    # color output (no-op)

use async_trait::async_trait;
use regex::Regex;

use super::search_common::{build_search_regex, collect_files_recursive, parse_numeric_flag_arg};
use super::{Builtin, Context, read_text_file, resolve_path};
use crate::error::{Error, Result};
use crate::interpreter::ExecResult;

/// rg command - recursive pattern search (simplified ripgrep)
pub struct Rg;

struct RgOptions {
    pattern: String,
    paths: Vec<String>,
    ignore_case: bool,
    line_numbers: bool,
    count_only: bool,
    files_with_matches: bool,
    invert_match: bool,
    word_boundary: bool,
    fixed_strings: bool,
    max_count: Option<usize>,
    no_filename: bool,
}

impl RgOptions {
    fn parse(args: &[String]) -> Result<Self> {
        let mut opts = RgOptions {
            pattern: String::new(),
            paths: Vec::new(),
            ignore_case: false,
            line_numbers: true, // rg shows line numbers by default
            count_only: false,
            files_with_matches: false,
            invert_match: false,
            word_boundary: false,
            fixed_strings: false,
            max_count: None,
            no_filename: false,
        };

        let mut positional = Vec::new();
        let mut i = 0;

        while i < args.len() {
            let arg = &args[i];
            if arg.starts_with('-') && arg.len() > 1 && !arg.starts_with("--") {
                let chars: Vec<char> = arg[1..].chars().collect();
                let mut j = 0;
                while j < chars.len() {
                    match chars[j] {
                        'i' => opts.ignore_case = true,
                        'n' => opts.line_numbers = true,
                        'c' => opts.count_only = true,
                        'l' => opts.files_with_matches = true,
                        'v' => opts.invert_match = true,
                        'w' => opts.word_boundary = true,
                        'F' => opts.fixed_strings = true,
                        'm' => {
                            opts.max_count =
                                Some(parse_numeric_flag_arg(&chars, j, &mut i, args, "rg", "-m")?);
                            break;
                        }
                        _ => {} // ignore unknown
                    }
                    j += 1;
                }
            } else if let Some(opt) = arg.strip_prefix("--") {
                if opt == "no-filename" {
                    opts.no_filename = true;
                } else if opt == "color" || opt.starts_with("color=") {
                    // no-op
                } else if opt == "no-line-number" {
                    opts.line_numbers = false;
                }
                // ignore other long options
            } else {
                positional.push(arg.clone());
            }
            i += 1;
        }

        if positional.is_empty() {
            return Err(Error::Execution("rg: missing pattern".to_string()));
        }

        opts.pattern = positional.remove(0);
        opts.paths = positional;

        Ok(opts)
    }

    fn build_regex(&self) -> Result<Regex> {
        build_search_regex(
            &self.pattern,
            self.fixed_strings,
            self.word_boundary,
            self.ignore_case,
            "rg",
        )
    }
}

#[async_trait]
impl Builtin for Rg {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let opts = RgOptions::parse(ctx.args)?;
        let regex = opts.build_regex()?;

        // Collect input files - rg is recursive by default
        let inputs: Vec<(String, String)> = if opts.paths.is_empty() {
            // Read from stdin when no paths given and stdin is available
            if let Some(stdin) = ctx.stdin {
                vec![("(stdin)".to_string(), stdin.to_string())]
            } else {
                // Search current directory recursively
                let files = collect_files_recursive(&ctx.fs, std::slice::from_ref(ctx.cwd)).await;
                let mut inputs = Vec::new();
                for path in files {
                    if let Ok(content) = ctx.fs.read_file(&path).await {
                        let text = String::from_utf8_lossy(&content).into_owned();
                        inputs.push((path.to_string_lossy().into_owned(), text));
                    }
                }
                inputs
            }
        } else {
            let mut inputs = Vec::new();
            for p in &opts.paths {
                let path = resolve_path(ctx.cwd, p);
                // Check if it's a directory → recurse
                if let Ok(meta) = ctx.fs.stat(&path).await
                    && meta.file_type.is_dir()
                {
                    let files = collect_files_recursive(&ctx.fs, std::slice::from_ref(&path)).await;
                    for fpath in files {
                        if let Ok(content) = ctx.fs.read_file(&fpath).await {
                            let text = String::from_utf8_lossy(&content).into_owned();
                            inputs.push((fpath.to_string_lossy().into_owned(), text));
                        }
                    }
                    continue;
                }
                // It's a file
                let text = match read_text_file(&*ctx.fs, &path, "rg").await {
                    Ok(t) => t,
                    Err(e) => return Ok(e),
                };
                inputs.push((p.clone(), text));
            }
            inputs
        };

        let show_filename = if opts.no_filename {
            false
        } else {
            inputs.len() > 1
        };

        let mut output = String::new();
        let mut any_match = false;

        for (filename, content) in &inputs {
            let mut match_count = 0usize;

            for (line_idx, line) in content.lines().enumerate() {
                let matched = regex.is_match(line);
                let matched = if opts.invert_match { !matched } else { matched };

                if !matched {
                    continue;
                }

                match_count += 1;
                any_match = true;

                if let Some(max) = opts.max_count
                    && match_count > max
                {
                    break;
                }

                if opts.files_with_matches || opts.count_only {
                    continue;
                }

                // Build output line
                if show_filename {
                    output.push_str(filename);
                    output.push(':');
                }
                if opts.line_numbers {
                    output.push_str(&(line_idx + 1).to_string());
                    output.push(':');
                }
                output.push_str(line);
                output.push('\n');
            }

            if opts.files_with_matches && match_count > 0 {
                output.push_str(filename);
                output.push('\n');
            }
            if opts.count_only {
                if show_filename {
                    output.push_str(filename);
                    output.push(':');
                }
                output.push_str(&match_count.to_string());
                output.push('\n');
            }
        }

        if any_match {
            Ok(ExecResult::ok(output))
        } else {
            Ok(ExecResult::with_code(String::new(), 1))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::{FileSystem, InMemoryFs};
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    async fn run_rg(args: &[&str], stdin: Option<&str>, files: &[(&str, &[u8])]) -> ExecResult {
        let fs = Arc::new(InMemoryFs::new());
        for (path, content) in files {
            let p = Path::new(path);
            // Ensure parent dirs exist
            if let Some(parent) = p.parent()
                && parent != Path::new("/")
            {
                let fs_trait: &dyn FileSystem = &*fs;
                let _ = fs_trait.mkdir(parent, true).await;
            }
            let fs_trait: &dyn FileSystem = &*fs;
            fs_trait.write_file(p, content).await.unwrap();
        }

        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let env = HashMap::new();
        let mut variables = HashMap::new();
        let mut cwd = PathBuf::from("/");
        let fs_dyn = fs as Arc<dyn FileSystem>;
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs: fs_dyn,
            stdin,
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
            shell: None,
        };

        Rg.execute(ctx).await.unwrap()
    }

    #[tokio::test]
    async fn test_rg_basic_match() {
        let result = run_rg(
            &["hello", "/test.txt"],
            None,
            &[("/test.txt", b"hello world\ngoodbye\nhello again\n")],
        )
        .await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("hello world"));
        assert!(result.stdout.contains("hello again"));
        assert!(!result.stdout.contains("goodbye"));
    }

    #[tokio::test]
    async fn test_rg_no_match() {
        let result = run_rg(
            &["missing", "/test.txt"],
            None,
            &[("/test.txt", b"hello world\n")],
        )
        .await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stdout.is_empty());
    }

    #[tokio::test]
    async fn test_rg_case_insensitive() {
        let result = run_rg(
            &["-i", "HELLO", "/test.txt"],
            None,
            &[("/test.txt", b"Hello World\nhello world\nHELLO\n")],
        )
        .await;
        assert_eq!(result.exit_code, 0);
        // All three lines match
        let lines: Vec<&str> = result.stdout.trim().lines().collect();
        assert_eq!(lines.len(), 3);
    }

    #[tokio::test]
    async fn test_rg_count() {
        let result = run_rg(
            &["-c", "hello", "/test.txt"],
            None,
            &[("/test.txt", b"hello\nworld\nhello again\n")],
        )
        .await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.trim().ends_with('2'));
    }

    #[tokio::test]
    async fn test_rg_files_with_matches() {
        let result = run_rg(
            &["-l", "hello", "/a.txt", "/b.txt"],
            None,
            &[("/a.txt", b"hello\n"), ("/b.txt", b"world\n")],
        )
        .await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("/a.txt"));
        assert!(!result.stdout.contains("/b.txt"));
    }

    #[tokio::test]
    async fn test_rg_invert_match() {
        let result = run_rg(
            &["-v", "hello", "/test.txt"],
            None,
            &[("/test.txt", b"hello\nworld\nfoo\n")],
        )
        .await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("world"));
        assert!(result.stdout.contains("foo"));
        assert!(!result.stdout.contains("hello"));
    }

    #[tokio::test]
    async fn test_rg_fixed_strings() {
        let result = run_rg(
            &["-F", "a.b", "/test.txt"],
            None,
            &[("/test.txt", b"a.b matches\naxb no match\n")],
        )
        .await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("a.b matches"));
        assert!(!result.stdout.contains("axb"));
    }

    #[tokio::test]
    async fn test_rg_word_boundary() {
        let result = run_rg(
            &["-w", "cat", "/test.txt"],
            None,
            &[("/test.txt", b"the cat sat\ncatch this\nmy cat\n")],
        )
        .await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("the cat sat"));
        assert!(result.stdout.contains("my cat"));
        assert!(!result.stdout.contains("catch"));
    }

    #[tokio::test]
    async fn test_rg_max_count() {
        let result = run_rg(
            &["-m", "1", "hello", "/test.txt"],
            None,
            &[("/test.txt", b"hello one\nhello two\nhello three\n")],
        )
        .await;
        assert_eq!(result.exit_code, 0);
        let lines: Vec<&str> = result.stdout.trim().lines().collect();
        assert_eq!(lines.len(), 1);
    }

    #[tokio::test]
    async fn test_rg_recursive_directory() {
        let result = run_rg(
            &["needle", "/dir"],
            None,
            &[
                ("/dir/a.txt", b"has needle here\n"),
                ("/dir/sub/b.txt", b"no match\n"),
                ("/dir/sub/c.txt", b"another needle\n"),
            ],
        )
        .await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("needle"));
        // Should have matches from 2 files
        assert!(result.stdout.contains("a.txt"));
        assert!(result.stdout.contains("c.txt"));
    }

    #[tokio::test]
    async fn test_rg_stdin() {
        let result = run_rg(&["world"], Some("hello\nworld\nfoo\n"), &[]).await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("world"));
    }

    #[tokio::test]
    async fn test_rg_missing_pattern() {
        let fs = Arc::new(InMemoryFs::new()) as Arc<dyn FileSystem>;
        let args: Vec<String> = vec![];
        let env = HashMap::new();
        let mut variables = HashMap::new();
        let mut cwd = PathBuf::from("/");
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
            shell: None,
        };
        let result = Rg.execute(ctx).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_rg_file_not_found() {
        let result = run_rg(&["pattern", "/nonexistent"], None, &[]).await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("rg:"));
    }

    #[tokio::test]
    async fn test_rg_no_filename_flag() {
        let result = run_rg(
            &["--no-filename", "hello", "/a.txt", "/b.txt"],
            None,
            &[("/a.txt", b"hello\n"), ("/b.txt", b"hello there\n")],
        )
        .await;
        assert_eq!(result.exit_code, 0);
        // Should not contain filenames
        assert!(!result.stdout.contains("/a.txt"));
        assert!(!result.stdout.contains("/b.txt"));
    }

    #[tokio::test]
    async fn test_rg_line_numbers_default() {
        let result = run_rg(
            &["world", "/test.txt"],
            None,
            &[("/test.txt", b"hello\nworld\n")],
        )
        .await;
        assert_eq!(result.exit_code, 0);
        // Line numbers on by default, "world" is on line 2
        assert!(result.stdout.contains("2:"));
    }
}
