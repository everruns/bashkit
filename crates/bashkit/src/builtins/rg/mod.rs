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
use serde_json::json;
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

use super::search_common::build_regex_opts;
use super::{Builtin, Context, read_text_file, resolve_path};
use crate::error::{Error, Result};
use crate::interpreter::ExecResult;

/// rg command - recursive pattern search (simplified ripgrep)
pub struct Rg;

struct RgOptions {
    patterns: Vec<String>,
    pattern_files: Vec<String>,
    paths: Vec<String>,
    ignore_case: bool,
    smart_case: bool,
    line_numbers: bool,
    line_regexp: bool,
    count_only: bool,
    count_matches: bool,
    column: bool,
    byte_offset: bool,
    vimgrep: bool,
    json: bool,
    stats: bool,
    files_with_matches: bool,
    invert_match: bool,
    word_boundary: bool,
    fixed_strings: bool,
    text: bool,
    binary: bool,
    max_count: Option<usize>,
    max_columns: Option<usize>,
    max_columns_preview: bool,
    max_depth: Option<usize>,
    before_context: usize,
    after_context: usize,
    no_filename: bool,
    show_filename: bool,
    only_matching: bool,
    quiet: bool,
    files_without_matches: bool,
    list_files: bool,
    replacement: Option<String>,
    passthru: bool,
    trim: bool,
    include_zero: bool,
    heading: bool,
    null: bool,
    sort_reverse: bool,
    path_separator: String,
    encoding: RgEncoding,
    hidden: bool,
    type_list: bool,
    no_ignore: bool,
    no_ignore_dot: bool,
    no_ignore_vcs: bool,
    require_git: bool,
    messages: bool,
    context_separator: String,
    field_match_separator: String,
    field_context_separator: String,
    stdin_consumed_for_patterns: bool,
    ignore_file_paths: Vec<String>,
    explicit_ignore_rules: Vec<RgIgnoreRule>,
    glob_rules: Vec<RgGlobRule>,
    type_database: RgTypeDatabase,
    type_includes: Vec<RgFileType>,
    type_excludes: Vec<RgFileType>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RgEncoding {
    Auto,
    None,
    Utf8,
    Utf16Le,
    Utf16Be,
}

#[derive(Clone)]
struct RgGlobRule {
    raw: String,
    include: bool,
    has_slash: bool,
    anchored: bool,
    regex: Regex,
}

#[derive(Clone)]
struct RgIgnoreRule {
    include: bool,
    dir_only: bool,
    base: PathBuf,
    has_slash: bool,
    anchored: bool,
    regex: Regex,
}

#[derive(Clone)]
struct RgTypeDatabase {
    definitions: BTreeMap<String, Vec<RgTypeGlob>>,
}

#[derive(Clone)]
struct RgFileType {
    globs: Vec<RgTypeGlob>,
}

#[derive(Clone)]
struct RgTypeGlob {
    raw: String,
    regex: Regex,
}

impl RgOptions {
    fn apply_unrestricted(&mut self) {
        if !self.no_ignore {
            self.no_ignore = true;
        } else if !self.hidden {
            self.hidden = true;
        } else {
            self.binary = true;
        }
    }

