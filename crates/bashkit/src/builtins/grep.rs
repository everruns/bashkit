//! grep - Pattern matching builtin
//!
//! Target: GNU grep 3.11 in a UTF-8 locale (Debian's grep), checked against
//! recorded GNU goldens.
//!
//! Decisions:
//! - Patterns: `-G` (default) and `-E` are GNU BRE/ERE translated by
//!   `grep_pattern` and matched leftmost-longest (`-o` prints the longest
//!   alternative); back-references fall back to fancy-regex. `-P` is
//!   fancy-regex as given. `-F` is escaped literals on the POSIX path.
//!   Each `-e`/`-f` entry is split on newlines; `-f` of an empty file is
//!   zero patterns and matches nothing.
//! - Output follows GNU's prtext/prline: prefix order FILE, LINE, BYTE, then
//!   a tab for `-T`; selected lines use `:`, context lines `-`. A group
//!   separator (`--`, `--group-separator`, none with `--no-group-separator`)
//!   is printed between non-adjacent groups, also across files, whenever any
//!   context option was given (even `-A0`). `-o` prints each non-empty match
//!   of an output line (a context line only under `-v`), `-b` with `-o` is
//!   the match's offset. After `-m NUM` the trailing context is printed in
//!   full, selectable lines included (as context lines).
//! - Lines end at `\n` only (`\r` is data, `x$` does not match `x\r`). A file
//!   with a NUL byte is binary: line output is suppressed, the first match
//!   stops the file and `grep: FILE: binary file matches` goes to stderr
//!   (grep >= 3.5). A line that is not valid UTF-8 is printed only under
//!   `-a`; reaching one stops the file with the same message.
//! - Options are parsed getopt_long style: options and operands may mix,
//!   long options accept unique prefixes (`--no-gr`), `-NUM` sets context.
//! - Directory operands: `-d read` (default) reports "Is a directory",
//!   `-d skip` ignores them, `-d recurse`/`-r` recurse (`-R` also follows
//!   symlinks met during traversal). Traversal is in name order; GNU uses
//!   readdir order, which bashkit cannot reproduce (L-GREP-003).
//! - A pipeline stage that prints matching lines streams them (see
//!   L-PIPE-001); every path shares the same per-line engine (`FileScan`).

use std::collections::VecDeque;

use async_trait::async_trait;

use super::grep_pattern::{CompileOptions, PatternMatcher, Syntax};
use super::{Builtin, Context};
use crate::error::Result;
use crate::fs::vfs_join;
use crate::interpreter::ExecResult;

/// grep command - pattern matching
pub struct Grep;

const USAGE_TAIL: &str =
    "Usage: grep [OPTION]... PATTERNS [FILE]...\nTry 'grep --help' for more information.\n";

#[derive(Clone, Copy, PartialEq, Eq)]
enum BinaryFiles {
    Binary,
    Text,
    WithoutMatch,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Directories {
    Read,
    Skip,
    Recurse,
}

struct GrepOptions {
    /// `-e` patterns (each may hold several, newline-separated).
    patterns: Vec<String>,
    /// `-f` pattern files, read before matching.
    pattern_files: Vec<String>,
    files: Vec<String>,
    syntax: Syntax,
    ignore_case: bool,
    invert_match: bool,
    line_numbers: bool,
    count_only: bool,
    files_with_matches: bool,
    files_without_match: bool,
    only_matching: bool,
    word_regex: bool,
    whole_line: bool,
    quiet: bool,
    max_count: Option<usize>,
    after_context: Option<usize>,
    before_context: Option<usize>,
    default_context: Option<usize>,
    /// `-H` (Some(true)) / `-h` (Some(false)); last one wins.
    with_filename: Option<bool>,
    byte_offset: bool,
    null_data: bool,
    binary_files: BinaryFiles,
    directories: Directories,
    /// `-R`: follow every symlink while recursing.
    dereference: bool,
    include_patterns: Vec<String>,
    exclude_patterns: Vec<String>,
    exclude_dir_patterns: Vec<String>,
    exclude_from: Vec<String>,
    suppress_errors: bool,
    null_filename: bool,
    initial_tab: bool,
    label: Option<String>,
    /// `None` after `--no-group-separator`.
    group_separator: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ArgKind {
    No,
    Required,
    Optional,
}

/// GNU grep 3.11 long options (getopt_long table).
const LONG_OPTIONS: &[(&str, ArgKind)] = &[
    ("after-context", ArgKind::Required),
    ("basic-regexp", ArgKind::No),
    ("before-context", ArgKind::Required),
    ("binary", ArgKind::No),
    ("binary-files", ArgKind::Required),
    ("byte-offset", ArgKind::No),
    ("color", ArgKind::Optional),
    ("colour", ArgKind::Optional),
    ("context", ArgKind::Required),
    ("count", ArgKind::No),
    ("dereference-recursive", ArgKind::No),
    ("devices", ArgKind::Required),
    ("directories", ArgKind::Required),
    ("exclude", ArgKind::Required),
    ("exclude-dir", ArgKind::Required),
    ("exclude-from", ArgKind::Required),
    ("extended-regexp", ArgKind::No),
    ("file", ArgKind::Required),
    ("files-with-matches", ArgKind::No),
    ("files-without-match", ArgKind::No),
    ("fixed-strings", ArgKind::No),
    ("group-separator", ArgKind::Required),
    ("help", ArgKind::No),
    ("ignore-case", ArgKind::No),
    ("include", ArgKind::Required),
    ("initial-tab", ArgKind::No),
    ("invert-match", ArgKind::No),
    ("label", ArgKind::Required),
    ("line-buffered", ArgKind::No),
    ("line-number", ArgKind::No),
    ("line-regexp", ArgKind::No),
    ("max-count", ArgKind::Required),
    ("no-filename", ArgKind::No),
    ("no-group-separator", ArgKind::No),
    ("no-ignore-case", ArgKind::No),
    ("no-messages", ArgKind::No),
    ("null", ArgKind::No),
    ("null-data", ArgKind::No),
    ("only-matching", ArgKind::No),
    ("perl-regexp", ArgKind::No),
    ("quiet", ArgKind::No),
    ("recursive", ArgKind::No),
    ("regexp", ArgKind::Required),
    ("silent", ArgKind::No),
    ("text", ArgKind::No),
    ("unix-byte-offsets", ArgKind::No),
    ("version", ArgKind::No),
    ("with-filename", ArgKind::No),
    ("word-regexp", ArgKind::No),
];

/// Resolve a long option name like getopt_long: exact match, else a unique
/// prefix. `Err` is the full diagnostic.
fn resolve_long(name: &str) -> std::result::Result<(&'static str, ArgKind), String> {
    if let Some(&(n, k)) = LONG_OPTIONS.iter().find(|(n, _)| *n == name) {
        return Ok((n, k));
    }
    let hits: Vec<&(&str, ArgKind)> = LONG_OPTIONS
        .iter()
        .filter(|(n, _)| n.starts_with(name))
        .collect();
    match hits.as_slice() {
        [] => Err(format!("grep: unrecognized option '--{name}'\n")),
        [one] => Ok((one.0, one.1)),
        many => {
            let list: Vec<String> = many.iter().map(|(n, _)| format!("'--{n}'")).collect();
            Err(format!(
                "grep: option '--{name}' is ambiguous; possibilities: {}\n",
                list.join(" ")
            ))
        }
    }
}

fn usage_error(msg: String) -> String {
    format!("{msg}{USAGE_TAIL}")
}

fn parse_context(value: &str) -> std::result::Result<usize, String> {
    value
        .parse::<usize>()
        .map_err(|_| format!("grep: {value}: invalid context length argument\n"))
}

impl GrepOptions {
    fn new() -> Self {
        GrepOptions {
            patterns: Vec::new(),
            pattern_files: Vec::new(),
            files: Vec::new(),
            syntax: Syntax::Basic,
            ignore_case: false,
            invert_match: false,
            line_numbers: false,
            count_only: false,
            files_with_matches: false,
            files_without_match: false,
            only_matching: false,
            word_regex: false,
            whole_line: false,
            quiet: false,
            max_count: None,
            after_context: None,
            before_context: None,
            default_context: None,
            with_filename: None,
            byte_offset: false,
            null_data: false,
            binary_files: BinaryFiles::Binary,
            directories: Directories::Read,
            dereference: false,
            include_patterns: Vec::new(),
            exclude_patterns: Vec::new(),
            exclude_dir_patterns: Vec::new(),
            exclude_from: Vec::new(),
            suppress_errors: false,
            null_filename: false,
            initial_tab: false,
            label: None,
            group_separator: Some("--".to_string()),
        }
    }

