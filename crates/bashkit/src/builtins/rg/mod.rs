//! rg - Simplified ripgrep builtin
//!
//! Recursive file search by default, similar to grep but with rg-style defaults.
//!
//! Usage:
//!   rg PATTERN [PATH...]
//!   rg -i PATTERN file          # case insensitive
//!   rg -n PATTERN file          # show line numbers (off by default in non-tty)
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
use std::collections::HashSet;
use std::path::{Path, PathBuf};

use super::search_common::{build_regex_opts, build_search_regex};
use super::{Builtin, Context, read_text_file, resolve_path};
use crate::error::{Error, Result};
use crate::interpreter::ExecResult;

/// rg command - recursive pattern search (simplified ripgrep)
pub struct Rg;

struct RgOptions {
    patterns: Vec<String>,
    paths: Vec<String>,
    ignore_case: bool,
    line_numbers: bool,
    count_only: bool,
    files_with_matches: bool,
    invert_match: bool,
    word_boundary: bool,
    fixed_strings: bool,
    max_count: Option<usize>,
    before_context: usize,
    after_context: usize,
    no_filename: bool,
    show_filename: bool,
    only_matching: bool,
    quiet: bool,
    files_without_matches: bool,
    glob_rules: Vec<RgGlobRule>,
}

#[derive(Clone)]
struct RgGlobRule {
    raw: String,
    include: bool,
    has_slash: bool,
    anchored: bool,
    regex: Regex,
}

