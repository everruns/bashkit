//! Glob pattern matching and expansion for the interpreter.
//!
//! Extracted from mod.rs to reduce the god-module size.
//! Contains: pattern matching (glob, extglob, bracket expressions),
//! glob expansion against the filesystem, and related option helpers.

use std::path::{Path, PathBuf};

use crate::error::Result;

use super::Interpreter;

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

impl Interpreter {
    // ── Pattern matching ──────────────────────────────────────────────

    /// Check if pattern contains extglob operators
    pub(crate) fn contains_extglob(&self, s: &str) -> bool {
        if !self.is_extglob() {
            return false;
        }
        let bytes = s.as_bytes();
        for i in 0..bytes.len().saturating_sub(1) {
            if matches!(bytes[i], b'@' | b'?' | b'*' | b'+' | b'!') && bytes[i + 1] == b'(' {
                return true;
            }
        }
        false
    }

    /// Check if a value matches a shell pattern
    pub(crate) fn pattern_matches(&self, value: &str, pattern: &str) -> bool {
        // Handle special case of * (match anything)
        if pattern == "*" {
            return true;
        }

        // Glob pattern matching with *, ?, [], and extglob support
        if pattern.contains('*')
            || pattern.contains('?')
            || pattern.contains('[')
            || self.contains_extglob(pattern)
        {
            self.glob_match(value, pattern)
        } else {
            // Literal match
            value == pattern
        }
    }

