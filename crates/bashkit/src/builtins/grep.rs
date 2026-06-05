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

use super::search_common::{
    Matcher, build_fancy_matcher, build_regex_opts, parse_numeric_flag_arg,
};
use super::{Builtin, Context};
use crate::error::{Error, Result};
use crate::interpreter::ExecResult;

/// grep command - pattern matching
pub struct Grep;

/// Which pattern syntax to compile with. `-G/-E/-F/-P` select one of these and
/// are mutually exclusive (GNU grep: the last one on the command line wins).
enum PatternType {
    Basic,    // -G: basic regular expressions (the default)
    Extended, // -E: extended regular expressions
    Fixed,    // -F: literal fixed strings
    Perl,     // -P: Perl-compatible (PCRE) via fancy-regex
}

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
    perl_regex: bool, // -P: Perl-compatible (PCRE) via fancy-regex
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
            perl_regex: false,
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
                        // Pattern type: -G/-E/-F/-P are mutually exclusive,
                        // last one wins (GNU grep semantics).
                        'F' => opts.set_pattern_type(PatternType::Fixed),
                        'E' => opts.set_pattern_type(PatternType::Extended),
                        'G' => opts.set_pattern_type(PatternType::Basic),
                        'P' => opts.set_pattern_type(PatternType::Perl),
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
                            opts.max_count = Some(parse_numeric_flag_arg(
                                &chars, j, &mut i, args, "grep", "-m",
                            )?);
                            break;
                        }
                        'A' => {
                            opts.after_context =
                                parse_numeric_flag_arg(&chars, j, &mut i, args, "grep", "-A")?;
                            break;
                        }
                        'B' => {
                            opts.before_context =
                                parse_numeric_flag_arg(&chars, j, &mut i, args, "grep", "-B")?;
                            break;
                        }
                        'C' => {
                            let ctx =
                                parse_numeric_flag_arg(&chars, j, &mut i, args, "grep", "-C")?;
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
                // Long options. GNU getopt_long accepts both `--name=value` and
                // `--name value`; split the inline form here and fall back to
                // the next argv entry for value-taking options.
                if opt.is_empty() {
                    // End of options
                    positional.extend(args[i + 1..].iter().cloned());
                    break;
                }
                let (name, inline_val) = match opt.split_once('=') {
                    Some((n, v)) => (n, Some(v.to_string())),
                    None => (opt, None),
                };
                match name {
                    // Boolean flags — long-form aliases of the short flags.
                    "ignore-case" => opts.ignore_case = true,
                    "no-ignore-case" => opts.ignore_case = false,
                    "invert-match" => opts.invert_match = true,
                    "line-number" => opts.line_numbers = true,
                    "count" => opts.count_only = true,
                    "files-with-matches" => opts.files_with_matches = true,
                    "files-without-match" => opts.files_without_matches = true,
                    "only-matching" => opts.only_matching = true,
                    "word-regexp" => opts.word_regex = true,
                    "line-regexp" => opts.whole_line = true,
                    // Pattern type: mutually exclusive, last one wins.
                    "fixed-strings" => opts.set_pattern_type(PatternType::Fixed),
                    "extended-regexp" => opts.set_pattern_type(PatternType::Extended),
                    "basic-regexp" => opts.set_pattern_type(PatternType::Basic),
                    "perl-regexp" => opts.set_pattern_type(PatternType::Perl),
                    "quiet" | "silent" => opts.quiet = true,
                    "byte-offset" => opts.byte_offset = true,
                    "text" => opts.binary_as_text = true,
                    "null-data" => opts.null_terminated = true,
                    "recursive" => opts.recursive = true,
                    "no-messages" => opts.suppress_errors = true,
                    "with-filename" => opts.show_filename = true,
                    "no-filename" => opts.no_filename = true,
                    "null" => opts.null_filename = true,
                    // No-ops: output is already line-oriented and uncolored.
                    "color" | "colour" | "line-buffered" => {}
                    // Value-taking options.
                    "regexp" => {
                        opts.patterns
                            .push(long_opt_value(&inline_val, name, &mut i, args)?)
                    }
                    "file" => {
                        opts.pattern_file = Some(long_opt_value(&inline_val, name, &mut i, args)?)
                    }
                    "max-count" => {
                        opts.max_count = Some(parse_long_numeric(
                            &long_opt_value(&inline_val, name, &mut i, args)?,
                            name,
                        )?)
                    }
                    "after-context" => {
                        opts.after_context = parse_long_numeric(
                            &long_opt_value(&inline_val, name, &mut i, args)?,
                            name,
                        )?
                    }
                    "before-context" => {
                        opts.before_context = parse_long_numeric(
                            &long_opt_value(&inline_val, name, &mut i, args)?,
                            name,
                        )?
                    }
                    "context" => {
                        let ctx = parse_long_numeric(
                            &long_opt_value(&inline_val, name, &mut i, args)?,
                            name,
                        )?;
                        opts.before_context = ctx;
                        opts.after_context = ctx;
                    }
                    "include" => opts.include_patterns.push(strip_quotes(&long_opt_value(
                        &inline_val,
                        name,
                        &mut i,
                        args,
                    )?)),
                    "exclude" => opts.exclude_patterns.push(strip_quotes(&long_opt_value(
                        &inline_val,
                        name,
                        &mut i,
                        args,
                    )?)),
                    "exclude-dir" => opts.exclude_dir_patterns.push(strip_quotes(&long_opt_value(
                        &inline_val,
                        name,
                        &mut i,
                        args,
                    )?)),
                    // Ignore other unknown long options.
                    _ => {}
                }
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

    /// Select the pattern syntax, clearing any previously-selected type so the
    /// last `-G/-E/-F/-P` flag wins (GNU grep semantics).
    fn set_pattern_type(&mut self, pt: PatternType) {
        self.fixed_strings = matches!(pt, PatternType::Fixed);
        self.extended_regex = matches!(pt, PatternType::Extended);
        self.perl_regex = matches!(pt, PatternType::Perl);
    }

    fn build_matcher(&self) -> Result<Matcher> {
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
                } else if self.perl_regex {
                    // PCRE mode (-P): pass through to fancy-regex unchanged so
                    // lookaround / backreferences are preserved.
                    p.clone()
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

        // -P uses the backtracking PCRE engine; everything else uses the
        // default linear-time engine. `-F` (fixed strings) takes precedence
        // over `-P`, matching GNU grep.
        if self.perl_regex && !self.fixed_strings {
            build_fancy_matcher(&final_pattern, self.ignore_case)
                .map_err(|e| Error::Execution(format!("grep: invalid pattern: {}", e)))
        } else {
            build_regex_opts(&final_pattern, self.ignore_case)
                .map(Matcher::Standard)
                .map_err(|e| Error::Execution(format!("grep: invalid pattern: {}", e)))
        }
    }
}