impl RgOptions {
    fn parse(args: &[String]) -> Result<Self> {
        let mut opts = RgOptions {
            patterns: Vec::new(),
            paths: Vec::new(),
            ignore_case: false,
            line_numbers: false, // non-tty: suppress line numbers (real rg behavior)
            count_only: false,
            files_with_matches: false,
            invert_match: false,
            word_boundary: false,
            fixed_strings: false,
            max_count: None,
            before_context: 0,
            after_context: 0,
            no_filename: false,
            show_filename: false,
            only_matching: false,
            quiet: false,
            files_without_matches: false,
            glob_rules: Vec::new(),
        };

        let mut positional = Vec::new();
        let mut p = super::arg_parser::ArgParser::new(args);

        while !p.is_done() {
            if let Some(val) = p
                .flag_value_any(&["-e", "--regexp"], "rg")
                .map_err(Error::Execution)?
            {
                opts.patterns.push(val.to_string());
            } else if let Some(val) = long_value(&mut p, "--regexp")? {
                opts.patterns.push(val.to_string());
            } else if let Some(val) = p.flag_value("-m", "rg").map_err(Error::Execution)? {
                opts.max_count = Some(
                    val.parse()
                        .map_err(|_| Error::Execution(format!("rg: invalid -m value: {val}")))?,
                );
            } else if let Some(val) = long_value(&mut p, "--max-count")? {
                opts.max_count = Some(val.parse().map_err(|_| {
                    Error::Execution(format!("rg: invalid --max-count value: {val}"))
                })?);
            } else if let Some(val) = p.flag_value("-A", "rg").map_err(Error::Execution)? {
                opts.after_context = parse_context_value(val, "-A")?;
            } else if let Some(val) = p.flag_value("-B", "rg").map_err(Error::Execution)? {
                opts.before_context = parse_context_value(val, "-B")?;
            } else if let Some(val) = p.flag_value("-C", "rg").map_err(Error::Execution)? {
                let context = parse_context_value(val, "-C")?;
                opts.before_context = context;
                opts.after_context = context;
            } else if let Some(val) = long_value(&mut p, "--after-context")? {
                opts.after_context = parse_context_value(&val, "--after-context")?;
            } else if let Some(val) = long_value(&mut p, "--before-context")? {
                opts.before_context = parse_context_value(&val, "--before-context")?;
            } else if let Some(val) = long_value(&mut p, "--context")? {
                let context = parse_context_value(&val, "--context")?;
                opts.before_context = context;
                opts.after_context = context;
            } else if let Some(val) = p.flag_value("-g", "rg").map_err(Error::Execution)? {
                opts.glob_rules.push(RgGlobRule::parse(val)?);
            } else if let Some(val) = long_value(&mut p, "--glob")? {
                opts.glob_rules.push(RgGlobRule::parse(&val)?);
            } else if p.flag_any(&["-I", "--no-filename"]) {
                opts.no_filename = true;
            } else if p.flag("--with-filename") {
                opts.show_filename = true;
            } else if p.flag("--no-line-number") {
                opts.line_numbers = false;
            } else if p.flag("--line-number") {
                opts.line_numbers = true;
            } else if p.flag_any(&["--ignore-case"]) {
                opts.ignore_case = true;
            } else if p.flag_any(&["--case-sensitive"]) {
                opts.ignore_case = false;
            } else if p.flag_any(&["--count"]) {
                opts.count_only = true;
            } else if p.flag_any(&["--files-with-matches"]) {
                opts.files_with_matches = true;
            } else if p.flag_any(&["--files-without-match"]) {
                opts.files_without_matches = true;
            } else if p.flag_any(&["--invert-match"]) {
                opts.invert_match = true;
            } else if p.flag_any(&["--word-regexp"]) {
                opts.word_boundary = true;
            } else if p.flag_any(&["--fixed-strings"]) {
                opts.fixed_strings = true;
            } else if p.flag_any(&["--only-matching"]) {
                opts.only_matching = true;
            } else if p.flag_any(&["--quiet", "--silent"]) {
                opts.quiet = true;
            } else if p.flag("--color") {
                // no-op (may have separate value arg like "never", skip it)
                let _ = p.positional();
            } else if p.current().is_some_and(|s| s.starts_with("--color=")) {
                // --color=VALUE is a no-op
                p.advance();
            } else if p.flag_any(&[
                "--hidden",
                "--no-hidden",
                "--no-ignore",
                "--no-ignore-vcs",
                "--no-ignore-parent",
                "--follow",
            ]) {
                // no-op: bashkit's VFS search has no ignore-file or symlink policy layer.
            } else if p.is_flag() {
                // Combined short flags like -inFw
                // Safe: is_flag() guarantees current() is Some
                let arg = p.current().expect("is_flag guarantees Some");
                if arg.starts_with("--") {
                    // Unknown long option, skip
                    p.advance();
                    continue;
                }
                let chars: Vec<char> = arg[1..].chars().collect();
                p.advance();
                for (j, &c) in chars.iter().enumerate() {
                    match c {
                        'i' => opts.ignore_case = true,
                        'n' => opts.line_numbers = true,
                        'N' => opts.line_numbers = false,
                        'c' => opts.count_only = true,
                        'l' => opts.files_with_matches = true,
                        'v' => opts.invert_match = true,
                        'w' => opts.word_boundary = true,
                        'F' => opts.fixed_strings = true,
                        'H' => opts.show_filename = true,
                        'I' => opts.no_filename = true,
                        'o' => opts.only_matching = true,
                        'q' => opts.quiet = true,
                        'e' => {
                            let rest: String = chars[j + 1..].iter().collect();
                            let pattern = if !rest.is_empty() {
                                rest
                            } else {
                                match p.positional() {
                                    Some(v) => v.to_string(),
                                    None => {
                                        return Err(Error::Execution(
                                            "rg: -e requires an argument".to_string(),
                                        ));
                                    }
                                }
                            };
                            opts.patterns.push(pattern);
                            break;
                        }
                        'm' => {
                            // Rest of this flag group or next arg is the value
                            let rest: String = chars[j + 1..].iter().collect();
                            let num_str = if !rest.is_empty() {
                                rest
                            } else {
                                match p.positional() {
                                    Some(v) => v.to_string(),
                                    None => {
                                        return Err(Error::Execution(
                                            "rg: -m requires an argument".to_string(),
                                        ));
                                    }
                                }
                            };
                            opts.max_count = Some(num_str.parse().map_err(|_| {
                                Error::Execution(format!("rg: invalid -m value: {num_str}"))
                            })?);
                            break;
                        }
                        'A' => {
                            let rest: String = chars[j + 1..].iter().collect();
                            opts.after_context = if !rest.is_empty() {
                                parse_context_value(&rest, "-A")?
                            } else {
                                match p.positional() {
                                    Some(v) => parse_context_value(v, "-A")?,
                                    None => {
                                        return Err(Error::Execution(
                                            "rg: -A requires an argument".to_string(),
                                        ));
                                    }
                                }
                            };
                            break;
                        }
                        'B' => {
                            let rest: String = chars[j + 1..].iter().collect();
                            opts.before_context = if !rest.is_empty() {
                                parse_context_value(&rest, "-B")?
                            } else {
                                match p.positional() {
                                    Some(v) => parse_context_value(v, "-B")?,
                                    None => {
                                        return Err(Error::Execution(
                                            "rg: -B requires an argument".to_string(),
                                        ));
                                    }
                                }
                            };
                            break;
                        }
                        'C' => {
                            let rest: String = chars[j + 1..].iter().collect();
                            let context = if !rest.is_empty() {
                                parse_context_value(&rest, "-C")?
                            } else {
                                match p.positional() {
                                    Some(v) => parse_context_value(v, "-C")?,
                                    None => {
                                        return Err(Error::Execution(
                                            "rg: -C requires an argument".to_string(),
                                        ));
                                    }
                                }
                            };
                            opts.before_context = context;
                            opts.after_context = context;
                            break;
                        }
                        'g' => {
                            let rest: String = chars[j + 1..].iter().collect();
                            let glob = if !rest.is_empty() {
                                rest
                            } else {
                                match p.positional() {
                                    Some(v) => v.to_string(),
                                    None => {
                                        return Err(Error::Execution(
                                            "rg: -g requires an argument".to_string(),
                                        ));
                                    }
                                }
                            };
                            opts.glob_rules.push(RgGlobRule::parse(&glob)?);
                            break;
                        }
                        _ => {} // ignore unknown
                    }
                }
            } else if let Some(arg) = p.positional() {
                positional.push(arg.to_string());
            }
        }

        if positional.is_empty() {
            if opts.patterns.is_empty() {
                return Err(Error::Execution("rg: missing pattern".to_string()));
            }
        } else if opts.patterns.is_empty() {
            opts.patterns.push(positional.remove(0));
        }

        opts.paths = positional;

        Ok(opts)
    }