    fn parse(args: &[String]) -> std::result::Result<Self, String> {
        let mut opts = GrepOptions::new();
        let mut positional = Vec::new();
        let mut i = 0;
        while i < args.len() {
            let arg = &args[i];
            if arg == "--" {
                positional.extend(args[i + 1..].iter().cloned());
                break;
            }
            if let Some(opt) = arg.strip_prefix("--") {
                let (name, inline) = match opt.split_once('=') {
                    Some((n, v)) => (n, Some(v.to_string())),
                    None => (opt, None),
                };
                let (name, kind) = resolve_long(name).map_err(usage_error)?;
                let value = match kind {
                    ArgKind::No => {
                        if inline.is_some() {
                            return Err(usage_error(format!(
                                "grep: option '--{name}' doesn't allow an argument\n"
                            )));
                        }
                        None
                    }
                    ArgKind::Optional => inline,
                    ArgKind::Required => match inline {
                        Some(v) => Some(v),
                        None => {
                            i += 1;
                            match args.get(i) {
                                Some(v) => Some(v.clone()),
                                None => {
                                    return Err(usage_error(format!(
                                        "grep: option '--{name}' requires an argument\n"
                                    )));
                                }
                            }
                        }
                    },
                };
                opts.apply_long(name, value.unwrap_or_default())?;
            } else if arg.len() > 1 && arg.starts_with('-') {
                let chars: Vec<char> = arg[1..].chars().collect();
                let mut j = 0;
                let mut digits: Option<usize> = None;
                while j < chars.len() {
                    let c = chars[j];
                    if let Some(d) = c.to_digit(10) {
                        // -NUM: consecutive digits in one argument form one number.
                        let v = digits
                            .unwrap_or(0)
                            .saturating_mul(10)
                            .saturating_add(d as usize);
                        digits = Some(v);
                        opts.default_context = Some(v);
                        j += 1;
                        continue;
                    }
                    digits = None;
                    if matches!(c, 'A' | 'B' | 'C' | 'D' | 'd' | 'e' | 'f' | 'm' | 'X') {
                        let rest: String = chars[j + 1..].iter().collect();
                        let value = if !rest.is_empty() {
                            rest
                        } else {
                            i += 1;
                            match args.get(i) {
                                Some(v) => v.clone(),
                                None => {
                                    return Err(usage_error(format!(
                                        "grep: option requires an argument -- '{c}'\n"
                                    )));
                                }
                            }
                        };
                        opts.apply_short_value(c, value)?;
                        break;
                    }
                    opts.apply_short_flag(c)?;
                    j += 1;
                }
            } else {
                positional.push(arg.clone());
            }
            i += 1;
        }

        if opts.patterns.is_empty() && opts.pattern_files.is_empty() {
            if positional.is_empty() {
                return Err(USAGE_TAIL.to_string());
            }
            opts.patterns.push(positional.remove(0));
        }
        opts.files = positional;
        Ok(opts)
    }

    fn apply_short_flag(&mut self, c: char) -> std::result::Result<(), String> {
        match c {
            'i' | 'y' => self.ignore_case = true,
            'v' => self.invert_match = true,
            'n' => self.line_numbers = true,
            'c' => self.count_only = true,
            'l' => self.files_with_matches = true,
            'L' => self.files_without_match = true,
            'o' => self.only_matching = true,
            'w' => self.word_regex = true,
            'x' => self.whole_line = true,
            // Pattern type: last of -G/-E/-F/-P wins.
            'F' => self.syntax = Syntax::Fixed,
            'E' => self.syntax = Syntax::Extended,
            'G' => self.syntax = Syntax::Basic,
            'P' => self.syntax = Syntax::Perl,
            'q' => self.quiet = true,
            'H' => self.with_filename = Some(true),
            'h' => self.with_filename = Some(false),
            'b' => self.byte_offset = true,
            'a' => self.binary_files = BinaryFiles::Text,
            'I' => self.binary_files = BinaryFiles::WithoutMatch,
            'z' => self.null_data = true,
            's' => self.suppress_errors = true,
            'Z' => self.null_filename = true,
            'T' => self.initial_tab = true,
            'r' => self.directories = Directories::Recurse,
            'R' => {
                self.directories = Directories::Recurse;
                self.dereference = true;
            }
            // -U (binary), -u (unix byte offsets): no-ops on POSIX.
            'U' | 'u' => {}
            _ => {
                return Err(usage_error(format!("grep: invalid option -- '{c}'\n")));
            }
        }
        Ok(())
    }

    fn apply_short_value(&mut self, c: char, value: String) -> std::result::Result<(), String> {
        match c {
            'A' => self.after_context = Some(parse_context(&value)?),
            'B' => self.before_context = Some(parse_context(&value)?),
            'C' => self.default_context = Some(parse_context(&value)?),
            'e' => self.patterns.push(value),
            'f' => self.pattern_files.push(value),
            'm' => self.max_count = Some(parse_max_count(&value)?),
            'd' => self.set_directories(&value)?,
            'D' => check_devices(&value)?,
            // -X MATCHER (undocumented): grep, egrep, fgrep, perl.
            'X' => {
                self.syntax = match value.as_str() {
                    "grep" => Syntax::Basic,
                    "egrep" => Syntax::Extended,
                    "fgrep" => Syntax::Fixed,
                    "perl" => Syntax::Perl,
                    _ => {
                        return Err(format!("grep: invalid matcher {value}\n"));
                    }
                }
            }
            _ => unreachable!("only value-taking short options reach here"),
        }
        Ok(())
    }