/// Resolve the value of a value-taking long option: prefer the inline
/// `--name=value` form, else consume the next argv entry (`--name value`).
fn long_opt_value(
    inline: &Option<String>,
    name: &str,
    i: &mut usize,
    args: &[String],
) -> Result<String> {
    if let Some(v) = inline {
        Ok(v.clone())
    } else {
        *i += 1;
        args.get(*i).cloned().ok_or_else(|| {
            Error::Execution(format!("grep: option '--{}' requires an argument", name))
        })
    }
}

/// Parse a non-negative integer for a numeric long option (`--max-count`, etc.).
fn parse_long_numeric(value: &str, name: &str) -> Result<usize> {
    value
        .parse()
        .map_err(|_| Error::Execution(format!("grep: invalid --{} value: {}", name, value)))
}

/// Strip surrounding single or double quotes from a value
fn strip_quotes(s: &str) -> String {
    if let Some(inner) = s.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')) {
        inner.to_string()
    } else if let Some(inner) = s.strip_prefix('"').and_then(|s| s.strip_suffix('"')) {
        inner.to_string()
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

fn path_has_excluded_dir(
    root: &std::path::Path,
    candidate: &std::path::Path,
    exclude_dir: &[String],
) -> bool {
    if exclude_dir.is_empty() {
        return false;
    }

    let relative = candidate.strip_prefix(root).unwrap_or(candidate);
    let Some(parent) = relative.parent() else {
        return false;
    };

    parent.components().any(|component| {
        let std::path::Component::Normal(name) = component else {
            return false;
        };
        let Some(name) = name.to_str() else {
            return false;
        };
        exclude_dir
            .iter()
            .any(|pattern| glob_matches(name, pattern))
    })
}

fn process_content(content: Vec<u8>, binary_as_text: bool) -> String {
    if binary_as_text {
        let filtered: Vec<u8> = content.into_iter().filter(|&b| b != 0).collect();
        String::from_utf8_lossy(&filtered).into_owned()
    } else {
        String::from_utf8_lossy(&content).into_owned()
    }
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
        if let Some(r) = super::check_help_version(
            ctx.args,
            "Usage: grep [OPTION]... PATTERN [FILE]...\nSearch for PATTERN in each FILE.\n\n  -i, --ignore-case\t\tignore case distinctions\n  -v, --invert-match\t\tselect non-matching lines\n  -n, --line-number\t\tprint line number with output lines\n  -c, --count\t\t\tprint only a count of matching lines\n  -l, --files-with-matches\tprint only names of files with matches\n  -L, --files-without-match\tprint only names of files without matches\n  -o, --only-matching\t\tshow only the matching part of lines\n  -q, --quiet, --silent\t\tsuppress all normal output\n  -w, --word-regexp\t\tmatch whole words only\n  -x, --line-regexp\t\tmatch whole lines only\n  -m, --max-count=NUM\t\tstop after NUM matches\n  -E, --extended-regexp\t\textended regular expressions\n  -F, --fixed-strings\t\tfixed string matching\n  -G, --basic-regexp\t\tbasic regular expressions (default)\n  -P, --perl-regexp\t\tPerl-compatible regular expressions\n  -e, --regexp=PATTERN\t\tuse PATTERN for matching\n  -f, --file=FILE\t\tread patterns from FILE\n  -A, --after-context=NUM\tprint NUM lines of trailing context\n  -B, --before-context=NUM\tprint NUM lines of leading context\n  -C, --context=NUM\t\tprint NUM lines of output context\n  -H, --with-filename\t\talways print filename headers\n  -h, --no-filename\t\tsuppress filename headers\n  -b, --byte-offset\t\tprint byte offset of matches\n  -a, --text\t\t\ttreat binary files as text\n  -z, --null-data\t\tuse NUL as line separator\n  -r, -R, --recursive\t\trecursive search\n  -s, --no-messages\t\tsuppress error messages\n  -Z, --null\t\t\tprint NUL after filenames\n  --include=GLOB\t\tsearch only files matching GLOB\n  --exclude=GLOB\t\tskip files matching GLOB\n  --exclude-dir=GLOB\t\tskip directories matching GLOB\n  --color=WHEN\t\t\tcolor output (no-op)\n  --line-buffered\t\tline-buffered output (no-op)\n  --help\t\t\tdisplay this help and exit\n  --version\t\t\toutput version information and exit\n",
            Some("grep (bashkit) 0.1"),
        ) {
            return Ok(r);
        }
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

        let matcher = opts.build_matcher()?;

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
        let inputs: Vec<(String, String)> = if opts.files.is_empty() {
            // Read from stdin
            let mut stdin_content = ctx.stdin.unwrap_or("").to_string();
            if opts.binary_as_text {
                // Filter null bytes for -a flag
                stdin_content = stdin_content.replace('\0', "");
            }
            vec![(stdin_name.to_string(), stdin_content)]
        } else if opts.recursive {
            // Try indexed search via SearchCapable if available. Skip it for -P:
            // the backend's regex engine doesn't speak PCRE (lookaround /
            // backreferences), so prefiltering there could drop real matches.
            let search_result = if opts.perl_regex {
                None
            } else {
                try_indexed_search(&*ctx.fs, &opts, ctx.cwd).await
            };

            if let Some(indexed_inputs) = search_result {
                indexed_inputs
            } else {
                // Fallback: linear directory traversal
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
                                && let Ok(content) = ctx.fs.read_file(&entry_path).await
                            {
                                let text = process_content(content, opts.binary_as_text);
                                inputs.push((entry_path.to_string_lossy().into_owned(), text));
                            }
                        }
                    } else if let Ok(content) = ctx.fs.read_file(&path).await {
                        // It's a file, not a directory
                        let text = process_content(content, opts.binary_as_text);
                        inputs.push((path.to_string_lossy().into_owned(), text));
                    }
                }
                inputs
            }
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
            if let Some(max) = opts.max_count
                && total_matches >= max
            {
                break 'file_loop;
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
                if let Some(max) = opts.max_count
                    && total_matches >= max
                {
                    max_reached = true;
                    break; // Break inner loop, continue to output phase
                }

                if opts.only_matching && !opts.invert_match {
                    // -o mode: count each match separately, stopping the lazy
                    // matcher as soon as grep's early-exit conditions are met.
                    matcher.for_each_range(line, |_| {
                        file_matched = true;
                        if !opts.files_without_matches {
                            any_match = true;
                        }
                        match_count += 1;
                        total_matches += 1;

                        if opts.files_with_matches || opts.files_without_matches || opts.quiet {
                            return false;
                        }

                        if let Some(max) = opts.max_count
                            && total_matches >= max
                        {
                            max_reached = true;
                            return false;
                        }

                        true
                    });
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
                    let matches = matcher.is_match(line);
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
                        if let Some(max) = opts.max_count
                            && total_matches >= max
                        {
                            max_reached = true;
                            break;
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
                    // -o mode: output each match lazily so -m can stop before
                    // materializing every match range on dense inputs.
                    let mut o_matches = 0usize;
                    for (line_num, line) in lines.iter().enumerate() {
                        matcher.for_each_range(line, |(start, end)| {
                            if let Some(max) = opts.max_count
                                && o_matches >= max
                            {
                                return false;
                            }
                            if show_filename {
                                output.push_str(filename);
                                output.push(fname_sep);
                            }
                            if opts.byte_offset {
                                output.push_str(&format!("{}:", byte_offsets[line_num] + start));
                            }
                            if opts.line_numbers {
                                output.push_str(&format!("{}:", line_num + 1));
                            }
                            output.push_str(&line[start..end]);
                            output.push('\n');
                            o_matches += 1;
                            true
                        });
                        if let Some(max) = opts.max_count
                            && o_matches >= max
                        {
                            break;
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

                    let match_line_set: std::collections::HashSet<usize> =
                        match_lines.iter().copied().collect();

                    let mut prev_line: Option<usize> = None;
                    for line_idx in sorted_lines {
                        // Print separator if there's a gap
                        if let Some(prev) = prev_line
                            && line_idx > prev + 1
                        {
                            output.push_str("--\n");
                        }
                        prev_line = Some(line_idx);

                        // Determine if this is a match line or context line
                        let is_match = match_line_set.contains(&line_idx);
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
                        if let Some(max) = opts.max_count
                            && out_count >= max
                        {
                            break;
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

/// Try to use an indexed search provider for recursive grep.
///
/// Returns `Some(inputs)` if a `SearchCapable` provider handled the search,
/// `None` to fall back to linear scan.
async fn try_indexed_search(
    fs: &dyn crate::fs::FileSystem,
    opts: &GrepOptions,
    cwd: &std::path::Path,
) -> Option<Vec<(String, String)>> {
    if opts.invert_match
        || opts.files_without_matches
        || opts.count_only
        || opts.patterns.len() != 1
    {
        return None;
    }

    let sc = fs.as_search_capable()?;
    let mut seen_paths = std::collections::HashSet::new();
    let mut inputs = Vec::new();

    for file in &opts.files {
        let root = crate::fs::normalize_path(&if file.starts_with('/') {
            std::path::PathBuf::from(file)
        } else {
            cwd.join(file)
        });
        let provider = sc.search_provider(&root)?;
        let caps = provider.capabilities();
        if !caps.content_search {
            return None;
        }

        let pattern = if opts.fixed_strings {
            opts.patterns[0].clone()
        } else {
            if opts.perl_regex || !caps.regex {
                return None;
            }

            let pattern = if opts.extended_regex {
                opts.patterns[0].clone()
            } else {
                bre_to_ere(&opts.patterns[0])
            };
            let pattern = if opts.word_regex {
                format!(r"\b{}\b", pattern)
            } else {
                pattern
            };
            if opts.whole_line {
                format!("^(?:{})$", pattern)
            } else {
                pattern
            }
        };

        let query = crate::fs::SearchQuery {
            pattern,
            is_regex: !opts.fixed_strings,
            case_insensitive: opts.ignore_case,
            root: root.clone(),
            glob_filter: if caps.glob_filter && opts.include_patterns.len() == 1 {
                opts.include_patterns.first().cloned()
            } else {
                None
            },
            max_results: None,
        };

        let results = provider.search(&query).ok()?;

        for m in &results.matches {
            let candidate = if m.path.is_absolute() {
                crate::fs::normalize_path(&m.path)
            } else {
                crate::fs::normalize_path(&root.join(&m.path))
            };

            if !candidate.starts_with(&root) || !seen_paths.insert(candidate.clone()) {
                continue;
            }

            let Some(name) = candidate.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            if !should_include_file(name, &opts.include_patterns, &opts.exclude_patterns)
                || path_has_excluded_dir(&root, &candidate, &opts.exclude_dir_patterns)
            {
                continue;
            }

            if let Ok(content) = fs.read_file(&candidate).await {
                let text = process_content(content, opts.binary_as_text);
                inputs.push((candidate.to_string_lossy().into_owned(), text));
            }
        }
    }

    Some(inputs)
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::{
        FileSystem, FileSystemExt, InMemoryFs, OverlayFs, SearchCapabilities, SearchCapable,
        SearchMatch, SearchProvider, SearchQuery, SearchResults,
    };
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};
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
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
        };

        grep.execute(ctx).await
    }

    struct IndexedTestFs {
        inner: InMemoryFs,
        matches: Vec<SearchMatch>,
    }

    #[async_trait::async_trait]
    impl FileSystemExt for IndexedTestFs {}

    #[async_trait::async_trait]
    impl FileSystem for IndexedTestFs {
        async fn read_file(&self, path: &Path) -> Result<Vec<u8>> {
            self.inner.read_file(path).await
        }
        async fn write_file(&self, path: &Path, content: &[u8]) -> Result<()> {
            self.inner.write_file(path, content).await
        }
        async fn append_file(&self, path: &Path, content: &[u8]) -> Result<()> {
            self.inner.append_file(path, content).await
        }
        async fn mkdir(&self, path: &Path, recursive: bool) -> Result<()> {
            self.inner.mkdir(path, recursive).await
        }
        async fn remove(&self, path: &Path, recursive: bool) -> Result<()> {
            self.inner.remove(path, recursive).await
        }
        async fn stat(&self, path: &Path) -> Result<crate::fs::Metadata> {
            self.inner.stat(path).await
        }
        async fn read_dir(&self, path: &Path) -> Result<Vec<crate::fs::DirEntry>> {
            self.inner.read_dir(path).await
        }
        async fn exists(&self, path: &Path) -> Result<bool> {
            self.inner.exists(path).await
        }
        async fn rename(&self, from: &Path, to: &Path) -> Result<()> {
            self.inner.rename(from, to).await
        }
        async fn copy(&self, from: &Path, to: &Path) -> Result<()> {
            self.inner.copy(from, to).await
        }
        async fn symlink(&self, target: &Path, link: &Path) -> Result<()> {
            self.inner.symlink(target, link).await
        }
        async fn read_link(&self, path: &Path) -> Result<PathBuf> {
            self.inner.read_link(path).await
        }
        async fn chmod(&self, path: &Path, mode: u32) -> Result<()> {
            self.inner.chmod(path, mode).await
        }
        fn as_search_capable(&self) -> Option<&dyn SearchCapable> {
            Some(self)
        }
    }

    struct IndexedProvider {
        matches: Vec<SearchMatch>,
    }

    impl SearchProvider for IndexedProvider {
        fn search(&self, _query: &SearchQuery) -> Result<SearchResults> {
            Ok(SearchResults {
                matches: self.matches.clone(),
                truncated: false,
            })
        }

        fn capabilities(&self) -> SearchCapabilities {
            SearchCapabilities {
                regex: true,
                glob_filter: true,
                content_search: true,
                filename_search: false,
            }
        }
    }

    impl SearchCapable for IndexedTestFs {
        fn search_provider(&self, _path: &Path) -> Option<Box<dyn SearchProvider>> {
            Some(Box::new(IndexedProvider {
                matches: self.matches.clone(),
            }))
        }
    }

    async fn run_grep_with_indexed_fs(
        inner: InMemoryFs,
        matches: Vec<SearchMatch>,
        args: &[&str],
    ) -> Result<ExecResult> {
        let grep = Grep;
        let fs: Arc<dyn FileSystem> = Arc::new(IndexedTestFs { inner, matches });
        let mut vars = HashMap::new();
        let mut cwd = PathBuf::from("/");
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();

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
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
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
    async fn test_grep_only_matching_max_count_stops_on_first_dense_match() {
        let haystack = "x".repeat(100_000);
        let result = run_grep(&["-om1", "."], Some(&haystack)).await.unwrap();

        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "x\n");
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

    #[test]
    fn test_strip_quotes_single_quote_char_does_not_panic() {
        assert_eq!(strip_quotes("'"), "'");
        assert_eq!(strip_quotes("\""), "\"");
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
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
        };

        let result = grep.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("/dir/a.txt:hello"));
        assert!(!result.stdout.contains("b.log"));
    }

    #[tokio::test]
    async fn test_grep_recursive_single_file() {
        let grep = Grep;
        let fs = Arc::new(InMemoryFs::new());
        fs.mkdir(&PathBuf::from("/data"), true).await.unwrap();
        fs.write_file(&PathBuf::from("/data/test.md"), b"hello world\n")
            .await
            .unwrap();

        let mut vars = HashMap::new();
        let mut cwd = PathBuf::from("/");
        let args: Vec<String> = ["-r", "hello", "/data/test.md"]
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
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
        };

        let result = grep.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0, "grep -r on a single file should match");
        assert!(
            result.stdout.contains("hello world"),
            "expected 'hello world' in stdout, got: {:?}", // debug-ok: assert-failure message
            result.stdout
        );
    }

    /// Regression: grep -r on a single file with OverlayFs returned empty
    /// because OverlayFs::read_dir returned Ok(vec![]) for files instead of Err.
    #[tokio::test]
    async fn test_grep_recursive_single_file_overlay() {
        let grep = Grep;
        let base = Arc::new(InMemoryFs::new());
        let fs: Arc<dyn FileSystem> = Arc::new(OverlayFs::new(base));
        fs.mkdir(&PathBuf::from("/data"), true).await.unwrap();
        fs.write_file(&PathBuf::from("/data/test.md"), b"hello world\n")
            .await
            .unwrap();

        let mut vars = HashMap::new();
        let mut cwd = PathBuf::from("/");
        let args: Vec<String> = ["-r", "hello", "/data/test.md"]
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
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
        };

        let result = grep.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0, "grep -r on single file via OverlayFs");
        assert!(
            result.stdout.contains("hello world"),
            "expected 'hello world' in stdout, got: {:?}", // debug-ok: assert-failure message
            result.stdout
        );
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
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
        };

        let result = grep.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("/dir/a.txt:hello"));
        assert!(!result.stdout.contains("b.log"));
    }

    #[tokio::test]
    async fn test_grep_recursive_indexed_search_ignores_outside_root_match_paths() {
        let grep = Grep;
        let inner = InMemoryFs::new();
        inner.mkdir(Path::new("/safe"), true).await.unwrap();
        inner
            .write_file(Path::new("/safe/a.txt"), b"safe text\n")
            .await
            .unwrap();
        inner
            .write_file(Path::new("/leak.txt"), b"secret\n")
            .await
            .unwrap();

        let fs: Arc<dyn FileSystem> = Arc::new(IndexedTestFs {
            inner,
            matches: vec![SearchMatch {
                path: PathBuf::from("/leak.txt"),
                line_number: 1,
                line_content: "secret".to_string(),
            }],
        });

        let mut vars = HashMap::new();
        let mut cwd = PathBuf::from("/");
        let args: Vec<String> = ["-r", "secret", "/safe"]
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
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
        };

        let result = grep.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 1);
        assert_eq!(result.stdout, "");
    }

    #[tokio::test]
    async fn test_grep_recursive_indexed_search_falls_back_for_multiple_roots() {
        let grep = Grep;
        let inner = InMemoryFs::new();
        inner.mkdir(Path::new("/safe"), true).await.unwrap();
        inner.mkdir(Path::new("/other"), true).await.unwrap();
        inner
            .write_file(Path::new("/safe/clean.txt"), b"nothing\n")
            .await
            .unwrap();
        inner
            .write_file(Path::new("/other/hit.txt"), b"SECRET\n")
            .await
            .unwrap();

        let fs: Arc<dyn FileSystem> = Arc::new(IndexedTestFs {
            inner,
            matches: Vec::new(),
        });

        let mut vars = HashMap::new();
        let mut cwd = PathBuf::from("/");
        let args: Vec<String> = ["-r", "SECRET", "/safe", "/other"]
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
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
        };

        let result = grep.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("/other/hit.txt:SECRET"));
    }

    #[tokio::test]
    async fn test_grep_recursive_indexed_search_honors_exclude_dir() {
        let grep = Grep;
        let inner = InMemoryFs::new();
        inner.mkdir(Path::new("/safe/public"), true).await.unwrap();
        inner.mkdir(Path::new("/safe/secret"), true).await.unwrap();
        inner
            .write_file(Path::new("/safe/public/visible.txt"), b"public SECRET\n")
            .await
            .unwrap();
        inner
            .write_file(Path::new("/safe/secret/token.txt"), b"hidden SECRET\n")
            .await
            .unwrap();

        let fs: Arc<dyn FileSystem> = Arc::new(IndexedTestFs {
            inner,
            matches: vec![
                SearchMatch {
                    path: PathBuf::from("/safe/public/visible.txt"),
                    line_number: 1,
                    line_content: "public SECRET".to_string(),
                },
                SearchMatch {
                    path: PathBuf::from("/safe/secret/token.txt"),
                    line_number: 1,
                    line_content: "hidden SECRET".to_string(),
                },
            ],
        });

        let mut vars = HashMap::new();
        let mut cwd = PathBuf::from("/");
        let args: Vec<String> = ["-r", "--exclude-dir=secret", "SECRET", "/safe"]
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
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
        };

        let result = grep.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(
            result
                .stdout
                .contains("/safe/public/visible.txt:public SECRET")
        );
        assert!(!result.stdout.contains("token.txt"));
        assert!(!result.stdout.contains("hidden SECRET"));
    }

    // -L (--files-without-match) tests

    #[tokio::test]
    async fn test_grep_recursive_indexed_search_uses_all_roots() {
        let inner = InMemoryFs::new();
        inner.mkdir(Path::new("/a"), true).await.unwrap();
        inner.mkdir(Path::new("/b"), true).await.unwrap();
        inner
            .write_file(Path::new("/a/first.txt"), b"needle in a\n")
            .await
            .unwrap();
        inner
            .write_file(Path::new("/b/second.txt"), b"needle in b\n")
            .await
            .unwrap();

        let result = run_grep_with_indexed_fs(
            inner,
            vec![
                SearchMatch {
                    path: PathBuf::from("/a/first.txt"),
                    line_number: 1,
                    line_content: "needle in a".to_string(),
                },
                SearchMatch {
                    path: PathBuf::from("/b/second.txt"),
                    line_number: 1,
                    line_content: "needle in b".to_string(),
                },
            ],
            &["-r", "needle", "/a", "/b"],
        )
        .await
        .unwrap();

        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("/a/first.txt:needle in a"));
        assert!(result.stdout.contains("/b/second.txt:needle in b"));
    }

    #[tokio::test]
    async fn test_grep_recursive_indexed_search_respects_exclude_dir() {
        let inner = InMemoryFs::new();
        inner.mkdir(Path::new("/dir/keep"), true).await.unwrap();
        inner.mkdir(Path::new("/dir/skip"), true).await.unwrap();
        inner
            .write_file(Path::new("/dir/keep/a.txt"), b"needle keep\n")
            .await
            .unwrap();
        inner
            .write_file(Path::new("/dir/skip/a.txt"), b"needle skip\n")
            .await
            .unwrap();

        let result = run_grep_with_indexed_fs(
            inner,
            vec![
                SearchMatch {
                    path: PathBuf::from("/dir/keep/a.txt"),
                    line_number: 1,
                    line_content: "needle keep".to_string(),
                },
                SearchMatch {
                    path: PathBuf::from("/dir/skip/a.txt"),
                    line_number: 1,
                    line_content: "needle skip".to_string(),
                },
            ],
            &["-r", "--exclude-dir=skip", "needle", "/dir"],
        )
        .await
        .unwrap();

        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("/dir/keep/a.txt:needle keep"));
        assert!(!result.stdout.contains("skip"));
    }

    #[tokio::test]
    async fn test_grep_recursive_indexed_search_falls_back_for_invert_match() {
        let inner = InMemoryFs::new();
        inner.mkdir(Path::new("/dir"), true).await.unwrap();
        inner
            .write_file(Path::new("/dir/a.txt"), b"needle\nplain\n")
            .await
            .unwrap();

        let result = run_grep_with_indexed_fs(
            inner,
            vec![SearchMatch {
                path: PathBuf::from("/dir/a.txt"),
                line_number: 1,
                line_content: "needle".to_string(),
            }],
            &["-r", "-v", "needle", "/dir"],
        )
        .await
        .unwrap();

        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "/dir/a.txt:plain\n");
    }

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
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
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
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
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
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
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
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
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
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
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
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
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
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
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

    // TM-INF-022: malformed-regex stderr must not leak `regex` crate Debug.
    #[tokio::test]
    async fn no_leak_invalid_regex() {
        let r = crate::builtins::debug_leak_check::run(r"echo 1 | grep -E '['").await;
        crate::builtins::debug_leak_check::assert_no_leak(
            &r,
            "grep_invalid_regex",
            &["regex::Error", "ParseError {"],
        );
    }

    // -P (PCRE via fancy-regex) capability tests.

    #[tokio::test]
    async fn test_grep_perl_lookahead() {
        // Lookahead is unsupported by the default `regex` engine; -P must
        // route to fancy-regex. Match "foo" only when followed by "bar".
        let result = run_grep(&["-oP", r"foo(?=bar)"], Some("foobar\nfoobaz\nfoo"))
            .await
            .unwrap();
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "foo\n");
    }

    #[tokio::test]
    async fn test_grep_perl_backreference() {
        // Backreferences are rejected by `regex`; fancy-regex accepts them.
        let result = run_grep(&["-P", r"(\w+) \1"], Some("hello hello\nhello world"))
            .await
            .unwrap();
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "hello hello\n");
    }

    #[tokio::test]
    async fn test_grep_perl_lookbehind() {
        let result = run_grep(&["-oP", r"(?<=\$)\d+"], Some("price $42 and 99"))
            .await
            .unwrap();
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "42\n");
    }

    #[tokio::test]
    async fn test_grep_perl_long_flag() {
        let result = run_grep(&["--perl-regexp", r"\d{3}"], Some("ab12\nxy345"))
            .await
            .unwrap();
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "xy345\n");
    }

    #[tokio::test]
    async fn test_grep_pattern_type_extended_then_perl_last_wins() {
        // -E then -P: perl wins, so the backreference compiles and matches.
        let result = run_grep(&["-E", "-P", r"(.)\1"], Some("aa\nab"))
            .await
            .unwrap();
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "aa\n");
    }

    #[tokio::test]
    async fn test_grep_pattern_type_perl_then_extended_last_wins() {
        // -P then -E: extended wins, so -E cleared perl_regex and the
        // backreference is rejected by the linear-time engine (error).
        let result = run_grep(&["-P", "-E", r"(.)\1"], Some("aa")).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_grep_pattern_type_long_options_last_wins() {
        // --perl-regexp then --extended-regexp: extended wins -> backref rejected.
        let result = run_grep(
            &["--perl-regexp", "--extended-regexp", r"(.)\1"],
            Some("aa"),
        )
        .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_grep_pattern_type_perl_then_fixed_last_wins() {
        // -P then -F: fixed wins, so the pattern is matched literally.
        let result = run_grep(&["-P", "-F", r"a.b"], Some("a.b\naxb"))
            .await
            .unwrap();
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "a.b\n");
    }

    #[tokio::test]
    async fn test_grep_perl_invalid_pattern_errors() {
        // Unbalanced group: fancy-regex rejects it; we surface an error, not a
        // panic, and must not leak the engine's Debug shape.
        let result = run_grep(&["-P", "(foo"], Some("foo")).await;
        assert!(result.is_err());
    }

    // GNU long-option alias capability tests.

    #[tokio::test]
    async fn test_grep_long_ignore_case() {
        let result = run_grep(&["--ignore-case", "HELLO"], Some("Hello World\ngoodbye"))
            .await
            .unwrap();
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "Hello World\n");
    }

    #[tokio::test]
    async fn test_grep_long_invert_and_line_number() {
        let result = run_grep(
            &["--invert-match", "--line-number", "hello"],
            Some("hello\nworld\nhello again"),
        )
        .await
        .unwrap();
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "2:world\n");
    }

    #[tokio::test]
    async fn test_grep_long_max_count_inline() {
        let result = run_grep(&["--max-count=2", "o"], Some("foo\nbar\nboo\nzoo"))
            .await
            .unwrap();
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "foo\nboo\n");
    }

    #[tokio::test]
    async fn test_grep_long_max_count_separate_arg() {
        // Space-separated value form: `--max-count 1`.
        let result = run_grep(&["--max-count", "1", "o"], Some("foo\nboo\nzoo"))
            .await
            .unwrap();
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "foo\n");
    }

    #[tokio::test]
    async fn test_grep_long_regexp_multiple() {
        let result = run_grep(&["--regexp=foo", "--regexp", "baz"], Some("foo\nbar\nbaz"))
            .await
            .unwrap();
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "foo\nbaz\n");
    }

    #[tokio::test]
    async fn test_grep_long_fixed_strings() {
        let result = run_grep(&["--fixed-strings", "a.b"], Some("a.b\naxb"))
            .await
            .unwrap();
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "a.b\n");
    }

    #[tokio::test]
    async fn test_grep_long_word_regexp() {
        let result = run_grep(&["--word-regexp", "foo"], Some("foo\nfoobar\nbar foo"))
            .await
            .unwrap();
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "foo\nbar foo\n");
    }

    #[tokio::test]
    async fn test_grep_long_missing_value_errors() {
        let result = run_grep(&["--max-count"], Some("foo")).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_grep_only_matching_byte_offset() {
        // -b with -o reports the byte offset of the match, not the line start.
        let result = run_grep(&["-ob", "bar"], Some("foobar\n")).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "3:bar\n");
    }

    #[tokio::test]
    async fn test_grep_perl_catastrophic_backtrack_is_bounded() {
        // TM-DOS-025: a classic catastrophic-backtracking pattern against a
        // long non-matching line must terminate (backtrack-limit -> "no match")
        // rather than hang the sandbox.
        let haystack = format!("{}!", "a".repeat(40));
        let result = run_grep(&["-P", r"(a+)+$"], Some(&haystack)).await.unwrap();
        assert_eq!(result.exit_code, 1);
        assert_eq!(result.stdout, "");
    }
}
