//! POSIX regex translation and matching for `sed`.
//!
//! Important decisions (issue #2427, finding B):
//!
//! - BRE and ERE are translated into the `regex` crate dialect by a single
//!   [`translate`] pass used for **both** `s///` patterns and address regexes.
//!   Previously address regexes skipped conversion entirely and were compiled
//!   as ERE, so `/a\+/` never matched while `s/a\+//` did.
//! - Whether `+ ? | * ^ $` are operators or literals is positional in BRE, so
//!   the translator tracks "start of a branch" state rather than doing string
//!   replacements. `s/a+b/`, `s/a?b/`, `s/a|b/`, `/a^b/` and `s/*a/` are all
//!   literal matches in BRE, exactly as GNU sed treats them.
//! - Bracket expressions are copied by a dedicated scanner: inside `[...]`
//!   POSIX gives `\` no special meaning, while the `regex` crate treats `\`,
//!   `[`, `&&`, `~~` and `--` as syntax, so those are escaped on the way out.

use regex::{Regex, RegexBuilder};

use crate::builtins::search_common::{REGEX_DFA_SIZE_LIMIT, REGEX_SIZE_LIMIT};

/// Translate a POSIX BRE (or ERE, when `extended`) into `regex` crate syntax.
pub(super) fn translate(pattern: &str, extended: bool) -> String {
    let chars: Vec<char> = pattern.chars().collect();
    let mut out = String::with_capacity(pattern.len() + 8);
    let mut i = 0;
    // True when the next token begins an alternation branch: `^` anchors there
    // and (BRE only) a `*` there is an ordinary character.
    let mut branch_start = true;

    while i < chars.len() {
        let ch = chars[i];
        match ch {
            '\\' if i + 1 < chars.len() => {
                let next = chars[i + 1];
                i += 2;
                match next {
                    // In BRE the backslashed forms are the operators.
                    '(' | ')' | '{' | '}' | '+' | '?' | '|' if !extended => {
                        out.push(next);
                        branch_start = matches!(next, '(' | '|');
                    }
                    // Elsewhere a backslashed punctuation character is literal.
                    '(' | ')' | '{' | '}' | '+' | '?' | '|' | '.' | '*' | '[' | ']' | '^' | '$'
                    | '\\' | '/' => {
                        push_literal(&mut out, next);
                        branch_start = false;
                    }
                    'n' => push_raw(&mut out, '\n', &mut branch_start),
                    't' => push_raw(&mut out, '\t', &mut branch_start),
                    'r' => push_raw(&mut out, '\r', &mut branch_start),
                    'f' => push_raw(&mut out, '\x0c', &mut branch_start),
                    'v' => push_raw(&mut out, '\x0b', &mut branch_start),
                    'a' => push_raw(&mut out, '\x07', &mut branch_start),
                    // GNU character-class and word-boundary escapes.
                    'w' | 'W' | 's' | 'S' | 'b' | 'B' => {
                        out.push('\\');
                        out.push(next);
                        branch_start = false;
                    }
                    // GNU word-edge operators; the `regex` crate has no
                    // look-around, so both collapse onto `\b`.
                    '<' | '>' => {
                        out.push_str("\\b");
                        branch_start = false;
                    }
                    '`' => {
                        out.push_str("\\A");
                        branch_start = false;
                    }
                    '\'' => {
                        out.push_str("\\z");
                        branch_start = false;
                    }
                    // Back-references: passed through for the fancy-regex path.
                    '1'..='9' => {
                        out.push('\\');
                        out.push(next);
                        branch_start = false;
                    }
                    _ => {
                        push_literal(&mut out, next);
                        branch_start = false;
                    }
                }
            }
            // Trailing lone backslash.
            '\\' => {
                out.push_str("\\\\");
                i += 1;
                branch_start = false;
            }
            '[' => {
                i = copy_bracket(&chars, i, &mut out);
                branch_start = false;
            }
            '^' => {
                if extended || branch_start {
                    out.push('^');
                } else {
                    out.push_str("\\^");
                }
                i += 1;
                // `^*` keeps `*` literal in BRE.
                branch_start = !extended;
            }
            '$' => {
                if extended || is_bre_dollar_anchor(&chars, i) {
                    out.push('$');
                } else {
                    out.push_str("\\$");
                }
                i += 1;
                branch_start = false;
            }
            '*' => {
                if !extended && branch_start {
                    out.push_str("\\*");
                } else {
                    out.push('*');
                }
                i += 1;
                branch_start = false;
            }
            '+' | '?' | '|' | '(' | ')' | '{' | '}' if !extended => {
                push_literal(&mut out, ch);
                i += 1;
                branch_start = false;
            }
            '(' | '|' if extended => {
                out.push(ch);
                i += 1;
                branch_start = true;
            }
            _ => {
                out.push(ch);
                i += 1;
                branch_start = false;
            }
        }
    }

    out
}

