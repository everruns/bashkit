//! Glob pattern matching and expansion for the interpreter.
//!
//! Extracted from mod.rs to reduce the god-module size.
//! Contains: pattern matching (glob, extglob, bracket expressions),
//! glob expansion against the filesystem, and related option helpers.

use std::path::{Path, PathBuf};

use crate::error::Result;

use super::Interpreter;
use crate::fs::vfs_join;

/// Expand a POSIX character class name into a list of characters.
pub(super) fn expand_posix_class(name: &str, out: &mut Vec<char>) {
    match name {
        "space" => out.extend([' ', '\t', '\n', '\r', '\x0b', '\x0c']),
        "blank" => out.extend([' ', '\t']),
        "digit" => out.extend('0'..='9'),
        "lower" => out.extend('a'..='z'),
        "upper" => out.extend('A'..='Z'),
        "alpha" => {
            out.extend('a'..='z');
            out.extend('A'..='Z');
        }
        "alnum" => {
            out.extend('a'..='z');
            out.extend('A'..='Z');
            out.extend('0'..='9');
        }
        "xdigit" => {
            out.extend('0'..='9');
            out.extend('a'..='f');
            out.extend('A'..='F');
        }
        "punct" => {
            for c in '!'..='/' {
                out.push(c);
            }
            for c in ':'..='@' {
                out.push(c);
            }
            for c in '['..='`' {
                out.push(c);
            }
            for c in '{'..='~' {
                out.push(c);
            }
        }
        "print" => {
            out.extend(' '..='~');
        }
        "graph" => {
            out.extend('!'..='~');
        }
        "cntrl" => {
            out.extend((0u8..=31).map(|b| b as char));
            out.push(127 as char);
        }
        _ => {} // Unknown class: ignore
    }
}

/// Matching knobs for one pattern match. Callers pick them per context:
/// `[[ == ]]` always understands extglob (bash 5.2), `case` and parameter
/// expansion follow `shopt extglob`; `nocasematch` folds case for `case`,
/// `[[ ]]` and `${v/pat/rep}` but not for `${v#pat}` removal; filename
/// expansion folds case under `nocaseglob`.
#[derive(Clone, Copy, Default)]
pub(crate) struct PatternOpts {
    pub(crate) nocase: bool,
    pub(crate) extglob: bool,
}

impl Interpreter {
    // ── Pattern matching ──────────────────────────────────────────────

    /// Check if pattern contains extglob operators (only when `shopt extglob`).
    pub(crate) fn contains_extglob(&self, s: &str) -> bool {
        self.is_extglob() && Self::has_extglob_op(s)
    }

    /// Unescaped `@(`, `?(`, `*(`, `+(` or `!(` anywhere in `s`.
    fn has_extglob_op(s: &str) -> bool {
        let mut escaped = false;
        let mut chars = s.chars().peekable();
        while let Some(ch) = chars.next() {
            if escaped {
                escaped = false;
                continue;
            }
            if ch == '\\' {
                escaped = true;
                continue;
            }
            if matches!(ch, '@' | '?' | '*' | '+' | '!') && chars.peek() == Some(&'(') {
                return true;
            }
        }
        false
    }

    /// Options for `case` patterns and parameter-expansion patterns:
    /// `shopt extglob`, case-sensitive.
    pub(crate) fn shell_pattern_opts(&self) -> PatternOpts {
        PatternOpts {
            nocase: false,
            extglob: self.is_extglob(),
        }
    }

    /// Options for filename expansion: `shopt extglob` and `nocaseglob`.
    pub(crate) fn glob_opts(&self) -> PatternOpts {
        PatternOpts {
            nocase: self.is_nocaseglob(),
            extglob: self.is_extglob(),
        }
    }

    /// Check if `shopt -s nocasematch` is enabled.
    pub(crate) fn is_nocasematch(&self) -> bool {
        self.scoped
            .variables
            .get("SHOPT_nocasematch")
            .map(|v| v == "1")
            .unwrap_or(false)
    }

    /// Check if a value matches a shell pattern (`shopt extglob`, case-sensitive).
    pub(crate) fn pattern_matches(&self, value: &str, pattern: &str) -> bool {
        self.pattern_matches_opts(value, pattern, self.shell_pattern_opts())
    }

    /// Check if a value matches a shell pattern with explicit options.
    pub(crate) fn pattern_matches_opts(
        &self,
        value: &str,
        pattern: &str,
        opts: PatternOpts,
    ) -> bool {
        // Handle special case of * (match anything)
        if pattern == "*" {
            return true;
        }

        // Glob pattern matching with unescaped *, ?, [], and extglob support
        if self.contains_glob_chars(pattern) || (opts.extglob && Self::has_extglob_op(pattern)) {
            self.glob_match_impl(value, pattern, opts, 0)
        } else {
            // Literal match; a backslash still escapes the next character
            // (`\*` from quoted pattern text matches a literal `*`, and an
            // unquoted `a\b` pattern matches `ab`, as in bash).
            let literal = if pattern.contains('\\') {
                let mut literal = String::with_capacity(pattern.len());
                let mut chars = pattern.chars();
                while let Some(ch) = chars.next() {
                    match ch {
                        '\\' => literal.push(chars.next().unwrap_or('\\')),
                        _ => literal.push(ch),
                    }
                }
                literal
            } else {
                pattern.to_string()
            };
            if opts.nocase {
                value.chars().count() == literal.chars().count()
                    && value
                        .chars()
                        .zip(literal.chars())
                        .all(|(a, b)| a.to_lowercase().eq(b.to_lowercase()))
            } else {
                value == literal
            }
        }
    }

