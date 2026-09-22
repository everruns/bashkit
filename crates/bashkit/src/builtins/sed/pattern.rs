//! Regex construction for `sed`.
//!
//! The POSIX BRE/ERE translation itself lives in
//! [`crate::builtins::posix_regex`], shared with `grep`; this module only adds
//! sed's engine selection (fancy-regex fallback for back-references) and its
//! size/backtracking limits. Both `s///` patterns and address regexes come
//! through here, which is what issue #2427 finding B was about: address
//! regexes used to skip translation entirely and compile as ERE.
//!
//! sed is the *strict* caller — an invalid quantifier is an error, as in GNU
//! sed — so it passes `lenient: false`.

use regex::{Regex, RegexBuilder};

use crate::builtins::posix_regex::{Syntax, translate};
use crate::builtins::search_common::{REGEX_DFA_SIZE_LIMIT, REGEX_SIZE_LIMIT};

/// A compiled sed regex. Falls back to `fancy_regex` when the pattern uses
/// back-references, which the default engine rejects.
pub(super) enum SedRegex {
    Standard(Regex),
    Fancy(fancy_regex::Regex),
}

/// Backtracking step cap for the fancy-regex fallback.
const FANCY_BACKTRACK_LIMIT: usize = 1_000_000;

/// One match with its capture group spans (byte offsets into the subject).
pub(super) struct MatchSpan {
    pub(super) start: usize,
    pub(super) end: usize,
    pub(super) groups: Vec<Option<(usize, usize)>>,
}

impl SedRegex {
    pub(super) fn new(
        pattern: &str,
        extended: bool,
        case_insensitive: bool,
        multi_line: bool,
    ) -> std::result::Result<Self, String> {
        let translated = translate(pattern, Syntax::new(extended, false)).regex;
        match RegexBuilder::new(&translated)
            .case_insensitive(case_insensitive)
            .multi_line(multi_line)
            .size_limit(REGEX_SIZE_LIMIT)
            .dfa_size_limit(REGEX_DFA_SIZE_LIMIT)
            .build()
        {
            Ok(re) => Ok(SedRegex::Standard(re)),
            Err(e) => {
                let message = e.to_string();
                if message.contains("backreference") {
                    Self::build_fancy(&translated, case_insensitive, multi_line)
                        .map_err(|_| message)
                } else {
                    Err(message)
                }
            }
        }
    }

    pub(super) fn build_fancy(
        pattern: &str,
        case_insensitive: bool,
        multi_line: bool,
    ) -> std::result::Result<Self, String> {
        Self::build_fancy_with_limit(pattern, case_insensitive, multi_line, FANCY_BACKTRACK_LIMIT)
    }

    pub(super) fn build_fancy_with_limit(
        pattern: &str,
        case_insensitive: bool,
        multi_line: bool,
        backtrack_limit: usize,
    ) -> std::result::Result<Self, String> {
        fancy_regex::RegexBuilder::new(pattern)
            .case_insensitive(case_insensitive)
            .multi_line(multi_line)
            .delegate_size_limit(REGEX_SIZE_LIMIT)
            .delegate_dfa_size_limit(REGEX_DFA_SIZE_LIMIT)
            .backtrack_limit(backtrack_limit)
            .build()
            .map(SedRegex::Fancy)
            .map_err(|e| e.to_string())
    }

    /// Number of capture groups, excluding the whole match.
    pub(super) fn group_count(&self) -> usize {
        match self {
            SedRegex::Standard(re) => re.captures_len().saturating_sub(1),
            SedRegex::Fancy(re) => re.captures_len().saturating_sub(1),
        }
    }

    pub(super) fn is_match(&self, text: &str) -> bool {
        match self {
            SedRegex::Standard(re) => re.is_match(text),
            SedRegex::Fancy(re) => re.is_match(text).unwrap_or(false),
        }
    }

    /// All non-overlapping matches, with capture spans.
    pub(super) fn matches(&self, text: &str) -> Vec<MatchSpan> {
        let mut spans = Vec::new();
        match self {
            SedRegex::Standard(re) => {
                for caps in re.captures_iter(text) {
                    let whole = match caps.get(0) {
                        Some(m) => m,
                        None => continue,
                    };
                    spans.push(MatchSpan {
                        start: whole.start(),
                        end: whole.end(),
                        groups: (1..caps.len())
                            .map(|i| caps.get(i).map(|m| (m.start(), m.end())))
                            .collect(),
                    });
                }
            }
            SedRegex::Fancy(re) => {
                for caps in re.captures_iter(text).flatten() {
                    let whole = match caps.get(0) {
                        Some(m) => m,
                        None => continue,
                    };
                    spans.push(MatchSpan {
                        start: whole.start(),
                        end: whole.end(),
                        groups: (1..caps.len())
                            .map(|i| caps.get(i).map(|m| (m.start(), m.end())))
                            .collect(),
                    });
                }
            }
        }
        spans
    }
}
