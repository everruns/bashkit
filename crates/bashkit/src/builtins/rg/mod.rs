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
//!   rg --color never PATTERN    # disable ANSI color output

use async_trait::async_trait;
use fancy_regex::RegexBuilder as FancyRegexBuilder;
use regex::{Regex, RegexBuilder};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::OnceLock;

use super::search_common::build_regex_opts;
use super::{Builtin, Context, read_text_file, resolve_path};
use crate::error::{Error, Result};
use crate::interpreter::ExecResult;

// Ignore files are repository input. Keep parsing/matching bounded so a large
// ignore file cannot force unbounded regex compilation or per-path scans.
const RG_IGNORE_FILE_MAX_BYTES: usize = 1024 * 1024;
const RG_IGNORE_RULES_MAX_PER_FILE: usize = 10_000;
const RG_IGNORE_RULES_MAX_TOTAL: usize = 50_000;
/// rg command - recursive pattern search (simplified ripgrep)
pub struct Rg;

/// Upper bound on replacement output produced per call to
/// [`RgMatcher::replace_all`] / [`RgMatcher::replace_first`]. Guards the
/// `--replace` path against memory amplification from attacker-controlled
/// replacement text combined with many matches (TM-DOS-RG-REPLACE).
const RG_MAX_REPLACEMENT_OUTPUT_BYTES: usize = 1_048_576;
const RG_MAX_JSON_CONTEXT_EVENTS: usize = 100_000;

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
    search_zip: bool,
    preprocessor: Option<String>,
    pre_glob_rules: Vec<RgGlobRule>,
    crlf: bool,
    multiline: bool,
    multiline_dotall: bool,
    line_numbers_explicit: bool,
    max_count: Option<usize>,
    max_columns: Option<usize>,
    max_columns_preview: bool,
    max_filesize: Option<u64>,
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
    null_data: bool,
    stop_on_nonmatch: bool,
    sort: RgSort,
    sort_reverse: bool,
    path_separator: String,
    encoding: RgEncoding,
    hidden: bool,
    type_list: bool,
    no_ignore: bool,
    no_ignore_dot: bool,
    no_ignore_exclude: bool,
    no_ignore_global: bool,
    no_ignore_parent: bool,
    no_ignore_files: bool,
    no_ignore_vcs: bool,
    require_git: bool,
    follow_symlinks: bool,
    ignore_file_case_insensitive: bool,
    unicode: bool,
    pcre2: bool,
    messages: bool,
    unrestricted_level: u8,
    context_separator: String,
    no_context_separator: bool,
    field_match_separator: String,
    field_context_separator: String,
    stdin_consumed_for_patterns: bool,
    ignore_file_paths: Vec<String>,
    explicit_ignore_rules: Vec<RgIgnoreRule>,
    global_ignore_rules: Vec<RgIgnoreRule>,
    glob_rules: Vec<RgGlobRule>,
    glob_case_insensitive: bool,
    type_database: RgTypeDatabase,
    type_includes: Vec<RgFileType>,
    type_excludes: Vec<RgFileType>,
    color: RgColorMode,
    color_scheme: RgColorScheme,
    hyperlink_format: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RgEncoding {
    Auto,
    None,
    Utf8,
    Utf16Le,
    Utf16Be,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RgColorMode {
    Auto,
    Never,
    Always,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RgSort {
    Path,
    Modified,
    Accessed,
    Created,
    None,
}

#[derive(Clone)]
struct RgColorScheme {
    path: RgColorStyle,
    line: RgColorStyle,
    column: RgColorStyle,
    highlight: RgColorStyle,
    matches: RgColorStyle,
}

#[derive(Clone)]
struct RgColorStyle {
    enabled: bool,
    bold: bool,
    intense: bool,
    underline: bool,
    italic: bool,
    fg: Option<String>,
    bg: Option<String>,
}

impl Default for RgColorScheme {
    fn default() -> Self {
        Self {
            path: RgColorStyle::fg("35"),
            line: RgColorStyle::fg("32"),
            column: RgColorStyle::plain(),
            highlight: RgColorStyle::disabled(),
            matches: RgColorStyle {
                enabled: true,
                bold: true,
                intense: false,
                underline: false,
                italic: false,
                fg: Some("31".to_string()),
                bg: None,
            },
        }
    }
}

impl RgColorScheme {
    fn apply(&mut self, spec: &str) -> Result<()> {
        const MAX_COLOR_SPEC_LEN: usize = 256;
        if spec.len() > MAX_COLOR_SPEC_LEN {
            return Err(invalid_color_spec_error(spec));
        }

        let mut fields = spec.splitn(4, ':');
        let field0 = fields.next().unwrap_or_default();
        let field1 = fields.next();
        let field2 = fields.next();
        let extra = fields.next();

        if field1 == Some("none") && field2.is_none() && extra.is_none() {
            self.style_mut(field0)?.disable();
            return Ok(());
        }
        if field1.is_none() || field2.is_none() || extra.is_some() {
            return Err(invalid_color_spec_error(spec));
        }
        let style = self.style_mut(field0)?;
        match field1.unwrap_or_default() {
            "fg" => style.set_fg(parse_ansi_fg(field2.unwrap_or_default())?),
            "bg" => style.set_bg(parse_ansi_bg(field2.unwrap_or_default())?),
            "style" => match field2.unwrap_or_default() {
                "bold" => style.set_bold(true),
                "nobold" => style.set_bold(false),
                "intense" => style.set_intense(true),
                "nointense" => style.set_intense(false),
                "underline" => style.set_underline(true),
                "nounderline" => style.set_underline(false),
                "italic" => style.set_italic(true),
                "noitalic" => style.set_italic(false),
                _ => {
                    return Err(Error::Execution(format!(
                        "rg: error parsing flag --colors: invalid style '{}'",
                        field2.unwrap_or_default()
                    )));
                }
            },
            _ => return Err(invalid_color_spec_error(spec)),
        }
        Ok(())
    }

    fn style_mut(&mut self, name: &str) -> Result<&mut RgColorStyle> {
        match name {
            "path" => Ok(&mut self.path),
            "line" => Ok(&mut self.line),
            "column" => Ok(&mut self.column),
            "highlight" => Ok(&mut self.highlight),
            "match" => Ok(&mut self.matches),
            _ => Err(Error::Execution(format!(
                "rg: error parsing flag --colors: unrecognized color type '{name}'"
            ))),
        }
    }
}

fn invalid_color_spec_error(spec: &str) -> Error {
    const MAX_SPEC_ECHO_CHARS: usize = 80;
    let mut truncated = spec.chars().take(MAX_SPEC_ECHO_CHARS).collect::<String>();
    if spec.chars().count() > MAX_SPEC_ECHO_CHARS {
        truncated.push_str("...");
    }
    Error::Execution(format!(
        "rg: error parsing flag --colors: invalid color spec '{truncated}'"
    ))
}

impl RgColorStyle {
    fn plain() -> Self {
        Self {
            enabled: true,
            bold: false,
            intense: false,
            underline: false,
            italic: false,
            fg: None,
            bg: None,
        }
    }

    fn fg(code: &str) -> Self {
        Self {
            enabled: true,
            bold: false,
            intense: false,
            underline: false,
            italic: false,
            fg: Some(code.to_string()),
            bg: None,
        }
    }

    fn disabled() -> Self {
        let mut style = Self::plain();
        style.disable();
        style
    }

    fn disable(&mut self) {
        self.enabled = false;
        self.bold = false;
        self.intense = false;
        self.underline = false;
        self.italic = false;
        self.fg = None;
        self.bg = None;
    }

    fn set_fg(&mut self, code: String) {
        self.enabled = true;
        self.fg = Some(code);
    }

    fn set_bg(&mut self, code: String) {
        self.enabled = true;
        self.bg = Some(code);
    }

    fn set_bold(&mut self, bold: bool) {
        self.enabled = true;
        self.bold = bold;
    }

    fn set_intense(&mut self, intense: bool) {
        self.enabled = true;
        self.intense = intense;
    }

    fn set_underline(&mut self, underline: bool) {
        self.enabled = true;
        self.underline = underline;
    }

    fn set_italic(&mut self, italic: bool) {
        self.enabled = true;
        self.italic = italic;
    }
}

#[derive(Clone)]
struct RgGlobRule {
    raw: String,
    include: bool,
    has_slash: bool,
    anchored: bool,
    iglob: bool,
    case_insensitive: bool,
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

struct RgIgnoreRuleSet {
    parent: Option<Arc<RgIgnoreRuleSet>>,
    local: Vec<RgIgnoreRule>,
    len: usize,
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

enum RgMatcher {
    Rust(Regex),
    Fancy(fancy_regex::Regex),
}

#[derive(Clone, Copy)]
struct RgMatch<'a> {
    text: &'a str,
    start: usize,
    end: usize,
}

impl<'a> RgMatch<'a> {
    fn as_str(self) -> &'a str {
        self.text
    }

    fn start(self) -> usize {
        self.start
    }

    fn end(self) -> usize {
        self.end
    }
}

fn rg_replacement_cap_marker() -> String {
    format!(
        "[rg: replacement output capped at {} bytes]",
        RG_MAX_REPLACEMENT_OUTPUT_BYTES
    )
}

fn replacement_output_exceeds_cap(
    haystack_len: usize,
    replacement: &str,
    match_count: usize,
    include_unmatched_text: bool,
) -> bool {
    let capture_ref_count = replacement.bytes().filter(|&byte| byte == b'$').count();
    let per_match = replacement
        .len()
        .saturating_add(capture_ref_count.saturating_mul(haystack_len));
    let projected =
        match_count
            .saturating_mul(per_match)
            .saturating_add(if include_unmatched_text {
                haystack_len
            } else {
                0
            });
    projected > RG_MAX_REPLACEMENT_OUTPUT_BYTES
}

impl RgMatcher {
    fn is_match(&self, text: &str) -> bool {
        match self {
            Self::Rust(regex) => regex.is_match(text),
            Self::Fancy(regex) => regex.is_match(text).unwrap_or(false),
        }
    }

    fn find<'a>(&self, text: &'a str) -> Option<RgMatch<'a>> {
        match self {
            Self::Rust(regex) => regex.find(text).map(|mat| RgMatch {
                text: mat.as_str(),
                start: mat.start(),
                end: mat.end(),
            }),
            Self::Fancy(regex) => regex.find(text).ok().flatten().map(|mat| RgMatch {
                text: mat.as_str(),
                start: mat.start(),
                end: mat.end(),
            }),
        }
    }

    // Callers use this streaming matcher in resource-sensitive paths; the bool
    // return preserves early exit without rebuilding eager per-line match Vecs.
    fn for_each_match<'a>(&self, text: &'a str, mut f: impl FnMut(RgMatch<'a>) -> bool) {
        match self {
            Self::Rust(regex) => {
                for mat in regex.find_iter(text) {
                    if !f(RgMatch {
                        text: mat.as_str(),
                        start: mat.start(),
                        end: mat.end(),
                    }) {
                        break;
                    }
                }
            }
            Self::Fancy(regex) => {
                for mat in regex.find_iter(text).flatten() {
                    if !f(RgMatch {
                        text: mat.as_str(),
                        start: mat.start(),
                        end: mat.end(),
                    }) {
                        break;
                    }
                }
            }
        }
    }

    fn count_matches(&self, text: &str) -> usize {
        match self {
            Self::Rust(regex) => regex.find_iter(text).count(),
            Self::Fancy(regex) => regex.find_iter(text).flatten().count(),
        }
    }

    fn replace_all(&self, text: &str, replacement: &str) -> String {
        // THREAT[TM-DOS-RG-REPLACE]: attacker-controlled `--replace` text combined
        // with many matches can allocate output before interpreter stdout
        // truncation. Budget on a conservative expansion upper bound so capture
        // references like `$1` cannot bypass the plain replacement length check.
        let match_count = self.count_matches(text);
        if replacement_output_exceeds_cap(text.len(), replacement, match_count, true) {
            return rg_replacement_cap_marker();
        }
        match self {
            Self::Rust(regex) => regex.replace_all(text, replacement).into_owned(),
            Self::Fancy(regex) => regex.replace_all(text, replacement).into_owned(),
        }
    }

    fn replace_first(&self, text: &str, replacement: &str) -> String {
        if replacement_output_exceeds_cap(text.len(), replacement, 1, true) {
            return rg_replacement_cap_marker();
        }
        match self {
            Self::Rust(regex) => regex.replace(text, replacement).into_owned(),
            Self::Fancy(regex) => regex.replacen(text, 1, replacement).into_owned(),
        }
    }

    fn replacement_matches_exceed_cap(&self, text: &str, replacement: &str) -> bool {
        replacement_output_exceeds_cap(text.len(), replacement, self.count_matches(text), false)
    }
}

impl RgOptions {
    fn apply_unrestricted(&mut self) {
        self.unrestricted_level = self.unrestricted_level.saturating_add(1);
        match self.unrestricted_level {
            1 => {
                self.no_ignore = true;
                self.no_ignore_dot = true;
                self.no_ignore_exclude = true;
                self.no_ignore_global = true;
                self.no_ignore_parent = true;
                self.no_ignore_vcs = true;
            }
            2 => self.hidden = true,
            _ => self.binary = true,
        }
    }

    fn set_count_only(&mut self) {
        self.count_only = true;
        self.count_matches = false;
        self.files_with_matches = false;
        self.files_without_matches = false;
        self.list_files = false;
        self.json = false;
    }

    fn set_count_matches(&mut self) {
        self.count_only = false;
        self.count_matches = true;
        self.files_with_matches = false;
        self.files_without_matches = false;
        self.list_files = false;
        self.json = false;
    }

    fn set_files_with_matches(&mut self) {
        self.count_only = false;
        self.count_matches = false;
        self.files_with_matches = true;
        self.files_without_matches = false;
        self.list_files = false;
        self.json = false;
    }

    fn set_files_without_matches(&mut self) {
        self.count_only = false;
        self.count_matches = false;
        self.files_with_matches = false;
        self.files_without_matches = true;
        self.list_files = false;
        self.json = false;
    }

    fn set_json(&mut self) {
        self.count_only = false;
        self.count_matches = false;
        self.files_with_matches = false;
        self.files_without_matches = false;
        self.list_files = false;
        self.json = true;
    }

    fn set_list_files(&mut self) {
        self.count_only = false;
        self.count_matches = false;
        self.files_with_matches = false;
        self.files_without_matches = false;
        self.list_files = true;
        self.json = false;
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
            search_zip: false,
            preprocessor: None,
            pre_glob_rules: Vec::new(),
            crlf: false,
            multiline: false,
            multiline_dotall: false,
            line_numbers_explicit: false,
            max_count: None,
            max_columns: None,
            max_columns_preview: false,
            max_filesize: None,
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
            null_data: false,
            stop_on_nonmatch: false,
            sort: RgSort::Path,
            sort_reverse: false,
            path_separator: "/".to_string(),
            encoding: RgEncoding::Auto,
            hidden: false,
            type_list: false,
            no_ignore: false,
            no_ignore_dot: false,
            no_ignore_exclude: false,
            no_ignore_global: false,
            no_ignore_parent: false,
            no_ignore_files: false,
            no_ignore_vcs: false,
            require_git: true,
            follow_symlinks: false,
            ignore_file_case_insensitive: false,
            unicode: true,
            pcre2: false,
            messages: true,
            unrestricted_level: 0,
            context_separator: "--".to_string(),
            no_context_separator: false,
            field_match_separator: ":".to_string(),
            field_context_separator: "-".to_string(),
            stdin_consumed_for_patterns: false,
            ignore_file_paths: Vec::new(),
            explicit_ignore_rules: Vec::new(),
            global_ignore_rules: Vec::new(),
            glob_rules: Vec::new(),
            glob_case_insensitive: false,
            type_database: RgTypeDatabase::default(),
            type_includes: Vec::new(),
            type_excludes: Vec::new(),
            color: RgColorMode::Auto,
            color_scheme: RgColorScheme::default(),
            hyperlink_format: None,
        };

        let mut positional = Vec::new();
        let mut pending_type_includes = Vec::new();
        let mut pending_type_excludes = Vec::new();
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
            } else if let Some(val) = long_value(&mut p, "--max-filesize")? {
                opts.max_filesize = Some(parse_max_filesize(&val)?);
            } else if let Some(val) = p.flag_value("-j", "rg").map_err(Error::Execution)? {
                parse_threads(val, "-j")?;
            } else if let Some(val) = long_value(&mut p, "--threads")? {
                parse_threads(&val, "--threads")?;
            } else if let Some(val) = long_value(&mut p, "--regex-size-limit")? {
                parse_noop_size_limit(&val, "--regex-size-limit")?;
            } else if let Some(val) = long_value(&mut p, "--dfa-size-limit")? {
                parse_noop_size_limit(&val, "--dfa-size-limit")?;
            } else if let Some(val) = p.flag_value("-d", "rg").map_err(Error::Execution)? {
                opts.max_depth = Some(parse_max_depth(val, "-d")?);
            } else if let Some(val) = long_value(&mut p, "--max-depth")? {
                opts.max_depth = Some(parse_max_depth(&val, "--max-depth")?);
            } else if let Some(val) = long_value(&mut p, "--maxdepth")? {
                opts.max_depth = Some(parse_max_depth(&val, "--maxdepth")?);
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
                opts.context_separator = parse_rg_separator(&val);
                opts.no_context_separator = false;
            } else if p.flag("--no-context-separator") {
                opts.no_context_separator = true;
            } else if let Some(val) = long_value(&mut p, "--field-match-separator")? {
                opts.field_match_separator = parse_rg_separator(&val);
            } else if let Some(val) = long_value(&mut p, "--field-context-separator")? {
                opts.field_context_separator = parse_rg_separator(&val);
            } else if let Some(val) = p.flag_value("-g", "rg").map_err(Error::Execution)? {
                opts.glob_rules
                    .push(RgGlobRule::parse(val, false, opts.glob_case_insensitive)?);
            } else if let Some(val) = long_value(&mut p, "--glob")? {
                opts.glob_rules
                    .push(RgGlobRule::parse(&val, false, opts.glob_case_insensitive)?);
            } else if let Some(val) = long_value(&mut p, "--iglob")? {
                opts.glob_rules.push(RgGlobRule::parse(&val, true, true)?);
            } else if p.flag("--glob-case-insensitive") {
                opts.glob_case_insensitive = true;
            } else if p.flag("--no-glob-case-insensitive") {
                opts.glob_case_insensitive = false;
            } else if let Some(val) = long_value(&mut p, "--ignore-file")? {
                opts.ignore_file_paths.push(val);
            } else if let Some(val) = p.flag_value("-t", "rg").map_err(Error::Execution)? {
                pending_type_includes.push(val.to_string());
            } else if let Some(val) = p.flag_value("-T", "rg").map_err(Error::Execution)? {
                pending_type_excludes.push(val.to_string());
            } else if let Some(val) = long_value(&mut p, "--type")? {
                pending_type_includes.push(val);
            } else if let Some(val) = long_value(&mut p, "--type-not")? {
                pending_type_excludes.push(val);
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
                opts.line_numbers_explicit = false;
            } else if p.flag("--line-number") {
                opts.line_numbers = true;
                opts.line_numbers_explicit = true;
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
                opts.set_count_only();
            } else if p.flag_any(&["--count-matches"]) {
                opts.set_count_matches();
            } else if p.flag_any(&["--files-with-matches"]) {
                opts.set_files_with_matches();
            } else if p.flag_any(&["--files-without-match"]) {
                opts.set_files_without_matches();
            } else if p.flag_any(&["--invert-match"]) {
                opts.invert_match = true;
            } else if p.flag("--no-invert-match") {
                opts.invert_match = false;
            } else if p.flag_any(&["--word-regexp"]) {
                opts.word_boundary = true;
            } else if p.flag_any(&["--line-regexp"]) {
                opts.line_regexp = true;
            } else if p.flag_any(&["--fixed-strings"]) {
                opts.fixed_strings = true;
            } else if p.flag("--no-fixed-strings") {
                opts.fixed_strings = false;
            } else if p.flag_any(&["--text"]) {
                opts.text = true;
            } else if p.flag("--no-text") {
                opts.text = false;
            } else if p.flag("--binary") {
                opts.binary = true;
                opts.text = false;
            } else if p.flag("--no-binary") {
                opts.binary = false;
            } else if p.flag("--search-zip") {
                opts.search_zip = true;
                opts.preprocessor = None;
            } else if p.flag("--no-search-zip") {
                opts.search_zip = false;
            } else if let Some(val) = long_value(&mut p, "--pre")? {
                opts.preprocessor = Some(val);
                opts.search_zip = false;
            } else if p.flag("--no-pre") {
                opts.preprocessor = None;
            } else if let Some(val) = long_value(&mut p, "--pre-glob")? {
                opts.pre_glob_rules.push(RgGlobRule::parse(
                    &val,
                    false,
                    opts.glob_case_insensitive,
                )?);
            } else if p.flag("--crlf") {
                opts.crlf = true;
                opts.null_data = false;
            } else if p.flag("--no-crlf") {
                opts.crlf = false;
            } else if p.flag_any(&["--multiline"]) {
                opts.multiline = true;
            } else if p.flag("--no-multiline") {
                opts.multiline = false;
            } else if p.flag("--multiline-dotall") {
                opts.multiline_dotall = true;
            } else if p.flag("--no-multiline-dotall") {
                opts.multiline_dotall = false;
            } else if p.flag_any(&["--only-matching"]) {
                opts.only_matching = true;
            } else if p.flag_any(&["--quiet", "--silent"]) {
                opts.quiet = true;
            } else if p.flag("--column") {
                opts.column = true;
                opts.line_numbers = true;
            } else if p.flag("--no-column") {
                opts.column = false;
                if !opts.line_numbers_explicit {
                    opts.line_numbers = false;
                }
            } else if p.flag("--byte-offset") {
                opts.byte_offset = true;
            } else if p.flag("--no-byte-offset") {
                opts.byte_offset = false;
            } else if p.flag("--vimgrep") {
                opts.vimgrep = true;
                opts.show_filename = true;
            } else if p.flag("--json") {
                opts.set_json();
            } else if p.flag("--no-json") {
                opts.json = false;
            } else if p.flag("--stats") {
                opts.stats = true;
            } else if p.flag("--no-stats") {
                opts.stats = false;
            } else if p.flag("--files") {
                opts.set_list_files();
            } else if p.flag_any(&["--passthru", "--passthrough"]) {
                opts.passthru = true;
            } else if p.flag("--trim") {
                opts.trim = true;
            } else if p.flag("--no-trim") {
                opts.trim = false;
            } else if p.flag("--max-columns-preview") {
                opts.max_columns_preview = true;
            } else if p.flag("--no-max-columns-preview") {
                opts.max_columns_preview = false;
            } else if p.flag("--include-zero") {
                opts.include_zero = true;
            } else if p.flag("--no-include-zero") {
                opts.include_zero = false;
            } else if p.flag("--heading") {
                opts.heading = true;
            } else if p.flag("--no-heading") {
                opts.heading = false;
            } else if p.flag("--null") {
                opts.null = true;
            } else if p.flag("--null-data") {
                opts.null_data = true;
                opts.text = true;
                opts.crlf = false;
            } else if p.flag("--stop-on-nonmatch") {
                opts.stop_on_nonmatch = true;
                opts.multiline = false;
            } else if p.flag_any(&["--sort-files", "--no-sort-files"]) {
                // no-op: bashkit's recursive walker already sorts paths.
            } else if let Some(val) = long_value(&mut p, "--sort")? {
                opts.sort = parse_sort_value(&val, "--sort")?;
                opts.sort_reverse = false;
            } else if let Some(val) = long_value(&mut p, "--sortr")? {
                opts.sort = parse_sort_value(&val, "--sortr")?;
                opts.sort_reverse = true;
            } else if let Some(val) = long_value(&mut p, "--path-separator")? {
                opts.path_separator = parse_path_separator(&val)?;
            } else if let Some(val) = p.flag_value("-E", "rg").map_err(Error::Execution)? {
                opts.encoding = parse_encoding(val)?;
            } else if let Some(val) = long_value(&mut p, "--encoding")? {
                opts.encoding = parse_encoding(&val)?;
            } else if p.flag("--no-encoding") {
                opts.encoding = RgEncoding::Auto;
            } else if let Some(val) = long_value(&mut p, "--engine")? {
                opts.pcre2 = parse_regex_engine(&val)?;
            } else if let Some(val) = long_value(&mut p, "--color")? {
                opts.color = parse_color_mode(&val)?;
            } else if let Some(val) = long_value(&mut p, "--colors")? {
                opts.color_scheme.apply(&val)?;
            } else if p.flag_any(&["--pretty"]) {
                opts.heading = true;
                opts.line_numbers = true;
            } else if p.flag("--hidden") {
                opts.hidden = true;
            } else if p.flag("--no-hidden") {
                opts.hidden = false;
            } else if p.flag("--no-ignore") {
                opts.no_ignore = true;
                opts.no_ignore_dot = true;
                opts.no_ignore_exclude = true;
                opts.no_ignore_global = true;
                opts.no_ignore_parent = true;
                opts.no_ignore_vcs = true;
            } else if p.flag("--ignore") {
                opts.no_ignore = false;
                opts.no_ignore_dot = false;
                opts.no_ignore_exclude = false;
                opts.no_ignore_global = false;
                opts.no_ignore_parent = false;
                opts.no_ignore_vcs = false;
            } else if p.flag("--no-ignore-dot") {
                opts.no_ignore_dot = true;
            } else if p.flag("--ignore-dot") {
                opts.no_ignore = false;
                opts.no_ignore_dot = false;
            } else if p.flag("--no-ignore-files") {
                opts.no_ignore_files = true;
            } else if p.flag("--ignore-files") {
                opts.no_ignore_files = false;
            } else if p.flag("--no-ignore-exclude") {
                opts.no_ignore_exclude = true;
            } else if p.flag("--ignore-exclude") {
                opts.no_ignore = false;
                opts.no_ignore_exclude = false;
            } else if p.flag("--no-ignore-global") {
                opts.no_ignore_global = true;
            } else if p.flag("--ignore-global") {
                opts.no_ignore = false;
                opts.no_ignore_global = false;
            } else if p.flag("--no-ignore-parent") {
                opts.no_ignore_parent = true;
            } else if p.flag("--ignore-parent") {
                opts.no_ignore = false;
                opts.no_ignore_parent = false;
            } else if p.flag("--no-ignore-vcs") {
                opts.no_ignore_vcs = true;
            } else if p.flag("--ignore-vcs") {
                opts.no_ignore = false;
                opts.no_ignore_vcs = false;
                opts.no_ignore_parent = false;
            } else if p.flag("--ignore-file-case-insensitive") {
                opts.ignore_file_case_insensitive = true;
            } else if p.flag("--no-ignore-file-case-insensitive") {
                opts.ignore_file_case_insensitive = false;
            } else if p.flag("--no-require-git") {
                opts.require_git = false;
            } else if p.flag("--require-git") {
                opts.require_git = true;
            } else if p.flag("--follow") {
                opts.follow_symlinks = true;
            } else if p.flag("--no-follow") {
                opts.follow_symlinks = false;
            } else if p.flag("--unrestricted") {
                opts.apply_unrestricted();
            } else if p.flag("--no-messages") {
                opts.messages = false;
            } else if p.flag("--messages") {
                opts.messages = true;
            } else if p.flag("--") {
                while let Some(arg) = p.positional() {
                    positional.push(arg.to_string());
                }
            } else if p.flag_any(&[
                "--debug",
                "--trace",
                "--no-ignore-messages",
                "--ignore-messages",
            ]) {
                // no-op: these flags only affect real rg's diagnostic stderr.
            } else if long_value(&mut p, "--hostname-bin")?.is_some() {
                // no-op: hyperlink hostname discovery is not modeled.
            } else if let Some(val) = long_value(&mut p, "--hyperlink-format")? {
                opts.hyperlink_format = parse_hyperlink_format(&val)?;
            } else if p.flag("--unicode") {
                opts.unicode = true;
            } else if p.flag("--no-unicode") {
                opts.unicode = false;
            } else if p.flag_any(&[
                "--no-config",
                "--line-buffered",
                "--block-buffered",
                "--no-block-buffered",
                "--no-line-buffered",
                "--one-file-system",
                "--no-one-file-system",
                "--mmap",
                "--no-mmap",
                "--pcre2-unicode",
                "--no-pcre2-unicode",
            ]) {
                // no-op: these flags affect host config, buffering, parent ignore
                // discovery, symlink walking, IO strategy, or regex engine
                // selection, none of which are modeled here for simple searches.
            } else if p.flag("--pcre2") {
                opts.pcre2 = true;
            } else if p.flag("--no-pcre2") {
                opts.pcre2 = false;
            } else if p.flag("--auto-hybrid-regex") {
                opts.pcre2 = true;
            } else if p.flag("--no-auto-hybrid-regex") {
                opts.pcre2 = false;
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
                        'n' => {
                            opts.line_numbers = true;
                            opts.line_numbers_explicit = true;
                        }
                        'N' => {
                            opts.line_numbers = false;
                            opts.line_numbers_explicit = false;
                        }
                        'c' => opts.set_count_only(),
                        'l' => opts.set_files_with_matches(),
                        'v' => opts.invert_match = true,
                        'w' => opts.word_boundary = true,
                        'x' => opts.line_regexp = true,
                        'F' => opts.fixed_strings = true,
                        'a' => opts.text = true,
                        'z' => {
                            opts.search_zip = true;
                            opts.preprocessor = None;
                        }
                        '0' => opts.null = true,
                        '.' => opts.hidden = true,
                        'j' => {
                            let rest: String = chars[j + 1..].iter().collect();
                            let threads = if !rest.is_empty() {
                                rest
                            } else {
                                match p.positional() {
                                    Some(v) => v.to_string(),
                                    None => {
                                        return Err(Error::Execution(
                                            "rg: -j requires an argument".to_string(),
                                        ));
                                    }
                                }
                            };
                            parse_threads(&threads, "-j")?;
                            break;
                        }
                        'p' => {
                            opts.heading = true;
                            opts.line_numbers = true;
                        }
                        'u' => opts.apply_unrestricted(),
                        'L' => opts.follow_symlinks = true,
                        'H' => opts.show_filename = true,
                        'I' => opts.no_filename = true,
                        'o' => opts.only_matching = true,
                        'q' => opts.quiet = true,
                        'b' => opts.byte_offset = true,
                        'P' => opts.pcre2 = true,
                        'U' => opts.multiline = true,
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
                        'd' => {
                            let rest: String = chars[j + 1..].iter().collect();
                            let depth = if !rest.is_empty() {
                                rest
                            } else {
                                match p.positional() {
                                    Some(v) => v.to_string(),
                                    None => {
                                        return Err(Error::Execution(
                                            "rg: -d requires an argument".to_string(),
                                        ));
                                    }
                                }
                            };
                            opts.max_depth = Some(parse_max_depth(&depth, "-d")?);
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
                            opts.glob_rules.push(RgGlobRule::parse(
                                &glob,
                                false,
                                opts.glob_case_insensitive,
                            )?);
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
                            pending_type_includes.push(file_type);
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
                            pending_type_excludes.push(file_type);
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

        opts.set_glob_case_insensitive(opts.glob_case_insensitive)?;

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
        opts.type_includes = pending_type_includes
            .into_iter()
            .map(|name| opts.type_database.parse(&name))
            .collect::<Result<Vec<_>>>()?;
        opts.type_excludes = pending_type_excludes
            .into_iter()
            .map(|name| opts.type_database.parse(&name))
            .collect::<Result<Vec<_>>>()?;

        if opts.only_matching && opts.patterns.iter().any(|pattern| pattern.is_empty()) {
            return Err(Error::Execution(
                "rg: empty pattern is not allowed with --only-matching".to_string(),
            ));
        }

        Ok(opts)
    }

    fn build_regex(&self) -> Result<RgMatcher> {
        if !self.multiline && self.patterns.iter().any(|pattern| pattern.contains('\n')) {
            return Err(Error::Execution(
                "rg: the literal \"\\n\" is not allowed in a regex\n\nConsider enabling multiline mode with the --multiline flag (or -U for short).\nWhen multiline mode is enabled, new line characters can be matched.".to_string(),
            ));
        }
        let combined = self
            .patterns
            .iter()
            .map(|pattern| format!("(?:{})", self.prepare_pattern(pattern)))
            .collect::<Vec<_>>()
            .join("|");
        if self.pcre2 {
            return FancyRegexBuilder::new(&combined)
                .case_insensitive(self.effective_ignore_case())
                .unicode_mode(self.unicode)
                .dot_matches_new_line(self.multiline && self.multiline_dotall)
                .backtrack_limit(super::search_common::REGEX_DFA_SIZE_LIMIT)
                .build()
                .map(RgMatcher::Fancy)
                .map_err(|e| Error::Execution(format!("rg: invalid pattern: {}", e)));
        }
        RegexBuilder::new(&combined)
            .case_insensitive(self.effective_ignore_case())
            .unicode(self.unicode)
            .dot_matches_new_line(self.multiline && self.multiline_dotall)
            .size_limit(super::search_common::REGEX_SIZE_LIMIT)
            .dfa_size_limit(super::search_common::REGEX_DFA_SIZE_LIMIT)
            .build()
            .map(RgMatcher::Rust)
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
        let mut matched = None;
        let mut has_include = false;
        for rule in &self.glob_rules {
            has_include |= rule.include;
            if rule.matches(path, cwd) {
                matched = Some(rule.include);
            }
        }
        matched.unwrap_or(!has_include)
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

    fn matches_max_filesize(&self, size: u64) -> bool {
        self.max_filesize.is_none_or(|max| size <= max)
    }

    fn preprocessor_applies_to(&self, path: &Path, cwd: &Path) -> bool {
        if self.preprocessor.is_none() || self.pre_glob_rules.is_empty() {
            return self.preprocessor.is_some();
        }

        let mut matched = None;
        let mut has_include = false;
        for rule in &self.pre_glob_rules {
            has_include |= rule.include;
            if rule.matches(path, cwd) {
                matched = Some(rule.include);
            }
        }
        matched.unwrap_or(!has_include)
    }

    fn validate_preprocessor_for(
        &self,
        path: &Path,
        cwd: &Path,
    ) -> std::result::Result<(), String> {
        let Some(preprocessor) = &self.preprocessor else {
            return Ok(());
        };
        if preprocessor.is_empty()
            || preprocessor == "cat"
            || !self.preprocessor_applies_to(path, cwd)
        {
            return Ok(());
        }
        Err(format!(
            "preprocessor command could not start: '\"{preprocessor}\" \"{}\"': No such file or directory (os error 2)",
            path.display()
        ))
    }

    fn first_positive_glob(&self) -> Option<String> {
        self.glob_rules
            .iter()
            .find(|g| g.include)
            .map(|g| g.raw.clone())
    }

    fn uses_ignore_files(&self) -> bool {
        !self.no_ignore || (!self.no_ignore_files && !self.ignore_file_paths.is_empty())
    }

    fn set_glob_case_insensitive(&mut self, case_insensitive: bool) -> Result<()> {
        for rule in &mut self.glob_rules {
            if !rule.iglob {
                rule.set_case_insensitive(case_insensitive)?;
            }
        }
        Ok(())
    }

    fn is_ignored_by_rules(&self, path: &Path, is_dir: bool, rules: &RgIgnoreRuleSet) -> bool {
        rules.is_ignored(path, is_dir)
    }

    fn color_enabled(&self) -> bool {
        self.color == RgColorMode::Always
    }
}

impl RgIgnoreRuleSet {
    fn root(local: Vec<RgIgnoreRule>) -> Self {
        let len = local.len();
        Self {
            parent: None,
            local,
            len,
        }
    }

    fn child(parent: Arc<Self>, local: Vec<RgIgnoreRule>) -> Self {
        let len = parent.len + local.len();
        Self {
            parent: Some(parent),
            local,
            len,
        }
    }

    fn len(&self) -> usize {
        self.len
    }

    fn is_ignored(&self, path: &Path, is_dir: bool) -> bool {
        let mut ignored = false;
        self.apply_matches(path, is_dir, &mut ignored);
        ignored
    }

    fn apply_matches(&self, path: &Path, is_dir: bool, ignored: &mut bool) {
        if let Some(parent) = &self.parent {
            parent.apply_matches(path, is_dir, ignored);
        }
        for rule in &self.local {
            if rule.matches(path, is_dir) || (!is_dir && rule.matches_parent_dir(path)) {
                *ignored = !rule.include;
            }
        }
    }
}

impl RgIgnoreRule {
    fn parse(line: &str, base: &Path, case_insensitive: bool) -> Result<Option<Self>> {
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
        let regex = build_regex_opts(&glob_to_regex(normalized), case_insensitive)
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
        static DEFAULT: OnceLock<RgTypeDatabase> = OnceLock::new();
        DEFAULT.get_or_init(Self::build_default).clone()
    }

    fn build_default() -> Self {
        let mut db = Self {
            definitions: BTreeMap::new(),
        };
        db.insert_defaults("ada", &["*.adb", "*.ads"]);
        db.insert_defaults("agda", &["*.agda", "*.lagda"]);
        db.insert_defaults("aidl", &["*.aidl"]);
        db.insert_defaults("alire", &["alire.toml"]);
        db.insert_defaults("amake", &["*.bp", "*.mk"]);
        db.insert_defaults("asciidoc", &["*.adoc", "*.asc", "*.asciidoc"]);
        db.insert_defaults("asm", &["*.S", "*.asm", "*.s"]);
        db.insert_defaults(
            "asp",
            &[
                "*.ascx",
                "*.ascx.cs",
                "*.ascx.vb",
                "*.asp",
                "*.aspx",
                "*.aspx.cs",
                "*.aspx.vb",
            ],
        );
        db.insert_defaults("ats", &["*.ats", "*.dats", "*.hats", "*.sats"]);
        db.insert_defaults("avro", &["*.avdl", "*.avpr", "*.avsc"]);
        db.insert_defaults("awk", &["*.awk"]);
        db.insert_defaults(
            "bazel",
            &[
                "*.BUILD",
                "*.bazel",
                "*.bazelrc",
                "*.bzl",
                "BUILD",
                "MODULE.bazel",
                "WORKSPACE",
                "WORKSPACE.bazel",
                "WORKSPACE.bzlmod",
            ],
        );
        db.insert_defaults(
            "bitbake",
            &["*.bb", "*.bbappend", "*.bbclass", "*.conf", "*.inc"],
        );
        db.insert_defaults("boxlang", &["*.bx", "*.bxm", "*.bxs"]);
        db.insert_defaults("brotli", &["*.br"]);
        db.insert_defaults("buildstream", &["*.bst"]);
        db.insert_defaults("bzip2", &["*.bz2", "*.tbz2"]);
        db.insert_defaults("c", &["*.c", "*.h"]);
        db.insert_defaults("cabal", &["*.cabal"]);
        db.insert_defaults("candid", &["*.did"]);
        db.insert_defaults("carp", &["*.carp"]);
        db.insert_defaults("cbor", &["*.cbor"]);
        db.insert_defaults("ceylon", &["*.ceylon"]);
        db.insert_defaults("coffeescript", &["*.coffee"]);
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
        db.insert_defaults("cs", &["*.cs"]);
        db.insert_defaults("csharp", &["*.cs"]);
        db.insert_defaults("cshtml", &["*.cshtml"]);
        db.insert_defaults("csproj", &["*.csproj"]);
        db.insert_defaults("clojure", &["*.clj", "*.cljc", "*.cljs", "*.cljx"]);
        db.insert_defaults("crystal", &["*.cr", "*.ecr", "Projectfile", "shard.yml"]);
        db.insert_defaults("csv", &["*.csv"]);
        db.insert_defaults("cython", &["*.pxd", "*.pxi", "*.pyx"]);
        db.insert_defaults("dart", &["*.dart"]);
        db.insert_defaults("devicetree", &["*.dts", "*.dtsi", "*.dtso"]);
        db.insert_defaults("elm", &["*.elm"]);
        db.insert_defaults("bat", &["*.bat"]);
        db.insert_defaults("batch", &["*.bat"]);
        db.insert_defaults("cmd", &["*.bat", "*.cmd"]);
        db.insert_defaults("cfml", &["*.cfc", "*.cfm"]);
        db.insert_defaults("cmake", &["*.cmake", "CMakeLists.txt"]);
        db.insert_defaults("cml", &["*.cml"]);
        db.insert_defaults("config", &["*.cfg", "*.conf", "*.config", "*.ini"]);
        db.insert_defaults("coq", &["*.v"]);
        db.insert_defaults("creole", &["*.creole"]);
        db.insert_defaults("cuda", &["*.cu", "*.cuh"]);
        db.insert_defaults("d", &["*.d"]);
        db.insert_defaults("dhall", &["*.dhall"]);
        db.insert_defaults("diff", &["*.diff", "*.patch"]);
        db.insert_defaults("dita", &["*.dita", "*.ditamap", "*.ditaval"]);
        db.insert_defaults("docker", &["*Dockerfile*"]);
        db.insert_defaults(
            "dockercompose",
            &["docker-compose.*.yml", "docker-compose.yml"],
        );
        db.insert_defaults("dts", &["*.dts", "*.dtsi"]);
        db.insert_defaults("dvc", &["*.dvc", "Dvcfile"]);
        db.insert_defaults("ebuild", &["*.ebuild", "*.eclass"]);
        db.insert_defaults("edn", &["*.edn"]);
        db.insert_defaults("elisp", &["*.el"]);
        db.insert_defaults(
            "elixir",
            &["*.eex", "*.ex", "*.exs", "*.heex", "*.leex", "*.livemd"],
        );
        db.insert_defaults("erlang", &["*.erl", "*.hrl"]);
        db.insert_defaults("erb", &["*.erb"]);
        db.insert_defaults("fennel", &["*.fnl"]);
        db.insert_defaults("fidl", &["*.fidl"]);
        db.insert_defaults("fish", &["*.fish"]);
        db.insert_defaults("flatbuffers", &["*.fbs"]);
        db.insert_defaults(
            "fortran",
            &[
                "*.F", "*.F77", "*.F90", "*.F95", "*.f", "*.f77", "*.f90", "*.f95", "*.pfo",
            ],
        );
        db.insert_defaults("fsharp", &["*.fs", "*.fsi", "*.fsx"]);
        db.insert_defaults("fut", &["*.fut"]);
        db.insert_defaults("gap", &["*.g", "*.gap", "*.gd", "*.gi", "*.tst"]);
        db.insert_defaults("gdscript", &["*.gd"]);
        db.insert_defaults("gleam", &["*.gleam"]);
        db.insert_defaults("gn", &["*.gn", "*.gni"]);
        db.insert_defaults("go", &["*.go"]);
        db.insert_defaults("gprbuild", &["*.gpr"]);
        db.insert_defaults(
            "gradle",
            &[
                "*.gradle",
                "*.gradle.kts",
                "gradle-wrapper.*",
                "gradle.properties",
                "gradlew",
                "gradlew.bat",
            ],
        );
        db.insert_defaults("groovy", &["*.gradle", "*.groovy"]);
        db.insert_defaults("graphql", &["*.graphql", "*.graphqls"]);
        db.insert_defaults("gzip", &["*.gz", "*.tgz"]);
        db.insert_defaults("h", &["*.h", "*.hh", "*.hpp"]);
        db.insert_defaults("haml", &["*.haml"]);
        db.insert_defaults("hare", &["*.ha"]);
        db.insert_defaults("haskell", &["*.c2hs", "*.cpphs", "*.hs", "*.hsc", "*.lhs"]);
        db.insert_defaults("hbs", &["*.hbs"]);
        db.insert_defaults("hs", &["*.hs", "*.lhs"]);
        db.insert_defaults("html", &["*.htm", "*.html"]);
        db.insert_defaults("htm", &["*.htm", "*.html"]);
        db.insert_defaults("hy", &["*.hy"]);
        db.insert_defaults("idris", &["*.idr", "*.lidr"]);
        db.insert_defaults("janet", &["*.janet"]);
        db.insert_defaults("java", &["*.java"]);
        db.insert_defaults("jinja", &["*.j2", "*.jinja", "*.jinja2"]);
        db.insert_defaults("jl", &["*.jl"]);
        db.insert_defaults("json", &["*.json", "*.jsonl"]);
        db.insert_defaults("jsonl", &["*.jsonl"]);
        db.insert_defaults("julia", &["*.jl"]);
        db.insert_defaults("jupyter", &["*.ipynb", "*.jpynb"]);
        db.insert_defaults("k", &["*.k"]);
        db.insert_defaults("kconfig", &["Kconfig", "Kconfig.*"]);
        db.insert_defaults("kotlin", &["*.kt", "*.kts"]);
        db.insert_defaults("lean", &["*.lean"]);
        db.insert_defaults("less", &["*.less"]);
        db.insert_defaults(
            "license",
            &[
                "*[.-]LICEN[CS]E*",
                "AGPL-*[0-9]*",
                "APACHE-*[0-9]*",
                "BSD-*[0-9]*",
                "CC-BY-*",
                "COPYING",
                "COPYING[.-]*",
                "COPYRIGHT",
                "COPYRIGHT[.-]*",
                "EULA",
                "EULA[.-]*",
                "GFDL-*[0-9]*",
                "GNU-*[0-9]*",
                "GPL-*[0-9]*",
                "LGPL-*[0-9]*",
                "LICEN[CS]E",
                "LICEN[CS]E[.-]*",
                "MIT-*[0-9]*",
                "MPL-*[0-9]*",
                "NOTICE",
                "NOTICE[.-]*",
                "OFL-*[0-9]*",
                "PATENTS",
                "PATENTS[.-]*",
                "UNLICEN[CS]E",
                "UNLICEN[CS]E[.-]*",
                "agpl[.-]*",
                "gpl[.-]*",
                "lgpl[.-]*",
                "licen[cs]e",
                "licen[cs]e.*",
            ],
        );
        db.insert_defaults("lilypond", &["*.ily", "*.ly"]);
        db.insert_defaults("lua", &["*.lua"]);
        db.insert_defaults("llvm", &["*.ll"]);
        db.insert_defaults("lock", &["*.lock", "package-lock.json"]);
        db.insert_defaults("log", &["*.log"]);
        db.insert_defaults("lz4", &["*.lz4"]);
        db.insert_defaults("lzma", &["*.lzma"]);
        db.insert_defaults("m4", &["*.ac", "*.m4"]);
        db.insert_defaults(
            "lisp",
            &["*.el", "*.jl", "*.lisp", "*.lsp", "*.sc", "*.scm"],
        );
        db.insert_defaults(
            "make",
            &[
                "*.mak",
                "*.mk",
                "Makefile.*",
                "[Gg][Nn][Uu]makefile",
                "[Gg][Nn][Uu]makefile.am",
                "[Gg][Nn][Uu]makefile.in",
                "[Mm]akefile",
                "[Mm]akefile.am",
                "[Mm]akefile.in",
            ],
        );
        db.insert_defaults("mako", &["*.mako", "*.mao"]);
        db.insert_defaults("man", &["*.[0-9][cEFMmpSx]", "*.[0-9lnpx]"]);
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
        db.insert_defaults("matlab", &["*.m"]);
        db.insert_defaults(
            "meson",
            &["meson.build", "meson.options", "meson_options.txt"],
        );
        db.insert_defaults("minified", &["*.min.css", "*.min.html", "*.min.js"]);
        db.insert_defaults("mint", &["*.mint"]);
        db.insert_defaults("mk", &["mkfile"]);
        db.insert_defaults("ml", &["*.ml"]);
        db.insert_defaults("motoko", &["*.mo"]);
        db.insert_defaults(
            "msbuild",
            &[
                "*.csproj",
                "*.fsproj",
                "*.proj",
                "*.props",
                "*.sln",
                "*.slnf",
                "*.targets",
                "*.vcxproj",
            ],
        );
        db.insert_defaults("nim", &["*.nim", "*.nimble", "*.nimf", "*.nims"]);
        db.insert_defaults("nix", &["*.nix"]);
        db.insert_defaults("objc", &["*.h", "*.m"]);
        db.insert_defaults("objcpp", &["*.h", "*.mm"]);
        db.insert_defaults("ocaml", &["*.ml", "*.mli", "*.mll", "*.mly"]);
        db.insert_defaults("org", &["*.org", "*.org_archive"]);
        db.insert_defaults("pants", &["BUILD"]);
        db.insert_defaults("pascal", &["*.dpr", "*.inc", "*.lpr", "*.pas", "*.pp"]);
        db.insert_defaults("pdf", &["*.pdf"]);
        db.insert_defaults("py", &["*.py", "*.pyi", "*.pyw"]);
        db.insert_defaults(
            "php",
            &[
                "*.php", "*.php3", "*.php4", "*.php5", "*.php7", "*.php8", "*.pht", "*.phtml",
            ],
        );
        db.insert_defaults(
            "perl",
            &["*.PL", "*.perl", "*.pl", "*.plh", "*.plx", "*.pm", "*.t"],
        );
        db.insert_defaults("po", &["*.po"]);
        db.insert_defaults("pod", &["*.pod"]);
        db.insert_defaults("postscript", &["*.eps", "*.ps"]);
        db.insert_defaults("prolog", &["*.P", "*.pl", "*.pro", "*.prolog"]);
        db.insert_defaults("protobuf", &["*.proto"]);
        db.insert_defaults("ps", &["*.cdxml", "*.ps1", "*.ps1xml", "*.psd1", "*.psm1"]);
        db.insert_defaults("puppet", &["*.epp", "*.erb", "*.pp", "*.rb"]);
        db.insert_defaults("purs", &["*.purs"]);
        db.insert_defaults("python", &["*.py", "*.pyi", "*.pyw"]);
        db.insert_defaults("qmake", &["*.prf", "*.pri", "*.pro"]);
        db.insert_defaults("qml", &["*.qml"]);
        db.insert_defaults("qrc", &["*.qrc"]);
        db.insert_defaults("qui", &["*.ui"]);
        db.insert_defaults("r", &["*.R", "*.Rmd", "*.Rnw", "*.r"]);
        db.insert_defaults("racket", &["*.rkt"]);
        db.insert_defaults(
            "raku",
            &[
                "*.p6",
                "*.pl6",
                "*.pm6",
                "*.raku",
                "*.rakudoc",
                "*.rakumod",
                "*.rakutest",
            ],
        );
        db.insert_defaults("rdoc", &["*.rdoc"]);
        db.insert_defaults("readme", &["*README", "README*"]);
        db.insert_defaults("reasonml", &["*.re", "*.rei"]);
        db.insert_defaults("red", &["*.r", "*.red", "*.reds"]);
        db.insert_defaults("rescript", &["*.res", "*.resi"]);
        db.insert_defaults("robot", &["*.robot"]);
        db.insert_defaults("rst", &["*.rst"]);
        db.insert_defaults(
            "ruby",
            &[
                "*.gemspec",
                "*.rake",
                "*.rb",
                "*.rbw",
                ".irbrc",
                "Gemfile",
                "Rakefile",
                "config.ru",
            ],
        );
        db.insert_defaults("rs", &["*.rs"]);
        db.insert_defaults("rust", &["*.rs"]);
        db.insert_defaults("sass", &["*.sass", "*.scss"]);
        db.insert_defaults("scala", &["*.sbt", "*.scala"]);
        db.insert_defaults("scdoc", &["*.scd", "*.scdoc"]);
        db.insert_defaults("seed7", &["*.s7i", "*.sd7"]);
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
        db.insert_defaults("slim", &["*.skim", "*.slim", "*.slime"]);
        db.insert_defaults("smarty", &["*.tpl"]);
        db.insert_defaults("sml", &["*.sig", "*.sml"]);
        db.insert_defaults("text", &["*.txt"]);
        db.insert_defaults("txt", &["*.txt"]);
        db.insert_defaults("toml", &["*.toml"]);
        db.insert_defaults("spark", &["*.spark"]);
        db.insert_defaults("spec", &["*.spec"]);
        db.insert_defaults("ssa", &["*.ssa"]);
        db.insert_defaults(
            "tf",
            &[
                "*.terraform.lock.hcl",
                "*.terraformrc",
                "*.tf",
                "*.tf.json",
                "*.tfrc",
                "*.auto.tfvars",
                "*.auto.tfvars.json",
                "terraform.rc",
                "terraform.tfvars",
                "terraform.tfvars.json",
            ],
        );
        db.insert_defaults("ts", &["*.cts", "*.mts", "*.ts", "*.tsx"]);
        db.insert_defaults("typescript", &["*.cts", "*.mts", "*.ts", "*.tsx"]);
        db.insert_defaults("sql", &["*.psql", "*.sql"]);
        db.insert_defaults("solidity", &["*.sol"]);
        db.insert_defaults("soy", &["*.soy"]);
        db.insert_defaults("stylus", &["*.styl"]);
        db.insert_defaults("sv", &["*.h", "*.sv", "*.svh", "*.v", "*.vg"]);
        db.insert_defaults("svg", &["*.svg"]);
        db.insert_defaults("swift", &["*.swift"]);
        db.insert_defaults("swig", &["*.def", "*.i"]);
        db.insert_defaults(
            "systemd",
            &[
                "*.automount",
                "*.conf",
                "*.device",
                "*.link",
                "*.mount",
                "*.path",
                "*.scope",
                "*.service",
                "*.slice",
                "*.socket",
                "*.swap",
                "*.target",
                "*.timer",
            ],
        );
        db.insert_defaults("svelte", &["*.svelte", "*.svelte.ts"]);
        db.insert_defaults("taskpaper", &["*.taskpaper"]);
        db.insert_defaults("tcl", &["*.tcl"]);
        db.insert_defaults(
            "tex",
            &[
                "*.bib", "*.cls", "*.dtx", "*.ins", "*.ltx", "*.sty", "*.tex",
            ],
        );
        db.insert_defaults("texinfo", &["*.texi"]);
        db.insert_defaults("textile", &["*.textile"]);
        db.insert_defaults("thrift", &["*.thrift"]);
        db.insert_defaults("twig", &["*.twig"]);
        db.insert_defaults("typoscript", &["*.ts", "*.typoscript"]);
        db.insert_defaults("typst", &["*.typ"]);
        db.insert_defaults("usd", &["*.usd", "*.usda", "*.usdc"]);
        db.insert_defaults("v", &["*.v", "*.vsh"]);
        db.insert_defaults("vala", &["*.vala"]);
        db.insert_defaults("vb", &["*.vb"]);
        db.insert_defaults("vcl", &["*.vcl"]);
        db.insert_defaults("verilog", &["*.sv", "*.svh", "*.v", "*.vh"]);
        db.insert_defaults("vhdl", &["*.vhd", "*.vhdl"]);
        db.insert_defaults(
            "vim",
            &[
                "*.vim", ".gvimrc", ".vimrc", "_gvimrc", "_vimrc", "gvimrc", "vimrc",
            ],
        );
        db.insert_defaults(
            "vimscript",
            &[
                "*.vim", ".gvimrc", ".vimrc", "_gvimrc", "_vimrc", "gvimrc", "vimrc",
            ],
        );
        db.insert_defaults("webidl", &["*.idl", "*.webidl", "*.widl"]);
        db.insert_defaults("wgsl", &["*.wgsl"]);
        db.insert_defaults("wiki", &["*.mediawiki", "*.wiki"]);
        db.insert_defaults("js", &["*.cjs", "*.js", "*.jsx", "*.mjs", "*.vue"]);
        db.insert_defaults("javascript", &["*.cjs", "*.js", "*.jsx", "*.mjs", "*.vue"]);
        db.insert_defaults("vue", &["*.vue"]);
        db.insert_defaults(
            "xml",
            &[
                "*.dtd",
                "*.rng",
                "*.sch",
                "*.xhtml",
                "*.xjb",
                "*.xml",
                "*.xml.dist",
                "*.xsd",
                "*.xsl",
                "*.xslt",
            ],
        );
        db.insert_defaults("yaml", &["*.yaml", "*.yml"]);
        db.insert_defaults("xz", &["*.txz", "*.xz"]);
        db.insert_defaults("yacc", &["*.y"]);
        db.insert_defaults("yang", &["*.yang"]);
        db.insert_defaults("yml", &["*.yaml", "*.yml"]);
        db.insert_defaults("z", &["*.Z"]);
        db.insert_defaults("zig", &["*.zig"]);
        db.insert_defaults(
            "zsh",
            &[
                "*.zsh",
                ".zlogin",
                ".zlogout",
                ".zprofile",
                ".zshenv",
                ".zshrc",
                "zlogin",
                "zlogout",
                "zprofile",
                "zshenv",
                "zshrc",
            ],
        );
        db.insert_defaults("zstd", &["*.zst", "*.zstd"]);
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
        if name == "all" {
            return Ok(RgFileType {
                globs: self
                    .definitions
                    .values()
                    .flat_map(|globs| globs.iter().cloned())
                    .collect(),
            });
        }

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
    fn parse(pattern: &str, iglob: bool, case_insensitive: bool) -> Result<Self> {
        let (include, raw_pattern) = match pattern.strip_prefix('!') {
            Some(rest) => (false, rest),
            None => (true, pattern),
        };
        let normalized = raw_pattern.strip_prefix("./").unwrap_or(raw_pattern);
        let has_slash = normalized.contains('/');
        let anchored = normalized.starts_with('/');
        let regex = build_regex_opts(&glob_to_regex(normalized), case_insensitive)
            .map_err(|e| Error::Execution(format!("rg: invalid --glob value: {}", e)))?;
        Ok(Self {
            raw: normalized.to_string(),
            include,
            has_slash,
            anchored,
            iglob,
            case_insensitive,
            regex,
        })
    }

    fn set_case_insensitive(&mut self, case_insensitive: bool) -> Result<()> {
        self.regex = build_regex_opts(&glob_to_regex(&self.raw), case_insensitive)
            .map_err(|e| Error::Execution(format!("rg: invalid --glob value: {}", e)))?;
        self.case_insensitive = case_insensitive;
        Ok(())
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

const RG_GLOB_MAX_BRACE_DEPTH: usize = 32;

fn glob_to_regex(pattern: &str) -> String {
    glob_to_regex_with_depth(pattern, 0)
}

fn glob_to_regex_with_depth(pattern: &str, depth: usize) -> String {
    if depth >= RG_GLOB_MAX_BRACE_DEPTH {
        return format!("^{}$", regex::escape(pattern.trim_start_matches('/')));
    }

    let mut out = String::new();
    out.push('^');

    let chars: Vec<char> = pattern.trim_start_matches('/').chars().collect();
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '*' if i + 2 < chars.len() && chars[i + 1] == '*' && chars[i + 2] == '/' => {
                out.push_str("(?:.*/)?");
                i += 3;
            }
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
            '[' => {
                if let Some((class, next)) = glob_class_to_regex(&chars, i) {
                    out.push_str(&class);
                    i = next;
                } else {
                    out.push_str(r"\[");
                    i += 1;
                }
            }
            '{' => {
                if let Some((alternation, next)) = glob_alternation_to_regex(&chars, i, depth + 1) {
                    out.push_str(&alternation);
                    i = next;
                } else {
                    out.push_str(r"\{");
                    i += 1;
                }
            }
            '\\' if i + 1 < chars.len() => {
                out.push_str(&regex::escape(&chars[i + 1].to_string()));
                i += 2;
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

fn glob_alternation_to_regex(
    chars: &[char],
    start: usize,
    recursion_depth: usize,
) -> Option<(String, usize)> {
    if recursion_depth >= RG_GLOB_MAX_BRACE_DEPTH {
        return None;
    }
    let mut alts = Vec::new();
    let mut current = String::new();
    let mut depth = 0usize;
    let mut saw_comma = false;
    let mut i = start + 1;

    while i < chars.len() {
        match chars[i] {
            '\\' if i + 1 < chars.len() => {
                current.push('\\');
                current.push(chars[i + 1]);
                i += 2;
                continue;
            }
            '{' => {
                depth += 1;
                current.push('{');
            }
            '}' if depth == 0 => {
                if !saw_comma {
                    return None;
                }
                alts.push(current);
                let mut out = String::from("(?:");
                for (idx, alt) in alts.iter().enumerate() {
                    if idx > 0 {
                        out.push('|');
                    }
                    let alt_regex = glob_to_regex_with_depth(alt, recursion_depth);
                    out.push_str(&alt_regex[1..alt_regex.len() - 1]);
                }
                out.push(')');
                return Some((out, i + 1));
            }
            '}' => {
                depth -= 1;
                current.push('}');
            }
            ',' if depth == 0 => {
                saw_comma = true;
                alts.push(std::mem::take(&mut current));
            }
            c => current.push(c),
        }
        i += 1;
    }
    None
}

fn glob_class_to_regex(chars: &[char], start: usize) -> Option<(String, usize)> {
    let mut i = start + 1;
    let mut negated = false;
    if i >= chars.len() {
        return None;
    }
    if matches!(chars[i], '!' | '^') {
        negated = true;
        i += 1;
    }
    if i >= chars.len() {
        return None;
    }

    let mut body = String::new();
    let mut saw_member = false;
    if chars[i] == ']' {
        body.push_str(r"\]");
        saw_member = true;
        i += 1;
    }

    while i < chars.len() {
        let c = chars[i];
        if c == ']' {
            if !saw_member {
                return None;
            }
            let mut out = String::from("[");
            if negated {
                out.push('^');
            }
            out.push_str(&body);
            out.push(']');
            return Some((out, i + 1));
        }
        saw_member = true;
        if c == '-' && chars.get(i + 1) == Some(&'-') && !body.is_empty() {
            // Preserve glob range validation by avoiding Rust regex `--` set difference syntax.
            body.push_str(r"-\-");
            i += 2;
            continue;
        }
        push_glob_class_char(&mut body, c, chars.get(i + 1).copied());
        i += 1;
    }
    None
}

fn push_glob_class_char(out: &mut String, c: char, next: Option<char>) {
    match c {
        '\\' => out.push_str(r"\\"),
        '[' => out.push_str(r"\["),
        ']' => out.push_str(r"\]"),
        '^' if out.is_empty() => out.push_str(r"\^"),
        '-' if out.is_empty() || next == Some(']') => out.push_str(r"\-"),
        '&' => out.push_str(r"\&"),
        '~' => out.push_str(r"\~"),
        _ => out.push(c),
    }
}

fn parse_context_value(value: &str, flag: &str) -> Result<usize> {
    value
        .parse()
        .map_err(|_| Error::Execution(format!("rg: invalid {} value: {}", flag, value)))
}

fn parse_max_depth(value: &str, flag: &str) -> Result<usize> {
    value
        .parse()
        .map_err(|_| Error::Execution(format!("rg: invalid {flag} value: {value}")))
}

fn parse_max_filesize(value: &str) -> Result<u64> {
    let (digits, multiplier) = match value.as_bytes().last().copied() {
        Some(b'K') => (&value[..value.len() - 1], 1024_u64),
        Some(b'M') => (&value[..value.len() - 1], 1024_u64 * 1024),
        Some(b'G') => (&value[..value.len() - 1], 1024_u64 * 1024 * 1024),
        _ => (value, 1),
    };

    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return Err(Error::Execution(format!(
            "rg: error parsing flag --max-filesize: invalid size: invalid format for size '{value}', which should be a non-empty sequence of digits followed by an optional 'K', 'M' or 'G' suffix"
        )));
    }

    digits
        .parse::<u64>()
        .ok()
        .and_then(|size| size.checked_mul(multiplier))
        .ok_or_else(|| {
            Error::Execution(format!(
                "rg: error parsing flag --max-filesize: invalid size: size '{value}' is too large"
            ))
        })
}

fn parse_threads(value: &str, flag: &str) -> Result<()> {
    value.parse::<usize>().map(|_| ()).map_err(|_| {
        Error::Execution(format!(
            "rg: error parsing flag {flag}: value is not a valid number: invalid digit found in string"
        ))
    })
}

fn parse_noop_size_limit(value: &str, flag: &str) -> Result<()> {
    parse_max_filesize(value).map(|_| ()).map_err(|_| {
        Error::Execution(format!(
            "rg: error parsing flag {flag}: invalid size limit: {value}"
        ))
    })
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

fn parse_sort_value(value: &str, flag: &str) -> Result<RgSort> {
    match value {
        "path" => Ok(RgSort::Path),
        "modified" => Ok(RgSort::Modified),
        "accessed" => Ok(RgSort::Accessed),
        "created" => Ok(RgSort::Created),
        "none" => Ok(RgSort::None),
        _ => Err(Error::Execution(format!(
            "rg: error parsing flag {flag}: choice '{value}' is unrecognized"
        ))),
    }
}

fn parse_color_mode(value: &str) -> Result<RgColorMode> {
    match value {
        "auto" => Ok(RgColorMode::Auto),
        "never" => Ok(RgColorMode::Never),
        "always" | "ansi" => Ok(RgColorMode::Always),
        _ => Err(Error::Execution(format!(
            "rg: error parsing flag --color: invalid choice '{value}'"
        ))),
    }
}

fn parse_hyperlink_format(value: &str) -> Result<Option<String>> {
    let template = match value {
        "none" => return Ok(None),
        "default" | "file" | "kitty" => "file://{path}",
        "cursor" => "cursor://file{path}:{line}:{column}",
        "macvim" => "mvim://open?url=file://{path}&line={line}&column={column}",
        "textmate" => "txmt://open?url=file://{path}&line={line}&column={column}",
        "vscode" => "vscode://file{path}:{line}:{column}",
        "vscode-insiders" => "vscode-insiders://file{path}:{line}:{column}",
        "vscodium" => "vscodium://file{path}:{line}:{column}",
        "grep+" => "grep+://{path}:{line}:{column}",
        custom if is_valid_hyperlink_template(custom) => custom,
        _ => {
            return Err(Error::Execution(format!(
                "rg: error parsing flag --hyperlink-format: invalid hyperlink format: {value}"
            )));
        }
    };
    Ok(Some(template.to_string()))
}

fn is_valid_hyperlink_template(value: &str) -> bool {
    value.contains("{path}")
        && value
            .split_once(':')
            .is_some_and(|(scheme, _)| is_valid_url_scheme(scheme))
}

fn is_valid_url_scheme(scheme: &str) -> bool {
    let mut chars = scheme.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    first.is_ascii_alphabetic()
        && chars.all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '+' | '-' | '.'))
}

fn parse_ansi_fg(value: &str) -> Result<String> {
    if let Some(rgb) = parse_rgb_color(value) {
        return Ok(format!("38;2;{};{};{}", rgb.0, rgb.1, rgb.2));
    }
    if let Some(number) = parse_ansi_color_number(value) {
        return Ok(format!("38;5;{number}"));
    }
    match value {
        "black" => Ok("30".to_string()),
        "red" => Ok("31".to_string()),
        "green" => Ok("32".to_string()),
        "yellow" => Ok("33".to_string()),
        "blue" => Ok("34".to_string()),
        "magenta" => Ok("35".to_string()),
        "cyan" => Ok("36".to_string()),
        "white" => Ok("37".to_string()),
        _ => Err(Error::Execution(format!(
            "rg: error parsing flag --colors: invalid foreground color '{value}'"
        ))),
    }
}

fn parse_ansi_bg(value: &str) -> Result<String> {
    if let Some(rgb) = parse_rgb_color(value) {
        return Ok(format!("48;2;{};{};{}", rgb.0, rgb.1, rgb.2));
    }
    if let Some(number) = parse_ansi_color_number(value) {
        return Ok(format!("48;5;{number}"));
    }
    match value {
        "black" => Ok("40".to_string()),
        "red" => Ok("41".to_string()),
        "green" => Ok("42".to_string()),
        "yellow" => Ok("43".to_string()),
        "blue" => Ok("44".to_string()),
        "magenta" => Ok("45".to_string()),
        "cyan" => Ok("46".to_string()),
        "white" => Ok("47".to_string()),
        _ => Err(Error::Execution(format!(
            "rg: error parsing flag --colors: invalid background color '{value}'"
        ))),
    }
}

fn parse_rgb_color(value: &str) -> Option<(u8, u8, u8)> {
    let parts: Vec<&str> = value.split(',').collect();
    if parts.len() != 3 {
        return None;
    }
    let r = parts[0].parse().ok()?;
    let g = parts[1].parse().ok()?;
    let b = parts[2].parse().ok()?;
    Some((r, g, b))
}

fn parse_ansi_color_number(value: &str) -> Option<u8> {
    if let Some(hex) = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
    {
        return u8::from_str_radix(hex, 16).ok();
    }
    if value.chars().all(|ch| ch.is_ascii_digit()) {
        value.parse().ok()
    } else {
        None
    }
}

fn parse_rg_separator(value: &str) -> String {
    let mut output = Vec::new();
    let mut chars = value.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            let mut buf = [0; 4];
            output.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
            continue;
        }

        let Some(escaped) = chars.next() else {
            output.push(b'\\');
            break;
        };

        match escaped {
            'n' => output.push(b'\n'),
            'r' => output.push(b'\r'),
            't' => output.push(b'\t'),
            '\\' => output.push(b'\\'),
            'x' => {
                let first = chars.peek().copied();
                let second = {
                    let mut iter = chars.clone();
                    iter.next();
                    iter.peek().copied()
                };
                if let (Some(a), Some(b)) = (first, second)
                    && a.is_ascii_hexdigit()
                    && b.is_ascii_hexdigit()
                {
                    chars.next();
                    chars.next();
                    let hex = format!("{a}{b}");
                    if let Ok(byte) = u8::from_str_radix(&hex, 16) {
                        output.push(byte);
                    }
                } else {
                    output.extend_from_slice(b"\\x");
                }
            }
            other => {
                output.push(b'\\');
                let mut buf = [0; 4];
                output.extend_from_slice(other.encode_utf8(&mut buf).as_bytes());
            }
        }
    }
    String::from_utf8_lossy(&output).into_owned()
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

const RG_GZIP_MAX_DECOMPRESSED_BYTES: usize = 10 * 1024 * 1024;
const RG_GZIP_MAX_DECOMPRESSION_RATIO: usize = 100;

fn rg_search_bytes(
    path: &Path,
    content: &[u8],
    opts: &RgOptions,
) -> std::result::Result<Vec<u8>, String> {
    if !opts.search_zip || !is_gzip_path(path) {
        return Ok(content.to_vec());
    }

    let mut decoder = flate2::read::GzDecoder::new(content);
    let mut decompressed = Vec::new();
    let mut chunk = [0u8; 8192];

    loop {
        let n = decoder
            .read(&mut chunk)
            .map_err(|e| format!("gzip decompression failed: {e}"))?;
        if n == 0 {
            break;
        }

        decompressed.extend_from_slice(&chunk[..n]);

        if decompressed.len() > RG_GZIP_MAX_DECOMPRESSED_BYTES {
            return Err(format!(
                "gzip decompression exceeds {} byte limit",
                RG_GZIP_MAX_DECOMPRESSED_BYTES
            ));
        }

        if !content.is_empty()
            && decompressed.len() > content.len() * RG_GZIP_MAX_DECOMPRESSION_RATIO
        {
            return Err(format!(
                "gzip decompression ratio exceeds {}:1",
                RG_GZIP_MAX_DECOMPRESSION_RATIO
            ));
        }
    }

    Ok(decompressed)
}

fn is_gzip_path(path: &Path) -> bool {
    // Real rg's -z path decides decompression from recognized file names, not gzip magic bytes.
    path.file_name()
        .and_then(|name| name.to_str())
        .map(|name| {
            let name = name.to_ascii_lowercase();
            name.ends_with(".gz") || name.ends_with(".tgz")
        })
        .unwrap_or(false)
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

fn parse_regex_engine(value: &str) -> Result<bool> {
    match value {
        "default" | "auto" => Ok(false),
        "pcre2" => Ok(true),
        _ => Err(Error::Execution(format!(
            "rg: error parsing flag --engine: unrecognized regex engine '{value}'"
        ))),
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
    cwd: &Path,
    display_path: &str,
    opts: &RgOptions,
) -> std::result::Result<String, ExecResult> {
    let content = fs
        .read_file(path)
        .await
        .map_err(|e| ExecResult::err(format!("rg: {display_path}: {e}\n"), 1))?;

    opts.validate_preprocessor_for(path, cwd)
        .map_err(|e| ExecResult::err(format!("rg: {display_path}: {e}\n"), 1))?;

    let content = rg_search_bytes(path, &content, opts)
        .map_err(|e| ExecResult::err(format!("rg: {display_path}: {e}\n"), 1))?;
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
            return Ok(RgCollectedInputs::new(vec![RgInput::explicit(
                "(stdin)",
                stdin.to_string(),
            )]));
        }

        if let Some(inputs) = try_indexed_search(&*ctx.fs, opts, ctx.cwd).await {
            return Ok(RgCollectedInputs::new(inputs));
        }

        let root = RgSearchRoot {
            logical: ctx.cwd.to_path_buf(),
            actual: ctx.cwd.to_path_buf(),
            display_hint: RgDisplayHint::None,
        };
        let files = collect_rg_files_recursive(&*ctx.fs, &[root], opts, ctx.cwd).await;
        return Ok(read_rg_files(&*ctx.fs, files, ctx.cwd, opts).await);
    }

    if let Some(inputs) = try_indexed_search(&*ctx.fs, opts, ctx.cwd).await {
        return Ok(RgCollectedInputs::new(inputs));
    }

    let mut collected = RgCollectedInputs::default();
    let mut candidates = Vec::new();
    let mut roots = Vec::new();
    for p in &opts.paths {
        let path = resolve_path(ctx.cwd, p);
        match resolve_rg_explicit_path(&*ctx.fs, &path, opts.follow_symlinks).await {
            Some((actual_path, meta)) if meta.file_type.is_dir() => {
                roots.push(RgSearchRoot {
                    logical: path,
                    actual: actual_path,
                    display_hint: RgDisplayHint::from_root_arg(Some(p)),
                });
            }
            Some((actual_path, meta)) => {
                candidates.push(RgFileCandidate {
                    logical: path,
                    actual: actual_path,
                    metadata: meta,
                    display_hint: RgDisplayHint::from_root_arg(Some(p)),
                    display_override: Some(apply_path_separator_to_display(p, opts)),
                    explicit: true,
                });
            }
            None => {
                let text = match read_rg_text_file(&*ctx.fs, &path, ctx.cwd, p, opts).await {
                    Ok(t) => t,
                    Err(e) => {
                        collected.had_errors = true;
                        if opts.messages {
                            collected.stderr.push_str(&e.stderr);
                        }
                        continue;
                    }
                };
                collected.inputs.push(RgInput::explicit(
                    apply_path_separator_to_display(p, opts),
                    text,
                ));
            }
        }
    }
    if !roots.is_empty() {
        candidates.extend(collect_rg_files_recursive(&*ctx.fs, &roots, opts, ctx.cwd).await);
    }
    if opts.sort != RgSort::Path || opts.sort_reverse {
        sort_rg_candidates(&mut candidates, opts);
    }
    let read = read_rg_files(&*ctx.fs, candidates, ctx.cwd, opts).await;
    collected.had_errors |= read.had_errors;
    collected.stderr.push_str(&read.stderr);
    collected.inputs.extend(read.inputs);
    Ok(collected)
}

#[derive(Default)]
struct RgCollectedInputs {
    inputs: Vec<RgInput>,
    stderr: String,
    had_errors: bool,
}

struct RgInput {
    display: String,
    content: String,
    explicit: bool,
}

impl RgInput {
    fn explicit(display: impl Into<String>, content: String) -> Self {
        Self {
            display: display.into(),
            content,
            explicit: true,
        }
    }

    fn discovered(display: impl Into<String>, content: String) -> Self {
        Self {
            display: display.into(),
            content,
            explicit: false,
        }
    }
}

impl RgCollectedInputs {
    fn new(inputs: Vec<RgInput>) -> Self {
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
    if opts.no_ignore_files {
        return Ok(());
    }

    for ignore_file in opts.ignore_file_paths.clone() {
        let path = resolve_path(cwd, &ignore_file);
        let content = match read_rg_ignore_file(fs, &path).await {
            Ok(content) => content,
            Err(Error::Execution(message)) if message.starts_with("rg: ignore file") => {
                return Err(ExecResult::err(format!("{message}\n"), 2));
            }
            Err(err) => {
                return Err(ExecResult::err(
                    format!("rg: {}: {err}\n", path.display()),
                    1,
                ));
            }
        };
        let content = String::from_utf8_lossy(&content);
        let rules = parse_rg_ignore_rules(&content, cwd, opts.ignore_file_case_insensitive)
            .map_err(|e| ExecResult::err(format!("{}\n", e), 2))?;
        append_rg_ignore_rules_with_base(
            &mut opts.explicit_ignore_rules,
            rules,
            opts.global_ignore_rules.len(),
        )
        .map_err(|e| ExecResult::err(format!("{}\n", e), 2))?;
    }
    Ok(())
}

async fn load_rg_global_ignore_files(
    fs: &dyn crate::fs::FileSystem,
    env: &std::collections::HashMap<String, String>,
    opts: &mut RgOptions,
) -> Result<()> {
    if opts.no_ignore || opts.no_ignore_global {
        return Ok(());
    }

    let mut configured_paths = Vec::new();
    if let Some(home) = non_empty_env_path(env, "HOME") {
        configured_paths.extend(git_config_excludes_files(fs, &home.join(".gitconfig")).await?);
    }

    if configured_paths.is_empty() {
        if let Some(path) = default_git_global_ignore_path(env) {
            load_optional_ignore_file(
                fs,
                &path,
                Path::new("/"),
                opts.ignore_file_case_insensitive,
                &mut opts.global_ignore_rules,
            )
            .await?;
        }
    } else {
        for path in configured_paths {
            load_optional_ignore_file(
                fs,
                &path,
                Path::new("/"),
                opts.ignore_file_case_insensitive,
                &mut opts.global_ignore_rules,
            )
            .await?;
        }
    }

    Ok(())
}

fn non_empty_env_path(
    env: &std::collections::HashMap<String, String>,
    key: &str,
) -> Option<PathBuf> {
    env.get(key)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn default_git_global_ignore_path(
    env: &std::collections::HashMap<String, String>,
) -> Option<PathBuf> {
    non_empty_env_path(env, "XDG_CONFIG_HOME")
        .map(|path| path.join("git/ignore"))
        .or_else(|| non_empty_env_path(env, "HOME").map(|path| path.join(".config/git/ignore")))
}

async fn git_config_excludes_files(
    fs: &dyn crate::fs::FileSystem,
    config_path: &Path,
) -> Result<Vec<PathBuf>> {
    let Ok(content) = fs.read_file(config_path).await else {
        return Ok(Vec::new());
    };
    let content = String::from_utf8_lossy(&content);
    let home = config_path.parent().unwrap_or(Path::new("/"));
    Ok(parse_git_config_excludes_files(&content, home))
}

fn parse_git_config_excludes_files(content: &str, home: &Path) -> Vec<PathBuf> {
    let mut in_core = false;
    let mut paths = Vec::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            in_core = line
                .trim_matches(['[', ']'])
                .split_whitespace()
                .next()
                .is_some_and(|section| section.eq_ignore_ascii_case("core"));
            continue;
        }
        if !in_core {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if key.trim().eq_ignore_ascii_case("excludesFile") {
            paths.push(expand_git_config_path(value.trim(), home));
        }
    }
    paths
}

fn expand_git_config_path(value: &str, home: &Path) -> PathBuf {
    let value = value.trim_matches('"');
    if value == "~" {
        return home.to_path_buf();
    }
    if let Some(rest) = value.strip_prefix("~/") {
        return home.join(rest);
    }
    PathBuf::from(value)
}

fn parse_rg_ignore_rules(
    content: &str,
    base: &Path,
    case_insensitive: bool,
) -> Result<Vec<RgIgnoreRule>> {
    let mut rules = Vec::new();
    for line in content.lines() {
        if let Some(rule) = RgIgnoreRule::parse(line, base, case_insensitive)? {
            if rules.len() >= RG_IGNORE_RULES_MAX_PER_FILE {
                return Err(Error::Execution(format!(
                    "rg: ignore file has too many rules (max {})",
                    RG_IGNORE_RULES_MAX_PER_FILE
                )));
            }
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
    inherited_rule_count: usize,
    rules: &mut Vec<RgIgnoreRule>,
) -> Result<()> {
    if opts.no_ignore {
        return Ok(());
    }

    // Ignore evaluation is last-match-wins, so load lower-precedence files first.
    if !opts.no_ignore_vcs && (!opts.require_git || has_git_dir_in_ancestors(fs, dir, root).await) {
        if !opts.no_ignore_exclude {
            load_optional_ignore_file_with_rule_base(
                fs,
                &dir.join(".git/info/exclude"),
                dir,
                opts.ignore_file_case_insensitive,
                inherited_rule_count,
                rules,
            )
            .await?;
        }
        load_optional_ignore_file_with_rule_base(
            fs,
            &dir.join(".gitignore"),
            dir,
            opts.ignore_file_case_insensitive,
            inherited_rule_count,
            rules,
        )
        .await?;
    }
    if !opts.no_ignore_dot {
        load_optional_ignore_file_with_rule_base(
            fs,
            &dir.join(".ignore"),
            dir,
            opts.ignore_file_case_insensitive,
            inherited_rule_count,
            rules,
        )
        .await?;
        load_optional_ignore_file_with_rule_base(
            fs,
            &dir.join(".rgignore"),
            dir,
            opts.ignore_file_case_insensitive,
            inherited_rule_count,
            rules,
        )
        .await?;
    }
    Ok(())
}

async fn load_parent_ignore_rules(
    fs: &dyn crate::fs::FileSystem,
    dir: &Path,
    opts: &RgOptions,
    rules: &mut Vec<RgIgnoreRule>,
) -> Result<()> {
    if opts.no_ignore || opts.no_ignore_parent {
        return Ok(());
    }

    let Some(parent) = dir.parent() else {
        return Ok(());
    };
    let mut ancestors: Vec<PathBuf> = parent.ancestors().map(Path::to_path_buf).collect();
    ancestors.reverse();

    for ancestor in ancestors {
        load_local_ignore_rules(fs, &ancestor, Path::new("/"), opts, 0, rules).await?;
    }
    Ok(())
}

async fn append_global_ignore_rules_for_root(
    fs: &dyn crate::fs::FileSystem,
    root: &Path,
    opts: &RgOptions,
    rules: &mut Vec<RgIgnoreRule>,
) {
    if opts.no_ignore
        || opts.no_ignore_global
        || opts.global_ignore_rules.is_empty()
        || (opts.require_git && !has_git_dir_in_ancestors(fs, root, Path::new("/")).await)
    {
        return;
    }
    rules.extend(opts.global_ignore_rules.clone());
}

async fn load_optional_ignore_file(
    fs: &dyn crate::fs::FileSystem,
    path: &Path,
    base: &Path,
    case_insensitive: bool,
    rules: &mut Vec<RgIgnoreRule>,
) -> Result<()> {
    load_optional_ignore_file_with_rule_base(fs, path, base, case_insensitive, 0, rules).await
}

async fn load_optional_ignore_file_with_rule_base(
    fs: &dyn crate::fs::FileSystem,
    path: &Path,
    base: &Path,
    case_insensitive: bool,
    inherited_rule_count: usize,
    rules: &mut Vec<RgIgnoreRule>,
) -> Result<()> {
    let content = match read_rg_ignore_file(fs, path).await {
        Ok(content) => content,
        Err(Error::Execution(message)) if message.starts_with("rg: ignore file") => {
            return Err(Error::Execution(message));
        }
        Err(_) => return Ok(()),
    };
    let content = String::from_utf8_lossy(&content);
    let parsed = parse_rg_ignore_rules(&content, base, case_insensitive)?;
    append_rg_ignore_rules_with_base(rules, parsed, inherited_rule_count)
}

async fn read_rg_ignore_file(fs: &dyn crate::fs::FileSystem, path: &Path) -> Result<Vec<u8>> {
    if let Ok(meta) = fs.stat(path).await {
        validate_rg_ignore_file_size(path, meta.size)?;
    }
    let content = fs.read_file(path).await?;
    validate_rg_ignore_file_size(path, content.len() as u64)?;
    Ok(content)
}

fn validate_rg_ignore_file_size(path: &Path, len: u64) -> Result<()> {
    if len > RG_IGNORE_FILE_MAX_BYTES as u64 {
        return Err(Error::Execution(format!(
            "rg: ignore file too large (max {} bytes): {}",
            RG_IGNORE_FILE_MAX_BYTES,
            path.display()
        )));
    }
    Ok(())
}

fn append_rg_ignore_rules_with_base(
    rules: &mut Vec<RgIgnoreRule>,
    parsed: Vec<RgIgnoreRule>,
    inherited_rule_count: usize,
) -> Result<()> {
    let Some(total) = inherited_rule_count
        .checked_add(rules.len())
        .and_then(|count| count.checked_add(parsed.len()))
    else {
        return Err(Error::Execution("rg: too many ignore rules".to_string()));
    };
    if total > RG_IGNORE_RULES_MAX_TOTAL {
        return Err(Error::Execution(format!(
            "rg: too many ignore rules (max {})",
            RG_IGNORE_RULES_MAX_TOTAL
        )));
    }
    rules.extend(parsed);
    Ok(())
}

#[cfg(test)]
fn test_ignore_content_with_n_rules(n: usize) -> String {
    (0..n)
        .map(|i| format!("rule-{i}"))
        .collect::<Vec<_>>()
        .join("\n")
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

async fn has_directory_path(
    fs: &dyn crate::fs::FileSystem,
    cwd: &Path,
    paths: &[String],
    follow_symlinks: bool,
) -> bool {
    for p in paths {
        let path = resolve_path(cwd, p);
        if let Some((_, meta)) = resolve_rg_explicit_path(fs, &path, follow_symlinks).await
            && meta.file_type.is_dir()
        {
            return true;
        }
    }
    false
}

#[derive(Clone)]
struct RgSearchRoot {
    logical: PathBuf,
    actual: PathBuf,
    /// Compact display hint derived from user arg; avoids per-file String clones.
    display_hint: RgDisplayHint,
}

#[derive(Clone)]
struct RgFileCandidate {
    logical: PathBuf,
    actual: PathBuf,
    metadata: crate::fs::Metadata,
    display_hint: RgDisplayHint,
    display_override: Option<String>,
    explicit: bool,
}

struct RgWalkItem {
    logical: PathBuf,
    actual: PathBuf,
    actual_root: PathBuf,
    depth: usize,
    rules: Arc<RgIgnoreRuleSet>,
    ancestors: Vec<PathBuf>,
    display_hint: RgDisplayHint,
}

#[derive(Clone, Copy)]
enum RgDisplayHint {
    None,
    DotSlash,
    Absolute,
}

impl RgDisplayHint {
    fn from_root_arg(root_arg: Option<&str>) -> Self {
        match root_arg {
            Some(arg) if arg.starts_with('/') => Self::Absolute,
            Some(arg) if arg == "." || arg.starts_with("./") => Self::DotSlash,
            _ => Self::None,
        }
    }

    fn root_arg(self) -> Option<&'static str> {
        match self {
            Self::None => None,
            Self::DotSlash => Some("./"),
            Self::Absolute => Some("/"),
        }
    }
}

async fn resolve_rg_symlink_target(
    fs: &dyn crate::fs::FileSystem,
    link_path: &Path,
    containment_root: &Path,
) -> Option<(PathBuf, crate::fs::Metadata)> {
    let target = fs.read_link(link_path).await.ok()?;
    let resolved = if target.is_absolute() {
        crate::fs::normalize_path(&target)
    } else {
        crate::fs::normalize_path(&link_path.parent().unwrap_or(Path::new("/")).join(target))
    };
    // rg may emulate ripgrep's -L behavior only inside the requested VFS search root;
    // rejecting escapes preserves TM-ESC-002's inert-symlink sandbox boundary.
    if !resolved.starts_with(containment_root) {
        return None;
    }
    let metadata = fs.stat(&resolved).await.ok()?;
    Some((resolved, metadata))
}

async fn resolve_rg_explicit_path(
    fs: &dyn crate::fs::FileSystem,
    path: &Path,
    follow_symlinks: bool,
) -> Option<(PathBuf, crate::fs::Metadata)> {
    let meta = fs.stat(path).await.ok()?;
    if meta.file_type.is_symlink() && follow_symlinks {
        let containment_root = path.parent().unwrap_or(Path::new("/"));
        resolve_rg_symlink_target(fs, path, containment_root).await
    } else {
        Some((path.to_path_buf(), meta))
    }
}

async fn collect_rg_files_recursive(
    fs: &dyn crate::fs::FileSystem,
    roots: &[RgSearchRoot],
    opts: &RgOptions,
    cwd: &Path,
) -> Vec<RgFileCandidate> {
    let mut result = Vec::new();
    let mut stack = Vec::new();
    for root in roots {
        let mut rules = Vec::new();
        append_global_ignore_rules_for_root(fs, &root.actual, opts, &mut rules).await;
        rules.extend(opts.explicit_ignore_rules.clone());
        let _ = load_parent_ignore_rules(fs, &root.actual, opts, &mut rules).await;
        stack.push(RgWalkItem {
            logical: root.logical.clone(),
            actual: root.actual.clone(),
            actual_root: root.actual.clone(),
            depth: 0,
            rules: Arc::new(RgIgnoreRuleSet::root(rules)),
            ancestors: vec![root.actual.clone()],
            display_hint: root.display_hint,
        });
    }

    while let Some(item) = stack.pop() {
        let mut local_rules = Vec::new();
        let _ = load_local_ignore_rules(
            fs,
            &item.actual,
            &item.actual_root,
            opts,
            item.rules.len(),
            &mut local_rules,
        )
        .await;
        let rules = if local_rules.is_empty() {
            item.rules.clone()
        } else {
            Arc::new(RgIgnoreRuleSet::child(item.rules.clone(), local_rules))
        };
        if let Ok(entries) = fs.read_dir(&item.actual).await {
            for entry in entries {
                if !opts.hidden && is_hidden_name(&entry.name) {
                    continue;
                }
                let path = item.logical.join(&entry.name);
                let actual_path = item.actual.join(&entry.name);
                let entry_depth = item.depth + 1;
                let (entry_actual_path, entry_metadata) =
                    if entry.metadata.file_type.is_symlink() && opts.follow_symlinks {
                        let Some((target, target_meta)) =
                            resolve_rg_symlink_target(fs, &actual_path, &item.actual_root).await
                        else {
                            continue;
                        };
                        (target, target_meta)
                    } else {
                        (actual_path, entry.metadata)
                    };

                if entry_metadata.file_type.is_dir() {
                    if opts.is_ignored_by_rules(&path, true, &rules) {
                        continue;
                    }
                    if opts
                        .max_depth
                        .is_none_or(|max_depth| entry_depth < max_depth)
                        && !item.ancestors.contains(&entry_actual_path)
                    {
                        let mut child_ancestors = item.ancestors.clone();
                        child_ancestors.push(entry_actual_path.clone());
                        stack.push(RgWalkItem {
                            logical: path,
                            actual: entry_actual_path,
                            actual_root: item.actual_root.clone(),
                            depth: entry_depth,
                            rules: rules.clone(),
                            ancestors: child_ancestors,
                            display_hint: item.display_hint,
                        });
                    }
                } else if entry_metadata.file_type.is_file()
                    && opts
                        .max_depth
                        .is_none_or(|max_depth| entry_depth <= max_depth)
                    && opts.matches_max_filesize(entry_metadata.size)
                    && !opts.is_ignored_by_rules(&path, false, &rules)
                    && opts.matches_globs(&path, cwd)
                    && opts.matches_type_filters(&path)
                {
                    result.push(RgFileCandidate {
                        logical: path,
                        actual: entry_actual_path,
                        metadata: entry_metadata,
                        display_hint: item.display_hint,
                        display_override: None,
                        explicit: false,
                    });
                }
            }
        } else if let Ok(meta) = fs.stat(&item.actual).await
            && meta.file_type.is_file()
            && opts.matches_max_filesize(meta.size)
            && opts.matches_globs(&item.logical, cwd)
            && opts.matches_type_filters(&item.logical)
        {
            result.push(RgFileCandidate {
                logical: item.logical,
                actual: item.actual,
                metadata: meta,
                display_hint: item.display_hint,
                display_override: None,
                explicit: false,
            });
        }
    }

    sort_rg_candidates(&mut result, opts);
    result
}

fn sort_rg_candidates(files: &mut [RgFileCandidate], opts: &RgOptions) {
    match opts.sort {
        RgSort::Path => files.sort_by(|a, b| a.logical.cmp(&b.logical)),
        // The VFS metadata contract does not expose atime, so --sort accessed
        // uses mtime until the filesystem layer grows access timestamps.
        RgSort::Modified | RgSort::Accessed => {
            files.sort_by(|a, b| {
                a.metadata
                    .modified
                    .cmp(&b.metadata.modified)
                    .then_with(|| a.logical.cmp(&b.logical))
            });
        }
        RgSort::Created => {
            files.sort_by(|a, b| {
                a.metadata
                    .created
                    .cmp(&b.metadata.created)
                    .then_with(|| a.logical.cmp(&b.logical))
            });
        }
        RgSort::None => {}
    }
    if opts.sort_reverse && opts.sort != RgSort::None {
        files.reverse();
    }
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
        let root = RgSearchRoot {
            logical: cwd.to_path_buf(),
            actual: cwd.to_path_buf(),
            display_hint: RgDisplayHint::None,
        };
        let files = collect_rg_files_recursive(fs, &[root], opts, cwd).await;
        return files
            .iter()
            .map(|file| display_path_for(&file.logical, cwd, file.display_hint.root_arg(), opts))
            .collect();
    }

    let mut result = Vec::new();
    let mut candidates = Vec::new();
    for p in &opts.paths {
        let path = resolve_path(cwd, p);
        if let Some((actual_path, meta)) =
            resolve_rg_explicit_path(fs, &path, opts.follow_symlinks).await
            && meta.file_type.is_dir()
        {
            let root = RgSearchRoot {
                logical: path.clone(),
                actual: actual_path,
                display_hint: RgDisplayHint::from_root_arg(Some(p)),
            };
            let files = collect_rg_files_recursive(fs, &[root], opts, cwd).await;
            candidates.extend(files);
        } else if meta_is_file_and_matches(fs, &path, opts, cwd).await
            && let Some((actual_path, meta)) =
                resolve_rg_explicit_path(fs, &path, opts.follow_symlinks).await
        {
            candidates.push(RgFileCandidate {
                logical: path,
                actual: actual_path,
                metadata: meta,
                display_hint: RgDisplayHint::from_root_arg(Some(p)),
                display_override: None,
                explicit: true,
            });
        }
    }
    sort_rg_candidates(&mut candidates, opts);
    result.extend(
        candidates
            .iter()
            .map(|file| candidate_display_path(file, cwd, opts)),
    );
    result
}

async fn meta_is_file_and_matches(
    fs: &dyn crate::fs::FileSystem,
    path: &Path,
    opts: &RgOptions,
    _cwd: &Path,
) -> bool {
    resolve_rg_explicit_path(fs, path, opts.follow_symlinks)
        .await
        .is_some_and(|(_, meta)| meta.file_type.is_file())
}

async fn read_rg_files(
    fs: &dyn crate::fs::FileSystem,
    files: Vec<RgFileCandidate>,
    cwd: &Path,
    opts: &RgOptions,
) -> RgCollectedInputs {
    let mut collected = RgCollectedInputs::default();
    for file in files {
        let display = candidate_display_path(&file, cwd, opts);
        match read_rg_text_file(fs, &file.actual, cwd, &display, opts).await {
            Ok(text) if file.explicit => collected.inputs.push(RgInput::explicit(display, text)),
            Ok(text) => collected.inputs.push(RgInput::discovered(display, text)),
            Err(e) => {
                collected.had_errors = true;
                if opts.messages {
                    collected.stderr.push_str(&e.stderr);
                }
            }
        }
    }
    collected
}

fn candidate_display_path(file: &RgFileCandidate, cwd: &Path, opts: &RgOptions) -> String {
    file.display_override
        .clone()
        .unwrap_or_else(|| display_path_for(&file.logical, cwd, file.display_hint.root_arg(), opts))
}

async fn try_indexed_search(
    fs: &dyn crate::fs::FileSystem,
    opts: &RgOptions,
    cwd: &Path,
) -> Option<Vec<RgInput>> {
    let index_can_use_literal = opts.fixed_strings && !opts.word_boundary && !opts.line_regexp;
    if opts.invert_match
        || opts.files_without_matches
        || opts.crlf
        || opts.uses_ignore_files()
        || opts.patterns.len() != 1
        || opts.max_filesize.is_some()
        || !opts.type_includes.is_empty()
        || !opts.type_excludes.is_empty()
        || opts.follow_symlinks
        || opts.search_zip
        || opts.preprocessor.is_some()
        || (!opts.unicode && !index_can_use_literal)
        || opts.sort != RgSort::Path
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
        let explicit_file_root = root_arg.is_some()
            && fs
                .stat(&root)
                .await
                .ok()
                .is_some_and(|meta| meta.file_type.is_file());
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
            glob_filter: if caps.glob_filter && !explicit_file_root {
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
            let explicit_file_match = explicit_file_root && candidate == root;

            if !candidate.starts_with(&root)
                || !seen_paths.insert(candidate.clone())
                || (!explicit_file_match && !opts.matches_globs(&candidate, cwd))
                || (!explicit_file_match
                    && !opts.hidden
                    && path_has_hidden_component_relative_to(&candidate, &root))
            {
                continue;
            }
            if let Ok(content) = fs.read_file(&candidate).await {
                let display = display_path_for(&candidate, cwd, root_arg.as_deref(), opts);
                let content = decode_rg_content(&content, opts);
                let input = if explicit_file_match {
                    RgInput::explicit(display, content)
                } else {
                    RgInput::discovered(display, content)
                };
                inputs.push(input);
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
    color: bool,
    color_scheme: &'a RgColorScheme,
    hyperlink_format: Option<&'a str>,
}

const RG_ANSI_RESET: &str = "\x1b[0m";
const RG_COLOR_MATCH_EXTRA_BYTES_LIMIT: usize = 64 * 1024;

fn color_path(path: &str, color: bool, color_scheme: &RgColorScheme) -> String {
    if color {
        color_text(path, &color_scheme.path, true)
    } else {
        path.to_string()
    }
}

fn color_numeric(
    value: usize,
    color: bool,
    line_number: bool,
    color_scheme: &RgColorScheme,
) -> String {
    if color {
        let style = if line_number {
            &color_scheme.line
        } else {
            &color_scheme.column
        };
        color_text(&value.to_string(), style, true)
    } else {
        value.to_string()
    }
}

fn color_text(text: &str, style: &RgColorStyle, reset_when_disabled: bool) -> String {
    if !style.enabled {
        return if reset_when_disabled {
            format!("{RG_ANSI_RESET}{text}{RG_ANSI_RESET}")
        } else {
            text.to_string()
        };
    }
    let mut output = String::from(RG_ANSI_RESET);
    if style.bold {
        output.push_str("\x1b[1m");
    }
    if style.italic {
        output.push_str("\x1b[3m");
    }
    if style.underline {
        output.push_str("\x1b[4m");
    }
    if let Some(fg) = style.fg.as_deref() {
        output.push_str("\x1b[");
        output.push_str(&intense_ansi_code(fg, style.intense));
        output.push('m');
    }
    if let Some(bg) = style.bg.as_deref() {
        output.push_str("\x1b[");
        output.push_str(&intense_ansi_code(bg, style.intense));
        output.push('m');
    }
    output.push_str(text);
    output.push_str(RG_ANSI_RESET);
    output
}

fn color_prefix(style: &RgColorStyle, reset_first: bool) -> String {
    if !style.enabled {
        return String::new();
    }
    let mut output = if reset_first {
        String::from(RG_ANSI_RESET)
    } else {
        String::new()
    };
    if style.bold {
        output.push_str("\x1b[1m");
    }
    if style.italic {
        output.push_str("\x1b[3m");
    }
    if style.underline {
        output.push_str("\x1b[4m");
    }
    if let Some(fg) = style.fg.as_deref() {
        output.push_str("\x1b[");
        output.push_str(&intense_ansi_code(fg, style.intense));
        output.push('m');
    }
    if let Some(bg) = style.bg.as_deref() {
        output.push_str("\x1b[");
        output.push_str(&intense_ansi_code(bg, style.intense));
        output.push('m');
    }
    output
}

fn intense_ansi_code(code: &str, intense: bool) -> String {
    if !intense {
        return code.to_string();
    }
    match code.parse::<u8>() {
        Ok(code @ 30..=37) => format!("38;5;{}", code - 22),
        Ok(code @ 40..=47) => format!("48;5;{}", code - 32),
        _ => code.to_string(),
    }
}

fn color_matches(text: &str, regex: &RgMatcher, opts: &RgOptions) -> String {
    if !opts.color_enabled() {
        return text.to_string();
    }
    let mut estimated_extra = 0usize;
    let per_match_extra = 32usize;
    let mut bailout = false;
    regex.for_each_match(text, |mat| {
        if bailout {
            return false;
        }
        estimated_extra = estimated_extra.saturating_add(per_match_extra);
        if mat.start() == mat.end() {
            estimated_extra = estimated_extra.saturating_add(1);
        }
        if estimated_extra > RG_COLOR_MATCH_EXTRA_BYTES_LIMIT {
            bailout = true;
            return false;
        }
        true
    });
    if bailout {
        return text.to_string();
    }
    if opts.color_scheme.highlight.enabled {
        let mut output = color_prefix(&opts.color_scheme.highlight, true);
        let mut last = 0;
        let mut matched = false;
        regex.for_each_match(text, |mat| {
            matched = true;
            output.push_str(&text[last..mat.start()]);
            output.push_str(&color_text(mat.as_str(), &opts.color_scheme.matches, false));
            output.push_str(&color_prefix(&opts.color_scheme.highlight, false));
            last = mat.end();
            true
        });
        if !matched {
            output.push_str(text);
        } else {
            output.push_str(&text[last..]);
        }
        output.push_str(RG_ANSI_RESET);
        return output;
    }
    let mut output = String::new();
    let mut last = 0;
    regex.for_each_match(text, |mat| {
        output.push_str(&text[last..mat.start()]);
        output.push_str(&color_text(mat.as_str(), &opts.color_scheme.matches, false));
        last = mat.end();
        true
    });
    if last == 0 {
        text.to_string()
    } else {
        output.push_str(&text[last..]);
        output
    }
}

fn write_rg_prefix(output: &mut String, prefix: RgPrefix<'_>) {
    let hyperlink_url = if prefix.show_filename
        && prefix.color
        && !prefix.null_path_separator
        && let Some(format) = prefix.hyperlink_format
    {
        Some(format_hyperlink_url(
            format,
            prefix.filename,
            prefix.line_idx + 1,
            prefix.column.unwrap_or(1),
        ))
    } else {
        None
    };
    let hyperlink_open = hyperlink_url.is_some();

    if prefix.show_filename {
        if let Some(url) = &hyperlink_url {
            output.push_str("\x1b]8;;");
            output.push_str(url);
            output.push_str("\x1b\\");
        }
        if prefix.color {
            output.push_str(&color_path(prefix.filename, true, prefix.color_scheme));
        } else {
            output.push_str(prefix.filename);
        }
        if prefix.null_path_separator {
            output.push('\0');
        } else if hyperlink_open
            && !prefix.line_numbers
            && prefix.column.is_none()
            && prefix.byte_offset.is_none()
        {
            output.push_str("\x1b]8;;\x1b\\");
            output.push_str(prefix.separator);
        } else {
            output.push_str(prefix.separator);
        }
    }
    if prefix.line_numbers {
        output.push_str(&color_numeric(
            prefix.line_idx + 1,
            prefix.color,
            true,
            prefix.color_scheme,
        ));
        if hyperlink_open && prefix.column.is_none() && prefix.byte_offset.is_none() {
            output.push_str("\x1b]8;;\x1b\\");
            output.push_str(prefix.separator);
        } else {
            output.push_str(prefix.separator);
        }
    }
    if let Some(column) = prefix.column {
        output.push_str(&color_numeric(
            column,
            prefix.color,
            false,
            prefix.color_scheme,
        ));
        if hyperlink_open && prefix.byte_offset.is_none() {
            output.push_str("\x1b]8;;\x1b\\");
            output.push_str(prefix.separator);
        } else {
            output.push_str(prefix.separator);
        }
    }
    if let Some(byte_offset) = prefix.byte_offset {
        output.push_str(&color_numeric(
            byte_offset,
            prefix.color,
            false,
            prefix.color_scheme,
        ));
        if hyperlink_open {
            output.push_str("\x1b]8;;\x1b\\");
        }
        output.push_str(prefix.separator);
    }
}

fn format_hyperlink_url(format: &str, filename: &str, line: usize, column: usize) -> String {
    let path = hyperlink_path(filename);
    let line = line.to_string();
    let column = column.to_string();
    let mut out = String::with_capacity(format.len() + path.len());
    let mut rest = format;

    while let Some(start) = rest.find('{') {
        out.push_str(&rest[..start]);
        let tail = &rest[start..];
        if let Some(remaining) = tail.strip_prefix("{path}") {
            out.push_str(&path);
            rest = remaining;
        } else if let Some(remaining) = tail.strip_prefix("{line}") {
            out.push_str(&line);
            rest = remaining;
        } else if let Some(remaining) = tail.strip_prefix("{column}") {
            out.push_str(&column);
            rest = remaining;
        } else {
            out.push('{');
            rest = &tail[1..];
        }
    }
    out.push_str(rest);
    out
}

fn hyperlink_path(filename: &str) -> String {
    let normalized = if filename.starts_with('/') {
        filename.to_string()
    } else {
        format!("/{}", filename.trim_start_matches("./"))
    };
    percent_encode_url_path(&normalized)
}

fn percent_encode_url_path(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for &b in path.as_bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~' | b'/') {
            out.push(char::from(b));
        } else {
            out.push('%');
            out.push_str(&format!("{b:02X}"));
        }
    }
    out
}

#[derive(Clone, Copy)]
struct RgLine<'a> {
    text: &'a str,
    match_text: &'a str,
    raw: &'a str,
    start_offset: usize,
}

#[derive(Clone, Copy)]
struct RgMultilineMatch<'a> {
    text: &'a str,
    start_offset: usize,
    end_offset: usize,
    line_idx: usize,
    end_line_idx: usize,
    column: usize,
}

fn iter_rg_lines(content: &str, crlf: bool, null_data: bool) -> impl Iterator<Item = RgLine<'_>> {
    let mut offset = 0usize;
    let terminator = if null_data { '\0' } else { '\n' };
    content.split_inclusive(terminator).map(move |raw| {
        let start_offset = offset;
        offset += raw.len();
        let text = raw.strip_suffix(terminator).unwrap_or(raw);
        let match_text = if crlf {
            text.strip_suffix('\r').unwrap_or(text)
        } else {
            text
        };
        RgLine {
            text,
            match_text,
            raw,
            start_offset,
        }
    })
}

fn split_rg_lines(content: &str, crlf: bool, null_data: bool) -> Vec<RgLine<'_>> {
    iter_rg_lines(content, crlf, null_data).collect()
}

fn rg_record_terminator(opts: &RgOptions) -> char {
    if opts.null_data { '\0' } else { '\n' }
}

fn rg_line_index_for_offset(lines: &[RgLine<'_>], offset: usize) -> usize {
    lines
        .partition_point(|line| line.start_offset <= offset)
        .saturating_sub(1)
}

fn collect_rg_multiline_matches<'a>(
    regex: &RgMatcher,
    content: &'a str,
    lines: &[RgLine<'a>],
    max_count: Option<usize>,
) -> Vec<RgMultilineMatch<'a>> {
    if lines.is_empty() || max_count == Some(0) {
        return Vec::new();
    }
    let mut matches = Vec::new();
    regex.for_each_match(content, |mat| {
        if let Some(max) = max_count
            && matches.len() >= max
        {
            return false;
        }
        let line_idx = rg_line_index_for_offset(lines, mat.start());
        let end_line_idx = rg_line_index_for_offset(lines, mat.end().saturating_sub(1));
        matches.push(RgMultilineMatch {
            text: mat.as_str(),
            start_offset: mat.start(),
            end_offset: mat.end(),
            line_idx,
            end_line_idx,
            column: mat.start().saturating_sub(lines[line_idx].start_offset) + 1,
        });
        max_count.is_none_or(|max| matches.len() < max)
    });
    matches
}

fn rg_multiline_match_lines(matches: &[RgMultilineMatch<'_>]) -> Vec<usize> {
    let mut lines = Vec::new();
    for mat in matches {
        lines.extend(mat.line_idx..=mat.end_line_idx);
    }
    lines
}

fn rg_unique_sorted_lines(line_indices: &[usize]) -> Vec<usize> {
    let mut lines: Vec<usize> = line_indices.to_vec();
    lines.sort_unstable();
    lines.dedup();
    lines
}

fn format_rg_multiline_replacement(
    regex: &RgMatcher,
    lines: &[RgLine<'_>],
    mat: RgMultilineMatch<'_>,
    replacement: &str,
) -> String {
    let start_line = lines[mat.line_idx];
    let end_line = lines[mat.end_line_idx];
    let prefix_end = mat.start_offset.saturating_sub(start_line.start_offset);
    let suffix_start = mat.end_offset.saturating_sub(end_line.start_offset);
    let prefix = start_line.text.get(..prefix_end).unwrap_or("");
    let suffix = end_line.text.get(suffix_start..).unwrap_or("");
    format!(
        "{}{}{}",
        prefix,
        regex.replace_first(mat.text, replacement),
        suffix
    )
}

fn first_nul_offset(content: &str) -> Option<usize> {
    content.as_bytes().iter().position(|&byte| byte == 0)
}

fn format_rg_line(
    line: &str,
    match_line: &str,
    regex: &RgMatcher,
    opts: &RgOptions,
    matched: bool,
) -> String {
    let line = if matched {
        if let Some(replacement) = &opts.replacement {
            let mut replaced = regex.replace_all(match_line, replacement.as_str());
            if line.len() > match_line.len() {
                replaced.push_str(&line[match_line.len()..]);
            }
            replaced
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

fn format_rg_output_line(
    line: &str,
    match_line: &str,
    regex: &RgMatcher,
    opts: &RgOptions,
    matched: bool,
) -> String {
    let line = format_rg_line(line, match_line, regex, opts, matched);
    let display = if let Some(max_columns) = opts.max_columns {
        if max_columns == 0 || line.chars().count() <= max_columns {
            line
        } else if opts.max_columns_preview {
            let preview: String = line.chars().take(max_columns).collect();
            format!("{preview} [... omitted end of long line]")
        } else if matched {
            "[Omitted long matching line]".to_string()
        } else {
            "[Omitted long context line]".to_string()
        }
    } else {
        line
    };

    if matched && opts.replacement.is_none() && !opts.invert_match {
        color_matches(&display, regex, opts)
    } else {
        display
    }
}

fn format_rg_match_text(text: &str, regex: &RgMatcher, opts: &RgOptions) -> String {
    if let Some(replacement) = &opts.replacement {
        regex.replace_first(text, replacement.as_str())
    } else {
        color_matches(text, regex, opts)
    }
}

fn rg_multiline_match_segments(
    mat: RgMultilineMatch<'_>,
    regex: &RgMatcher,
    opts: &RgOptions,
) -> Vec<String> {
    let display = format_rg_match_text(mat.text, regex, opts);
    if display.is_empty() {
        Vec::new()
    } else {
        display
            .split('\n')
            .map(|segment| segment.to_string())
            .collect()
    }
}

struct RgMultilineMatchPrefix<'a> {
    filename: &'a str,
    show_filename: bool,
    line_numbers: bool,
    column: bool,
    byte_offset: bool,
    vimgrep: bool,
    separator: &'a str,
    null_path_separator: bool,
    color: bool,
    color_scheme: &'a RgColorScheme,
    hyperlink_format: Option<&'a str>,
}

fn write_rg_multiline_match_segments(
    output: &mut String,
    mat: RgMultilineMatch<'_>,
    segments: &[String],
    prefix: RgMultilineMatchPrefix<'_>,
    terminator: char,
) {
    for (offset, segment) in segments.iter().enumerate() {
        write_rg_prefix(
            output,
            RgPrefix {
                filename: prefix.filename,
                show_filename: prefix.show_filename || prefix.vimgrep,
                line_numbers: prefix.line_numbers || prefix.vimgrep,
                line_idx: mat.line_idx + offset,
                column: if prefix.column || prefix.vimgrep {
                    Some(mat.column)
                } else {
                    None
                },
                byte_offset: if prefix.byte_offset {
                    Some(mat.start_offset)
                } else {
                    None
                },
                separator: prefix.separator,
                null_path_separator: prefix.null_path_separator,
                color: prefix.color,
                color_scheme: prefix.color_scheme,
                hyperlink_format: prefix.hyperlink_format,
            },
        );
        output.push_str(segment);
        output.push(terminator);
    }
}

fn rg_context_line_indices(
    lines_len: usize,
    match_lines: &[usize],
    before_context: usize,
    after_context: usize,
    include_matches: bool,
) -> Vec<usize> {
    if lines_len == 0 || match_lines.is_empty() {
        return Vec::new();
    }

    let mut ranges = Vec::with_capacity(match_lines.len());
    for &match_idx in match_lines {
        if match_idx >= lines_len {
            continue;
        }
        let start = match_idx.saturating_sub(before_context);
        let end = match_idx
            .saturating_add(after_context)
            .saturating_add(1)
            .min(lines_len);
        ranges.push((start, end));
    }
    if ranges.is_empty() {
        return Vec::new();
    }

    ranges.sort_unstable_by_key(|&(start, end)| (start, end));
    let mut merged: Vec<(usize, usize)> = Vec::with_capacity(ranges.len());
    for (start, end) in ranges {
        if let Some((_, last_end)) = merged.last_mut()
            && start <= *last_end
        {
            *last_end = (*last_end).max(end);
            continue;
        }
        merged.push((start, end));
    }

    let match_set = (!include_matches).then(|| match_lines.iter().copied().collect::<HashSet<_>>());
    let mut indices = Vec::new();
    for (start, end) in merged {
        for idx in start..end {
            if match_set
                .as_ref()
                .is_some_and(|matches| matches.contains(&idx))
            {
                continue;
            }
            indices.push(idx);
        }
    }
    indices
}

fn write_rg_context(
    output: &mut String,
    filename: &str,
    regex: &RgMatcher,
    lines: &[RgLine<'_>],
    match_lines: &[usize],
    opts: &RgOptions,
    show_filename: bool,
) {
    // Merge context windows before expansion. Large -A/-B/-C values must cost
    // proportional to emitted lines, not matches × context.
    let sorted = rg_context_line_indices(
        lines.len(),
        match_lines,
        opts.before_context,
        opts.after_context,
        true,
    );
    let match_set: HashSet<usize> = match_lines.iter().copied().collect();
    let mut prev_line = None;
    let record_terminator = rg_record_terminator(opts);

    for line_idx in sorted {
        if let Some(prev) = prev_line
            && line_idx > prev + 1
            && !opts.no_context_separator
        {
            output.push_str(&opts.context_separator);
            output.push(record_terminator);
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
                color: opts.color_enabled(),
                color_scheme: &opts.color_scheme,
                hyperlink_format: opts.hyperlink_format.as_deref(),
            },
        );
        output.push_str(&format_rg_output_line(
            lines[line_idx].text,
            lines[line_idx].match_text,
            regex,
            opts,
            matched,
        ));
        output.push(record_terminator);
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
    regex: &RgMatcher,
    replacement: Option<&str>,
) {
    write_rg_json_match_with_text(
        output,
        filename,
        line,
        line_idx,
        regex,
        replacement,
        line.match_text,
    );
}

fn write_rg_json_match_with_text(
    output: &mut String,
    filename: &str,
    line: RgLine<'_>,
    line_idx: usize,
    regex: &RgMatcher,
    replacement: Option<&str>,
    match_text: &str,
) {
    // Stream submatches straight into the JSON output buffer instead of first
    // collecting them into a Vec<serde_json::Value> and then serializing the
    // whole tree. Avoids an O(matches_per_line) intermediate allocation on
    // attacker-controlled lines with many matches.
    //
    // serde_json serializes Object keys in lexicographic order, so the outer
    // event looks like `{"data":{...},"type":"match"}` — only one trailing `}`.
    // Build the inner `data` object on its own, strip its trailing `}`,
    // append `"submatches":[...]` (which sorts last alphabetically after
    // `path`), close `data`, then close the outer envelope by hand.
    let data = json!({
        "path":{"text":filename},
        "lines":{"text":line.raw},
        "line_number":line_idx + 1,
        "absolute_offset":line.start_offset,
    });
    let data_str = data.to_string();
    let Some(data_open) = data_str.strip_suffix('}') else {
        // Defensive fallback — preserve the previous semantics if serde_json
        // ever produces an unexpected shape.
        write_rg_json_event(
            output,
            json!({
                "type":"match",
                "data":{
                    "path":{"text":filename},
                    "lines":{"text":line.raw},
                    "line_number":line_idx + 1,
                    "absolute_offset":line.start_offset,
                    "submatches":[],
                }
            }),
        );
        return;
    };
    output.push_str("{\"data\":");
    output.push_str(data_open);
    output.push_str(",\"submatches\":[");
    let mut first = true;
    regex.for_each_match(match_text, |mat| {
        if !first {
            output.push(',');
        }
        first = false;
        let mut value = json!({
            "match":{"text":mat.as_str()},
            "start":mat.start(),
            "end":mat.end()
        });
        if let Some(replacement) = replacement
            && let Some(obj) = value.as_object_mut()
        {
            obj.insert(
                "replacement".to_string(),
                json!({"text":regex.replace_first(mat.as_str(), replacement)}),
            );
        }
        output.push_str(&value.to_string());
        true
    });
    output.push_str("]},\"type\":\"match\"}\n");
}

fn write_rg_json_context(output: &mut String, filename: &str, line: RgLine<'_>, line_idx: usize) {
    write_rg_json_event(
        output,
        json!({
            "type":"context",
            "data":{
                "path":{"text":filename},
                "lines":{"text":line.raw},
                "line_number":line_idx + 1,
                "absolute_offset":line.start_offset,
                "submatches":[],
            }
        }),
    );
}

fn write_rg_json_multiline_match(
    output: &mut String,
    filename: &str,
    lines: &[RgLine<'_>],
    mat: RgMultilineMatch<'_>,
    regex: &RgMatcher,
    replacement: Option<&str>,
) {
    let start_line_offset = lines[mat.line_idx].start_offset;
    let line_text = lines[mat.line_idx..=mat.end_line_idx]
        .iter()
        .map(|line| line.raw)
        .collect::<String>();
    let mut submatch = json!({
        "match":{"text":mat.text},
        "start":mat.start_offset - start_line_offset,
        "end":mat.end_offset - start_line_offset
    });
    if let Some(replacement) = replacement
        && let Some(obj) = submatch.as_object_mut()
    {
        obj.insert(
            "replacement".to_string(),
            json!({"text":regex.replace_first(mat.text, replacement)}),
        );
    }
    write_rg_json_event(
        output,
        json!({
            "type":"match",
            "data":{
                "path":{"text":filename},
                "lines":{"text":line_text},
                "line_number":mat.line_idx + 1,
                "absolute_offset":start_line_offset,
                "submatches":[submatch],
            }
        }),
    );
}

fn write_rg_json_end(
    output: &mut String,
    filename: &str,
    binary_offset: Option<usize>,
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
                "binary_offset":binary_offset,
                "stats":rg_json_stats(bytes_searched, matched_lines, matches, 1, usize::from(matched_lines > 0)),
            }
        }),
    );
}

fn rg_json_stats(
    bytes_searched: usize,
    matched_lines: usize,
    matches: usize,
    searches: usize,
    searches_with_match: usize,
) -> serde_json::Value {
    json!({
        "elapsed":{"secs":0,"nanos":0,"human":"0.000000s"},
        "searches":searches,
        "searches_with_match":searches_with_match,
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
    searches_with_match: usize,
) {
    write_rg_json_event(
        output,
        json!({
            "type":"summary",
            "data":{
                "elapsed_total":{"secs":0,"nanos":0,"human":"0.000000s"},
                "stats":rg_json_stats(bytes_searched, matched_lines, matches, searches, searches_with_match),
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

fn rg_quiet_result(
    opts: &RgOptions,
    match_count: usize,
    any_match: &mut bool,
    stderr: &str,
) -> Option<ExecResult> {
    let selected = if opts.files_without_matches {
        match_count == 0
    } else {
        match_count > 0
    };
    if selected {
        *any_match = true;
        if !opts.stats {
            return Some(ExecResult {
                stdout: String::new(),
                stderr: stderr.to_string(),
                exit_code: 0,
                ..Default::default()
            });
        }
    }
    None
}

fn rg_option_takes_value(arg: &str) -> bool {
    matches!(
        arg,
        "-e" | "--regexp"
            | "-f"
            | "--file"
            | "-r"
            | "--replace"
            | "-A"
            | "--after-context"
            | "-B"
            | "--before-context"
            | "-C"
            | "--context"
            | "-d"
            | "--max-depth"
            | "-E"
            | "--encoding"
            | "--engine"
            | "--field-context-separator"
            | "--field-match-separator"
            | "-g"
            | "--glob"
            | "--iglob"
            | "--ignore-file"
            | "-M"
            | "--max-columns"
            | "-m"
            | "--max-count"
            | "--max-filesize"
            | "--path-separator"
            | "--pre"
            | "--pre-glob"
            | "--regex-size-limit"
            | "--dfa-size-limit"
            | "--sort"
            | "--sortr"
            | "-j"
            | "--threads"
            | "-t"
            | "--type"
            | "-T"
            | "--type-not"
            | "--type-add"
            | "--type-clear"
            | "--context-separator"
            | "--hostname-bin"
            | "--hyperlink-format"
            | "--colors"
    )
}

fn rg_arg_before_delimiter(args: &[String], needle: &str) -> bool {
    args.iter()
        .take_while(|arg| arg.as_str() != "--")
        .any(|arg| arg == needle)
}

fn rg_generate_kind(args: &[String]) -> Result<Option<String>> {
    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        if arg == "--" {
            break;
        }
        if rg_option_takes_value(arg) {
            i += 2;
            continue;
        }
        if arg == "--generate" {
            let Some(kind) = args.get(i + 1) else {
                return Err(Error::Execution(
                    "rg: --generate requires an argument".to_string(),
                ));
            };
            return Ok(Some(kind.clone()));
        }
        if let Some(kind) = arg.strip_prefix("--generate=") {
            return Ok(Some(kind.to_string()));
        }
        i += 1;
    }
    Ok(None)
}

fn rg_generate_output(kind: &str, help_text: &str) -> Result<String> {
    const FLAGS: &str = "--regexp -e --file -f --after-context -A --before-context -B --binary --no-binary --block-buffered --no-block-buffered --byte-offset -b --no-byte-offset --case-sensitive -s --color --colors --column --no-column --context -C --context-separator --no-context-separator --count -c --count-matches --crlf --no-crlf --debug --dfa-size-limit --encoding -E --no-encoding --engine --field-context-separator --field-match-separator --files --files-with-matches -l --files-without-match --fixed-strings -F --no-fixed-strings --follow -L --no-follow --generate --glob -g --glob-case-insensitive --no-glob-case-insensitive --heading --no-heading --help -h --hidden -. --no-hidden --hostname-bin --hyperlink-format --iglob --ignore-case -i --ignore-file --ignore-file-case-insensitive --no-ignore-file-case-insensitive --include-zero --no-include-zero --invert-match -v --no-invert-match --json --no-json --line-buffered --no-line-buffered --line-number -n --no-line-number -N --line-regexp -x --max-columns -M --max-columns-preview --no-max-columns-preview --max-count -m --max-depth --maxdepth -d --max-filesize --mmap --no-mmap --multiline -U --no-multiline --multiline-dotall --no-multiline-dotall --no-config --no-ignore --ignore --no-ignore-dot --ignore-dot --no-ignore-exclude --ignore-exclude --no-ignore-files --ignore-files --no-ignore-global --ignore-global --no-ignore-messages --ignore-messages --no-ignore-parent --ignore-parent --no-ignore-vcs --ignore-vcs --no-messages --messages --no-require-git --require-git --no-unicode --unicode --null -0 --null-data --one-file-system --no-one-file-system --only-matching -o --path-separator --passthru --passthrough --pcre2 -P --no-pcre2 --pcre2-version --pre --no-pre --pre-glob --pretty -p --quiet -q --regex-size-limit --replace -r --search-zip -z --no-search-zip --smart-case -S --sort --sortr --stats --no-stats --stop-on-nonmatch --text -a --no-text --threads -j --trace --trim --no-trim --type -t --type-not -T --type-add --type-clear --type-list --unrestricted -u --version -V --vimgrep --with-filename -H --no-filename -I --word-regexp -w --auto-hybrid-regex --no-auto-hybrid-regex --no-pcre2-unicode --pcre2-unicode --sort-files --no-sort-files";

    match kind {
        "man" => Ok(format!(
            ".TH RG 1 \"\" \"bashkit\" \"User Commands\"\n.SH NAME\nrg \\- recursively search the current directory for lines matching a pattern\n.SH SYNOPSIS\n.B rg\n[OPTIONS] PATTERN [PATH...]\n.SH DESCRIPTION\nripgrep (rg) recursively searches files. bashkit provides a sandboxed rg-compatible builtin.\n.SH OPTIONS\n.nf\n{}\n.fi\n",
            help_text
        )),
        "complete-bash" => Ok(format!(
            "_rg() {{\n  local cur opts\n  COMPREPLY=()\n  cur=\"${{COMP_WORDS[COMP_CWORD]}}\"\n  opts=\"{}\"\n  COMPREPLY=($(compgen -W \"${{opts}}\" -- \"${{cur}}\"))\n}}\ncomplete -F _rg rg\n",
            FLAGS
        )),
        "complete-zsh" => Ok(format!(
            "#compdef rg\n\n_rg() {{\n  _arguments '*::arg:->args' {}\n}}\n_rg \"$@\"\n",
            FLAGS
                .split_whitespace()
                .filter(|flag| flag.starts_with("--"))
                .map(|flag| format!("'{}[ripgrep option]'", flag))
                .collect::<Vec<_>>()
                .join(" ")
        )),
        "complete-fish" => {
            let mut output = String::new();
            output.push_str("# fish completions for rg\n");
            for flag in FLAGS
                .split_whitespace()
                .filter(|flag| flag.starts_with("--"))
            {
                output.push_str("complete -c rg -l ");
                output.push_str(flag.trim_start_matches("--"));
                output.push_str(" -d 'ripgrep option'\n");
            }
            Ok(output)
        }
        "complete-powershell" => Ok(format!(
            "using namespace System.Management.Automation\nRegister-ArgumentCompleter -Native -CommandName 'rg' -ScriptBlock {{\n  param($wordToComplete, $commandAst, $cursorPosition)\n  @({}) | Where-Object {{ $_ -like \"$wordToComplete*\" }} | ForEach-Object {{ [CompletionResult]::new($_, $_, [CompletionResultType]::ParameterName, 'ripgrep option') }}\n}}\n",
            FLAGS
                .split_whitespace()
                .filter(|flag| flag.starts_with("--"))
                .map(|flag| format!("'{}'", flag))
                .collect::<Vec<_>>()
                .join(", ")
        )),
        _ => Err(Error::Execution(format!(
            "rg: invalid --generate value: {kind}"
        ))),
    }
}

#[async_trait]
impl Builtin for Rg {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let help_text = "Usage: rg [OPTIONS] PATTERN [PATH...]\nRecursively search for a pattern.\n\n  -i, --ignore-case\tcase insensitive\n  -S, --smart-case\tcase insensitive if pattern is lowercase\n  -s, --case-sensitive\tcase sensitive\n  -n, --line-number\tshow line numbers\n  -N, --no-line-number\tsuppress line numbers\n  --column\tshow column numbers\n  -b, --byte-offset\tshow byte offsets\n  --vimgrep\tshow file:line:column:match lines\n  --json\tshow JSON Lines events\n  --stats\tshow search statistics\n  --null\tterminate path fields with NUL\n  -c, --count\tcount matching lines\n  --count-matches\tcount individual matches\n  --include-zero\tinclude zero counts\n  -l, --files-with-matches\tfiles with matches\n  --files-without-match\tfiles without matches\n  --files\tprint files that would be searched\n  -v, --invert-match\tinvert match\n  -w, --word-regexp\tword boundary\n  -x, --line-regexp\tmatch whole lines\n  -F, --fixed-strings\tfixed strings (literal)\n  -a, --text\tsearch binary files as text\n  --binary\tsearch binary files and print binary-match summaries\n  -z, --search-zip\tsearch gzip-compressed files\n  --no-search-zip\tdisable compressed file search\n  --pre COMMAND\trun a preprocessor before searching (cat/empty supported)\n  --pre-glob GLOB\tlimit preprocessor paths by glob\n  --no-pre\tdisable preprocessing\n  --crlf\ttreat CRLF as line terminators for $ anchors\n  --no-crlf\tdisable CRLF line terminator mode\n  -U, --multiline\tenable matching across line boundaries\n  --no-multiline\tdisable multiline matching\n  --multiline-dotall\tmake . match line terminators in multiline mode\n  --no-multiline-dotall\tdisable multiline dotall mode\n  -o, --only-matching\tshow only matching text\n  -q, --quiet\tsuppress output; exit status only\n  -e, --regexp PATTERN\tuse PATTERN for matching\n  -f, --file PATTERNFILE\tread patterns from file\n  -E, --encoding ENCODING\tdecode searched files using ENCODING\n  -r, --replace REPLACEMENT\treplace matches in output\n  --passthru\tprint matching and non-matching lines\n  --trim\ttrim whitespace from output lines\n  -m, --max-count NUM\tmax count per file\n  -M, --max-columns NUM\tomit lines longer than NUM columns\n  --max-columns-preview\tshow prefixes of long lines\n  -j, --threads NUM\tset number of search threads (no-op)\n  --regex-size-limit NUM\tset regex size limit (no-op)\n  --dfa-size-limit NUM\tset DFA size limit (no-op)\n  --max-depth NUM\tlimit recursive directory depth\n  -A, --after-context NUM\tshow trailing context\n  -B, --before-context NUM\tshow leading context\n  -C, --context NUM\tshow leading and trailing context\n  --context-separator SEP\tset context group separator\n  --field-match-separator SEP\tset match field separator\n  --field-context-separator SEP\tset context field separator\n  -p, --pretty\talias for heading plus line numbers\n  --heading\tgroup matches by file\n  --no-heading\tdisable heading output\n  --sort SORTBY\tsort paths (path only)\n  --sortr SORTBY\tsort paths in reverse (path only)\n  --sort-files\tsort --files output\n  --path-separator SEP\tset displayed path separator\n  -g, --glob GLOB\tinclude/exclude paths by glob (!GLOB excludes)\n  -t, --type TYPE\tinclude files matching TYPE\n  -T, --type-not TYPE\texclude files matching TYPE\n  --type-add TYPE:GLOB\tadd a file type glob\n  --type-clear TYPE\tclear a file type definition\n  --type-list\tshow file type definitions\n  --ignore-file FILE\tuse additional ignore file\n  --no-ignore\tdo not use ignore files\n  --no-ignore-dot\tdo not use .ignore files\n  --no-ignore-vcs\tdo not use .gitignore files\n  --no-require-git\tuse .gitignore outside git repositories\n  --require-git\trequire a git repository for .gitignore files\n  -u, --unrestricted\treduce filtering (repeatable)\n  --messages\tshow file read diagnostics\n  --no-messages\tsuppress file read diagnostics\n  --hidden\tsearch hidden files and directories\n  --no-hidden\tdo not search hidden files and directories\n  -H, --with-filename\tshow filename\n  -I, --no-filename\tsuppress filename\n  --line-buffered\tforce line buffering (no-op)\n  --block-buffered\tforce block buffering (no-op)\n  --no-config\tdo not read config files (no-op)\n  --mmap\tsearch using memory maps when possible (no-op)\n  --no-mmap\tdisable memory maps (no-op)\n  -P, --pcre2\tuse PCRE2-compatible regex engine\n  --no-pcre2\tdisable PCRE2-compatible regex engine\n  --pcre2-version\tshow PCRE2 version information\n  --unicode\tenable Unicode regex mode\n  --no-unicode\tdisable Unicode regex mode\n  --pcre2-unicode\tenable PCRE2 Unicode mode (no-op)\n  --no-pcre2-unicode\tdisable PCRE2 Unicode mode (no-op)\n  --engine ENGINE\tselect regex engine: default, auto, pcre2\n  --auto-hybrid-regex\tuse PCRE2-compatible regex when needed\n  --no-auto-hybrid-regex\tdisable auto hybrid regex\n  --color MODE\tcolor output: never, auto, always, ansi\n  --colors SPEC\tconfigure color styles\n  -h, --help\tdisplay this help and exit\n  -V, --version\toutput version information and exit\n";
        let help_text = help_text.replace(
            "  --sort SORTBY\tsort paths (path only)\n  --sortr SORTBY\tsort paths in reverse (path only)\n",
            "  --sort SORTBY\tsort paths: path, modified, accessed, created, none\n  --sortr SORTBY\tsort paths in reverse\n",
        );
        let help_text = help_text.replace(
            "  --max-depth NUM\tlimit recursive directory depth\n",
            "  -d, --max-depth NUM\tlimit recursive directory depth\n  --maxdepth NUM\talias for --max-depth\n",
        );
        let help_text = help_text.replace(
            "  --no-messages\tsuppress file read diagnostics\n",
            "  --no-messages\tsuppress file read diagnostics\n  --ignore-messages\tshow ignore parse diagnostics (no-op)\n  --no-ignore-messages\tsuppress ignore parse diagnostics (no-op)\n  --debug\tshow debug diagnostics (no-op)\n  --trace\tshow trace diagnostics (no-op)\n",
        );
        let help_text = help_text.replace(
            "  -v, --invert-match\tinvert match\n",
            "  -v, --invert-match\tinvert match\n  --no-invert-match\tdisable inverted matching\n",
        );
        let help_text = help_text.replace(
            "  -E, --encoding ENCODING\tdecode searched files using ENCODING\n",
            "  -E, --encoding ENCODING\tdecode searched files using ENCODING\n  --no-encoding\trestore automatic encoding detection\n",
        );
        let help_text = help_text.replace(
            "  --colors SPEC\tconfigure color styles\n",
            "  --colors SPEC\tconfigure color styles\n  --hostname-bin COMMAND\tcommand for hyperlink hostname discovery (no-op)\n  --hyperlink-format FORMAT\tconfigure hyperlink output\n",
        );
        let help_text = help_text.replace(
            "  --context-separator SEP\tset context group separator\n",
            "  --context-separator SEP\tset context group separator\n  --no-context-separator\tdisable context group separators\n",
        );
        let help_text = help_text.replace(
            "  --max-columns-preview\tshow prefixes of long lines\n",
            "  --max-columns-preview\tshow prefixes of long lines\n  --max-filesize NUM\tignore files larger than NUM bytes, K, M, or G\n",
        );
        let help_text = help_text.replace(
            "  --ignore-file FILE\tuse additional ignore file\n",
            "  --ignore-file FILE\tuse additional ignore file\n  --ignore-file-case-insensitive\tprocess ignore files case-insensitively\n  --no-ignore-file-case-insensitive\tdisable case-insensitive ignore files\n  --no-ignore-files\tdo not use --ignore-file paths\n  --ignore-files\tuse --ignore-file paths\n",
        );
        let help_text = help_text.replace(
            "  --passthru\tprint matching and non-matching lines\n",
            "  --passthru\tprint matching and non-matching lines\n  --passthrough\talias for --passthru\n",
        );
        let help_text = help_text.replace(
            "  --null\tterminate path fields with NUL\n",
            "  -0, --null\tterminate path fields with NUL\n  --null-data\tuse NUL as input and output record terminator\n",
        );
        let help_text = help_text.replace(
            "  --hidden\tsearch hidden files and directories\n",
            "  -., --hidden\tsearch hidden files and directories\n  -L, --follow\tfollow symbolic links during recursive search\n  --no-follow\tdo not follow symbolic links during recursive search\n  --one-file-system\tstay on one file system (no-op)\n  --no-one-file-system\tdisable one-file-system mode (no-op)\n",
        );
        let help_text = help_text.replace(
            "  --no-ignore\tdo not use ignore files\n  --no-ignore-dot\tdo not use .ignore files\n  --no-ignore-vcs\tdo not use .gitignore files\n",
            "  --no-ignore\tdo not use ignore files\n  --ignore\tuse ignore files\n  --no-ignore-dot\tdo not use .ignore files\n  --ignore-dot\tuse .ignore files\n  --no-ignore-exclude\tdo not use .git/info/exclude files\n  --ignore-exclude\tuse .git/info/exclude files\n  --no-ignore-global\tdo not use global ignore files\n  --ignore-global\tuse global ignore files\n  --no-ignore-parent\tdo not use parent ignore files\n  --ignore-parent\tuse parent ignore files\n  --no-ignore-vcs\tdo not use .gitignore files\n  --ignore-vcs\tuse .gitignore files\n",
        );
        let help_text = help_text.replace(
            "  --no-auto-hybrid-regex\tdisable auto hybrid regex\n",
            "  --no-auto-hybrid-regex\tdisable auto hybrid regex\n  --stop-on-nonmatch\tstop after a non-matching line follows a match\n",
        );
        let help_text = help_text.replace(
            "  --files\tprint files that would be searched\n",
            "  --files\tprint files that would be searched\n  --generate KIND\tgenerate man page or shell completion output\n",
        );
        let help_text = help_text.replace(
            "  -g, --glob GLOB\tinclude/exclude paths by glob (!GLOB excludes)\n",
            "  -g, --glob GLOB\tinclude/exclude paths by glob (!GLOB excludes)\n  --iglob GLOB\tcase-insensitive include/exclude path glob\n  --glob-case-insensitive\tmake -g/--glob rules case-insensitive\n  --no-glob-case-insensitive\tdisable case-insensitive -g/--glob rules\n",
        );
        if rg_arg_before_delimiter(ctx.args, "-h") {
            return Ok(ExecResult::ok(help_text));
        }
        if rg_arg_before_delimiter(ctx.args, "-V") {
            return Ok(ExecResult::ok("rg (bashkit) 0.1\n".to_string()));
        }
        if let Some(r) = super::check_help_version(ctx.args, &help_text, Some("rg (bashkit) 0.1")) {
            return Ok(r);
        }
        if rg_arg_before_delimiter(ctx.args, "--pcre2-version") {
            return Ok(ExecResult::ok(
                "PCRE2 10.45 is available (JIT is available)\n".to_string(),
            ));
        }
        if let Some(kind) = rg_generate_kind(ctx.args)? {
            return Ok(ExecResult::ok(rg_generate_output(&kind, &help_text)?));
        }
        let mut opts = match RgOptions::parse(ctx.args) {
            Ok(opts) => opts,
            Err(Error::Execution(message)) => {
                return Ok(ExecResult::err(format!("{message}\n"), 2));
            }
            Err(err) => return Err(err),
        };
        if opts.type_list {
            return Ok(ExecResult::ok(opts.type_database.list()));
        }
        load_rg_global_ignore_files(&*ctx.fs, ctx.env, &mut opts).await?;
        if let Err(result) = load_rg_ignore_files(&*ctx.fs, ctx.cwd, &mut opts).await {
            return Ok(result);
        }
        if opts.list_files {
            let files = collect_rg_file_list(&*ctx.fs, &opts, ctx.cwd).await;
            let found_files = !files.is_empty();
            let files: Vec<String> = files
                .into_iter()
                .map(|file| color_path(&file, opts.color_enabled(), &opts.color_scheme))
                .collect();
            let output = if opts.null {
                let mut output = String::new();
                for file in files {
                    output.push_str(file.as_str());
                    output.push('\0');
                }
                output
            } else if files.is_empty() {
                String::new()
            } else {
                format!("{}\n", files.join("\n"))
            };
            return Ok(ExecResult {
                stdout: output,
                exit_code: if found_files { 0 } else { 1 },
                ..Default::default()
            });
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
            && (opts.paths.is_empty()
                || has_directory_path(&*ctx.fs, ctx.cwd, &opts.paths, opts.follow_symlinks).await);

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
        let mut json_searches_with_match = 0usize;
        let mut stats = RgSearchStats::default();

        for input in &inputs {
            let filename = input.display.as_str();
            let content = input.content.as_str();
            let mut match_count = 0usize;
            let mut count_value = 0usize;
            let mut match_lines = Vec::new();
            stats.files_searched += 1;
            let binary_offset = first_nul_offset(content).filter(|_| !opts.text);
            let json_binary_search = binary_offset.is_some() && json_output;
            if json_binary_search {
                json_searches += 1;
                if !opts.binary && !input.explicit {
                    continue;
                }
            }
            if let Some(nul_offset) = binary_offset
                && !json_output
            {
                if !opts.binary && !input.explicit {
                    continue;
                }
                stats.bytes_searched += content.len();

                let matched = regex.is_match(content);
                let matched = if opts.invert_match { !matched } else { matched };
                if !matched {
                    if opts.quiet {
                        if let Some(result) =
                            rg_quiet_result(&opts, 0, &mut any_match, &collected_inputs.stderr)
                        {
                            return Ok(result);
                        }
                        continue;
                    }
                    if opts.files_without_matches {
                        any_match = true;
                        output.push_str(&color_path(
                            filename,
                            opts.color_enabled(),
                            &opts.color_scheme,
                        ));
                        output.push(if opts.null { '\0' } else { '\n' });
                    } else if (opts.count_only || opts.count_matches) && opts.include_zero {
                        if show_filename {
                            output.push_str(&color_path(
                                filename,
                                opts.color_enabled(),
                                &opts.color_scheme,
                            ));
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
                    if let Some(result) =
                        rg_quiet_result(&opts, 1, &mut any_match, &collected_inputs.stderr)
                    {
                        return Ok(result);
                    }
                    continue;
                }
                if opts.files_without_matches {
                    continue;
                }
                if opts.files_with_matches {
                    output.push_str(&color_path(
                        filename,
                        opts.color_enabled(),
                        &opts.color_scheme,
                    ));
                    output.push(if opts.null { '\0' } else { '\n' });
                    continue;
                }
                if opts.count_only || opts.count_matches {
                    if show_filename {
                        output.push_str(&color_path(
                            filename,
                            opts.color_enabled(),
                            &opts.color_scheme,
                        ));
                        output.push(if opts.null { '\0' } else { ':' });
                    }
                    output.push_str("1\n");
                    continue;
                }

                if show_filename {
                    output.push_str(&color_path(
                        filename,
                        opts.color_enabled(),
                        &opts.color_scheme,
                    ));
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
            if let Some(nul_offset) = binary_offset
                && json_output
                && cfg!(target_os = "linux")
                && !opts.binary
            {
                // ripgrep 15.1.0's JSON printer is platform-sensitive for
                // explicit binary files: Linux matches the original line
                // containing the NUL, but reports bytes searched only up to it.
                let search_content = content;
                let search_lines = split_rg_lines(search_content, opts.crlf, opts.null_data);
                let display_lines = split_rg_lines(content, opts.crlf, opts.null_data);
                let mut file_matches = 0usize;
                json_bytes_searched += nul_offset;
                stats.bytes_searched += nul_offset;

                for (line_idx, line) in search_lines.iter().enumerate() {
                    let matched = regex.is_match(line.match_text);
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
                        regex.count_matches(line.match_text)
                    } else {
                        1
                    };
                    file_matches += matches_on_line;
                    json_matches += matches_on_line;
                    json_matched_lines += 1;
                    stats.matches += matches_on_line;
                    stats.matched_lines += 1;
                    match_lines.push(line_idx);
                    any_match = true;
                }

                if match_count > 0 {
                    stats.files_with_matches += 1;
                    write_rg_json_begin(&mut output, filename);
                    for &line_idx in &match_lines {
                        let display_line = display_lines
                            .get(line_idx)
                            .copied()
                            .unwrap_or(search_lines[line_idx]);
                        write_rg_json_match_with_text(
                            &mut output,
                            filename,
                            display_line,
                            line_idx,
                            &regex,
                            opts.replacement.as_deref(),
                            search_lines[line_idx].match_text,
                        );
                    }
                    write_rg_json_end(
                        &mut output,
                        filename,
                        binary_offset,
                        nul_offset,
                        match_count,
                        file_matches,
                    );
                    json_searches_with_match += 1;
                }
                continue;
            }
            let binary_json_content;
            let content = if json_binary_search {
                binary_json_content = content.replace('\0', "\n");
                binary_json_content.as_str()
            } else {
                content
            };
            let record_terminator = rg_record_terminator(&opts);
            json_bytes_searched += content.len();
            if !json_binary_search {
                json_searches += 1;
            }
            stats.bytes_searched += content.len();

            if opts.multiline
                && !opts.invert_match
                && !opts.stats
                && !json_output
                && (opts.quiet || opts.files_with_matches || opts.files_without_matches)
            {
                // THREAT[TM-DOS-RG-LINES]: multiline existence modes can decide
                // from the whole buffer without materializing one `RgLine` per
                // input record. Invert/stats modes still need line accounting.
                let matched = regex.is_match(content);
                let match_count = usize::from(matched);
                if matched {
                    stats.matches += 1;
                    stats.matched_lines += 1;
                    stats.files_with_matches += 1;
                    if !opts.files_without_matches {
                        any_match = true;
                    }
                }

                if opts.quiet {
                    if let Some(result) = rg_quiet_result(
                        &opts,
                        match_count,
                        &mut any_match,
                        &collected_inputs.stderr,
                    ) {
                        return Ok(result);
                    }
                    continue;
                }
                if opts.files_with_matches && matched {
                    output.push_str(&color_path(
                        filename,
                        opts.color_enabled(),
                        &opts.color_scheme,
                    ));
                    output.push(if opts.null || opts.null_data {
                        '\0'
                    } else {
                        '\n'
                    });
                    continue;
                }
                if opts.files_without_matches {
                    if !matched {
                        any_match = true;
                        output.push_str(&color_path(
                            filename,
                            opts.color_enabled(),
                            &opts.color_scheme,
                        ));
                        output.push(if opts.null || opts.null_data {
                            '\0'
                        } else {
                            '\n'
                        });
                    }
                    continue;
                }
            }

            let summary_only = !opts.multiline
                && !json_output
                && !opts.passthru
                && !has_context
                && (opts.quiet
                    || opts.files_with_matches
                    || opts.files_without_matches
                    || opts.count_only
                    || opts.count_matches);
            if summary_only {
                // THREAT[TM-DOS-RG-LINES]: summary/early-exit modes do not need
                // random line access. Stream them to avoid one allocation per
                // attacker-controlled input line before `-q`/`-l` can return.
                for line in iter_rg_lines(content, opts.crlf, opts.null_data) {
                    let matched = regex.is_match(line.match_text);
                    let matched = if opts.invert_match { !matched } else { matched };

                    if !matched {
                        if opts.stop_on_nonmatch && match_count > 0 {
                            break;
                        }
                        continue;
                    }

                    if let Some(max) = opts.max_count
                        && match_count >= max
                    {
                        break;
                    }

                    match_count += 1;
                    let matches_on_line = if opts.invert_match {
                        if opts.count_only || opts.count_matches {
                            1
                        } else {
                            0
                        }
                    } else {
                        regex.count_matches(line.match_text)
                    };
                    if opts.count_matches && !opts.invert_match {
                        count_value += matches_on_line;
                    } else {
                        count_value += 1;
                    }
                    stats.matches += matches_on_line;
                    stats.matched_lines += 1;
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

                if opts.quiet {
                    if let Some(result) = rg_quiet_result(
                        &opts,
                        match_count,
                        &mut any_match,
                        &collected_inputs.stderr,
                    ) {
                        return Ok(result);
                    }
                    continue;
                }
                if opts.files_with_matches && match_count > 0 {
                    output.push_str(&color_path(
                        filename,
                        opts.color_enabled(),
                        &opts.color_scheme,
                    ));
                    output.push(if opts.null || opts.null_data {
                        '\0'
                    } else {
                        '\n'
                    });
                    continue;
                }
                if opts.files_without_matches {
                    if match_count == 0 {
                        any_match = true;
                        output.push_str(&color_path(
                            filename,
                            opts.color_enabled(),
                            &opts.color_scheme,
                        ));
                        output.push(if opts.null || opts.null_data {
                            '\0'
                        } else {
                            '\n'
                        });
                    }
                    continue;
                }
                if opts.count_only || opts.count_matches {
                    if count_value == 0 && !opts.include_zero {
                        continue;
                    }
                    if show_filename {
                        output.push_str(&color_path(
                            filename,
                            opts.color_enabled(),
                            &opts.color_scheme,
                        ));
                        output.push(if opts.null { '\0' } else { ':' });
                    }
                    output.push_str(&count_value.to_string());
                    output.push(record_terminator);
                    continue;
                }
            }

            let lines = split_rg_lines(content, opts.crlf, opts.null_data);

            if opts.multiline {
                // Early-exit modes only need to know whether any match exists, so cap
                // collection to 1. Invert mode needs all matches to compute the inverse.
                let collect_limit = if (opts.quiet && !opts.stats || opts.files_with_matches)
                    && !opts.invert_match
                {
                    Some(opts.max_count.unwrap_or(usize::MAX).min(1))
                } else if opts.invert_match {
                    None
                } else {
                    opts.max_count
                };
                let matches = collect_rg_multiline_matches(&regex, content, &lines, collect_limit);
                let match_line_indices = rg_multiline_match_lines(&matches);
                let context_match_lines = rg_unique_sorted_lines(&match_line_indices);

                if opts.invert_match {
                    let matched_line_set: HashSet<usize> =
                        context_match_lines.iter().copied().collect();
                    let mut inverted_match_lines: Vec<usize> = lines
                        .iter()
                        .enumerate()
                        .filter_map(|(line_idx, _)| {
                            (!matched_line_set.contains(&line_idx)).then_some(line_idx)
                        })
                        .collect();
                    if let Some(limit) = opts.max_count
                        && inverted_match_lines.len() > limit
                    {
                        inverted_match_lines.truncate(limit);
                    }
                    match_count = inverted_match_lines.len();
                    count_value = inverted_match_lines.len();
                    let inverted_matches = if opts.count_only || opts.count_matches {
                        inverted_match_lines.len()
                    } else {
                        0
                    };
                    json_matched_lines += inverted_match_lines.len();
                    stats.matches += inverted_matches;
                    stats.matched_lines += inverted_match_lines.len();

                    if match_count > 0 {
                        stats.files_with_matches += 1;
                        if !opts.files_without_matches {
                            any_match = true;
                        }
                    }

                    if opts.quiet {
                        if let Some(result) = rg_quiet_result(
                            &opts,
                            match_count,
                            &mut any_match,
                            &collected_inputs.stderr,
                        ) {
                            return Ok(result);
                        }
                        continue;
                    }
                    if opts.files_with_matches && match_count > 0 {
                        output.push_str(&color_path(
                            filename,
                            opts.color_enabled(),
                            &opts.color_scheme,
                        ));
                        output.push(if opts.null || opts.null_data {
                            '\0'
                        } else {
                            '\n'
                        });
                        continue;
                    }
                    if opts.files_without_matches {
                        if match_count == 0 {
                            any_match = true;
                            output.push_str(&color_path(
                                filename,
                                opts.color_enabled(),
                                &opts.color_scheme,
                            ));
                            output.push(if opts.null || opts.null_data {
                                '\0'
                            } else {
                                '\n'
                            });
                        }
                        continue;
                    }
                    if json_output {
                        if match_count > 0 {
                            write_rg_json_begin(&mut output, filename);
                            for &line_idx in &inverted_match_lines {
                                write_rg_json_match(
                                    &mut output,
                                    filename,
                                    lines[line_idx],
                                    line_idx,
                                    &regex,
                                    opts.replacement.as_deref(),
                                );
                            }
                            write_rg_json_end(
                                &mut output,
                                filename,
                                binary_offset,
                                content.len(),
                                inverted_match_lines.len(),
                                0,
                            );
                            json_searches_with_match += 1;
                        }
                        continue;
                    }
                    if opts.count_only || opts.count_matches {
                        if count_value == 0 && !opts.include_zero {
                            continue;
                        }
                        if show_filename {
                            output.push_str(&color_path(
                                filename,
                                opts.color_enabled(),
                                &opts.color_scheme,
                            ));
                            output.push(if opts.null { '\0' } else { ':' });
                        }
                        output.push_str(&count_value.to_string());
                        output.push(record_terminator);
                        continue;
                    }
                    if opts.quiet {
                        continue;
                    }

                    let line_show_filename = if opts.heading && show_filename && match_count > 0 {
                        if !output.is_empty() {
                            output.push(record_terminator);
                        }
                        output.push_str(&color_path(
                            filename,
                            opts.color_enabled(),
                            &opts.color_scheme,
                        ));
                        output.push(record_terminator);
                        false
                    } else {
                        show_filename
                    };
                    if has_context {
                        if !opts.no_context_separator
                            && !opts.heading
                            && !output.is_empty()
                            && !inverted_match_lines.is_empty()
                        {
                            output.push_str(&opts.context_separator);
                            output.push(record_terminator);
                        }
                        write_rg_context(
                            &mut output,
                            filename,
                            &regex,
                            &lines,
                            &inverted_match_lines,
                            &opts,
                            line_show_filename,
                        );
                    } else {
                        for &line_idx in &inverted_match_lines {
                            write_rg_prefix(
                                &mut output,
                                RgPrefix {
                                    filename,
                                    show_filename: line_show_filename,
                                    line_numbers: opts.line_numbers,
                                    line_idx,
                                    column: None,
                                    byte_offset: if opts.byte_offset {
                                        Some(lines[line_idx].start_offset)
                                    } else {
                                        None
                                    },
                                    separator: opts.field_match_separator.as_str(),
                                    null_path_separator: opts.null,
                                    color: opts.color_enabled(),
                                    color_scheme: &opts.color_scheme,
                                    hyperlink_format: opts.hyperlink_format.as_deref(),
                                },
                            );
                            output.push_str(&format_rg_output_line(
                                lines[line_idx].text,
                                lines[line_idx].match_text,
                                &regex,
                                &opts,
                                true,
                            ));
                            output.push(record_terminator);
                        }
                    }
                    continue;
                }

                match_count = matches.len();
                count_value = matches.len();
                json_matches += matches.len();
                json_matched_lines += match_line_indices.len();
                stats.matches += matches.len();
                stats.matched_lines += match_line_indices.len();

                if match_count > 0 {
                    stats.files_with_matches += 1;
                    if !opts.files_without_matches {
                        any_match = true;
                    }
                }

                if opts.quiet {
                    if let Some(result) = rg_quiet_result(
                        &opts,
                        match_count,
                        &mut any_match,
                        &collected_inputs.stderr,
                    ) {
                        return Ok(result);
                    }
                    continue;
                }
                if opts.files_with_matches && match_count > 0 {
                    output.push_str(&color_path(
                        filename,
                        opts.color_enabled(),
                        &opts.color_scheme,
                    ));
                    output.push(if opts.null || opts.null_data {
                        '\0'
                    } else {
                        '\n'
                    });
                    continue;
                }
                if opts.files_without_matches {
                    if match_count == 0 {
                        any_match = true;
                        output.push_str(&color_path(
                            filename,
                            opts.color_enabled(),
                            &opts.color_scheme,
                        ));
                        output.push(if opts.null || opts.null_data {
                            '\0'
                        } else {
                            '\n'
                        });
                    }
                    continue;
                }
                if json_output {
                    if match_count > 0 {
                        write_rg_json_begin(&mut output, filename);
                        let match_line_set: HashSet<usize> =
                            context_match_lines.iter().copied().collect();
                        let context_lines = if opts.passthru {
                            (0..lines.len())
                                .filter(|line_idx| !match_line_set.contains(line_idx))
                                .collect::<Vec<_>>()
                        } else if has_context {
                            rg_context_line_indices(
                                lines.len(),
                                &context_match_lines,
                                opts.before_context,
                                opts.after_context,
                                false,
                            )
                        } else {
                            Vec::new()
                        };
                        if context_lines.len() > RG_MAX_JSON_CONTEXT_EVENTS {
                            return Ok(ExecResult::err(
                                "rg: too many JSON context events (output capped)\n".to_string(),
                                2,
                            ));
                        }
                        if context_lines.is_empty() {
                            for &mat in &matches {
                                write_rg_json_multiline_match(
                                    &mut output,
                                    filename,
                                    &lines,
                                    mat,
                                    &regex,
                                    opts.replacement.as_deref(),
                                );
                            }
                        } else {
                            let mut match_by_start_line = BTreeMap::new();
                            for &mat in &matches {
                                match_by_start_line.entry(mat.line_idx).or_insert(mat);
                            }
                            let mut event_lines: BTreeSet<usize> =
                                context_lines.into_iter().collect();
                            event_lines.extend(match_by_start_line.keys().copied());
                            for line_idx in event_lines {
                                if let Some(mat) = match_by_start_line.get(&line_idx).copied() {
                                    write_rg_json_multiline_match(
                                        &mut output,
                                        filename,
                                        &lines,
                                        mat,
                                        &regex,
                                        opts.replacement.as_deref(),
                                    );
                                } else {
                                    write_rg_json_context(
                                        &mut output,
                                        filename,
                                        lines[line_idx],
                                        line_idx,
                                    );
                                }
                            }
                        }
                        write_rg_json_end(
                            &mut output,
                            filename,
                            binary_offset,
                            content.len(),
                            match_line_indices.len(),
                            matches.len(),
                        );
                        json_searches_with_match += 1;
                    }
                    continue;
                }
                if opts.count_only || opts.count_matches {
                    if count_value == 0 && !opts.include_zero {
                        continue;
                    }
                    if show_filename {
                        output.push_str(&color_path(
                            filename,
                            opts.color_enabled(),
                            &opts.color_scheme,
                        ));
                        output.push(if opts.null { '\0' } else { ':' });
                    }
                    output.push_str(&count_value.to_string());
                    output.push(record_terminator);
                    continue;
                }
                if opts.quiet {
                    continue;
                }

                let line_show_filename = if opts.heading && show_filename && match_count > 0 {
                    if !output.is_empty() {
                        output.push(record_terminator);
                    }
                    output.push_str(&color_path(
                        filename,
                        opts.color_enabled(),
                        &opts.color_scheme,
                    ));
                    output.push(record_terminator);
                    false
                } else {
                    show_filename
                };
                if opts.passthru {
                    let mut matches_by_start_line: BTreeMap<usize, Vec<RgMultilineMatch<'_>>> =
                        BTreeMap::new();
                    for mat in &matches {
                        matches_by_start_line
                            .entry(mat.line_idx)
                            .or_default()
                            .push(*mat);
                    }
                    let match_line_set: HashSet<usize> =
                        context_match_lines.iter().copied().collect();
                    let mut line_idx = 0usize;
                    while line_idx < lines.len() {
                        if opts.only_matching {
                            if let Some(line_matches) = matches_by_start_line.get(&line_idx) {
                                for &mat in line_matches {
                                    let segments = rg_multiline_match_segments(mat, &regex, &opts);
                                    write_rg_multiline_match_segments(
                                        &mut output,
                                        mat,
                                        &segments,
                                        RgMultilineMatchPrefix {
                                            filename,
                                            show_filename: line_show_filename,
                                            line_numbers: opts.line_numbers,
                                            column: opts.column,
                                            byte_offset: opts.byte_offset,
                                            vimgrep: false,
                                            separator: opts.field_match_separator.as_str(),
                                            null_path_separator: opts.null,
                                            color: opts.color_enabled(),
                                            color_scheme: &opts.color_scheme,
                                            hyperlink_format: opts.hyperlink_format.as_deref(),
                                        },
                                        record_terminator,
                                    );
                                }
                            }
                            if match_line_set.contains(&line_idx) {
                                line_idx += 1;
                                continue;
                            }
                        }
                        let line = lines[line_idx];
                        let matched = match_line_set.contains(&line_idx);
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
                                column: None,
                                byte_offset: if opts.byte_offset {
                                    Some(line.start_offset)
                                } else {
                                    None
                                },
                                separator,
                                null_path_separator: opts.null,
                                color: opts.color_enabled(),
                                color_scheme: &opts.color_scheme,
                                hyperlink_format: opts.hyperlink_format.as_deref(),
                            },
                        );
                        output.push_str(&format_rg_output_line(
                            line.text,
                            line.match_text,
                            &regex,
                            &opts,
                            matched,
                        ));
                        output.push(record_terminator);
                        line_idx += 1;
                    }
                } else if opts.vimgrep {
                    for mat in &matches {
                        if opts.only_matching {
                            let segments = rg_multiline_match_segments(*mat, &regex, &opts);
                            write_rg_multiline_match_segments(
                                &mut output,
                                *mat,
                                &segments,
                                RgMultilineMatchPrefix {
                                    filename,
                                    show_filename: true,
                                    line_numbers: true,
                                    column: true,
                                    byte_offset: false,
                                    vimgrep: true,
                                    separator: opts.field_match_separator.as_str(),
                                    null_path_separator: opts.null,
                                    color: opts.color_enabled(),
                                    color_scheme: &opts.color_scheme,
                                    hyperlink_format: opts.hyperlink_format.as_deref(),
                                },
                                record_terminator,
                            );
                            continue;
                        }
                        write_rg_prefix(
                            &mut output,
                            RgPrefix {
                                filename,
                                show_filename: true,
                                line_numbers: true,
                                line_idx: mat.line_idx,
                                column: Some(mat.column),
                                byte_offset: None,
                                separator: opts.field_match_separator.as_str(),
                                null_path_separator: opts.null,
                                color: opts.color_enabled(),
                                color_scheme: &opts.color_scheme,
                                hyperlink_format: opts.hyperlink_format.as_deref(),
                            },
                        );
                        output.push_str(&format_rg_output_line(
                            lines[mat.line_idx].text,
                            lines[mat.line_idx].match_text,
                            &regex,
                            &opts,
                            true,
                        ));
                        output.push(record_terminator);
                    }
                } else if opts.only_matching {
                    for mat in &matches {
                        let segments = rg_multiline_match_segments(*mat, &regex, &opts);
                        write_rg_multiline_match_segments(
                            &mut output,
                            *mat,
                            &segments,
                            RgMultilineMatchPrefix {
                                filename,
                                show_filename: line_show_filename,
                                line_numbers: opts.line_numbers,
                                column: opts.column,
                                byte_offset: opts.byte_offset,
                                vimgrep: false,
                                separator: opts.field_match_separator.as_str(),
                                null_path_separator: opts.null,
                                color: opts.color_enabled(),
                                color_scheme: &opts.color_scheme,
                                hyperlink_format: opts.hyperlink_format.as_deref(),
                            },
                            record_terminator,
                        );
                    }
                } else if has_context {
                    if !opts.no_context_separator
                        && !opts.heading
                        && !output.is_empty()
                        && !context_match_lines.is_empty()
                    {
                        output.push_str(&opts.context_separator);
                        output.push(record_terminator);
                    }
                    write_rg_context(
                        &mut output,
                        filename,
                        &regex,
                        &lines,
                        &context_match_lines,
                        &opts,
                        line_show_filename,
                    );
                } else if let Some(replacement) = &opts.replacement {
                    for mat in &matches {
                        write_rg_prefix(
                            &mut output,
                            RgPrefix {
                                filename,
                                show_filename: line_show_filename,
                                line_numbers: opts.line_numbers,
                                line_idx: mat.line_idx,
                                column: if opts.column { Some(mat.column) } else { None },
                                byte_offset: if opts.byte_offset {
                                    Some(mat.start_offset)
                                } else {
                                    None
                                },
                                separator: opts.field_match_separator.as_str(),
                                null_path_separator: opts.null,
                                color: opts.color_enabled(),
                                color_scheme: &opts.color_scheme,
                                hyperlink_format: opts.hyperlink_format.as_deref(),
                            },
                        );
                        output.push_str(&format_rg_multiline_replacement(
                            &regex,
                            &lines,
                            *mat,
                            replacement,
                        ));
                        output.push(record_terminator);
                    }
                } else {
                    let mut seen_line_indices = HashSet::new();
                    for mat in &matches {
                        for (line_idx, line) in lines
                            .iter()
                            .enumerate()
                            .take(mat.end_line_idx + 1)
                            .skip(mat.line_idx)
                        {
                            if !seen_line_indices.insert(line_idx) {
                                continue;
                            }
                            write_rg_prefix(
                                &mut output,
                                RgPrefix {
                                    filename,
                                    show_filename: line_show_filename,
                                    line_numbers: opts.line_numbers,
                                    line_idx,
                                    column: if opts.column { Some(mat.column) } else { None },
                                    byte_offset: if opts.byte_offset {
                                        Some(lines[line_idx].start_offset)
                                    } else {
                                        None
                                    },
                                    separator: opts.field_match_separator.as_str(),
                                    null_path_separator: opts.null,
                                    color: opts.color_enabled(),
                                    color_scheme: &opts.color_scheme,
                                    hyperlink_format: opts.hyperlink_format.as_deref(),
                                },
                            );
                            output.push_str(&format_rg_output_line(
                                line.text,
                                line.match_text,
                                &regex,
                                &opts,
                                true,
                            ));
                            output.push(record_terminator);
                        }
                    }
                }
                continue;
            }

            for (line_idx, line) in lines.iter().enumerate() {
                let matched = regex.is_match(line.match_text);
                let matched = if opts.invert_match { !matched } else { matched };

                if !matched {
                    if opts.stop_on_nonmatch && match_count > 0 {
                        break;
                    }
                    continue;
                }

                if let Some(max) = opts.max_count
                    && match_count >= max
                {
                    break;
                }

                match_count += 1;
                let matches_on_line = if opts.invert_match {
                    if opts.count_only || opts.count_matches {
                        1
                    } else {
                        0
                    }
                } else {
                    regex.count_matches(line.match_text)
                };
                if opts.count_matches && !opts.invert_match {
                    count_value += matches_on_line;
                } else {
                    count_value += 1;
                }
                if !opts.invert_match {
                    json_matches += matches_on_line;
                }
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

            if opts.quiet {
                if let Some(result) =
                    rg_quiet_result(&opts, match_count, &mut any_match, &collected_inputs.stderr)
                {
                    return Ok(result);
                }
                continue;
            }
            if opts.files_with_matches && match_count > 0 {
                output.push_str(&color_path(
                    filename,
                    opts.color_enabled(),
                    &opts.color_scheme,
                ));
                output.push(if opts.null || opts.null_data {
                    '\0'
                } else {
                    '\n'
                });
                continue;
            }
            if opts.files_without_matches {
                if match_count == 0 {
                    any_match = true;
                    output.push_str(&color_path(
                        filename,
                        opts.color_enabled(),
                        &opts.color_scheme,
                    ));
                    output.push(if opts.null || opts.null_data {
                        '\0'
                    } else {
                        '\n'
                    });
                }
                continue;
            }
            if json_output {
                if match_count > 0 {
                    write_rg_json_begin(&mut output, filename);
                    let match_line_set: HashSet<usize> = match_lines.iter().copied().collect();
                    let context_lines = if opts.passthru {
                        (0..lines.len())
                            .filter(|line_idx| !match_line_set.contains(line_idx))
                            .collect::<Vec<_>>()
                    } else if has_context {
                        rg_context_line_indices(
                            lines.len(),
                            &match_lines,
                            opts.before_context,
                            opts.after_context,
                            false,
                        )
                    } else {
                        Vec::new()
                    };
                    if context_lines.len() > RG_MAX_JSON_CONTEXT_EVENTS {
                        return Ok(ExecResult::err(
                            "rg: too many JSON context events (output capped)\n".to_string(),
                            2,
                        ));
                    }
                    if context_lines.is_empty() {
                        for &line_idx in &match_lines {
                            write_rg_json_match(
                                &mut output,
                                filename,
                                lines[line_idx],
                                line_idx,
                                &regex,
                                opts.replacement.as_deref(),
                            );
                        }
                    } else {
                        let mut event_lines: BTreeSet<usize> = context_lines.into_iter().collect();
                        event_lines.extend(match_lines.iter().copied());
                        for line_idx in event_lines {
                            if match_line_set.contains(&line_idx) {
                                write_rg_json_match(
                                    &mut output,
                                    filename,
                                    lines[line_idx],
                                    line_idx,
                                    &regex,
                                    opts.replacement.as_deref(),
                                );
                            } else {
                                write_rg_json_context(
                                    &mut output,
                                    filename,
                                    lines[line_idx],
                                    line_idx,
                                );
                            }
                        }
                    }
                    write_rg_json_end(
                        &mut output,
                        filename,
                        binary_offset,
                        content.len(),
                        match_lines.len(),
                        match_lines
                            .iter()
                            .map(|&line_idx| regex.count_matches(lines[line_idx].match_text))
                            .sum(),
                    );
                    json_matched_lines += match_lines.len();
                    json_searches_with_match += 1;
                }
                continue;
            }
            if opts.count_only || opts.count_matches {
                if count_value == 0 && !opts.include_zero {
                    continue;
                }
                if show_filename {
                    output.push_str(&color_path(
                        filename,
                        opts.color_enabled(),
                        &opts.color_scheme,
                    ));
                    output.push(if opts.null { '\0' } else { ':' });
                }
                output.push_str(&count_value.to_string());
                output.push(record_terminator);
                continue;
            }
            if opts.quiet {
                continue;
            }

            let line_show_filename = if opts.heading && show_filename && match_count > 0 {
                if !output.is_empty() {
                    output.push(record_terminator);
                }
                output.push_str(&color_path(
                    filename,
                    opts.color_enabled(),
                    &opts.color_scheme,
                ));
                output.push(record_terminator);
                false
            } else {
                show_filename
            };
            if opts.passthru {
                let match_line_set: HashSet<usize> = match_lines.iter().copied().collect();
                for (line_idx, line) in lines.iter().enumerate() {
                    let matched = match_line_set.contains(&line_idx);
                    if opts.only_matching && !opts.invert_match && matched {
                        regex.for_each_match(line.match_text, |mat| {
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
                                        Some(line.start_offset + mat.start())
                                    } else {
                                        None
                                    },
                                    separator: opts.field_match_separator.as_str(),
                                    null_path_separator: opts.null,
                                    color: opts.color_enabled(),
                                    color_scheme: &opts.color_scheme,
                                    hyperlink_format: opts.hyperlink_format.as_deref(),
                                },
                            );
                            output.push_str(&format_rg_match_text(mat.as_str(), &regex, &opts));
                            output.push(record_terminator);
                            true
                        });
                        continue;
                    }
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
                            column: None,
                            byte_offset: if opts.byte_offset {
                                Some(line.start_offset)
                            } else {
                                None
                            },
                            separator,
                            null_path_separator: opts.null,
                            color: opts.color_enabled(),
                            color_scheme: &opts.color_scheme,
                            hyperlink_format: opts.hyperlink_format.as_deref(),
                        },
                    );
                    output.push_str(&format_rg_output_line(
                        line.text,
                        line.match_text,
                        &regex,
                        &opts,
                        matched,
                    ));
                    output.push(record_terminator);
                }
            } else if opts.vimgrep && !opts.invert_match {
                for &line_idx in &match_lines {
                    if opts.only_matching
                        && let Some(replacement) = &opts.replacement
                        && regex.replacement_matches_exceed_cap(
                            lines[line_idx].match_text,
                            replacement.as_str(),
                        )
                    {
                        output.push_str(&rg_replacement_cap_marker());
                        output.push(record_terminator);
                        continue;
                    }
                    let mut output_col_offset: usize = 0;
                    regex.for_each_match(lines[line_idx].match_text, |mat| {
                        let col = if opts.only_matching && opts.replacement.is_some() {
                            mat.start() + output_col_offset + 1
                        } else {
                            mat.start() + 1
                        };
                        write_rg_prefix(
                            &mut output,
                            RgPrefix {
                                filename,
                                show_filename: true,
                                line_numbers: true,
                                line_idx,
                                column: Some(col),
                                byte_offset: None,
                                separator: opts.field_match_separator.as_str(),
                                null_path_separator: opts.null,
                                color: opts.color_enabled(),
                                color_scheme: &opts.color_scheme,
                                hyperlink_format: opts.hyperlink_format.as_deref(),
                            },
                        );
                        if opts.only_matching {
                            let formatted = format_rg_match_text(mat.as_str(), &regex, &opts);
                            if opts.replacement.is_some() {
                                let orig_len = mat.as_str().len();
                                let repl_len = formatted.len();
                                if repl_len >= orig_len {
                                    output_col_offset += repl_len - orig_len;
                                } else {
                                    output_col_offset =
                                        output_col_offset.saturating_sub(orig_len - repl_len);
                                }
                            }
                            output.push_str(&formatted);
                        } else {
                            output.push_str(&format_rg_output_line(
                                lines[line_idx].text,
                                lines[line_idx].match_text,
                                &regex,
                                &opts,
                                true,
                            ));
                        }
                        output.push(record_terminator);
                        true
                    });
                }
            } else if opts.only_matching && !opts.invert_match {
                for &line_idx in &match_lines {
                    if let Some(replacement) = &opts.replacement
                        && regex.replacement_matches_exceed_cap(
                            lines[line_idx].match_text,
                            replacement.as_str(),
                        )
                    {
                        output.push_str(&rg_replacement_cap_marker());
                        output.push(record_terminator);
                        continue;
                    }
                    regex.for_each_match(lines[line_idx].match_text, |mat| {
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
                                color: opts.color_enabled(),
                                color_scheme: &opts.color_scheme,
                                hyperlink_format: opts.hyperlink_format.as_deref(),
                            },
                        );
                        if let Some(replacement) = &opts.replacement {
                            output
                                .push_str(&regex.replace_first(mat.as_str(), replacement.as_str()));
                        } else {
                            output.push_str(&color_matches(mat.as_str(), &regex, &opts));
                        }
                        output.push(record_terminator);
                        true
                    });
                }
            } else if has_context {
                if !opts.no_context_separator
                    && !opts.heading
                    && !output.is_empty()
                    && !match_lines.is_empty()
                {
                    output.push_str(&opts.context_separator);
                    output.push(record_terminator);
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
                                regex
                                    .find(lines[line_idx].match_text)
                                    .map(|mat| mat.start() + 1)
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
                            color: opts.color_enabled(),
                            color_scheme: &opts.color_scheme,
                            hyperlink_format: opts.hyperlink_format.as_deref(),
                        },
                    );
                    output.push_str(&format_rg_output_line(
                        lines[line_idx].text,
                        lines[line_idx].match_text,
                        &regex,
                        &opts,
                        true,
                    ));
                    output.push(record_terminator);
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
                json_searches_with_match,
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
    use flate2::Compression;
    use flate2::write::GzEncoder;
    use std::collections::HashMap;
    use std::io::Write;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    #[test]
    fn rg_line_iterator_tracks_offsets_without_eager_collection() {
        let mut lines = iter_rg_lines("a\nb\r\nc", true, false);

        let first = lines.next().expect("first line");
        assert_eq!(first.text, "a");
        assert_eq!(first.match_text, "a");
        assert_eq!(first.raw, "a\n");
        assert_eq!(first.start_offset, 0);

        let second = lines.next().expect("second line");
        assert_eq!(second.text, "b\r");
        assert_eq!(second.match_text, "b");
        assert_eq!(second.raw, "b\r\n");
        assert_eq!(second.start_offset, 2);

        let third = lines.next().expect("third line");
        assert_eq!(third.text, "c");
        assert_eq!(third.match_text, "c");
        assert_eq!(third.raw, "c");
        assert_eq!(third.start_offset, 5);
        assert!(lines.next().is_none());
    }

    #[test]
    fn rg_match_stream_callback_can_stop_iteration() {
        let regex = RgMatcher::Rust(Regex::new("a").expect("valid regex"));
        let mut seen = 0usize;

        regex.for_each_match("aaaa", |_| {
            seen += 1;
            false
        });

        assert_eq!(seen, 1);
    }

    #[test]
    fn rg_multiline_collection_stops_at_max_count() {
        let regex = RgMatcher::Rust(Regex::new("a").expect("valid regex"));
        let content = "a\na\na";
        let lines: Vec<_> = iter_rg_lines(content, false, false).collect();

        let matches = collect_rg_multiline_matches(&regex, content, &lines, Some(1));

        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].start_offset, 0);
    }

    #[test]
    fn rg_line_iterator_honors_null_data_records() {
        let lines: Vec<_> = iter_rg_lines("a\0b", false, true).collect();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].text, "a");
        assert_eq!(lines[0].raw, "a\0");
        assert_eq!(lines[0].start_offset, 0);
        assert_eq!(lines[1].text, "b");
        assert_eq!(lines[1].raw, "b");
        assert_eq!(lines[1].start_offset, 2);
    }

    #[test]
    fn glob_brace_alternation_depth_limit_does_not_expand_nested_pattern() {
        let regex = glob_to_regex_with_depth("{a,b}", RG_GLOB_MAX_BRACE_DEPTH);
        assert_eq!(regex, r"^\{a,b\}$");
    }

    #[test]
    fn glob_bracket_classes_escape_regex_set_operator_chars() {
        let regex =
            build_regex_opts(&glob_to_regex("[a&&b].txt"), false).expect("valid ampersand class");
        assert!(regex.is_match("a.txt"));
        assert!(regex.is_match("&.txt"));
        assert!(regex.is_match("b.txt"));

        let regex =
            build_regex_opts(&glob_to_regex("[a~~b].txt"), false).expect("valid tilde class");
        assert!(regex.is_match("a.txt"));
        assert!(regex.is_match("~.txt"));
        assert!(regex.is_match("b.txt"));
    }

    #[test]
    fn glob_bracket_classes_reject_double_hyphen_operator_ranges() {
        assert!(RgGlobRule::parse("[a--b].txt", false, false).is_err());
        assert!(RgTypeGlob::parse("[a--b].txt").is_err());
    }

    #[test]
    fn rg_ignore_rule_set_preserves_parent_then_child_precedence() {
        let parent = Arc::new(RgIgnoreRuleSet::root(vec![
            RgIgnoreRule::parse("*.log", Path::new("/proj"), false)
                .expect("parse")
                .expect("rule"),
        ]));
        let child = RgIgnoreRuleSet::child(
            parent,
            vec![
                RgIgnoreRule::parse("!keep.log", Path::new("/proj/sub"), false)
                    .expect("parse")
                    .expect("rule"),
            ],
        );

        assert!(child.is_ignored(Path::new("/proj/sub/drop.log"), false));
        assert!(!child.is_ignored(Path::new("/proj/sub/keep.log"), false));
        assert_eq!(child.len(), 2);
    }

    #[test]
    fn rg_rejects_too_many_ignore_rules_per_file() {
        let content = test_ignore_content_with_n_rules(RG_IGNORE_RULES_MAX_PER_FILE + 1);
        let err = match parse_rg_ignore_rules(&content, Path::new("/proj"), false) {
            Ok(_) => panic!("expected error"),
            Err(err) => err,
        };
        assert!(err.to_string().contains("too many rules"));
    }

    #[tokio::test]
    async fn rg_rejects_oversized_ignore_files() {
        let fs = InMemoryFs::new();
        fs.mkdir(Path::new("/proj"), true).await.expect("mkdir");
        let content = vec![b'a'; RG_IGNORE_FILE_MAX_BYTES + 1];
        fs.write_file(Path::new("/proj/.ignore"), &content)
            .await
            .expect("write");
        let mut rules = Vec::new();
        let err = load_optional_ignore_file(
            &fs,
            Path::new("/proj/.ignore"),
            Path::new("/proj"),
            false,
            &mut rules,
        )
        .await
        .expect_err("error");
        assert!(err.to_string().contains("ignore file too large"));
    }

    #[test]
    fn colors_rejects_too_long_spec_with_truncated_echo() {
        let mut scheme = RgColorScheme::default();
        let spec = "x".repeat(300);
        let err = scheme.apply(&spec).expect_err("spec should be rejected");
        let msg = err.to_string();
        assert!(msg.contains("invalid color spec"));
        assert!(msg.contains("..."));
        assert!(!msg.contains(&spec));
    }

    #[test]
    fn colors_rejects_extra_delimiters_without_split_collect_amplification() {
        let mut scheme = RgColorScheme::default();
        let err = scheme
            .apply("match:fg:blue:extra")
            .expect_err("spec with extra delimiter should be rejected");
        assert!(err.to_string().contains("invalid color spec"));
    }

    async fn run_rg(args: &[&str], stdin: Option<&str>, files: &[(&str, &[u8])]) -> ExecResult {
        run_rg_with_cwd(args, stdin, files, "/").await
    }

    async fn run_rg_with_cwd(
        args: &[&str],
        stdin: Option<&str>,
        files: &[(&str, &[u8])],
        cwd: &str,
    ) -> ExecResult {
        run_rg_fixture_with_cwd(args, stdin, files, &[], cwd).await
    }

    async fn run_rg_fixture_with_cwd(
        args: &[&str],
        stdin: Option<&str>,
        files: &[(&str, &[u8])],
        symlinks: &[(&str, &str)],
        cwd: &str,
    ) -> ExecResult {
        run_rg_fixture_with_cwd_and_env(args, stdin, files, symlinks, cwd, &[]).await
    }

    async fn run_rg_fixture_with_cwd_and_env(
        args: &[&str],
        stdin: Option<&str>,
        files: &[(&str, &[u8])],
        symlinks: &[(&str, &str)],
        cwd: &str,
        env: &[(&str, &str)],
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
        for (link, target) in symlinks {
            let p = Path::new(link);
            if let Some(parent) = p.parent()
                && parent != Path::new("/")
            {
                let fs_trait: &dyn FileSystem = &*fs;
                let _ = fs_trait.mkdir(parent, true).await;
            }
            let fs_trait: &dyn FileSystem = &*fs;
            fs_trait.symlink(Path::new(target), p).await.unwrap();
        }

        run_rg_with_fs_and_cwd_and_env(args, stdin, fs, cwd, env).await
    }

    async fn run_rg_with_cwd_and_mtimes(
        args: &[&str],
        files: &[(&str, &[u8])],
        mtimes: &[(&str, u64)],
        cwd: &str,
    ) -> ExecResult {
        let fs = Arc::new(InMemoryFs::new());
        let fs_trait: &dyn FileSystem = &*fs;
        for (path, content) in files {
            let p = Path::new(path);
            if let Some(parent) = p.parent()
                && parent != Path::new("/")
            {
                let _ = fs_trait.mkdir(parent, true).await;
            }
            fs_trait.write_file(p, content).await.unwrap();
        }
        for (path, secs) in mtimes {
            let time = std::time::UNIX_EPOCH + std::time::Duration::from_secs(*secs);
            fs_trait
                .set_modified_time(Path::new(path), time)
                .await
                .unwrap();
        }

        run_rg_with_fs_and_cwd(args, None, fs, cwd).await
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
        run_rg_with_fs_and_cwd_and_env(args, stdin, fs, cwd, &[]).await
    }

    async fn run_rg_with_fs_and_cwd_and_env<F>(
        args: &[&str],
        stdin: Option<&str>,
        fs: Arc<F>,
        cwd: &str,
        env: &[(&str, &str)],
    ) -> ExecResult
    where
        F: FileSystem + 'static,
    {
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let env: HashMap<String, String> = env
            .iter()
            .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
            .collect();
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
        UnorderedJsonEvents,
        Stats,
        StatsWithoutBytesSearched,
        ContainsAll(&'static [&'static str]),
    }

    struct RgDiffCase {
        name: &'static str,
        args: &'static [&'static str],
        stdin: Option<&'static str>,
        files: &'static [(&'static str, &'static [u8])],
        cwd: &'static str,
        output: RgDiffOutput,
    }

    struct RgTimedDiffCase {
        name: &'static str,
        args: &'static [&'static str],
        files: &'static [(&'static str, &'static [u8])],
        mtimes: &'static [(&'static str, u64)],
        cwd: &'static str,
        output: RgDiffOutput,
    }

    struct RgSymlinkDiffCase {
        name: &'static str,
        args: &'static [&'static str],
        files: &'static [(&'static str, &'static [u8])],
        symlinks: &'static [(&'static str, &'static str)],
        cwd: &'static str,
        output: RgDiffOutput,
    }

    struct RgEnvDiffCase {
        name: &'static str,
        args: &'static [&'static str],
        stdin: Option<&'static str>,
        files: &'static [(&'static str, &'static [u8])],
        cwd: &'static str,
        env: &'static [(&'static str, &'static str)],
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

    const DIFF_DASH_PATTERN_FILES: &[(&str, &[u8])] = &[("/proj/dash.txt", b"-needle\nneedle\n")];
    const DIFF_DASH_FLAG_FILES: &[(&str, &[u8])] = &[(
        "/proj/dash-flags.txt",
        b"-h\n-V\n--help\n--version\n--pcre2-version\nneedle\n",
    )];

    const DIFF_PCRE2_FILES: &[(&str, &[u8])] = &[(
        "/proj/pcre.txt",
        b"foobar\nfoobaz\nmirror mirror\nmirror window\n",
    )];

    const DIFF_NULL_DATA_FILES: &[(&str, &[u8])] = &[
        ("/proj/a.bin", b"a\0needle\0b\0"),
        ("/proj/b.bin", b"none\0needle again\0"),
        ("/proj/stop.txt", b"none\nneedle\nnone\nneedle again\n"),
    ];

    const DIFF_TWO_CONTEXT_FILES: &[(&str, &[u8])] = &[
        ("/proj/a.txt", b"before\nneedle\nafter\n"),
        ("/proj/b.txt", b"x\nneedle\ny\n"),
    ];

    const DIFF_CONTEXT_GAP_FILES: &[(&str, &[u8])] = &[(
        "/proj/gap.txt",
        b"before\nneedle\nafter\ngap\nbefore2\nneedle again\nafter2\n",
    )];

    const DIFF_SORT_FILES: &[(&str, &[u8])] =
        &[("/proj/a.txt", b"needle\n"), ("/proj/b.txt", b"needle\n")];

    const DIFF_TIMED_SORT_FILES: &[(&str, &[u8])] = &[
        ("/proj/middle.txt", b"needle\n"),
        ("/proj/new.txt", b"needle\n"),
        ("/proj/old.txt", b"needle\n"),
    ];

    const DIFF_TIMED_SORT_MTIMES: &[(&str, u64)] = &[
        ("/proj/old.txt", 1_700_000_000),
        ("/proj/middle.txt", 1_700_000_100),
        ("/proj/new.txt", 1_700_000_200),
    ];
    const DIFF_TIMED_MULTI_ROOT_SORT_FILES: &[(&str, &[u8])] =
        &[("/d1/new.txt", b"needle\n"), ("/d2/old.txt", b"needle\n")];
    const DIFF_TIMED_MULTI_ROOT_SORT_MTIMES: &[(&str, u64)] = &[
        ("/d2/old.txt", 1_700_000_000),
        ("/d1/new.txt", 1_700_000_200),
    ];

    const DIFF_GLOB_CASE_FILES: &[(&str, &[u8])] = &[
        ("/proj/Foo.RS", b"needle\n"),
        ("/proj/a.rs", b"needle\n"),
        ("/proj/a.txt", b"needle\n"),
    ];

    const DIFF_GLOB_CLASS_FILES: &[(&str, &[u8])] = &[
        ("/proj/a.txt", b"needle\n"),
        ("/proj/b.txt", b"needle\n"),
        ("/proj/c.txt", b"needle\n"),
        ("/proj/a.rs", b"needle\n"),
    ];

    const DIFF_GLOB_BRACE_FILES: &[(&str, &[u8])] = &[
        ("/proj/a.rs", b"needle\n"),
        ("/proj/Cargo.toml", b"needle\n"),
        ("/proj/a.txt", b"needle\n"),
    ];

    const DIFF_GLOB_ESCAPE_FILES: &[(&str, &[u8])] = &[
        ("/proj/*.txt", b"needle\n"),
        ("/proj/?.txt", b"needle\n"),
        ("/proj/[x].txt", b"needle\n"),
        ("/proj/{a,b}.txt", b"needle\n"),
        ("/proj/a.txt", b"needle\n"),
        ("/proj/x.txt", b"needle\n"),
    ];

    const DIFF_GLOB_GLOBSTAR_FILES: &[(&str, &[u8])] = &[
        ("/proj/foo.txt", b"needle\n"),
        ("/proj/dir/foo.txt", b"needle\n"),
        ("/proj/dir/bar.txt", b"needle\n"),
    ];

    const DIFF_COMMON_TYPE_FILES: &[(&str, &[u8])] = &[
        ("/proj/data.csv", b"needle\n"),
        ("/proj/Dockerfile", b"needle\n"),
        ("/proj/Makefile", b"needle\n"),
        ("/proj/gnumakefile", b"needle\n"),
        ("/proj/app.rb", b"needle\n"),
        ("/proj/index.php", b"needle\n"),
        ("/proj/schema.xml", b"needle\n"),
        ("/proj/query.sql", b"needle\n"),
        ("/proj/Main.kt", b"needle\n"),
        ("/proj/View.swift", b"needle\n"),
        ("/proj/a.txt", b"needle\n"),
    ];

    const DIFF_MORE_TYPE_FILES: &[(&str, &[u8])] = &[
        ("/proj/CMakeLists.txt", b"needle\n"),
        ("/proj/patch.diff", b"needle\n"),
        ("/proj/app.ini", b"needle\n"),
        ("/proj/build.bat", b"needle\n"),
        ("/proj/config.fish", b"needle\n"),
        ("/proj/schema.graphql", b"needle\n"),
        ("/proj/Main.hs", b"needle\n"),
        ("/proj/script.pl", b"needle\n"),
        ("/proj/a.txt", b"needle\n"),
    ];

    const DIFF_LANGUAGE_TYPE_FILES: &[(&str, &[u8])] = &[
        ("/proj/core.clj", b"needle\n"),
        ("/proj/app.ex", b"needle\n"),
        ("/proj/mod.erl", b"needle\n"),
        ("/proj/math.jl", b"needle\n"),
        ("/proj/lib.nim", b"needle\n"),
        ("/proj/shell.nix", b"needle\n"),
        ("/proj/stats.R", b"needle\n"),
        ("/proj/main.tf", b"needle\n"),
        ("/proj/plugin.vim", b"needle\n"),
        ("/proj/app.zig", b"needle\n"),
        ("/proj/a.txt", b"needle\n"),
    ];

    const DIFF_MORE_LANGUAGE_TYPE_FILES: &[(&str, &[u8])] = &[
        ("/proj/guide.adoc", b"needle\n"),
        ("/proj/pkg.cabal", b"needle\n"),
        ("/proj/server.cr", b"needle\n"),
        ("/proj/Main.elm", b"needle\n"),
        ("/proj/Lib.fs", b"needle\n"),
        ("/proj/build.gradle", b"needle\n"),
        ("/proj/source.ml", b"needle\n"),
        ("/proj/notes.org", b"needle\n"),
        ("/proj/App.res", b"needle\n"),
        ("/proj/Token.sol", b"needle\n"),
        ("/proj/Page.svelte", b"needle\n"),
        ("/proj/widget.vala", b"needle\n"),
        ("/proj/a.txt", b"needle\n"),
    ];

    const DIFF_EXTRA_LANGUAGE_TYPE_FILES: &[(&str, &[u8])] = &[
        ("/proj/proof.v", b"needle\n"),
        ("/proj/kernel.cu", b"needle\n"),
        ("/proj/app.d", b"needle\n"),
        ("/proj/config.dhall", b"needle\n"),
        ("/proj/Main.idr", b"needle\n"),
        ("/proj/Theorem.lean", b"needle\n"),
        ("/proj/model.m", b"needle\n"),
        ("/proj/unit.pas", b"needle\n"),
        ("/proj/View.qml", b"needle\n"),
        ("/proj/style.scss", b"needle\n"),
        ("/proj/theme.styl", b"needle\n"),
        ("/proj/api.thrift", b"needle\n"),
        ("/proj/dom.webidl", b"needle\n"),
        ("/proj/shader.wgsl", b"needle\n"),
        ("/proj/a.txt", b"needle\n"),
    ];

    const DIFF_EARLY_LANGUAGE_TYPE_FILES: &[(&str, &[u8])] = &[
        ("/proj/pkg.ads", b"needle\n"),
        ("/proj/proof.agda", b"needle\n"),
        ("/proj/iface.aidl", b"needle\n"),
        ("/proj/start.S", b"needle\n"),
        ("/proj/page.aspx", b"needle\n"),
        ("/proj/main.dats", b"needle\n"),
        ("/proj/schema.avsc", b"needle\n"),
        ("/proj/script.awk", b"needle\n"),
        ("/proj/BUILD", b"needle\n"),
        ("/proj/recipe.bb", b"needle\n"),
        ("/proj/view.coffee", b"needle\n"),
        ("/proj/ext.pyx", b"needle\n"),
        ("/proj/board.dts", b"needle\n"),
        ("/proj/docker-compose.yml", b"needle\n"),
        ("/proj/a.txt", b"needle\n"),
    ];

    const DIFF_COMMON_LANGUAGE_TYPE_FILES: &[(&str, &[u8])] = &[
        ("/proj/run.cmd", b"needle\n"),
        ("/proj/component.cfc", b"needle\n"),
        ("/proj/view.cshtml", b"needle\n"),
        ("/proj/app.csproj", b"needle\n"),
        ("/proj/board.dtsi", b"needle\n"),
        ("/proj/data.edn", b"needle\n"),
        ("/proj/template.erb", b"needle\n"),
        ("/proj/schema.fbs", b"needle\n"),
        ("/proj/solver.f90", b"needle\n"),
        ("/proj/player.gd", b"needle\n"),
        ("/proj/main.gleam", b"needle\n"),
        ("/proj/BUILD.gn", b"needle\n"),
        ("/proj/settings.gradle.kts", b"needle\n"),
        ("/proj/page.haml", b"needle\n"),
        ("/proj/a.txt", b"needle\n"),
    ];

    const DIFF_FORMAT_TYPE_FILES: &[(&str, &[u8])] = &[
        ("/proj/alire.toml", b"needle\n"),
        ("/proj/Android.bp", b"needle\n"),
        ("/proj/source.bx", b"needle\n"),
        ("/proj/archive.br", b"needle\n"),
        ("/proj/pipeline.bst", b"needle\n"),
        ("/proj/archive.tbz2", b"needle\n"),
        ("/proj/service.did", b"needle\n"),
        ("/proj/game.carp", b"needle\n"),
        ("/proj/data.cbor", b"needle\n"),
        ("/proj/app.ceylon", b"needle\n"),
        ("/proj/model.cml", b"needle\n"),
        ("/proj/page.creole", b"needle\n"),
        ("/proj/topic.ditamap", b"needle\n"),
        ("/proj/cache.dvc", b"needle\n"),
        ("/proj/a.txt", b"needle\n"),
    ];

    const DIFF_ADDITIONAL_FORMAT_TYPE_FILES: &[(&str, &[u8])] = &[
        ("/proj/package.ebuild", b"needle\n"),
        ("/proj/init.el", b"needle\n"),
        ("/proj/script.fnl", b"needle\n"),
        ("/proj/device.fidl", b"needle\n"),
        ("/proj/kernel.fut", b"needle\n"),
        ("/proj/algebra.gap", b"needle\n"),
        ("/proj/project.gpr", b"needle\n"),
        ("/proj/archive.tgz", b"needle\n"),
        ("/proj/header.hh", b"needle\n"),
        ("/proj/main.ha", b"needle\n"),
        ("/proj/template.hbs", b"needle\n"),
        ("/proj/macro.hy", b"needle\n"),
        ("/proj/app.janet", b"needle\n"),
        ("/proj/page.jinja2", b"needle\n"),
        ("/proj/notebook.jl", b"needle\n"),
        ("/proj/analysis.ipynb", b"needle\n"),
        ("/proj/a.txt", b"needle\n"),
    ];

    const DIFF_METADATA_TYPE_FILES: &[(&str, &[u8])] = &[
        ("/proj/rule.k", b"needle\n"),
        ("/proj/Kconfig", b"needle\n"),
        ("/proj/theme.less", b"needle\n"),
        ("/proj/LICENSE", b"needle\n"),
        ("/proj/music.ly", b"needle\n"),
        ("/proj/ir.ll", b"needle\n"),
        ("/proj/package-lock.json", b"needle\n"),
        ("/proj/server.log", b"needle\n"),
        ("/proj/archive.lz4", b"needle\n"),
        ("/proj/archive.lzma", b"needle\n"),
        ("/proj/configure.ac", b"needle\n"),
        ("/proj/page.mako", b"needle\n"),
        ("/proj/tool.1", b"needle\n"),
        ("/proj/meson.build", b"needle\n"),
        ("/proj/app.min.js", b"needle\n"),
        ("/proj/main.mint", b"needle\n"),
        ("/proj/a.txt", b"needle\n"),
    ];

    const DIFF_PROJECT_TYPE_FILES: &[(&str, &[u8])] = &[
        ("/proj/mkfile", b"needle\n"),
        ("/proj/model.ml", b"needle\n"),
        ("/proj/canister.mo", b"needle\n"),
        ("/proj/app.sln", b"needle\n"),
        ("/proj/view.m", b"needle\n"),
        ("/proj/view.mm", b"needle\n"),
        ("/proj/BUILD", b"needle\n"),
        ("/proj/spec.pdf", b"needle\n"),
        ("/proj/messages.po", b"needle\n"),
        ("/proj/module.pod", b"needle\n"),
        ("/proj/figure.eps", b"needle\n"),
        ("/proj/facts.prolog", b"needle\n"),
        ("/proj/profile.ps1", b"needle\n"),
        ("/proj/site.pp", b"needle\n"),
        ("/proj/lib.purs", b"needle\n"),
        ("/proj/project.pri", b"needle\n"),
        ("/proj/a.txt", b"needle\n"),
    ];

    const DIFF_DOC_TYPE_FILES: &[(&str, &[u8])] = &[
        ("/proj/resources.qrc", b"needle\n"),
        ("/proj/window.ui", b"needle\n"),
        ("/proj/main.rkt", b"needle\n"),
        ("/proj/module.rakumod", b"needle\n"),
        ("/proj/guide.rdoc", b"needle\n"),
        ("/proj/README.md", b"needle\n"),
        ("/proj/lib.re", b"needle\n"),
        ("/proj/color.red", b"needle\n"),
        ("/proj/suite.robot", b"needle\n"),
        ("/proj/manual.rst", b"needle\n"),
        ("/proj/page.scdoc", b"needle\n"),
        ("/proj/program.s7i", b"needle\n"),
        ("/proj/view.slim", b"needle\n"),
        ("/proj/template.tpl", b"needle\n"),
        ("/proj/signature.sig", b"needle\n"),
        ("/proj/message.soy", b"needle\n"),
        ("/proj/a.txt", b"needle\n"),
    ];

    const DIFF_TAIL_TYPE_FILES: &[(&str, &[u8])] = &[
        ("/proj/job.spark", b"needle\n"),
        ("/proj/package.spec", b"needle\n"),
        ("/proj/subtitle.ssa", b"needle\n"),
        ("/proj/design.sv", b"needle\n"),
        ("/proj/icon.svg", b"needle\n"),
        ("/proj/bindings.i", b"needle\n"),
        ("/proj/app.service", b"needle\n"),
        ("/proj/tasks.taskpaper", b"needle\n"),
        ("/proj/script.tcl", b"needle\n"),
        ("/proj/paper.tex", b"needle\n"),
        ("/proj/manual.texi", b"needle\n"),
        ("/proj/article.textile", b"needle\n"),
        ("/proj/view.twig", b"needle\n"),
        ("/proj/setup.typoscript", b"needle\n"),
        ("/proj/doc.typ", b"needle\n"),
        ("/proj/scene.usda", b"needle\n"),
        ("/proj/source.vsh", b"needle\n"),
        ("/proj/form.vb", b"needle\n"),
        ("/proj/cache.vcl", b"needle\n"),
        ("/proj/cell.vh", b"needle\n"),
        ("/proj/entity.vhdl", b"needle\n"),
        ("/proj/plugin.vim", b"needle\n"),
        ("/proj/page.mediawiki", b"needle\n"),
        ("/proj/archive.txz", b"needle\n"),
        ("/proj/parser.y", b"needle\n"),
        ("/proj/model.yang", b"needle\n"),
        ("/proj/archive.Z", b"needle\n"),
        ("/proj/init.zsh", b"needle\n"),
        ("/proj/archive.zstd", b"needle\n"),
        ("/proj/a.txt", b"needle\n"),
    ];

    const DIFF_IGNORE_FILES: &[(&str, &[u8])] = &[
        ("/proj/.git/config", b"[core]\n"),
        ("/proj/.git/info/exclude", b"local.txt\n"),
        (
            "/proj/.gitignore",
            b"target/\n*.log\n!keep.log\nvendor/**\n",
        ),
        ("/proj/.ignore", b"src/ignored.txt\n"),
        ("/proj/.rgignore", b"rgonly.txt\n"),
        ("/proj/custom.ignore", b"*.tmp\n"),
        ("/proj/a.txt", b"needle\n"),
        ("/proj/a.log", b"needle\n"),
        ("/proj/keep.log", b"needle\n"),
        ("/proj/target/out.txt", b"needle\n"),
        ("/proj/src/ignored.txt", b"needle\n"),
        ("/proj/rgonly.txt", b"needle\n"),
        ("/proj/local.txt", b"needle\n"),
        ("/proj/vendor/lib.rs", b"needle\n"),
        ("/proj/scratch.tmp", b"needle\n"),
    ];

    const DIFF_PARENT_IGNORE_FILES: &[(&str, &[u8])] = &[
        ("/proj/.git/config", b"[core]\n"),
        ("/proj/.ignore", b"sub/ignored.txt\n"),
        ("/proj/.gitignore", b"sub/vcs.txt\n"),
        ("/proj/sub/ignored.txt", b"needle\n"),
        ("/proj/sub/keep.txt", b"needle\n"),
        ("/proj/sub/vcs.txt", b"needle\n"),
    ];

    const DIFF_GLOBAL_IGNORE_FILES: &[(&str, &[u8])] = &[
        ("/home/.config/git/ignore", b"global.txt\n"),
        (
            "/home/.gitconfig",
            b"[core]\n\texcludesFile = ~/custom-global.ignore\n",
        ),
        ("/home/custom-global.ignore", b"custom.txt\n"),
        ("/proj/.git/config", b"[core]\n"),
        ("/proj/global.txt", b"needle\n"),
        ("/proj/custom.txt", b"needle\n"),
        ("/proj/keep.txt", b"needle\n"),
    ];

    const DIFF_DEFAULT_GLOBAL_IGNORE_FILES: &[(&str, &[u8])] = &[
        ("/home/.config/git/ignore", b"global.txt\n"),
        ("/proj/.git/config", b"[core]\n"),
        ("/proj/global.txt", b"needle\n"),
        ("/proj/keep.txt", b"needle\n"),
    ];

    const DIFF_GLOBAL_IGNORE_ENV: &[(&str, &str)] = &[("HOME", "/home"), ("XDG_CONFIG_HOME", "")];

    const DIFF_BINARY_FILES: &[(&str, &[u8])] = &[
        ("/proj/bin.dat", b"abc\0needle\n"),
        ("/proj/text.txt", b"needle\n"),
    ];

    const DIFF_ENCODING_FILES: &[(&str, &[u8])] = &[
        ("/proj/utf16le.txt", b"n\0e\0e\0d\0l\0e\0\n\0"),
        ("/proj/utf16bom.txt", b"\xff\xfen\0e\0e\0d\0l\0e\0\n\0"),
    ];

    const DIFF_CRLF_FILES: &[(&str, &[u8])] = &[("/proj/crlf.txt", b"needle\r\nother\r\n")];

    const DIFF_UNICODE_FILES: &[(&str, &[u8])] =
        &[("/proj/unicode.txt", "cafe\ncafé\nκαφες\n".as_bytes())];

    const DIFF_MULTILINE_FILES: &[(&str, &[u8])] =
        &[("/proj/multi.txt", b"foo\nbar\nbaz\nxxfoo\nbar\nfoo bar\n")];
    const DIFF_MULTILINE_ALL_MATCH_FILES: &[(&str, &[u8])] =
        &[("/proj/all-multiline.txt", b"foo\nbar\nxxfoo\nbar\n")];

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

    const DIFF_MAX_FILESIZE_FILES: &[(&str, &[u8])] = &[
        ("/proj/small.txt", b"needle\n"),
        ("/proj/big.txt", b"needle long\n"),
    ];

    const DIFF_OUTPUT_MODE_FILES: &[(&str, &[u8])] =
        &[("/proj/output.txt", b"foo1 foo2 bar\nnone\nfoo3 baz\n")];

    const DIFF_JSON_MODE_FILES: &[(&str, &[u8])] = &[
        ("/proj/a.txt", b"before\nfoo bar foo\nafter\n"),
        ("/proj/b.txt", b"foo\n"),
    ];

    const GZIP_NEEDLE: &[u8] = &[
        0x1f, 0x8b, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x02, 0x13, 0x4b, 0xcc, 0x29, 0xc8, 0x48,
        0xe4, 0xca, 0x4b, 0x4d, 0x4d, 0xc9, 0x49, 0xe5, 0x02, 0x00, 0x08, 0x8e, 0x37, 0xc8, 0x0d,
        0x00, 0x00, 0x00,
    ];

    const DIFF_GZIP_FILES: &[(&str, &[u8])] = &[
        ("/proj/plain.txt", b"needle\n"),
        ("/proj/compressed.txt.gz", GZIP_NEEDLE),
    ];
    const DIFF_GZIP_EXTENSION_FILES: &[(&str, &[u8])] = &[
        ("/proj/compressed.txt", GZIP_NEEDLE),
        ("/proj/fake.gz", b"needle\n"),
    ];

    fn gzip_bytes(payload: &[u8]) -> Vec<u8> {
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(payload).expect("write gzip payload");
        encoder.finish().expect("finish gzip payload")
    }

    fn search_zip_opts() -> RgOptions {
        let args = vec!["needle".to_string()];
        let mut opts = RgOptions::parse(&args).expect("parse rg options");
        opts.search_zip = true;
        opts
    }

    #[test]
    fn rg_search_zip_rejects_large_gzip_decompression() {
        let opts = search_zip_opts();
        let payload = vec![b'a'; RG_GZIP_MAX_DECOMPRESSED_BYTES + 1];
        let compressed = gzip_bytes(&payload);
        let err = rg_search_bytes(Path::new("/payload.gz"), &compressed, &opts)
            .expect_err("must reject oversized gzip");
        assert!(err.contains("exceeds"));
    }

    #[test]
    fn rg_search_zip_rejects_suspicious_decompression_ratio() {
        let opts = search_zip_opts();
        let payload = vec![b'a'; 300_000];
        let compressed = gzip_bytes(&payload);
        let err = rg_search_bytes(Path::new("/payload.gz"), &compressed, &opts)
            .expect_err("must reject suspicious ratio");
        assert!(err.contains("ratio exceeds"));
    }

    #[tokio::test]
    async fn rg_follow_rejects_recursive_symlink_escape() {
        let result = run_rg_fixture_with_cwd(
            &["-L", "needle", "workspace"],
            None,
            &[
                ("/workspace/public.txt", b"needle public\n"),
                ("/secret/flag.txt", b"needle secret\n"),
            ],
            &[
                ("/workspace/flag.txt", "../secret/flag.txt"),
                ("/workspace/flagdir", "../secret"),
            ],
            "/",
        )
        .await;

        assert!(result.stdout.contains("workspace/public.txt"));
        assert!(!result.stdout.contains("secret"));
        assert!(!result.stdout.contains("flag.txt"));
        assert!(!result.stdout.contains("flagdir"));
    }

    #[tokio::test]
    async fn rg_follow_rejects_explicit_symlink_escape() {
        let result = run_rg_fixture_with_cwd(
            &["-L", "needle", "workspace/flag.txt"],
            None,
            &[("/secret/flag.txt", b"needle secret\n")],
            &[("/workspace/flag.txt", "../secret/flag.txt")],
            "/",
        )
        .await;

        assert!(!result.stdout.contains("secret"));
        assert!(!result.stdout.contains("flag.txt"));
    }

    const DIFF_SYMLINK_FILES: &[(&str, &[u8])] = &[
        ("/proj/targets/file.txt", b"needle\n"),
        ("/proj/targets/dir/nested.txt", b"needle\n"),
        ("/proj/plain.txt", b"needle\n"),
    ];

    const DIFF_SYMLINKS: &[(&str, &str)] = &[
        ("/proj/link.txt", "targets/file.txt"),
        ("/proj/linkdir", "targets/dir"),
    ];

    const RG_SYMLINK_DIFF_CASES: &[RgSymlinkDiffCase] = &[
        RgSymlinkDiffCase {
            name: "recursive skips symlink entries by default",
            args: &["needle", "proj"],
            files: DIFF_SYMLINK_FILES,
            symlinks: DIFF_SYMLINKS,
            cwd: "/",
            output: RgDiffOutput::UnorderedLines,
        },
        RgSymlinkDiffCase {
            name: "recursive follow searches symlink entries",
            args: &["-L", "needle", "proj"],
            files: DIFF_SYMLINK_FILES,
            symlinks: DIFF_SYMLINKS,
            cwd: "/",
            output: RgDiffOutput::UnorderedLines,
        },
        RgSymlinkDiffCase {
            name: "no follow disables recursive symlink following",
            args: &["-L", "--no-follow", "needle", "proj"],
            files: DIFF_SYMLINK_FILES,
            symlinks: DIFF_SYMLINKS,
            cwd: "/",
            output: RgDiffOutput::UnorderedLines,
        },
        RgSymlinkDiffCase {
            name: "explicit file symlink requires follow",
            args: &["-L", "needle", "proj/link.txt"],
            files: DIFF_SYMLINK_FILES,
            symlinks: DIFF_SYMLINKS,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgSymlinkDiffCase {
            name: "explicit directory symlink requires follow",
            args: &["-L", "needle", "proj/linkdir"],
            files: DIFF_SYMLINK_FILES,
            symlinks: DIFF_SYMLINKS,
            cwd: "/",
            output: RgDiffOutput::UnorderedLines,
        },
        RgSymlinkDiffCase {
            name: "files list follows symlinks with follow",
            args: &["--files", "-L", "proj"],
            files: DIFF_SYMLINK_FILES,
            symlinks: DIFF_SYMLINKS,
            cwd: "/",
            output: RgDiffOutput::UnorderedLines,
        },
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
            name: "dash delimiter permits pattern starting with dash",
            args: &["--", "-needle", "proj/dash.txt"],
            stdin: None,
            files: DIFF_DASH_PATTERN_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "dash delimiter treats short help as pattern",
            args: &["--", "-h", "proj/dash-flags.txt"],
            stdin: None,
            files: DIFF_DASH_FLAG_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "dash delimiter treats short version as pattern",
            args: &["--", "-V", "proj/dash-flags.txt"],
            stdin: None,
            files: DIFF_DASH_FLAG_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "dash delimiter treats long help as pattern",
            args: &["--", "--help", "proj/dash-flags.txt"],
            stdin: None,
            files: DIFF_DASH_FLAG_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "dash delimiter treats long version as pattern",
            args: &["--", "--version", "proj/dash-flags.txt"],
            stdin: None,
            files: DIFF_DASH_FLAG_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "dash delimiter treats pcre2 version as pattern",
            args: &["--", "--pcre2-version", "proj/dash-flags.txt"],
            stdin: None,
            files: DIFF_DASH_FLAG_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "generate bash completion",
            args: &["--generate=complete-bash"],
            stdin: None,
            files: &[],
            cwd: "/",
            output: RgDiffOutput::ContainsAll(&[
                "_rg()",
                "--generate",
                "--regexp",
                "--glob",
                "--no-pre",
                "--null-data",
                "--glob-case-insensitive",
            ]),
        },
        RgDiffCase {
            name: "generate fish completion",
            args: &["--generate", "complete-fish"],
            stdin: None,
            files: &[],
            cwd: "/",
            output: RgDiffOutput::ContainsAll(&["complete -c rg", "generate", "regexp"]),
        },
        RgDiffCase {
            name: "generate zsh completion",
            args: &["--generate=complete-zsh"],
            stdin: None,
            files: &[],
            cwd: "/",
            output: RgDiffOutput::ContainsAll(&[
                "#compdef rg",
                "_rg()",
                "--generate",
                "--no-search-zip",
            ]),
        },
        RgDiffCase {
            name: "generate powershell completion",
            args: &["--generate=complete-powershell"],
            stdin: None,
            files: &[],
            cwd: "/",
            output: RgDiffOutput::ContainsAll(&[
                "Register-ArgumentCompleter",
                "--generate",
                "--regexp",
            ]),
        },
        RgDiffCase {
            name: "generate man page",
            args: &["--generate=man"],
            stdin: None,
            files: &[],
            cwd: "/",
            output: RgDiffOutput::ContainsAll(&[".TH RG", ".SH NAME", "ripgrep"]),
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
            name: "quiet keeps missing file diagnostics when match found",
            args: &["-q", "needle", "proj/missing.txt", "proj/a.txt"],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "quiet no messages suppresses missing file diagnostics",
            args: &[
                "--no-messages",
                "-q",
                "needle",
                "proj/missing.txt",
                "proj/a.txt",
            ],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
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
            name: "explicit file ignores exclude glob",
            args: &["-g", "!*.txt", "needle", "proj/a.txt"],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "explicit file ignores include glob",
            args: &["-g", "*.rs", "needle", "proj/a.txt"],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "files explicit file ignores exclude glob",
            args: &["--files", "-g", "!*.txt", "proj/a.txt"],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "explicit directory still applies exclude glob",
            args: &["-g", "!*.txt", "needle", "proj"],
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
            name: "glob last include wins",
            args: &["--files", "-g", "!*.rs", "-g", "*.rs", "proj"],
            stdin: None,
            files: DIFF_GLOB_CASE_FILES,
            cwd: "/",
            output: RgDiffOutput::UnorderedLines,
        },
        RgDiffCase {
            name: "glob last exclude wins",
            args: &["--files", "-g", "*.rs", "-g", "!*.rs", "proj"],
            stdin: None,
            files: DIFF_GLOB_CASE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "iglob is case insensitive",
            args: &["--files", "--iglob", "*.rs", "proj"],
            stdin: None,
            files: DIFF_GLOB_CASE_FILES,
            cwd: "/",
            output: RgDiffOutput::UnorderedLines,
        },
        RgDiffCase {
            name: "glob case insensitive applies after glob",
            args: &["--files", "-g", "*.rs", "--glob-case-insensitive", "proj"],
            stdin: None,
            files: DIFF_GLOB_CASE_FILES,
            cwd: "/",
            output: RgDiffOutput::UnorderedLines,
        },
        RgDiffCase {
            name: "glob case insensitive can be disabled",
            args: &[
                "--files",
                "-g",
                "*.rs",
                "--glob-case-insensitive",
                "--no-glob-case-insensitive",
                "proj",
            ],
            stdin: None,
            files: DIFF_GLOB_CASE_FILES,
            cwd: "/",
            output: RgDiffOutput::UnorderedLines,
        },
        RgDiffCase {
            name: "glob bracket class",
            args: &["--files", "-g", "[ab].txt", "proj"],
            stdin: None,
            files: DIFF_GLOB_CLASS_FILES,
            cwd: "/",
            output: RgDiffOutput::UnorderedLines,
        },
        RgDiffCase {
            name: "glob negated bracket class",
            args: &["--files", "-g", "[!a].txt", "proj"],
            stdin: None,
            files: DIFF_GLOB_CLASS_FILES,
            cwd: "/",
            output: RgDiffOutput::UnorderedLines,
        },
        RgDiffCase {
            name: "glob brace alternation",
            args: &["--files", "-g", "*.{rs,toml}", "proj"],
            stdin: None,
            files: DIFF_GLOB_BRACE_FILES,
            cwd: "/",
            output: RgDiffOutput::UnorderedLines,
        },
        RgDiffCase {
            name: "glob escaped star",
            args: &["--files", "-g", r"\*.txt", "proj"],
            stdin: None,
            files: DIFF_GLOB_ESCAPE_FILES,
            cwd: "/",
            output: RgDiffOutput::UnorderedLines,
        },
        RgDiffCase {
            name: "glob escaped question",
            args: &["--files", "-g", r"\?.txt", "proj"],
            stdin: None,
            files: DIFF_GLOB_ESCAPE_FILES,
            cwd: "/",
            output: RgDiffOutput::UnorderedLines,
        },
        RgDiffCase {
            name: "glob escaped class brackets",
            args: &["--files", "-g", r"\[x\].txt", "proj"],
            stdin: None,
            files: DIFF_GLOB_ESCAPE_FILES,
            cwd: "/",
            output: RgDiffOutput::UnorderedLines,
        },
        RgDiffCase {
            name: "glob escaped brace alternation",
            args: &["--files", "-g", r"\{a,b\}.txt", "proj"],
            stdin: None,
            files: DIFF_GLOB_ESCAPE_FILES,
            cwd: "/",
            output: RgDiffOutput::UnorderedLines,
        },
        RgDiffCase {
            name: "globstar slash matches zero or more directories",
            args: &["--files", "-g", "**/foo.txt", "."],
            stdin: None,
            files: DIFF_GLOB_GLOBSTAR_FILES,
            cwd: "/proj",
            output: RgDiffOutput::UnorderedLines,
        },
        RgDiffCase {
            name: "glob anchored to cwd",
            args: &["--files", "-g", "/foo.txt", "."],
            stdin: None,
            files: DIFF_GLOB_GLOBSTAR_FILES,
            cwd: "/proj",
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
            name: "files with matches then count uses count",
            args: &["-l", "-c", "needle", "proj"],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::UnorderedLines,
        },
        RgDiffCase {
            name: "count then files with matches uses files",
            args: &["-c", "-l", "needle", "proj"],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::UnorderedLines,
        },
        RgDiffCase {
            name: "files with matches then count matches uses count matches",
            args: &["-l", "--count-matches", "needle", "proj"],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::UnorderedLines,
        },
        RgDiffCase {
            name: "count matches then files with matches uses files",
            args: &["--count-matches", "-l", "needle", "proj"],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::UnorderedLines,
        },
        RgDiffCase {
            name: "files without matches then count uses count",
            args: &["--files-without-match", "-c", "needle", "proj"],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::UnorderedLines,
        },
        RgDiffCase {
            name: "count then files without matches uses files",
            args: &["-c", "--files-without-match", "needle", "proj"],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::UnorderedLines,
        },
        RgDiffCase {
            name: "files without then files with uses files with",
            args: &["--files-without-match", "-l", "needle", "proj"],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::UnorderedLines,
        },
        RgDiffCase {
            name: "files with then files without uses files without",
            args: &["-l", "--files-without-match", "needle", "proj"],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::UnorderedLines,
        },
        RgDiffCase {
            name: "count then count matches uses count matches",
            args: &["-c", "--count-matches", "needle", "proj"],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::UnorderedLines,
        },
        RgDiffCase {
            name: "count matches then count uses count",
            args: &["--count-matches", "-c", "needle", "proj"],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::UnorderedLines,
        },
        RgDiffCase {
            name: "json then files with matches uses files",
            args: &["--json", "-l", "needle", "proj"],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::UnorderedLines,
        },
        RgDiffCase {
            name: "files with matches then json uses json",
            args: &["-l", "--json", "needle", "proj/a.txt"],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::JsonEvents,
        },
        RgDiffCase {
            name: "json then count uses count",
            args: &["--json", "-c", "needle", "proj"],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::UnorderedLines,
        },
        RgDiffCase {
            name: "count then json uses json",
            args: &["-c", "--json", "needle", "proj/a.txt"],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::JsonEvents,
        },
        RgDiffCase {
            name: "json then no json restores normal output",
            args: &["--json", "--no-json", "needle", "proj/a.txt"],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "list files then count uses count",
            args: &["--files", "-c", "needle", "proj"],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::UnorderedLines,
        },
        RgDiffCase {
            name: "count then list files uses files",
            args: &["-c", "--files", "proj"],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::UnorderedLines,
        },
        RgDiffCase {
            name: "list files then json uses json",
            args: &["--files", "--json", "needle", "proj/a.txt"],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::JsonEvents,
        },
        RgDiffCase {
            name: "json then list files uses files",
            args: &["--json", "--files", "proj"],
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
            name: "color always highlights matches",
            args: &["--color=always", "needle", "proj/a.txt"],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "color always highlights prefixes",
            args: &["--color=always", "-n", "--column", "needle", "proj/a.txt"],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "color always only matching",
            args: &["--color=always", "-o", "needle", "proj/a.txt"],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "color always files",
            args: &["--color=always", "--files", "proj"],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::UnorderedLines,
        },
        RgDiffCase {
            name: "colors custom match fg",
            args: &[
                "--color=always",
                "--colors",
                "match:fg:blue",
                "needle",
                "proj/a.txt",
            ],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "colors custom highlight fg",
            args: &[
                "--color=always",
                "--colors",
                "highlight:fg:blue",
                "needle",
                "proj/a.txt",
            ],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "colors custom match bg and nobold",
            args: &[
                "--color=always",
                "--colors",
                "match:bg:red",
                "--colors",
                "match:style:nobold",
                "needle",
                "proj/a.txt",
            ],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "colors custom prefixes",
            args: &[
                "--color=always",
                "--colors",
                "path:fg:green",
                "--colors",
                "line:fg:yellow",
                "--colors",
                "column:fg:cyan",
                "-H",
                "-n",
                "--column",
                "needle",
                "proj/a.txt",
            ],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "colors none",
            args: &[
                "--color=always",
                "--colors",
                "match:none",
                "needle",
                "proj/a.txt",
            ],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "colors style underline",
            args: &[
                "--color=always",
                "--colors",
                "match:style:underline",
                "needle",
                "proj/a.txt",
            ],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "colors style italic",
            args: &[
                "--color=always",
                "--colors",
                "match:style:italic",
                "needle",
                "proj/a.txt",
            ],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "colors style intense",
            args: &[
                "--color=always",
                "--colors",
                "match:fg:blue",
                "--colors",
                "match:style:intense",
                "needle",
                "proj/a.txt",
            ],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "colors rgb foreground",
            args: &[
                "--color=always",
                "--colors",
                "match:fg:200,100,50",
                "needle",
                "proj/a.txt",
            ],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "colors ansi foreground number",
            args: &[
                "--color=always",
                "--colors",
                "match:fg:5",
                "needle",
                "proj/a.txt",
            ],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "colors ansi background number",
            args: &[
                "--color=always",
                "--colors",
                "match:bg:5",
                "needle",
                "proj/a.txt",
            ],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "colors ansi foreground hex number",
            args: &[
                "--color=always",
                "--colors",
                "match:fg:0xff",
                "needle",
                "proj/a.txt",
            ],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "colors ansi background hex number",
            args: &[
                "--color=always",
                "--colors",
                "match:bg:0x05",
                "needle",
                "proj/a.txt",
            ],
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
            name: "quiet count suppresses output",
            args: &["-q", "-c", "needle", "proj/a.txt"],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "quiet count matches suppresses output",
            args: &["-q", "--count-matches", "needle", "proj/a.txt"],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "quiet files with matches suppresses output",
            args: &["-q", "-l", "needle", "proj/a.txt", "proj/b.txt"],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "quiet files without match suppresses output",
            args: &[
                "-q",
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
            name: "quiet files without match hit exits success",
            args: &["-q", "--files-without-match", "needle", "proj/a.txt"],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "quiet files without match miss exits failure",
            args: &["-q", "--files-without-match", "needle", "proj/b.txt"],
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
            name: "invert match stats counts zero matches",
            args: &["-v", "--stats", "needle", "proj/a.txt"],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::Stats,
        },
        RgDiffCase {
            name: "invert count stats counts inverted lines",
            args: &["-v", "-c", "--stats", "needle", "proj/a.txt"],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::Stats,
        },
        RgDiffCase {
            name: "json invert match counts zero submatches",
            args: &["--json", "-v", "foo", "proj/a.txt"],
            stdin: None,
            files: DIFF_JSON_MODE_FILES,
            cwd: "/",
            output: RgDiffOutput::JsonEvents,
        },
        RgDiffCase {
            name: "no invert match disables invert",
            args: &["-v", "--no-invert-match", "needle", "proj/a.txt"],
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
            name: "max count only matching",
            args: &["-m1", "-o", "needle", "proj/a.txt"],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "max count count matches",
            args: &["-m1", "--count-matches", "needle", "proj/a.txt"],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "max count passthru",
            args: &["-m1", "--passthru", "needle", "proj/a.txt"],
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
            name: "no column disables column",
            args: &[
                "--column",
                "--no-column",
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
            name: "no byte offset disables byte offset",
            args: &["-b", "--no-byte-offset", "needle", "proj/a.txt"],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "no fixed strings reenables regex",
            args: &["-F", "--no-fixed-strings", "need.e", "proj/a.txt"],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "no json disables json",
            args: &["--json", "--no-json", "needle", "proj/a.txt"],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "no stats disables stats",
            args: &["--stats", "--no-stats", "needle", "proj/a.txt"],
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
            name: "maxdepth alias",
            args: &["--maxdepth=1", "needle", "proj"],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::UnorderedLines,
        },
        RgDiffCase {
            name: "short max depth attached",
            args: &["-d1", "needle", "proj"],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::UnorderedLines,
        },
        RgDiffCase {
            name: "short max depth zero",
            args: &["-d", "0", "needle", "proj"],
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
            name: "files list short max depth",
            args: &["--files", "-d1", "proj"],
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
            name: "passthrough alias",
            args: &["--passthrough", "needle", "proj/a.txt", "proj/b.txt"],
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
            name: "no trim disables trim",
            args: &["--trim", "--no-trim", "needle", "proj/trim.txt"],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "one file system accepted",
            args: &["--one-file-system", "needle", "proj/a.txt"],
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
            name: "context separator between discontiguous groups",
            args: &[
                "-n",
                "-C1",
                "--context-separator=@@",
                "needle",
                "proj/gap.txt",
            ],
            stdin: None,
            files: DIFF_CONTEXT_GAP_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "no context separator",
            args: &[
                "-n",
                "-C1",
                "--no-context-separator",
                "needle",
                "proj/gap.txt",
            ],
            stdin: None,
            files: DIFF_CONTEXT_GAP_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "no context separator between files",
            args: &[
                "-n",
                "-C1",
                "--no-context-separator",
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
            name: "context separator empty still prints blank line",
            args: &[
                "-n",
                "-C1",
                "--context-separator=",
                "needle",
                "proj/gap.txt",
            ],
            stdin: None,
            files: DIFF_CONTEXT_GAP_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "context separator escape",
            args: &[
                "-n",
                "-C1",
                r"--context-separator=\t",
                "needle",
                "proj/gap.txt",
            ],
            stdin: None,
            files: DIFF_CONTEXT_GAP_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "context separator last flag wins",
            args: &[
                "-n",
                "-C1",
                "--no-context-separator",
                "--context-separator=@@",
                "needle",
                "proj/gap.txt",
            ],
            stdin: None,
            files: DIFF_CONTEXT_GAP_FILES,
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
            name: "field match separator escape",
            args: &[
                "-n",
                r"--field-match-separator=\x7F",
                "needle",
                "proj/a.txt",
            ],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "field match separator high byte escape",
            args: &[
                "-n",
                r"--field-match-separator=\x80",
                "needle",
                "proj/a.txt",
            ],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "field match separator utf8 byte escape",
            args: &[
                "-n",
                r"--field-match-separator=\xC2\xA9",
                "needle",
                "proj/a.txt",
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
            name: "field context separator escape",
            args: &[
                "-n",
                "-C1",
                r"--field-context-separator=\t",
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
            name: "invalid sort value",
            args: &["--sort", "junk", "needle", "proj"],
            stdin: None,
            files: DIFF_SORT_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "invalid reverse sort value",
            args: &["--sortr", "junk", "needle", "proj"],
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
            name: "null short files with matches",
            args: &["-0", "-l", "needle", "proj/a.txt", "proj/src/main.rs"],
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
            name: "null data basic",
            args: &["--null-data", "needle", "proj/a.bin"],
            stdin: None,
            files: DIFF_NULL_DATA_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "null data line numbers",
            args: &["--null-data", "-n", "needle", "proj/a.bin"],
            stdin: None,
            files: DIFF_NULL_DATA_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "null data only matching",
            args: &["--null-data", "-o", "needle", "proj/a.bin"],
            stdin: None,
            files: DIFF_NULL_DATA_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "null data count",
            args: &["--null-data", "-c", "needle", "proj/a.bin"],
            stdin: None,
            files: DIFF_NULL_DATA_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "null data files with matches",
            args: &["--null-data", "-l", "needle", "proj/a.bin", "proj/b.bin"],
            stdin: None,
            files: DIFF_NULL_DATA_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "null data context",
            args: &["--null-data", "-A1", "needle", "proj/a.bin"],
            stdin: None,
            files: DIFF_NULL_DATA_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "stop on nonmatch",
            args: &["--stop-on-nonmatch", "needle", "proj/stop.txt"],
            stdin: None,
            files: DIFF_NULL_DATA_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
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
            name: "json replacement submatches",
            args: &["--json", "-r", "<$0>", "foo", "proj/a.txt"],
            stdin: None,
            files: DIFF_JSON_MODE_FILES,
            cwd: "/",
            output: RgDiffOutput::JsonEvents,
        },
        RgDiffCase {
            name: "json context events",
            args: &["--json", "-n", "-C1", "foo", "proj/a.txt"],
            stdin: None,
            files: DIFF_JSON_MODE_FILES,
            cwd: "/",
            output: RgDiffOutput::JsonEvents,
        },
        RgDiffCase {
            name: "json passthru context events",
            args: &["--json", "--passthru", "foo", "proj/a.txt"],
            stdin: None,
            files: DIFF_JSON_MODE_FILES,
            cwd: "/",
            output: RgDiffOutput::JsonEvents,
        },
        RgDiffCase {
            name: "json summary counts files with matches",
            args: &["--json", "foo", "proj/a.txt", "proj/b.txt"],
            stdin: None,
            files: DIFF_JSON_MODE_FILES,
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
            name: "stats quiet count suppresses count output",
            args: &["--stats", "-q", "-c", "needle", "proj/a.txt", "proj/b.txt"],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::Stats,
        },
        RgDiffCase {
            name: "stats files with matches",
            args: &["--stats", "-l", "needle", "proj/a.txt", "proj/b.txt"],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::Stats,
        },
        RgDiffCase {
            name: "stats files without match",
            args: &[
                "--stats",
                "--files-without-match",
                "needle",
                "proj/a.txt",
                "proj/b.txt",
            ],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::Stats,
        },
        RgDiffCase {
            name: "stats include zero count",
            args: &[
                "--stats",
                "--include-zero",
                "-c",
                "needle",
                "proj/a.txt",
                "proj/b.txt",
            ],
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
            name: "hidden short dot",
            args: &["-.", "needle", "proj"],
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
            name: "type all searches recognized types",
            args: &["--type", "all", "needle", "proj/lang"],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::UnorderedLines,
        },
        RgDiffCase {
            name: "type all includes added custom types",
            args: &[
                "--type-add",
                "foo:*.foo",
                "--type",
                "all",
                "needle",
                "proj/lang",
            ],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::UnorderedLines,
        },
        RgDiffCase {
            name: "type not all searches unrecognized types",
            args: &["--type-not", "all", "needle", "proj/lang"],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::UnorderedLines,
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
            name: "type csv",
            args: &["-t", "csv", "needle", "proj"],
            stdin: None,
            files: DIFF_COMMON_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type docker",
            args: &["-t", "docker", "needle", "proj"],
            stdin: None,
            files: DIFF_COMMON_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type make",
            args: &["-t", "make", "needle", "proj"],
            stdin: None,
            files: DIFF_COMMON_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::UnorderedLines,
        },
        RgDiffCase {
            name: "type ruby",
            args: &["-t", "ruby", "needle", "proj"],
            stdin: None,
            files: DIFF_COMMON_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type php",
            args: &["-t", "php", "needle", "proj"],
            stdin: None,
            files: DIFF_COMMON_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type xml",
            args: &["-t", "xml", "needle", "proj"],
            stdin: None,
            files: DIFF_COMMON_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type sql",
            args: &["-t", "sql", "needle", "proj"],
            stdin: None,
            files: DIFF_COMMON_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type kotlin",
            args: &["-t", "kotlin", "needle", "proj"],
            stdin: None,
            files: DIFF_COMMON_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type swift",
            args: &["-t", "swift", "needle", "proj"],
            stdin: None,
            files: DIFF_COMMON_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type cmake",
            args: &["-t", "cmake", "needle", "proj"],
            stdin: None,
            files: DIFF_MORE_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type diff",
            args: &["-t", "diff", "needle", "proj"],
            stdin: None,
            files: DIFF_MORE_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type config",
            args: &["-t", "config", "needle", "proj"],
            stdin: None,
            files: DIFF_MORE_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type bat",
            args: &["-t", "bat", "needle", "proj"],
            stdin: None,
            files: DIFF_MORE_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type fish",
            args: &["-t", "fish", "needle", "proj"],
            stdin: None,
            files: DIFF_MORE_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type graphql",
            args: &["-t", "graphql", "needle", "proj"],
            stdin: None,
            files: DIFF_MORE_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type haskell",
            args: &["-t", "haskell", "needle", "proj"],
            stdin: None,
            files: DIFF_MORE_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type perl",
            args: &["-t", "perl", "needle", "proj"],
            stdin: None,
            files: DIFF_MORE_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type clojure",
            args: &["-t", "clojure", "needle", "proj"],
            stdin: None,
            files: DIFF_LANGUAGE_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type elixir",
            args: &["-t", "elixir", "needle", "proj"],
            stdin: None,
            files: DIFF_LANGUAGE_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type erlang",
            args: &["-t", "erlang", "needle", "proj"],
            stdin: None,
            files: DIFF_LANGUAGE_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type julia",
            args: &["-t", "julia", "needle", "proj"],
            stdin: None,
            files: DIFF_LANGUAGE_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type nim",
            args: &["-t", "nim", "needle", "proj"],
            stdin: None,
            files: DIFF_LANGUAGE_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type nix",
            args: &["-t", "nix", "needle", "proj"],
            stdin: None,
            files: DIFF_LANGUAGE_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type r",
            args: &["-t", "r", "needle", "proj"],
            stdin: None,
            files: DIFF_LANGUAGE_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type terraform alias",
            args: &["-t", "tf", "needle", "proj"],
            stdin: None,
            files: DIFF_LANGUAGE_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type vim",
            args: &["-t", "vim", "needle", "proj"],
            stdin: None,
            files: DIFF_LANGUAGE_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type zig",
            args: &["-t", "zig", "needle", "proj"],
            stdin: None,
            files: DIFF_LANGUAGE_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type asciidoc",
            args: &["-t", "asciidoc", "needle", "proj"],
            stdin: None,
            files: DIFF_MORE_LANGUAGE_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type cabal",
            args: &["-t", "cabal", "needle", "proj"],
            stdin: None,
            files: DIFF_MORE_LANGUAGE_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type crystal",
            args: &["-t", "crystal", "needle", "proj"],
            stdin: None,
            files: DIFF_MORE_LANGUAGE_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type elm",
            args: &["-t", "elm", "needle", "proj"],
            stdin: None,
            files: DIFF_MORE_LANGUAGE_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type fsharp",
            args: &["-t", "fsharp", "needle", "proj"],
            stdin: None,
            files: DIFF_MORE_LANGUAGE_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type groovy",
            args: &["-t", "groovy", "needle", "proj"],
            stdin: None,
            files: DIFF_MORE_LANGUAGE_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type ocaml",
            args: &["-t", "ocaml", "needle", "proj"],
            stdin: None,
            files: DIFF_MORE_LANGUAGE_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type org",
            args: &["-t", "org", "needle", "proj"],
            stdin: None,
            files: DIFF_MORE_LANGUAGE_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type rescript",
            args: &["-t", "rescript", "needle", "proj"],
            stdin: None,
            files: DIFF_MORE_LANGUAGE_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type solidity",
            args: &["-t", "solidity", "needle", "proj"],
            stdin: None,
            files: DIFF_MORE_LANGUAGE_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type svelte",
            args: &["-t", "svelte", "needle", "proj"],
            stdin: None,
            files: DIFF_MORE_LANGUAGE_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type vala",
            args: &["-t", "vala", "needle", "proj"],
            stdin: None,
            files: DIFF_MORE_LANGUAGE_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type coq",
            args: &["-t", "coq", "needle", "proj"],
            stdin: None,
            files: DIFF_EXTRA_LANGUAGE_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type cuda",
            args: &["-t", "cuda", "needle", "proj"],
            stdin: None,
            files: DIFF_EXTRA_LANGUAGE_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type d",
            args: &["-t", "d", "needle", "proj"],
            stdin: None,
            files: DIFF_EXTRA_LANGUAGE_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type dhall",
            args: &["-t", "dhall", "needle", "proj"],
            stdin: None,
            files: DIFF_EXTRA_LANGUAGE_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type idris",
            args: &["-t", "idris", "needle", "proj"],
            stdin: None,
            files: DIFF_EXTRA_LANGUAGE_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type lean",
            args: &["-t", "lean", "needle", "proj"],
            stdin: None,
            files: DIFF_EXTRA_LANGUAGE_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type matlab",
            args: &["-t", "matlab", "needle", "proj"],
            stdin: None,
            files: DIFF_EXTRA_LANGUAGE_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type pascal",
            args: &["-t", "pascal", "needle", "proj"],
            stdin: None,
            files: DIFF_EXTRA_LANGUAGE_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type qml",
            args: &["-t", "qml", "needle", "proj"],
            stdin: None,
            files: DIFF_EXTRA_LANGUAGE_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type sass",
            args: &["-t", "sass", "needle", "proj"],
            stdin: None,
            files: DIFF_EXTRA_LANGUAGE_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type stylus",
            args: &["-t", "stylus", "needle", "proj"],
            stdin: None,
            files: DIFF_EXTRA_LANGUAGE_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type thrift",
            args: &["-t", "thrift", "needle", "proj"],
            stdin: None,
            files: DIFF_EXTRA_LANGUAGE_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type webidl",
            args: &["-t", "webidl", "needle", "proj"],
            stdin: None,
            files: DIFF_EXTRA_LANGUAGE_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type wgsl",
            args: &["-t", "wgsl", "needle", "proj"],
            stdin: None,
            files: DIFF_EXTRA_LANGUAGE_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type ada",
            args: &["-t", "ada", "needle", "proj"],
            stdin: None,
            files: DIFF_EARLY_LANGUAGE_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type agda",
            args: &["-t", "agda", "needle", "proj"],
            stdin: None,
            files: DIFF_EARLY_LANGUAGE_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type aidl",
            args: &["-t", "aidl", "needle", "proj"],
            stdin: None,
            files: DIFF_EARLY_LANGUAGE_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type asm",
            args: &["-t", "asm", "needle", "proj"],
            stdin: None,
            files: DIFF_EARLY_LANGUAGE_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type asp",
            args: &["-t", "asp", "needle", "proj"],
            stdin: None,
            files: DIFF_EARLY_LANGUAGE_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type ats",
            args: &["-t", "ats", "needle", "proj"],
            stdin: None,
            files: DIFF_EARLY_LANGUAGE_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type avro",
            args: &["-t", "avro", "needle", "proj"],
            stdin: None,
            files: DIFF_EARLY_LANGUAGE_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type awk",
            args: &["-t", "awk", "needle", "proj"],
            stdin: None,
            files: DIFF_EARLY_LANGUAGE_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type bazel",
            args: &["-t", "bazel", "needle", "proj"],
            stdin: None,
            files: DIFF_EARLY_LANGUAGE_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type bitbake",
            args: &["-t", "bitbake", "needle", "proj"],
            stdin: None,
            files: DIFF_EARLY_LANGUAGE_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type coffeescript",
            args: &["-t", "coffeescript", "needle", "proj"],
            stdin: None,
            files: DIFF_EARLY_LANGUAGE_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type cython",
            args: &["-t", "cython", "needle", "proj"],
            stdin: None,
            files: DIFF_EARLY_LANGUAGE_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type devicetree",
            args: &["-t", "devicetree", "needle", "proj"],
            stdin: None,
            files: DIFF_EARLY_LANGUAGE_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type dockercompose",
            args: &["-t", "dockercompose", "needle", "proj"],
            stdin: None,
            files: DIFF_EARLY_LANGUAGE_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type cmd",
            args: &["-t", "cmd", "needle", "proj"],
            stdin: None,
            files: DIFF_COMMON_LANGUAGE_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type cfml",
            args: &["-t", "cfml", "needle", "proj"],
            stdin: None,
            files: DIFF_COMMON_LANGUAGE_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type cshtml",
            args: &["-t", "cshtml", "needle", "proj"],
            stdin: None,
            files: DIFF_COMMON_LANGUAGE_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type csproj",
            args: &["-t", "csproj", "needle", "proj"],
            stdin: None,
            files: DIFF_COMMON_LANGUAGE_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type dts",
            args: &["-t", "dts", "needle", "proj"],
            stdin: None,
            files: DIFF_COMMON_LANGUAGE_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type edn",
            args: &["-t", "edn", "needle", "proj"],
            stdin: None,
            files: DIFF_COMMON_LANGUAGE_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type erb",
            args: &["-t", "erb", "needle", "proj"],
            stdin: None,
            files: DIFF_COMMON_LANGUAGE_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type flatbuffers",
            args: &["-t", "flatbuffers", "needle", "proj"],
            stdin: None,
            files: DIFF_COMMON_LANGUAGE_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type fortran",
            args: &["-t", "fortran", "needle", "proj"],
            stdin: None,
            files: DIFF_COMMON_LANGUAGE_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type gdscript",
            args: &["-t", "gdscript", "needle", "proj"],
            stdin: None,
            files: DIFF_COMMON_LANGUAGE_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type gleam",
            args: &["-t", "gleam", "needle", "proj"],
            stdin: None,
            files: DIFF_COMMON_LANGUAGE_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type gn",
            args: &["-t", "gn", "needle", "proj"],
            stdin: None,
            files: DIFF_COMMON_LANGUAGE_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type gradle",
            args: &["-t", "gradle", "needle", "proj"],
            stdin: None,
            files: DIFF_COMMON_LANGUAGE_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type haml",
            args: &["-t", "haml", "needle", "proj"],
            stdin: None,
            files: DIFF_COMMON_LANGUAGE_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type alire",
            args: &["-t", "alire", "needle", "proj"],
            stdin: None,
            files: DIFF_FORMAT_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type amake",
            args: &["-t", "amake", "needle", "proj"],
            stdin: None,
            files: DIFF_FORMAT_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type boxlang",
            args: &["-t", "boxlang", "needle", "proj"],
            stdin: None,
            files: DIFF_FORMAT_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type brotli",
            args: &["-t", "brotli", "needle", "proj"],
            stdin: None,
            files: DIFF_FORMAT_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type buildstream",
            args: &["-t", "buildstream", "needle", "proj"],
            stdin: None,
            files: DIFF_FORMAT_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type bzip2",
            args: &["-t", "bzip2", "needle", "proj"],
            stdin: None,
            files: DIFF_FORMAT_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type candid",
            args: &["-t", "candid", "needle", "proj"],
            stdin: None,
            files: DIFF_FORMAT_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type carp",
            args: &["-t", "carp", "needle", "proj"],
            stdin: None,
            files: DIFF_FORMAT_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type cbor",
            args: &["-t", "cbor", "needle", "proj"],
            stdin: None,
            files: DIFF_FORMAT_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type ceylon",
            args: &["-t", "ceylon", "needle", "proj"],
            stdin: None,
            files: DIFF_FORMAT_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type cml",
            args: &["-t", "cml", "needle", "proj"],
            stdin: None,
            files: DIFF_FORMAT_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type creole",
            args: &["-t", "creole", "needle", "proj"],
            stdin: None,
            files: DIFF_FORMAT_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type dita",
            args: &["-t", "dita", "needle", "proj"],
            stdin: None,
            files: DIFF_FORMAT_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type dvc",
            args: &["-t", "dvc", "needle", "proj"],
            stdin: None,
            files: DIFF_FORMAT_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type ebuild",
            args: &["-t", "ebuild", "needle", "proj"],
            stdin: None,
            files: DIFF_ADDITIONAL_FORMAT_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type elisp",
            args: &["-t", "elisp", "needle", "proj"],
            stdin: None,
            files: DIFF_ADDITIONAL_FORMAT_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type fennel",
            args: &["-t", "fennel", "needle", "proj"],
            stdin: None,
            files: DIFF_ADDITIONAL_FORMAT_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type fidl",
            args: &["-t", "fidl", "needle", "proj"],
            stdin: None,
            files: DIFF_ADDITIONAL_FORMAT_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type fut",
            args: &["-t", "fut", "needle", "proj"],
            stdin: None,
            files: DIFF_ADDITIONAL_FORMAT_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type gap",
            args: &["-t", "gap", "needle", "proj"],
            stdin: None,
            files: DIFF_ADDITIONAL_FORMAT_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type gprbuild",
            args: &["-t", "gprbuild", "needle", "proj"],
            stdin: None,
            files: DIFF_ADDITIONAL_FORMAT_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type gzip",
            args: &["-t", "gzip", "needle", "proj"],
            stdin: None,
            files: DIFF_ADDITIONAL_FORMAT_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type h",
            args: &["-t", "h", "needle", "proj"],
            stdin: None,
            files: DIFF_ADDITIONAL_FORMAT_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type hare",
            args: &["-t", "hare", "needle", "proj"],
            stdin: None,
            files: DIFF_ADDITIONAL_FORMAT_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type hbs",
            args: &["-t", "hbs", "needle", "proj"],
            stdin: None,
            files: DIFF_ADDITIONAL_FORMAT_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type hy",
            args: &["-t", "hy", "needle", "proj"],
            stdin: None,
            files: DIFF_ADDITIONAL_FORMAT_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type janet",
            args: &["-t", "janet", "needle", "proj"],
            stdin: None,
            files: DIFF_ADDITIONAL_FORMAT_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type jinja",
            args: &["-t", "jinja", "needle", "proj"],
            stdin: None,
            files: DIFF_ADDITIONAL_FORMAT_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type jl",
            args: &["-t", "jl", "needle", "proj"],
            stdin: None,
            files: DIFF_ADDITIONAL_FORMAT_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type jupyter",
            args: &["-t", "jupyter", "needle", "proj"],
            stdin: None,
            files: DIFF_ADDITIONAL_FORMAT_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type k",
            args: &["-t", "k", "needle", "proj"],
            stdin: None,
            files: DIFF_METADATA_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type kconfig",
            args: &["-t", "kconfig", "needle", "proj"],
            stdin: None,
            files: DIFF_METADATA_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type less",
            args: &["-t", "less", "needle", "proj"],
            stdin: None,
            files: DIFF_METADATA_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type license",
            args: &["-t", "license", "needle", "proj"],
            stdin: None,
            files: DIFF_METADATA_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type lilypond",
            args: &["-t", "lilypond", "needle", "proj"],
            stdin: None,
            files: DIFF_METADATA_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type llvm",
            args: &["-t", "llvm", "needle", "proj"],
            stdin: None,
            files: DIFF_METADATA_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type lock",
            args: &["-t", "lock", "needle", "proj"],
            stdin: None,
            files: DIFF_METADATA_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type log",
            args: &["-t", "log", "needle", "proj"],
            stdin: None,
            files: DIFF_METADATA_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type lz4",
            args: &["-t", "lz4", "needle", "proj"],
            stdin: None,
            files: DIFF_METADATA_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type lzma",
            args: &["-t", "lzma", "needle", "proj"],
            stdin: None,
            files: DIFF_METADATA_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type m4",
            args: &["-t", "m4", "needle", "proj"],
            stdin: None,
            files: DIFF_METADATA_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type mako",
            args: &["-t", "mako", "needle", "proj"],
            stdin: None,
            files: DIFF_METADATA_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type man",
            args: &["-t", "man", "needle", "proj"],
            stdin: None,
            files: DIFF_METADATA_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type meson",
            args: &["-t", "meson", "needle", "proj"],
            stdin: None,
            files: DIFF_METADATA_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type minified",
            args: &["-t", "minified", "needle", "proj"],
            stdin: None,
            files: DIFF_METADATA_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type mint",
            args: &["-t", "mint", "needle", "proj"],
            stdin: None,
            files: DIFF_METADATA_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type mk",
            args: &["-t", "mk", "needle", "proj"],
            stdin: None,
            files: DIFF_PROJECT_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type ml",
            args: &["-t", "ml", "needle", "proj"],
            stdin: None,
            files: DIFF_PROJECT_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type motoko",
            args: &["-t", "motoko", "needle", "proj"],
            stdin: None,
            files: DIFF_PROJECT_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type msbuild",
            args: &["-t", "msbuild", "needle", "proj"],
            stdin: None,
            files: DIFF_PROJECT_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type objc",
            args: &["-t", "objc", "needle", "proj"],
            stdin: None,
            files: DIFF_PROJECT_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type objcpp",
            args: &["-t", "objcpp", "needle", "proj"],
            stdin: None,
            files: DIFF_PROJECT_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type pants",
            args: &["-t", "pants", "needle", "proj"],
            stdin: None,
            files: DIFF_PROJECT_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type pdf",
            args: &["-t", "pdf", "needle", "proj"],
            stdin: None,
            files: DIFF_PROJECT_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type po",
            args: &["-t", "po", "needle", "proj"],
            stdin: None,
            files: DIFF_PROJECT_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type pod",
            args: &["-t", "pod", "needle", "proj"],
            stdin: None,
            files: DIFF_PROJECT_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type postscript",
            args: &["-t", "postscript", "needle", "proj"],
            stdin: None,
            files: DIFF_PROJECT_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type prolog",
            args: &["-t", "prolog", "needle", "proj"],
            stdin: None,
            files: DIFF_PROJECT_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type ps",
            args: &["-t", "ps", "needle", "proj"],
            stdin: None,
            files: DIFF_PROJECT_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type puppet",
            args: &["-t", "puppet", "needle", "proj"],
            stdin: None,
            files: DIFF_PROJECT_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type purs",
            args: &["-t", "purs", "needle", "proj"],
            stdin: None,
            files: DIFF_PROJECT_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type qmake",
            args: &["-t", "qmake", "needle", "proj"],
            stdin: None,
            files: DIFF_PROJECT_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type qrc",
            args: &["-t", "qrc", "needle", "proj"],
            stdin: None,
            files: DIFF_DOC_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type qui",
            args: &["-t", "qui", "needle", "proj"],
            stdin: None,
            files: DIFF_DOC_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type racket",
            args: &["-t", "racket", "needle", "proj"],
            stdin: None,
            files: DIFF_DOC_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type raku",
            args: &["-t", "raku", "needle", "proj"],
            stdin: None,
            files: DIFF_DOC_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type rdoc",
            args: &["-t", "rdoc", "needle", "proj"],
            stdin: None,
            files: DIFF_DOC_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type readme",
            args: &["-t", "readme", "needle", "proj"],
            stdin: None,
            files: DIFF_DOC_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type reasonml",
            args: &["-t", "reasonml", "needle", "proj"],
            stdin: None,
            files: DIFF_DOC_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type red",
            args: &["-t", "red", "needle", "proj"],
            stdin: None,
            files: DIFF_DOC_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type robot",
            args: &["-t", "robot", "needle", "proj"],
            stdin: None,
            files: DIFF_DOC_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type rst",
            args: &["-t", "rst", "needle", "proj"],
            stdin: None,
            files: DIFF_DOC_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type scdoc",
            args: &["-t", "scdoc", "needle", "proj"],
            stdin: None,
            files: DIFF_DOC_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type seed7",
            args: &["-t", "seed7", "needle", "proj"],
            stdin: None,
            files: DIFF_DOC_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type slim",
            args: &["-t", "slim", "needle", "proj"],
            stdin: None,
            files: DIFF_DOC_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type smarty",
            args: &["-t", "smarty", "needle", "proj"],
            stdin: None,
            files: DIFF_DOC_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type sml",
            args: &["-t", "sml", "needle", "proj"],
            stdin: None,
            files: DIFF_DOC_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type soy",
            args: &["-t", "soy", "needle", "proj"],
            stdin: None,
            files: DIFF_DOC_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type spark",
            args: &["-t", "spark", "needle", "proj"],
            stdin: None,
            files: DIFF_TAIL_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type spec",
            args: &["-t", "spec", "needle", "proj"],
            stdin: None,
            files: DIFF_TAIL_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type ssa",
            args: &["-t", "ssa", "needle", "proj"],
            stdin: None,
            files: DIFF_TAIL_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type sv",
            args: &["-t", "sv", "needle", "proj"],
            stdin: None,
            files: DIFF_TAIL_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type svg",
            args: &["-t", "svg", "needle", "proj"],
            stdin: None,
            files: DIFF_TAIL_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type swig",
            args: &["-t", "swig", "needle", "proj"],
            stdin: None,
            files: DIFF_TAIL_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type systemd",
            args: &["-t", "systemd", "needle", "proj"],
            stdin: None,
            files: DIFF_TAIL_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type taskpaper",
            args: &["-t", "taskpaper", "needle", "proj"],
            stdin: None,
            files: DIFF_TAIL_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type tcl",
            args: &["-t", "tcl", "needle", "proj"],
            stdin: None,
            files: DIFF_TAIL_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type tex",
            args: &["-t", "tex", "needle", "proj"],
            stdin: None,
            files: DIFF_TAIL_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type texinfo",
            args: &["-t", "texinfo", "needle", "proj"],
            stdin: None,
            files: DIFF_TAIL_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type textile",
            args: &["-t", "textile", "needle", "proj"],
            stdin: None,
            files: DIFF_TAIL_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type twig",
            args: &["-t", "twig", "needle", "proj"],
            stdin: None,
            files: DIFF_TAIL_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type typoscript",
            args: &["-t", "typoscript", "needle", "proj"],
            stdin: None,
            files: DIFF_TAIL_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type typst",
            args: &["-t", "typst", "needle", "proj"],
            stdin: None,
            files: DIFF_TAIL_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type usd",
            args: &["-t", "usd", "needle", "proj"],
            stdin: None,
            files: DIFF_TAIL_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type v",
            args: &["-t", "v", "needle", "proj"],
            stdin: None,
            files: DIFF_TAIL_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type vb",
            args: &["-t", "vb", "needle", "proj"],
            stdin: None,
            files: DIFF_TAIL_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type vcl",
            args: &["-t", "vcl", "needle", "proj"],
            stdin: None,
            files: DIFF_TAIL_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type verilog",
            args: &["--sort", "path", "-t", "verilog", "needle", "proj"],
            stdin: None,
            files: DIFF_TAIL_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type vhdl",
            args: &["-t", "vhdl", "needle", "proj"],
            stdin: None,
            files: DIFF_TAIL_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type vimscript",
            args: &["-t", "vimscript", "needle", "proj"],
            stdin: None,
            files: DIFF_TAIL_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type wiki",
            args: &["-t", "wiki", "needle", "proj"],
            stdin: None,
            files: DIFF_TAIL_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type xz",
            args: &["-t", "xz", "needle", "proj"],
            stdin: None,
            files: DIFF_TAIL_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type yacc",
            args: &["-t", "yacc", "needle", "proj"],
            stdin: None,
            files: DIFF_TAIL_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type yang",
            args: &["-t", "yang", "needle", "proj"],
            stdin: None,
            files: DIFF_TAIL_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type z",
            args: &["-t", "z", "needle", "proj"],
            stdin: None,
            files: DIFF_TAIL_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type zsh",
            args: &["-t", "zsh", "needle", "proj"],
            stdin: None,
            files: DIFF_TAIL_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "type zstd",
            args: &["-t", "zstd", "needle", "proj"],
            stdin: None,
            files: DIFF_TAIL_TYPE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
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
            name: "type add bracket class",
            args: &[
                "--type-add",
                "letters:[ab].txt",
                "-t",
                "letters",
                "needle",
                "proj",
            ],
            stdin: None,
            files: DIFF_GLOB_CLASS_FILES,
            cwd: "/",
            output: RgDiffOutput::UnorderedLines,
        },
        RgDiffCase {
            name: "type add brace alternation",
            args: &[
                "--type-add",
                "code:*.{rs,toml}",
                "-t",
                "code",
                "needle",
                "proj",
            ],
            stdin: None,
            files: DIFF_GLOB_BRACE_FILES,
            cwd: "/",
            output: RgDiffOutput::UnorderedLines,
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
            name: "parent ignore files apply to child root",
            args: &["needle", "proj/sub"],
            stdin: None,
            files: DIFF_PARENT_IGNORE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "no ignore parent disables parent ignore files",
            args: &["--no-ignore-parent", "needle", "proj/sub"],
            stdin: None,
            files: DIFF_PARENT_IGNORE_FILES,
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
            name: "ignore reenables auto ignore files",
            args: &["--no-ignore", "--ignore", "needle", "proj"],
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
            name: "ignore vcs reenables vcs ignore",
            args: &["--no-ignore-vcs", "--ignore-vcs", "needle", "proj"],
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
            name: "ignore dot reenables dot ignore",
            args: &["--no-ignore-dot", "--ignore-dot", "needle", "proj"],
            stdin: None,
            files: DIFF_IGNORE_FILES,
            cwd: "/",
            output: RgDiffOutput::UnorderedLines,
        },
        RgDiffCase {
            name: "no ignore exclude keeps vcs ignore",
            args: &["--no-ignore-exclude", "needle", "proj"],
            stdin: None,
            files: DIFF_IGNORE_FILES,
            cwd: "/",
            output: RgDiffOutput::UnorderedLines,
        },
        RgDiffCase {
            name: "ignore exclude reenables local exclude",
            args: &["--no-ignore-exclude", "--ignore-exclude", "needle", "proj"],
            stdin: None,
            files: DIFF_IGNORE_FILES,
            cwd: "/",
            output: RgDiffOutput::UnorderedLines,
        },
        RgDiffCase {
            name: "no ignore partial dot reenable",
            args: &["--no-ignore", "--ignore-dot", "needle", "proj"],
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
            name: "no ignore files disables explicit ignore file",
            args: &[
                "--no-ignore-files",
                "--ignore-file",
                "proj/custom.ignore",
                "needle",
                "proj",
            ],
            stdin: None,
            files: DIFF_IGNORE_FILES,
            cwd: "/",
            output: RgDiffOutput::UnorderedLines,
        },
        RgDiffCase {
            name: "ignore files reenables explicit ignore file",
            args: &[
                "--no-ignore-files",
                "--ignore-files",
                "--ignore-file",
                "proj/custom.ignore",
                "needle",
                "proj",
            ],
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
            name: "explicit binary file reports match by default",
            args: &["needle", "proj/bin.dat"],
            stdin: None,
            files: DIFF_BINARY_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "explicit binary no binary still reports match",
            args: &["--no-binary", "needle", "proj/bin.dat"],
            stdin: None,
            files: DIFF_BINARY_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
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
            name: "text then binary reports binary match",
            args: &["--text", "--binary", "needle", "proj/bin.dat"],
            stdin: None,
            files: DIFF_BINARY_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "binary then text searches as text",
            args: &["--binary", "--text", "needle", "proj/bin.dat"],
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
            name: "binary count",
            args: &["--binary", "-c", "needle", "proj/bin.dat", "proj/text.txt"],
            stdin: None,
            files: DIFF_BINARY_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "binary files without match",
            args: &[
                "--binary",
                "--files-without-match",
                "missing",
                "proj/bin.dat",
                "proj/text.txt",
            ],
            stdin: None,
            files: DIFF_BINARY_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "binary stats",
            args: &[
                "--binary",
                "--stats",
                "needle",
                "proj/bin.dat",
                "proj/text.txt",
            ],
            stdin: None,
            files: DIFF_BINARY_FILES,
            cwd: "/",
            output: RgDiffOutput::StatsWithoutBytesSearched,
        },
        RgDiffCase {
            name: "binary json explicit before nul",
            args: &["--json", "abc", "proj/bin.dat"],
            stdin: None,
            files: DIFF_BINARY_FILES,
            cwd: "/",
            output: RgDiffOutput::JsonEvents,
        },
        RgDiffCase {
            name: "binary json explicit after nul",
            args: &["--json", "needle", "proj/bin.dat"],
            stdin: None,
            files: DIFF_BINARY_FILES,
            cwd: "/",
            output: RgDiffOutput::JsonEvents,
        },
        RgDiffCase {
            name: "binary json recursive skips default",
            args: &["--json", "needle", "proj"],
            stdin: None,
            files: DIFF_BINARY_FILES,
            cwd: "/",
            output: RgDiffOutput::JsonEvents,
        },
        RgDiffCase {
            name: "binary json recursive binary",
            args: &["--json", "--binary", "needle", "proj"],
            stdin: None,
            files: DIFF_BINARY_FILES,
            cwd: "/",
            output: RgDiffOutput::UnorderedJsonEvents,
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
            name: "no encoding restores bom sniffing",
            args: &[
                "--encoding=none",
                "--no-encoding",
                "needle",
                "proj/utf16bom.txt",
            ],
            stdin: None,
            files: DIFF_ENCODING_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "crlf default anchor sees carriage return",
            args: &["needle$", "proj/crlf.txt"],
            stdin: None,
            files: DIFF_CRLF_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "crlf anchor matches before carriage return",
            args: &["--crlf", "needle$", "proj/crlf.txt"],
            stdin: None,
            files: DIFF_CRLF_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "no crlf then crlf uses last flag",
            args: &["--no-crlf", "--crlf", "needle$", "proj/crlf.txt"],
            stdin: None,
            files: DIFF_CRLF_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "crlf then no crlf uses last flag",
            args: &["--crlf", "--no-crlf", "needle$", "proj/crlf.txt"],
            stdin: None,
            files: DIFF_CRLF_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "crlf output preserves carriage return",
            args: &["needle", "proj/crlf.txt"],
            stdin: None,
            files: DIFF_CRLF_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "crlf replacement preserves carriage return",
            args: &["--crlf", "-r", "X", "needle$", "proj/crlf.txt"],
            stdin: None,
            files: DIFF_CRLF_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "multiline literal newline",
            args: &["-U", "foo\nbar", "proj/multi.txt"],
            stdin: None,
            files: DIFF_MULTILINE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "multiline line numbers",
            args: &["-nU", "foo\nbar", "proj/multi.txt"],
            stdin: None,
            files: DIFF_MULTILINE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "multiline count matches",
            args: &["-c", "-U", "foo\nbar", "proj/multi.txt"],
            stdin: None,
            files: DIFF_MULTILINE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "multiline only matching",
            args: &["-o", "-U", "foo\nbar", "proj/multi.txt"],
            stdin: None,
            files: DIFF_MULTILINE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "multiline replacement",
            args: &["-U", "-r", "X", "foo\nbar", "proj/multi.txt"],
            stdin: None,
            files: DIFF_MULTILINE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "multiline invert excludes matched spans",
            args: &["-v", "-U", "foo\nbar", "proj/multi.txt"],
            stdin: None,
            files: DIFF_MULTILINE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "multiline invert count",
            args: &["-c", "-v", "-U", "foo\nbar", "proj/multi.txt"],
            stdin: None,
            files: DIFF_MULTILINE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "multiline invert files without match",
            args: &[
                "--files-without-match",
                "-v",
                "-U",
                "foo\nbar",
                "proj/all-multiline.txt",
            ],
            stdin: None,
            files: DIFF_MULTILINE_ALL_MATCH_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "multiline column and byte offset",
            args: &["-n", "--column", "-b", "-U", "foo\nbar", "proj/multi.txt"],
            stdin: None,
            files: DIFF_MULTILINE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "multiline vimgrep",
            args: &["--vimgrep", "-U", "foo\nbar", "proj/multi.txt"],
            stdin: None,
            files: DIFF_MULTILINE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "multiline json",
            args: &["--json", "-U", "foo\nbar", "proj/multi.txt"],
            stdin: None,
            files: DIFF_MULTILINE_FILES,
            cwd: "/",
            output: RgDiffOutput::JsonEvents,
        },
        RgDiffCase {
            name: "multiline json replacement submatches",
            args: &["--json", "-U", "-r", "<$0>", "foo\nbar", "proj/multi.txt"],
            stdin: None,
            files: DIFF_MULTILINE_FILES,
            cwd: "/",
            output: RgDiffOutput::JsonEvents,
        },
        RgDiffCase {
            name: "multiline json context events",
            args: &["--json", "-U", "-C1", "foo\nbar", "proj/multi.txt"],
            stdin: None,
            files: DIFF_MULTILINE_FILES,
            cwd: "/",
            output: RgDiffOutput::JsonEvents,
        },
        RgDiffCase {
            name: "multiline json passthru context events",
            args: &["--json", "-U", "--passthru", "foo\nbar", "proj/multi.txt"],
            stdin: None,
            files: DIFF_MULTILINE_FILES,
            cwd: "/",
            output: RgDiffOutput::JsonEvents,
        },
        RgDiffCase {
            name: "multiline json invert counts zero submatches",
            args: &["--json", "-U", "-v", "foo\nbar", "proj/multi.txt"],
            stdin: None,
            files: DIFF_MULTILINE_FILES,
            cwd: "/",
            output: RgDiffOutput::JsonEvents,
        },
        RgDiffCase {
            name: "multiline invert stats counts zero matches",
            args: &["-U", "-v", "--stats", "foo\nbar", "proj/multi.txt"],
            stdin: None,
            files: DIFF_MULTILINE_FILES,
            cwd: "/",
            output: RgDiffOutput::Stats,
        },
        RgDiffCase {
            name: "multiline stdin",
            args: &["-n", "-U", "foo\nbar"],
            stdin: Some("foo\nbar\nbaz\n"),
            files: &[],
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "multiline dotall",
            args: &["-U", "--multiline-dotall", "foo.bar", "proj/multi.txt"],
            stdin: None,
            files: DIFF_MULTILINE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "multiline dotall disabled",
            args: &[
                "-U",
                "--multiline-dotall",
                "--no-multiline-dotall",
                "foo.bar",
                "proj/multi.txt",
            ],
            stdin: None,
            files: DIFF_MULTILINE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "no multiline restores line mode",
            args: &["--multiline", "--no-multiline", "foo", "proj/multi.txt"],
            stdin: None,
            files: DIFF_MULTILINE_FILES,
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
            name: "explicit no-ignore does not advance unrestricted level",
            args: &["--no-ignore", "-u", "needle", "proj"],
            stdin: None,
            files: DIFF_UNRESTRICTED_FILES,
            cwd: "/",
            output: RgDiffOutput::UnorderedLines,
        },
        RgDiffCase {
            name: "explicit hidden does not advance unrestricted level",
            args: &["--hidden", "-uu", "needle", "proj"],
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
            name: "unrestricted and ignore-dot only restores dot ignores",
            args: &["-u", "--ignore-dot", "needle", "proj"],
            stdin: None,
            files: DIFF_IGNORE_FILES,
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
            name: "pcre2 lookahead",
            args: &["-P", "foo(?=bar)", "proj/pcre.txt"],
            stdin: None,
            files: DIFF_PCRE2_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "pcre2 lookbehind only matching",
            args: &["--pcre2", "-o", "(?<=foo)bar", "proj/pcre.txt"],
            stdin: None,
            files: DIFF_PCRE2_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "pcre2 backreference",
            args: &["--engine=pcre2", r"(\w+) \1", "proj/pcre.txt"],
            stdin: None,
            files: DIFF_PCRE2_FILES,
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
            name: "threads is accepted",
            args: &["--threads", "2", "needle", "proj/a.txt"],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "threads short attached is accepted",
            args: &["-j2", "needle", "proj/a.txt"],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "colors is accepted when color disabled",
            args: &[
                "--color=never",
                "--colors",
                "match:none",
                "needle",
                "proj/a.txt",
            ],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "regex and dfa size limits accepted",
            args: &[
                "--regex-size-limit",
                "10M",
                "--dfa-size-limit=10M",
                "needle",
                "proj/a.txt",
            ],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "unicode toggles accepted",
            args: &[
                "--no-unicode",
                "--unicode",
                "--no-pcre2-unicode",
                "--pcre2-unicode",
                "needle",
                "proj/a.txt",
            ],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "unicode word class default",
            args: &[r"\w+", "-o", "-n", "proj/unicode.txt"],
            stdin: None,
            files: DIFF_UNICODE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "no unicode word class is ascii",
            args: &["--no-unicode", r"\w+", "-o", "-n", "proj/unicode.txt"],
            stdin: None,
            files: DIFF_UNICODE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "no unicode word boundary is ascii",
            args: &["--no-unicode", "-w", "caf", "-n", "proj/unicode.txt"],
            stdin: None,
            files: DIFF_UNICODE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "pretty with color disabled",
            args: &[
                "--pretty",
                "--color=never",
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
            name: "max filesize filters recursive search",
            args: &["--max-filesize", "10", "needle", "proj"],
            stdin: None,
            files: DIFF_MAX_FILESIZE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "max filesize suffix allows larger files",
            args: &["--max-filesize", "1K", "needle", "proj"],
            stdin: None,
            files: DIFF_MAX_FILESIZE_FILES,
            cwd: "/",
            output: RgDiffOutput::UnorderedLines,
        },
        RgDiffCase {
            name: "max filesize does not filter explicit file",
            args: &[
                "--max-filesize",
                "10",
                "needle",
                "proj/small.txt",
                "proj/big.txt",
            ],
            stdin: None,
            files: DIFF_MAX_FILESIZE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "max filesize filters files listing",
            args: &["--files", "--max-filesize", "10", "proj"],
            stdin: None,
            files: DIFF_MAX_FILESIZE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "max filesize empty files listing exits one",
            args: &["--files", "--max-filesize", "1", "proj"],
            stdin: None,
            files: DIFF_MAX_FILESIZE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "passthru only matching prints match and context",
            args: &["--passthru", "-n", "-o", r"foo[0-9]", "proj/output.txt"],
            stdin: None,
            files: DIFF_OUTPUT_MODE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "passthru only matching replacement",
            args: &[
                "--passthru",
                "-n",
                "-o",
                "-r",
                "<$0>",
                r"foo[0-9]",
                "proj/output.txt",
            ],
            stdin: None,
            files: DIFF_OUTPUT_MODE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "passthru only matching column",
            args: &[
                "--passthru",
                "-n",
                "--column",
                "-o",
                r"foo[0-9]",
                "proj/output.txt",
            ],
            stdin: None,
            files: DIFF_OUTPUT_MODE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "vimgrep only matching replacement",
            args: &[
                "--vimgrep",
                "-o",
                "-r",
                "<$0>",
                r"foo[0-9]",
                "proj/output.txt",
            ],
            stdin: None,
            files: DIFF_OUTPUT_MODE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "multiline only matching prefixes each match line",
            args: &["--multiline", "-n", "-o", "foo\nbar", "proj/multi.txt"],
            stdin: None,
            files: DIFF_MULTILINE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "multiline only matching replacement prefixes each output line",
            args: &[
                "--multiline",
                "-n",
                "-o",
                "-r",
                "<$0>",
                "foo\nbar",
                "proj/multi.txt",
            ],
            stdin: None,
            files: DIFF_MULTILINE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "multiline passthru only matching keeps context lines",
            args: &[
                "--multiline",
                "--passthru",
                "-n",
                "-o",
                "foo\nbar",
                "proj/multi.txt",
            ],
            stdin: None,
            files: DIFF_MULTILINE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "multiline passthru only matching replacement keeps context lines",
            args: &[
                "--multiline",
                "--passthru",
                "-n",
                "-o",
                "-r",
                "<$0>",
                "foo\nbar",
                "proj/multi.txt",
            ],
            stdin: None,
            files: DIFF_MULTILINE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "multiline vimgrep only matching replacement",
            args: &[
                "--multiline",
                "--vimgrep",
                "-o",
                "-r",
                "<$0>",
                "foo\nbar",
                "proj/multi.txt",
            ],
            stdin: None,
            files: DIFF_MULTILINE_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "search zip explicit gzip",
            args: &["-z", "needle", "proj/compressed.txt.gz"],
            stdin: None,
            files: DIFF_GZIP_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "search zip recursive gzip",
            args: &["--search-zip", "needle", "proj"],
            stdin: None,
            files: DIFF_GZIP_FILES,
            cwd: "/",
            output: RgDiffOutput::UnorderedLines,
        },
        RgDiffCase {
            name: "no search zip disables gzip search",
            args: &["-z", "--no-search-zip", "needle", "proj/compressed.txt.gz"],
            stdin: None,
            files: DIFF_GZIP_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "search zip ignores gzip magic without gzip extension",
            args: &["-z", "needle", "proj/compressed.txt"],
            stdin: None,
            files: DIFF_GZIP_EXTENSION_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "search zip errors on invalid gzip extension",
            args: &["-z", "needle", "proj/fake.gz"],
            stdin: None,
            files: DIFF_GZIP_EXTENSION_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "print0 is not a ripgrep flag",
            args: &["--print0", "-l", "needle", "proj/a.txt"],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "short R is not a ripgrep flag",
            args: &["-R", "needle", "proj/a.txt"],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "search zip overrides earlier pre",
            args: &[
                "--pre=definitely-not-a-command",
                "-z",
                "needle",
                "proj/compressed.txt.gz",
            ],
            stdin: None,
            files: DIFF_GZIP_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "later pre overrides search zip",
            args: &[
                "-z",
                "--pre=definitely-not-a-command",
                "needle",
                "proj/compressed.txt.gz",
            ],
            stdin: None,
            files: DIFF_GZIP_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "pre cat explicit file",
            args: &["--pre", "cat", "needle", "proj/a.txt"],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "pre empty explicit file",
            args: &["--pre=", "needle", "proj/a.txt"],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "pre glob with cat recursive",
            args: &["--pre=cat", "--pre-glob", "*.txt", "needle", "proj"],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::UnorderedLines,
        },
        RgDiffCase {
            name: "pre glob skips unsupported preprocessor",
            args: &[
                "--pre=definitely-not-a-command",
                "--pre-glob",
                "*.md",
                "needle",
                "proj/a.txt",
            ],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "no pre disables pre command",
            args: &["--pre=false", "--no-pre", "needle", "proj/a.txt"],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "debug keeps stdout",
            args: &["--debug", "needle", "proj/a.txt"],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "trace keeps stdout",
            args: &["--trace", "needle", "proj/a.txt"],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "no ignore messages accepted",
            args: &["--no-ignore-messages", "needle", "proj/a.txt"],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "ignore messages accepted",
            args: &["--ignore-messages", "needle", "proj/a.txt"],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "hyperlink format none accepted",
            args: &["--hyperlink-format=none", "needle", "proj/a.txt"],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "hyperlink custom format",
            args: &[
                "--color=always",
                "--hyperlink-format=file://{path}:{line}:{column}",
                "-H",
                "-n",
                "--column",
                "needle",
                "proj/a.txt",
            ],
            stdin: None,
            files: DIFF_BASIC_FILES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgDiffCase {
            name: "hostname bin accepted",
            args: &["--hostname-bin", "hostname", "needle", "proj/a.txt"],
            stdin: None,
            files: DIFF_BASIC_FILES,
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

    const RG_TIMED_DIFF_CASES: &[RgTimedDiffCase] = &[
        RgTimedDiffCase {
            name: "sort modified",
            args: &["--sort", "modified", "needle", "proj"],
            files: DIFF_TIMED_SORT_FILES,
            mtimes: DIFF_TIMED_SORT_MTIMES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgTimedDiffCase {
            name: "reverse sort modified",
            args: &["--sortr", "modified", "needle", "proj"],
            files: DIFF_TIMED_SORT_FILES,
            mtimes: DIFF_TIMED_SORT_MTIMES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgTimedDiffCase {
            name: "sort modified across explicit roots",
            args: &["--sort", "modified", "needle", "d1", "d2"],
            files: DIFF_TIMED_MULTI_ROOT_SORT_FILES,
            mtimes: DIFF_TIMED_MULTI_ROOT_SORT_MTIMES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgTimedDiffCase {
            name: "sort modified across explicit files",
            args: &["--sort", "modified", "needle", "d1/new.txt", "d2/old.txt"],
            files: DIFF_TIMED_MULTI_ROOT_SORT_FILES,
            mtimes: DIFF_TIMED_MULTI_ROOT_SORT_MTIMES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgTimedDiffCase {
            name: "files sort modified across explicit roots",
            args: &["--files", "--sort", "modified", "d1", "d2"],
            files: DIFF_TIMED_MULTI_ROOT_SORT_FILES,
            mtimes: DIFF_TIMED_MULTI_ROOT_SORT_MTIMES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
        RgTimedDiffCase {
            name: "files sort modified across explicit files",
            args: &["--files", "--sort", "modified", "d1/new.txt", "d2/old.txt"],
            files: DIFF_TIMED_MULTI_ROOT_SORT_FILES,
            mtimes: DIFF_TIMED_MULTI_ROOT_SORT_MTIMES,
            cwd: "/",
            output: RgDiffOutput::Exact,
        },
    ];

    const RG_ENV_DIFF_CASES: &[RgEnvDiffCase] = &[
        RgEnvDiffCase {
            name: "default global git ignore file applies in git repo",
            args: &["needle", "proj"],
            stdin: None,
            files: DIFF_DEFAULT_GLOBAL_IGNORE_FILES,
            cwd: "/",
            env: DIFF_GLOBAL_IGNORE_ENV,
            output: RgDiffOutput::UnorderedLines,
        },
        RgEnvDiffCase {
            name: "configured global git ignore file overrides default",
            args: &["needle", "proj"],
            stdin: None,
            files: DIFF_GLOBAL_IGNORE_FILES,
            cwd: "/",
            env: DIFF_GLOBAL_IGNORE_ENV,
            output: RgDiffOutput::UnorderedLines,
        },
        RgEnvDiffCase {
            name: "no ignore global disables global git ignore files",
            args: &["--no-ignore-global", "needle", "proj"],
            stdin: None,
            files: DIFF_GLOBAL_IGNORE_FILES,
            cwd: "/",
            env: DIFF_GLOBAL_IGNORE_ENV,
            output: RgDiffOutput::UnorderedLines,
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

    /// CI pins ripgrep to this version (see `RG_VERSION` in
    /// `.github/workflows/ci.yml` and `scripts/install-ripgrep-ci.sh`). The
    /// differential tests compare byte-for-byte against real ripgrep, whose
    /// output, accepted `--colors` specs, and built-in file types vary across
    /// releases, so they only run against the pinned version.
    const PINNED_RG_VERSION: &str = "15.1.0";

    fn real_rg_matches_pinned_version() -> bool {
        let Ok(output) = std::process::Command::new("rg").arg("--version").output() else {
            return false;
        };
        if !output.status.success() {
            return false;
        }
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .next()
            .is_some_and(|line| line.contains(&format!("ripgrep {PINNED_RG_VERSION}")))
    }

    /// Returns `true` (and prints a skip notice) when the local `rg` is not the
    /// pinned version, so differential tests can early-return instead of
    /// emitting confusing byte-mismatch failures against an unexpected release.
    fn skip_if_rg_version_mismatch(test: &str) -> bool {
        if real_rg_matches_pinned_version() {
            return false;
        }
        eprintln!(
            "skipping {test}: differential tests require pinned ripgrep \
             {PINNED_RG_VERSION} (install via scripts/install-ripgrep-ci.sh)"
        );
        true
    }

    fn normalize_real_rg_temp_paths(output: &[u8], tempdir: &tempfile::TempDir) -> String {
        let mut stdout = String::from_utf8_lossy(output).into_owned();
        if let Ok(canonical) = std::fs::canonicalize(tempdir.path()) {
            stdout = stdout.replace(&canonical.to_string_lossy().to_string(), "");
        }
        stdout = stdout.replace(&tempdir.path().to_string_lossy().to_string(), "");
        stdout
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

        let stdout = normalize_real_rg_temp_paths(&output.stdout, &tempdir);
        (stdout, output.status.code().unwrap_or(-1))
    }

    fn run_real_rg_timed(case: &RgTimedDiffCase) -> (String, i32) {
        use std::process::Command;

        require_real_rg();

        let tempdir = tempfile::tempdir().expect("tempdir for timed rg differential test");
        for (path, content) in case.files {
            let host_path = tempdir.path().join(path.trim_start_matches('/'));
            if let Some(parent) = host_path.parent() {
                std::fs::create_dir_all(parent).expect("create parent dir for timed rg fixture");
            }
            std::fs::write(host_path, content).expect("write timed rg fixture file");
        }
        for (path, secs) in case.mtimes {
            let host_path = tempdir.path().join(path.trim_start_matches('/'));
            let time = std::time::UNIX_EPOCH + std::time::Duration::from_secs(*secs);
            std::fs::File::open(host_path)
                .expect("open timed rg fixture file")
                .set_modified(time)
                .expect("set timed rg fixture mtime");
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

        let output = Command::new("rg")
            .args(["--threads", "1"])
            .args(&mapped_args)
            .current_dir(host_cwd)
            .env("LC_ALL", "C")
            .output()
            .expect("run real timed rg differential test");

        let stdout = normalize_real_rg_temp_paths(&output.stdout, &tempdir);
        (stdout, output.status.code().unwrap_or(-1))
    }

    fn run_real_rg_env(case: &RgEnvDiffCase) -> (String, i32) {
        use std::io::Write;
        use std::process::{Command, Stdio};

        require_real_rg();

        let tempdir = tempfile::tempdir().expect("tempdir for rg env differential test");
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
        for (key, value) in case.env {
            let mapped = if value.starts_with('/') {
                tempdir
                    .path()
                    .join(value.trim_start_matches('/'))
                    .to_string_lossy()
                    .into_owned()
            } else {
                (*value).to_string()
            };
            command.env(key, mapped);
        }

        let output = if let Some(stdin) = case.stdin {
            let mut child = command
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .spawn()
                .expect("spawn real rg for env differential test");
            child
                .stdin
                .as_mut()
                .expect("real rg stdin pipe")
                .write_all(stdin.as_bytes())
                .expect("write stdin to real rg");
            child
                .wait_with_output()
                .expect("wait for real rg env differential test")
        } else {
            command.output().expect("run real rg env differential test")
        };

        let stdout = normalize_real_rg_temp_paths(&output.stdout, &tempdir);
        (stdout, output.status.code().unwrap_or(-1))
    }

    #[cfg(unix)]
    fn run_real_rg_symlink(case: &RgSymlinkDiffCase) -> (String, i32) {
        use std::os::unix::fs::symlink;
        use std::process::{Command, Stdio};

        require_real_rg();

        let tempdir = tempfile::tempdir().expect("tempdir for rg symlink differential test");
        for (path, content) in case.files {
            let host_path = tempdir.path().join(path.trim_start_matches('/'));
            if let Some(parent) = host_path.parent() {
                std::fs::create_dir_all(parent).expect("create parent dir for rg fixture");
            }
            std::fs::write(host_path, content).expect("write rg fixture file");
        }
        for (link, target) in case.symlinks {
            let host_link = tempdir.path().join(link.trim_start_matches('/'));
            if let Some(parent) = host_link.parent() {
                std::fs::create_dir_all(parent).expect("create parent dir for rg symlink");
            }
            symlink(target, host_link).expect("create rg fixture symlink");
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

        let output = command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .output()
            .expect("run real rg symlink differential test");

        let stdout = normalize_real_rg_temp_paths(&output.stdout, &tempdir);
        (stdout, output.status.code().unwrap_or(-1))
    }

    async fn assert_rg_diff_case(case: &RgDiffCase) {
        let (real_stdout, real_code) = run_real_rg(case);
        let bashkit = run_rg_with_cwd(case.args, case.stdin, case.files, case.cwd).await;
        assert_rg_outputs_match(case.name, case.output, &bashkit, &real_stdout, real_code);
    }

    async fn assert_rg_timed_diff_case(case: &RgTimedDiffCase) {
        let (real_stdout, real_code) = run_real_rg_timed(case);
        let bashkit =
            run_rg_with_cwd_and_mtimes(case.args, case.files, case.mtimes, case.cwd).await;
        assert_rg_outputs_match(case.name, case.output, &bashkit, &real_stdout, real_code);
    }

    async fn assert_rg_env_diff_case(case: &RgEnvDiffCase) {
        let (real_stdout, real_code) = run_real_rg_env(case);
        let bashkit = run_rg_fixture_with_cwd_and_env(
            case.args,
            case.stdin,
            case.files,
            &[],
            case.cwd,
            case.env,
        )
        .await;
        assert_rg_outputs_match(case.name, case.output, &bashkit, &real_stdout, real_code);
    }

    fn assert_rg_outputs_match(
        name: &str,
        output: RgDiffOutput,
        bashkit: &ExecResult,
        real_stdout: &str,
        real_code: i32,
    ) {
        match output {
            RgDiffOutput::Exact => assert_eq!(
                bashkit.stdout, real_stdout,
                "stdout mismatch for rg differential case {}",
                name
            ),
            RgDiffOutput::UnorderedLines => assert_eq!(
                sorted_lines(&bashkit.stdout),
                sorted_lines(real_stdout),
                "stdout line-set mismatch for rg differential case {}",
                name
            ),
            RgDiffOutput::UnorderedNul => assert_eq!(
                sorted_nul_items(&bashkit.stdout),
                sorted_nul_items(real_stdout),
                "stdout NUL-item mismatch for rg differential case {}",
                name
            ),
            RgDiffOutput::JsonEvents => assert_eq!(
                normalize_rg_json(&bashkit.stdout),
                normalize_rg_json(real_stdout),
                "stdout JSON-event mismatch for rg differential case {}",
                name
            ),
            RgDiffOutput::UnorderedJsonEvents => assert_eq!(
                unordered_normalized_rg_json(&bashkit.stdout),
                unordered_normalized_rg_json(real_stdout),
                "stdout JSON-event set mismatch for rg differential case {}",
                name
            ),
            RgDiffOutput::Stats => assert_eq!(
                normalize_rg_stats(&bashkit.stdout),
                normalize_rg_stats(real_stdout),
                "stdout stats mismatch for rg differential case {}",
                name
            ),
            RgDiffOutput::StatsWithoutBytesSearched => assert_eq!(
                normalize_rg_stats_without_bytes_searched(&bashkit.stdout),
                normalize_rg_stats_without_bytes_searched(real_stdout),
                "stdout stats mismatch for rg differential case {}",
                name
            ),
            RgDiffOutput::ContainsAll(needles) => {
                assert!(
                    !bashkit.stdout.is_empty(),
                    "bashkit stdout unexpectedly empty for rg differential case {}",
                    name
                );
                assert!(
                    !real_stdout.is_empty(),
                    "real rg stdout unexpectedly empty for rg differential case {}",
                    name
                );
                for needle in needles {
                    assert!(
                        bashkit.stdout.contains(needle),
                        "bashkit stdout for rg differential case {} did not contain {needle}",
                        name
                    );
                    assert!(
                        real_stdout.contains(needle),
                        "real rg stdout for rg differential case {} did not contain {needle}",
                        name
                    );
                }
            }
        }
        assert_eq!(
            bashkit.exit_code, real_code,
            "exit-code mismatch for rg differential case {}",
            name
        );
    }

    #[cfg(unix)]
    async fn assert_rg_symlink_diff_case(case: &RgSymlinkDiffCase) {
        let (real_stdout, real_code) = run_real_rg_symlink(case);
        let bashkit =
            run_rg_fixture_with_cwd(case.args, None, case.files, case.symlinks, case.cwd).await;
        match case.output {
            RgDiffOutput::Exact => assert_eq!(
                bashkit.stdout, real_stdout,
                "stdout mismatch for rg symlink differential case {}",
                case.name
            ),
            RgDiffOutput::UnorderedLines => assert_eq!(
                sorted_lines(&bashkit.stdout),
                sorted_lines(&real_stdout),
                "stdout line-set mismatch for rg symlink differential case {}",
                case.name
            ),
            RgDiffOutput::UnorderedNul => assert_eq!(
                sorted_nul_items(&bashkit.stdout),
                sorted_nul_items(&real_stdout),
                "stdout NUL-item mismatch for rg symlink differential case {}",
                case.name
            ),
            RgDiffOutput::JsonEvents => assert_eq!(
                normalize_rg_json(&bashkit.stdout),
                normalize_rg_json(&real_stdout),
                "stdout JSON-event mismatch for rg symlink differential case {}",
                case.name
            ),
            RgDiffOutput::UnorderedJsonEvents => assert_eq!(
                unordered_normalized_rg_json(&bashkit.stdout),
                unordered_normalized_rg_json(&real_stdout),
                "stdout JSON-event set mismatch for rg symlink differential case {}",
                case.name
            ),
            RgDiffOutput::Stats => assert_eq!(
                normalize_rg_stats(&bashkit.stdout),
                normalize_rg_stats(&real_stdout),
                "stdout stats mismatch for rg symlink differential case {}",
                case.name
            ),
            RgDiffOutput::StatsWithoutBytesSearched => assert_eq!(
                normalize_rg_stats_without_bytes_searched(&bashkit.stdout),
                normalize_rg_stats_without_bytes_searched(&real_stdout),
                "stdout stats mismatch for rg symlink differential case {}",
                case.name
            ),
            RgDiffOutput::ContainsAll(needles) => {
                assert!(
                    !bashkit.stdout.is_empty(),
                    "bashkit stdout unexpectedly empty for rg symlink differential case {}",
                    case.name
                );
                assert!(
                    !real_stdout.is_empty(),
                    "real rg stdout unexpectedly empty for rg symlink differential case {}",
                    case.name
                );
                for needle in needles {
                    assert!(
                        bashkit.stdout.contains(needle),
                        "bashkit stdout for rg symlink differential case {} did not contain {needle}",
                        case.name
                    );
                    assert!(
                        real_stdout.contains(needle),
                        "real rg stdout for rg symlink differential case {} did not contain {needle}",
                        case.name
                    );
                }
            }
        }
        assert_eq!(
            bashkit.exit_code, real_code,
            "exit-code mismatch for rg symlink differential case {}",
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

    fn unordered_normalized_rg_json(output: &str) -> Vec<String> {
        let mut events: Vec<String> = normalize_rg_json(output)
            .into_iter()
            .map(|event| serde_json::to_string(&event).expect("serialize normalized rg JSON event"))
            .collect();
        events.sort_unstable();
        events
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

    fn normalize_rg_stats_without_bytes_searched(output: &str) -> Vec<&str> {
        normalize_rg_stats(output)
            .into_iter()
            .filter(|line| !line.ends_with(" bytes searched"))
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
    async fn quiet_files_without_match_uses_match_exit_status() {
        let files = [
            ("/proj/hit.txt", b"needle\n".as_slice()),
            ("/proj/miss.txt", b"hay\n".as_slice()),
        ];

        let hit = run_rg(
            &["-q", "--files-without-match", "needle", "/proj/hit.txt"],
            None,
            &files,
        )
        .await;
        // hit.txt has needle, so --files-without-match finds no qualifying file → exit 1
        assert_eq!(hit.exit_code, 1);
        assert_eq!(hit.stdout, "");

        let miss = run_rg(
            &["-q", "--files-without-match", "needle", "/proj/miss.txt"],
            None,
            &files,
        )
        .await;
        // miss.txt has no needle, so --files-without-match finds it → exit 0
        assert_eq!(miss.exit_code, 0);
        assert_eq!(miss.stdout, "");
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
        assert!(long_help.stdout.contains("--generate KIND"));
        assert!(long_help.stdout.contains("--unrestricted"));
        assert!(long_help.stdout.contains("--no-require-git"));
        assert!(long_help.stdout.contains("--no-config"));
        assert!(long_help.stdout.contains("--path-separator"));
        assert!(long_help.stdout.contains("--engine"));
        assert!(long_help.stdout.contains("--mmap"));
        assert!(long_help.stdout.contains("--pcre2"));
        assert!(long_help.stdout.contains("--pcre2-version"));
        assert!(long_help.stdout.contains("--no-unicode"));
        assert!(long_help.stdout.contains("-j, --threads"));
        assert!(long_help.stdout.contains("--colors"));
        assert!(long_help.stdout.contains("-p, --pretty"));
        assert!(long_help.stdout.contains("--encoding"));
        assert!(long_help.stdout.contains("--crlf"));
        assert!(long_help.stdout.contains("--multiline"));
        assert!(long_help.stdout.contains("--multiline-dotall"));
        assert!(long_help.stdout.contains("-z, --search-zip"));
        assert!(long_help.stdout.contains("--pre COMMAND"));
        assert!(long_help.stdout.contains("--pre-glob"));
        assert!(long_help.stdout.contains("--no-pre"));
        assert!(long_help.stdout.contains("--ignore-messages"));
        assert!(long_help.stdout.contains("--no-ignore-messages"));
        assert!(long_help.stdout.contains("--debug"));
        assert!(long_help.stdout.contains("--trace"));
        assert!(long_help.stdout.contains("--hostname-bin"));
        assert!(long_help.stdout.contains("--hyperlink-format"));
        assert!(long_help.stdout.contains("--no-invert-match"));
        assert!(long_help.stdout.contains("--no-encoding"));
        assert!(long_help.stdout.contains("--no-ignore-files"));
        assert!(long_help.stdout.contains("--ignore-file-case-insensitive"));
        assert!(long_help.stdout.contains("--ignore-dot"));
        assert!(long_help.stdout.contains("--no-ignore-exclude"));
        assert!(long_help.stdout.contains("--ignore-vcs"));
        assert!(long_help.stdout.contains("--passthrough"));
        assert!(long_help.stdout.contains("-0, --null"));
        assert!(long_help.stdout.contains("--null-data"));
        assert!(long_help.stdout.contains("-., --hidden"));
        assert!(long_help.stdout.contains("--one-file-system"));
        assert!(long_help.stdout.contains("--stop-on-nonmatch"));
        assert!(long_help.stdout.contains("--no-context-separator"));
        assert!(long_help.stdout.contains("-d, --max-depth"));
        assert!(long_help.stdout.contains("--maxdepth"));

        let short_help = run_rg(&["-h"], None, &[]).await;
        assert_eq!(short_help.exit_code, 0);
        assert_eq!(short_help.stdout, long_help.stdout);

        let version = run_rg(&["--version"], None, &[]).await;
        assert_eq!(version.exit_code, 0);
        assert_eq!(version.stdout, "rg (bashkit) 0.1\n");

        let short_version = run_rg(&["-V"], None, &[]).await;
        assert_eq!(short_version.exit_code, 0);
        assert_eq!(short_version.stdout, version.stdout);

        let pcre2_version = run_rg(&["--pcre2-version"], None, &[]).await;
        assert_eq!(pcre2_version.exit_code, 0);
        assert_eq!(
            pcre2_version.stdout,
            "PCRE2 10.45 is available (JIT is available)\n"
        );
    }

    #[tokio::test]
    async fn test_rg_no_unicode_ascii_regex_mode() {
        let files = [("/proj/unicode.txt", "cafe\ncafé\nκαφες\n".as_bytes())];

        let default = run_rg(&[r"\w+", "-o", "-n", "/proj/unicode.txt"], None, &files).await;
        assert_eq!(default.exit_code, 0);
        assert_eq!(default.stdout, "1:cafe\n2:café\n3:καφες\n");

        let no_unicode = run_rg(
            &["--no-unicode", r"\w+", "-o", "-n", "/proj/unicode.txt"],
            None,
            &files,
        )
        .await;
        assert_eq!(no_unicode.exit_code, 0);
        assert_eq!(no_unicode.stdout, "1:cafe\n2:caf\n");

        let ascii_word_boundary = run_rg(
            &["--no-unicode", "-w", "caf", "-n", "/proj/unicode.txt"],
            None,
            &files,
        )
        .await;
        assert_eq!(ascii_word_boundary.exit_code, 0);
        assert_eq!(ascii_word_boundary.stdout, "2:café\n");
    }

    #[tokio::test]
    async fn test_rg_generate_outputs() {
        let bash = run_rg(&["--generate=complete-bash"], None, &[]).await;
        assert_eq!(bash.exit_code, 0);
        assert!(bash.stdout.contains("_rg()"));
        assert!(bash.stdout.contains("--generate"));
        for flag in [
            "--no-byte-offset",
            "--no-column",
            "--crlf",
            "--no-crlf",
            "--debug",
            "--dfa-size-limit",
            "--no-fixed-strings",
            "--glob-case-insensitive",
            "--no-glob-case-insensitive",
            "--ignore-file-case-insensitive",
            "--no-ignore-files",
            "--null-data",
            "--no-pre",
            "--no-search-zip",
            "--stop-on-nonmatch",
            "--no-text",
        ] {
            assert!(bash.stdout.contains(flag), "missing {flag}");
        }

        let zsh = run_rg(&["--generate", "complete-zsh"], None, &[]).await;
        assert_eq!(zsh.exit_code, 0);
        assert!(zsh.stdout.contains("#compdef rg"));
        assert!(zsh.stdout.contains("--regexp"));
        assert!(zsh.stdout.contains("--no-search-zip"));

        let fish = run_rg(&["--generate=complete-fish"], None, &[]).await;
        assert_eq!(fish.exit_code, 0);
        assert!(fish.stdout.contains("complete -c rg"));
        assert!(fish.stdout.contains("-l generate"));

        let powershell = run_rg(&["--generate=complete-powershell"], None, &[]).await;
        assert_eq!(powershell.exit_code, 0);
        assert!(powershell.stdout.contains("Register-ArgumentCompleter"));
        assert!(powershell.stdout.contains("--generate"));

        let man = run_rg(&["--generate=man"], None, &[]).await;
        assert_eq!(man.exit_code, 0);
        assert!(man.stdout.contains(".TH RG"));
        assert!(man.stdout.contains(".SH NAME"));
    }

    #[tokio::test]
    async fn test_rg_generate_not_detected_inside_value_args() {
        let files = [("/tmp/input.txt", b"--generate=complete-bash\n".as_slice())];

        let with_short_regexp = run_rg(
            &["-e", "--generate=complete-bash", "/tmp/input.txt"],
            None,
            &files,
        )
        .await;
        assert_eq!(with_short_regexp.exit_code, 0);
        assert_eq!(with_short_regexp.stdout, "--generate=complete-bash\n");

        let with_long_regexp = run_rg(
            &["--regexp", "--generate=complete-bash", "/tmp/input.txt"],
            None,
            &files,
        )
        .await;
        assert_eq!(with_long_regexp.exit_code, 0);
        assert_eq!(with_long_regexp.stdout, "--generate=complete-bash\n");
    }

    #[tokio::test]
    async fn test_rg_invalid_sort_values_exit_two() {
        let invalid = run_rg(&["--sort", "junk", "needle", "/file.txt"], None, &[]).await;
        assert_eq!(invalid.exit_code, 2);
        assert_eq!(invalid.stdout, "");
        assert_eq!(
            invalid.stderr,
            "rg: error parsing flag --sort: choice 'junk' is unrecognized\n"
        );

        let invalid_reverse = run_rg(&["--sortr", "junk", "needle", "/file.txt"], None, &[]).await;
        assert_eq!(invalid_reverse.exit_code, 2);
        assert_eq!(invalid_reverse.stdout, "");
        assert_eq!(
            invalid_reverse.stderr,
            "rg: error parsing flag --sortr: choice 'junk' is unrecognized\n"
        );
    }

    #[test]
    fn test_rg_generate_errors() {
        let args = vec!["--generate".to_string()];
        let missing = rg_generate_kind(&args);
        assert!(matches!(
            missing,
            Err(Error::Execution(msg)) if msg == "rg: --generate requires an argument"
        ));

        let invalid = rg_generate_output("complete-elvish", "");
        assert!(matches!(
            invalid,
            Err(Error::Execution(msg)) if msg == "rg: invalid --generate value: complete-elvish"
        ));
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

        let add_after_all = run_rg(
            &[
                "--type",
                "all",
                "--type-add",
                "foo:*.foo",
                "needle",
                "/proj",
            ],
            None,
            &[
                ("/proj/main.rs", b"needle\n"),
                ("/proj/main.foo", b"needle\n"),
            ],
        )
        .await;
        assert_eq!(add_after_all.exit_code, 0);
        assert!(add_after_all.stdout.contains("main.rs"));
        assert!(add_after_all.stdout.contains("main.foo"));
    }

    #[tokio::test]
    async fn test_rg_ignore_files_and_disable_flags() {
        let files: &[(&str, &[u8])] = &[
            ("/proj/.git/config", b"[core]\n"),
            ("/proj/.gitignore", b"target/\n*.log\n!keep.log\n"),
            ("/proj/.ignore", b"src/ignored.txt\n"),
            ("/proj/.rgignore", b"rgonly.txt\n"),
            ("/proj/a.txt", b"needle\n"),
            ("/proj/a.log", b"needle\n"),
            ("/proj/keep.log", b"needle\n"),
            ("/proj/target/out.txt", b"needle\n"),
            ("/proj/src/ignored.txt", b"needle\n"),
            ("/proj/rgonly.txt", b"needle\n"),
        ];

        let default = run_rg(&["needle", "/proj"], None, files).await;
        assert_eq!(default.exit_code, 0);
        assert!(default.stdout.contains("a.txt"));
        assert!(default.stdout.contains("keep.log"));
        assert!(!default.stdout.contains("a.log"));
        assert!(!default.stdout.contains("target/out.txt"));
        assert!(!default.stdout.contains("src/ignored.txt"));
        assert!(!default.stdout.contains("rgonly.txt"));

        let no_ignore = run_rg(&["--no-ignore", "needle", "/proj"], None, files).await;
        assert_eq!(no_ignore.exit_code, 0);
        assert!(no_ignore.stdout.contains("a.log"));
        assert!(no_ignore.stdout.contains("target/out.txt"));
        assert!(no_ignore.stdout.contains("src/ignored.txt"));
        assert!(no_ignore.stdout.contains("rgonly.txt"));

        let no_vcs = run_rg(&["--no-ignore-vcs", "needle", "/proj"], None, files).await;
        assert_eq!(no_vcs.exit_code, 0);
        assert!(no_vcs.stdout.contains("a.log"));
        assert!(!no_vcs.stdout.contains("src/ignored.txt"));
        assert!(!no_vcs.stdout.contains("rgonly.txt"));
    }

    #[tokio::test]
    async fn test_rg_rgignore_overrides_gitignore_conflicts() {
        let rgignore_ignores_files: &[(&str, &[u8])] = &[
            ("/proj/.git/config", b"[core]\n"),
            ("/proj/.gitignore", b"!secret.txt\n"),
            ("/proj/.rgignore", b"secret.txt\n"),
            ("/proj/public.txt", b"needle\n"),
            ("/proj/secret.txt", b"needle\n"),
        ];
        let ignored = run_rg(&["--files", "/proj"], None, rgignore_ignores_files).await;
        assert_eq!(ignored.exit_code, 0);
        assert!(ignored.stdout.contains("public.txt"));
        assert!(!ignored.stdout.contains("secret.txt"));

        let rgignore_unignores_files: &[(&str, &[u8])] = &[
            ("/proj/.git/config", b"[core]\n"),
            ("/proj/.gitignore", b"secret.txt\n"),
            ("/proj/.rgignore", b"!secret.txt\n"),
            ("/proj/secret.txt", b"needle\n"),
        ];
        let unignored = run_rg(&["--files", "/proj"], None, rgignore_unignores_files).await;
        assert_eq!(unignored.exit_code, 0);
        assert!(unignored.stdout.contains("secret.txt"));
    }

    #[tokio::test]
    async fn test_rg_parent_ignore_files() {
        let files: &[(&str, &[u8])] = &[
            ("/proj/.git/config", b"[core]\n"),
            ("/proj/.ignore", b"sub/ignored.txt\n"),
            ("/proj/.gitignore", b"sub/vcs.txt\n"),
            ("/proj/sub/ignored.txt", b"needle\n"),
            ("/proj/sub/keep.txt", b"needle\n"),
            ("/proj/sub/vcs.txt", b"needle\n"),
        ];

        let default = run_rg(&["needle", "/proj/sub"], None, files).await;
        assert_eq!(default.exit_code, 0);
        assert!(default.stdout.contains("keep.txt"));
        assert!(!default.stdout.contains("ignored.txt"));
        assert!(!default.stdout.contains("vcs.txt"));

        let no_parent = run_rg(&["--no-ignore-parent", "needle", "/proj/sub"], None, files).await;
        assert_eq!(no_parent.exit_code, 0);
        assert!(no_parent.stdout.contains("ignored.txt"));
        assert!(no_parent.stdout.contains("keep.txt"));
        assert!(no_parent.stdout.contains("vcs.txt"));

        let no_vcs = run_rg(&["--no-ignore-vcs", "needle", "/proj/sub"], None, files).await;
        assert_eq!(no_vcs.exit_code, 0);
        assert!(!no_vcs.stdout.contains("ignored.txt"));
        assert!(no_vcs.stdout.contains("keep.txt"));
        assert!(no_vcs.stdout.contains("vcs.txt"));
    }

    #[tokio::test]
    async fn test_rg_global_ignore_files() {
        let default_global = run_rg_fixture_with_cwd_and_env(
            &["needle", "/proj"],
            None,
            DIFF_DEFAULT_GLOBAL_IGNORE_FILES,
            &[],
            "/",
            DIFF_GLOBAL_IGNORE_ENV,
        )
        .await;
        assert_eq!(default_global.exit_code, 0);
        assert!(default_global.stdout.contains("keep.txt"));
        assert!(!default_global.stdout.contains("global.txt"));

        let default = run_rg_fixture_with_cwd_and_env(
            &["needle", "/proj"],
            None,
            DIFF_GLOBAL_IGNORE_FILES,
            &[],
            "/",
            DIFF_GLOBAL_IGNORE_ENV,
        )
        .await;
        assert_eq!(default.exit_code, 0);
        assert!(default.stdout.contains("keep.txt"));
        assert!(default.stdout.contains("global.txt"));
        assert!(!default.stdout.contains("custom.txt"));

        let no_global = run_rg_fixture_with_cwd_and_env(
            &["--no-ignore-global", "needle", "/proj"],
            None,
            DIFF_GLOBAL_IGNORE_FILES,
            &[],
            "/",
            DIFF_GLOBAL_IGNORE_ENV,
        )
        .await;
        assert_eq!(no_global.exit_code, 0);
        assert!(no_global.stdout.contains("global.txt"));
        assert!(no_global.stdout.contains("custom.txt"));
        assert!(no_global.stdout.contains("keep.txt"));
    }

    #[test]
    fn test_rg_git_config_excludes_file_parsing() {
        let paths = parse_git_config_excludes_files(
            "[core]\n\texcludesFile = ~/custom.ignore\n[other]\n\texcludesFile = /skip\n",
            Path::new("/home"),
        );
        assert_eq!(paths, vec![PathBuf::from("/home/custom.ignore")]);
    }

    #[tokio::test]
    async fn test_rg_ignore_file_control_flags() {
        let files: &[(&str, &[u8])] = &[
            ("/proj/custom.ignore", b"*.tmp\n"),
            ("/proj/case.ignore", b"*.log\n"),
            ("/proj/a.tmp", b"needle\n"),
            ("/proj/Foo.LOG", b"needle\n"),
            ("/proj/keep.txt", b"needle\n"),
        ];

        let disabled = run_rg(
            &[
                "--no-ignore-files",
                "--ignore-file",
                "/proj/custom.ignore",
                "needle",
                "/proj",
            ],
            None,
            files,
        )
        .await;
        assert_eq!(disabled.exit_code, 0);
        assert!(disabled.stdout.contains("a.tmp"));

        let reenabled = run_rg(
            &[
                "--no-ignore-files",
                "--ignore-files",
                "--ignore-file",
                "/proj/custom.ignore",
                "needle",
                "/proj",
            ],
            None,
            files,
        )
        .await;
        assert_eq!(reenabled.exit_code, 0);
        assert!(!reenabled.stdout.contains("a.tmp"));

        let case_sensitive = run_rg(
            &["--ignore-file", "/proj/case.ignore", "needle", "/proj"],
            None,
            files,
        )
        .await;
        assert_eq!(case_sensitive.exit_code, 0);
        assert!(case_sensitive.stdout.contains("Foo.LOG"));

        let case_insensitive = run_rg(
            &[
                "--ignore-file-case-insensitive",
                "--ignore-file",
                "/proj/case.ignore",
                "needle",
                "/proj",
            ],
            None,
            files,
        )
        .await;
        assert_eq!(case_insensitive.exit_code, 0);
        assert!(!case_insensitive.stdout.contains("Foo.LOG"));
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

        let explicit_default = run_rg(&["needle", "/proj/bin.dat"], None, files).await;
        assert_eq!(explicit_default.exit_code, 0);
        assert_eq!(
            explicit_default.stdout,
            "binary file matches (found \"\\0\" byte around offset 3)\n"
        );

        let stdin_default = run_rg(&["needle"], Some("abc\0needle\n"), files).await;
        assert_eq!(stdin_default.exit_code, 0);
        assert_eq!(
            stdin_default.stdout,
            "binary file matches (found \"\\0\" byte around offset 3)\n"
        );

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
        let result = run_rg(&["--definitely-not-rg"], None, &[]).await;
        assert_eq!(result.exit_code, 2);
        assert_eq!(result.stdout, "");
        assert_eq!(
            result.stderr,
            "rg: unrecognized option '--definitely-not-rg'\n"
        );
    }

    #[tokio::test]
    async fn test_rg_unsupported_preprocessor_errors() {
        let result = run_rg(
            &["--pre", "sed s/x/y/", "needle", "/file.txt"],
            None,
            &[("/file.txt", b"needle\n")],
        )
        .await;
        assert_eq!(result.exit_code, 2);
        assert!(result.stdout.is_empty());
        assert!(
            result
                .stderr
                .contains("preprocessor command could not start")
        );
    }

    #[tokio::test]
    async fn test_rg_pre_glob_skips_unsupported_preprocessor() {
        let excluded = run_rg(
            &[
                "--pre",
                "sed s/x/y/",
                "--pre-glob",
                "*.md",
                "needle",
                "/file.txt",
            ],
            None,
            &[("/file.txt", b"needle\n")],
        )
        .await;
        assert_eq!(excluded.exit_code, 0);
        assert_eq!(excluded.stdout, "needle\n");
        assert!(excluded.stderr.is_empty());

        let included = run_rg(
            &[
                "--pre",
                "sed s/x/y/",
                "--pre-glob",
                "*.txt",
                "needle",
                "/file.txt",
            ],
            None,
            &[("/file.txt", b"needle\n")],
        )
        .await;
        assert_eq!(included.exit_code, 2);
        assert!(included.stdout.is_empty());
        assert!(
            included
                .stderr
                .contains("preprocessor command could not start")
        );
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
    async fn test_rg_multiline_invert_max_count() {
        let result = run_rg(
            &["-m", "1", "-v", "-U", "foo\nbar", "/test.txt"],
            None,
            &[("/test.txt", b"foo\nbar\nkeep1\nfoo\nbar\nkeep2\n")],
        )
        .await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "keep1\n");
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
        let result = run_rg(&[], None, &[]).await;
        assert_eq!(result.exit_code, 2);
        assert_eq!(result.stdout, "");
        assert_eq!(result.stderr, "rg: missing pattern\n");
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
    async fn test_rg_no_line_number_clears_explicit_state_short() {
        let result = run_rg(
            &["-n", "-N", "--column", "--no-column", "world", "/test.txt"],
            None,
            &[("/test.txt", b"hello\nworld\n")],
        )
        .await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout.trim(), "world");
    }

    #[tokio::test]
    async fn test_rg_no_line_number_clears_explicit_state_long() {
        let result = run_rg(
            &[
                "--line-number",
                "--no-line-number",
                "--column",
                "--no-column",
                "world",
                "/test.txt",
            ],
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

    #[test]
    fn test_rg_context_indices_merge_before_expanding() {
        let lines = rg_context_line_indices(10, &[2, 3, 4], 2, usize::MAX, true);
        assert_eq!(lines, (0..10).collect::<Vec<_>>());

        let context_only = rg_context_line_indices(6, &[1, 2, 3], 1, 1, false);
        assert_eq!(context_only, vec![0, 4]);
    }

    #[tokio::test]
    async fn test_rg_context_extreme_value_does_not_overflow() {
        let max_context = format!("-nC{}", usize::MAX);
        let result = run_rg(
            &[max_context.as_str(), "needle", "/test.txt"],
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
    async fn test_rg_replace_caps_amplified_output() {
        // A line with many matches × a giant replacement string would otherwise
        // allocate proportional to matches × replacement.len() — well past the
        // 1 MiB cap. The cap surfaces as a visible marker in the output instead
        // of silently truncating.
        let payload = "a".repeat(10_000);
        let replacement = "x".repeat(2048);
        let files: &[(&str, &[u8])] = &[("/big.txt", payload.as_bytes())];

        let all = run_rg(
            &["--replace", replacement.as_str(), "a", "/big.txt"],
            None,
            files,
        )
        .await;
        assert_eq!(all.exit_code, 0);
        assert!(
            all.stdout.contains("replacement output capped"),
            "expected cap marker, got: {:?}", // debug-ok: assert-failure message
            &all.stdout[..all.stdout.len().min(200)]
        );
        // The cap must actually prevent allocation past the threshold (plus
        // some framing overhead like filename + colon + the marker itself).
        assert!(
            all.stdout.len() < 2 * RG_MAX_REPLACEMENT_OUTPUT_BYTES,
            "replace_all output bypassed the cap: {} bytes",
            all.stdout.len()
        );
    }

    #[tokio::test]
    async fn test_rg_only_matching_replace_caps_amplified_output() {
        let payload = "a".repeat(10_000);
        let replacement = "x".repeat(2048);
        let files: &[(&str, &[u8])] = &[("/big.txt", payload.as_bytes())];

        for args in [
            vec![
                "--only-matching",
                "--replace",
                replacement.as_str(),
                "a",
                "/big.txt",
            ],
            vec![
                "--vimgrep",
                "--only-matching",
                "--replace",
                replacement.as_str(),
                "a",
                "/big.txt",
            ],
        ] {
            let result = run_rg(&args, None, files).await;

            assert_eq!(result.exit_code, 0);
            assert!(
                result.stdout.contains("replacement output capped"),
                "expected cap marker, got: {:?}", // debug-ok: assert-failure message
                &result.stdout[..result.stdout.len().min(200)]
            );
            assert!(
                result.stdout.len() < 2 * RG_MAX_REPLACEMENT_OUTPUT_BYTES,
                "only-matching replacement output bypassed the cap: {} bytes",
                result.stdout.len()
            );
        }
    }

    #[tokio::test]
    async fn test_rg_replace_caps_capture_amplified_output() {
        let payload = "a".repeat(500_000);
        let replacement = "$1$1$1$1";
        let files: &[(&str, &[u8])] = &[("/big.txt", payload.as_bytes())];

        let result = run_rg(
            &["--replace", replacement, "(a{1000})", "/big.txt"],
            None,
            files,
        )
        .await;

        assert_eq!(result.exit_code, 0);
        assert!(
            result.stdout.contains("replacement output capped"),
            "expected cap marker, got: {:?}", // debug-ok: assert-failure message
            &result.stdout[..result.stdout.len().min(200)]
        );
        assert!(
            result.stdout.len() < 2 * RG_MAX_REPLACEMENT_OUTPUT_BYTES,
            "capture replacement output bypassed the cap: {} bytes",
            result.stdout.len()
        );
    }

    #[test]
    fn test_rg_only_matching_rejects_empty_pattern() {
        let args = vec![
            "-o".to_string(),
            "-e".to_string(),
            "".to_string(),
            "/test.txt".to_string(),
        ];
        match RgOptions::parse(&args) {
            Err(Error::Execution(msg)) => {
                assert_eq!(msg, "rg: empty pattern is not allowed with --only-matching")
            }
            Err(other) => panic!("unexpected error: {other}"),
            Ok(_) => panic!("expected parse error"),
        }
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
    async fn test_rg_multiline_quiet_and_files_with_matches() {
        let quiet = run_rg(
            &["-q", "-U", "foo\nbar", "/test.txt"],
            None,
            &[("/test.txt", b"foo\nbar\nfoo\nbar\n")],
        )
        .await;
        assert_eq!(quiet.exit_code, 0);
        assert_eq!(quiet.stdout, "");

        let files = run_rg(
            &["-l", "-U", "foo\nbar", "/test.txt"],
            None,
            &[("/test.txt", b"foo\nbar\nfoo\nbar\n")],
        )
        .await;
        assert_eq!(files.exit_code, 0);
        assert_eq!(files.stdout, "/test.txt\n");
    }

    #[tokio::test]
    async fn test_rg_color_always() {
        let files: &[(&str, &[u8])] = &[("/file.txt", b"needle again\n")];

        let result = run_rg(&["--color=always", "needle", "/file.txt"], None, files).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "\x1b[0m\x1b[1m\x1b[31mneedle\x1b[0m again\n");

        let prefixed = run_rg(
            &["--color=always", "-n", "--column", "needle", "/file.txt"],
            None,
            files,
        )
        .await;
        assert_eq!(prefixed.exit_code, 0);
        assert_eq!(
            prefixed.stdout,
            "\x1b[0m\x1b[32m1\x1b[0m:\x1b[0m1\x1b[0m:\x1b[0m\x1b[1m\x1b[31mneedle\x1b[0m again\n"
        );

        let custom = run_rg(
            &[
                "--color=always",
                "--colors",
                "match:fg:blue",
                "--colors",
                "match:style:nobold",
                "needle",
                "/file.txt",
            ],
            None,
            files,
        )
        .await;
        assert_eq!(custom.exit_code, 0);
        assert_eq!(custom.stdout, "\x1b[0m\x1b[34mneedle\x1b[0m again\n");

        let custom_style = run_rg(
            &[
                "--color=always",
                "--colors",
                "match:style:underline",
                "--colors",
                "match:style:italic",
                "--colors",
                "match:fg:200,100,50",
                "needle",
                "/file.txt",
            ],
            None,
            files,
        )
        .await;
        assert_eq!(custom_style.exit_code, 0);
        assert_eq!(
            custom_style.stdout,
            "\x1b[0m\x1b[1m\x1b[3m\x1b[4m\x1b[38;2;200;100;50mneedle\x1b[0m again\n"
        );

        let hyperlink = run_rg(
            &[
                "--color=always",
                "--hyperlink-format=file://{path}:{line}:{column}",
                "-H",
                "-n",
                "--column",
                "needle",
                "/file.txt",
            ],
            None,
            files,
        )
        .await;
        assert_eq!(hyperlink.exit_code, 0);
        assert_eq!(
            hyperlink.stdout,
            "\x1b]8;;file:///file.txt:1:1\x1b\\\x1b[0m\x1b[35m/file.txt\x1b[0m:\x1b[0m\x1b[32m1\x1b[0m:\x1b[0m1\x1b[0m\x1b]8;;\x1b\\:\x1b[0m\x1b[1m\x1b[31mneedle\x1b[0m again\n"
        );
    }

    #[test]
    fn format_hyperlink_url_does_not_rewrite_placeholder_text_in_path() {
        let url = format_hyperlink_url("file://{path}:{line}:{column}", "/proj/a{line}.txt", 12, 5);
        assert_eq!(url, "file:///proj/a%7Bline%7D.txt:12:5");
    }

    #[test]
    fn format_hyperlink_url_percent_encodes_url_delimiters_in_path() {
        let url = format_hyperlink_url("file://{path}", "/proj/sp ace#q?.txt", 1, 1);
        assert_eq!(url, "file:///proj/sp%20ace%23q%3F.txt");
    }

    #[tokio::test]
    async fn test_rg_color_always_falls_back_for_dense_matches() {
        let dense = "a".repeat((RG_COLOR_MATCH_EXTRA_BYTES_LIMIT / 8) + 16);
        let files = vec![("/file.txt", dense.as_bytes())];
        let result = run_rg(&["--color=always", "a", "/file.txt"], None, &files).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, format!("{dense}\n"));
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

    #[tokio::test]
    async fn test_rg_indexed_explicit_binary_file_reports_match_by_default() {
        let inner = InMemoryFs::new();
        inner.mkdir(Path::new("/proj"), true).await.unwrap();
        inner
            .write_file(Path::new("/proj/bin.dat"), b"abc\0needle\n")
            .await
            .unwrap();

        let fs = Arc::new(IndexedTestFs {
            inner,
            matches: vec![SearchMatch {
                path: PathBuf::from("/proj/bin.dat"),
                line_number: 1,
                line_content: "abc\0needle".to_string(),
            }],
        });

        let result = run_rg_with_fs(&["needle", "/proj/bin.dat"], None, fs).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(
            result.stdout,
            "binary file matches (found \"\\0\" byte around offset 3)\n"
        );
    }

    #[tokio::test]
    async fn test_rg_crlf_skips_indexed_prefilter_and_falls_back_to_linear_scan() {
        let inner = InMemoryFs::new();
        inner.mkdir(Path::new("/safe"), true).await.unwrap();
        inner
            .write_file(Path::new("/safe/crlf.txt"), b"needle\r\nother\r\n")
            .await
            .unwrap();

        let fs = Arc::new(IndexedTestFs {
            inner,
            matches: Vec::new(),
        });

        let result = run_rg_with_fs(&["--crlf", "needle$", "/safe/crlf.txt"], None, fs).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "needle\r\n");
    }

    #[tokio::test]
    async fn test_rg_indexed_search_skipped_for_no_unicode_non_literal_queries() {
        let inner = InMemoryFs::new();
        inner.mkdir(Path::new("/safe"), true).await.unwrap();
        inner
            .write_file(Path::new("/safe/unicode.txt"), b"cafe\ncafe\ncaf\xc3\xa9\n")
            .await
            .unwrap();

        let fs = Arc::new(IndexedTestFs {
            inner,
            matches: vec![],
        });

        let result = run_rg_with_fs(
            &["--no-ignore", "--no-unicode", "-F", "-w", "caf", "/safe"],
            None,
            fs,
        )
        .await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "/safe/unicode.txt:café\n");
    }

    #[test]
    fn real_rg_binary_is_available_for_differential_tests() {
        if skip_if_rg_version_mismatch("real_rg_binary_is_available_for_differential_tests") {
            return;
        }
        require_real_rg();
    }

    #[tokio::test]
    async fn diff_rg_matches_real_rg_cases() {
        if skip_if_rg_version_mismatch("diff_rg_matches_real_rg_cases") {
            return;
        }
        for case in RG_DIFF_CASES {
            assert_rg_diff_case(case).await;
        }
        for case in RG_TIMED_DIFF_CASES {
            assert_rg_timed_diff_case(case).await;
        }
        for case in RG_ENV_DIFF_CASES {
            assert_rg_env_diff_case(case).await;
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn diff_rg_matches_real_rg_symlink_cases() {
        if skip_if_rg_version_mismatch("diff_rg_matches_real_rg_symlink_cases") {
            return;
        }
        for case in RG_SYMLINK_DIFF_CASES {
            assert_rg_symlink_diff_case(case).await;
        }
    }
}