    fn set_directories(&mut self, value: &str) -> std::result::Result<(), String> {
        self.directories = match value {
            "read" => Directories::Read,
            "skip" => Directories::Skip,
            "recurse" => Directories::Recurse,
            _ => {
                return Err(usage_error(format!(
                    "grep: invalid argument '{value}' for '--directories'\nValid arguments are:\n  - 'read'\n  - 'recurse'\n  - 'skip'\n"
                )));
            }
        };
        Ok(())
    }

    fn apply_long(&mut self, name: &str, value: String) -> std::result::Result<(), String> {
        match name {
            "ignore-case" => self.ignore_case = true,
            "no-ignore-case" => self.ignore_case = false,
            "invert-match" => self.invert_match = true,
            "line-number" => self.line_numbers = true,
            "count" => self.count_only = true,
            "files-with-matches" => self.files_with_matches = true,
            "files-without-match" => self.files_without_match = true,
            "only-matching" => self.only_matching = true,
            "word-regexp" => self.word_regex = true,
            "line-regexp" => self.whole_line = true,
            "fixed-strings" => self.syntax = Syntax::Fixed,
            "extended-regexp" => self.syntax = Syntax::Extended,
            "basic-regexp" => self.syntax = Syntax::Basic,
            "perl-regexp" => self.syntax = Syntax::Perl,
            "quiet" | "silent" => self.quiet = true,
            "byte-offset" => self.byte_offset = true,
            "text" => self.binary_files = BinaryFiles::Text,
            "binary-files" => {
                self.binary_files = match value.as_str() {
                    "binary" => BinaryFiles::Binary,
                    "text" => BinaryFiles::Text,
                    "without-match" => BinaryFiles::WithoutMatch,
                    _ => {
                        return Err("grep: unknown binary-files type\n".to_string());
                    }
                }
            }
            "null-data" => self.null_data = true,
            "recursive" => self.directories = Directories::Recurse,
            "dereference-recursive" => {
                self.directories = Directories::Recurse;
                self.dereference = true;
            }
            "directories" => self.set_directories(&value)?,
            "devices" => check_devices(&value)?,
            "no-messages" => self.suppress_errors = true,
            "with-filename" => self.with_filename = Some(true),
            "no-filename" => self.with_filename = Some(false),
            "null" => self.null_filename = true,
            "initial-tab" => self.initial_tab = true,
            "label" => self.label = Some(value),
            "group-separator" => self.group_separator = Some(value),
            "no-group-separator" => self.group_separator = None,
            // Output is uncolored and written per call; binary and unix
            // byte offsets are no-ops on POSIX.
            "color" | "colour" | "line-buffered" | "binary" | "unix-byte-offsets" => {}
            "regexp" => self.patterns.push(value),
            "file" => self.pattern_files.push(value),
            "max-count" => self.max_count = Some(parse_max_count(&value)?),
            "after-context" => self.after_context = Some(parse_context(&value)?),
            "before-context" => self.before_context = Some(parse_context(&value)?),
            "context" => self.default_context = Some(parse_context(&value)?),
            "include" => self.include_patterns.push(value),
            "exclude" => self.exclude_patterns.push(value),
            "exclude-dir" => self.exclude_dir_patterns.push(value),
            "exclude-from" => self.exclude_from.push(value),
            // --help/--version are answered before parsing; reaching here
            // means an abbreviation such as `--hel`.
            "help" | "version" => {
                return Err(format!(
                    "grep: option '--{name}' must be spelled out in full\n"
                ));
            }
            _ => unreachable!("every LONG_OPTIONS entry is handled"),
        }
        Ok(())
    }

    fn before(&self) -> usize {
        self.before_context.or(self.default_context).unwrap_or(0)
    }

    fn after(&self) -> usize {
        self.after_context.or(self.default_context).unwrap_or(0)
    }

    /// Any context option given (even 0): group separators are printed.
    fn context_requested(&self) -> bool {
        self.before_context.is_some()
            || self.after_context.is_some()
            || self.default_context.is_some()
    }

    fn recursive(&self) -> bool {
        self.directories == Directories::Recurse
    }
}

fn parse_max_count(value: &str) -> std::result::Result<usize, String> {
    match value.parse::<i64>() {
        // GNU 3.11: a negative count means no limit.
        Ok(n) if n < 0 => Ok(usize::MAX),
        Ok(n) => Ok(usize::try_from(n).unwrap_or(usize::MAX)),
        Err(_) => Err("grep: invalid max count\n".to_string()),
    }
}

fn check_devices(value: &str) -> std::result::Result<(), String> {
    if matches!(value, "read" | "skip") {
        Ok(())
    } else {
        Err("grep: unknown devices method\n".to_string())
    }
}

/// Split pattern text into grep's pattern list: one pattern per line.
fn split_patterns(text: &str, out: &mut Vec<String>) {
    out.extend(text.split('\n').map(str::to_string));
}

/// Check if a filename matches a shell glob (`--include`/`--exclude`).
fn glob_matches(filename: &str, pattern: &str) -> bool {
    super::ls::glob::glob_match(filename, pattern)
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

/// State shared across all inputs of one grep run.
struct Shared<'a> {
    opts: &'a GrepOptions,
    matcher: &'a PatternMatcher,
    with_filename: bool,
    /// Something was printed through prtext: later groups get a separator.
    used: bool,
}

/// Per-input matching and output state, fed one record at a time. Mirrors
/// GNU grep's `grep()`/`prtext()`/`prline()`.
struct FileScan<'n> {
    name: &'n str,
    line_no: usize,
    byte_pos: u64,
    /// Selected lines so far.
    nlines: usize,
    /// Trailing context lines still to print.
    pending: usize,
    /// Index of the line after the last one printed.
    lastout: Option<usize>,
    /// Unprinted lines kept for leading context: (index, offset, bytes).
    before: VecDeque<(usize, u64, Vec<u8>)>,
    out_quiet: bool,
    out_quiet_0: bool,
    done_on_match: bool,
    /// `-m` reached: only trailing context is left to print.
    max_reached: bool,
    binary: bool,
    nlines_first_null: usize,
    encoding_error_output: bool,
    /// No more input is needed for this file.
    done: bool,
}

const SEP_SELECTED: u8 = b':';
const SEP_CONTEXT: u8 = b'-';

impl<'n> FileScan<'n> {
    fn new(name: &'n str, opts: &GrepOptions) -> Self {
        let done_on_match = opts.quiet || opts.files_with_matches || opts.files_without_match;
        let out_quiet = opts.count_only || done_on_match;
        FileScan {
            name,
            line_no: 0,
            byte_pos: 0,
            nlines: 0,
            pending: 0,
            lastout: None,
            before: VecDeque::new(),
            out_quiet,
            out_quiet_0: out_quiet,
            done_on_match,
            max_reached: false,
            binary: false,
            nlines_first_null: 0,
            encoding_error_output: false,
            done: opts.max_count == Some(0),
        }
    }

