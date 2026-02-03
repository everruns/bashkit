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
//!   grep -q pattern file        # quiet mode (exit status only)
//!   grep -m N pattern file      # stop after N matches
//!   grep -x pattern file        # match whole line only
//!   grep -A N pattern file      # show N lines after match
//!   grep -B N pattern file      # show N lines before match
//!   grep -C N pattern file      # show N lines before and after match
//!   grep -e pat1 -e pat2 file   # multiple patterns

use async_trait::async_trait;
use regex::{Regex, RegexBuilder};

use super::{Builtin, Context};
use crate::error::{Error, Result};
use crate::interpreter::ExecResult;

/// grep command - pattern matching
pub struct Grep;

struct GrepOptions {
    patterns: Vec<String>,
    files: Vec<String>,
    ignore_case: bool,
    invert_match: bool,
    line_numbers: bool,
    count_only: bool,
    files_with_matches: bool,
    fixed_strings: bool,
    only_matching: bool,
    word_regex: bool,
    quiet: bool,
    max_count: Option<usize>,
    whole_line: bool,
    after_context: usize,
    before_context: usize,
}

impl GrepOptions {
    fn parse(args: &[String]) -> Result<Self> {
        let mut opts = GrepOptions {
            patterns: Vec::new(),
            files: Vec::new(),
            ignore_case: false,
            invert_match: false,
            line_numbers: false,
            count_only: false,
            files_with_matches: false,
            fixed_strings: false,
            only_matching: false,
            word_regex: false,
            quiet: false,
            max_count: None,
            whole_line: false,
            after_context: 0,
            before_context: 0,
        };

        let mut positional = Vec::new();
        let mut i = 0;

        while i < args.len() {
            let arg = &args[i];
            if arg.starts_with('-') && arg.len() > 1 && !arg.starts_with("--") {
                // Handle combined flags like -iv
                let chars: Vec<char> = arg[1..].chars().collect();
                let mut j = 0;
                while j < chars.len() {
                    let c = chars[j];
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
                        'q' => opts.quiet = true,
                        'x' => opts.whole_line = true,
                        'h' => {} // No filename prefix (already default for single file)
                        'e' => {
                            // -e pattern (remaining chars or next arg)
                            let rest: String = chars[j + 1..].iter().collect();
                            if !rest.is_empty() {
                                opts.patterns.push(rest);
                            } else {
                                i += 1;
                                if i < args.len() {
                                    opts.patterns.push(args[i].clone());
                                }
                            }
                            break; // Consumed rest of this arg
                        }
                        'm' => {
                            // -m N (remaining chars or next arg)
                            let rest: String = chars[j + 1..].iter().collect();
                            let num_str = if !rest.is_empty() {
                                rest
                            } else {
                                i += 1;
                                if i < args.len() {
                                    args[i].clone()
                                } else {
                                    return Err(Error::Execution(
                                        "grep: -m requires an argument".to_string(),
                                    ));
                                }
                            };
                            opts.max_count = Some(num_str.parse().map_err(|_| {
                                Error::Execution(format!("grep: invalid max count: {}", num_str))
                            })?);
                            break; // Consumed rest of this arg
                        }
                        'A' => {
                            // -A N (after context)
                            let rest: String = chars[j + 1..].iter().collect();
                            let num_str = if !rest.is_empty() {
                                rest
                            } else {
                                i += 1;
                                if i < args.len() {
                                    args[i].clone()
                                } else {
                                    return Err(Error::Execution(
                                        "grep: -A requires an argument".to_string(),
                                    ));
                                }
                            };
                            opts.after_context = num_str.parse().map_err(|_| {
                                Error::Execution(format!(
                                    "grep: invalid context length: {}",
                                    num_str
                                ))
                            })?;
                            break;
                        }
                        'B' => {
                            // -B N (before context)
                            let rest: String = chars[j + 1..].iter().collect();
                            let num_str = if !rest.is_empty() {
                                rest
                            } else {
                                i += 1;
                                if i < args.len() {
                                    args[i].clone()
                                } else {
                                    return Err(Error::Execution(
                                        "grep: -B requires an argument".to_string(),
                                    ));
                                }
                            };
                            opts.before_context = num_str.parse().map_err(|_| {
                                Error::Execution(format!(
                                    "grep: invalid context length: {}",
                                    num_str
                                ))
                            })?;
                            break;
                        }
                        'C' => {
                            // -C N (context before and after)
                            let rest: String = chars[j + 1..].iter().collect();
                            let num_str = if !rest.is_empty() {
                                rest
                            } else {
                                i += 1;
                                if i < args.len() {
                                    args[i].clone()
                                } else {
                                    return Err(Error::Execution(
                                        "grep: -C requires an argument".to_string(),
                                    ));
                                }
                            };
                            let ctx: usize = num_str.parse().map_err(|_| {
                                Error::Execution(format!(
                                    "grep: invalid context length: {}",
                                    num_str
                                ))
                            })?;
                            opts.before_context = ctx;
                            opts.after_context = ctx;
                            break;
                        }
                        _ => {} // Ignore unknown flags
                    }
                    j += 1;
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

        // First positional is pattern (if no -e patterns)
        if opts.patterns.is_empty() {
            if positional.is_empty() {
                return Err(Error::Execution("grep: missing pattern".to_string()));
            }
            opts.patterns.push(positional.remove(0));
        }

        // Rest are files
        opts.files = positional;

        Ok(opts)
    }

    fn build_regex(&self) -> Result<Regex> {
        // Build patterns for each -e pattern
        let escaped_patterns: Vec<String> = self
            .patterns
            .iter()
            .map(|p| {
                let pat = if self.fixed_strings {
                    regex::escape(p)
                } else {
                    p.clone()
                };
                // Wrap with word boundaries if -w flag is set
                if self.word_regex {
                    format!(r"\b{}\b", pat)
                } else {
                    pat
                }
            })
            .collect();

        // Combine multiple patterns with alternation
        let combined = if escaped_patterns.len() == 1 {
            escaped_patterns[0].clone()
        } else {
            escaped_patterns
                .iter()
                .map(|p| format!("(?:{})", p))
                .collect::<Vec<_>>()
                .join("|")
        };

        // Wrap for whole-line matching if -x flag is set
        let final_pattern = if self.whole_line {
            format!("^(?:{})$", combined)
        } else {
            combined
        };

        RegexBuilder::new(&final_pattern)
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
        let mut total_matches = 0usize;

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
                        if !opts.quiet {
                            output.push_str(&format!("grep: {}: {}\n", file, e));
                        }
                    }
                }
            }
            inputs
        };

        let show_filename = opts.files.len() > 1;
        let has_context = opts.before_context > 0 || opts.after_context > 0;

        let mut max_reached = false;

        'file_loop: for (filename, content) in inputs {
            // Check if we already reached max count from previous files
            if let Some(max) = opts.max_count {
                if total_matches >= max {
                    break 'file_loop;
                }
            }

            let mut match_count = 0;
            let mut file_matched = false;
            let lines: Vec<&str> = content.lines().collect();

            // For context output, track which lines have been printed
            // Use a set of line indices that should be printed
            let mut printed_lines: std::collections::HashSet<usize> =
                std::collections::HashSet::new();
            let mut match_lines: Vec<usize> = Vec::new();

            // First pass: find all matching lines (up to max_count)
            for (line_num, line) in lines.iter().enumerate() {
                // Check max count limit before adding more matches
                if let Some(max) = opts.max_count {
                    if total_matches >= max {
                        max_reached = true;
                        break; // Break inner loop, continue to output phase
                    }
                }

                if opts.only_matching && !opts.invert_match {
                    // -o mode: count each match separately
                    for _ in regex.find_iter(line) {
                        file_matched = true;
                        any_match = true;
                        match_count += 1;
                        total_matches += 1;

                        if opts.files_with_matches || opts.quiet {
                            break;
                        }

                        if let Some(max) = opts.max_count {
                            if total_matches >= max {
                                max_reached = true;
                                break;
                            }
                        }
                    }
                    if opts.files_with_matches && file_matched {
                        break;
                    }
                    if opts.quiet && file_matched {
                        break 'file_loop;
                    }
                    if max_reached {
                        break;
                    }
                } else {
                    let matches = regex.is_match(line);
                    let should_match = if opts.invert_match { !matches } else { matches };

                    if should_match {
                        file_matched = true;
                        any_match = true;
                        match_count += 1;
                        total_matches += 1;
                        match_lines.push(line_num);

                        if opts.files_with_matches {
                            break;
                        }
                        if opts.quiet {
                            break 'file_loop;
                        }

                        // Check max after recording this match
                        if let Some(max) = opts.max_count {
                            if total_matches >= max {
                                max_reached = true;
                                break;
                            }
                        }
                    }
                }
            }

            // If quiet mode and we found a match, we're done
            if opts.quiet && any_match {
                break 'file_loop;
            }

            // Now generate output
            if opts.files_with_matches && file_matched {
                output.push_str(filename);
                output.push('\n');
            } else if opts.count_only {
                if show_filename {
                    output.push_str(&format!("{}:{}\n", filename, match_count));
                } else {
                    output.push_str(&format!("{}\n", match_count));
                }
            } else if !opts.quiet {
                if opts.only_matching && !opts.invert_match {
                    // -o mode: output each match
                    let mut o_matches = 0usize;
                    for (line_num, line) in lines.iter().enumerate() {
                        for mat in regex.find_iter(line) {
                            if let Some(max) = opts.max_count {
                                if o_matches >= max {
                                    break;
                                }
                            }
                            if show_filename {
                                output.push_str(filename);
                                output.push(':');
                            }
                            if opts.line_numbers {
                                output.push_str(&format!("{}:", line_num + 1));
                            }
                            output.push_str(mat.as_str());
                            output.push('\n');
                            o_matches += 1;
                        }
                        if let Some(max) = opts.max_count {
                            if o_matches >= max {
                                break;
                            }
                        }
                    }
                } else if has_context {
                    // Context mode: calculate which lines to print
                    // match_lines already respects max_count from the first pass
                    for &match_idx in &match_lines {
                        let start = match_idx.saturating_sub(opts.before_context);
                        let end = (match_idx + opts.after_context + 1).min(lines.len());
                        for i in start..end {
                            printed_lines.insert(i);
                        }
                    }

                    // Output lines in order
                    let mut sorted_lines: Vec<usize> = printed_lines.iter().copied().collect();
                    sorted_lines.sort_unstable();

                    let mut prev_line: Option<usize> = None;
                    for line_idx in sorted_lines {
                        // Print separator if there's a gap
                        if let Some(prev) = prev_line {
                            if line_idx > prev + 1 {
                                output.push_str("--\n");
                            }
                        }
                        prev_line = Some(line_idx);

                        // Determine if this is a match line or context line
                        let is_match = match_lines.contains(&line_idx);
                        let separator = if is_match { ':' } else { '-' };

                        if show_filename {
                            output.push_str(filename);
                            output.push(separator);
                        }
                        if opts.line_numbers {
                            output.push_str(&format!("{}{}", line_idx + 1, separator));
                        }
                        output.push_str(lines[line_idx]);
                        output.push('\n');
                    }
                } else {
                    // Normal mode: output matching lines
                    for (out_count, &line_idx) in match_lines.iter().enumerate() {
                        if let Some(max) = opts.max_count {
                            if out_count >= max {
                                break;
                            }
                        }
                        if show_filename {
                            output.push_str(filename);
                            output.push(':');
                        }
                        if opts.line_numbers {
                            output.push_str(&format!("{}:", line_idx + 1));
                        }
                        output.push_str(lines[line_idx]);
                        output.push('\n');
                    }
                }
            }
        }

        if any_match {
            exit_code = 0;
        }

        // In quiet mode, return empty output
        if opts.quiet {
            return Ok(ExecResult::with_code(String::new(), exit_code));
        }

        Ok(ExecResult::with_code(output, exit_code))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
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
            #[cfg(feature = "network")]
            http_client: None,
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
