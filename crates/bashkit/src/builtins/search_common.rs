//! Shared utilities for grep and rg builtins.
//!
//! Extracted from duplicated code in grep.rs and rg.rs to provide a single
//! canonical implementation of common search operations.

use std::path::PathBuf;
use std::sync::Arc;

use regex::{Regex, RegexBuilder};

use crate::error::{Error, Result};
use crate::fs::FileSystem;

/// Recursively collect all files under the given directories in the VFS.
///
/// Returns sorted list of file paths (directories are traversed but not included).
pub(crate) async fn collect_files_recursive(
    fs: &Arc<dyn FileSystem>,
    dirs: &[PathBuf],
) -> Vec<PathBuf> {
    let mut result = Vec::new();
    let mut stack: Vec<PathBuf> = dirs.to_vec();

    while let Some(current) = stack.pop() {
        if let Ok(entries) = fs.read_dir(&current).await {
            for entry in entries {
                let path = current.join(&entry.name);
                if entry.metadata.file_type.is_dir() {
                    stack.push(path);
                } else if entry.metadata.file_type.is_file() {
                    result.push(path);
                }
            }
        }
    }

    result.sort();
    result
}

/// Build a regex from a single pattern with common options.
///
/// Handles fixed-string escaping, word-boundary wrapping, and case insensitivity.
pub(crate) fn build_search_regex(
    pattern: &str,
    fixed_strings: bool,
    word_boundary: bool,
    ignore_case: bool,
    cmd_name: &str,
) -> Result<Regex> {
    let pat = if fixed_strings {
        regex::escape(pattern)
    } else {
        pattern.to_string()
    };

    let pat = if word_boundary {
        format!(r"\b{}\b", pat)
    } else {
        pat
    };

    RegexBuilder::new(&pat)
        .case_insensitive(ignore_case)
        .build()
        .map_err(|e| Error::Execution(format!("{}: invalid pattern: {}", cmd_name, e)))
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