    /// A NUL byte was seen: the input is binary from here on.
    fn set_binary(&mut self, opts: &GrepOptions) {
        if self.binary || opts.binary_files == BinaryFiles::Text || opts.null_data {
            return;
        }
        self.binary = true;
        if opts.binary_files == BinaryFiles::WithoutMatch {
            // GNU returns "no lines selected" for the whole file.
            self.nlines = 0;
            self.done = true;
            return;
        }
        if !opts.count_only {
            self.done_on_match = true;
            self.out_quiet = true;
        }
        self.nlines_first_null = self.nlines;
    }

    fn is_selected(&self, sh: &Shared<'_>, text: &str) -> bool {
        sh.matcher.is_match(text) != sh.opts.invert_match
    }

    /// Feed one record (without its terminator).
    fn feed(&mut self, sh: &mut Shared<'_>, record: &[u8], out: &mut Vec<u8>) {
        if self.done {
            return;
        }
        let idx = self.line_no;
        let offset = self.byte_pos;
        self.line_no += 1;
        self.byte_pos += record.len() as u64 + 1;
        let text = String::from_utf8_lossy(record);

        if self.max_reached {
            // GNU 3.11 prints the trailing context after the last allowed
            // match unconditionally, selectable lines included (as context).
            if self.pending > 0 && !self.out_quiet {
                self.print_line(sh, idx, offset, record, &text, SEP_CONTEXT, out);
                self.lastout = Some(idx + 1);
                self.pending -= 1;
            } else {
                self.pending = 0;
            }
            if self.pending == 0 {
                self.done = true;
            }
            return;
        }

        if self.is_selected(sh, &text) {
            self.nlines += 1;
            if !self.out_quiet {
                self.prtext(sh, idx, offset, record, &text, out);
            }
            if self.done_on_match {
                self.done = true;
                return;
            }
            if sh.opts.max_count.is_some_and(|m| self.nlines >= m) {
                self.max_reached = true;
                if self.pending == 0 || self.out_quiet {
                    self.done = true;
                }
            }
            return;
        }
        if self.out_quiet {
            return;
        }
        if self.pending > 0 {
            self.print_line(sh, idx, offset, record, &text, SEP_CONTEXT, out);
            self.lastout = Some(idx + 1);
            self.pending -= 1;
        } else {
            let keep = sh.opts.before();
            if keep > 0 {
                if self.before.len() == keep {
                    self.before.pop_front();
                }
                self.before.push_back((idx, offset, record.to_vec()));
            }
        }
    }

    /// Print a selected line with its leading context and arm trailing
    /// context.
    fn prtext(
        &mut self,
        sh: &mut Shared<'_>,
        idx: usize,
        offset: u64,
        record: &[u8],
        text: &str,
        out: &mut Vec<u8>,
    ) {
        let first = self.before.front().map_or(idx, |b| b.0);
        if sh.opts.context_requested()
            && sh.used
            && self.lastout != Some(first)
            && let Some(sep) = &sh.opts.group_separator
        {
            out.extend_from_slice(sep.as_bytes());
            out.push(b'\n');
        }
        let before: Vec<(usize, u64, Vec<u8>)> = self.before.drain(..).collect();
        for (bi, boff, bytes) in before {
            let btext = String::from_utf8_lossy(&bytes).into_owned();
            self.print_line(sh, bi, boff, &bytes, &btext, SEP_CONTEXT, out);
            if self.out_quiet {
                break;
            }
        }
        if !self.out_quiet {
            self.print_line(sh, idx, offset, record, text, SEP_SELECTED, out);
        }
        self.lastout = Some(idx + 1);
        self.pending = sh.opts.after();
        sh.used = true;
    }

    /// GNU print_line_head: false when the output would contain an encoding
    /// error, which turns the rest of the file binary.
    fn head(
        &mut self,
        sh: &Shared<'_>,
        idx: usize,
        offset: u64,
        content: &[u8],
        sep: u8,
        out: &mut Vec<u8>,
    ) -> bool {
        if sh.opts.binary_files != BinaryFiles::Text && std::str::from_utf8(content).is_err() {
            self.encoding_error_output = true;
            self.done_on_match = true;
            self.out_quiet = true;
            return false;
        }
        let opts = sh.opts;
        if sh.with_filename {
            out.extend_from_slice(self.name.as_bytes());
            out.push(if opts.null_filename { 0 } else { sep });
        }
        if opts.line_numbers {
            out.extend_from_slice((idx + 1).to_string().as_bytes());
            out.push(sep);
        }
        if opts.byte_offset {
            out.extend_from_slice(offset.to_string().as_bytes());
            out.push(sep);
        }
        if opts.initial_tab
            && (sh.with_filename || opts.line_numbers || opts.byte_offset)
            && !content.is_empty()
        {
            out.push(b'\t');
        }
        true
    }

    #[allow(clippy::too_many_arguments)]
    fn print_line(
        &mut self,
        sh: &Shared<'_>,
        idx: usize,
        offset: u64,
        record: &[u8],
        text: &str,
        sep: u8,
        out: &mut Vec<u8>,
    ) {
        let eol = if sh.opts.null_data { 0 } else { b'\n' };
        if !sh.opts.only_matching {
            if self.head(sh, idx, offset, record, sep, out) {
                out.extend_from_slice(record);
                out.push(eol);
            }
            return;
        }
        // -o: only lines that contain a match have something to print.
        let matching = (sep == SEP_SELECTED) != sh.opts.invert_match;
        if !matching {
            return;
        }
        let msep = if sh.opts.invert_match {
            SEP_CONTEXT
        } else {
            SEP_SELECTED
        };
        let valid = std::str::from_utf8(record).is_ok();
        let mut cur = 0;
        while cur <= text.len() {
            let Some((s, e)) = sh.matcher.find_at(text, cur) else {
                break;
            };
            if s == text.len() {
                break;
            }
            if e == s {
                // Empty match: skip one character.
                cur = s + text[s..].chars().next().map_or(1, char::len_utf8);
                continue;
            }
            let piece: &[u8] = if valid {
                &record[s..e]
            } else {
                &text.as_bytes()[s..e]
            };
            let check: &[u8] = if valid { piece } else { record };
            if !self.head(sh, idx, offset + s as u64, check, msep, out) {
                return;
            }
            out.extend_from_slice(piece);
            out.push(eol);
            cur = e;
        }
    }

