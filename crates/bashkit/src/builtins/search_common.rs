//! Shared regex limits and caches for the search builtins (grep, rg, sed, awk).
//!
//! Extracted from duplicated code in grep.rs and rg.rs to provide a single
//! canonical implementation of common search operations.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use regex::{Regex, RegexBuilder};
use regex_automata::{Input, MatchKind, meta, util::captures::Captures};

use crate::builtins::limits::{RUNTIME_REGEX_CACHE_ENTRIES, RUNTIME_REGEX_RETAINED_SCRATCH_BYTES};

/// Default compiled-regex size limit (1 MB).
pub(crate) const REGEX_SIZE_LIMIT: usize = 1_000_000;

/// Default DFA budget (1 MB).
///
/// For `regex` this is `dfa_size_limit`, the lazy-DFA cache capacity. For
/// fancy-regex it is `delegate_dfa_size_limit`, which since 0.20.0 also sets
/// the lazy-DFA cache capacity; 0.20.0 additionally
/// enables `delegate_dfa_skip_cache_capacity_check` by default, so a value too
/// small for a pattern degrades instead of failing to compile.
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
/// One execution-local search cache avoids rebuilding lazy-DFA state on each
/// match. Its retained heap is capped independently of the program entry count.
#[derive(Default)]
pub(crate) struct RuntimeRegexCache {
    // Scripts without conditional regexes need no cache allocation, and the
    // cache does not enlarge every interpreter/async execution frame.
    state: Option<Box<RuntimeRegexState>>,
}

#[derive(Default)]
struct RuntimeRegexState {
    entries: HashMap<Arc<str>, Option<meta::Regex>>,
    insertion_order: VecDeque<Arc<str>>,
    pattern_bytes: usize,
    // Keep the large engine cache out of the Interpreter's inline storage.
    scratch: Option<Box<RegexScratch>>,
    #[cfg(test)]
    compile_count: usize,
}

struct RegexScratch {
    pattern: Arc<str>,
    cache: meta::Cache,
}

pub(crate) struct RuntimeRegexSearch<'a> {
    regex: &'a meta::Regex,
    pattern: &'a Arc<str>,
    scratch: &'a mut Option<Box<RegexScratch>>,
}

impl RuntimeRegexSearch<'_> {
    pub(crate) fn captures(self, haystack: &str) -> Option<Captures> {
        if self
            .scratch
            .as_ref()
            .is_none_or(|scratch| scratch.pattern.as_ref() != self.pattern.as_ref())
        {
            // Release the old cache before allocating another: at most one
            // scratch buffer is live, including when patterns alternate.
            *self.scratch = None;
            *self.scratch = Some(Box::new(RegexScratch {
                pattern: Arc::clone(self.pattern),
                cache: self.regex.create_cache(),
            }));
        }
        let mut captures = self.regex.create_captures();
        let scratch = self.scratch.as_mut().expect("prepared regex scratch");
        self.regex
            .search_captures_with(&mut scratch.cache, &Input::new(haystack), &mut captures);
        // THREAT[TM-DOS-023]: retention cannot multiply the engine's scratch
        // allowance by the number of cached programs or executor threads.
        if scratch.cache.memory_usage() > RUNTIME_REGEX_RETAINED_SCRATCH_BYTES {
            *self.scratch = None;
        }
        captures.is_match().then_some(captures)
    }
}

impl RuntimeRegexCache {
    pub(crate) fn get_or_compile(&mut self, pattern: &str) -> Option<RuntimeRegexSearch<'_>> {
        if pattern.len() > REGEX_SIZE_LIMIT {
            return None;
        }
        self.state
            .get_or_insert_with(Default::default)
            .get_or_compile(pattern)
    }

    #[cfg(test)]
    pub(crate) fn compile_count(&self) -> usize {
        self.state.as_ref().map_or(0, |state| state.compile_count)
    }
}