fn push_raw(out: &mut String, ch: char, branch_start: &mut bool) {
    push_literal(out, ch);
    *branch_start = false;
}

/// Emit `ch` so the `regex` crate reads it as an ordinary character.
fn push_literal(out: &mut String, ch: char) {
    if ch.is_ascii_alphanumeric() || !ch.is_ascii() {
        out.push(ch);
    } else {
        out.push('\\');
        out.push(ch);
    }
}

/// In BRE, `$` anchors only at the very end of the expression or immediately
/// before `\)` / `\|`; anywhere else it is an ordinary character.
fn is_bre_dollar_anchor(chars: &[char], i: usize) -> bool {
    match chars.get(i + 1) {
        None => true,
        Some('\\') => matches!(chars.get(i + 2), Some(')') | Some('|')),
        Some(_) => false,
    }
}

/// Copy a POSIX bracket expression starting at `chars[start] == '['`.
/// Returns the index just past the closing `]`.
fn copy_bracket(chars: &[char], start: usize, out: &mut String) -> usize {
    let mut i = start + 1;
    let negated = matches!(chars.get(i), Some('^'));
    if negated {
        i += 1;
    }

    let mut content = String::new();
    let mut seen = 0usize;
    let mut closed = false;

    while i < chars.len() {
        let ch = chars[i];
        if ch == ']' && seen > 0 {
            closed = true;
            i += 1;
            break;
        }
        if ch == '['
            && let Some(&kind @ (':' | '.' | '=')) = chars.get(i + 1)
            && let Some(end) = find_class_end(chars, i + 2, kind)
        {
            if kind == ':' {
                // [:alpha:] and friends are understood by the regex crate.
                content.extend(&chars[i..=end + 1]);
            } else {
                // [.x.] / [=x=] degrade to the literal characters inside.
                for &c in &chars[i + 2..end] {
                    push_class_literal(&mut content, c);
                }
            }
            i = end + 2;
            seen += 1;
            continue;
        }
        push_class_literal(&mut content, ch);
        i += 1;
        seen += 1;
    }

    if !closed {
        // Unterminated: hand the raw text to the regex crate so it reports the
        // error instead of silently matching something else.
        out.push('[');
        if negated {
            out.push('^');
        }
        out.push_str(&content);
        return chars.len();
    }

    out.push('[');
    if negated {
        out.push('^');
    }
    out.push_str(&content);
    out.push(']');
    i
}

fn find_class_end(chars: &[char], from: usize, kind: char) -> Option<usize> {
    let mut j = from;
    while j + 1 < chars.len() {
        if chars[j] == kind && chars[j + 1] == ']' {
            return Some(j);
        }
        j += 1;
    }
    None
}

/// Escape the characters the `regex` crate treats as class syntax but POSIX
/// treats as ordinary members of a bracket expression.
fn push_class_literal(out: &mut String, ch: char) {
    match ch {
        '\\' | '[' | ']' | '&' | '~' => {
            out.push('\\');
            out.push(ch);
        }
        _ => out.push(ch),
    }
}

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
        let translated = translate(pattern, extended);
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