    fn build_regex(&self) -> Result<Regex> {
        if self.patterns.len() == 1 {
            return build_search_regex(
                &self.patterns[0],
                self.fixed_strings,
                self.word_boundary,
                self.ignore_case,
                "rg",
            );
        }

        let combined = self
            .patterns
            .iter()
            .map(|pattern| {
                let pat = if self.fixed_strings {
                    regex::escape(pattern)
                } else {
                    pattern.clone()
                };
                let pat = if self.word_boundary {
                    format!(r"\b{}\b", pat)
                } else {
                    pat
                };
                format!("(?:{})", pat)
            })
            .collect::<Vec<_>>()
            .join("|");
        build_regex_opts(&combined, self.ignore_case)
            .map_err(|e| Error::Execution(format!("rg: invalid pattern: {}", e)))
    }

    fn matches_globs(&self, path: &Path, cwd: &Path) -> bool {
        let includes: Vec<&RgGlobRule> = self.glob_rules.iter().filter(|g| g.include).collect();
        if !includes.is_empty() && !includes.iter().any(|g| g.matches(path, cwd)) {
            return false;
        }
        !self
            .glob_rules
            .iter()
            .filter(|g| !g.include)
            .any(|g| g.matches(path, cwd))
    }

    fn first_positive_glob(&self) -> Option<String> {
        self.glob_rules
            .iter()
            .find(|g| g.include)
            .map(|g| g.raw.clone())
    }
}

impl RgGlobRule {
    fn parse(pattern: &str) -> Result<Self> {
        let (include, raw_pattern) = match pattern.strip_prefix('!') {
            Some(rest) => (false, rest),
            None => (true, pattern),
        };
        let normalized = raw_pattern.strip_prefix("./").unwrap_or(raw_pattern);
        let has_slash = normalized.contains('/');
        let anchored = normalized.starts_with('/');
        let regex = build_regex_opts(&glob_to_regex(normalized), false)
            .map_err(|e| Error::Execution(format!("rg: invalid --glob value: {}", e)))?;
        Ok(Self {
            raw: normalized.to_string(),
            include,
            has_slash,
            anchored,
            regex,
        })
    }

    fn matches(&self, path: &Path, cwd: &Path) -> bool {
        let target = if self.has_slash {
            if self.anchored {
                path_to_slash(path)
            } else {
                relative_path_to_slash(path, cwd)
            }
        } else {
            path.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_string()
        };
        self.regex.is_match(&target)
    }
}

fn path_to_slash(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn relative_path_to_slash(path: &Path, cwd: &Path) -> String {
    let relative = path.strip_prefix(cwd).unwrap_or(path);
    path_to_slash(relative).trim_start_matches('/').to_string()
}

fn display_path_for(path: &Path, cwd: &Path, root_arg: Option<&str>) -> String {
    match root_arg {
        Some(arg) if arg.starts_with('/') => path_to_slash(path),
        Some(".") | Some("./") => {
            let relative = relative_path_to_slash(path, cwd);
            if relative.is_empty() {
                ".".to_string()
            } else {
                format!("./{}", relative)
            }
        }
        Some(arg) if arg.starts_with("./") => {
            let relative = relative_path_to_slash(path, cwd);
            if relative.is_empty() {
                arg.trim_end_matches('/').to_string()
            } else {
                format!("./{}", relative)
            }
        }
        _ => relative_path_to_slash(path, cwd),
    }
}

fn glob_to_regex(pattern: &str) -> String {
    let mut out = String::new();
    out.push('^');

    let chars: Vec<char> = pattern.trim_start_matches('/').chars().collect();
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '*' if i + 1 < chars.len() && chars[i + 1] == '*' => {
                out.push_str(".*");
                i += 2;
            }
            '*' => {
                out.push_str("[^/]*");
                i += 1;
            }
            '?' => {
                out.push_str("[^/]");
                i += 1;
            }
            c => {
                out.push_str(&regex::escape(&c.to_string()));
                i += 1;
            }
        }
    }
    out.push('$');
    out
}

fn parse_context_value(value: &str, flag: &str) -> Result<usize> {
    value
        .parse()
        .map_err(|_| Error::Execution(format!("rg: invalid {} value: {}", flag, value)))
}

fn long_value(p: &mut super::arg_parser::ArgParser<'_>, name: &str) -> Result<Option<String>> {
    let Some(current) = p.current() else {
        return Ok(None);
    };
    if current == name {
        p.advance();
        let Some(value) = p.positional() else {
            return Err(Error::Execution(format!(
                "rg: {} requires an argument",
                name
            )));
        };
        Ok(Some(value.to_string()))
    } else if let Some(value) = current.strip_prefix(&format!("{name}=")) {
        p.advance();
        Ok(Some(value.to_string()))
    } else {
        Ok(None)
    }
}