    /// End of input: per-file summaries. Returns the selected-line count.
    fn finish(&self, sh: &Shared<'_>, out: &mut Vec<u8>, errors: &mut String) -> usize {
        let opts = sh.opts;
        if opts.quiet {
            return self.nlines;
        }
        if opts.count_only {
            if sh.with_filename {
                out.extend_from_slice(self.name.as_bytes());
                out.push(if opts.null_filename { 0 } else { b':' });
            }
            out.extend_from_slice(self.nlines.to_string().as_bytes());
            out.push(b'\n');
        }
        let list = (opts.files_with_matches && self.nlines > 0)
            || (opts.files_without_match && self.nlines == 0);
        if list {
            out.extend_from_slice(self.name.as_bytes());
            out.push(if opts.null_filename { 0 } else { b'\n' });
        }
        if !self.out_quiet_0
            && (self.encoding_error_output || (self.binary && self.nlines_first_null < self.nlines))
        {
            errors.push_str(&format!("grep: {}: binary file matches\n", self.name));
        }
        self.nlines
    }
}

/// One input to search.
enum Source {
    /// Standard input (operand `-`, or no operands).
    Stdin(String),
    /// File contents, with the name to print.
    File(String, Vec<u8>),
}

/// Feed one input through a [`FileScan`]. Output goes to `stream` as it is
/// produced when given (a pipeline stage), else accumulates in `out`.
/// Returns (selected lines, reader gone).
async fn scan_input(
    ctx: &Context<'_>,
    sh: &mut Shared<'_>,
    source: Source,
    out: &mut Vec<u8>,
    errors: &mut String,
    stream: Option<&super::StdoutStream>,
) -> Result<(usize, bool)> {
    let opts = sh.opts;
    let term = if opts.null_data { b'\0' } else { b'\n' };
    let (name, mut input, whole_binary) = match source {
        Source::Stdin(name) => (name, super::InputChunks::stdin(ctx), false),
        Source::File(name, bytes) => {
            let nul = bytes.contains(&0);
            (name, super::InputChunks::bytes(bytes), nul)
        }
    };
    let mut scan = FileScan::new(&name, opts);
    if whole_binary {
        scan.set_binary(opts);
    }
    while !scan.done {
        let Some(piece) = input.next(super::cut_records(term)).await else {
            break;
        };
        ctx.consume_budget_work(1)?;
        if !scan.binary && piece.contains(&0) {
            scan.set_binary(opts);
        }
        for record in piece.split_inclusive(|&b| b == term) {
            if scan.done {
                break;
            }
            let record = record.strip_suffix(&[term]).unwrap_or(record);
            scan.feed(sh, record, out);
        }
        if let Some(s) = stream
            && !out.is_empty()
        {
            if !s.write(out).await {
                return Ok((scan.nlines, true));
            }
            out.clear();
        }
    }
    let n = scan.finish(sh, out, errors);
    if let Some(s) = stream
        && !out.is_empty()
    {
        if !s.write(out).await {
            return Ok((n, true));
        }
        out.clear();
    }
    Ok((n, false))
}

#[async_trait]
impl Builtin for Grep {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        if let Some(r) = super::check_help_version(
            ctx.args,
            "Usage: grep [OPTION]... PATTERNS [FILE]...\nSearch for PATTERNS in each FILE.\n\n  -E, --extended-regexp\t\tPATTERNS are extended regular expressions\n  -F, --fixed-strings\t\tPATTERNS are strings\n  -G, --basic-regexp\t\tPATTERNS are basic regular expressions (default)\n  -P, --perl-regexp\t\tPATTERNS are Perl regular expressions\n  -e, --regexp=PATTERNS\t\tuse PATTERNS for matching\n  -f, --file=FILE\t\ttake PATTERNS from FILE\n  -i, --ignore-case\t\tignore case distinctions\n      --no-ignore-case\t\tdo not ignore case distinctions (default)\n  -w, --word-regexp\t\tmatch only whole words\n  -x, --line-regexp\t\tmatch only whole lines\n  -z, --null-data\t\ta data line ends in 0 byte, not newline\n  -s, --no-messages\t\tsuppress error messages\n  -v, --invert-match\t\tselect non-matching lines\n  -m, --max-count=NUM\t\tstop after NUM selected lines\n  -b, --byte-offset\t\tprint the byte offset with output lines\n  -n, --line-number\t\tprint line number with output lines\n      --line-buffered\t\tflush output on every line (no-op)\n  -H, --with-filename\t\tprint file name with output lines\n  -h, --no-filename\t\tsuppress the file name prefix on output\n      --label=LABEL\t\tuse LABEL as the standard input file name prefix\n  -o, --only-matching\t\tshow only nonempty parts of lines that match\n  -q, --quiet, --silent\t\tsuppress all normal output\n      --binary-files=TYPE\tbinary, text or without-match\n  -a, --text\t\t\tequivalent to --binary-files=text\n  -I\t\t\t\tequivalent to --binary-files=without-match\n  -d, --directories=ACTION\tread, recurse or skip\n  -r, --recursive\t\tlike --directories=recurse\n  -R, --dereference-recursive\tlikewise, but follow all symlinks\n      --include=GLOB\t\tsearch only files that match GLOB\n      --exclude=GLOB\t\tskip files that match GLOB\n      --exclude-from=FILE\tskip files that match any GLOB in FILE\n      --exclude-dir=GLOB\tskip directories that match GLOB\n  -L, --files-without-match\tprint only names of FILEs with no selected lines\n  -l, --files-with-matches\tprint only names of FILEs with selected lines\n  -c, --count\t\t\tprint only a count of selected lines per FILE\n  -T, --initial-tab\t\tmake tabs line up (if needed)\n  -Z, --null\t\t\tprint 0 byte after FILE name\n  -B, --before-context=NUM\tprint NUM lines of leading context\n  -A, --after-context=NUM\tprint NUM lines of trailing context\n  -C, --context=NUM\t\tprint NUM lines of output context\n  -NUM\t\t\t\tsame as --context=NUM\n      --group-separator=SEP\tprint SEP on line between matches with context\n      --no-group-separator\tdo not print separator for matches with context\n      --color[=WHEN]\t\tuse markers to highlight (no-op)\n  --help\t\t\tdisplay this help and exit\n  --version\t\t\toutput version information and exit\n",
            Some("grep (bashkit) 0.1"),
        ) {
            return Ok(r);
        }
        let mut opts = match GrepOptions::parse(ctx.args) {
            Ok(o) => o,
            Err(msg) => return Ok(ExecResult::err(msg, 2)),
        };

        // Pattern list: every -e entry and -f line, split on newlines.
        let mut patterns = Vec::new();
        for p in &opts.patterns {
            split_patterns(p, &mut patterns);
        }
        for pattern_file in &opts.pattern_files {
            let content = match read_operand_file(&ctx, pattern_file).await {
                Ok(c) => c,
                Err(reason) => {
                    return Ok(ExecResult::err(
                        format!("grep: {pattern_file}: {reason}\n"),
                        2,
                    ));
                }
            };
            let text = String::from_utf8_lossy(&content);
            // A final newline ends the last pattern; it does not add an
            // empty one. An empty file holds no patterns at all.
            let text = text.strip_suffix('\n').unwrap_or(&text);
            if !content.is_empty() {
                split_patterns(text, &mut patterns);
            }
        }
        for exclude_file in &opts.exclude_from {
            match read_operand_file(&ctx, exclude_file).await {
                Ok(content) => {
                    let text = String::from_utf8_lossy(&content);
                    opts.exclude_patterns
                        .extend(text.lines().filter(|l| !l.is_empty()).map(str::to_string));
                }
                Err(reason) => {
                    return Ok(ExecResult::err(
                        format!("grep: {exclude_file}: {reason}\n"),
                        2,
                    ));
                }
            }
        }