    /// Parse an extglob pattern-list from pattern string starting after '('.
    /// Returns (alternatives, rest_of_pattern) or None if malformed.
    fn parse_extglob_pattern_list(pattern: &str) -> Option<(Vec<String>, String)> {
        let mut depth = 1;
        let mut end = 0;
        let chars: Vec<char> = pattern.chars().collect();
        while end < chars.len() {
            match chars[end] {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        let inner: String = chars[..end].iter().collect();
                        let rest: String = chars[end + 1..].iter().collect();
                        // Split on | at depth 0
                        let mut alts = Vec::new();
                        let mut current = String::new();
                        let mut d = 0;
                        let mut inner_chars = inner.chars();
                        while let Some(c) = inner_chars.next() {
                            match c {
                                // `\|` and `\)` stay literal inside an alternative.
                                '\\' => {
                                    current.push(c);
                                    if let Some(next) = inner_chars.next() {
                                        current.push(next);
                                    }
                                }
                                '(' => {
                                    d += 1;
                                    current.push(c);
                                }
                                ')' => {
                                    d -= 1;
                                    current.push(c);
                                }
                                '|' if d == 0 => {
                                    alts.push(current.clone());
                                    current.clear();
                                }
                                _ => current.push(c),
                            }
                        }
                        alts.push(current);
                        return Some((alts, rest));
                    }
                }
                '\\' => {
                    end += 1; // skip escaped char
                }
                _ => {}
            }
            end += 1;
        }
        None // unclosed paren
    }

    /// `rest` starts at the `(` after an extglob operator and the group
    /// closes; an unbalanced `@(a` is literal text, as in bash.
    fn extglob_group_closes(rest: &std::iter::Peekable<std::str::Chars<'_>>) -> bool {
        let mut rest = rest.clone();
        if rest.next() != Some('(') {
            return false;
        }
        let tail: String = rest.collect();
        Self::parse_extglob_pattern_list(&tail).is_some()
    }

    /// Glob match with explicit options and a fresh step budget.
    pub(crate) fn glob_match_impl(
        &self,
        value: &str,
        pattern: &str,
        opts: PatternOpts,
        depth: usize,
    ) -> bool {
        let steps = std::cell::Cell::new(Self::MAX_GLOB_STEPS);
        self.glob_match_steps(value, pattern, opts, depth, &steps)
    }

    /// Glob match drawing on a caller-owned step budget, so a loop of
    /// matches (pattern removal/substitution scans) shares one budget.
    pub(crate) fn glob_match_steps(
        &self,
        value: &str,
        pattern: &str,
        opts: PatternOpts,
        depth: usize,
        steps: &std::cell::Cell<usize>,
    ) -> bool {
        // THREAT[TM-DOS-031]: Bail on excessive recursion depth
        if depth >= Self::MAX_GLOB_DEPTH {
            return false;
        }
        // THREAT[TM-DOS-031]: extglob alternation/repetition backtracks
        // exponentially (`+(a|aa)` vs `aaa…b`); every recursive step spends
        // from a shared budget and the match fails once it is exhausted.
        if steps.get() == 0 {
            return false;
        }
        steps.set(steps.get() - 1);

        let PatternOpts { nocase, extglob } = opts;
        // THREAT[TM-DOS-039]: Once the remaining pattern has no closing bracket,
        // every `[` is literal. Cache that state so an unmatched suffix is scanned
        // at most once instead of from every subsequent `[` start.
        let mut no_more_bracket_closes = !pattern.contains(']');

        // Check for extglob at the start of pattern
        if extglob && pattern.len() >= 2 {
            let bytes = pattern.as_bytes();
            if matches!(bytes[0], b'@' | b'?' | b'*' | b'+' | b'!') && bytes[1] == b'(' {
                let op = bytes[0];
                if let Some((alts, rest)) = Self::parse_extglob_pattern_list(&pattern[2..]) {
                    return self.match_extglob(op, &alts, &rest, value, opts, depth + 1, steps);
                }
            }
        }

        let mut value_chars = value.chars().peekable();
        let mut pattern_chars = pattern.chars().peekable();

        // THREAT[TM-DOS-031]: `*` matching uses a single backtracking restore
        // point (the classic linear wildcard algorithm) instead of recursing
        // over every value position. A run of `*` — consecutive (`****`) or
        // separated (`*a*b*c`) — no longer causes exponential blowup: each new
        // `*` simply overwrites the restore point (an earlier `*` is provably
        // never needed again), keeping the worst case O(value.len * pattern.len).
        let mut star_pat: Option<std::iter::Peekable<std::str::Chars>> = None;
        let mut star_val: Option<std::iter::Peekable<std::str::Chars>> = None;
        let mut star_no_more_bracket_closes = false;

        // On a content mismatch, resume from the most recent `*`, letting it
        // consume one more value char. With no active `*`, the match fails.
        macro_rules! backtrack {
            () => {{
                if let (Some(sp), Some(sv)) = (&star_pat, star_val.as_mut()) {
                    if sv.peek().is_some() {
                        sv.next();
                        pattern_chars = sp.clone();
                        value_chars = sv.clone();
                        no_more_bracket_closes = star_no_more_bracket_closes;
                        continue;
                    }
                }
                return false;
            }};
        }

        loop {
            match (pattern_chars.peek().copied(), value_chars.peek().copied()) {
                (None, None) => return true,
                (None, Some(_)) => backtrack!(),
                (Some('\\'), Some(v)) => {
                    pattern_chars.next();
                    let literal = pattern_chars.next().unwrap_or('\\');
                    let matches = if nocase {
                        literal.eq_ignore_ascii_case(&v)
                    } else {
                        literal == v
                    };
                    if matches {
                        value_chars.next();
                    } else {
                        backtrack!();
                    }
                }
                (Some('\\'), None) => backtrack!(),
                (Some('*'), _) => {
                    // Check for extglob *(...)
                    let mut pc_clone = pattern_chars.clone();
                    pc_clone.next();
                    if extglob && Self::extglob_group_closes(&pc_clone) {
                        // Extglob *(pattern-list) — collect remaining pattern
                        let remaining_pattern: String = pattern_chars.collect();
                        let remaining_value: String = value_chars.collect();
                        if self.glob_match_steps(
                            &remaining_value,
                            &remaining_pattern,
                            opts,
                            depth + 1,
                            steps,
                        ) {
                            return true;
                        }
                        backtrack!();
                    }
                    pattern_chars.next();
                    // * matches zero or more characters
                    if pattern_chars.peek().is_none() {
                        return true; // * at end matches everything
                    }
                    // Record a restore point: the pattern after this `*` is
                    // matched greedily against the current value position. On a
                    // later mismatch, `backtrack!` lets the `*` consume one more
                    // value char and retries. Overwrites any earlier `*`.
                    star_pat = Some(pattern_chars.clone());
                    star_val = Some(value_chars.clone());
                    star_no_more_bracket_closes = no_more_bracket_closes;
                }
                (Some('?'), _) => {
                    // Check for extglob ?(...)
                    let mut pc_clone = pattern_chars.clone();
                    pc_clone.next();
                    if extglob && Self::extglob_group_closes(&pc_clone) {
                        let remaining_pattern: String = pattern_chars.collect();
                        let remaining_value: String = value_chars.collect();
                        if self.glob_match_steps(
                            &remaining_value,
                            &remaining_pattern,
                            opts,
                            depth + 1,
                            steps,
                        ) {
                            return true;
                        }
                        backtrack!();
                    }
                    if value_chars.peek().is_some() {
                        pattern_chars.next();
                        value_chars.next();
                    } else {
                        backtrack!();
                    }
                }
                (Some('['), Some(v)) => {
                    if no_more_bracket_closes || !pattern_chars.clone().any(|c| c == ']') {
                        no_more_bracket_closes = true;
                        pattern_chars.next();
                        if v == '[' {
                            value_chars.next();
                        } else {
                            backtrack!();
                        }
                        continue;
                    }

                    // Save state before consuming '[' — if bracket expr is
                    // invalid (e.g. "[]"), we fall back to literal '[' match.
                    let saved_pattern = pattern_chars.clone();
                    pattern_chars.next(); // consume '['
                    let match_char = if nocase { v.to_ascii_lowercase() } else { v };
                    if let Some(matched) =
                        self.match_bracket_expr(&mut pattern_chars, match_char, nocase)
                    {
                        if matched {
                            value_chars.next();
                        } else {
                            backtrack!();
                        }
                    } else {
                        // Invalid bracket expression — treat '[' as literal
                        pattern_chars = saved_pattern;
                        pattern_chars.next(); // consume '[' as literal
                        let p = '[';
                        let match_ok = if nocase {
                            p.eq_ignore_ascii_case(&v)
                        } else {
                            p == v
                        };
                        if match_ok {
                            value_chars.next();
                        } else {
                            backtrack!();
                        }
                    }
                }
                (Some('['), None) => backtrack!(),
                (Some(p), Some(v)) => {
                    // Check for extglob operators: @(, +(, !(
                    if extglob && matches!(p, '@' | '+' | '!') {
                        let mut pc_clone = pattern_chars.clone();
                        pc_clone.next();
                        if Self::extglob_group_closes(&pc_clone) {
                            let remaining_pattern: String = pattern_chars.collect();
                            let remaining_value: String = value_chars.collect();
                            if self.glob_match_steps(
                                &remaining_value,
                                &remaining_pattern,
                                opts,
                                depth + 1,
                                steps,
                            ) {
                                return true;
                            }
                            backtrack!();
                        }
                    }
                    let matches = if nocase {
                        p.eq_ignore_ascii_case(&v)
                    } else {
                        p == v
                    };
                    if matches {
                        pattern_chars.next();
                        value_chars.next();
                    } else {
                        backtrack!();
                    }
                }
                (Some(_), None) => backtrack!(),
            }
        }
    }

    /// Match an extglob pattern against a value.
    /// op: b'@', b'?', b'*', b'+', b'!'
    /// alts: the | separated alternatives
    /// rest: pattern after the closing )
    #[allow(clippy::too_many_arguments)]
    fn match_extglob(
        &self,
        op: u8,
        alts: &[String],
        rest: &str,
        value: &str,
        opts: PatternOpts,
        depth: usize,
        steps: &std::cell::Cell<usize>,
    ) -> bool {
        // THREAT[TM-DOS-031]: Bail on excessive recursion depth
        if depth >= Self::MAX_GLOB_DEPTH {
            return false;
        }

        match op {
            b'@' => {
                // @(a|b) — exactly one of the alternatives
                for alt in alts {
                    let full = format!("{}{}", alt, rest);
                    if self.glob_match_steps(value, &full, opts, depth + 1, steps) {
                        return true;
                    }
                }
                false
            }
            b'?' => {
                // ?(a|b) — zero or one of the alternatives
                // Try zero: skip the extglob entirely
                if self.glob_match_steps(value, rest, opts, depth + 1, steps) {
                    return true;
                }
                // Try one
                for alt in alts {
                    let full = format!("{}{}", alt, rest);
                    if self.glob_match_steps(value, &full, opts, depth + 1, steps) {
                        return true;
                    }
                }
                false
            }
            b'+' => {
                // +(a|b) — one or more of the alternatives
                let split_points = value
                    .char_indices()
                    .map(|(i, _)| i)
                    .skip(1)
                    .chain(std::iter::once(value.len()));
                for alt in alts {
                    let full = format!("{}{}", alt, rest);
                    if self.glob_match_steps(value, &full, opts, depth + 1, steps) {
                        return true;
                    }
                    // Try alt followed by more +(a|b)rest
                    // We need to try consuming `alt` prefix then matching +(...)rest again
                    for split in split_points.clone() {
                        let prefix = &value[..split];
                        let suffix = &value[split..];
                        if self.glob_match_steps(prefix, alt, opts, depth + 1, steps) {
                            // Rebuild the extglob for the suffix
                            let inner = alts.join("|");
                            let re_pattern = format!("+({}){}", inner, rest);
                            if self.glob_match_steps(suffix, &re_pattern, opts, depth + 1, steps) {
                                return true;
                            }
                        }
                    }
                }
                false
            }
            b'*' => {
                // *(a|b) — zero or more of the alternatives
                // Try zero
                if self.glob_match_steps(value, rest, opts, depth + 1, steps) {
                    return true;
                }
                let split_points = value
                    .char_indices()
                    .map(|(i, _)| i)
                    .skip(1)
                    .chain(std::iter::once(value.len()));
                // Try one or more (same as +(...))
                for alt in alts {
                    let full = format!("{}{}", alt, rest);
                    if self.glob_match_steps(value, &full, opts, depth + 1, steps) {
                        return true;
                    }
                    for split in split_points.clone() {
                        let prefix = &value[..split];
                        let suffix = &value[split..];
                        if self.glob_match_steps(prefix, alt, opts, depth + 1, steps) {
                            let inner = alts.join("|");
                            let re_pattern = format!("*({}){}", inner, rest);
                            if self.glob_match_steps(suffix, &re_pattern, opts, depth + 1, steps) {
                                return true;
                            }
                        }
                    }
                }
                false
            }
            b'!' => {
                // !(a|b) consumes some prefix (possibly empty) that matches
                // none of the alternatives, and `rest` matches what remains.
                value
                    .char_indices()
                    .map(|(i, _)| i)
                    .chain(std::iter::once(value.len()))
                    .any(|split| {
                        let (prefix, suffix) = value.split_at(split);
                        !alts
                            .iter()
                            .any(|a| self.glob_match_steps(prefix, a, opts, depth + 1, steps))
                            && self.glob_match_steps(suffix, rest, opts, depth + 1, steps)
                    })
            }
            _ => false,
        }
    }

    /// Match a bracket expression [abc], [a-z], [!abc], [^abc]
    /// Returns Some(true) if matched, Some(false) if not matched, None if invalid
    pub(crate) fn match_bracket_expr(
        &self,
        pattern_chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
        value_char: char,
        nocase: bool,
    ) -> Option<bool> {
        // Security: stream range/class checks; avoid unbounded range materialization.
        let mut matched = false;
        let mut saw_class_char = false;
        let mut prev_char: Option<char> = None;
        let mut negate = false;

        // Check for negation
        if matches!(pattern_chars.peek(), Some('!') | Some('^')) {
            negate = true;
            pattern_chars.next();
        }

        // Collect all characters in the bracket expression
        loop {
            match pattern_chars.next() {
                Some(']') if saw_class_char => break,
                Some(']') if !saw_class_char => {
                    // ] as first char is literal
                    saw_class_char = true;
                    prev_char = Some(']');
                    matched |= if nocase {
                        ']'.eq_ignore_ascii_case(&value_char)
                    } else {
                        value_char == ']'
                    };
                }
                Some('[') if matches!(pattern_chars.peek(), Some(':')) => {
                    // POSIX character class [:name:]
                    pattern_chars.next(); // consume ':'
                    let mut class_name = String::new();
                    loop {
                        match pattern_chars.next() {
                            Some(':') if matches!(pattern_chars.peek(), Some(']')) => {
                                pattern_chars.next(); // consume ']'
                                break;
                            }
                            Some(c) => class_name.push(c),
                            None => return None,
                        }
                    }
                    let mut class_chars = Vec::new();
                    expand_posix_class(&class_name, &mut class_chars);
                    if !class_chars.is_empty() {
                        saw_class_char = true;
                        prev_char = class_chars.last().copied();
                    }
                    matched |= if nocase {
                        let lc = value_char.to_ascii_lowercase();
                        class_chars.iter().any(|c| c.to_ascii_lowercase() == lc)
                    } else {
                        class_chars.contains(&value_char)
                    };
                }
                Some('-') if saw_class_char => {
                    // Could be a range
                    if let Some(&next) = pattern_chars.peek() {
                        if next == ']' {
                            // - at end is literal
                            saw_class_char = true;
                            prev_char = Some('-');
                            matched |= if nocase {
                                '-'.eq_ignore_ascii_case(&value_char)
                            } else {
                                value_char == '-'
                            };
                        } else {
                            // Range: prev-next
                            pattern_chars.next();
                            if let Some(prev) = prev_char {
                                let (start, end) = if prev <= next {
                                    (prev, next)
                                } else {
                                    (next, prev)
                                };
                                let probe = if nocase {
                                    value_char.to_ascii_lowercase()
                                } else {
                                    value_char
                                };
                                let (start_cmp, end_cmp) = if nocase {
                                    (start.to_ascii_lowercase(), end.to_ascii_lowercase())
                                } else {
                                    (start, end)
                                };
                                matched |= probe >= start_cmp && probe <= end_cmp;
                                saw_class_char = true;
                                prev_char = Some(next);
                            }
                        }
                    } else {
                        return None; // Unclosed bracket
                    }
                }
                Some(c) => {
                    saw_class_char = true;
                    prev_char = Some(c);
                    matched |= if nocase {
                        c.eq_ignore_ascii_case(&value_char)
                    } else {
                        c == value_char
                    };
                }
                None => return None, // Unclosed bracket
            }
        }
        Some(if negate { !matched } else { matched })
    }

    // ── Glob option helpers ───────────────────────────────────────────

    pub(crate) fn contains_glob_chars(&self, s: &str) -> bool {
        let mut escaped = false;
        for ch in s.chars() {
            if escaped {
                escaped = false;
                continue;
            }
            if ch == '\\' {
                escaped = true;
                continue;
            }
            if matches!(ch, '*' | '?' | '[') {
                return true;
            }
        }
        false
    }

    /// Check if dotglob shopt is enabled
    pub(crate) fn is_dotglob(&self) -> bool {
        self.scoped
            .variables
            .get("SHOPT_dotglob")
            .map(|v| v == "1")
            .unwrap_or(false)
    }

    /// Check if nocaseglob shopt is enabled
    pub(crate) fn is_nocaseglob(&self) -> bool {
        self.scoped
            .variables
            .get("SHOPT_nocaseglob")
            .map(|v| v == "1")
            .unwrap_or(false)
    }

    /// Check if noglob (set -f) is enabled
    pub(crate) fn is_noglob(&self) -> bool {
        self.scoped
            .variables
            .get("SHOPT_f")
            .map(|v| v == "1")
            .unwrap_or(false)
    }

    /// Check if failglob shopt is enabled
    pub(crate) fn is_failglob(&self) -> bool {
        self.scoped
            .variables
            .get("SHOPT_failglob")
            .map(|v| v == "1")
            .unwrap_or(false)
    }

    /// Check if globstar shopt is enabled
    pub(crate) fn is_globstar(&self) -> bool {
        self.scoped
            .variables
            .get("SHOPT_globstar")
            .map(|v| v == "1")
            .unwrap_or(false)
    }

    /// Check if extglob shopt is enabled
    pub(crate) fn is_extglob(&self) -> bool {
        self.scoped
            .variables
            .get("SHOPT_extglob")
            .map(|v| v == "1")
            .unwrap_or(false)
    }

    // ── Glob expansion against filesystem ─────────────────────────────

    /// Expand glob for a single item, applying noglob/failglob/nullglob.
    /// Returns Err(pattern) if failglob triggers, Ok(items) otherwise.
    /// Glob-expand one field. `escaped` marks a field from a word whose
    /// quoted glob characters the lexer backslash-escaped (quoted &&
    /// has_unquoted_glob): when the field is not replaced by matches, those
    /// escapes are removed so `"*"zz*` with no match prints `*zz*`.
    pub(crate) async fn expand_glob_item(
        &self,
        item: &str,
        escaped: bool,
    ) -> std::result::Result<Vec<String>, String> {
        let literal = |s: &str| {
            if escaped {
                Self::glob_path_unescape(s)
            } else {
                s.to_string()
            }
        };
        if !(self.contains_glob_chars(item) || self.contains_extglob(item)) || self.is_noglob() {
            return Ok(vec![literal(item)]);
        }
        let mut glob_matches = self.expand_glob(item).await.unwrap_or_default();
        self.apply_globignore(&mut glob_matches);
        if glob_matches.is_empty() {
            if self.is_failglob() {
                return Err(literal(item));
            }
            let nullglob = self
                .scoped
                .variables
                .get("SHOPT_nullglob")
                .map(|v| v == "1")
                .unwrap_or(false);
            if nullglob {
                Ok(vec![])
            } else {
                Ok(vec![literal(item)])
            }
        } else {
            Ok(glob_matches)
        }
    }

    /// Non-empty `GLOBIGNORE`, if set. Bash treats an empty value as unset.
    fn globignore(&self) -> Option<&str> {
        self.scoped
            .variables
            .get("GLOBIGNORE")
            .map(String::as_str)
            .filter(|v| !v.is_empty())
    }

    /// Drop matches named by `GLOBIGNORE` (colon-separated patterns), the way
    /// bash's `glob_name_is_acceptable` does: `.` and `..` always go, and each
    /// pattern matches the whole match string with `FNM_PATHNAME` (a `*`
    /// never crosses `/`), so `*.txt` keeps `foo/two.txt`. An emptied list
    /// then falls back to the no-match rules (literal word, nullglob, failglob).
    fn apply_globignore(&self, matches: &mut Vec<String>) {
        let Some(ignore) = self.globignore() else {
            return;
        };
        let opts = self.glob_opts();
        let patterns = Self::split_globignore(ignore);
        matches.retain(|name| {
            let last = name
                .trim_end_matches('/')
                .rsplit('/')
                .next()
                .unwrap_or(name);
            if last == "." || last == ".." {
                return false;
            }
            !patterns
                .iter()
                .any(|pat| self.glob_match_pathname(name, pat, opts))
        });
    }

    /// Split `GLOBIGNORE` on `:` outside bracket expressions and escapes,
    /// so `[[:alnum:]]*` stays one pattern (bash `split_ignorespec`).
    fn split_globignore(spec: &str) -> Vec<&str> {
        let bytes = spec.as_bytes();
        let mut patterns = Vec::new();
        let mut start = 0;
        let mut i = 0;
        while i < bytes.len() {
            match bytes[i] {
                b'\\' => i += 1,
                b'[' => {
                    // Find the closing `]`: a leading `!`/`^` and `]` are
                    // members, and `[:class:]` may hold a `]`-free name.
                    let mut j = i + 1;
                    if j < bytes.len() && matches!(bytes[j], b'!' | b'^') {
                        j += 1;
                    }
                    if j < bytes.len() && bytes[j] == b']' {
                        j += 1;
                    }
                    while j < bytes.len() && bytes[j] != b']' {
                        if bytes[j] == b'['
                            && bytes.get(j + 1) == Some(&b':')
                            && let Some(end) = spec[j + 2..].find(":]")
                        {
                            j += end + 4;
                            continue;
                        }
                        j += 1;
                    }
                    if j < bytes.len() {
                        i = j;
                    }
                }
                b':' => {
                    if i > start {
                        patterns.push(&spec[start..i]);
                    }
                    start = i + 1;
                }
                _ => {}
            }
            i += 1;
        }
        if start < spec.len() {
            patterns.push(&spec[start..]);
        }
        patterns
    }

    /// `fnmatch(FNM_PATHNAME)`: match component by component so wildcards
    /// stay inside one path segment.
    fn glob_match_pathname(&self, name: &str, pattern: &str, opts: PatternOpts) -> bool {
        let names: Vec<&str> = name.split('/').collect();
        let pats: Vec<&str> = pattern.split('/').collect();
        names.len() == pats.len()
            && names
                .iter()
                .zip(&pats)
                .all(|(n, p)| self.glob_match_impl(n, p, opts, 0))
    }

    /// Strip only parser-inserted glob metacharacter escapes from a path string.
    ///
    /// `quote_expansion_for_quoted_glob` inserts `\` before metacharacters in
    /// quoted segments to keep them literal while an adjacent unquoted glob suffix
    /// remains active.  Directory lookup must remove those synthetic escapes, but
    /// must not remove arbitrary `\X` pairs from expanded data: parameter and
    /// command substitution can produce literal backslashes after parsing, and
    /// turning `.\.` into `..` would change the lookup directory.
    ///
    /// The metacharacter set below must stay in sync with the escape set in
    /// `Interpreter::quote_expansion_for_quoted_glob` (interpreter/mod.rs); if
    /// one side adds or drops a character, lookups break or escapes leak.
    pub(super) fn glob_path_unescape(s: &str) -> String {
        let mut result = String::with_capacity(s.len());
        let mut chars = s.chars().peekable();
        while let Some(ch) = chars.next() {
            if ch == '\\'
                && let Some(&next) = chars.peek()
                && matches!(
                    next,
                    '\\' | '*'
                        | '?'
                        | '['
                        | ']'
                        | '{'
                        | '}'
                        | ','
                        | '@'
                        | '!'
                        | '+'
                        | '('
                        | ')'
                        | '|'
                )
            {
                result.push(next);
                chars.next();
                continue;
            }
            result.push(ch);
        }
        result
    }

    /// Join an output path component onto the accumulated output prefix.
    ///
    /// Output strings are built independently of the VFS lookup path so that
    /// caller-supplied prefixes (`./`, `../`) survive expansion verbatim while
    /// lookups use the normalized absolute path.
    fn glob_join_output(prefix: &str, name: &str, is_absolute: bool) -> String {
        if prefix.is_empty() {
            if is_absolute {
                format!("/{name}")
            } else {
                name.to_string()
            }
        } else {
            format!("{prefix}/{name}")
        }
    }

    /// Expand a glob pattern against the filesystem.
    ///
    /// Every path component is expanded, not just the trailing one: bash matches
    /// `/skills/*/SKILL.md` by globbing each component in turn against the VFS.
    /// Non-final components only match directories (a plain file can't be
    /// descended into), which is what makes read-only tree mounts globbable.
    /// A trailing `/` keeps only directories and stays on every match
    /// (`d/*/` gives `d/e/`). Under `shopt -s globstar` a `**` component
    /// matches zero or more directories, and files too when it is last.
    pub(crate) async fn expand_glob(&self, pattern: &str) -> Result<Vec<String>> {
        // A non-empty GLOBIGNORE turns dotglob on (bash `setup_glob_ignore`).
        let dotglob = self.is_dotglob() || self.globignore().is_some();
        let globstar = self.is_globstar();
        let opts = self.glob_opts();
        let is_absolute = pattern.starts_with('/');
        let trailing_slash = pattern.len() > 1 && pattern.ends_with('/');

        // Empty components collapse `//`; a trailing `/` is tracked above.
        let components: Vec<&str> = pattern.split('/').filter(|s| !s.is_empty()).collect();
        if components.is_empty() {
            return Ok(Vec::new());
        }

        // THREAT[TM-DOS-012]: a pattern deeper than the filesystem allows can
        // never match; refuse it instead of walking every component.
        let limits = self.fs.limits();
        if components.len() > limits.max_path_depth {
            return Ok(Vec::new());
        }
        // THREAT[TM-DOS-095]: each glob component multiplies the working set
        // (`/*/*/*/*` is a cross-product), so cap live candidates.
        let max_candidates = limits.max_file_count as usize;

        // (VFS lookup path, output path built from the caller's spelling)
        let mut candidates: Vec<(PathBuf, String)> = vec![(
            if is_absolute {
                PathBuf::from("/")
            } else {
                self.cwd.clone()
            },
            String::new(),
        )];

        let mut literal_prefix = true;
        for (idx, component) in components.iter().enumerate() {
            let is_last = idx + 1 == components.len();
            // Only directories can carry the rest of the pattern or a `/`.
            let dirs_only = !is_last || trailing_slash;
            let mut next: Vec<(PathBuf, String)> = Vec::new();

            if globstar && *component == "**" {
                for (dir, out) in &candidates {
                    // Zero directories: the candidate itself. A bare `**`
                    // never yields the empty cwd path; a literal `d/**`
                    // yields `d/`, a matched `*/**` yields plain `d`.
                    if !is_last {
                        next.push((dir.clone(), out.clone()));
                    } else if !out.is_empty() {
                        let own = if literal_prefix {
                            format!("{out}/")
                        } else {
                            out.clone()
                        };
                        next.push((dir.clone(), own));
                    }
                    self.globstar_walk(
                        dir,
                        out,
                        is_absolute,
                        dotglob,
                        dirs_only,
                        max_candidates,
                        &mut next,
                    )
                    .await;
                    if next.len() > max_candidates {
                        break;
                    }
                }
            } else if self.contains_glob_chars(component) || self.contains_extglob(component) {
                // Dotfiles are hidden per component unless dotglob is set or this
                // component explicitly starts with '.'.
                let component_starts_with_dot = component.starts_with('.');

                for (dir, out) in &candidates {
                    let entries = match self.fs.read_dir(dir).await {
                        Ok(entries) => entries,
                        Err(_) => continue,
                    };

                    let mut matched: Vec<String> = Vec::new();
                    for entry in entries {
                        if entry.name.starts_with('.') && !dotglob && !component_starts_with_dot {
                            continue;
                        }
                        if !self.glob_match_impl(&entry.name, component, opts, 0) {
                            continue;
                        }
                        // A symlink to a directory counts as one (bash's `*/`).
                        if dirs_only
                            && !entry.metadata.file_type.is_dir()
                            && !(entry.metadata.file_type.is_symlink()
                                && self
                                    .fs
                                    .stat(&vfs_join(dir, &entry.name))
                                    .await
                                    .is_ok_and(|m| m.file_type.is_dir()))
                        {
                            continue;
                        }
                        matched.push(entry.name);
                    }

                    // Looks redundant next to the final sort, but is not: `read_dir`
                    // order is unspecified (`InMemoryFs` iterates a `HashMap`), and
                    // sorting each level keeps the candidate set ordered so the
                    // TM-DOS-095 truncation below drops a deterministic tail.
                    matched.sort();
                    for name in matched {
                        let output = Self::glob_join_output(out, &name, is_absolute);
                        next.push((vfs_join(dir, &name), output));
                    }
                }
            } else {
                // Literal component: `\*` and friends are parser-inserted escapes
                // that must not reach the filesystem or the expanded word.
                let literal = Self::glob_path_unescape(component);
                for (dir, out) in &candidates {
                    let path = crate::fs::normalize_path(&vfs_join(dir, &literal));
                    // Intermediate literals are validated implicitly by the next
                    // `read_dir`; only the final component needs an existence check.
                    if is_last && !self.fs.exists(&path).await.unwrap_or(false) {
                        continue;
                    }
                    if is_last
                        && trailing_slash
                        && !self
                            .fs
                            .stat(&path)
                            .await
                            .is_ok_and(|m| m.file_type.is_dir())
                    {
                        continue;
                    }
                    let output = Self::glob_join_output(out, &literal, is_absolute);
                    next.push((path, output));
                }
            }

            if next.len() > max_candidates {
                next.truncate(max_candidates);
            }
            if next.is_empty() {
                return Ok(Vec::new());
            }
            candidates = next;
            literal_prefix &=
                !(self.contains_glob_chars(component) || self.contains_extglob(component));
        }

        // Sort matches alphabetically (bash behavior)
        let mut matches: Vec<String> = candidates
            .into_iter()
            .map(|(_, mut out)| {
                if trailing_slash && !out.ends_with('/') {
                    out.push('/');
                }
                out
            })
            .collect();
        matches.sort();
        matches.dedup();
        Ok(matches)
    }

    /// Every directory below `dir` (and every file too unless `dirs_only`),
    /// for a globstar `**` component. Hidden entries need `dotglob`.
    /// THREAT[TM-DOS-049]/[TM-DOS-095]: iterative, depth-capped by the
    /// filesystem path-depth limit, and stops at `max_candidates` results.
    #[allow(clippy::too_many_arguments)]
    async fn globstar_walk(
        &self,
        dir: &Path,
        out: &str,
        is_absolute: bool,
        dotglob: bool,
        dirs_only: bool,
        max_candidates: usize,
        next: &mut Vec<(PathBuf, String)>,
    ) {
        let max_depth = self.fs.limits().max_path_depth;
        let mut stack: Vec<(PathBuf, String, usize)> =
            vec![(dir.to_path_buf(), out.to_string(), 0)];
        while let Some((dir, out, depth)) = stack.pop() {
            if depth >= max_depth || next.len() > max_candidates {
                continue;
            }
            let Ok(mut entries) = self.fs.read_dir(&dir).await else {
                continue;
            };
            entries.sort_by(|a, b| b.name.cmp(&a.name));
            for entry in entries {
                if entry.name.starts_with('.') && !dotglob {
                    continue;
                }
                let is_dir = entry.metadata.file_type.is_dir();
                if !is_dir && dirs_only {
                    continue;
                }
                let path = vfs_join(&dir, &entry.name);
                let output = Self::glob_join_output(&out, &entry.name, is_absolute);
                next.push((path.clone(), output.clone()));
                if is_dir {
                    stack.push((path, output, depth + 1));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::fs::{FileSystem, InMemoryFs};

    use super::Interpreter;

    fn interp() -> Interpreter {
        let fs: Arc<dyn FileSystem> = Arc::new(InMemoryFs::new());
        Interpreter::new(fs)
    }

    #[test]
    fn unmatched_bracket_run_matches_literals() {
        let interp = interp();
        let brackets = "[".repeat(65_536);

        assert!(interp.pattern_matches(&brackets, &brackets));
        assert!(!interp.pattern_matches(&format!("{brackets}x"), &brackets));
    }

    #[test]
    fn invalid_and_valid_bracket_patterns_still_match() {
        let interp = interp();

        assert!(interp.pattern_matches("[]", "[]"));
        assert!(interp.pattern_matches("[", "["));
        assert!(interp.pattern_matches("a", "[a]"));
    }

    #[test]
    fn star_backtracking_restores_bracket_cache_state() {
        let interp = interp();

        assert!(!interp.pattern_matches("azX[a]z[", "*[a]z["));
    }

    // THREAT[TM-DOS-031]: a run of `*` must not cause exponential backtracking.
    // Before the linear restore-point rewrite, patterns like `****…%=` (found
    // by `glob_fuzz`) or `*a*a*…b` blew up to a libFuzzer timeout. These cases
    // finish instantly now; if the exponential path returns, the test hangs.
    #[test]
    fn many_stars_do_not_blow_up() {
        let interp = interp();

        // The reproducer minimized by `glob_fuzz`: a leading byte, a long run
        // of `*`, then a suffix the value never ends with → must fail fast.
        let pathological = format!("\u{1}{}%=", "*".repeat(1000));
        assert!(!interp.pattern_matches("hello.world", &pathological));
        assert!(!interp.pattern_matches("test.txt", &pathological));

        // Consecutive `*` collapse to a single wildcard (matches anything).
        assert!(interp.pattern_matches("anything at all", &"*".repeat(1000)));
        assert!(interp.pattern_matches("test.txt", &format!("{}txt", "*".repeat(500))));

        // Separated stars are the other classic exponential shape.
        let separated = format!("{}b", "a*".repeat(500));
        assert!(!interp.pattern_matches(&"a".repeat(2000), &separated));
        assert!(interp.pattern_matches(&format!("{}b", "a".repeat(2000)), &separated));
    }

    // Ordinary `*` / `?` / literal semantics are unchanged by the rewrite.
    #[test]
    fn star_matching_semantics_preserved() {
        let interp = interp();

        assert!(interp.pattern_matches("test.txt", "*.txt"));
        assert!(!interp.pattern_matches("test.md", "*.txt"));
        assert!(interp.pattern_matches("axxbyyc", "a*b*c"));
        assert!(!interp.pattern_matches("axxbyy", "a*b*c"));
        assert!(interp.pattern_matches("abcabc", "*abc"));
        assert!(interp.pattern_matches("abc", "a?c"));
        assert!(!interp.pattern_matches("ac", "a?c"));
        assert!(interp.pattern_matches("hello", "he*"));
        assert!(interp.pattern_matches("hello", "*llo"));
        assert!(interp.pattern_matches("hello", "*"));
    }
}