async fn collect_rg_inputs(
    ctx: Context<'_>,
    opts: &RgOptions,
) -> std::result::Result<Vec<(String, String)>, ExecResult> {
    if opts.paths.is_empty() {
        if let Some(stdin) = ctx.stdin {
            return Ok(vec![("(stdin)".to_string(), stdin.to_string())]);
        }

        if let Some(inputs) = try_indexed_search(&*ctx.fs, opts, ctx.cwd).await {
            return Ok(inputs);
        }

        let files =
            collect_rg_files_recursive(&*ctx.fs, std::slice::from_ref(ctx.cwd), opts, ctx.cwd)
                .await;
        return Ok(read_rg_files(&*ctx.fs, files, ctx.cwd, None).await);
    }

    if let Some(inputs) = try_indexed_search(&*ctx.fs, opts, ctx.cwd).await {
        return Ok(inputs);
    }

    let mut inputs = Vec::new();
    for p in &opts.paths {
        let path = resolve_path(ctx.cwd, p);
        if let Ok(meta) = ctx.fs.stat(&path).await
            && meta.file_type.is_dir()
        {
            let files =
                collect_rg_files_recursive(&*ctx.fs, std::slice::from_ref(&path), opts, ctx.cwd)
                    .await;
            inputs.extend(read_rg_files(&*ctx.fs, files, ctx.cwd, Some(p)).await);
            continue;
        }

        if !opts.matches_globs(&path, ctx.cwd) {
            continue;
        }
        let text = match read_text_file(&*ctx.fs, &path, "rg").await {
            Ok(t) => t,
            Err(e) => return Err(e),
        };
        inputs.push((p.clone(), text));
    }
    Ok(inputs)
}

async fn has_directory_path(fs: &dyn crate::fs::FileSystem, cwd: &Path, paths: &[String]) -> bool {
    for p in paths {
        let path = resolve_path(cwd, p);
        if let Ok(meta) = fs.stat(&path).await
            && meta.file_type.is_dir()
        {
            return true;
        }
    }
    false
}

async fn collect_rg_files_recursive(
    fs: &dyn crate::fs::FileSystem,
    roots: &[PathBuf],
    opts: &RgOptions,
    cwd: &Path,
) -> Vec<PathBuf> {
    let mut result = Vec::new();
    let mut stack: Vec<PathBuf> = roots.to_vec();

    while let Some(current) = stack.pop() {
        if let Ok(entries) = fs.read_dir(&current).await {
            for entry in entries {
                let path = current.join(&entry.name);
                if entry.metadata.file_type.is_dir() {
                    stack.push(path);
                } else if entry.metadata.file_type.is_file() && opts.matches_globs(&path, cwd) {
                    result.push(path);
                }
            }
        } else if let Ok(meta) = fs.stat(&current).await
            && meta.file_type.is_file()
            && opts.matches_globs(&current, cwd)
        {
            result.push(current);
        }
    }

    result.sort();
    result
}

async fn read_rg_files(
    fs: &dyn crate::fs::FileSystem,
    files: Vec<PathBuf>,
    cwd: &Path,
    root_arg: Option<&str>,
) -> Vec<(String, String)> {
    let mut inputs = Vec::new();
    for path in files {
        if let Ok(content) = fs.read_file(&path).await {
            inputs.push((
                display_path_for(&path, cwd, root_arg),
                String::from_utf8_lossy(&content).into_owned(),
            ));
        }
    }
    inputs
}

async fn try_indexed_search(
    fs: &dyn crate::fs::FileSystem,
    opts: &RgOptions,
    cwd: &Path,
) -> Option<Vec<(String, String)>> {
    if opts.invert_match || opts.files_without_matches || opts.patterns.len() != 1 {
        return None;
    }

    let sc = fs.as_search_capable()?;
    let roots: Vec<(PathBuf, Option<String>)> = if opts.paths.is_empty() {
        vec![(cwd.to_path_buf(), None)]
    } else {
        opts.paths
            .iter()
            .map(|p| {
                let root = if p.starts_with('/') {
                    PathBuf::from(p)
                } else {
                    cwd.join(p)
                };
                (root, Some(p.clone()))
            })
            .collect()
    };

    let mut inputs = Vec::new();
    let mut seen_paths = HashSet::new();
    for (root, root_arg) in roots {
        let root = crate::fs::normalize_path(&root);
        let provider = sc.search_provider(&root)?;
        let caps = provider.capabilities();
        if !caps.content_search || (!opts.fixed_strings && !caps.regex) {
            return None;
        }
        let pattern = if opts.fixed_strings {
            opts.patterns[0].clone()
        } else {
            if opts.word_boundary {
                format!(r"\b{}\b", opts.patterns[0])
            } else {
                opts.patterns[0].clone()
            }
        };
        let query = crate::fs::SearchQuery {
            pattern,
            is_regex: !opts.fixed_strings,
            case_insensitive: opts.ignore_case,
            root: root.clone(),
            glob_filter: if caps.glob_filter {
                opts.first_positive_glob()
            } else {
                None
            },
            max_results: opts.max_count,
        };

        let results = provider.search(&query).ok()?;
        for m in &results.matches {
            let candidate = if m.path.is_absolute() {
                crate::fs::normalize_path(&m.path)
            } else {
                crate::fs::normalize_path(&root.join(&m.path))
            };

            if !candidate.starts_with(&root)
                || !seen_paths.insert(candidate.clone())
                || !opts.matches_globs(&candidate, cwd)
            {
                continue;
            }
            if let Ok(content) = fs.read_file(&candidate).await {
                inputs.push((
                    display_path_for(&candidate, cwd, root_arg.as_deref()),
                    String::from_utf8_lossy(&content).into_owned(),
                ));
            }
        }
    }

    Some(inputs)
}

