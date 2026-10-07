//! POSIX ERE (gawk dialect) on top of the `regex` crate, with POSIX
//! leftmost-longest match semantics.
//!
//! Decisions:
//! - Patterns are translated, not passed through: `.` matches newline,
//!   `\y`/`\<`/`\>` are word boundaries, `{` that does not start a valid
//!   interval is literal, a leading `*`/`+`/`?` is literal, bracket
//!   expressions follow POSIX (`]` first is literal, backslash escapes,
//!   `[:class:]`), unknown escapes like `\d` are the literal letter (gawk).
//! - Leftmost-longest: the `regex` crate is leftmost-first. Both agree on
//!   where the leftmost match starts; from that start an anchored lazy DFA
//!   with `MatchKind::All` finds the longest end. Patterns whose
//!   leftmost-first and leftmost-longest results cannot differ (no
//!   alternation, no quantifier followed by more pattern) skip the DFA.
//! - THREAT[TM-DOS-023]: both engines are built with the shared regex size
//!   limits; compiled regexes (and failures) are cached per run with a
//!   bounded entry count and pattern bytes.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, OnceLock};

use regex::Regex;
use regex_automata::hybrid::dfa::{Cache as DfaCache, DFA};
use regex_automata::nfa::thompson;
use regex_automata::{Anchored, Input, MatchKind};

use crate::builtins::limits::RUNTIME_REGEX_CACHE_ENTRIES;
use crate::builtins::search_common::{REGEX_DFA_SIZE_LIMIT, REGEX_SIZE_LIMIT};

pub(crate) struct AwkRegex {
    re: Regex,
    longest: Option<(DFA, Mutex<DfaCache>)>,
    /// Translated pattern, for the whole-span capture fallback.
    rust: String,
    full: OnceLock<Option<Regex>>,
}

impl std::fmt::Debug for AwkRegex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.re.as_str())
    }
}

/// Compile failure, as a short user-facing message.
pub(crate) struct RegexError(pub(crate) String);

impl AwkRegex {
    pub(crate) fn new(ere: &str, icase: bool) -> Result<AwkRegex, RegexError> {
        let t = translate(ere);
        let mut pat = String::with_capacity(t.pattern.len() + 8);
        pat.push_str(if icase { "(?si)" } else { "(?s)" });
        pat.push_str(&t.pattern);
        let re = regex::RegexBuilder::new(&pat)
            .size_limit(REGEX_SIZE_LIMIT)
            .dfa_size_limit(REGEX_DFA_SIZE_LIMIT)
            .build()
            .map_err(|e| RegexError(short_error(&e)))?;
        let longest = if t.needs_longest {
            DFA::builder()
                .configure(
                    DFA::config()
                        .match_kind(MatchKind::All)
                        .cache_capacity(REGEX_DFA_SIZE_LIMIT)
                        .unicode_word_boundary(true),
                )
                .thompson(thompson::Config::new().nfa_size_limit(Some(REGEX_SIZE_LIMIT)))
                .build(&pat)
                .ok()
                .map(|dfa| {
                    let cache = dfa.create_cache();
                    (dfa, Mutex::new(cache))
                })
        } else {
            None
        };
        Ok(AwkRegex {
            re,
            longest,
            rust: pat,
            full: OnceLock::new(),
        })
    }

    pub(crate) fn is_match(&self, s: &str) -> bool {
        self.re.is_match(s)
    }

    /// Leftmost-longest match at or after byte `start`.
    pub(crate) fn find_at(&self, s: &str, start: usize) -> Option<(usize, usize)> {
        let m = self.re.find_at(s, start)?;
        Some((m.start(), self.longest_end(s, m.start(), m.end())))
    }

    fn longest_end(&self, s: &str, start: usize, end: usize) -> usize {
        let Some((dfa, cache)) = &self.longest else {
            return end;
        };
        if end == s.len() {
            return end;
        }
        let Ok(mut cache) = cache.lock() else {
            return end;
        };
        let input = Input::new(s).range(start..).anchored(Anchored::Yes);
        match dfa.try_search_fwd(&mut cache, &input) {
            Ok(Some(hm)) if hm.offset() > end && s.is_char_boundary(hm.offset()) => hm.offset(),
            _ => end,
        }
    }

