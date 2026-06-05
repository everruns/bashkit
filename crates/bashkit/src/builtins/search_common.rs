//! Shared utilities for grep and rg builtins.
//!
//! Extracted from duplicated code in grep.rs and rg.rs to provide a single
//! canonical implementation of common search operations.

use regex::{Regex, RegexBuilder};

use crate::error::{Error, Result};

/// Default compiled-regex size limit (1 MB).
pub(crate) const REGEX_SIZE_LIMIT: usize = 1_000_000;

/// Default DFA size limit (1 MB).
pub(crate) const REGEX_DFA_SIZE_LIMIT: usize = 1_000_000;

/// Backtracking step cap for the fancy-regex (PCRE / `grep -P`) engine.
/// Bounds worst-case backtracking so a pathological pattern can't hang the
/// sandbox (mirrors `sed.rs`'s fallback limit; see TM-INF threat model).
pub(crate) const FANCY_BACKTRACK_LIMIT: usize = 1_000_000;

/// Build a regex with enforced size limits.
pub(crate) fn build_regex(pattern: &str) -> std::result::Result<Regex, regex::Error> {
    build_regex_opts(pattern, false)
}

/// Build a regex with enforced size limits and optional case-insensitivity.
pub(crate) fn build_regex_opts(
    pattern: &str,
    case_insensitive: bool,
) -> std::result::Result<Regex, regex::Error> {
    RegexBuilder::new(pattern)
        .case_insensitive(case_insensitive)
        .size_limit(REGEX_SIZE_LIMIT)
        .dfa_size_limit(REGEX_DFA_SIZE_LIMIT)
        .build()
}

/// A compiled search pattern backed by either the default linear-time `regex`
/// engine or the backtracking `fancy_regex` engine (used for `grep -P`,
/// enabling lookaround and backreferences that `regex` rejects).
///
/// The two engines expose different APIs (`fancy_regex` returns `Result` from
/// `is_match`/`find_iter`); this enum hides that behind a uniform surface so
/// callers needn't branch on the engine. Match failures from the backtracking
/// engine (e.g. hitting `FANCY_BACKTRACK_LIMIT`) are treated as "no match".
pub(crate) enum Matcher {
    Standard(Regex),
    Fancy(fancy_regex::Regex),
}

impl Matcher {
    /// Whether `text` contains at least one match.
    pub(crate) fn is_match(&self, text: &str) -> bool {
        match self {
            Matcher::Standard(re) => re.is_match(text),
            Matcher::Fancy(re) => re.is_match(text).unwrap_or(false),
        }
    }

    /// Visit non-overlapping match byte ranges left to right.
    ///
    /// The callback returns `false` to stop scanning immediately. Keep this
    /// lazy so grep early exits (`-m`, `-q`, `-l`, `-L`) bound work and memory
    /// even for dense `-o` matches on large single-line files.
    pub(crate) fn for_each_range(&self, text: &str, mut visit: impl FnMut((usize, usize)) -> bool) {
        match self {
            Matcher::Standard(re) => {
                for m in re.find_iter(text) {
                    if !visit((m.start(), m.end())) {
                        break;
                    }
                }
            }
            // `find_iter` yields `Result<Match, _>`; `Err` arms
            // (backtrack-limit / internal errors) keep the same "no match"
            // policy as `is_match` by not calling the visitor.
            Matcher::Fancy(re) => {
                for m in re.find_iter(text).flatten() {
                    if !visit((m.start(), m.end())) {
                        break;
                    }
                }
            }
        }
    }
}

/// Build a PCRE-style matcher (`grep -P`) with enforced size and backtracking
/// limits and optional case-insensitivity.
// THREAT[TM-DOS-025]: fancy-regex is a backtracking engine; the backtrack_limit
// caps worst-case work so a crafted pattern can't hang the sandbox.
pub(crate) fn build_fancy_matcher(
    pattern: &str,
    case_insensitive: bool,
) -> std::result::Result<Matcher, fancy_regex::Error> {
    fancy_regex::RegexBuilder::new(pattern)
        .case_insensitive(case_insensitive)
        .delegate_size_limit(REGEX_SIZE_LIMIT)
        .delegate_dfa_size_limit(REGEX_DFA_SIZE_LIMIT)
        .backtrack_limit(FANCY_BACKTRACK_LIMIT)
        .build()
        .map(Matcher::Fancy)
}

/// Parse a numeric flag argument from short-flag character stream.
///
/// Handles both `-m5` (value in same arg) and `-m 5` (value in next arg) forms.
/// Returns the parsed value and the new index into `args`.
///
/// # Arguments
/// * `chars` — remaining characters in the current short flag arg
/// * `j` — current position in `chars` (after the flag letter)
/// * `i` — current position in `args`
/// * `args` — full argument list
/// * `cmd_name` — command name for error messages (e.g. "grep", "rg")
/// * `flag_name` — flag name for error messages (e.g. "-m", "-A")
pub(crate) fn parse_numeric_flag_arg(
    chars: &[char],
    j: usize,
    i: &mut usize,
    args: &[String],
    cmd_name: &str,
    flag_name: &str,
) -> Result<usize> {
    let rest: String = chars[j + 1..].iter().collect();
    let num_str = if !rest.is_empty() {
        rest
    } else {
        *i += 1;
        if *i < args.len() {
            args[*i].clone()
        } else {
            return Err(Error::Execution(format!(
                "{}: {} requires an argument",
                cmd_name, flag_name
            )));
        }
    };
    num_str.parse().map_err(|_| {
        Error::Execution(format!(
            "{}: invalid {} value: {}",
            cmd_name, flag_name, num_str
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn for_each_range_stops_when_visitor_returns_false() {
        let matcher = Matcher::Standard(build_regex(".").unwrap());
        let mut visited = 0usize;

        matcher.for_each_range(&"x".repeat(100_000), |_| {
            visited += 1;
            false
        });

        assert_eq!(visited, 1);
    }
}