fn write_rg_prefix(
    output: &mut String,
    filename: &str,
    show_filename: bool,
    line_numbers: bool,
    line_idx: usize,
    separator: char,
) {
    if show_filename {
        output.push_str(filename);
        output.push(separator);
    }
    if line_numbers {
        output.push_str(&(line_idx + 1).to_string());
        output.push(separator);
    }
}

fn write_rg_context(
    output: &mut String,
    filename: &str,
    lines: &[&str],
    match_lines: &[usize],
    opts: &RgOptions,
    show_filename: bool,
) {
    let mut printed = HashSet::new();
    for &match_idx in match_lines {
        let start = match_idx.saturating_sub(opts.before_context);
        let end = (match_idx + opts.after_context + 1).min(lines.len());
        for idx in start..end {
            printed.insert(idx);
        }
    }

    let mut sorted: Vec<usize> = printed.into_iter().collect();
    sorted.sort_unstable();
    let match_set: HashSet<usize> = match_lines.iter().copied().collect();
    let mut prev_line = None;

    for line_idx in sorted {
        if let Some(prev) = prev_line
            && line_idx > prev + 1
        {
            output.push_str("--\n");
        }
        prev_line = Some(line_idx);

        let separator = if match_set.contains(&line_idx) {
            ':'
        } else {
            '-'
        };
        write_rg_prefix(
            output,
            filename,
            show_filename,
            opts.line_numbers,
            line_idx,
            separator,
        );
        output.push_str(lines[line_idx]);
        output.push('\n');
    }
}

#[async_trait]
impl Builtin for Rg {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let help_text = "Usage: rg [OPTIONS] PATTERN [PATH...]\nRecursively search for a pattern.\n\n  -i, --ignore-case\tcase insensitive\n  -n, --line-number\tshow line numbers\n  -N, --no-line-number\tsuppress line numbers\n  -c, --count\tcount matches\n  -l, --files-with-matches\tfiles with matches\n  --files-without-match\tfiles without matches\n  -v, --invert-match\tinvert match\n  -w, --word-regexp\tword boundary\n  -F, --fixed-strings\tfixed strings (literal)\n  -o, --only-matching\tshow only matching text\n  -q, --quiet\tsuppress output; exit status only\n  -e, --regexp PATTERN\tuse PATTERN for matching\n  -m, --max-count NUM\tmax count per file\n  -A, --after-context NUM\tshow trailing context\n  -B, --before-context NUM\tshow leading context\n  -C, --context NUM\tshow leading and trailing context\n  -g, --glob GLOB\tinclude/exclude paths by glob (!GLOB excludes)\n  -H, --with-filename\tshow filename\n  -I, --no-filename\tsuppress filename\n  --color MODE\tcolor output (no-op)\n  -h, --help\tdisplay this help and exit\n  --version\toutput version information and exit\n";
        if ctx.args.iter().any(|arg| arg == "-h") {
            return Ok(ExecResult::ok(help_text.to_string()));
        }
        if let Some(r) = super::check_help_version(ctx.args, help_text, Some("rg (bashkit) 0.1")) {
            return Ok(r);
        }
        let opts = RgOptions::parse(ctx.args)?;
        let regex = opts.build_regex()?;
        let stdin_input = opts.paths.is_empty() && ctx.stdin.is_some();
        let recursive_output = !stdin_input
            && (opts.paths.is_empty() || has_directory_path(&*ctx.fs, ctx.cwd, &opts.paths).await);

        let inputs = match collect_rg_inputs(ctx, &opts).await {
            Ok(inputs) => inputs,
            Err(result) => return Ok(result),
        };

        let show_filename = if opts.no_filename {
            false
        } else if opts.show_filename {
            true
        } else {
            recursive_output || inputs.len() > 1
        };
        let has_context = opts.before_context > 0 || opts.after_context > 0;

        let mut output = String::new();
        let mut any_match = false;