        // GNU grep treats `-m0` as "stop before selecting anything": it exits 1
        // with no output, no diagnostic, and without opening any operand or
        // compiling the pattern. Verified against GNU grep 3.11, where
        // `grep -m0 '[' missing` exits 1 rather than reporting either the
        // invalid pattern or the missing file. This must therefore run *before*
        // compiling, or an unparseable pattern would still fail.
        //
        // `-L` is the documented exception: "no line was selected" is exactly
        // what makes every operand qualify, so it still opens each one, reports
        // read errors with status 2, and lists the files.
        if opts.max_count == Some(0) && !opts.files_without_match {
            return Ok(ExecResult::with_code(String::new(), 1));
        }

        let compile_opts = CompileOptions {
            syntax: opts.syntax,
            icase: opts.ignore_case,
            word: opts.word_regex,
            line: opts.whole_line,
            null_data: opts.null_data,
        };
        let (matcher, warnings) = match super::grep_pattern::compile(&patterns, &compile_opts) {
            Ok(m) => m,
            Err(msg) => return Ok(ExecResult::err(format!("grep: {msg}\n"), 2)),
        };
        let mut errors = String::new();
        for w in warnings {
            errors.push_str(&format!("grep: {w}\n"));
        }

        let stdin_name = opts
            .label
            .clone()
            .unwrap_or_else(|| "(standard input)".to_string());
        // GNU `grep -r PAT` with no operand searches the working directory
        // and names matches relative to it (`a:x`, not `./a:x`).
        let implicit_root = opts.recursive() && opts.files.is_empty();
        let operands: Vec<String> = if implicit_root {
            vec![".".to_string()]
        } else if opts.files.is_empty() {
            vec!["-".to_string()]
        } else {
            opts.files.clone()
        };

        // GNU: filenames are shown for several operands, or when recursing
        // into a directory; -H/-h override.
        let multiple = operands.len() > 1;

        // A pipeline stage that prints matching lines streams them: lines
        // go out as they are found, and `loop | grep y | head -1` ends with
        // SIGPIPE. Options that summarize (-c, -l, -L, -q), look around (-A,
        // -B, -C), offset (-b) or recurse keep the buffered path.
        let stream = if !opts.recursive()
            && !opts.count_only
            && !opts.files_with_matches
            && !opts.files_without_match
            && !opts.quiet
            && !opts.byte_offset
            && !opts.context_requested()
        {
            ctx.stdout_stream()
        } else {
            None
        };

        let mut out: Vec<u8> = Vec::new();
        let mut any_match = false;
        // GNU grep exits 2 when any operand could not be read, whether or not
        // another operand matched, and `-s` silences the message but not the
        // status.
        let mut read_failed = false;
        let mut sh = Shared {
            opts: &opts,
            matcher: &matcher,
            with_filename: false,
            used: false,
        };

        'operands: for operand in &operands {
            let mut sources: Vec<Source> = Vec::new();
            let mut is_dir_walk = false;
            if operand == "-" {
                sources.push(Source::Stdin(stdin_name.clone()));
            } else {
                let path = resolve(ctx.cwd, operand);
                let is_dir = matches!(ctx.fs.stat(&path).await, Ok(m) if m.file_type.is_dir());
                if is_dir {
                    match opts.directories {
                        Directories::Skip => continue,
                        Directories::Read => {
                            read_failed = true;
                            if !opts.suppress_errors {
                                errors.push_str(&format!("grep: {operand}: Is a directory\n"));
                            }
                            continue;
                        }
                        Directories::Recurse => {
                            is_dir_walk = true;
                            let display = if implicit_root { "" } else { operand.as_str() };
                            collect_recursive(
                                &ctx,
                                &opts,
                                &matcher,
                                display,
                                &path,
                                &mut sources,
                                &mut errors,
                                &mut read_failed,
                            )
                            .await;
                        }
                    }
                } else {
                    match ctx.fs.read_file(&path).await {
                        Ok(bytes) => sources.push(Source::File(operand.clone(), bytes)),
                        Err(e) => {
                            read_failed = true;
                            if !opts.suppress_errors {
                                let reason = crate::error::io_error_reason(&e);
                                errors.push_str(&format!("grep: {operand}: {reason}\n"));
                            }
                            continue;
                        }
                    }
                }
            }
            sh.with_filename = opts.with_filename.unwrap_or(multiple || is_dir_walk);
            for source in sources {
                let (n, gone) = scan_input(
                    &ctx,
                    &mut sh,
                    source,
                    &mut out,
                    &mut errors,
                    stream.as_ref(),
                )
                .await?;
                if gone {
                    return Ok(ExecResult::err(errors, 141));
                }
                if n > 0 {
                    any_match = true;
                    if opts.quiet {
                        break 'operands;
                    }
                }
            }
        }

        // An unreadable operand outranks "no match", but not a match found
        // under `-q`: GNU `grep -q hello ok.txt /nope` exits 0, because it
        // stops at the first match without reaching the bad operand.
        // Bashkit reads operands lazily in order, so a later bad operand is
        // never reached; an earlier one already printed its message.
        let exit_code = if opts.quiet && any_match {
            0
        } else if read_failed {
            2
        } else if any_match {
            0
        } else {
            1
        };
        if opts.quiet {
            return Ok(ExecResult {
                stderr: errors.into(),
                ..ExecResult::with_code(String::new(), exit_code)
            });
        }
        Ok(ExecResult {
            stderr: errors.into(),
            ..ExecResult::with_code(out, exit_code)
        })
    }
}

fn resolve(cwd: &std::path::Path, file: &str) -> std::path::PathBuf {
    if file.starts_with('/') {
        std::path::PathBuf::from(file)
    } else {
        vfs_join(cwd, file)
    }
}

/// Read a file named on the command line (`-f`, `--exclude-from`); `-`
/// is standard input. `Err` is the strerror-style reason.
async fn read_operand_file(ctx: &Context<'_>, name: &str) -> std::result::Result<Vec<u8>, String> {
    if name == "-" {
        return Ok(ctx
            .stdin_to_end()
            .await
            .map(|s| s.as_bytes().to_vec())
            .unwrap_or_default());
    }
    ctx.fs
        .read_file(&resolve(ctx.cwd, name))
        .await
        .map_err(|e| crate::error::io_error_reason(&e))
}