    /// Leftmost-longest match with capture group spans (group 0 first).
    pub(crate) fn captures_at(&self, s: &str, start: usize) -> Option<Vec<Option<(usize, usize)>>> {
        let caps = self.re.captures_at(s, start)?;
        let m0 = caps.get(0)?;
        let end = self.longest_end(s, m0.start(), m0.end());
        if end == m0.end() {
            return Some(
                caps.iter()
                    .map(|g| g.map(|m| (m.start(), m.end())))
                    .collect(),
            );
        }
        // The longest match is not the leftmost-first one: take groups from
        // a whole-span match of the same pattern on that span.
        let begin = m0.start();
        let full = self.full.get_or_init(|| {
            regex::RegexBuilder::new(&format!("^(?:{})$", self.rust))
                .size_limit(REGEX_SIZE_LIMIT)
                .dfa_size_limit(REGEX_DFA_SIZE_LIMIT)
                .build()
                .ok()
        });
        let sub = &s[begin..end];
        if let Some(full) = full
            && let Some(c) = full.captures(sub)
        {
            return Some(
                c.iter()
                    .map(|g| g.map(|m| (begin + m.start(), begin + m.end())))
                    .collect(),
            );
        }
        let mut v: Vec<Option<(usize, usize)>> = vec![None; caps.len()];
        v[0] = Some((begin, end));
        Some(v)
    }
}

fn short_error(e: &regex::Error) -> String {
    match e {
        regex::Error::CompiledTooBig(_) => "regular expression too big".to_string(),
        _ => {
            let s = e.to_string();
            // The last line of the regex crate's message is the summary.
            let line = s
                .lines()
                .rev()
                .find(|l| !l.trim().is_empty())
                .unwrap_or("invalid regexp");
            let line = line.trim().trim_start_matches("error: ");
            line.chars().take(200).collect()
        }
    }
}

struct Translated {
    pattern: String,
    needs_longest: bool,
}

fn regex_syntax_is_meta(c: char) -> bool {
    matches!(
        c,
        '\\' | '.'
            | '+'
            | '*'
            | '?'
            | '('
            | ')'
            | '|'
            | '['
            | ']'
            | '{'
            | '}'
            | '^'
            | '$'
            | '#'
            | '&'
            | '-'
            | '~'
    )
}

/// Translate a POSIX/gawk ERE to `regex` crate syntax.
fn translate(ere: &str) -> Translated {
    let chars: Vec<char> = ere.chars().collect();
    let mut out = String::with_capacity(ere.len() + 8);
    let mut i = 0;
    // A quantifier here would have nothing to repeat (start, after `(`,
    // `|`, `^`): POSIX/gawk treat it as literal.
    let mut at_start = true;
    let mut has_alt = false;
    let mut quantified = false;
    let mut needs_longest = false;
    let mut depth = 0usize;
    // Escape only regex metacharacters: `\<`, `\>` and friends are
    // assertions in the regex crate, not literals.
    let push_lit = |out: &mut String, c: char| {
        if regex_syntax_is_meta(c) {
            out.push('\\');
        }
        out.push(c);
    };
    while i < chars.len() {
        let c = chars[i];
        // An atom after a quantifier means the match length can trade off.
        let is_quant = matches!(c, '*' | '+' | '?' | '{');
        if quantified && !is_quant && c != ')' && c != '|' && c != '$' {
            needs_longest = true;
        }
        match c {
            '\\' => {
                i += 1;
                let Some(&e) = chars.get(i) else {
                    out.push_str("\\\\");
                    break;
                };
                match e {
                    'y' => out.push_str("\\b"),
                    'B' => out.push_str("\\B"),
                    '<' => out.push_str("\\b{start}"),
                    '>' => out.push_str("\\b{end}"),
                    '`' => out.push_str("\\A"),
                    '\'' => out.push_str("\\z"),
                    's' | 'S' | 'w' | 'W' => {
                        out.push('\\');
                        out.push(e);
                    }
                    'n' => out.push_str("\\n"),
                    't' => out.push_str("\\t"),
                    'r' => out.push_str("\\r"),
                    'f' => out.push_str("\\x0C"),
                    'v' => out.push_str("\\x0B"),
                    'a' => out.push_str("\\x07"),
                    'b' => out.push_str("\\x08"),
                    '0'..='7' => {
                        let mut v = e as u32 - '0' as u32;
                        let mut n = 1;
                        while n < 3 {
                            match chars.get(i + 1) {
                                Some(d @ '0'..='7') => {
                                    v = v * 8 + (*d as u32 - '0' as u32);
                                    i += 1;
                                    n += 1;
                                }
                                _ => break,
                            }
                        }
                        push_lit(&mut out, char::from_u32(v).unwrap_or('\0'));
                    }
                    other => push_lit(&mut out, other),
                }
                at_start = false;
                i += 1;
            }
            '[' => {
                match bracket(&chars, i) {
                    Some((class, next)) => {
                        out.push_str(&class);
                        i = next;
                    }
                    None => {
                        out.push_str("\\[");
                        i += 1;
                    }
                }
                at_start = false;
            }
            '(' => {
                out.push('(');
                depth += 1;
                at_start = true;
                i += 1;
            }
            ')' => {
                if depth > 0 {
                    depth -= 1;
                    out.push(')');
                } else {
                    out.push_str("\\)");
                }
                at_start = false;
                i += 1;
            }
            '|' => {
                out.push('|');
                has_alt = true;
                at_start = true;
                i += 1;
            }
            '^' => {
                out.push('^');
                at_start = true;
                i += 1;
            }
            '$' => {
                out.push('$');
                at_start = false;
                i += 1;
            }
            '*' | '+' | '?' => {
                if at_start {
                    push_lit(&mut out, c);
                    at_start = false;
                } else {
                    out.push(c);
                    quantified = true;
                }
                i += 1;
            }
            '{' => match interval(&chars, i) {
                Some((text, next)) if !at_start => {
                    out.push_str(&text);
                    quantified = true;
                    i = next;
                }
                _ => {
                    out.push_str("\\{");
                    at_start = false;
                    i += 1;
                }
            },
            '}' => {
                out.push_str("\\}");
                at_start = false;
                i += 1;
            }
            '.' => {
                out.push('.');
                at_start = false;
                i += 1;
            }
            c => {
                push_lit(&mut out, c);
                at_start = false;
                i += 1;
            }
        }
    }
    // Unbalanced `(`: close them so the pattern compiles like gawk's
    // (gawk reports an error; an unclosed group is rare in practice).
    for _ in 0..depth {
        out.push(')');
    }
    Translated {
        pattern: out,
        needs_longest: needs_longest || has_alt,
    }
}