        for (filename, content) in &inputs {
            let mut match_count = 0usize;
            let mut match_lines = Vec::new();

            for (line_idx, line) in content.lines().enumerate() {
                let matched = regex.is_match(line);
                let matched = if opts.invert_match { !matched } else { matched };

                if !matched {
                    continue;
                }

                if let Some(max) = opts.max_count
                    && match_count >= max
                {
                    break;
                }

                match_count += 1;
                match_lines.push(line_idx);
                if !opts.files_without_matches {
                    any_match = true;
                }

                if opts.files_with_matches || opts.files_without_matches || opts.quiet {
                    break;
                }
            }

            if opts.quiet && match_count > 0 {
                return Ok(ExecResult::ok(String::new()));
            }
            if opts.files_with_matches && match_count > 0 {
                output.push_str(filename);
                output.push('\n');
                continue;
            }
            if opts.files_without_matches {
                if match_count == 0 {
                    any_match = true;
                    output.push_str(filename);
                    output.push('\n');
                }
                continue;
            }
            if opts.count_only {
                if match_count == 0 {
                    continue;
                }
                if show_filename {
                    output.push_str(filename);
                    output.push(':');
                }
                output.push_str(&match_count.to_string());
                output.push('\n');
                continue;
            }
            if opts.quiet {
                continue;
            }

            let lines: Vec<&str> = content.lines().collect();
            if opts.only_matching && !opts.invert_match {
                for &line_idx in &match_lines {
                    for mat in regex.find_iter(lines[line_idx]) {
                        write_rg_prefix(
                            &mut output,
                            filename,
                            show_filename,
                            opts.line_numbers,
                            line_idx,
                            ':',
                        );
                        output.push_str(mat.as_str());
                        output.push('\n');
                    }
                }
            } else if has_context {
                if !output.is_empty() && !match_lines.is_empty() {
                    output.push_str("--\n");
                }
                write_rg_context(
                    &mut output,
                    filename,
                    &lines,
                    &match_lines,
                    &opts,
                    show_filename,
                );
            } else {
                for &line_idx in &match_lines {
                    write_rg_prefix(
                        &mut output,
                        filename,
                        show_filename,
                        opts.line_numbers,
                        line_idx,
                        ':',
                    );
                    output.push_str(lines[line_idx]);
                    output.push('\n');
                }
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
    use crate::fs::{
        FileSystem, FileSystemExt, InMemoryFs, SearchCapabilities, SearchCapable, SearchMatch,
        SearchProvider, SearchQuery, SearchResults,
    };
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    async fn run_rg(args: &[&str], stdin: Option<&str>, files: &[(&str, &[u8])]) -> ExecResult {
        run_rg_with_cwd(args, stdin, files, "/").await
    }

    async fn run_rg_with_cwd(
        args: &[&str],
        stdin: Option<&str>,
        files: &[(&str, &[u8])],
        cwd: &str,
    ) -> ExecResult {
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

        run_rg_with_fs_and_cwd(args, stdin, fs, cwd).await
    }

    async fn run_rg_with_fs<F>(args: &[&str], stdin: Option<&str>, fs: Arc<F>) -> ExecResult
    where
        F: FileSystem + 'static,
    {
        run_rg_with_fs_and_cwd(args, stdin, fs, "/").await
    }

    async fn run_rg_with_fs_and_cwd<F>(
        args: &[&str],
        stdin: Option<&str>,
        fs: Arc<F>,
        cwd: &str,
    ) -> ExecResult
    where
        F: FileSystem + 'static,
    {
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let env = HashMap::new();
        let mut variables = HashMap::new();
        let mut cwd = PathBuf::from(cwd);
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
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
        };

        Rg.execute(ctx).await.unwrap()
    }

    #[derive(Clone, Copy)]
    enum RgDiffOutput {
        Exact,
        UnorderedLines,
    }

    struct RgDiffCase {
        name: &'static str,
        args: &'static [&'static str],
        stdin: Option<&'static str>,
        files: &'static [(&'static str, &'static [u8])],
        cwd: &'static str,
        output: RgDiffOutput,
    }

    const DIFF_BASIC_FILES: &[(&str, &[u8])] = &[
        ("/proj/a.txt", b"needle\nnone\nneedle again\n"),
        ("/proj/b.txt", b"none\n"),
        ("/proj/src/main.rs", b"needle\n"),
        ("/proj/src/main.txt", b"needle\n"),
        ("/proj/vendor/lib.rs", b"needle\n"),
        ("/proj/case.txt", b"Hello\nhello\n"),
        ("/proj/fixed.txt", b"a.b\naxb\n"),
        ("/proj/words.txt", b"cat\ncatch\nmy cat\n"),
        ("/proj/context.txt", b"before\nneedle\nafter\n"),
    ];

    const DIFF_TWO_CONTEXT_FILES: &[(&str, &[u8])] = &[
        ("/proj/a.txt", b"before\nneedle\nafter\n"),
        ("/proj/b.txt", b"x\nneedle\ny\n"),
    ];

