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