/// `{n}`, `{n,}`, `{n,m}`, `{,m}` starting at `i`; `None` if not valid.
fn interval(chars: &[char], i: usize) -> Option<(String, usize)> {
    let mut j = i + 1;
    let mut lo = String::new();
    while let Some(c) = chars.get(j).filter(|c| c.is_ascii_digit()) {
        lo.push(*c);
        j += 1;
    }
    let mut hi = None;
    if chars.get(j) == Some(&',') {
        j += 1;
        let mut h = String::new();
        while let Some(c) = chars.get(j).filter(|c| c.is_ascii_digit()) {
            h.push(*c);
            j += 1;
        }
        hi = Some(h);
    }
    if chars.get(j) != Some(&'}') {
        return None;
    }
    if lo.is_empty() && hi.as_ref().is_none_or(|h| h.is_empty()) {
        return None;
    }
    let lo = if lo.is_empty() { "0".to_string() } else { lo };
    let text = match hi {
        None => format!("{{{lo}}}"),
        Some(h) if h.is_empty() => format!("{{{lo},}}"),
        Some(h) => format!("{{{lo},{h}}}"),
    };
    Some((text, j + 1))
}

/// Translate a bracket expression starting at `chars[i] == '['`.
fn bracket(chars: &[char], i: usize) -> Option<(String, usize)> {
    let mut j = i + 1;
    let mut out = String::from("[");
    if chars.get(j) == Some(&'^') {
        out.push('^');
        j += 1;
    }
    let mut first = true;
    let mut items = 0;
    loop {
        let c = *chars.get(j)?;
        if c == ']' && !first {
            j += 1;
            break;
        }
        first = false;
        // [:class:], [.c.], [=c=]
        if c == '['
            && let Some(&d) = chars.get(j + 1)
            && matches!(d, ':' | '.' | '=')
        {
            let mut k = j + 2;
            let mut name = String::new();
            while k + 1 < chars.len() && !(chars[k] == d && chars[k + 1] == ']') {
                name.push(chars[k]);
                k += 1;
            }
            if k + 1 >= chars.len() {
                return None;
            }
            j = k + 2;
            if d == ':' {
                out.push_str(match name.as_str() {
                    "alpha" => "\\p{Alphabetic}",
                    "upper" => "\\p{Uppercase}",
                    "lower" => "\\p{Lowercase}",
                    "alnum" => "\\p{Alphabetic}0-9",
                    "digit" => "0-9",
                    "space" => "\\s",
                    "blank" => " \\t",
                    "punct" => "[:punct:]",
                    "print" => "[:print:]",
                    "graph" => "[:graph:]",
                    "cntrl" => "[:cntrl:]",
                    "xdigit" => "[:xdigit:]",
                    "word" => "\\w",
                    _ => return None,
                });
            } else {
                for ch in name.chars() {
                    class_lit(&mut out, ch);
                }
            }
            items += 1;
            continue;
        }
        let (lit, next) = class_char(chars, j);
        j = next;
        // Range?
        if chars.get(j) == Some(&'-') && chars.get(j + 1).is_some_and(|&n| n != ']') {
            let (hi, after) = class_char(chars, j + 1);
            if hi >= lit {
                class_lit(&mut out, lit);
                out.push('-');
                class_lit(&mut out, hi);
            }
            j = after;
        } else {
            class_lit(&mut out, lit);
        }
        items += 1;
    }
    if items == 0 {
        return None;
    }
    out.push(']');
    Some((out, j))
}