    const RG_DIFF_CASES: &[RgDiffCase] = &[
        RgDiffCase {
            name: "stdin line numbers",
            args: &["-n", "needle"],
            stdin: Some("x\nneedle\n"),
            files: &[],
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "no path recursive cwd display",
            args: &["needle"],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/proj",
            output: RgDiffOutput::UnorderedLines,
        },
        RgDiffCase {
            name: "relative recursive display",
            args: &["needle", "proj"],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::UnorderedLines,
        },
        RgDiffCase {
            name: "absolute path display",
            args: &["-H", "needle", "/proj/a.txt"],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "dot root glob excludes cwd-relative path",
            args: &["-g", "*.rs", "-g", "!vendor/**", "needle", "."],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/proj",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "relative root glob keeps nested vendor",
            args: &["-g", "*.rs", "-g", "!vendor/**", "needle", "proj"],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::UnorderedLines,
        },
        RgDiffCase {
            name: "long glob equals include",
            args: &["--glob=*.rs", "needle", "proj"],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::UnorderedLines,
        },
        RgDiffCase {
            name: "context combined",
            args: &["-nC1", "needle", "proj/context.txt"],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "context across explicit files",
            args: &["-n", "-A1", "-B1", "needle", "proj/a.txt", "proj/b.txt"],
            stdin: None,
            files: DIFF_TWO_CONTEXT_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "count suppresses zero-match files",
            args: &["-c", "needle", "proj"],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::UnorderedLines,
        },
        RgDiffCase {
            name: "only matching",
            args: &["-o", "needle", "proj/a.txt"],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "quiet",
            args: &["-q", "needle", "proj"],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "multiple regexp explicit files",
            args: &["-e", "needle", "-e", "Hello", "proj/a.txt", "proj/case.txt"],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "long regexp equals",
            args: &["--regexp=needle", "proj/a.txt"],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "case insensitive",
            args: &["-i", "hello", "proj/case.txt"],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "fixed strings",
            args: &["-F", "a.b", "proj/fixed.txt"],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "word regexp",
            args: &["-w", "cat", "proj/words.txt"],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "invert match",
            args: &["-v", "needle", "proj/a.txt"],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "max count",
            args: &["-m", "1", "needle", "proj/a.txt"],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "files with matches",
            args: &["-l", "needle", "proj/a.txt", "proj/b.txt"],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "files without match",
            args: &[
                "--files-without-match",
                "needle",
                "proj/a.txt",
                "proj/b.txt",
            ],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "force no filename",
            args: &["-I", "needle", "proj/a.txt", "proj/src/main.rs"],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "combined line number off",
            args: &["-nN", "needle", "proj/a.txt"],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
    ];

    fn require_real_rg() {
        let output = std::process::Command::new("rg")
            .arg("--version")
            .output()
            .expect("real rg binary must be installed for rg differential tests");
        assert!(
            output.status.success(),
            "real rg binary must run successfully for rg differential tests"
        );
    }

    fn run_real_rg(case: &RgDiffCase) -> (String, i32) {
        use std::io::Write;
        use std::process::{Command, Stdio};

        require_real_rg();

        let tempdir = tempfile::tempdir().expect("tempdir for rg differential test");
        for (path, content) in case.files {
            let host_path = tempdir.path().join(path.trim_start_matches('/'));
            if let Some(parent) = host_path.parent() {
                std::fs::create_dir_all(parent).expect("create parent dir for rg fixture");
            }
            std::fs::write(host_path, content).expect("write rg fixture file");
        }

        let host_cwd = tempdir.path().join(case.cwd.trim_start_matches('/'));
        let mapped_args: Vec<String> = case
            .args
            .iter()
            .map(|arg| {
                if arg.starts_with('/') {
                    tempdir
                        .path()
                        .join(arg.trim_start_matches('/'))
                        .to_string_lossy()
                        .into_owned()
                } else {
                    (*arg).to_string()
                }
            })
            .collect();

        let mut command = Command::new("rg");
        command
            .args(["--threads", "1"])
            .args(&mapped_args)
            .current_dir(host_cwd)
            .env("LC_ALL", "C");

        let output = if let Some(stdin) = case.stdin {
            let mut child = command
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .spawn()
                .expect("spawn real rg for differential test");
            child
                .stdin
                .as_mut()
                .expect("real rg stdin pipe")
                .write_all(stdin.as_bytes())
                .expect("write stdin to real rg");
            child
                .wait_with_output()
                .expect("wait for real rg differential test")
        } else {
            command.output().expect("run real rg differential test")
        };

        let stdout = String::from_utf8_lossy(&output.stdout)
            .replace(&tempdir.path().to_string_lossy().to_string(), "");
        (stdout, output.status.code().unwrap_or(-1))
    }

    async fn assert_rg_diff_case(case: &RgDiffCase) {
        let (real_stdout, real_code) = run_real_rg(case);
        let bashkit = run_rg_with_cwd(case.args, case.stdin, case.files, case.cwd).await;
        match case.output {
            RgDiffOutput::Exact => assert_eq!(
                bashkit.stdout, real_stdout,
                "stdout mismatch for rg differential case {}",
                case.name
            ),
            RgDiffOutput::UnorderedLines => assert_eq!(
                sorted_lines(&bashkit.stdout),
                sorted_lines(&real_stdout),
                "stdout line-set mismatch for rg differential case {}",
                case.name
            ),
        }
        assert_eq!(
            bashkit.exit_code, real_code,
            "exit-code mismatch for rg differential case {}",
            case.name
        );
    }

    fn sorted_lines(output: &str) -> Vec<&str> {
        let mut lines: Vec<&str> = output.lines().collect();
        lines.sort_unstable();
        lines
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

    #[tokio::test]
    async fn test_rg_help_and_version() {
        let long_help = run_rg(&["--help"], None, &[]).await;
        assert_eq!(long_help.exit_code, 0);
        assert!(
            long_help
                .stdout
                .contains("Usage: rg [OPTIONS] PATTERN [PATH...]")
        );
        assert!(long_help.stdout.contains("-h, --help"));
        assert!(long_help.stdout.contains("--version"));

        let short_help = run_rg(&["-h"], None, &[]).await;
        assert_eq!(short_help.exit_code, 0);
        assert_eq!(short_help.stdout, long_help.stdout);

        let version = run_rg(&["--version"], None, &[]).await;
        assert_eq!(version.exit_code, 0);
        assert_eq!(version.stdout, "rg (bashkit) 0.1\n");
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
    async fn test_rg_max_count_requires_value() {
        let args: Vec<String> = vec!["hello".to_string(), "-m".to_string()];
        let result = RgOptions::parse(&args);
        assert!(matches!(
            result,
            Err(Error::Execution(msg)) if msg == "rg: -m requires an argument"
        ));
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
            #[cfg(feature = "ssh")]
            ssh_client: None,
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
    async fn test_rg_no_line_numbers_default() {
        // Non-tty: line numbers suppressed by default (like real rg)
        let result = run_rg(
            &["world", "/test.txt"],
            None,
            &[("/test.txt", b"hello\nworld\n")],
        )
        .await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout.trim(), "world");
        assert!(!result.stdout.contains("2:"));
    }

    #[tokio::test]
    async fn test_rg_line_numbers_explicit() {
        // -n flag enables line numbers
        let result = run_rg(
            &["-n", "world", "/test.txt"],
            None,
            &[("/test.txt", b"hello\nworld\n")],
        )
        .await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("2:world"));
    }

    #[tokio::test]
    async fn test_rg_no_line_number_flag_short() {
        // -N flag explicitly disables line numbers
        let result = run_rg(
            &["-N", "world", "/test.txt"],
            None,
            &[("/test.txt", b"hello\nworld\n")],
        )
        .await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout.trim(), "world");
    }

    #[tokio::test]
    async fn test_rg_no_line_number_flag_long() {
        // --no-line-number flag explicitly disables line numbers
        let result = run_rg(
            &["--no-line-number", "world", "/test.txt"],
            None,
            &[("/test.txt", b"hello\nworld\n")],
        )
        .await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout.trim(), "world");
    }