/// Collect the files under a recursive operand, in name order (files of a
/// directory first, then its subdirectories depth-first).
#[allow(clippy::too_many_arguments)]
async fn collect_recursive(
    ctx: &Context<'_>,
    opts: &GrepOptions,
    matcher: &PatternMatcher,
    operand: &str,
    root: &std::path::Path,
    sources: &mut Vec<Source>,
    errors: &mut String,
    read_failed: &mut bool,
) {
    let root = crate::fs::normalize_path(root);
    // Indexed search via SearchCapable when the backend offers one. Skip it
    // for -P and back-references: the backend's engine cannot express them.
    if !matches!(matcher, PatternMatcher::Fancy(_))
        && let Some(found) = try_indexed_search(&*ctx.fs, opts, &root, operand).await
    {
        sources.extend(found);
        return;
    }
    let mut stack: Vec<std::path::PathBuf> = vec![root.clone()];
    while let Some(dir) = stack.pop() {
        let mut entries = match ctx.fs.read_dir(&dir).await {
            Ok(e) => e,
            Err(e) => {
                *read_failed = true;
                if !opts.suppress_errors {
                    let shown = recursive_display(operand, &root, &dir);
                    let reason = crate::error::io_error_reason(&e);
                    errors.push_str(&format!("grep: {shown}: {reason}\n"));
                }
                continue;
            }
        };
        // Name order keeps output stable across VFS backends.
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        let mut subdirs = Vec::new();
        for entry in entries {
            let entry_path = vfs_join(&dir, &entry.name);
            let mut ft = entry.metadata.file_type;
            if ft.is_symlink() {
                // -r skips symlinks met while recursing; -R follows them.
                if !opts.dereference {
                    continue;
                }
                match ctx.fs.stat(&entry_path).await {
                    Ok(m) => ft = m.file_type,
                    Err(_) => continue,
                }
            }
            if ft.is_dir() {
                if opts
                    .exclude_dir_patterns
                    .iter()
                    .any(|p| glob_matches(&entry.name, p))
                {
                    continue;
                }
                subdirs.push(entry_path);
            } else if ft.is_file()
                && should_include_file(&entry.name, &opts.include_patterns, &opts.exclude_patterns)
            {
                match ctx.fs.read_file(&entry_path).await {
                    Ok(bytes) => sources.push(Source::File(
                        recursive_display(operand, &root, &entry_path),
                        bytes,
                    )),
                    Err(e) => {
                        *read_failed = true;
                        if !opts.suppress_errors {
                            let shown = recursive_display(operand, &root, &entry_path);
                            let reason = crate::error::io_error_reason(&e);
                            errors.push_str(&format!("grep: {shown}: {reason}\n"));
                        }
                    }
                }
            }
        }
        stack.extend(subdirs.into_iter().rev());
    }
}

/// Name a file found under a recursive-grep operand the way GNU grep does:
/// the operand as typed, then the path below it (`d/` + `b` -> `d/b`,
/// `.` + `b` -> `./b`). An empty operand is the implicit `-r` root and
/// yields the bare relative path. Always `/`-separated.
fn recursive_display(operand: &str, root: &std::path::Path, path: &std::path::Path) -> String {
    let rel: Vec<String> = match path.strip_prefix(root) {
        Ok(rel) => rel
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect(),
        Err(_) => return path.to_string_lossy().replace('\\', "/"),
    };
    if rel.is_empty() {
        return operand.to_string();
    }
    let rel = rel.join("/");
    if operand.is_empty() {
        return rel;
    }
    // `/` trims to empty and still yields `/rel`.
    let base = operand.trim_end_matches('/');
    format!("{base}/{rel}")
}