/// One (possibly escaped) character inside a bracket expression.
fn class_char(chars: &[char], j: usize) -> (char, usize) {
    if chars[j] == '\\'
        && let Some(&e) = chars.get(j + 1)
    {
        let c = match e {
            'n' => '\n',
            't' => '\t',
            'r' => '\r',
            'f' => '\x0c',
            'v' => '\x0b',
            'a' => '\x07',
            'b' => '\x08',
            other => other,
        };
        return (c, j + 2);
    }
    (chars[j], j + 1)
}

fn class_lit(out: &mut String, c: char) {
    if regex_syntax_is_meta(c) {
        out.push('\\');
    }
    out.push(c);
}

/// Per-run cache of compiled dynamic regexes (strings used as regexes).
#[derive(Default)]
pub(crate) struct RegexCache {
    entries: HashMap<(Arc<str>, bool), Result<Arc<AwkRegex>, String>>,
    order: VecDeque<(Arc<str>, bool)>,
    bytes: usize,
}

impl RegexCache {
    pub(crate) fn get(&mut self, pattern: &str, icase: bool) -> Result<Arc<AwkRegex>, String> {
        if let Some(r) = self.entries.get(&(Arc::from(pattern), icase)) {
            return r.clone();
        }
        if pattern.len() > REGEX_SIZE_LIMIT {
            return Err("regular expression too big".to_string());
        }
        let compiled = AwkRegex::new(pattern, icase).map(Arc::new).map_err(|e| e.0);
        while self.entries.len() >= RUNTIME_REGEX_CACHE_ENTRIES
            || self.bytes + pattern.len() > REGEX_SIZE_LIMIT
        {
            let Some(old) = self.order.pop_front() else {
                break;
            };
            self.bytes -= old.0.len();
            self.entries.remove(&old);
        }
        let key: (Arc<str>, bool) = (Arc::from(pattern), icase);
        self.bytes += pattern.len();
        self.order.push_back(key.clone());
        self.entries.insert(key, compiled.clone());
        compiled
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn re(p: &str) -> AwkRegex {
        AwkRegex::new(p, false).ok().unwrap()
    }

    #[test]
    fn leftmost_longest() {
        assert_eq!(re("ab|abcd").find_at("xabcd", 0), Some((1, 5)));
        assert_eq!(re("a*(ab)?b?").find_at("aab", 0), Some((0, 3)));
        assert_eq!(re("x*").find_at("abc", 0), Some((0, 0)));
        assert_eq!(re("(a|ab)(c|bcd)").find_at("abcd", 0), Some((0, 4)));
    }

    #[test]
    fn translation() {
        assert!(re("a.b").is_match("a\nb"));
        assert!(re("[/]").is_match("/"));
        assert!(re("[]a]").is_match("]"));
        assert!(re("[^]a]").is_match("b"));
        assert!(re("a{2}").is_match("aa"));
        assert!(re("{").is_match("{"));
        assert!(re("a{").is_match("a{"));
        assert!(re("^+").is_match("+"));
        assert!(!re("^+").is_match("a"));
        assert!(re("\\yfoo\\y").is_match("a foo b"));
        assert!(re("[[:alpha:]]+").is_match("olá"));
        assert!(re("\\.").is_match("."));
        assert!(!re("\\.").is_match("a"));
        assert!(re("\\d").is_match("d"));
        assert!(re("[a\\]]").is_match("]"));
        assert!(re("a|").is_match("b"));
        assert!(re("\\<the\\>").is_match("in the end"));
        assert!(!re("\\<the\\>").is_match("other"));
    }

    #[test]
    fn captures_follow_longest() {
        let r = re("(a|ab)(c|bcd)");
        let c = r.captures_at("abcd", 0).unwrap();
        assert_eq!(c[0], Some((0, 4)));
    }

    #[test]
    fn huge_pattern_fails_cleanly() {
        assert!(AwkRegex::new("((a|b){1000}){1000}", false).is_err());
    }
}