impl RuntimeRegexState {
    fn get_or_compile(&mut self, pattern: &str) -> Option<RuntimeRegexSearch<'_>> {
        if !self.entries.contains_key(pattern) {
            // Drop scratch before compiling. Even if a formerly active pattern
            // was evicted and is recompiled, no stale program/cache pair survives.
            self.scratch = None;
            #[cfg(test)]
            {
                self.compile_count += 1;
            }
            // Match regex::RegexBuilder's string defaults and existing caps.
            let regex = meta::Regex::builder()
                .configure(
                    meta::Regex::config()
                        .match_kind(MatchKind::LeftmostFirst)
                        .utf8_empty(true)
                        .nfa_size_limit(Some(REGEX_SIZE_LIMIT))
                        .hybrid_cache_capacity(REGEX_DFA_SIZE_LIMIT),
                )
                .syntax(regex_automata::util::syntax::Config::new().utf8(true))
                .build(pattern)
                .ok();
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
            self.entries.insert(key, regex);
        }
        let (key, regex) = self.entries.get_key_value(pattern)?;
        let regex = regex.as_ref()?;
        Some(RuntimeRegexSearch {
            regex,
            pattern: key,
            scratch: &mut self.scratch,
        })
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
        let state = cache.state.as_ref().unwrap();
        assert_eq!(state.entries.len(), RUNTIME_REGEX_CACHE_ENTRIES);
        assert!(state.pattern_bytes <= REGEX_SIZE_LIMIT);
        assert!(
            cache
                .get_or_compile(&"x".repeat(REGEX_SIZE_LIMIT + 1))
                .is_none()
        );
        assert_eq!(cache.compile_count(), RUNTIME_REGEX_CACHE_ENTRIES + 2);
    }

    #[test]
    fn runtime_captures_preserve_string_regex_semantics() {
        let mut cache = RuntimeRegexCache::default();
        for pattern in [
            r#""id":([0-9]+)"#,
            r"^(a)?(b+)$",
            r"(?i)(hello)",
            r"(é|😀)(.)?",
            r"\b(\w+)\b",
            r"(?m)^(.*)$",
            r"",
            r"a|ab",
            r"[",
            r"(?-u:\xFF)",
        ] {
            let ordinary = build_regex(pattern).ok();
            for _ in 0..3 {
                for haystack in [
                    "",
                    "ab",
                    "bbb",
                    "HELLO",
                    "é😀",
                    "first\nlast",
                    r#"{"id":123}"#,
                ] {
                    let expected = ordinary.as_ref().map(|re| {
                        re.captures(haystack).map(|caps| {
                            caps.iter()
                                .map(|m| m.map(|m| (m.start(), m.end())))
                                .collect::<Vec<_>>()
                        })
                    });
                    let actual = cache.get_or_compile(pattern).map(|re| {
                        re.captures(haystack).map(|caps| {
                            caps.iter()
                                .map(|m| m.map(|m| (m.start, m.end)))
                                .collect::<Vec<_>>()
                        })
                    });
                    assert_eq!(actual, expected, "pattern={pattern}, haystack={haystack}");
                    assert!(
                        cache
                            .state
                            .as_ref()
                            .unwrap()
                            .scratch
                            .as_ref()
                            .is_none_or(|scratch| {
                                scratch.cache.memory_usage() <= RUNTIME_REGEX_RETAINED_SCRATCH_BYTES
                            })
                    );
                }
            }
        }
    }

    #[test]
    fn runtime_scratch_handles_switches_and_recompiled_evicted_patterns() {
        let mut cache = RuntimeRegexCache::default();
        for (pattern, haystack, expected) in [
            (r"^(\d+)$", "42", (0, 2)),
            (r"^(x)(y)$", "xy", (0, 1)),
            (r"^(\d+)$", "987", (0, 3)),
        ] {
            let caps = cache
                .get_or_compile(pattern)
                .unwrap()
                .captures(haystack)
                .unwrap();
            let group = caps.get_group(1).unwrap();
            assert_eq!((group.start, group.end), expected);
        }
        assert_eq!(cache.compile_count(), 2);
        for index in 0..RUNTIME_REGEX_CACHE_ENTRIES {
            assert!(cache.get_or_compile(&format!("[invalid{index}")).is_none());
        }
        assert!(
            !cache
                .state
                .as_ref()
                .unwrap()
                .entries
                .contains_key(r"^(\d+)$")
        );
        let caps = cache
            .get_or_compile(r"^(\d+)$")
            .unwrap()
            .captures("1234")
            .unwrap();
        assert_eq!(caps.get_group(1).unwrap().range(), 0..4);
    }

    #[test]
    fn runtime_large_search_releases_scratch_but_keeps_compiled_program() {
        let mut cache = RuntimeRegexCache::default();
        let pattern = r"(?:a?){1000}";
        for haystack in ["", "a", "aaaa"] {
            let caps = cache
                .get_or_compile(pattern)
                .unwrap()
                .captures(haystack)
                .unwrap();
            assert_eq!(caps.get_match().unwrap().range(), 0..haystack.len());
            assert!(
                cache.state.as_ref().unwrap().scratch.is_none(),
                "oversized search heap must not be retained"
            );
        }
        assert_eq!(cache.compile_count(), 1);
    }
}
