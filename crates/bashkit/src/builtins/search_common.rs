//! Shared regex limits and caches for the search builtins (grep, rg, sed, awk).
//!
//! Extracted from duplicated code in grep.rs and rg.rs to provide a single
//! canonical implementation of common search operations.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use regex::{Regex, RegexBuilder};

use crate::builtins::limits::RUNTIME_REGEX_CACHE_ENTRIES;

/// Default compiled-regex size limit (1 MB).
pub(crate) const REGEX_SIZE_LIMIT: usize = 1_000_000;

/// Default DFA size limit (1 MB).
pub(crate) const REGEX_DFA_SIZE_LIMIT: usize = 1_000_000;

/// Backtracking step cap for the fancy-regex (PCRE / `grep -P`) engine.
/// Bounds worst-case backtracking so a pathological pattern can't hang the
/// sandbox (mirrors `sed.rs`'s fallback limit; see TM-INF threat model).
pub(crate) const FANCY_BACKTRACK_LIMIT: usize = 1_000_000;

/// THREAT[TM-DOS-023]: Per-evaluator cache for regex operands compiled during
/// execution, preventing repeated compilation from consuming the CPU budget.
///
/// Both successful and failed compilations are retained. Entry count bounds
/// compiled programs; retained pattern text shares [`REGEX_SIZE_LIMIT`].
#[derive(Default)]
pub(crate) struct RuntimeRegexCache {
    entries: HashMap<Arc<str>, Option<Regex>>,
    insertion_order: VecDeque<Arc<str>>,
    pattern_bytes: usize,
    #[cfg(test)]
    compile_count: usize,
}

impl RuntimeRegexCache {
    pub(crate) fn get_or_compile(&mut self, pattern: &str) -> Option<Regex> {
        if let Some(regex) = self.entries.get(pattern) {
            return regex.clone();
        }
        if pattern.len() > REGEX_SIZE_LIMIT {
            return None;
        }

        #[cfg(test)]
        {
            self.compile_count += 1;
        }
        let regex = build_regex(pattern).ok();
        while self.entries.len() >= RUNTIME_REGEX_CACHE_ENTRIES
            || self.pattern_bytes + pattern.len() > REGEX_SIZE_LIMIT
        {
            let Some(oldest) = self.insertion_order.pop_front() else {
                break;
            };
            self.pattern_bytes -= oldest.len();
            self.entries.remove(oldest.as_ref());
        }
        let key: Arc<str> = Arc::from(pattern);
        self.pattern_bytes += key.len();
        self.insertion_order.push_back(Arc::clone(&key));
        self.entries.insert(key, regex.clone());
        regex
    }

    #[cfg(test)]
    pub(crate) fn compile_count(&self) -> usize {
        self.compile_count
    }
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_regex_cache_retains_failures_and_bounds_entries() {
        let mut cache = RuntimeRegexCache::default();
        assert!(cache.get_or_compile("[").is_none());
        assert!(cache.get_or_compile("[").is_none());

        for i in 0..=RUNTIME_REGEX_CACHE_ENTRIES {
            assert!(cache.get_or_compile(&format!("^value{i}$")).is_some());
        }

        assert_eq!(cache.compile_count(), RUNTIME_REGEX_CACHE_ENTRIES + 2);
        assert_eq!(cache.entries.len(), RUNTIME_REGEX_CACHE_ENTRIES);
        assert!(cache.pattern_bytes <= REGEX_SIZE_LIMIT);
        assert!(
            cache
                .get_or_compile(&"x".repeat(REGEX_SIZE_LIMIT + 1))
                .is_none()
        );
        assert_eq!(cache.compile_count(), RUNTIME_REGEX_CACHE_ENTRIES + 2);
    }
}