    #[tokio::test]
    async fn test_rg_line_number_long_flag() {
        // --line-number flag enables line numbers
        let result = run_rg(
            &["--line-number", "world", "/test.txt"],
            None,
            &[("/test.txt", b"hello\nworld\n")],
        )
        .await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("2:world"));
    }

    #[tokio::test]
    async fn test_rg_stdin_no_line_numbers() {
        // Stdin piped: no line numbers by default
        let result = run_rg(&["hello"], Some("hello world\n"), &[]).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout.trim(), "hello world");
        assert!(!result.stdout.contains("1:"));
    }

    #[tokio::test]
    async fn test_rg_context_before_after() {
        let result = run_rg(
            &["-n", "-B", "1", "-A", "1", "needle", "/test.txt"],
            None,
            &[(
                "/test.txt",
                b"before\nneedle\nmiddle\nother\nneedle2\nafter\n",
            )],
        )
        .await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(
            result.stdout,
            "1-before\n2:needle\n3-middle\n4-other\n5:needle2\n6-after\n"
        );
    }

    #[tokio::test]
    async fn test_rg_context_combined_flag() {
        let result = run_rg(
            &["-nC1", "needle", "/test.txt"],
            None,
            &[("/test.txt", b"before\nneedle\nafter\n")],
        )
        .await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "1-before\n2:needle\n3-after\n");
    }

    #[tokio::test]
    async fn test_rg_glob_include_and_exclude() {
        let result = run_rg_with_cwd(
            &["--glob", "*.rs", "-g", "!vendor/**", "needle", "."],
            None,
            &[
                ("/proj/src/main.rs", b"needle\n"),
                ("/proj/src/main.txt", b"needle\n"),
                ("/proj/vendor/lib.rs", b"needle\n"),
            ],
            "/proj",
        )
        .await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("./src/main.rs:needle"));
        assert!(!result.stdout.contains("main.txt"));
        assert!(!result.stdout.contains("vendor"));
    }

    #[tokio::test]
    async fn test_rg_only_matching_and_quiet() {
        let only = run_rg(
            &["-o", "ba.", "/test.txt"],
            None,
            &[("/test.txt", b"bar baz\n")],
        )
        .await;
        assert_eq!(only.exit_code, 0);
        assert_eq!(only.stdout, "bar\nbaz\n");

        let quiet = run_rg(
            &["-q", "bar", "/test.txt"],
            None,
            &[("/test.txt", b"bar\n")],
        )
        .await;
        assert_eq!(quiet.exit_code, 0);
        assert_eq!(quiet.stdout, "");
    }

    #[tokio::test]
    async fn test_rg_indexed_search_ignores_outside_root_match_paths() {
        let inner = InMemoryFs::new();
        inner.mkdir(Path::new("/safe"), true).await.unwrap();
        inner
            .write_file(Path::new("/safe/a.txt"), b"safe text\n")
            .await
            .unwrap();
        inner
            .write_file(Path::new("/safe/secret.txt"), b"secret\n")
            .await
            .unwrap();
        inner
            .write_file(Path::new("/leak.txt"), b"secret\n")
            .await
            .unwrap();

        let fs = Arc::new(IndexedTestFs {
            inner,
            matches: vec![SearchMatch {
                path: PathBuf::from("/leak.txt"),
                line_number: 1,
                line_content: "secret".to_string(),
            }],
        });

        let result = run_rg_with_fs(&["secret", "/safe"], None, fs).await;
        assert_eq!(result.exit_code, 1);
        assert_eq!(result.stdout, "");
    }

    #[test]
    fn real_rg_binary_is_available_for_differential_tests() {
        require_real_rg();
    }

    #[tokio::test]
    async fn diff_rg_matches_real_rg_cases() {
        for case in RG_DIFF_CASES {
            assert_rg_diff_case(case).await;
        }
    }
}