/// Try to use an indexed search provider for recursive grep.
///
/// Returns `Some(inputs)` if a `SearchCapable` provider handled the search,
/// `None` to fall back to linear scan.
async fn try_indexed_search(
    fs: &dyn crate::fs::FileSystem,
    opts: &GrepOptions,
    root: &std::path::Path,
    operand: &str,
) -> Option<Vec<Source>> {
    if opts.invert_match
        || opts.files_without_match
        || opts.count_only
        || opts.patterns.len() != 1
        || !opts.pattern_files.is_empty()
        || opts.patterns[0].contains('\n')
    {
        return None;
    }

    let sc = fs.as_search_capable()?;
    let provider = sc.search_provider(root)?;
    let caps = provider.capabilities();
    if !caps.content_search {
        return None;
    }

    let pattern = if opts.syntax == Syntax::Fixed {
        opts.patterns[0].clone()
    } else {
        if opts.syntax == Syntax::Perl || !caps.regex {
            return None;
        }
        let dialect = if opts.syntax == Syntax::Extended {
            super::grep_pattern::Dialect::Extended
        } else {
            super::grep_pattern::Dialect::Basic
        };
        let t =
            super::grep_pattern::translate(&opts.patterns[0], dialect, opts.ignore_case, false, 0)
                .ok()?;
        if t.backrefs {
            return None;
        }
        let pattern = if opts.word_regex {
            format!(r"\b{{start-half}}(?:{})\b{{end-half}}", t.rust)
        } else {
            t.rust
        };
        if opts.whole_line {
            format!("^(?:{})$", pattern)
        } else {
            pattern
        }
    };

    let query = crate::fs::SearchQuery {
        pattern,
        is_regex: opts.syntax != Syntax::Fixed,
        case_insensitive: opts.ignore_case,
        root: root.to_path_buf(),
        glob_filter: if caps.glob_filter && opts.include_patterns.len() == 1 {
            opts.include_patterns.first().cloned()
        } else {
            None
        },
        max_results: opts.max_count,
    };

    let results = provider.search(&query).ok()?;
    let mut seen_paths = std::collections::HashSet::new();
    let mut inputs = Vec::new();
    for m in &results.matches {
        let candidate = if m.path.is_absolute() {
            crate::fs::normalize_path(&m.path)
        } else {
            crate::fs::normalize_path(&vfs_join(root, &m.path))
        };

        if !candidate.starts_with(root) || !seen_paths.insert(candidate.clone()) {
            continue;
        }

        let Some(name) = candidate.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if !should_include_file(name, &opts.include_patterns, &opts.exclude_patterns)
            || path_has_excluded_dir(root, &candidate, &opts.exclude_dir_patterns)
        {
            continue;
        }

        if let Ok(content) = fs.read_file(&candidate).await {
            inputs.push(Source::File(
                recursive_display(operand, root, &candidate),
                content,
            ));
        }
    }

    if inputs.is_empty() {
        return None;
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
    use std::sync::{Arc, Mutex};

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
            stdin: crate::builtins::test_stream_opt(stdin),
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
        query_max_results: Option<Arc<Mutex<Vec<Option<usize>>>>>,
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
        query_max_results: Option<Arc<Mutex<Vec<Option<usize>>>>>,
    }

    impl SearchProvider for IndexedProvider {
        fn search(&self, query: &SearchQuery) -> Result<SearchResults> {
            if let Some(query_max_results) = &self.query_max_results {
                query_max_results
                    .lock()
                    .expect("query max results lock should not be poisoned")
                    .push(query.max_results);
            }
            let matches = if let Some(max_results) = query.max_results {
                self.matches.iter().take(max_results).cloned().collect()
            } else {
                self.matches.clone()
            };
            Ok(SearchResults {
                matches,
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
                query_max_results: self.query_max_results.clone(),
            }))
        }
    }

    async fn run_grep_with_indexed_fs(
        inner: InMemoryFs,
        matches: Vec<SearchMatch>,
        args: &[&str],
    ) -> Result<ExecResult> {
        let grep = Grep;
        let fs: Arc<dyn FileSystem> = Arc::new(IndexedTestFs {
            inner,
            matches,
            query_max_results: None,
        });
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
    async fn test_grep_invalid_option() {
        let result = run_grep(&["-Q", "hello"], Some("hello world"))
            .await
            .unwrap();
        assert_eq!(result.exit_code, 2);
        assert!(result.stderr.contains("invalid option -- 'Q'"));
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
    async fn test_grep_only_matching_max_count_counts_lines() {
        // GNU: -m counts selected lines; -o still prints every match of the
        // last allowed line.
        let result = run_grep(&["-om1", "."], Some("ab\ncd\n")).await.unwrap();

        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "a\nb\n");
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
        assert_eq!(result.stdout, "(standard input)\n");
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
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
        };

        let result = grep.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("/dir/a.txt:hello"));
        assert!(!result.stdout.contains("b.log"));
    }

    // Issue #2425: `grep -r` prints the paths it walks, so a relative operand
    // resolved against the cwd and every directory entry below it must stay
    // `/`-separated even where the host separator is `\`.
    #[tokio::test]
    async fn windows_containment_grep_recursive_prints_slash_separated_paths() {
        let grep = Grep;
        let fs = Arc::new(InMemoryFs::new());
        fs.mkdir(&PathBuf::from("/d/proj/src"), true).await.unwrap();
        fs.write_file(&PathBuf::from("/d/proj/src/main.rs"), b"fn main() {}\n")
            .await
            .unwrap();

        let mut vars = HashMap::new();
        let mut cwd = PathBuf::from("/d/proj");
        let args: Vec<String> = ["-r", "main", "src"]
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
            result.stdout.starts_with("src/main.rs:"),
            "match path is not slash-separated:\n{}",
            result.stdout
        );
        assert!(
            !result.stdout.contains('\\'),
            "output leaked a host separator:\n{}",
            result.stdout
        );
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
            query_max_results: None,
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
            query_max_results: None,
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
            query_max_results: None,
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
    async fn test_grep_recursive_indexed_search_passes_max_count_to_provider() {
        let inner = InMemoryFs::new();
        inner.mkdir(Path::new("/dir"), true).await.unwrap();
        inner
            .write_file(Path::new("/dir/first.txt"), b"needle first\n")
            .await
            .unwrap();
        inner
            .write_file(Path::new("/dir/second.txt"), b"needle second\n")
            .await
            .unwrap();
        let query_max_results = Arc::new(Mutex::new(Vec::new()));
        let fs: Arc<dyn FileSystem> = Arc::new(IndexedTestFs {
            inner,
            matches: vec![
                SearchMatch {
                    path: PathBuf::from("/dir/first.txt"),
                    line_number: 1,
                    line_content: "needle first".to_string(),
                },
                SearchMatch {
                    path: PathBuf::from("/dir/second.txt"),
                    line_number: 1,
                    line_content: "needle second".to_string(),
                },
            ],
            query_max_results: Some(query_max_results.clone()),
        });

        let grep = Grep;
        let mut vars = HashMap::new();
        let mut cwd = PathBuf::from("/");
        let args: Vec<String> = ["-r", "-m1", "needle", "/dir"]
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
        assert_eq!(result.stdout, "/dir/first.txt:needle first\n");
        assert_eq!(
            query_max_results
                .lock()
                .expect("query max results lock should not be poisoned")
                .as_slice(),
            &[Some(1)]
        );
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
        // GNU: the name is printed, but no match was found, so the status is 1.
        assert_eq!(result.exit_code, 1);
        assert_eq!(result.stdout, "(standard input)\n");
    }

    #[tokio::test]
    async fn test_grep_files_without_match_stdin_has_match() {
        let result = run_grep(&["-L", "foo"], Some("foo\nbar")).await.unwrap();
        // GNU: -L prints nothing here, but a match *was* found, so status 0.
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "");
    }

    #[tokio::test]
    async fn test_grep_files_without_match_long_flag() {
        let result = run_grep(&["--files-without-match", "xyz"], Some("foo\nbar"))
            .await
            .unwrap();
        assert_eq!(result.exit_code, 1);
        assert_eq!(result.stdout, "(standard input)\n");
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
        // GNU grep: -s silences the message but not the status.
        assert_eq!(result.exit_code, 2);
        assert_eq!(result.stdout, "");
        assert_eq!(result.stderr, "");
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
        // GNU grep: the diagnostic goes to stderr, never into the data
        // stream, and an unreadable operand is status 2, not "no match".
        assert_eq!(result.exit_code, 2);
        assert_eq!(result.stdout, "");
        assert_eq!(
            result.stderr,
            "grep: /nonexistent: No such file or directory\n"
        );
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
        // --no-messages is the long form of -s: same status 2.
        assert_eq!(result.exit_code, 2);
        assert_eq!(result.stdout, "");
        assert_eq!(result.stderr, "");
    }

    // -Z (--null) tests

    #[tokio::test]
    async fn test_grep_null_filename_with_l() {
        let result = run_grep(&["-lZ", "foo"], Some("foo\nbar")).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "(standard input)\0");
    }

    #[tokio::test]
    async fn test_grep_null_filename_with_big_l() {
        let result = run_grep(&["-LZ", "xyz"], Some("foo\nbar")).await.unwrap();
        assert_eq!(result.exit_code, 1);
        assert_eq!(result.stdout, "(standard input)\0");
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
        assert_eq!(result.stdout, "(standard input)\0");
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
        // -P then -E: extended wins. GNU ERE supports back-references too,
        // but `\d` would be a literal `d`, not a digit.
        let result = run_grep(&["-P", "-E", r"\d"], Some("d\n1\n"))
            .await
            .unwrap();
        assert_eq!(result.stdout, "d\n");
    }

    #[tokio::test]
    async fn test_grep_pattern_type_long_options_last_wins() {
        // --perl-regexp then --extended-regexp: extended wins -> `\d` is `d`.
        let result = run_grep(
            &["--perl-regexp", "--extended-regexp", r"\d"],
            Some("d\n1\n"),
        )
        .await
        .unwrap();
        assert_eq!(result.stdout, "d\n");
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
        let result = run_grep(&["-P", "(foo"], Some("foo")).await.unwrap();
        assert_eq!(result.exit_code, 2);
        assert!(result.stderr.starts_with("grep: "));
        assert!(!result.stderr.contains("Error {"));
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
        let result = run_grep(&["--max-count"], Some("foo")).await.unwrap();
        assert_eq!(result.exit_code, 2);
        assert!(
            result
                .stderr
                .starts_with("grep: option '--max-count' requires an argument\n")
        );
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

    #[tokio::test]
    async fn test_grep_bre_backreference_backtracking_is_bounded() {
        // TM-DOS-025: BRE back-references run on the backtracking engine;
        // a pathological pattern must still terminate.
        let haystack = format!("{}!", "a".repeat(40));
        let result = run_grep(&[r"\(a*\)*\1b$"], Some(&haystack)).await.unwrap();
        assert_eq!(result.exit_code, 1);
    }

    #[tokio::test]
    async fn test_grep_invalid_pattern_messages_have_no_debug_shape() {
        for (flag, pat) in [
            ("-G", r"\("),
            ("-E", "["),
            ("-G", r"a\{1"),
            ("-E", "a{2,1}"),
        ] {
            let result = run_grep(&[flag, pat], Some("a")).await.unwrap();
            assert_eq!(result.exit_code, 2, "{pat}");
            assert!(result.stderr.starts_with("grep: "), "{pat}");
            assert!(!result.stderr.contains("Error"), "{pat}");
            assert!(result.stderr.len() < 1024);
        }
    }
}
