//! grep - Pattern matching builtin
//!
//! Implements grep functionality using the regex crate.
//!
//! Usage:
//!   grep pattern file
//!   echo "text" | grep pattern
//!   grep -i pattern file        # case insensitive
//!   grep -v pattern file        # invert match
//!   grep -n pattern file        # show line numbers
//!   grep -c pattern file        # count matches
//!   grep -o pattern file        # only show matching part
//!   grep -l pattern file1 file2 # list matching files
//!   grep -E pattern file        # extended regex (default)
//!   grep -F pattern file        # fixed string match

use async_trait::async_trait;
use regex::{Regex, RegexBuilder};

use super::{Builtin, Context};
use crate::error::{Error, Result};
use crate::interpreter::ExecResult;

/// grep command - pattern matching
pub struct Grep;

struct GrepOptions {
    pattern: String,
    files: Vec<String>,
    ignore_case: bool,
    invert_match: bool,
    line_numbers: bool,
    count_only: bool,
    files_with_matches: bool,
    fixed_strings: bool,
    only_matching: bool,
    word_regex: bool,
}

impl GrepOptions {
    fn parse(args: &[String]) -> Result<Self> {
        let mut opts = GrepOptions {
            pattern: String::new(),
            files: Vec::new(),
            ignore_case: false,
            invert_match: false,
            line_numbers: false,
            count_only: false,
            files_with_matches: false,
            fixed_strings: false,
            only_matching: false,
            word_regex: false,
        };

        let mut positional = Vec::new();
        let mut i = 0;

        while i < args.len() {
            let arg = &args[i];
            if arg.starts_with('-') && arg.len() > 1 && !arg.starts_with("--") {
                // Handle combined flags like -iv
                for c in arg[1..].chars() {
                    match c {
                        'i' => opts.ignore_case = true,
                        'v' => opts.invert_match = true,
                        'n' => opts.line_numbers = true,
                        'c' => opts.count_only = true,
                        'l' => opts.files_with_matches = true,
                        'o' => opts.only_matching = true,
                        'w' => opts.word_regex = true,
                        'F' => opts.fixed_strings = true,
                        'E' => {} // Extended regex is default
                        'e' => {
                            // -e pattern
                            i += 1;
                            if i < args.len() {
                                opts.pattern = args[i].clone();
                            }
                        }
                        _ => {} // Ignore unknown flags
                    }
                }
            } else if arg == "--" {
                // End of options
                positional.extend(args[i + 1..].iter().cloned());
                break;
            } else {
                positional.push(arg.clone());
            }
            i += 1;
        }

        // First positional is pattern (if not set by -e)
        if opts.pattern.is_empty() {
            if positional.is_empty() {
                return Err(Error::Execution("grep: missing pattern".to_string()));
            }
            opts.pattern = positional.remove(0);
        }

        // Rest are files
        opts.files = positional;

        Ok(opts)
    }

    fn build_regex(&self) -> Result<Regex> {
        let pattern = if self.fixed_strings {
            regex::escape(&self.pattern)
        } else {
            self.pattern.clone()
        };

        // Wrap with word boundaries if -w flag is set
        let pattern = if self.word_regex {
            format!(r"\b{}\b", pattern)
        } else {
            pattern
        };

        RegexBuilder::new(&pattern)
            .case_insensitive(self.ignore_case)
            .build()
            .map_err(|e| Error::Execution(format!("grep: invalid pattern: {}", e)))
    }
}

#[async_trait]
impl Builtin for Grep {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let opts = GrepOptions::parse(ctx.args)?;
        let regex = opts.build_regex()?;

        let mut output = String::new();
        let mut any_match = false;
        let mut exit_code = 1; // 1 = no match

        // Determine input sources
        // Use "(stdin)" for stdin when -l flag is set
        let stdin_name = if opts.files_with_matches {
            "(stdin)"
        } else {
            ""
        };
        let inputs: Vec<(&str, String)> = if opts.files.is_empty() {
            // Read from stdin
            vec![(stdin_name, ctx.stdin.unwrap_or("").to_string())]
        } else {
            // Read from files
            let mut inputs = Vec::new();
            for file in &opts.files {
                let path = if file.starts_with('/') {
                    std::path::PathBuf::from(file)
                } else {
                    ctx.cwd.join(file)
                };

                match ctx.fs.read_file(&path).await {
                    Ok(content) => {
                        let text = String::from_utf8_lossy(&content).into_owned();
                        inputs.push((file.as_str(), text));
                    }
                    Err(e) => {
                        // Report error but continue with other files
                        output.push_str(&format!("grep: {}: {}\n", file, e));
                    }
                }
            }
            inputs
        };

        let show_filename = opts.files.len() > 1;