    fn parse(args: &[String]) -> Result<Self> {
        let mut opts = RgOptions {
            patterns: Vec::new(),
            pattern_files: Vec::new(),
            paths: Vec::new(),
            ignore_case: false,
            smart_case: false,
            line_numbers: false, // non-tty: suppress line numbers (real rg behavior)
            line_regexp: false,
            count_only: false,
            count_matches: false,
            column: false,
            byte_offset: false,
            vimgrep: false,
            json: false,
            stats: false,
            files_with_matches: false,
            invert_match: false,
            word_boundary: false,
            fixed_strings: false,
            text: false,
            binary: false,
            max_count: None,
            max_columns: None,
            max_columns_preview: false,
            max_depth: None,
            before_context: 0,
            after_context: 0,
            no_filename: false,
            show_filename: false,
            only_matching: false,
            quiet: false,
            files_without_matches: false,
            list_files: false,
            replacement: None,
            passthru: false,
            trim: false,
            include_zero: false,
            heading: false,
            null: false,
            sort_reverse: false,
            path_separator: "/".to_string(),
            encoding: RgEncoding::Auto,
            hidden: false,
            type_list: false,
            no_ignore: false,
            no_ignore_dot: false,
            no_ignore_vcs: false,
            require_git: true,
            messages: true,
            context_separator: "--".to_string(),
            field_match_separator: ":".to_string(),
            field_context_separator: "-".to_string(),
            stdin_consumed_for_patterns: false,
            ignore_file_paths: Vec::new(),
            explicit_ignore_rules: Vec::new(),
            glob_rules: Vec::new(),
            type_database: RgTypeDatabase::default(),
            type_includes: Vec::new(),
            type_excludes: Vec::new(),
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
            } else if let Some(val) = p.flag_value("-f", "rg").map_err(Error::Execution)? {
                opts.pattern_files.push(val.to_string());
            } else if let Some(val) = long_value(&mut p, "--file")? {
                opts.pattern_files.push(val);
            } else if let Some(val) = p.flag_value("-r", "rg").map_err(Error::Execution)? {
                opts.replacement = Some(val.to_string());
            } else if let Some(val) = long_value(&mut p, "--replace")? {
                opts.replacement = Some(val);
            } else if let Some(val) = p.flag_value("-m", "rg").map_err(Error::Execution)? {
                opts.max_count = Some(
                    val.parse()
                        .map_err(|_| Error::Execution(format!("rg: invalid -m value: {val}")))?,
                );
            } else if let Some(val) = long_value(&mut p, "--max-count")? {
                opts.max_count = Some(val.parse().map_err(|_| {
                    Error::Execution(format!("rg: invalid --max-count value: {val}"))
                })?);
            } else if let Some(val) = p.flag_value("-M", "rg").map_err(Error::Execution)? {
                opts.max_columns = Some(
                    val.parse()
                        .map_err(|_| Error::Execution(format!("rg: invalid -M value: {val}")))?,
                );
            } else if let Some(val) = long_value(&mut p, "--max-columns")? {
                opts.max_columns = Some(val.parse().map_err(|_| {
                    Error::Execution(format!("rg: invalid --max-columns value: {val}"))
                })?);
            } else if let Some(val) = long_value(&mut p, "--max-depth")? {
                opts.max_depth = Some(val.parse().map_err(|_| {
                    Error::Execution(format!("rg: invalid --max-depth value: {val}"))
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
            } else if let Some(val) = long_value(&mut p, "--context-separator")? {
                opts.context_separator = val;
            } else if let Some(val) = long_value(&mut p, "--field-match-separator")? {
                opts.field_match_separator = val;
            } else if let Some(val) = long_value(&mut p, "--field-context-separator")? {
                opts.field_context_separator = val;
            } else if let Some(val) = p.flag_value("-g", "rg").map_err(Error::Execution)? {
                opts.glob_rules.push(RgGlobRule::parse(val)?);
            } else if let Some(val) = long_value(&mut p, "--glob")? {
                opts.glob_rules.push(RgGlobRule::parse(&val)?);
            } else if let Some(val) = long_value(&mut p, "--ignore-file")? {
                opts.ignore_file_paths.push(val);
            } else if let Some(val) = p.flag_value("-t", "rg").map_err(Error::Execution)? {
                opts.type_includes.push(opts.type_database.parse(val)?);
            } else if let Some(val) = p.flag_value("-T", "rg").map_err(Error::Execution)? {
                opts.type_excludes.push(opts.type_database.parse(val)?);
            } else if let Some(val) = long_value(&mut p, "--type")? {
                opts.type_includes.push(opts.type_database.parse(&val)?);
            } else if let Some(val) = long_value(&mut p, "--type-not")? {
                opts.type_excludes.push(opts.type_database.parse(&val)?);
            } else if let Some(val) = long_value(&mut p, "--type-add")? {
                opts.type_database.add(&val)?;
            } else if let Some(val) = long_value(&mut p, "--type-clear")? {
                opts.type_database.clear(&val);
            } else if p.flag("--type-list") {
                opts.type_list = true;
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
                opts.smart_case = false;
            } else if p.flag_any(&["--case-sensitive"]) {
                opts.ignore_case = false;
                opts.smart_case = false;
            } else if p.flag_any(&["--smart-case"]) {
                opts.ignore_case = false;
                opts.smart_case = true;
            } else if p.flag_any(&["--count"]) {
                opts.count_only = true;
            } else if p.flag_any(&["--count-matches"]) {
                opts.count_matches = true;
            } else if p.flag_any(&["--files-with-matches"]) {
                opts.files_with_matches = true;
            } else if p.flag_any(&["--files-without-match"]) {
                opts.files_without_matches = true;
            } else if p.flag_any(&["--invert-match"]) {
                opts.invert_match = true;
            } else if p.flag_any(&["--word-regexp"]) {
                opts.word_boundary = true;
            } else if p.flag_any(&["--line-regexp"]) {
                opts.line_regexp = true;
            } else if p.flag_any(&["--fixed-strings"]) {
                opts.fixed_strings = true;
            } else if p.flag_any(&["--text"]) {
                opts.text = true;
            } else if p.flag("--binary") {
                opts.binary = true;
            } else if p.flag_any(&["--only-matching"]) {
                opts.only_matching = true;
            } else if p.flag_any(&["--quiet", "--silent"]) {
                opts.quiet = true;
            } else if p.flag("--column") {
                opts.column = true;
                opts.line_numbers = true;
            } else if p.flag("--byte-offset") {
                opts.byte_offset = true;
            } else if p.flag("--vimgrep") {
                opts.vimgrep = true;
                opts.show_filename = true;
            } else if p.flag("--json") {
                opts.json = true;
            } else if p.flag("--stats") {
                opts.stats = true;
            } else if p.flag("--files") {
                opts.list_files = true;
            } else if p.flag("--passthru") {
                opts.passthru = true;
            } else if p.flag("--trim") {
                opts.trim = true;
            } else if p.flag("--max-columns-preview") {
                opts.max_columns_preview = true;
            } else if p.flag("--include-zero") {
                opts.include_zero = true;
            } else if p.flag("--heading") {
                opts.heading = true;
            } else if p.flag("--no-heading") {
                opts.heading = false;
            } else if p.flag("--null") {
                opts.null = true;
            } else if p.flag("--sort-files") {
                // no-op: bashkit's recursive walker already sorts paths.
            } else if long_value(&mut p, "--sort")?.is_some() {
                opts.sort_reverse = false;
            } else if long_value(&mut p, "--sortr")?.is_some() {
                opts.sort_reverse = true;
            } else if let Some(val) = long_value(&mut p, "--path-separator")? {
                opts.path_separator = parse_path_separator(&val)?;
            } else if let Some(val) = p.flag_value("-E", "rg").map_err(Error::Execution)? {
                opts.encoding = parse_encoding(val)?;
            } else if let Some(val) = long_value(&mut p, "--encoding")? {
                opts.encoding = parse_encoding(&val)?;
            } else if let Some(val) = long_equals_value(&mut p, "--engine") {
                parse_regex_engine(&val)?;
            } else if p.flag("--color") {
                // no-op (may have separate value arg like "never", skip it)
                let _ = p.positional();
            } else if p.current().is_some_and(|s| s.starts_with("--color=")) {
                // --color=VALUE is a no-op
                p.advance();
            } else if p.flag("--hidden") {
                opts.hidden = true;
            } else if p.flag("--no-hidden") {
                opts.hidden = false;
            } else if p.flag("--no-ignore") {
                opts.no_ignore = true;
                opts.no_ignore_dot = true;
                opts.no_ignore_vcs = true;
            } else if p.flag("--no-ignore-dot") {
                opts.no_ignore_dot = true;
            } else if p.flag("--no-ignore-vcs") {
                opts.no_ignore_vcs = true;
            } else if p.flag("--no-require-git") {
                opts.require_git = false;
            } else if p.flag("--require-git") {
                opts.require_git = true;
            } else if p.flag("--unrestricted") {
                opts.apply_unrestricted();
            } else if p.flag("--no-messages") {
                opts.messages = false;
            } else if p.flag("--messages") {
                opts.messages = true;
            } else if p.flag_any(&[
                "--no-config",
                "--line-buffered",
                "--block-buffered",
                "--no-ignore-parent",
                "--follow",
                "--mmap",
                "--no-mmap",
                "--pcre2",
                "--no-pcre2",
                "--auto-hybrid-regex",
                "--no-auto-hybrid-regex",
            ]) {
                // no-op: these flags affect host config, buffering, parent ignore
                // discovery, symlink walking, IO strategy, or regex engine
                // selection, none of which are modeled here for simple searches.
            } else if p.is_flag() {
                // Combined short flags like -inFw
                // Safe: is_flag() guarantees current() is Some
                let arg = p.current().expect("is_flag guarantees Some");
                if arg.starts_with("--") {
                    return Err(Error::Execution(format!(
                        "rg: unrecognized option '{}'",
                        arg
                    )));
                }
                let chars: Vec<char> = arg[1..].chars().collect();
                p.advance();
                for (j, &c) in chars.iter().enumerate() {
                    match c {
                        'i' => {
                            opts.ignore_case = true;
                            opts.smart_case = false;
                        }
                        's' => {
                            opts.ignore_case = false;
                            opts.smart_case = false;
                        }
                        'S' => {
                            opts.ignore_case = false;
                            opts.smart_case = true;
                        }
                        'n' => opts.line_numbers = true,
                        'N' => opts.line_numbers = false,
                        'c' => opts.count_only = true,
                        'l' => opts.files_with_matches = true,
                        'v' => opts.invert_match = true,
                        'w' => opts.word_boundary = true,
                        'x' => opts.line_regexp = true,
                        'F' => opts.fixed_strings = true,
                        'a' => opts.text = true,
                        'u' => opts.apply_unrestricted(),
                        'H' => opts.show_filename = true,
                        'I' => opts.no_filename = true,
                        'o' => opts.only_matching = true,
                        'q' => opts.quiet = true,
                        'b' => opts.byte_offset = true,
                        'P' => {}
                        'E' => {
                            let rest: String = chars[j + 1..].iter().collect();
                            let encoding = if !rest.is_empty() {
                                rest
                            } else {
                                match p.positional() {
                                    Some(v) => v.to_string(),
                                    None => {
                                        return Err(Error::Execution(
                                            "rg: -E requires an argument".to_string(),
                                        ));
                                    }
                                }
                            };
                            opts.encoding = parse_encoding(&encoding)?;
                            break;
                        }
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
                        'f' => {
                            let rest: String = chars[j + 1..].iter().collect();
                            let pattern_file = if !rest.is_empty() {
                                rest
                            } else {
                                match p.positional() {
                                    Some(v) => v.to_string(),
                                    None => {
                                        return Err(Error::Execution(
                                            "rg: -f requires an argument".to_string(),
                                        ));
                                    }
                                }
                            };
                            opts.pattern_files.push(pattern_file);
                            break;
                        }
                        'r' => {
                            let rest: String = chars[j + 1..].iter().collect();
                            let replacement = if !rest.is_empty() {
                                rest
                            } else {
                                match p.positional() {
                                    Some(v) => v.to_string(),
                                    None => {
                                        return Err(Error::Execution(
                                            "rg: -r requires an argument".to_string(),
                                        ));
                                    }
                                }
                            };
                            opts.replacement = Some(replacement);
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
                        'M' => {
                            let rest: String = chars[j + 1..].iter().collect();
                            let num_str = if !rest.is_empty() {
                                rest
                            } else {
                                match p.positional() {
                                    Some(v) => v.to_string(),
                                    None => {
                                        return Err(Error::Execution(
                                            "rg: -M requires an argument".to_string(),
                                        ));
                                    }
                                }
                            };
                            opts.max_columns = Some(num_str.parse().map_err(|_| {
                                Error::Execution(format!("rg: invalid -M value: {num_str}"))
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
                        't' => {
                            let rest: String = chars[j + 1..].iter().collect();
                            let file_type = if !rest.is_empty() {
                                rest
                            } else {
                                match p.positional() {
                                    Some(v) => v.to_string(),
                                    None => {
                                        return Err(Error::Execution(
                                            "rg: -t requires an argument".to_string(),
                                        ));
                                    }
                                }
                            };
                            opts.type_includes
                                .push(opts.type_database.parse(&file_type)?);
                            break;
                        }
                        'T' => {
                            let rest: String = chars[j + 1..].iter().collect();
                            let file_type = if !rest.is_empty() {
                                rest
                            } else {
                                match p.positional() {
                                    Some(v) => v.to_string(),
                                    None => {
                                        return Err(Error::Execution(
                                            "rg: -T requires an argument".to_string(),
                                        ));
                                    }
                                }
                            };
                            opts.type_excludes
                                .push(opts.type_database.parse(&file_type)?);
                            break;
                        }
                        _ => {
                            return Err(Error::Execution(format!(
                                "rg: unrecognized option '-{}'",
                                c
                            )));
                        }
                    }
                }
            } else if let Some(arg) = p.positional() {
                positional.push(arg.to_string());
            }
        }

        if positional.is_empty() {
            if opts.patterns.is_empty()
                && opts.pattern_files.is_empty()
                && !opts.list_files
                && !opts.type_list
            {
                return Err(Error::Execution("rg: missing pattern".to_string()));
            }
        } else if opts.patterns.is_empty()
            && opts.pattern_files.is_empty()
            && !opts.list_files
            && !opts.type_list
        {
            opts.patterns.push(positional.remove(0));
        }

        opts.paths = positional;

        Ok(opts)
    }

    fn build_regex(&self) -> Result<Regex> {
        let combined = self
            .patterns
            .iter()
            .map(|pattern| format!("(?:{})", self.prepare_pattern(pattern)))
            .collect::<Vec<_>>()
            .join("|");
        build_regex_opts(&combined, self.effective_ignore_case())
            .map_err(|e| Error::Execution(format!("rg: invalid pattern: {}", e)))
    }

    fn prepare_pattern(&self, pattern: &str) -> String {
        let pat = if self.fixed_strings {
            regex::escape(pattern)
        } else {
            pattern.to_string()
        };
        let pat = if self.word_boundary {
            format!(r"\b{}\b", pat)
        } else {
            pat
        };
        if self.line_regexp {
            format!("^(?:{})$", pat)
        } else {
            pat
        }
    }

    fn effective_ignore_case(&self) -> bool {
        self.ignore_case
            || (self.smart_case
                && !self
                    .patterns
                    .iter()
                    .any(|pattern| pattern.chars().any(|c| c.is_uppercase())))
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

    fn matches_type_filters(&self, path: &Path) -> bool {
        if !self.type_includes.is_empty()
            && !self
                .type_includes
                .iter()
                .any(|file_type| file_type.matches(path))
        {
            return false;
        }
        !self
            .type_excludes
            .iter()
            .any(|file_type| file_type.matches(path))
    }

    fn first_positive_glob(&self) -> Option<String> {
        self.glob_rules
            .iter()
            .find(|g| g.include)
            .map(|g| g.raw.clone())
    }

    fn uses_ignore_files(&self) -> bool {
        !self.no_ignore || !self.ignore_file_paths.is_empty()
    }

    fn is_ignored_by_rules(&self, path: &Path, is_dir: bool, rules: &[RgIgnoreRule]) -> bool {
        let mut ignored = false;
        for rule in rules {
            if rule.matches(path, is_dir) || (!is_dir && rule.matches_parent_dir(path)) {
                ignored = !rule.include;
            }
        }
        ignored
    }
}

impl RgIgnoreRule {
    fn parse(line: &str, base: &Path) -> Result<Option<Self>> {
        let mut pattern = line.trim();
        if pattern.is_empty() || pattern.starts_with('#') {
            return Ok(None);
        }
        if let Some(rest) = pattern.strip_prefix(r"\#") {
            pattern = rest;
        }

        let (include, pattern) = match pattern.strip_prefix('!') {
            Some(rest) => (true, rest),
            None => (false, pattern),
        };
        let pattern = pattern.strip_prefix(r"\!").unwrap_or(pattern);
        let dir_only = pattern.ends_with('/');
        let pattern = pattern
            .trim_end_matches('/')
            .strip_prefix("./")
            .unwrap_or_else(|| pattern.trim_end_matches('/'));
        if pattern.is_empty() {
            return Ok(None);
        }

        let anchored = pattern.starts_with('/');
        let normalized = pattern.trim_start_matches('/');
        let has_slash = normalized.contains('/');
        let regex = build_regex_opts(&glob_to_regex(normalized), false)
            .map_err(|e| Error::Execution(format!("rg: invalid ignore pattern: {}", e)))?;
        Ok(Some(Self {
            include,
            dir_only,
            base: base.to_path_buf(),
            has_slash,
            anchored,
            regex,
        }))
    }

    fn matches(&self, path: &Path, is_dir: bool) -> bool {
        if self.dir_only && !is_dir {
            return false;
        }
        let Ok(relative) = path.strip_prefix(&self.base) else {
            return false;
        };
        let relative = path_to_slash(relative).trim_start_matches('/').to_string();
        let target = if self.has_slash || self.anchored {
            relative
        } else {
            path.file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("")
                .to_string()
        };
        self.regex.is_match(&target)
    }

    fn matches_parent_dir(&self, path: &Path) -> bool {
        if !self.dir_only {
            return false;
        }
        let Some(parent) = path.parent() else {
            return false;
        };
        for ancestor in parent.ancestors() {
            if ancestor == self.base {
                break;
            }
            if self.matches(ancestor, true) {
                return true;
            }
        }
        false
    }
}

impl RgTypeDatabase {
    fn default() -> Self {
        let mut db = Self {
            definitions: BTreeMap::new(),
        };
        db.insert_defaults("c", &["*.c", "*.h"]);
        db.insert_defaults(
            "cpp",
            &[
                "*.c++", "*.cc", "*.cpp", "*.cxx", "*.h++", "*.hh", "*.hpp", "*.hxx",
            ],
        );
        db.insert_defaults(
            "c++",
            &[
                "*.c++", "*.cc", "*.cpp", "*.cxx", "*.h++", "*.hh", "*.hpp", "*.hxx",
            ],
        );
        db.insert_defaults("css", &["*.css"]);
        db.insert_defaults("go", &["*.go"]);
        db.insert_defaults("html", &["*.htm", "*.html"]);
        db.insert_defaults("htm", &["*.htm", "*.html"]);
        db.insert_defaults("java", &["*.java"]);
        db.insert_defaults("json", &["*.json", "*.jsonl"]);
        db.insert_defaults(
            "markdown",
            &[
                "*.markdown",
                "*.md",
                "*.mdown",
                "*.mdwn",
                "*.mdx",
                "*.mkd",
                "*.mkdn",
            ],
        );
        db.insert_defaults(
            "md",
            &[
                "*.markdown",
                "*.md",
                "*.mdown",
                "*.mdwn",
                "*.mdx",
                "*.mkd",
                "*.mkdn",
            ],
        );
        db.insert_defaults("py", &["*.py", "*.pyi", "*.pyw"]);
        db.insert_defaults("python", &["*.py", "*.pyi", "*.pyw"]);
        db.insert_defaults("rs", &["*.rs"]);
        db.insert_defaults("rust", &["*.rs"]);
        db.insert_defaults(
            "sh",
            &[
                "*.bash", "*.bashrc", "*.csh", "*.env", "*.ksh", "*.sh", "*.tcsh", "*.zsh",
                ".bashrc", ".env", ".profile", ".zshrc", "bashrc", "profile", "zshrc",
            ],
        );
        db.insert_defaults(
            "shell",
            &[
                "*.bash", "*.bashrc", "*.csh", "*.env", "*.ksh", "*.sh", "*.tcsh", "*.zsh",
                ".bashrc", ".env", ".profile", ".zshrc", "bashrc", "profile", "zshrc",
            ],
        );
        db.insert_defaults("text", &["*.txt"]);
        db.insert_defaults("txt", &["*.txt"]);
        db.insert_defaults("toml", &["*.toml"]);
        db.insert_defaults("ts", &["*.cts", "*.mts", "*.ts", "*.tsx"]);
        db.insert_defaults("typescript", &["*.cts", "*.mts", "*.ts", "*.tsx"]);
        db.insert_defaults("js", &["*.cjs", "*.js", "*.jsx", "*.mjs", "*.vue"]);
        db.insert_defaults("javascript", &["*.cjs", "*.js", "*.jsx", "*.mjs", "*.vue"]);
        db.insert_defaults("yaml", &["*.yaml", "*.yml"]);
        db.insert_defaults("yml", &["*.yaml", "*.yml"]);
        db
    }

    fn insert_defaults(&mut self, name: &str, globs: &[&str]) {
        self.definitions.insert(
            name.to_string(),
            globs
                .iter()
                .map(|glob| RgTypeGlob::parse(glob).expect("default rg type glob is valid"))
                .collect(),
        );
    }

    fn parse(&self, name: &str) -> Result<RgFileType> {
        let Some(globs) = self.definitions.get(name) else {
            return Err(Error::Execution(format!(
                "rg: unrecognized file type: {}",
                name
            )));
        };
        Ok(RgFileType {
            globs: globs.clone(),
        })
    }

    fn add(&mut self, definition: &str) -> Result<()> {
        let Some((name, glob)) = definition.split_once(':') else {
            return Err(Error::Execution(
                "rg: invalid definition (format is type:glob, e.g., html:*.html)".to_string(),
            ));
        };
        if name.is_empty() || glob.is_empty() || glob.contains(':') {
            return Err(Error::Execution(
                "rg: invalid definition (format is type:glob, e.g., html:*.html)".to_string(),
            ));
        }
        self.definitions
            .entry(name.to_string())
            .or_default()
            .push(RgTypeGlob::parse(glob)?);
        Ok(())
    }

    fn clear(&mut self, name: &str) {
        self.definitions.remove(name);
    }

    fn list(&self) -> String {
        let mut output = String::new();
        for (name, globs) in &self.definitions {
            let mut raw_globs: Vec<&str> = globs.iter().map(|glob| glob.raw.as_str()).collect();
            raw_globs.sort_unstable();
            raw_globs.dedup();
            output.push_str(name);
            output.push_str(": ");
            output.push_str(&raw_globs.join(", "));
            output.push('\n');
        }
        output
    }
}

impl RgFileType {
    fn matches(&self, path: &Path) -> bool {
        self.globs.iter().any(|glob| glob.matches(path))
    }
}

impl RgTypeGlob {
    fn parse(pattern: &str) -> Result<Self> {
        let regex = build_regex_opts(&glob_to_regex(pattern), false)
            .map_err(|e| Error::Execution(format!("rg: invalid --type-add value: {}", e)))?;
        Ok(Self {
            raw: pattern.to_string(),
            regex,
        })
    }

    fn matches(&self, path: &Path) -> bool {
        if self.raw.contains('/') {
            let path_string = path.to_string_lossy();
            self.regex.is_match(&path_string)
        } else {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| self.regex.is_match(name))
        }
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

fn display_path_for(path: &Path, cwd: &Path, root_arg: Option<&str>, opts: &RgOptions) -> String {
    let display = match root_arg {
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
    };
    apply_path_separator_to_display(&display, opts)
}

fn apply_path_separator_to_display(display: &str, opts: &RgOptions) -> String {
    if opts.path_separator == "/" {
        display.to_string()
    } else {
        display.replace('/', &opts.path_separator)
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

fn parse_path_separator(value: &str) -> Result<String> {
    if value.len() == 1 {
        Ok(value.to_string())
    } else {
        Err(Error::Execution(format!(
            "rg: error parsing flag --path-separator: path separator must be exactly one byte: {value}"
        )))
    }
}

fn parse_encoding(value: &str) -> Result<RgEncoding> {
    match value.to_ascii_lowercase().as_str() {
        "auto" => Ok(RgEncoding::Auto),
        "none" => Ok(RgEncoding::None),
        "utf-8" | "utf8" => Ok(RgEncoding::Utf8),
        "utf-16le" | "utf16le" => Ok(RgEncoding::Utf16Le),
        "utf-16be" | "utf16be" => Ok(RgEncoding::Utf16Be),
        _ => Err(Error::Execution(format!(
            "rg: error parsing flag --encoding: grep config error: unknown encoding: {value}"
        ))),
    }
}

fn decode_rg_content(content: &[u8], opts: &RgOptions) -> String {
    match opts.encoding {
        RgEncoding::None | RgEncoding::Utf8 => decode_utf8_lossy_strip_bom(content),
        RgEncoding::Utf16Le => decode_utf16_bytes(strip_utf16le_bom(content), false),
        RgEncoding::Utf16Be => decode_utf16_bytes(strip_utf16be_bom(content), true),
        RgEncoding::Auto => {
            if let Some(rest) = content.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
                String::from_utf8_lossy(rest).into_owned()
            } else if let Some(rest) = content.strip_prefix(&[0xFF, 0xFE]) {
                decode_utf16_bytes(rest, false)
            } else if let Some(rest) = content.strip_prefix(&[0xFE, 0xFF]) {
                decode_utf16_bytes(rest, true)
            } else {
                String::from_utf8_lossy(content).into_owned()
            }
        }
    }
}

fn decode_utf8_lossy_strip_bom(content: &[u8]) -> String {
    let content = content.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(content);
    String::from_utf8_lossy(content).into_owned()
}

fn strip_utf16le_bom(content: &[u8]) -> &[u8] {
    content.strip_prefix(&[0xFF, 0xFE]).unwrap_or(content)
}

fn strip_utf16be_bom(content: &[u8]) -> &[u8] {
    content.strip_prefix(&[0xFE, 0xFF]).unwrap_or(content)
}

fn decode_utf16_bytes(content: &[u8], big_endian: bool) -> String {
    let mut units = Vec::with_capacity(content.len().div_ceil(2));
    for chunk in content.chunks(2) {
        let bytes = [chunk[0], *chunk.get(1).unwrap_or(&0)];
        let unit = if big_endian {
            u16::from_be_bytes(bytes)
        } else {
            u16::from_le_bytes(bytes)
        };
        units.push(unit);
    }
    String::from_utf16_lossy(&units)
}

fn parse_regex_engine(value: &str) -> Result<()> {
    match value {
        "default" | "auto" | "pcre2" => Ok(()),
        _ => Err(Error::Execution(format!(
            "rg: error parsing flag --engine: unrecognized regex engine '{value}'"
        ))),
    }
}

fn long_equals_value(p: &mut super::arg_parser::ArgParser<'_>, name: &str) -> Option<String> {
    let current = p.current()?;
    if let Some(value) = current.strip_prefix(&format!("{name}=")) {
        p.advance();
        Some(value.to_string())
    } else {
        None
    }
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

async fn read_rg_text_file(
    fs: &dyn crate::fs::FileSystem,
    path: &Path,
    opts: &RgOptions,
) -> std::result::Result<String, ExecResult> {
    let content = fs
        .read_file(path)
        .await
        .map_err(|e| ExecResult::err(format!("rg: {}: {e}\n", path.display()), 1))?;

    Ok(decode_rg_content(&content, opts))
}

async fn collect_rg_inputs(
    ctx: Context<'_>,
    opts: &RgOptions,
) -> std::result::Result<RgCollectedInputs, ExecResult> {
    if opts.paths.is_empty() {
        if !opts.stdin_consumed_for_patterns
            && let Some(stdin) = ctx.stdin
        {
            return Ok(RgCollectedInputs::new(vec![(
                "(stdin)".to_string(),
                stdin.to_string(),
            )]));
        }

        if let Some(inputs) = try_indexed_search(&*ctx.fs, opts, ctx.cwd).await {
            return Ok(RgCollectedInputs::new(inputs));
        }

        let files =
            collect_rg_files_recursive(&*ctx.fs, std::slice::from_ref(ctx.cwd), opts, ctx.cwd)
                .await;
        return Ok(RgCollectedInputs::new(
            read_rg_files(&*ctx.fs, files, ctx.cwd, None, opts).await,
        ));
    }

    if let Some(inputs) = try_indexed_search(&*ctx.fs, opts, ctx.cwd).await {
        return Ok(RgCollectedInputs::new(inputs));
    }

    let mut collected = RgCollectedInputs::default();
    let mut inputs = Vec::new();
    for p in &opts.paths {
        let path = resolve_path(ctx.cwd, p);
        if let Ok(meta) = ctx.fs.stat(&path).await
            && meta.file_type.is_dir()
        {
            let files =
                collect_rg_files_recursive(&*ctx.fs, std::slice::from_ref(&path), opts, ctx.cwd)
                    .await;
            inputs.extend(read_rg_files(&*ctx.fs, files, ctx.cwd, Some(p), opts).await);
            continue;
        }

        if !opts.matches_globs(&path, ctx.cwd) {
            continue;
        }
        let text = match read_rg_text_file(&*ctx.fs, &path, opts).await {
            Ok(t) => t,
            Err(e) => {
                collected.had_errors = true;
                if opts.messages {
                    collected.stderr.push_str(&e.stderr);
                }
                continue;
            }
        };
        inputs.push((apply_path_separator_to_display(p, opts), text));
    }
    collected.inputs = inputs;
    Ok(collected)
}

#[derive(Default)]
struct RgCollectedInputs {
    inputs: Vec<(String, String)>,
    stderr: String,
    had_errors: bool,
}

impl RgCollectedInputs {
    fn new(inputs: Vec<(String, String)>) -> Self {
        Self {
            inputs,
            ..Default::default()
        }
    }
}

async fn load_rg_pattern_files(
    fs: &dyn crate::fs::FileSystem,
    cwd: &Path,
    stdin: Option<&str>,
    opts: &mut RgOptions,
) -> std::result::Result<(), ExecResult> {
    for pattern_file in opts.pattern_files.clone() {
        let content = if pattern_file == "-" {
            opts.stdin_consumed_for_patterns = true;
            stdin.unwrap_or("").to_string()
        } else {
            let path = resolve_path(cwd, &pattern_file);
            read_text_file(fs, &path, "rg").await?
        };

        opts.patterns
            .extend(content.lines().map(std::string::ToString::to_string));
    }
    Ok(())
}

async fn load_rg_ignore_files(
    fs: &dyn crate::fs::FileSystem,
    cwd: &Path,
    opts: &mut RgOptions,
) -> std::result::Result<(), ExecResult> {
    for ignore_file in opts.ignore_file_paths.clone() {
        let path = resolve_path(cwd, &ignore_file);
        let content = read_text_file(fs, &path, "rg").await?;
        let rules = parse_rg_ignore_rules(&content, cwd)
            .map_err(|e| ExecResult::err(format!("{}\n", e), 2))?;
        opts.explicit_ignore_rules.extend(rules);
    }
    Ok(())
}

fn parse_rg_ignore_rules(content: &str, base: &Path) -> Result<Vec<RgIgnoreRule>> {
    let mut rules = Vec::new();
    for line in content.lines() {
        if let Some(rule) = RgIgnoreRule::parse(line, base)? {
            rules.push(rule);
        }
    }
    Ok(rules)
}

async fn load_local_ignore_rules(
    fs: &dyn crate::fs::FileSystem,
    dir: &Path,
    root: &Path,
    opts: &RgOptions,
    rules: &mut Vec<RgIgnoreRule>,
) -> Result<()> {
    if opts.no_ignore {
        return Ok(());
    }

    if !opts.no_ignore_dot {
        load_optional_ignore_file(fs, &dir.join(".ignore"), dir, rules).await?;
    }
    if !opts.no_ignore_vcs && (!opts.require_git || has_git_dir_in_ancestors(fs, dir, root).await) {
        load_optional_ignore_file(fs, &dir.join(".gitignore"), dir, rules).await?;
    }
    Ok(())
}

async fn load_optional_ignore_file(
    fs: &dyn crate::fs::FileSystem,
    path: &Path,
    base: &Path,
    rules: &mut Vec<RgIgnoreRule>,
) -> Result<()> {
    let Ok(content) = fs.read_file(path).await else {
        return Ok(());
    };
    let content = String::from_utf8_lossy(&content);
    rules.extend(parse_rg_ignore_rules(&content, base)?);
    Ok(())
}

async fn has_git_dir_in_ancestors(fs: &dyn crate::fs::FileSystem, dir: &Path, root: &Path) -> bool {
    for ancestor in dir.ancestors() {
        if ancestor.starts_with(root)
            && fs
                .stat(&ancestor.join(".git"))
                .await
                .is_ok_and(|meta| meta.file_type.is_dir())
        {
            return true;
        }
        if ancestor == root {
            break;
        }
    }
    false
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
    let mut stack: Vec<(PathBuf, PathBuf, usize, Vec<RgIgnoreRule>)> = roots
        .iter()
        .cloned()
        .map(|root| (root.clone(), root, 0, opts.explicit_ignore_rules.clone()))
        .collect();

    while let Some((current, root, depth, inherited_rules)) = stack.pop() {
        let mut rules = inherited_rules;
        let _ = load_local_ignore_rules(fs, &current, &root, opts, &mut rules).await;
        if let Ok(entries) = fs.read_dir(&current).await {
            for entry in entries {
                if !opts.hidden && is_hidden_name(&entry.name) {
                    continue;
                }
                let path = current.join(&entry.name);
                let entry_depth = depth + 1;
                if entry.metadata.file_type.is_dir() {
                    if opts.is_ignored_by_rules(&path, true, &rules) {
                        continue;
                    }
                    if opts
                        .max_depth
                        .is_none_or(|max_depth| entry_depth < max_depth)
                    {
                        stack.push((path, root.clone(), entry_depth, rules.clone()));
                    }
                } else if entry.metadata.file_type.is_file()
                    && opts
                        .max_depth
                        .is_none_or(|max_depth| entry_depth <= max_depth)
                    && !opts.is_ignored_by_rules(&path, false, &rules)
                    && opts.matches_globs(&path, cwd)
                    && opts.matches_type_filters(&path)
                {
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
    if opts.sort_reverse {
        result.reverse();
    }
    result
}

fn is_hidden_name(name: &str) -> bool {
    name.starts_with('.') && name != "." && name != ".."
}

async fn collect_rg_file_list(
    fs: &dyn crate::fs::FileSystem,
    opts: &RgOptions,
    cwd: &Path,
) -> Vec<String> {
    if opts.paths.is_empty() {
        let root = cwd.to_path_buf();
        let files = collect_rg_files_recursive(fs, std::slice::from_ref(&root), opts, cwd).await;
        return files
            .iter()
            .map(|path| display_path_for(path, cwd, None, opts))
            .collect();
    }

    let mut result = Vec::new();
    for p in &opts.paths {
        let path = resolve_path(cwd, p);
        if let Ok(meta) = fs.stat(&path).await
            && meta.file_type.is_dir()
        {
            let files =
                collect_rg_files_recursive(fs, std::slice::from_ref(&path), opts, cwd).await;
            result.extend(
                files
                    .iter()
                    .map(|path| display_path_for(path, cwd, Some(p), opts)),
            );
        } else if meta_is_file_and_matches(fs, &path, opts, cwd).await {
            result.push(display_path_for(&path, cwd, Some(p), opts));
        }
    }
    result.sort();
    if opts.sort_reverse {
        result.reverse();
    }
    result
}

async fn meta_is_file_and_matches(
    fs: &dyn crate::fs::FileSystem,
    path: &Path,
    opts: &RgOptions,
    cwd: &Path,
) -> bool {
    fs.stat(path)
        .await
        .is_ok_and(|meta| meta.file_type.is_file() && opts.matches_globs(path, cwd))
}

async fn read_rg_files(
    fs: &dyn crate::fs::FileSystem,
    files: Vec<PathBuf>,
    cwd: &Path,
    root_arg: Option<&str>,
    opts: &RgOptions,
) -> Vec<(String, String)> {
    let mut inputs = Vec::new();
    for path in files {
        if let Ok(content) = fs.read_file(&path).await {
            inputs.push((
                display_path_for(&path, cwd, root_arg, opts),
                decode_rg_content(&content, opts),
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
    if opts.invert_match
        || opts.files_without_matches
        || opts.uses_ignore_files()
        || opts.patterns.len() != 1
        || !opts.type_includes.is_empty()
        || !opts.type_excludes.is_empty()
    {
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
        let index_can_use_literal = opts.fixed_strings && !opts.word_boundary && !opts.line_regexp;
        let pattern = if index_can_use_literal {
            opts.patterns[0].clone()
        } else {
            opts.prepare_pattern(&opts.patterns[0])
        };
        let query = crate::fs::SearchQuery {
            pattern,
            is_regex: !index_can_use_literal,
            case_insensitive: opts.effective_ignore_case(),
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
                || (!opts.hidden && path_has_hidden_component_relative_to(&candidate, &root))
            {
                continue;
            }
            if let Ok(content) = fs.read_file(&candidate).await {
                inputs.push((
                    display_path_for(&candidate, cwd, root_arg.as_deref(), opts),
                    decode_rg_content(&content, opts),
                ));
            }
        }
    }

    Some(inputs)
}

fn path_has_hidden_component_relative_to(path: &Path, root: &Path) -> bool {
    path.strip_prefix(root)
        .unwrap_or(path)
        .components()
        .any(|component| component.as_os_str().to_str().is_some_and(is_hidden_name))
}

struct RgPrefix<'a> {
    filename: &'a str,
    show_filename: bool,
    line_numbers: bool,
    line_idx: usize,
    column: Option<usize>,
    byte_offset: Option<usize>,
    separator: &'a str,
    null_path_separator: bool,
}

fn write_rg_prefix(output: &mut String, prefix: RgPrefix<'_>) {
    if prefix.show_filename {
        output.push_str(prefix.filename);
        if prefix.null_path_separator {
            output.push('\0');
        } else {
            output.push_str(prefix.separator);
        }
    }
    if prefix.line_numbers {
        output.push_str(&(prefix.line_idx + 1).to_string());
        output.push_str(prefix.separator);
    }
    if let Some(column) = prefix.column {
        output.push_str(&column.to_string());
        output.push_str(prefix.separator);
    }
    if let Some(byte_offset) = prefix.byte_offset {
        output.push_str(&byte_offset.to_string());
        output.push_str(prefix.separator);
    }
}

#[derive(Clone, Copy)]
struct RgLine<'a> {
    text: &'a str,
    raw: &'a str,
    start_offset: usize,
}

fn split_rg_lines(content: &str) -> Vec<RgLine<'_>> {
    let mut lines = Vec::new();
    let mut offset = 0usize;
    for raw in content.split_inclusive('\n') {
        let text = raw
            .strip_suffix('\n')
            .and_then(|line| line.strip_suffix('\r').or(Some(line)))
            .unwrap_or(raw);
        lines.push(RgLine {
            text,
            raw,
            start_offset: offset,
        });
        offset += raw.len();
    }
    lines
}

fn first_nul_offset(content: &str) -> Option<usize> {
    content.as_bytes().iter().position(|&byte| byte == 0)
}

fn format_rg_line(line: &str, regex: &Regex, opts: &RgOptions, matched: bool) -> String {
    let line = if matched {
        if let Some(replacement) = &opts.replacement {
            regex.replace_all(line, replacement.as_str()).into_owned()
        } else {
            line.to_string()
        }
    } else {
        line.to_string()
    };
    if opts.trim {
        line.trim_start().to_string()
    } else {
        line
    }
}

fn format_rg_output_line(line: &str, regex: &Regex, opts: &RgOptions, matched: bool) -> String {
    let line = format_rg_line(line, regex, opts, matched);
    let Some(max_columns) = opts.max_columns else {
        return line;
    };
    if max_columns == 0 || line.chars().count() <= max_columns {
        return line;
    }
    if opts.max_columns_preview {
        let preview: String = line.chars().take(max_columns).collect();
        format!("{preview} [... omitted end of long line]")
    } else if matched {
        "[Omitted long matching line]".to_string()
    } else {
        "[Omitted long context line]".to_string()
    }
}

fn write_rg_context(
    output: &mut String,
    filename: &str,
    regex: &Regex,
    lines: &[RgLine<'_>],
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
            output.push_str(&opts.context_separator);
            output.push('\n');
        }
        prev_line = Some(line_idx);

        let matched = match_set.contains(&line_idx);
        let separator = if matched {
            opts.field_match_separator.as_str()
        } else {
            opts.field_context_separator.as_str()
        };
        write_rg_prefix(
            output,
            RgPrefix {
                filename,
                show_filename,
                line_numbers: opts.line_numbers,
                line_idx,
                column: None,
                byte_offset: if opts.byte_offset {
                    Some(lines[line_idx].start_offset)
                } else {
                    None
                },
                separator,
                null_path_separator: opts.null,
            },
        );
        output.push_str(&format_rg_output_line(
            lines[line_idx].text,
            regex,
            opts,
            matched,
        ));
        output.push('\n');
    }
}

fn write_rg_json_event(output: &mut String, value: serde_json::Value) {
    output.push_str(&value.to_string());
    output.push('\n');
}

fn write_rg_json_begin(output: &mut String, filename: &str) {
    write_rg_json_event(
        output,
        json!({"type":"begin","data":{"path":{"text":filename}}}),
    );
}

fn write_rg_json_match(
    output: &mut String,
    filename: &str,
    line: RgLine<'_>,
    line_idx: usize,
    regex: &Regex,
) {
    let submatches: Vec<_> = regex
        .find_iter(line.text)
        .map(|mat| json!({"match":{"text":mat.as_str()},"start":mat.start(),"end":mat.end()}))
        .collect();
    write_rg_json_event(
        output,
        json!({
            "type":"match",
            "data":{
                "path":{"text":filename},
                "lines":{"text":line.raw},
                "line_number":line_idx + 1,
                "absolute_offset":line.start_offset,
                "submatches":submatches,
            }
        }),
    );
}

fn write_rg_json_end(
    output: &mut String,
    filename: &str,
    bytes_searched: usize,
    matched_lines: usize,
    matches: usize,
) {
    write_rg_json_event(
        output,
        json!({
            "type":"end",
            "data":{
                "path":{"text":filename},
                "binary_offset":null,
                "stats":rg_json_stats(bytes_searched, matched_lines, matches, 1),
            }
        }),
    );
}

fn rg_json_stats(
    bytes_searched: usize,
    matched_lines: usize,
    matches: usize,
    searches: usize,
) -> serde_json::Value {
    json!({
        "elapsed":{"secs":0,"nanos":0,"human":"0.000000s"},
        "searches":searches,
        "searches_with_match":usize::from(matched_lines > 0),
        "bytes_searched":bytes_searched,
        "bytes_printed":0,
        "matched_lines":matched_lines,
        "matches":matches,
    })
}

fn write_rg_json_summary(
    output: &mut String,
    bytes_searched: usize,
    matched_lines: usize,
    matches: usize,
    searches: usize,
) {
    write_rg_json_event(
        output,
        json!({
            "type":"summary",
            "data":{
                "elapsed_total":{"secs":0,"nanos":0,"human":"0.000000s"},
                "stats":rg_json_stats(bytes_searched, matched_lines, matches, searches),
            }
        }),
    );
}

#[derive(Default)]
struct RgSearchStats {
    matches: usize,
    matched_lines: usize,
    files_with_matches: usize,
    files_searched: usize,
    bytes_searched: usize,
}

fn append_rg_stats(output: &mut String, stats: &RgSearchStats, bytes_printed: usize) {
    output.push('\n');
    output.push_str(&format!("{} matches\n", stats.matches));
    output.push_str(&format!("{} matched lines\n", stats.matched_lines));
    output.push_str(&format!(
        "{} files contained matches\n",
        stats.files_with_matches
    ));
    output.push_str(&format!("{} files searched\n", stats.files_searched));
    output.push_str(&format!("{bytes_printed} bytes printed\n"));
    output.push_str(&format!("{} bytes searched\n", stats.bytes_searched));
    output.push_str("0.000000 seconds spent searching\n");
    output.push_str("0.000000 seconds total\n");
}

#[async_trait]
impl Builtin for Rg {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let help_text = "Usage: rg [OPTIONS] PATTERN [PATH...]\nRecursively search for a pattern.\n\n  -i, --ignore-case\tcase insensitive\n  -S, --smart-case\tcase insensitive if pattern is lowercase\n  -s, --case-sensitive\tcase sensitive\n  -n, --line-number\tshow line numbers\n  -N, --no-line-number\tsuppress line numbers\n  --column\tshow column numbers\n  -b, --byte-offset\tshow byte offsets\n  --vimgrep\tshow file:line:column:match lines\n  --json\tshow JSON Lines events\n  --stats\tshow search statistics\n  --null\tterminate path fields with NUL\n  -c, --count\tcount matching lines\n  --count-matches\tcount individual matches\n  --include-zero\tinclude zero counts\n  -l, --files-with-matches\tfiles with matches\n  --files-without-match\tfiles without matches\n  --files\tprint files that would be searched\n  -v, --invert-match\tinvert match\n  -w, --word-regexp\tword boundary\n  -x, --line-regexp\tmatch whole lines\n  -F, --fixed-strings\tfixed strings (literal)\n  -a, --text\tsearch binary files as text\n  --binary\tsearch binary files and print binary-match summaries\n  -o, --only-matching\tshow only matching text\n  -q, --quiet\tsuppress output; exit status only\n  -e, --regexp PATTERN\tuse PATTERN for matching\n  -f, --file PATTERNFILE\tread patterns from file\n  -E, --encoding ENCODING\tdecode searched files using ENCODING\n  -r, --replace REPLACEMENT\treplace matches in output\n  --passthru\tprint matching and non-matching lines\n  --trim\ttrim whitespace from output lines\n  -m, --max-count NUM\tmax count per file\n  -M, --max-columns NUM\tomit lines longer than NUM columns\n  --max-columns-preview\tshow prefixes of long lines\n  --max-depth NUM\tlimit recursive directory depth\n  -A, --after-context NUM\tshow trailing context\n  -B, --before-context NUM\tshow leading context\n  -C, --context NUM\tshow leading and trailing context\n  --context-separator SEP\tset context group separator\n  --field-match-separator SEP\tset match field separator\n  --field-context-separator SEP\tset context field separator\n  --heading\tgroup matches by file\n  --no-heading\tdisable heading output\n  --sort SORTBY\tsort paths (path only)\n  --sortr SORTBY\tsort paths in reverse (path only)\n  --sort-files\tsort --files output\n  --path-separator SEP\tset displayed path separator\n  -g, --glob GLOB\tinclude/exclude paths by glob (!GLOB excludes)\n  -t, --type TYPE\tinclude files matching TYPE\n  -T, --type-not TYPE\texclude files matching TYPE\n  --type-add TYPE:GLOB\tadd a file type glob\n  --type-clear TYPE\tclear a file type definition\n  --type-list\tshow file type definitions\n  --ignore-file FILE\tuse additional ignore file\n  --no-ignore\tdo not use ignore files\n  --no-ignore-dot\tdo not use .ignore files\n  --no-ignore-vcs\tdo not use .gitignore files\n  --no-require-git\tuse .gitignore outside git repositories\n  --require-git\trequire a git repository for .gitignore files\n  -u, --unrestricted\treduce filtering (repeatable)\n  --messages\tshow file read diagnostics\n  --no-messages\tsuppress file read diagnostics\n  --hidden\tsearch hidden files and directories\n  --no-hidden\tdo not search hidden files and directories\n  -H, --with-filename\tshow filename\n  -I, --no-filename\tsuppress filename\n  --line-buffered\tforce line buffering (no-op)\n  --block-buffered\tforce block buffering (no-op)\n  --no-config\tdo not read config files (no-op)\n  --mmap\tsearch using memory maps when possible (no-op)\n  --no-mmap\tdisable memory maps (no-op)\n  -P, --pcre2\tuse PCRE2 regex engine for supported patterns (no-op)\n  --no-pcre2\tdisable PCRE2 regex engine (no-op)\n  --engine ENGINE\tselect regex engine: default, auto, pcre2 (no-op)\n  --auto-hybrid-regex\tuse PCRE2 when needed (no-op)\n  --no-auto-hybrid-regex\tdisable auto hybrid regex (no-op)\n  --color MODE\tcolor output (no-op)\n  -h, --help\tdisplay this help and exit\n  -V, --version\toutput version information and exit\n";
        if ctx.args.iter().any(|arg| arg == "-h") {
            return Ok(ExecResult::ok(help_text.to_string()));
        }
        if ctx.args.iter().any(|arg| arg == "-V") {
            return Ok(ExecResult::ok("rg (bashkit) 0.1\n".to_string()));
        }
        if let Some(r) = super::check_help_version(ctx.args, help_text, Some("rg (bashkit) 0.1")) {
            return Ok(r);
        }
        let mut opts = RgOptions::parse(ctx.args)?;
        if opts.type_list {
            return Ok(ExecResult::ok(opts.type_database.list()));
        }
        if let Err(result) = load_rg_ignore_files(&*ctx.fs, ctx.cwd, &mut opts).await {
            return Ok(result);
        }
        if opts.list_files {
            let files = collect_rg_file_list(&*ctx.fs, &opts, ctx.cwd).await;
            let output = if opts.null {
                let mut output = String::new();
                for file in files {
                    output.push_str(&file);
                    output.push('\0');
                }
                output
            } else if files.is_empty() {
                String::new()
            } else {
                format!("{}\n", files.join("\n"))
            };
            return Ok(ExecResult::ok(output));
        }
        if let Err(result) = load_rg_pattern_files(&*ctx.fs, ctx.cwd, ctx.stdin, &mut opts).await {
            return Ok(result);
        }
        if opts.patterns.is_empty() {
            return Ok(ExecResult::with_code(String::new(), 1));
        }
        let regex = opts.build_regex()?;
        let stdin_input =
            opts.paths.is_empty() && ctx.stdin.is_some() && !opts.stdin_consumed_for_patterns;
        let recursive_output = !stdin_input
            && (opts.paths.is_empty() || has_directory_path(&*ctx.fs, ctx.cwd, &opts.paths).await);

        let collected_inputs = match collect_rg_inputs(ctx, &opts).await {
            Ok(inputs) => inputs,
            Err(result) => return Ok(result),
        };
        let inputs = collected_inputs.inputs;

        let show_filename = if opts.no_filename {
            false
        } else if opts.show_filename {
            true
        } else {
            recursive_output || inputs.len() > 1 || opts.paths.len() > 1
        };
        let has_context = opts.before_context > 0 || opts.after_context > 0;
        let json_output = opts.json
            && !opts.count_only
            && !opts.count_matches
            && !opts.files_with_matches
            && !opts.files_without_matches;

        let mut output = String::new();
        let mut any_match = false;
        let mut json_bytes_searched = 0usize;
        let mut json_matched_lines = 0usize;
        let mut json_matches = 0usize;
        let mut json_searches = 0usize;
        let mut stats = RgSearchStats::default();

        for (filename, content) in &inputs {
            let mut match_count = 0usize;
            let mut count_value = 0usize;
            let mut match_lines = Vec::new();
            stats.files_searched += 1;
            if let Some(nul_offset) = first_nul_offset(content)
                && !opts.text
            {
                if !opts.binary {
                    continue;
                }
                stats.bytes_searched += content.len();

                let matched = regex.is_match(content);
                let matched = if opts.invert_match { !matched } else { matched };
                if !matched {
                    if opts.files_without_matches {
                        any_match = true;
                        output.push_str(filename);
                        output.push(if opts.null { '\0' } else { '\n' });
                    } else if (opts.count_only || opts.count_matches) && opts.include_zero {
                        if show_filename {
                            output.push_str(filename);
                            output.push(if opts.null { '\0' } else { ':' });
                        }
                        output.push_str("0\n");
                    }
                    continue;
                }

                any_match = true;
                stats.matches += 1;
                stats.matched_lines += 1;
                stats.files_with_matches += 1;
                if opts.quiet {
                    if !opts.stats {
                        return Ok(ExecResult::ok(String::new()));
                    }
                    continue;
                }
                if opts.files_without_matches {
                    continue;
                }
                if opts.files_with_matches {
                    output.push_str(filename);
                    output.push(if opts.null { '\0' } else { '\n' });
                    continue;
                }
                if opts.count_only || opts.count_matches {
                    if show_filename {
                        output.push_str(filename);
                        output.push(if opts.null { '\0' } else { ':' });
                    }
                    output.push_str("1\n");
                    continue;
                }

                if show_filename {
                    output.push_str(filename);
                    output.push(if opts.null { '\0' } else { ':' });
                    if !opts.null {
                        output.push(' ');
                    }
                }
                output.push_str(&format!(
                    "binary file matches (found \"\\0\" byte around offset {})\n",
                    nul_offset
                ));
                continue;
            }
            let lines = split_rg_lines(content);
            json_bytes_searched += content.len();
            json_searches += 1;
            stats.bytes_searched += content.len();

            for (line_idx, line) in lines.iter().enumerate() {
                let matched = regex.is_match(line.text);
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
                let matches_on_line = if !opts.invert_match {
                    regex.find_iter(line.text).count()
                } else {
                    1
                };
                if opts.count_matches && !opts.invert_match {
                    count_value += matches_on_line;
                } else {
                    count_value += 1;
                }
                json_matches += matches_on_line;
                stats.matches += matches_on_line;
                stats.matched_lines += 1;
                match_lines.push(line_idx);
                if !opts.files_without_matches {
                    any_match = true;
                }

                if (opts.files_with_matches || opts.files_without_matches || opts.quiet)
                    && !opts.stats
                {
                    break;
                }
            }

            if match_count > 0 {
                stats.files_with_matches += 1;
            }

            if opts.quiet && match_count > 0 && !opts.stats {
                return Ok(ExecResult::ok(String::new()));
            }
            if opts.files_with_matches && match_count > 0 {
                output.push_str(filename);
                output.push(if opts.null { '\0' } else { '\n' });
                continue;
            }
            if opts.files_without_matches {
                if match_count == 0 {
                    any_match = true;
                    output.push_str(filename);
                    output.push(if opts.null { '\0' } else { '\n' });
                }
                continue;
            }
            if json_output {
                if match_count > 0 {
                    write_rg_json_begin(&mut output, filename);
                    for &line_idx in &match_lines {
                        write_rg_json_match(
                            &mut output,
                            filename,
                            lines[line_idx],
                            line_idx,
                            &regex,
                        );
                    }
                    write_rg_json_end(
                        &mut output,
                        filename,
                        content.len(),
                        match_lines.len(),
                        match_lines
                            .iter()
                            .map(|&line_idx| regex.find_iter(lines[line_idx].text).count())
                            .sum(),
                    );
                    json_matched_lines += match_lines.len();
                }
                continue;
            }
            if opts.count_only || opts.count_matches {
                if count_value == 0 && !opts.include_zero {
                    continue;
                }
                if show_filename {
                    output.push_str(filename);
                    output.push(if opts.null { '\0' } else { ':' });
                }
                output.push_str(&count_value.to_string());
                output.push('\n');
                continue;
            }
            if opts.quiet {
                continue;
            }

            let line_show_filename = if opts.heading && show_filename && match_count > 0 {
                if !output.is_empty() {
                    output.push('\n');
                }
                output.push_str(filename);
                output.push('\n');
                false
            } else {
                show_filename
            };
            if opts.passthru {
                let match_set: HashSet<usize> = match_lines.iter().copied().collect();
                for (line_idx, line) in lines.iter().enumerate() {
                    let matched = match_set.contains(&line_idx);
                    let separator = if matched {
                        opts.field_match_separator.as_str()
                    } else {
                        opts.field_context_separator.as_str()
                    };
                    write_rg_prefix(
                        &mut output,
                        RgPrefix {
                            filename,
                            show_filename: line_show_filename,
                            line_numbers: opts.line_numbers,
                            line_idx,
                            column: if opts.column && matched && !opts.invert_match {
                                regex.find(line.text).map(|mat| mat.start() + 1)
                            } else {
                                None
                            },
                            byte_offset: if opts.byte_offset {
                                Some(if matched && opts.only_matching && !opts.invert_match {
                                    regex
                                        .find(line.text)
                                        .map(|mat| line.start_offset + mat.start())
                                        .unwrap_or(line.start_offset)
                                } else {
                                    line.start_offset
                                })
                            } else {
                                None
                            },
                            separator,
                            null_path_separator: opts.null,
                        },
                    );
                    output.push_str(&format_rg_output_line(line.text, &regex, &opts, matched));
                    output.push('\n');
                }
            } else if opts.vimgrep && !opts.invert_match {
                for &line_idx in &match_lines {
                    for mat in regex.find_iter(lines[line_idx].text) {
                        write_rg_prefix(
                            &mut output,
                            RgPrefix {
                                filename,
                                show_filename: true,
                                line_numbers: true,
                                line_idx,
                                column: Some(mat.start() + 1),
                                byte_offset: None,
                                separator: opts.field_match_separator.as_str(),
                                null_path_separator: opts.null,
                            },
                        );
                        if opts.only_matching {
                            output.push_str(mat.as_str());
                        } else {
                            output.push_str(&format_rg_output_line(
                                lines[line_idx].text,
                                &regex,
                                &opts,
                                true,
                            ));
                        }
                        output.push('\n');
                    }
                }
            } else if opts.only_matching && !opts.invert_match {
                for &line_idx in &match_lines {
                    for mat in regex.find_iter(lines[line_idx].text) {
                        write_rg_prefix(
                            &mut output,
                            RgPrefix {
                                filename,
                                show_filename: line_show_filename,
                                line_numbers: opts.line_numbers,
                                line_idx,
                                column: if opts.column {
                                    Some(mat.start() + 1)
                                } else {
                                    None
                                },
                                byte_offset: if opts.byte_offset {
                                    Some(lines[line_idx].start_offset + mat.start())
                                } else {
                                    None
                                },
                                separator: opts.field_match_separator.as_str(),
                                null_path_separator: opts.null,
                            },
                        );
                        if let Some(replacement) = &opts.replacement {
                            output.push_str(&regex.replace(mat.as_str(), replacement.as_str()));
                        } else {
                            output.push_str(mat.as_str());
                        }
                        output.push('\n');
                    }
                }
            } else if has_context {
                if !opts.heading && !output.is_empty() && !match_lines.is_empty() {
                    output.push_str(&opts.context_separator);
                    output.push('\n');
                }
                write_rg_context(
                    &mut output,
                    filename,
                    &regex,
                    &lines,
                    &match_lines,
                    &opts,
                    line_show_filename,
                );
            } else {
                for &line_idx in &match_lines {
                    write_rg_prefix(
                        &mut output,
                        RgPrefix {
                            filename,
                            show_filename: line_show_filename,
                            line_numbers: opts.line_numbers,
                            line_idx,
                            column: if opts.column && !opts.invert_match {
                                regex.find(lines[line_idx].text).map(|mat| mat.start() + 1)
                            } else {
                                None
                            },
                            byte_offset: if opts.byte_offset {
                                Some(lines[line_idx].start_offset)
                            } else {
                                None
                            },
                            separator: opts.field_match_separator.as_str(),
                            null_path_separator: opts.null,
                        },
                    );
                    output.push_str(&format_rg_output_line(
                        lines[line_idx].text,
                        &regex,
                        &opts,
                        true,
                    ));
                    output.push('\n');
                }
            }
        }

        if json_output {
            write_rg_json_summary(
                &mut output,
                json_bytes_searched,
                json_matched_lines,
                json_matches,
                json_searches,
            );
        } else if opts.stats {
            let bytes_printed = if opts.count_only
                || opts.count_matches
                || opts.files_with_matches
                || opts.files_without_matches
                || opts.quiet
            {
                0
            } else {
                output.len()
            };
            append_rg_stats(&mut output, &stats, bytes_printed);
        }

        let exit_code = if collected_inputs.had_errors {
            2
        } else if any_match {
            0
        } else {
            1
        };
        Ok(ExecResult {
            stdout: output,
            stderr: collected_inputs.stderr,
            exit_code,
            ..Default::default()
        })
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
        UnorderedNul,
        JsonEvents,
        Stats,
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
        ("/proj/patterns.txt", b"needle\nHello\n"),
        ("/proj/trim.txt", b"  needle one  \n  none  \n"),
        ("/proj/offset.txt", b"abc needle\nxx needle yy\n"),
        ("/proj/.hidden.txt", b"needle\n"),
        ("/proj/.hidden/secret.txt", b"needle\n"),
        ("/proj/lang/lib.rs", b"needle\n"),
        ("/proj/lang/lib.py", b"needle\n"),
        ("/proj/lang/readme.md", b"needle\n"),
        ("/proj/lang/custom.foo", b"needle\n"),
    ];

    const DIFF_TWO_CONTEXT_FILES: &[(&str, &[u8])] = &[
        ("/proj/a.txt", b"before\nneedle\nafter\n"),
        ("/proj/b.txt", b"x\nneedle\ny\n"),
    ];

    const DIFF_SORT_FILES: &[(&str, &[u8])] =
        &[("/proj/a.txt", b"needle\n"), ("/proj/b.txt", b"needle\n")];

    const DIFF_IGNORE_FILES: &[(&str, &[u8])] = &[
        ("/proj/.git/config", b"[core]\n"),
        (
            "/proj/.gitignore",
            b"target/\n*.log\n!keep.log\nvendor/**\n",
        ),
        ("/proj/.ignore", b"src/ignored.txt\n"),
        ("/proj/custom.ignore", b"*.tmp\n"),
        ("/proj/a.txt", b"needle\n"),
        ("/proj/a.log", b"needle\n"),
        ("/proj/keep.log", b"needle\n"),
        ("/proj/target/out.txt", b"needle\n"),
        ("/proj/src/ignored.txt", b"needle\n"),
        ("/proj/vendor/lib.rs", b"needle\n"),
        ("/proj/scratch.tmp", b"needle\n"),
    ];

    const DIFF_BINARY_FILES: &[(&str, &[u8])] = &[
        ("/proj/bin.dat", b"abc\0needle\n"),
        ("/proj/text.txt", b"needle\n"),
    ];

    const DIFF_ENCODING_FILES: &[(&str, &[u8])] = &[
        ("/proj/utf16le.txt", b"n\0e\0e\0d\0l\0e\0\n\0"),
        ("/proj/utf16bom.txt", b"\xff\xfen\0e\0e\0d\0l\0e\0\n\0"),
    ];

    const DIFF_UNRESTRICTED_FILES: &[(&str, &[u8])] = &[
        ("/proj/.git/config", b"[core]\n"),
        ("/proj/.gitignore", b"target/\n"),
        ("/proj/plain.txt", b"needle\n"),
        ("/proj/.hidden.txt", b"needle\n"),
        ("/proj/target/out.txt", b"needle\n"),
        ("/proj/bin.dat", b"abc\0needle\n"),
    ];

    const DIFF_REQUIRE_GIT_FILES: &[(&str, &[u8])] = &[
        ("/proj/.gitignore", b"target/\n"),
        ("/proj/plain.txt", b"needle\n"),
        ("/proj/target/out.txt", b"needle\n"),
    ];

    const DIFF_MAX_COLUMNS_FILES: &[(&str, &[u8])] = &[(
        "/proj/long.txt",
        b"short needle\ncontext line is long\n0123456789 needle\nnomatch\n",
    )];

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
            output: RgDiffOutput::UnorderedNul,
        },
        RgDiffCase {
            name: "dot root glob excludes cwd-relative path",
            args: &["-g", "*.rs", "-g", "!vendor/**", "needle", "."],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/proj",
            output: RgDiffOutput::UnorderedLines,
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
        RgDiffCase {
            name: "pattern file",
            args: &["-f", "proj/patterns.txt", "proj/a.txt", "proj/case.txt"],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "smart case lowercase",
            args: &["-S", "hello", "proj/case.txt"],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "smart case uppercase",
            args: &["-S", "Hello", "proj/case.txt"],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "line regexp",
            args: &["-x", "needle", "proj/a.txt"],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "count matches",
            args: &["--count-matches", "needle", "proj/a.txt"],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "column",
            args: &["--column", "needle", "proj/a.txt"],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "max depth",
            args: &["--max-depth", "1", "needle", "proj"],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::UnorderedLines,
        },
        RgDiffCase {
            name: "files list max depth",
            args: &["--files", "--max-depth", "1", "proj"],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::UnorderedLines,
        },
        RgDiffCase {
            name: "replace",
            args: &["--replace", "X", "needle", "proj/a.txt"],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "replace only matching",
            args: &["-o", "-r", "X", "needle", "proj/a.txt"],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "passthru",
            args: &["--passthru", "needle", "proj/a.txt", "proj/b.txt"],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "trim",
            args: &["--trim", "needle", "proj/trim.txt"],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "include zero count",
            args: &["-c", "--include-zero", "needle", "proj/a.txt", "proj/b.txt"],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "heading",
            args: &["--heading", "needle", "proj/a.txt", "proj/src/main.rs"],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "context separator",
            args: &[
                "-n",
                "-C1",
                "--context-separator=@@",
                "needle",
                "proj/a.txt",
                "proj/b.txt",
            ],
            stdin: None,
            files: DIFF_TWO_CONTEXT_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "field match separator",
            args: &[
                "-n",
                "--field-match-separator=|",
                "needle",
                "proj/a.txt",
                "proj/src/main.rs",
            ],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "field context separator",
            args: &[
                "-n",
                "-C1",
                "--field-context-separator=~",
                "needle",
                "proj/context.txt",
            ],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "reverse sort path",
            args: &["--sortr", "path", "needle", "proj"],
            stdin: None,
            files: DIFF_SORT_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "path separator explicit file",
            args: &["-H", "--path-separator", "_", "needle", "proj/src/main.rs"],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "path separator dot relative file",
            args: &["-H", "--path-separator=@", "needle", "./proj/src/main.rs"],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "path separator files list",
            args: &["--files", "--path-separator", "_", "proj/src"],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::UnorderedLines,
        },
        RgDiffCase {
            name: "byte offset",
            args: &["-n", "--column", "-b", "needle", "proj/offset.txt"],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "only matching byte offset",
            args: &["-n", "-o", "-b", "needle", "proj/offset.txt"],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "vimgrep",
            args: &["--vimgrep", "needle", "proj/offset.txt"],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "null files with matches",
            args: &["--null", "-l", "needle", "proj/a.txt", "proj/src/main.rs"],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "null files list",
            args: &["--null", "--files", "proj/src"],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::UnorderedNul,
        },
        RgDiffCase {
            name: "no heading wins",
            args: &[
                "--heading",
                "--no-heading",
                "needle",
                "proj/a.txt",
                "proj/src/main.rs",
            ],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "json basic",
            args: &["--json", "needle", "proj/offset.txt"],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::JsonEvents,
        },
        RgDiffCase {
            name: "stats explicit file",
            args: &["--stats", "needle", "proj/a.txt"],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::Stats,
        },
        RgDiffCase {
            name: "stats no match",
            args: &["--stats", "missing", "proj/a.txt"],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::Stats,
        },
        RgDiffCase {
            name: "stats quiet scans all matches",
            args: &["--stats", "-q", "needle", "proj/a.txt", "proj/src/main.rs"],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::Stats,
        },
        RgDiffCase {
            name: "hidden recursive",
            args: &["--hidden", "needle", "proj"],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::UnorderedLines,
        },
        RgDiffCase {
            name: "no hidden wins",
            args: &["--hidden", "--no-hidden", "needle", "proj"],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::UnorderedLines,
        },
        RgDiffCase {
            name: "type rust",
            args: &["-t", "rust", "needle", "proj/lang"],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type not rust",
            args: &["-T", "rust", "needle", "proj/lang"],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::UnorderedLines,
        },
        RgDiffCase {
            name: "long type python",
            args: &["--type=python", "needle", "proj/lang"],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type add custom",
            args: &[
                "--type-add",
                "foo:*.foo",
                "-t",
                "foo",
                "needle",
                "proj/lang",
            ],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type clear then redefine",
            args: &[
                "--type-clear",
                "rust",
                "--type-add",
                "rust:*.foo",
                "-t",
                "rust",
                "needle",
                "proj/lang",
            ],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "ignore files default",
            args: &["needle", "proj"],
            stdin: None,
            files: DIFF_IGNORE_FILES,
            cwd: "/",
            output: RgDiffOutput::UnorderedLines,
        },
        RgDiffCase {
            name: "no ignore disables auto ignore files",
            args: &["--no-ignore", "needle", "proj"],
            stdin: None,
            files: DIFF_IGNORE_FILES,
            cwd: "/",
            output: RgDiffOutput::UnorderedLines,
        },
        RgDiffCase {
            name: "no ignore vcs keeps dot ignore",
            args: &["--no-ignore-vcs", "needle", "proj"],
            stdin: None,
            files: DIFF_IGNORE_FILES,
            cwd: "/",
            output: RgDiffOutput::UnorderedLines,
        },
        RgDiffCase {
            name: "no ignore dot keeps vcs ignore",
            args: &["--no-ignore-dot", "needle", "proj"],
            stdin: None,
            files: DIFF_IGNORE_FILES,
            cwd: "/",
            output: RgDiffOutput::UnorderedLines,
        },
        RgDiffCase {
            name: "explicit ignore file",
            args: &["--ignore-file", "proj/custom.ignore", "needle", "proj"],
            stdin: None,
            files: DIFF_IGNORE_FILES,
            cwd: "/",
            output: RgDiffOutput::UnorderedLines,
        },
        RgDiffCase {
            name: "explicit file bypasses ignore",
            args: &["needle", "proj/a.log"],
            stdin: None,
            files: DIFF_IGNORE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "binary skipped by default",
            args: &["needle", "proj"],
            stdin: None,
            files: DIFF_BINARY_FILES,
            cwd: "/",
            output: RgDiffOutput::UnorderedLines,
        },
        RgDiffCase {
            name: "binary as text",
            args: &["-a", "needle", "proj/bin.dat"],
            stdin: None,
            files: DIFF_BINARY_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "binary match summary",
            args: &["--binary", "needle", "proj"],
            stdin: None,
            files: DIFF_BINARY_FILES,
            cwd: "/",
            output: RgDiffOutput::UnorderedLines,
        },
        RgDiffCase {
            name: "encoding utf16le long",
            args: &["--encoding=utf-16le", "needle", "proj/utf16le.txt"],
            stdin: None,
            files: DIFF_ENCODING_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "encoding utf16le short",
            args: &["-E", "utf-16le", "needle", "proj/utf16le.txt"],
            stdin: None,
            files: DIFF_ENCODING_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "encoding auto sniffs utf16 bom",
            args: &["needle", "proj/utf16bom.txt"],
            stdin: None,
            files: DIFF_ENCODING_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "encoding none disables bom sniffing",
            args: &["--encoding=none", "needle", "proj/utf16bom.txt"],
            stdin: None,
            files: DIFF_ENCODING_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "unrestricted disables ignore files",
            args: &["-u", "needle", "proj"],
            stdin: None,
            files: DIFF_UNRESTRICTED_FILES,
            cwd: "/",
            output: RgDiffOutput::UnorderedLines,
        },
        RgDiffCase {
            name: "unrestricted twice includes hidden",
            args: &["-uu", "needle", "proj"],
            stdin: None,
            files: DIFF_UNRESTRICTED_FILES,
            cwd: "/",
            output: RgDiffOutput::UnorderedLines,
        },
        RgDiffCase {
            name: "unrestricted three times includes binary",
            args: &["-uuu", "needle", "proj"],
            stdin: None,
            files: DIFF_UNRESTRICTED_FILES,
            cwd: "/",
            output: RgDiffOutput::UnorderedLines,
        },
        RgDiffCase {
            name: "long unrestricted is repeatable",
            args: &["--unrestricted", "--unrestricted", "needle", "proj"],
            stdin: None,
            files: DIFF_UNRESTRICTED_FILES,
            cwd: "/",
            output: RgDiffOutput::UnorderedLines,
        },
        RgDiffCase {
            name: "gitignore requires git repo by default",
            args: &["needle", "proj"],
            stdin: None,
            files: DIFF_REQUIRE_GIT_FILES,
            cwd: "/",
            output: RgDiffOutput::UnorderedLines,
        },
        RgDiffCase {
            name: "no require git uses gitignore outside repo",
            args: &["--no-require-git", "needle", "proj"],
            stdin: None,
            files: DIFF_REQUIRE_GIT_FILES,
            cwd: "/",
            output: RgDiffOutput::UnorderedLines,
        },
        RgDiffCase {
            name: "require git restores default after no require git",
            args: &["--no-require-git", "--require-git", "needle", "proj"],
            stdin: None,
            files: DIFF_REQUIRE_GIT_FILES,
            cwd: "/",
            output: RgDiffOutput::UnorderedLines,
        },
        RgDiffCase {
            name: "no config is accepted",
            args: &["--no-config", "needle", "proj/a.txt"],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "line buffered is accepted",
            args: &["--line-buffered", "needle", "proj/a.txt"],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "block buffered is accepted",
            args: &[
                "--line-buffered",
                "--block-buffered",
                "needle",
                "proj/a.txt",
            ],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "mmap is accepted",
            args: &["--mmap", "needle", "proj/a.txt"],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "no mmap is accepted",
            args: &["--no-mmap", "needle", "proj/a.txt"],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "pcre2 short is accepted",
            args: &["-P", "needle", "proj/a.txt"],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "pcre2 long is accepted",
            args: &["--pcre2", "needle", "proj/a.txt"],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "no pcre2 is accepted",
            args: &["--no-pcre2", "needle", "proj/a.txt"],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "engine default is accepted",
            args: &["--engine=default", "needle", "proj/a.txt"],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "engine auto is accepted",
            args: &["--engine=auto", "needle", "proj/a.txt"],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "engine pcre2 is accepted",
            args: &["--engine=pcre2", "needle", "proj/a.txt"],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "auto hybrid regex is accepted",
            args: &["--auto-hybrid-regex", "needle", "proj/a.txt"],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "no auto hybrid regex is accepted",
            args: &["--no-auto-hybrid-regex", "needle", "proj/a.txt"],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "max columns omits long matches",
            args: &["--max-columns", "10", "needle", "proj/long.txt"],
            stdin: None,
            files: DIFF_MAX_COLUMNS_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "max columns preview",
            args: &[
                "--max-columns",
                "10",
                "--max-columns-preview",
                "needle",
                "proj/long.txt",
            ],
            stdin: None,
            files: DIFF_MAX_COLUMNS_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "max columns context",
            args: &["-C1", "--max-columns", "10", "needle", "proj/long.txt"],
            stdin: None,
            files: DIFF_MAX_COLUMNS_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "max columns passthru",
            args: &["--passthru", "-M10", "needle", "proj/long.txt"],
            stdin: None,
            files: DIFF_MAX_COLUMNS_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "missing file keeps stdout and exits 2",
            args: &["needle", "proj/a.txt", "proj/missing.txt"],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "no messages missing file keeps stdout and exits 2",
            args: &["--no-messages", "needle", "proj/a.txt", "proj/missing.txt"],
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
            RgDiffOutput::UnorderedNul => assert_eq!(
                sorted_nul_items(&bashkit.stdout),
                sorted_nul_items(&real_stdout),
                "stdout NUL-item mismatch for rg differential case {}",
                case.name
            ),
            RgDiffOutput::JsonEvents => assert_eq!(
                normalize_rg_json(&bashkit.stdout),
                normalize_rg_json(&real_stdout),
                "stdout JSON-event mismatch for rg differential case {}",
                case.name
            ),
            RgDiffOutput::Stats => assert_eq!(
                normalize_rg_stats(&bashkit.stdout),
                normalize_rg_stats(&real_stdout),
                "stdout stats mismatch for rg differential case {}",
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

    fn sorted_nul_items(output: &str) -> Vec<&str> {
        let mut items: Vec<&str> = output.split('\0').filter(|item| !item.is_empty()).collect();
        items.sort_unstable();
        items
    }

    fn normalize_rg_json(output: &str) -> Vec<serde_json::Value> {
        output
            .lines()
            .map(|line| {
                let mut value: serde_json::Value =
                    serde_json::from_str(line).expect("valid rg JSON line");
                normalize_rg_json_value(&mut value);
                value
            })
            .collect()
    }

    fn normalize_rg_json_value(value: &mut serde_json::Value) {
        let Some(obj) = value.as_object_mut() else {
            return;
        };
        if obj.get("type").and_then(|t| t.as_str()) == Some("summary")
            && let Some(data) = obj.get_mut("data").and_then(|data| data.as_object_mut())
        {
            data.remove("elapsed_total");
        }
        if let Some(stats) = obj
            .get_mut("data")
            .and_then(|data| data.as_object_mut())
            .and_then(|data| data.get_mut("stats"))
            .and_then(|stats| stats.as_object_mut())
        {
            stats.remove("elapsed");
            stats.remove("bytes_printed");
        }
    }

    fn normalize_rg_stats(output: &str) -> Vec<&str> {
        output
            .lines()
            .filter(|line| {
                !line.contains("seconds spent searching") && !line.contains("seconds total")
            })
            .collect()
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
        assert!(long_help.stdout.contains("--stats"));
        assert!(long_help.stdout.contains("--unrestricted"));
        assert!(long_help.stdout.contains("--no-require-git"));
        assert!(long_help.stdout.contains("--no-config"));
        assert!(long_help.stdout.contains("--path-separator"));
        assert!(long_help.stdout.contains("--engine"));
        assert!(long_help.stdout.contains("--mmap"));
        assert!(long_help.stdout.contains("--pcre2"));
        assert!(long_help.stdout.contains("--encoding"));

        let short_help = run_rg(&["-h"], None, &[]).await;
        assert_eq!(short_help.exit_code, 0);
        assert_eq!(short_help.stdout, long_help.stdout);

        let version = run_rg(&["--version"], None, &[]).await;
        assert_eq!(version.exit_code, 0);
        assert_eq!(version.stdout, "rg (bashkit) 0.1\n");

        let short_version = run_rg(&["-V"], None, &[]).await;
        assert_eq!(short_version.exit_code, 0);
        assert_eq!(short_version.stdout, version.stdout);
    }

    #[tokio::test]
    async fn test_rg_type_list_and_custom_types() {
        let type_list = run_rg(&["--type-add", "foo:*.foo", "--type-list"], None, &[]).await;
        assert_eq!(type_list.exit_code, 0);
        assert!(type_list.stdout.contains("foo: *.foo\n"));
        assert!(type_list.stdout.contains("rust: *.rs\n"));

        let cleared = run_rg(&["--type-clear", "rust", "--type-list"], None, &[]).await;
        assert_eq!(cleared.exit_code, 0);
        assert!(!cleared.stdout.contains("rust: *.rs\n"));

        let invalid = RgOptions::parse(&["--type-add".to_string(), "foo".to_string()]);
        assert!(invalid.is_err());

        let invalid_separator = RgOptions::parse(&[
            "--path-separator".to_string(),
            "xy".to_string(),
            "needle".to_string(),
        ]);
        assert!(invalid_separator.is_err());

        let invalid_engine = RgOptions::parse(&[
            "--engine=bad".to_string(),
            "needle".to_string(),
            "a.txt".to_string(),
        ]);
        assert!(invalid_engine.is_err());

        let invalid_encoding = RgOptions::parse(&[
            "--encoding=unknown".to_string(),
            "needle".to_string(),
            "a.txt".to_string(),
        ]);
        assert!(invalid_encoding.is_err());
    }

    #[tokio::test]
    async fn test_rg_ignore_files_and_disable_flags() {
        let files: &[(&str, &[u8])] = &[
            ("/proj/.git/config", b"[core]\n"),
            ("/proj/.gitignore", b"target/\n*.log\n!keep.log\n"),
            ("/proj/.ignore", b"src/ignored.txt\n"),
            ("/proj/a.txt", b"needle\n"),
            ("/proj/a.log", b"needle\n"),
            ("/proj/keep.log", b"needle\n"),
            ("/proj/target/out.txt", b"needle\n"),
            ("/proj/src/ignored.txt", b"needle\n"),
        ];

        let default = run_rg(&["needle", "/proj"], None, files).await;
        assert_eq!(default.exit_code, 0);
        assert!(default.stdout.contains("a.txt"));
        assert!(default.stdout.contains("keep.log"));
        assert!(!default.stdout.contains("a.log"));
        assert!(!default.stdout.contains("target/out.txt"));
        assert!(!default.stdout.contains("src/ignored.txt"));

        let no_ignore = run_rg(&["--no-ignore", "needle", "/proj"], None, files).await;
        assert_eq!(no_ignore.exit_code, 0);
        assert!(no_ignore.stdout.contains("a.log"));
        assert!(no_ignore.stdout.contains("target/out.txt"));
        assert!(no_ignore.stdout.contains("src/ignored.txt"));

        let no_vcs = run_rg(&["--no-ignore-vcs", "needle", "/proj"], None, files).await;
        assert_eq!(no_vcs.exit_code, 0);
        assert!(no_vcs.stdout.contains("a.log"));
        assert!(!no_vcs.stdout.contains("src/ignored.txt"));
    }

    #[tokio::test]
    async fn test_rg_binary_text_modes() {
        let files: &[(&str, &[u8])] = &[
            ("/proj/bin.dat", b"abc\0needle\n"),
            ("/proj/text.txt", b"needle\n"),
        ];

        let default = run_rg(&["needle", "/proj"], None, files).await;
        assert_eq!(default.exit_code, 0);
        assert!(default.stdout.contains("text.txt"));
        assert!(!default.stdout.contains("bin.dat"));

        let text = run_rg(&["--text", "needle", "/proj/bin.dat"], None, files).await;
        assert_eq!(text.exit_code, 0);
        assert_eq!(text.stdout, "abc\0needle\n");

        let binary = run_rg(&["--binary", "needle", "/proj/bin.dat"], None, files).await;
        assert_eq!(binary.exit_code, 0);
        assert_eq!(
            binary.stdout,
            "binary file matches (found \"\\0\" byte around offset 3)\n"
        );
    }

    #[tokio::test]
    async fn test_rg_max_columns_modes() {
        let files: &[(&str, &[u8])] = &[(
            "/proj/long.txt",
            b"short needle\ncontext line is long\n0123456789 needle\nnomatch\n",
        )];

        let omitted = run_rg(
            &["--max-columns", "10", "needle", "/proj/long.txt"],
            None,
            files,
        )
        .await;
        assert_eq!(omitted.exit_code, 0);
        assert_eq!(
            omitted.stdout,
            "[Omitted long matching line]\n[Omitted long matching line]\n"
        );

        let preview = run_rg(
            &["-M10", "--max-columns-preview", "needle", "/proj/long.txt"],
            None,
            files,
        )
        .await;
        assert_eq!(preview.exit_code, 0);
        assert_eq!(
            preview.stdout,
            "short need [... omitted end of long line]\n0123456789 [... omitted end of long line]\n"
        );

        let disabled = run_rg(&["-M", "0", "needle", "/proj/long.txt"], None, files).await;
        assert_eq!(disabled.exit_code, 0);
        assert_eq!(disabled.stdout, "short needle\n0123456789 needle\n");
    }

    #[tokio::test]
    async fn test_rg_max_columns_requires_value() {
        let args: Vec<String> = vec!["needle".to_string(), "-M".to_string()];
        let result = RgOptions::parse(&args);
        assert!(matches!(
            result,
            Err(Error::Execution(msg)) if msg == "rg: -M requires an argument"
        ));
    }

    #[tokio::test]
    async fn test_rg_unknown_option_errors() {
        let fs = Arc::new(InMemoryFs::new()) as Arc<dyn FileSystem>;
        let args = vec!["--definitely-not-rg".to_string()];
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
        assert_eq!(result.exit_code, 2);
        assert!(result.stderr.contains("rg:"));
    }

    #[tokio::test]
    async fn test_rg_messages_flags() {
        let files: &[(&str, &[u8])] = &[("/a.txt", b"needle\n")];

        let default = run_rg(&["needle", "/a.txt", "/missing.txt"], None, files).await;
        assert_eq!(default.exit_code, 2);
        assert_eq!(default.stdout, "/a.txt:needle\n");
        assert!(default.stderr.contains("rg: /missing.txt:"));

        let suppressed = run_rg(
            &["--no-messages", "needle", "/a.txt", "/missing.txt"],
            None,
            files,
        )
        .await;
        assert_eq!(suppressed.exit_code, 2);
        assert_eq!(suppressed.stdout, "/a.txt:needle\n");
        assert!(suppressed.stderr.is_empty());

        let reenabled = run_rg(
            &[
                "--no-messages",
                "--messages",
                "needle",
                "/a.txt",
                "/missing.txt",
            ],
            None,
            files,
        )
        .await;
        assert_eq!(reenabled.exit_code, 2);
        assert!(reenabled.stderr.contains("rg: /missing.txt:"));
    }

    #[tokio::test]
    async fn test_rg_stats() {
        let result = run_rg(
            &["--stats", "needle", "/a.txt"],
            None,
            &[("/a.txt", b"needle\nnone\nneedle again\n")],
        )
        .await;

        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.starts_with("needle\nneedle again\n\n"));
        assert!(result.stdout.contains("2 matches\n"));
        assert!(result.stdout.contains("2 matched lines\n"));
        assert!(result.stdout.contains("1 files contained matches\n"));
        assert!(result.stdout.contains("1 files searched\n"));
        assert!(result.stdout.contains("20 bytes printed\n"));
        assert!(result.stdout.contains("25 bytes searched\n"));
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

        let result = run_rg_with_fs(&["--no-ignore", "secret", "/safe"], None, fs).await;
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
