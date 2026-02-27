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
//!   grep -P pattern file        # Perl regex (same as default)
//!   grep -q pattern file        # quiet mode (exit status only)
//!   grep -m N pattern file      # stop after N matches
//!   grep -x pattern file        # match whole line only
//!   grep -w pattern file        # match whole words only
//!   grep -A N pattern file      # show N lines after match
//!   grep -B N pattern file      # show N lines before match
//!   grep -C N pattern file      # show N lines before and after match
//!   grep -e pat1 -e pat2 file   # multiple patterns
//!   grep -f FILE pattern file   # read patterns from FILE
//!   grep -H pattern file        # always show filename
//!   grep -h pattern file        # never show filename
//!   grep -b pattern file        # show byte offset
//!   grep -a pattern file        # treat binary as text (filter null bytes)
//!   grep -z pattern file        # null-terminated lines
//!   grep -r pattern dir         # recursive search
//!   grep -L pattern file        # list non-matching files
//!   grep -s pattern file        # suppress error messages
//!   grep -Z pattern file        # null byte after filenames
//!   grep --exclude-dir=GLOB dir # skip directories matching GLOB
//!   grep --color=always pattern # color output (no-op)
//!   grep --line-buffered pattern # line-buffered (no-op)

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
    extended_regex: bool,
    only_matching: bool,
    word_regex: bool,
    quiet: bool,
    max_count: Option<usize>,
    whole_line: bool,
    after_context: usize,
    before_context: usize,
    show_filename: bool,               // -H: always show filename
    no_filename: bool,                 // -h: never show filename
    byte_offset: bool,                 // -b: show byte offset
    pattern_file: Option<String>,      // -f: read patterns from file
    null_terminated: bool,             // -z: null-terminated lines
    recursive: bool,                   // -r: recursive search
    binary_as_text: bool,              // -a: treat binary as text
    include_patterns: Vec<String>,     // --include=GLOB
    exclude_patterns: Vec<String>,     // --exclude=GLOB
    exclude_dir_patterns: Vec<String>, // --exclude-dir=GLOB
    files_without_matches: bool,       // -L: list non-matching files
    suppress_errors: bool,             // -s: suppress error messages
    null_filename: bool,               // -Z: null byte after filenames
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
            extended_regex: false,
            only_matching: false,
            word_regex: false,
            quiet: false,
            max_count: None,
            whole_line: false,
            after_context: 0,
            before_context: 0,
            show_filename: false,
            no_filename: false,
            byte_offset: false,
            pattern_file: None,
            null_terminated: false,
            recursive: false,
            binary_as_text: false,
            include_patterns: Vec::new(),
            exclude_patterns: Vec::new(),
            exclude_dir_patterns: Vec::new(),
            files_without_matches: false,
            suppress_errors: false,
            null_filename: false,
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
                        'E' => opts.extended_regex = true,
                        'P' => opts.extended_regex = true, // Perl regex implies ERE
                        'q' => opts.quiet = true,
                        'x' => opts.whole_line = true,
                        'H' => opts.show_filename = true,
                        'h' => opts.no_filename = true,
                        'b' => opts.byte_offset = true,
                        'a' => opts.binary_as_text = true,
                        'z' => opts.null_terminated = true,
                        'L' => opts.files_without_matches = true,
                        's' => opts.suppress_errors = true,
                        'Z' => opts.null_filename = true,
                        'r' | 'R' => opts.recursive = true,
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
                        'f' => {
                            // -f FILE (read patterns from file)
                            let rest: String = chars[j + 1..].iter().collect();
                            let file_path = if !rest.is_empty() {
                                rest
                            } else {
                                i += 1;
                                if i < args.len() {
                                    args[i].clone()
                                } else {
                                    return Err(Error::Execution(
                                        "grep: -f requires an argument".to_string(),
                                    ));
                                }
                            };
                            opts.pattern_file = Some(file_path);
                            break;
                        }
                        _ => {} // Ignore unknown flags
                    }
                    j += 1;
                }
            } else if let Some(opt) = arg.strip_prefix("--") {
                // Long options
                if opt.is_empty() {
                    // End of options
                    positional.extend(args[i + 1..].iter().cloned());
                    break;
                } else if opt == "color" || opt.starts_with("color=") {
                    // --color / --color=always/never/auto - no-op (we don't output ANSI)
                } else if opt == "line-buffered" {
                    // --line-buffered - no-op (output is already line-oriented)
                } else if let Some(pat) = opt.strip_prefix("include=") {
                    opts.include_patterns.push(strip_quotes(pat));
                } else if let Some(pat) = opt.strip_prefix("exclude=") {
                    opts.exclude_patterns.push(strip_quotes(pat));
                } else if let Some(pat) = opt.strip_prefix("exclude-dir=") {
                    opts.exclude_dir_patterns.push(strip_quotes(pat));
                } else if opt == "files-without-match" {
                    opts.files_without_matches = true;
                } else if opt == "no-messages" {
                    opts.suppress_errors = true;
                } else if opt == "null" {
                    opts.null_filename = true;
                }
                // Ignore other unknown long options
            } else {
                positional.push(arg.clone());
            }
            i += 1;
        }

        // First positional is pattern (if no -e patterns and no -f file)
        if opts.patterns.is_empty() && opts.pattern_file.is_none() {
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
                // Empty pattern matches everything (like .*)
                if p.is_empty() {
                    return ".*".to_string();
                }
                let pat = if self.fixed_strings {
                    regex::escape(p)
                } else if !self.extended_regex {
                    // BRE mode: convert to ERE for the regex crate
                    // In BRE: ( ) are literal, \( \) are groups
                    // In ERE/regex crate: ( ) are groups, \( \) are literal
                    bre_to_ere(p)
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

/// Strip surrounding single or double quotes from a value
fn strip_quotes(s: &str) -> String {
    if (s.starts_with('\'') && s.ends_with('\'')) || (s.starts_with('"') && s.ends_with('"')) {
        s[1..s.len() - 1].to_string()
    } else {
        s.to_string()
    }
}

/// Check if a filename matches a simple glob pattern (e.g., "*.txt", "*.log")
fn glob_matches(filename: &str, pattern: &str) -> bool {
    if let Some(suffix) = pattern.strip_prefix('*') {
        filename.ends_with(suffix)
    } else if let Some(prefix) = pattern.strip_suffix('*') {
        filename.starts_with(prefix)
    } else {
        filename == pattern
    }
}

/// Check if a filename should be included based on include/exclude patterns
fn should_include_file(filename: &str, include: &[String], exclude: &[String]) -> bool {
    if !include.is_empty() && !include.iter().any(|p| glob_matches(filename, p)) {
        return false;
    }
    if exclude.iter().any(|p| glob_matches(filename, p)) {
        return false;
    }
    true
}

/// Convert a BRE (Basic Regular Expression) pattern to ERE for the regex crate.
/// In BRE: ( ) { } are literal; \( \) \{ \} \+ \? \| are metacharacters.
/// In ERE/regex crate: ( ) { } + ? | are metacharacters.
fn bre_to_ere(pattern: &str) -> String {
    let mut result = String::with_capacity(pattern.len());
    let chars: Vec<char> = pattern.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        if chars[i] == '\\' && i + 1 < chars.len() {
            match chars[i + 1] {
                // BRE escaped metacharacters → ERE unescaped
                '(' | ')' | '{' | '}' | '+' | '?' | '|' => {
                    result.push(chars[i + 1]);
                    i += 2;
                }
                // Other escapes pass through
                _ => {
                    result.push('\\');
                    result.push(chars[i + 1]);
                    i += 2;
                }
            }
        } else if chars[i] == '(' || chars[i] == ')' || chars[i] == '{' || chars[i] == '}' {
            // BRE literal chars → escape them for ERE
            result.push('\\');
            result.push(chars[i]);
            i += 1;
        } else {
            result.push(chars[i]);
            i += 1;
        }
    }

    result
}

#[async_trait]
impl Builtin for Grep {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let mut opts = GrepOptions::parse(ctx.args)?;

        // Load patterns from file if -f was specified
        if let Some(ref pattern_file) = opts.pattern_file {
            let path = if pattern_file.starts_with('/') {
                std::path::PathBuf::from(pattern_file)
            } else {
                ctx.cwd.join(pattern_file)
            };
            match ctx.fs.read_file(&path).await {
                Ok(content) => {
                    let text = String::from_utf8_lossy(&content);
                    for line in text.lines() {
                        if !line.is_empty() {
                            opts.patterns.push(line.to_string());
                        }
                    }
                }
                Err(e) => {
                    return Err(Error::Execution(format!("grep: {}: {}", pattern_file, e)));
                }
            }
        }

        // Ensure we have at least one pattern
        if opts.patterns.is_empty() {
            return Err(Error::Execution("grep: missing pattern".to_string()));
        }

        let regex = opts.build_regex()?;

        let mut output = String::new();
        let mut any_match = false;
        let mut exit_code = 1; // 1 = no match
        let mut total_matches = 0usize;

        // Determine input sources
        // Use "(standard input)" for -H flag, "(stdin)" for -l flag
        let stdin_name = if opts.show_filename {
            "(standard input)"
        } else if opts.files_with_matches || opts.files_without_matches {
            "(stdin)"
        } else {
            ""
        };
        // Helper to process content (filter null bytes if binary_as_text)
        let process_content = |content: Vec<u8>, binary_as_text: bool| -> String {
            if binary_as_text {
                // Filter out null bytes for proper regex matching
                let filtered: Vec<u8> = content.into_iter().filter(|&b| b != 0).collect();
                String::from_utf8_lossy(&filtered).into_owned()
            } else {
                String::from_utf8_lossy(&content).into_owned()
            }
        };

        let inputs: Vec<(String, String)> = if opts.files.is_empty() {
            // Read from stdin
            let mut stdin_content = ctx.stdin.unwrap_or("").to_string();
            if opts.binary_as_text {
                // Filter null bytes for -a flag
                stdin_content = stdin_content.replace('\0', "");
            }
            vec![(stdin_name.to_string(), stdin_content)]
        } else if opts.recursive {
            // Recursive mode: traverse directories
            let mut inputs = Vec::new();
            let mut dirs_to_process: Vec<std::path::PathBuf> = Vec::new();

            for file in &opts.files {
                let path = if file.starts_with('/') {
                    std::path::PathBuf::from(file)
                } else {
                    ctx.cwd.join(file)
                };
                dirs_to_process.push(path);
            }

            while let Some(path) = dirs_to_process.pop() {
                if let Ok(entries) = ctx.fs.read_dir(&path).await {
                    for entry in entries {
                        let entry_path = path.join(&entry.name);
                        if entry.metadata.file_type.is_dir() {
                            // Skip dirs matching --exclude-dir patterns
                            if opts
                                .exclude_dir_patterns
                                .iter()
                                .any(|p| glob_matches(&entry.name, p))
                            {
                                continue;
                            }
                            dirs_to_process.push(entry_path);
                        } else if entry.metadata.file_type.is_file()
                            && should_include_file(
                                &entry.name,
                                &opts.include_patterns,
                                &opts.exclude_patterns,
                            )
                        {
                            if let Ok(content) = ctx.fs.read_file(&entry_path).await {
                                let text = process_content(content, opts.binary_as_text);
                                inputs.push((entry_path.to_string_lossy().into_owned(), text));
                            }
                        }
                    }
                } else if let Ok(content) = ctx.fs.read_file(&path).await {
                    // It's a file, not a directory
                    let text = process_content(content, opts.binary_as_text);
                    inputs.push((path.to_string_lossy().into_owned(), text));
                }
            }
            inputs
        } else {
            // Read from specified files
            let mut inputs = Vec::new();
            for file in &opts.files {
                let path = if file.starts_with('/') {
                    std::path::PathBuf::from(file)
                } else {
                    ctx.cwd.join(file)
                };

                match ctx.fs.read_file(&path).await {
                    Ok(content) => {
                        let text = process_content(content, opts.binary_as_text);
                        inputs.push((file.clone(), text));
                    }
                    Err(e) => {
                        // Report error but continue with other files
                        if !opts.quiet && !opts.suppress_errors {
                            output.push_str(&format!("grep: {}: {}\n", file, e));
                        }
                    }
                }
            }
            inputs
        };

        // -H forces filename display, -h suppresses it, otherwise show for multiple files/recursive
        let show_filename = if opts.no_filename {
            false
        } else if opts.show_filename || opts.recursive {
            true
        } else {
            inputs.len() > 1
        };
        let has_context = opts.before_context > 0 || opts.after_context > 0;

        let mut max_reached = false;

        'file_loop: for (filename, content) in &inputs {
            // Check if we already reached max count from previous files
            if let Some(max) = opts.max_count {
                if total_matches >= max {
                    break 'file_loop;
                }
            }

            let mut match_count = 0;
            let mut file_matched = false;

            // Binary detection: content with null bytes, -a and -z not set
            let is_binary = !opts.binary_as_text && !opts.null_terminated && content.contains('\0');

            // Split on null bytes if -z flag is set, otherwise split on newlines
            let lines: Vec<&str> = if opts.null_terminated {
                content.split('\0').collect()
            } else {
                content.lines().collect()
            };

            // Calculate byte offsets for each line (for -b flag)
            let byte_offsets: Vec<usize> = if opts.byte_offset {
                let mut offsets = Vec::with_capacity(lines.len());
                let mut offset = 0usize;
                for line in &lines {
                    offsets.push(offset);
                    offset += line.len() + 1; // +1 for newline or null byte
                }
                offsets
            } else {
                Vec::new()
            };

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
                        if !opts.files_without_matches {
                            any_match = true;
                        }
                        match_count += 1;
                        total_matches += 1;

                        if opts.files_with_matches || opts.files_without_matches || opts.quiet {
                            break;
                        }

                        if let Some(max) = opts.max_count {
                            if total_matches >= max {
                                max_reached = true;
                                break;
                            }
                        }
                    }
                    if (opts.files_with_matches || opts.files_without_matches) && file_matched {
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
                        if !opts.files_without_matches {
                            any_match = true;
                        }
                        match_count += 1;
                        total_matches += 1;
                        match_lines.push(line_num);

                        if opts.files_with_matches || opts.files_without_matches {
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
            // Binary file: just report "Binary file X matches" instead of lines
            if is_binary
                && file_matched
                && !opts.count_only
                && !opts.files_with_matches
                && !opts.files_without_matches
            {
                let display_name = if filename.is_empty() {
                    "(standard input)"
                } else {
                    filename.as_str()
                };
                output.push_str(&format!("Binary file {} matches\n", display_name));
                continue 'file_loop;
            }
            // Filename terminator: \0 for -Z, \n otherwise
            let fname_term = if opts.null_filename { '\0' } else { '\n' };
            // Filename separator in line output: \0 for -Z, : otherwise
            let fname_sep = if opts.null_filename { '\0' } else { ':' };
            if opts.files_with_matches && file_matched {
                output.push_str(filename);
                output.push(fname_term);
            } else if opts.files_without_matches && !file_matched {
                output.push_str(filename);
                output.push(fname_term);
                // -L means at least one file printed => success
                any_match = true;
            } else if opts.files_without_matches {
                // -L mode but file matched: skip output for this file
            } else if opts.count_only {
                if show_filename {
                    output.push_str(&format!("{}{}{}\n", filename, fname_sep, match_count));
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
                                output.push(fname_sep);
                            }
                            if opts.byte_offset {
                                output.push_str(&format!("{}:", byte_offsets[line_num]));
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
                        let separator = if is_match { fname_sep } else { '-' };

                        if show_filename {
                            output.push_str(filename);
                            output.push(separator);
                        }
                        if opts.byte_offset {
                            output.push_str(&format!("{}{}", byte_offsets[line_idx], separator));
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
                            output.push(fname_sep);
                        }
                        if opts.byte_offset {
                            output.push_str(&format!("{}:", byte_offsets[line_idx]));
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
    use crate::fs::{FileSystem, InMemoryFs};
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
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
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

    #[test]
    fn test_glob_matches() {
        assert!(glob_matches("file.txt", "*.txt"));
        assert!(!glob_matches("file.log", "*.txt"));
        assert!(glob_matches("readme.md", "readme*"));
        assert!(!glob_matches("license.md", "readme*"));
        assert!(glob_matches("exact.txt", "exact.txt"));
        assert!(!glob_matches("other.txt", "exact.txt"));
    }

    #[test]
    fn test_should_include_file() {
        assert!(should_include_file("foo.txt", &[], &[]));

        let inc = vec!["*.txt".to_string()];
        assert!(should_include_file("foo.txt", &inc, &[]));
        assert!(!should_include_file("foo.log", &inc, &[]));

        let exc = vec!["*.log".to_string()];
        assert!(should_include_file("foo.txt", &[], &exc));
        assert!(!should_include_file("foo.log", &[], &exc));

        assert!(should_include_file("foo.txt", &inc, &exc));
        assert!(!should_include_file("foo.log", &inc, &exc));
    }

    #[tokio::test]
    async fn test_grep_recursive_include() {
        let grep = Grep;
        let fs = Arc::new(InMemoryFs::new());
        fs.mkdir(&PathBuf::from("/dir"), true).await.unwrap();
        fs.write_file(&PathBuf::from("/dir/a.txt"), b"hello\n")
            .await
            .unwrap();
        fs.write_file(&PathBuf::from("/dir/b.log"), b"hello\n")
            .await
            .unwrap();

        let mut vars = HashMap::new();
        let mut cwd = PathBuf::from("/");
        let args: Vec<String> = ["-r", "--include=*.txt", "hello", "/dir"]
            .iter()
            .map(|s| s.to_string())
            .collect();

        let ctx = Context {
            args: &args,
            env: &HashMap::new(),
            variables: &mut vars,
            cwd: &mut cwd,
            fs,
            stdin: None,
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
        };

        let result = grep.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("/dir/a.txt:hello"));
        assert!(!result.stdout.contains("b.log"));
    }

    #[tokio::test]
    async fn test_grep_recursive_exclude() {
        let grep = Grep;
        let fs = Arc::new(InMemoryFs::new());
        fs.mkdir(&PathBuf::from("/dir"), true).await.unwrap();
        fs.write_file(&PathBuf::from("/dir/a.txt"), b"hello\n")
            .await
            .unwrap();
        fs.write_file(&PathBuf::from("/dir/b.log"), b"hello\n")
            .await
            .unwrap();

        let mut vars = HashMap::new();
        let mut cwd = PathBuf::from("/");
        let args: Vec<String> = ["-r", "--exclude=*.log", "hello", "/dir"]
            .iter()
            .map(|s| s.to_string())
            .collect();

        let ctx = Context {
            args: &args,
            env: &HashMap::new(),
            variables: &mut vars,
            cwd: &mut cwd,
            fs,
            stdin: None,
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
        };

        let result = grep.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("/dir/a.txt:hello"));
        assert!(!result.stdout.contains("b.log"));
    }

    // -L (--files-without-match) tests

    #[tokio::test]
    async fn test_grep_files_without_match_stdin() {
        let result = run_grep(&["-L", "xyz"], Some("foo\nbar")).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "(stdin)\n");
    }

    #[tokio::test]
    async fn test_grep_files_without_match_stdin_has_match() {
        let result = run_grep(&["-L", "foo"], Some("foo\nbar")).await.unwrap();
        assert_eq!(result.exit_code, 1);
        assert_eq!(result.stdout, "");
    }

    #[tokio::test]
    async fn test_grep_files_without_match_long_flag() {
        let result = run_grep(&["--files-without-match", "xyz"], Some("foo\nbar"))
            .await
            .unwrap();
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "(stdin)\n");
    }

    #[tokio::test]
    async fn test_grep_files_without_match_with_files() {
        let grep = Grep;
        let fs = Arc::new(InMemoryFs::new());
        fs.mkdir(&PathBuf::from("/dir"), true).await.unwrap();
        fs.write_file(&PathBuf::from("/dir/a.txt"), b"hello\n")
            .await
            .unwrap();
        fs.write_file(&PathBuf::from("/dir/b.txt"), b"world\n")
            .await
            .unwrap();

        let mut vars = HashMap::new();
        let mut cwd = PathBuf::from("/");
        let args: Vec<String> = ["-L", "hello", "/dir/a.txt", "/dir/b.txt"]
            .iter()
            .map(|s| s.to_string())
            .collect();

        let ctx = Context {
            args: &args,
            env: &HashMap::new(),
            variables: &mut vars,
            cwd: &mut cwd,
            fs,
            stdin: None,
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
        };

        let result = grep.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "/dir/b.txt\n");
    }

    // --exclude-dir tests

    #[tokio::test]
    async fn test_grep_exclude_dir() {
        let grep = Grep;
        let fs = Arc::new(InMemoryFs::new());
        fs.mkdir(&PathBuf::from("/proj/src"), true).await.unwrap();
        fs.mkdir(&PathBuf::from("/proj/vendor"), true)
            .await
            .unwrap();
        fs.write_file(&PathBuf::from("/proj/src/main.rs"), b"hello\n")
            .await
            .unwrap();
        fs.write_file(&PathBuf::from("/proj/vendor/lib.rs"), b"hello\n")
            .await
            .unwrap();

        let mut vars = HashMap::new();
        let mut cwd = PathBuf::from("/");
        let args: Vec<String> = ["-r", "--exclude-dir=vendor", "hello", "/proj"]
            .iter()
            .map(|s| s.to_string())
            .collect();

        let ctx = Context {
            args: &args,
            env: &HashMap::new(),
            variables: &mut vars,
            cwd: &mut cwd,
            fs,
            stdin: None,
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
        };

        let result = grep.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("/proj/src/main.rs:hello"));
        assert!(!result.stdout.contains("vendor"));
    }

    #[tokio::test]
    async fn test_grep_exclude_dir_glob() {
        let grep = Grep;
        let fs = Arc::new(InMemoryFs::new());
        fs.mkdir(&PathBuf::from("/proj/src"), true).await.unwrap();
        fs.mkdir(&PathBuf::from("/proj/.git"), true).await.unwrap();
        fs.write_file(&PathBuf::from("/proj/src/main.rs"), b"hello\n")
            .await
            .unwrap();
        fs.write_file(&PathBuf::from("/proj/.git/config"), b"hello\n")
            .await
            .unwrap();

        let mut vars = HashMap::new();
        let mut cwd = PathBuf::from("/");
        let args: Vec<String> = ["-r", "--exclude-dir=.*", "hello", "/proj"]
            .iter()
            .map(|s| s.to_string())
            .collect();

        let ctx = Context {
            args: &args,
            env: &HashMap::new(),
            variables: &mut vars,
            cwd: &mut cwd,
            fs,
            stdin: None,
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
        };

        let result = grep.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("/proj/src/main.rs:hello"));
        assert!(!result.stdout.contains(".git"));
    }

    // -s (--no-messages) tests

    #[tokio::test]
    async fn test_grep_suppress_errors() {
        let grep = Grep;
        let fs = Arc::new(InMemoryFs::new());

        let mut vars = HashMap::new();
        let mut cwd = PathBuf::from("/");
        let args: Vec<String> = ["-s", "hello", "/nonexistent"]
            .iter()
            .map(|s| s.to_string())
            .collect();

        let ctx = Context {
            args: &args,
            env: &HashMap::new(),
            variables: &mut vars,
            cwd: &mut cwd,
            fs,
            stdin: None,
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
        };

        let result = grep.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 1);
        // -s suppresses error messages
        assert_eq!(result.stdout, "");
    }

    #[tokio::test]
    async fn test_grep_no_suppress_errors() {
        let grep = Grep;
        let fs = Arc::new(InMemoryFs::new());

        let mut vars = HashMap::new();
        let mut cwd = PathBuf::from("/");
        let args: Vec<String> = ["hello", "/nonexistent"]
            .iter()
            .map(|s| s.to_string())
            .collect();

        let ctx = Context {
            args: &args,
            env: &HashMap::new(),
            variables: &mut vars,
            cwd: &mut cwd,
            fs,
            stdin: None,
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
        };

        let result = grep.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 1);
        // Without -s, error message is shown
        assert!(result.stdout.contains("grep: /nonexistent:"));
    }

    #[tokio::test]
    async fn test_grep_suppress_errors_long_flag() {
        let grep = Grep;
        let fs = Arc::new(InMemoryFs::new());

        let mut vars = HashMap::new();
        let mut cwd = PathBuf::from("/");
        let args: Vec<String> = ["--no-messages", "hello", "/nonexistent"]
            .iter()
            .map(|s| s.to_string())
            .collect();

        let ctx = Context {
            args: &args,
            env: &HashMap::new(),
            variables: &mut vars,
            cwd: &mut cwd,
            fs,
            stdin: None,
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
        };

        let result = grep.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 1);
        assert_eq!(result.stdout, "");
    }

    // -Z (--null) tests

    #[tokio::test]
    async fn test_grep_null_filename_with_l() {
        let result = run_grep(&["-lZ", "foo"], Some("foo\nbar")).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "(stdin)\0");
    }

    #[tokio::test]
    async fn test_grep_null_filename_with_big_l() {
        let result = run_grep(&["-LZ", "xyz"], Some("foo\nbar")).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "(stdin)\0");
    }

    #[tokio::test]
    async fn test_grep_null_filename_with_h() {
        let grep = Grep;
        let fs = Arc::new(InMemoryFs::new());
        fs.write_file(&PathBuf::from("/a.txt"), b"hello\n")
            .await
            .unwrap();
        fs.write_file(&PathBuf::from("/b.txt"), b"hello\n")
            .await
            .unwrap();

        let mut vars = HashMap::new();
        let mut cwd = PathBuf::from("/");
        let args: Vec<String> = ["-Z", "hello", "/a.txt", "/b.txt"]
            .iter()
            .map(|s| s.to_string())
            .collect();

        let ctx = Context {
            args: &args,
            env: &HashMap::new(),
            variables: &mut vars,
            cwd: &mut cwd,
            fs,
            stdin: None,
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
        };

        let result = grep.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        // -Z uses \0 after filename instead of :
        assert!(result.stdout.contains("/a.txt\0hello"));
        assert!(result.stdout.contains("/b.txt\0hello"));
    }

    #[tokio::test]
    async fn test_grep_null_filename_long_flag() {
        let result = run_grep(&["-l", "--null", "foo"], Some("foo\nbar"))
            .await
            .unwrap();
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "(stdin)\0");
    }
}