        for (filename, content) in inputs {
            let mut match_count = 0;
            let mut file_matched = false;

            for (line_num, line) in content.lines().enumerate() {
                if opts.only_matching {
                    // -o mode: output each match separately
                    for mat in regex.find_iter(line) {
                        file_matched = true;
                        any_match = true;
                        match_count += 1;

                        if opts.files_with_matches {
                            break;
                        }

                        if !opts.count_only {
                            if show_filename {
                                output.push_str(filename);
                                output.push(':');
                            }
                            if opts.line_numbers {
                                output.push_str(&format!("{}:", line_num + 1));
                            }
                            output.push_str(mat.as_str());
                            output.push('\n');
                        }
                    }
                    if opts.files_with_matches && file_matched {
                        break;
                    }
                } else {
                    let matches = regex.is_match(line);
                    let should_output = if opts.invert_match { !matches } else { matches };

                    if should_output {
                        file_matched = true;
                        any_match = true;
                        match_count += 1;

                        if opts.files_with_matches {
                            // Just need to know if file matches, output later
                            break;
                        }

                        if !opts.count_only {
                            // Build output line
                            if show_filename {
                                output.push_str(filename);
                                output.push(':');
                            }
                            if opts.line_numbers {
                                output.push_str(&format!("{}:", line_num + 1));
                            }
                            output.push_str(line);
                            output.push('\n');
                        }
                    }
                }
            }

            if opts.files_with_matches && file_matched {
                output.push_str(filename);
                output.push('\n');
            } else if opts.count_only {
                if show_filename {
                    output.push_str(&format!("{}:{}\n", filename, match_count));
                } else {
                    output.push_str(&format!("{}\n", match_count));
                }
            }
        }

        if any_match {
            exit_code = 0;
        }

        Ok(ExecResult::with_code(output, exit_code))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::InMemoryFs;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Arc;

    async fn run_grep(args: &[&str], stdin: Option<&str>) -> Result<ExecResult> {
        let grep = Grep;
        let fs = Arc::new(InMemoryFs::new());
        let mut vars = HashMap::new();
        let mut cwd = PathBuf::from("/");
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();

        let ctx = Context {
            args: &args,
            env: &HashMap::new(),
            variables: &mut vars,
            cwd: &mut cwd,
            fs,
            stdin,
        };

        grep.execute(ctx).await
    }

    #[tokio::test]
    async fn test_grep_basic() {
        let result = run_grep(&["hello"], Some("hello world\ngoodbye world"))
            .await
            .unwrap();
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "hello world\n");
    }

    #[tokio::test]
    async fn test_grep_no_match() {
        let result = run_grep(&["xyz"], Some("hello world\ngoodbye world"))
            .await
            .unwrap();
        assert_eq!(result.exit_code, 1);
        assert_eq!(result.stdout, "");
    }

    #[tokio::test]
    async fn test_grep_case_insensitive() {
        let result = run_grep(&["-i", "HELLO"], Some("Hello World\ngoodbye"))
            .await
            .unwrap();
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "Hello World\n");
    }

    #[tokio::test]
    async fn test_grep_invert() {
        let result = run_grep(&["-v", "hello"], Some("hello\nworld\nhello again"))
            .await
            .unwrap();
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "world\n");
    }

    #[tokio::test]
    async fn test_grep_line_numbers() {
        let result = run_grep(&["-n", "world"], Some("hello\nworld\nfoo"))
            .await
            .unwrap();
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "2:world\n");
    }

    #[tokio::test]
    async fn test_grep_count() {
        let result = run_grep(&["-c", "o"], Some("hello\nworld\nfoo"))
            .await
            .unwrap();
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "3\n");
    }

    #[tokio::test]
    async fn test_grep_regex() {
        let result = run_grep(&["^h.*o$"], Some("hello\nworld\nhero"))
            .await
            .unwrap();
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "hello\nhero\n");
    }

    #[tokio::test]
    async fn test_grep_fixed_string() {
        let result = run_grep(&["-F", "a.b"], Some("a.b\naxb\na.b.c"))
            .await
            .unwrap();
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "a.b\na.b.c\n");
    }

    #[tokio::test]
    async fn test_grep_only_matching() {
        let result = run_grep(&["-o", "world"], Some("hello world\n"))
            .await
            .unwrap();
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "world\n");
    }

    #[tokio::test]
    async fn test_grep_only_matching_multiple() {
        let result = run_grep(&["-o", "o"], Some("hello world\nfoo"))
            .await
            .unwrap();
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "o\no\no\no\n");
    }

    #[tokio::test]
    async fn test_grep_word_boundary() {
        let result = run_grep(&["-w", "foo"], Some("foo\nfoobar\nbar foo baz"))
            .await
            .unwrap();
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "foo\nbar foo baz\n");
    }

    #[tokio::test]
    async fn test_grep_word_boundary_no_match() {
        let result = run_grep(&["-w", "bar"], Some("foobar\nbarbaz"))
            .await
            .unwrap();
        assert_eq!(result.exit_code, 1);
        assert_eq!(result.stdout, "");
    }

    #[tokio::test]
    async fn test_grep_files_with_matches_stdin() {
        let result = run_grep(&["-l", "foo"], Some("foo\nbar")).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "(stdin)\n");
    }
}