    /// Simple glob pattern matching with support for *, ?, and [...]
    pub(crate) fn glob_match(&self, value: &str, pattern: &str) -> bool {
        self.glob_match_impl(value, pattern, false, 0)
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
                        for c in inner.chars() {
                            match c {
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

    /// Glob match with optional case-insensitive mode
    pub(crate) fn glob_match_impl(
        &self,
        value: &str,
        pattern: &str,
        nocase: bool,
        depth: usize,
    ) -> bool {
        // THREAT[TM-DOS-031]: Bail on excessive recursion depth
        if depth >= Self::MAX_GLOB_DEPTH {
            return false;
        }

        let extglob = self.is_extglob();

        // Check for extglob at the start of pattern
        if extglob && pattern.len() >= 2 {
            let bytes = pattern.as_bytes();
            if matches!(bytes[0], b'@' | b'?' | b'*' | b'+' | b'!') && bytes[1] == b'(' {
                let op = bytes[0];
                if let Some((alts, rest)) = Self::parse_extglob_pattern_list(&pattern[2..]) {
                    return self.match_extglob(op, &alts, &rest, value, nocase, depth + 1);
                }
            }
        }

        let mut value_chars = value.chars().peekable();
        let mut pattern_chars = pattern.chars().peekable();

        loop {
            match (pattern_chars.peek().copied(), value_chars.peek().copied()) {
                (None, None) => return true,
                (None, Some(_)) => return false,
                (Some('*'), _) => {
                    // Check for extglob *(...)
                    let mut pc_clone = pattern_chars.clone();
                    pc_clone.next();
                    if extglob && pc_clone.peek() == Some(&'(') {
                        // Extglob *(pattern-list) — collect remaining pattern
                        let remaining_pattern: String = pattern_chars.collect();
                        let remaining_value: String = value_chars.collect();
                        return self.glob_match_impl(
                            &remaining_value,
                            &remaining_pattern,
                            nocase,
                            depth + 1,
                        );
                    }
                    pattern_chars.next();
                    // * matches zero or more characters
                    if pattern_chars.peek().is_none() {
                        return true; // * at end matches everything
                    }
                    // Try matching from each position
                    while value_chars.peek().is_some() {
                        let remaining_value: String = value_chars.clone().collect();
                        let remaining_pattern: String = pattern_chars.clone().collect();
                        if self.glob_match_impl(
                            &remaining_value,
                            &remaining_pattern,
                            nocase,
                            depth + 1,
                        ) {
                            return true;
                        }
                        value_chars.next();
                    }
                    // Also try with empty match
                    let remaining_pattern: String = pattern_chars.collect();
                    return self.glob_match_impl("", &remaining_pattern, nocase, depth + 1);
                }
                (Some('?'), _) => {
                    // Check for extglob ?(...)
                    let mut pc_clone = pattern_chars.clone();
                    pc_clone.next();
                    if extglob && pc_clone.peek() == Some(&'(') {
                        let remaining_pattern: String = pattern_chars.collect();
                        let remaining_value: String = value_chars.collect();
                        return self.glob_match_impl(
                            &remaining_value,
                            &remaining_pattern,
                            nocase,
                            depth + 1,
                        );
                    }
                    if value_chars.peek().is_some() {
                        pattern_chars.next();
                        value_chars.next();
                    } else {
                        return false;
                    }
                }
                (Some('['), Some(v)) => {
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
                            return false;
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
                            return false;
                        }
                    }
                }
                (Some('['), None) => return false,
                (Some(p), Some(v)) => {
                    // Check for extglob operators: @(, +(, !(
                    if extglob && matches!(p, '@' | '+' | '!') {
                        let mut pc_clone = pattern_chars.clone();
                        pc_clone.next();
                        if pc_clone.peek() == Some(&'(') {
                            let remaining_pattern: String = pattern_chars.collect();
                            let remaining_value: String = value_chars.collect();
                            return self.glob_match_impl(
                                &remaining_value,
                                &remaining_pattern,
                                nocase,
                                depth + 1,
                            );
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
                        return false;
                    }
                }
                (Some(_), None) => return false,
            }
        }
    }

    /// Match an extglob pattern against a value.
    /// op: b'@', b'?', b'*', b'+', b'!'
    /// alts: the | separated alternatives
    /// rest: pattern after the closing )
    fn match_extglob(
        &self,
        op: u8,
        alts: &[String],
        rest: &str,
        value: &str,
        nocase: bool,
        depth: usize,
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
                    if self.glob_match_impl(value, &full, nocase, depth + 1) {
                        return true;
                    }
                }
                false
            }
            b'?' => {
                // ?(a|b) — zero or one of the alternatives
                // Try zero: skip the extglob entirely
                if self.glob_match_impl(value, rest, nocase, depth + 1) {
                    return true;
                }
                // Try one
                for alt in alts {
                    let full = format!("{}{}", alt, rest);
                    if self.glob_match_impl(value, &full, nocase, depth + 1) {
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
                    if self.glob_match_impl(value, &full, nocase, depth + 1) {
                        return true;
                    }
                    // Try alt followed by more +(a|b)rest
                    // We need to try consuming `alt` prefix then matching +(...)rest again
                    for split in split_points.clone() {
                        let prefix = &value[..split];
                        let suffix = &value[split..];
                        if self.glob_match_impl(prefix, alt, nocase, depth + 1) {
                            // Rebuild the extglob for the suffix
                            let inner = alts.join("|");
                            let re_pattern = format!("+({}){}", inner, rest);
                            if self.glob_match_impl(suffix, &re_pattern, nocase, depth + 1) {
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
                if self.glob_match_impl(value, rest, nocase, depth + 1) {
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
                    if self.glob_match_impl(value, &full, nocase, depth + 1) {
                        return true;
                    }
                    for split in split_points.clone() {
                        let prefix = &value[..split];
                        let suffix = &value[split..];
                        if self.glob_match_impl(prefix, alt, nocase, depth + 1) {
                            let inner = alts.join("|");
                            let re_pattern = format!("*({}){}", inner, rest);
                            if self.glob_match_impl(suffix, &re_pattern, nocase, depth + 1) {
                                return true;
                            }
                        }
                    }
                }
                false
            }
            b'!' => {
                // !(a|b) — match anything except one of the alternatives
                // Try every possible split point: prefix must NOT match any alt, rest matches
                // Actually: !(pat) matches anything that doesn't match @(pat)
                let inner = alts.join("|");
                let positive = format!("@({}){}", inner, rest);
                !self.glob_match_impl(value, &positive, nocase, depth + 1)
                    && self.glob_match_impl(value, rest, nocase, depth + 1)
                    || {
                        // !(pat) can also consume characters — try each split
                        for split in value
                            .char_indices()
                            .map(|(i, _)| i)
                            .skip(1)
                            .chain(std::iter::once(value.len()))
                        {
                            let prefix = &value[..split];
                            let suffix = &value[split..];
                            // prefix must not match any alt
                            let prefix_matches_any = alts
                                .iter()
                                .any(|a| self.glob_match_impl(prefix, a, nocase, depth + 1));
                            if !prefix_matches_any
                                && self.glob_match_impl(suffix, rest, nocase, depth + 1)
                            {
                                return true;
                            }
                        }
                        false
                    }
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
        s.contains('*') || s.contains('?') || s.contains('[')
    }

    /// Check if dotglob shopt is enabled
    pub(crate) fn is_dotglob(&self) -> bool {
        self.variables
            .get("SHOPT_dotglob")
            .map(|v| v == "1")
            .unwrap_or(false)
    }

    /// Check if nocaseglob shopt is enabled
    pub(crate) fn is_nocaseglob(&self) -> bool {
        self.variables
            .get("SHOPT_nocaseglob")
            .map(|v| v == "1")
            .unwrap_or(false)
    }

    /// Check if noglob (set -f) is enabled
    pub(crate) fn is_noglob(&self) -> bool {
        self.variables
            .get("SHOPT_f")
            .map(|v| v == "1")
            .unwrap_or(false)
    }

    /// Check if failglob shopt is enabled
    pub(crate) fn is_failglob(&self) -> bool {
        self.variables
            .get("SHOPT_failglob")
            .map(|v| v == "1")
            .unwrap_or(false)
    }

    /// Check if globstar shopt is enabled
    pub(crate) fn is_globstar(&self) -> bool {
        self.variables
            .get("SHOPT_globstar")
            .map(|v| v == "1")
            .unwrap_or(false)
    }

    /// Check if extglob shopt is enabled
    pub(crate) fn is_extglob(&self) -> bool {
        self.variables
            .get("SHOPT_extglob")
            .map(|v| v == "1")
            .unwrap_or(false)
    }

    // ── Glob expansion against filesystem ─────────────────────────────

    /// Expand glob for a single item, applying noglob/failglob/nullglob.
    /// Returns Err(pattern) if failglob triggers, Ok(items) otherwise.
    pub(crate) async fn expand_glob_item(
        &self,
        item: &str,
    ) -> std::result::Result<Vec<String>, String> {
        if !self.contains_glob_chars(item) || self.is_noglob() {
            return Ok(vec![item.to_string()]);
        }
        let glob_matches = self.expand_glob(item).await.unwrap_or_default();
        if glob_matches.is_empty() {
            if self.is_failglob() {
                return Err(item.to_string());
            }
            let nullglob = self
                .variables
                .get("SHOPT_nullglob")
                .map(|v| v == "1")
                .unwrap_or(false);
            if nullglob {
                Ok(vec![])
            } else {
                Ok(vec![item.to_string()])
            }
        } else {
            Ok(glob_matches)
        }
    }

    /// Expand a glob pattern against the filesystem
    pub(crate) async fn expand_glob(&self, pattern: &str) -> Result<Vec<String>> {
        // Check for ** (recursive glob) — only when globstar is enabled
        if pattern.contains("**") && self.is_globstar() {
            return self.expand_glob_recursive(pattern).await;
        }

        let mut matches = Vec::new();
        let dotglob = self.is_dotglob();
        let nocase = self.is_nocaseglob();

        // Split pattern into directory and filename parts
        let path = Path::new(pattern);
        let (dir, file_pattern) = if path.is_absolute() {
            let parent = path.parent().unwrap_or(Path::new("/"));
            let name = path.file_name().map(|s| s.to_string_lossy().to_string());
            (parent.to_path_buf(), name)
        } else {
            // Relative path - use cwd
            let parent = path.parent();
            let name = path.file_name().map(|s| s.to_string_lossy().to_string());
            if let Some(p) = parent {
                if p.as_os_str().is_empty() {
                    (self.cwd.clone(), name)
                } else {
                    (crate::fs::normalize_path(&self.cwd.join(p)), name)
                }
            } else {
                (self.cwd.clone(), name)
            }
        };

        let file_pattern = match file_pattern {
            Some(p) => p,
            None => return Ok(matches),
        };

        // Check if the directory exists
        if !self.fs.exists(&dir).await.unwrap_or(false) {
            return Ok(matches);
        }

        // Read directory entries
        let entries = match self.fs.read_dir(&dir).await {
            Ok(e) => e,
            Err(_) => return Ok(matches),
        };

        // Check if pattern explicitly starts with dot
        let pattern_starts_with_dot = file_pattern.starts_with('.');

        // Match each entry against the pattern
        for entry in entries {
            // Skip dotfiles unless dotglob is set or pattern explicitly starts with '.'
            if entry.name.starts_with('.') && !dotglob && !pattern_starts_with_dot {
                continue;
            }

            if self.glob_match_impl(&entry.name, &file_pattern, nocase, 0) {
                // Construct the full path
                let full_path = if path.is_absolute() {
                    dir.join(&entry.name).to_string_lossy().to_string()
                } else {
                    // For relative patterns, return relative path
                    if let Some(parent) = path.parent() {
                        if parent.as_os_str().is_empty() {
                            entry.name.clone()
                        } else {
                            format!("{}/{}", parent.to_string_lossy(), entry.name)
                        }
                    } else {
                        entry.name.clone()
                    }
                };
                matches.push(full_path);
            }
        }

        // Sort matches alphabetically (bash behavior)
        matches.sort();
        Ok(matches)
    }

    /// Expand a glob pattern containing ** (recursive directory matching).
    async fn expand_glob_recursive(&self, pattern: &str) -> Result<Vec<String>> {
        let is_absolute = pattern.starts_with('/');
        let components: Vec<&str> = pattern.split('/').filter(|s| !s.is_empty()).collect();
        let dotglob = self.is_dotglob();
        let nocase = self.is_nocaseglob();

        // Find the ** component
        let star_star_idx = match components.iter().position(|&c| c == "**") {
            Some(i) => i,
            None => return Ok(Vec::new()),
        };

        // Build the base directory from components before **
        let base_dir = if is_absolute {
            let mut p = PathBuf::from("/");
            for c in &components[..star_star_idx] {
                p.push(c);
            }
            p
        } else {
            let mut p = self.cwd.clone();
            for c in &components[..star_star_idx] {
                p.push(c);
            }
            p
        };

        // Pattern components after **
        let after_pattern: Vec<&str> = components[star_star_idx + 1..].to_vec();

        // Collect all directories recursively (including the base)
        let mut all_dirs = vec![base_dir.clone()];
        // THREAT[TM-DOS-049]: Cap recursion depth using filesystem path depth limit
        let max_depth = self.fs.limits().max_path_depth;
        self.collect_dirs_recursive(&base_dir, &mut all_dirs, max_depth, dotglob)
            .await;

        let mut matches = Vec::new();

        for dir in &all_dirs {
            if after_pattern.is_empty() {
                // ** alone matches all files recursively
                if let Ok(entries) = self.fs.read_dir(dir).await {
                    for entry in entries {
                        if entry.name.starts_with('.') && !dotglob {
                            continue;
                        }
                        if !entry.metadata.file_type.is_dir() {
                            matches.push(dir.join(&entry.name).to_string_lossy().to_string());
                        }
                    }
                }
            } else if after_pattern.len() == 1 {
                // Single pattern after **: match files in this directory
                let pat = after_pattern[0];
                let pattern_starts_with_dot = pat.starts_with('.');
                if let Ok(entries) = self.fs.read_dir(dir).await {
                    for entry in entries {
                        if entry.name.starts_with('.') && !dotglob && !pattern_starts_with_dot {
                            continue;
                        }
                        if self.glob_match_impl(&entry.name, pat, nocase, 0) {
                            matches.push(dir.join(&entry.name).to_string_lossy().to_string());
                        }
                    }
                }
            }
        }

        matches.sort();
        Ok(matches)
    }

    /// Recursively collect all subdirectories starting from dir.
    /// THREAT[TM-DOS-049]: `max_depth` caps recursion to prevent stack exhaustion.
    pub(crate) fn collect_dirs_recursive<'a>(
        &'a self,
        dir: &'a Path,
        result: &'a mut Vec<PathBuf>,
        max_depth: usize,
        dotglob: bool,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + 'a>> {
        Box::pin(async move {
            if max_depth == 0 {
                return;
            }
            if let Ok(entries) = self.fs.read_dir(dir).await {
                for entry in entries {
                    if entry.metadata.file_type.is_dir() {
                        if entry.name.starts_with('.') && !dotglob {
                            continue;
                        }
                        let subdir = dir.join(&entry.name);
                        result.push(subdir.clone());
                        self.collect_dirs_recursive(&subdir, result, max_depth - 1, dotglob)
                            .await;
                    }
                }
            }
        })
    }
}
