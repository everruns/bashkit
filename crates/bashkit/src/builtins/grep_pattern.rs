//! GNU grep pattern compiler: POSIX BRE/ERE in the GNU dialect, translated
//! to `regex` crate syntax and matched with POSIX leftmost-longest rules.
//!
//! Decisions (GNU grep 3.11 in a UTF-8 locale is the reference):
//! - Matching reuses `awk::regex::AwkRegex` (leftmost-first find + anchored
//!   `MatchKind::All` DFA for the longest end), so `grep -oE 'ab|abcd'`
//!   prints `abcd`. Only the translation differs from awk's gawk dialect:
//!   `.` does not match newline, `\b` is a word boundary (not backspace),
//!   backslash is literal inside brackets, and errors are GNU's messages.
//! - Back-references (`\(a\)\1`, also in ERE) need a backtracking engine:
//!   such pattern sets compile with fancy-regex (leftmost-first, bounded by
//!   `FANCY_BACKTRACK_LIMIT`, TM-DOS-025).
//! - Syntax errors carry glibc's `regerror` text ("Unmatched ( or \\(",
//!   "Invalid content of \\{\\}", ...). A leading ERE repetition operator
//!   (`*a`, `(+b)`, `a|{1}c`) applies to nothing: it is dropped and the
//!   dfa.c warning ("* at start of expression") is reported. glibc skips a
//!   leading `{` even when it does not open a valid interval.
//! - Stacked repetitions (`a**`, `a+?`, `a*\{1\}`) wrap the previous
//!   repetition in a group, since `regex` rejects or reinterprets them
//!   (`+?` would be lazy).
//! - Bracket ranges with a non-ASCII endpoint fail with "Invalid collation
//!   character", as in glibc's C.UTF-8. Multi-character collating elements
//!   (`[[.ch.]]`) are rejected the same way.
//! - `[:alpha:]` & co. are Unicode-aware (C.UTF-8 classifies Unicode);
//!   under `-i`, `[:upper:]`/`[:lower:]` match any letter like glibc.
//! - "stray \\ before X" warnings (grep >= 3.8) are not emitted; see
//!   L-GREP-002 in knowledge/operations/limitations.md.
//! - THREAT[TM-DOS-023]: every compiled program uses the shared regex size
//!   limits; intervals above RE_DUP_MAX (32767) are rejected up front.

use super::awk::regex::AwkRegex;
use super::search_common::{FANCY_BACKTRACK_LIMIT, REGEX_DFA_SIZE_LIMIT, REGEX_SIZE_LIMIT};

/// glibc RE_DUP_MAX.
const DUP_MAX: u32 = 0x7fff;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Dialect {
    Basic,
    Extended,
}

/// One pattern translated to `regex` syntax.
pub(crate) struct Translated {
    pub(crate) rust: String,
    pub(crate) backrefs: bool,
    pub(crate) groups: usize,
    pub(crate) warnings: Vec<String>,
}

struct Tr<'a> {
    c: &'a [char],
    i: usize,
    dialect: Dialect,
    icase: bool,
    fancy: bool,
    group_offset: usize,
    out: String,
    /// Byte index in `out` where the most recent atom starts.
    last_atom: Option<usize>,
    /// The most recent atom already carries a repetition operator.
    quantified: bool,
    /// Start of an expression: pattern start, after `(` or `|`. Anchors
    /// leave it unchanged (`^*`).
    at_start: bool,
    /// BRE: `^` here is an anchor (start, after `\(` or `\|`).
    bre_anchor_ok: bool,
    /// ERE: a leading repetition operator was just dropped.
    dropped_op: bool,
    /// (index of `(` in `out`, group number)
    open: Vec<(usize, usize)>,
    groups: usize,
    closed: Vec<bool>,
    backrefs: bool,
    warnings: Vec<String>,
    /// dfa.c diagnoses some patterns glibc accepts; glibc errors win.
    deferred_error: Option<String>,
}

/// Translate one GNU BRE/ERE pattern. `Err` holds glibc's message.
pub(crate) fn translate(
    pattern: &str,
    dialect: Dialect,
    icase: bool,
    fancy: bool,
    group_offset: usize,
) -> Result<Translated, String> {
    let chars: Vec<char> = pattern.chars().collect();
    let mut t = Tr {
        c: &chars,
        i: 0,
        dialect,
        icase,
        fancy,
        group_offset,
        out: String::with_capacity(pattern.len() + 8),
        last_atom: None,
        quantified: false,
        at_start: true,
        bre_anchor_ok: true,
        dropped_op: false,
        open: Vec::new(),
        groups: 0,
        closed: Vec::new(),
        backrefs: false,
        warnings: Vec::new(),
        deferred_error: None,
    };
    t.run()?;
    if let Some(e) = t.deferred_error {
        return Err(e);
    }
    Ok(Translated {
        rust: t.out,
        backrefs: t.backrefs,
        groups: t.groups,
        warnings: t.warnings,
    })
}

fn push_lit(out: &mut String, c: char) {
    let mut buf = [0u8; 4];
    out.push_str(&regex::escape(c.encode_utf8(&mut buf)));
}

impl Tr<'_> {
    fn peek(&self, k: usize) -> Option<char> {
        self.c.get(self.i + k).copied()
    }

    fn begin_atom(&mut self) {
        self.last_atom = Some(self.out.len());
        self.quantified = false;
        self.at_start = false;
        self.bre_anchor_ok = false;
        self.dropped_op = false;
    }

    fn literal(&mut self, c: char) {
        self.begin_atom();
        push_lit(&mut self.out, c);
    }

    /// Zero-width assertion: quantifiable, but keeps the start context.
    fn anchor(&mut self, text: &str) {
        self.last_atom = Some(self.out.len());
        self.quantified = false;
        self.bre_anchor_ok = false;
        self.dropped_op = false;
        self.out.push_str(text);
    }

    /// Apply a repetition operator. `None` means it was at the start of an
    /// expression and the caller decides (literal in BRE, dropped in ERE).
    fn quantify(&mut self, op: &str) -> Option<()> {
        if self.at_start {
            return None;
        }
        let start = self.last_atom?;
        if self.quantified {
            self.out.insert_str(start, "(?:");
            self.out.push(')');
        }
        self.out.push_str(op);
        self.quantified = true;
        Some(())
    }

    fn drop_leading_op(&mut self, name: &str) {
        self.warnings
            .push(format!("warning: {name} at start of expression"));
        self.dropped_op = true;
    }

    fn run(&mut self) -> Result<(), String> {
        while self.i < self.c.len() {
            let ch = self.c[self.i];
            let ere = self.dialect == Dialect::Extended;
            match ch {
                '\\' => self.escape()?,
                '[' => {
                    let class = self.bracket()?;
                    self.begin_atom();
                    self.out.push_str(&class);
                }
                '.' => {
                    self.i += 1;
                    self.begin_atom();
                    self.out.push('.');
                }
                '*' => {
                    self.i += 1;
                    if self.quantify("*").is_none() {
                        if ere {
                            self.drop_leading_op("*");
                        } else {
                            self.literal('*');
                        }
                    }
                }
                '+' | '?' if ere => {
                    self.i += 1;
                    let op = if ch == '+' { "+" } else { "?" };
                    if self.quantify(op).is_none() {
                        self.drop_leading_op(op);
                    }
                }
                '{' if ere => self.ere_brace()?,
                '(' if ere => {
                    self.i += 1;
                    self.open_group();
                }
                ')' if ere => {
                    self.i += 1;
                    if self.open.is_empty() {
                        self.literal(')');
                    } else {
                        self.close_group()?;
                    }
                }
                '|' if ere => {
                    self.i += 1;
                    self.alternate();
                }
                '^' => {
                    self.i += 1;
                    if ere || self.bre_anchor_ok {
                        // BRE `^^`: only the first one anchors.
                        self.anchor("^");
                    } else {
                        self.literal('^');
                    }
                }
                '$' => {
                    self.i += 1;
                    let bre_anchor = self.i == self.c.len()
                        || (self.peek(0) == Some('\\') && matches!(self.peek(1), Some(')' | '|')));
                    if ere || bre_anchor {
                        self.anchor("$");
                    } else {
                        self.literal('$');
                    }
                }
                c => {
                    self.i += 1;
                    self.literal(c);
                }
            }
        }
        if !self.open.is_empty() {
            return Err("Unmatched ( or \\(".to_string());
        }
        Ok(())
    }

    fn open_group(&mut self) {
        self.groups += 1;
        self.open.push((self.out.len(), self.groups));
        self.out.push('(');
        self.last_atom = None;
        self.quantified = false;
        self.at_start = true;
        self.bre_anchor_ok = true;
        self.dropped_op = false;
    }

    fn close_group(&mut self) -> Result<(), String> {
        if self.dropped_op {
            // glibc: `(*)` leaves nothing for the group to hold.
            return Err("Unmatched ( or \\(".to_string());
        }
        let Some((start, number)) = self.open.pop() else {
            return Err("Unmatched ) or \\)".to_string());
        };
        self.out.push(')');
        if self.closed.len() <= number {
            self.closed.resize(number + 1, false);
        }
        self.closed[number] = true;
        self.last_atom = Some(start);
        self.quantified = false;
        self.at_start = false;
        self.bre_anchor_ok = false;
        Ok(())
    }

    fn alternate(&mut self) {
        self.out.push('|');
        self.last_atom = None;
        self.quantified = false;
        self.at_start = true;
        self.bre_anchor_ok = true;
        self.dropped_op = false;
    }

    fn escape(&mut self) -> Result<(), String> {
        let Some(e) = self.peek(1) else {
            return Err("Trailing backslash".to_string());
        };
        let bre = self.dialect == Dialect::Basic;
        if bre && e == '{' {
            return self.bre_brace();
        }
        self.i += 2;
        match e {
            '(' if bre => self.open_group(),
            ')' if bre => self.close_group()?,
            '|' if bre => self.alternate(),
            '+' | '?' if bre => {
                let op = if e == '+' { "+" } else { "?" };
                if self.quantify(op).is_none() {
                    self.literal(e);
                }
            }
            '1'..='9' => {
                let n = e as usize - '0' as usize;
                if !self.closed.get(n).copied().unwrap_or(false) {
                    return Err("Invalid back reference".to_string());
                }
                self.backrefs = true;
                self.begin_atom();
                let g = n + self.group_offset;
                if g < 10 {
                    self.out.push_str(&format!("\\{g}"));
                } else {
                    self.out.push_str(&format!("\\k<{g}>"));
                }
            }
            '<' => {
                let a = if self.fancy {
                    r"(?:(?<!\w)(?=\w))"
                } else {
                    r"\b{start}"
                };
                self.anchor(a);
            }
            '>' => {
                let a = if self.fancy {
                    r"(?:(?<=\w)(?!\w))"
                } else {
                    r"\b{end}"
                };
                self.anchor(a);
            }
            'b' => self.anchor(r"\b"),
            'B' => self.anchor(r"\B"),
            '`' => self.anchor(r"\A"),
            '\'' => self.anchor(r"\z"),
            'w' | 'W' | 's' | 'S' => {
                self.begin_atom();
                self.out.push('\\');
                self.out.push(e);
            }
            other => self.literal(other),
        }
        Ok(())
    }

    /// Parse `digits[,digits]` at `j`; returns (lo, comma, hi, next).
    fn interval_body(&self, mut j: usize) -> (Option<u32>, bool, Option<u32>, usize) {
        let digits = |j: &mut usize| -> Option<u32> {
            let mut v: Option<u32> = None;
            while let Some(d) = self.c.get(*j).and_then(|c| c.to_digit(10)) {
                v = Some(v.unwrap_or(0).saturating_mul(10).saturating_add(d));
                *j += 1;
            }
            v
        };
        let lo = digits(&mut j);
        let mut comma = false;
        let mut hi = None;
        if self.c.get(j) == Some(&',') {
            comma = true;
            j += 1;
            hi = digits(&mut j);
        }
        (lo, comma, hi, j)
    }

    fn interval_text(lo: Option<u32>, comma: bool, hi: Option<u32>) -> Result<String, String> {
        if lo.is_none() && !comma {
            return Err("Invalid content of \\{\\}".to_string());
        }
        let min = lo.unwrap_or(0);
        let max = if comma { hi } else { Some(min) };
        if max.is_some_and(|m| m < min) {
            return Err("Invalid content of \\{\\}".to_string());
        }
        if min > DUP_MAX || max.is_some_and(|m| m > DUP_MAX) {
            return Err("Regular expression too big".to_string());
        }
        Ok(match max {
            Some(_) if !comma => format!("{{{min}}}"),
            Some(m) => format!("{{{min},{m}}}"),
            None => format!("{{{min},}}"),
        })
    }

    /// ERE `{`: an interval if well formed, else a literal brace.
    fn ere_brace(&mut self) -> Result<(), String> {
        let (lo, comma, hi, j) = self.interval_body(self.i + 1);
        let closed = self.c.get(j) == Some(&'}');
        if self.at_start {
            if closed {
                Self::interval_text(lo, comma, hi)?;
                self.i = j + 1;
                self.drop_leading_op("{...}");
            } else {
                // glibc skips a leading `{` token, valid interval or not.
                self.i += 1;
                self.dropped_op = true;
            }
            return Ok(());
        }
        if !closed {
            self.i += 1;
            self.literal('{');
            return Ok(());
        }
        let text = Self::interval_text(lo, comma, hi)?;
        self.i = j + 1;
        if self.quantify(&text).is_none() {
            self.drop_leading_op("{...}");
        }
        Ok(())
    }

    /// BRE `\{`: must be a valid interval (literal at expression start).
    fn bre_brace(&mut self) -> Result<(), String> {
        if self.at_start {
            self.i += 2;
            self.literal('{');
            return Ok(());
        }
        let (lo, comma, hi, j) = self.interval_body(self.i + 2);
        if self.c.get(j) == Some(&'\\') && self.c.get(j + 1) == Some(&'}') {
            let text = Self::interval_text(lo, comma, hi)?;
            self.i = j + 2;
            if self.quantify(&text).is_none() {
                self.literal('{');
            }
            return Ok(());
        }
        let has_close = self.c[j..].windows(2).any(|w| w == ['\\', '}']);
        Err(if has_close {
            "Invalid content of \\{\\}".to_string()
        } else {
            "Unmatched \\{".to_string()
        })
    }

    /// POSIX bracket expression at `self.i` (`[`), as a `regex` class.
    fn bracket(&mut self) -> Result<String, String> {
        const UNMATCHED: &str = "Unmatched [, [^, [:, [., or [=";
        let c = self.c;
        let mut j = self.i + 1;
        let mut neg = false;
        if c.get(j) == Some(&'^') {
            neg = true;
            j += 1;
        }
        if j >= c.len() {
            return Err("Invalid regular expression".to_string());
        }
        let content_start = j;
        let mut items = String::new();
        let mut first = true;
        loop {
            let Some(&ch) = c.get(j) else {
                return Err(UNMATCHED.to_string());
            };
            if ch == ']' && !first {
                j += 1;
                break;
            }
            first = false;
            let lo = match self.bracket_elem(&mut j)? {
                Elem::Class(text) => {
                    items.push_str(&text);
                    continue;
                }
                Elem::Char(ch) => ch,
            };
            if c.get(j) == Some(&'-') && c.get(j + 1).is_some_and(|&n| n != ']') {
                j += 1;
                let hi = match self.bracket_elem(&mut j)? {
                    Elem::Char(h) => h,
                    Elem::Class(_) => return Err("Invalid range end".to_string()),
                };
                if !lo.is_ascii() || !hi.is_ascii() {
                    return Err("Invalid collation character".to_string());
                }
                if lo > hi {
                    return Err("Invalid range end".to_string());
                }
                class_lit(&mut items, lo);
                items.push('-');
                class_lit(&mut items, hi);
                if c.get(j) == Some(&'-') && c.get(j + 1).is_some_and(|&n| n != ']') {
                    return Err("Invalid range end".to_string());
                }
            } else {
                class_lit(&mut items, lo);
            }
        }
        // dfa.c: `[:space:]` is almost certainly a mistake for `[[:space:]]`.
        let content = &c[content_start..j - 1];
        if content.len() >= 2
            && content[0] == ':'
            && content[content.len() - 1] == ':'
            && content[1..content.len() - 1]
                .iter()
                .all(|ch| ch.is_ascii_alphabetic())
            && self.deferred_error.is_none()
        {
            self.deferred_error =
                Some("character class syntax is [[:space:]], not [:space:]".to_string());
        }
        self.i = j;
        let mut out = String::with_capacity(items.len() + 3);
        out.push('[');
        if neg {
            out.push('^');
        }
        out.push_str(&items);
        out.push(']');
        Ok(out)
    }

    fn bracket_elem(&self, j: &mut usize) -> Result<Elem, String> {
        const UNMATCHED: &str = "Unmatched [, [^, [:, [., or [=";
        let c = self.c;
        let ch = c[*j];
        if ch == '['
            && let Some(&d) = c.get(*j + 1)
            && matches!(d, ':' | '.' | '=')
        {
            let mut k = *j + 2;
            while k + 1 < c.len() && !(c[k] == d && c[k + 1] == ']') {
                k += 1;
            }
            if k + 1 >= c.len() {
                return Err(UNMATCHED.to_string());
            }
            let name: String = c[*j + 2..k].iter().collect();
            *j = k + 2;
            if d == ':' {
                let text = match name.as_str() {
                    "alpha" => r"\p{Alphabetic}",
                    "upper" if self.icase => r"\p{Alphabetic}",
                    "lower" if self.icase => r"\p{Alphabetic}",
                    "upper" => r"\p{Uppercase}",
                    "lower" => r"\p{Lowercase}",
                    "digit" => "0-9",
                    "alnum" => r"\p{Alphabetic}0-9",
                    "xdigit" => "0-9A-Fa-f",
                    "space" => r"\s",
                    "blank" => r" \t",
                    "punct" => r"\p{P}\p{S}",
                    "cntrl" => r"\p{Cc}",
                    "print" => r"\P{C}",
                    "graph" => r"[\P{C}&&\S]",
                    _ => return Err("Invalid character class name".to_string()),
                };
                return Ok(Elem::Class(text.to_string()));
            }
            let mut it = name.chars();
            return match (it.next(), it.next()) {
                (Some(one), None) => Ok(Elem::Char(one)),
                _ => Err("Invalid collation character".to_string()),
            };
        }
        *j += 1;
        Ok(Elem::Char(ch))
    }
}

enum Elem {
    Char(char),
    Class(String),
}

fn class_lit(out: &mut String, c: char) {
    if matches!(c, '\\' | ']' | '[' | '^' | '-' | '&' | '~') {
        out.push('\\');
    }
    out.push(c);
}

/// A compiled grep pattern set.
pub(crate) enum PatternMatcher {
    /// POSIX leftmost-longest (BRE/ERE/fixed strings).
    Posix(Box<AwkRegex>),
    /// Backtracking: back-references and `-P`.
    Fancy(fancy_regex::Regex),
    /// No patterns at all (`-f /dev/null`): nothing matches.
    Nothing,
}

impl PatternMatcher {
    pub(crate) fn is_match(&self, s: &str) -> bool {
        match self {
            PatternMatcher::Posix(re) => re.is_match(s),
            PatternMatcher::Fancy(re) => re.is_match(s).unwrap_or(false),
            PatternMatcher::Nothing => false,
        }
    }

    /// First match at or after byte `start` (context before `start` still
    /// counts for anchors and word boundaries).
    pub(crate) fn find_at(&self, s: &str, start: usize) -> Option<(usize, usize)> {
        match self {
            PatternMatcher::Posix(re) => re.find_at(s, start),
            PatternMatcher::Fancy(re) => re
                .find_from_pos(s, start)
                .ok()
                .flatten()
                .map(|m| (m.start(), m.end())),
            PatternMatcher::Nothing => None,
        }
    }
}

/// How the pattern list is to be read.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Syntax {
    Basic,
    Extended,
    Fixed,
    Perl,
}

pub(crate) struct CompileOptions {
    pub(crate) syntax: Syntax,
    pub(crate) icase: bool,
    pub(crate) word: bool,
    pub(crate) line: bool,
    /// `-z`: records may contain newlines, which `.` then matches.
    pub(crate) null_data: bool,
}

/// Compile grep's pattern list. Returns the matcher and dfa.c-style
/// warnings; `Err` is the message to print after `grep: `.
pub(crate) fn compile(
    patterns: &[String],
    o: &CompileOptions,
) -> Result<(PatternMatcher, Vec<String>), String> {
    if patterns.is_empty() {
        return Ok((PatternMatcher::Nothing, Vec::new()));
    }
    let mut warnings = Vec::new();
    let mut fancy = o.syntax == Syntax::Perl;
    let mut parts: Vec<String> = Vec::with_capacity(patterns.len());
    match o.syntax {
        Syntax::Fixed => {
            for p in patterns {
                parts.push(regex::escape(p));
            }
        }
        Syntax::Perl => parts.extend(patterns.iter().cloned()),
        Syntax::Basic | Syntax::Extended => {
            let dialect = if o.syntax == Syntax::Basic {
                Dialect::Basic
            } else {
                Dialect::Extended
            };
            let translate_all = |fancy: bool| -> Result<(Vec<Translated>, bool), String> {
                let mut out = Vec::with_capacity(patterns.len());
                let mut offset = 0;
                let mut any_backref = false;
                for p in patterns {
                    let t = translate(p, dialect, o.icase, fancy, offset)?;
                    offset += t.groups;
                    any_backref |= t.backrefs;
                    out.push(t);
                }
                Ok((out, any_backref))
            };
            let (mut ts, backrefs) = translate_all(false)?;
            if backrefs {
                fancy = true;
                ts = translate_all(true)?.0;
            }
            for t in ts {
                warnings.extend(t.warnings);
                parts.push(t.rust);
            }
        }
    }
    let combined = if parts.len() == 1 {
        parts.pop().unwrap_or_default()
    } else {
        parts
            .iter()
            .map(|p| format!("(?:{p})"))
            .collect::<Vec<_>>()
            .join("|")
    };
    let body = if o.line {
        format!("^(?:{combined})$")
    } else if o.word {
        if fancy {
            format!(r"(?<![\w])(?:{combined})(?![\w])")
        } else {
            format!(r"\b{{start-half}}(?:{combined})\b{{end-half}}")
        }
    } else {
        combined
    };
    let mut flags = String::new();
    if o.icase {
        flags.push('i');
    }
    if o.null_data {
        flags.push('s');
    }
    let full = if flags.is_empty() {
        body
    } else {
        format!("(?{flags}){body}")
    };
    if fancy {
        let re = fancy_regex::RegexBuilder::new(&full)
            .delegate_size_limit(REGEX_SIZE_LIMIT)
            .delegate_dfa_size_limit(REGEX_DFA_SIZE_LIMIT)
            .backtrack_limit(FANCY_BACKTRACK_LIMIT)
            .build()
            .map_err(|e| fancy_error(&e))?;
        return Ok((PatternMatcher::Fancy(re), warnings));
    }
    AwkRegex::from_translated(full, true)
        .map(|re| (PatternMatcher::Posix(Box::new(re)), warnings))
        .map_err(|e| {
            if e.0.contains("too big") {
                "Regular expression too big".to_string()
            } else {
                e.0
            }
        })
}

fn fancy_error(e: &fancy_regex::Error) -> String {
    let s = e.to_string();
    if s.contains("too big") || s.contains("size limit") {
        return "Regular expression too big".to_string();
    }
    let line = s
        .lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("invalid regular expression");
    line.trim().chars().take(200).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tr(p: &str, d: Dialect) -> Result<String, String> {
        translate(p, d, false, false, 0).map(|t| t.rust)
    }

    fn m(p: &str, syntax: Syntax) -> PatternMatcher {
        compile(
            &[p.to_string()],
            &CompileOptions {
                syntax,
                icase: false,
                word: false,
                line: false,
                null_data: false,
            },
        )
        .map(|(m, _)| m)
        .unwrap_or(PatternMatcher::Nothing)
    }

    #[test]
    fn bre_operators_and_literals() {
        assert_eq!(tr(r"a\(b\)c", Dialect::Basic).unwrap(), "a(b)c");
        assert_eq!(tr("a(b)|c+?", Dialect::Basic).unwrap(), r"a\(b\)\|c\+\?");
        assert_eq!(tr("*a", Dialect::Basic).unwrap(), r"\*a");
        assert_eq!(tr("^*", Dialect::Basic).unwrap(), r"^\*");
        assert_eq!(tr("a^b$c", Dialect::Basic).unwrap(), r"a\^b\$c");
        assert_eq!(tr(r"a\{2,3\}", Dialect::Basic).unwrap(), "a{2,3}");
        assert_eq!(tr(r"a\|b", Dialect::Basic).unwrap(), "a|b");
    }

    #[test]
    fn stacked_repetitions_are_grouped() {
        assert_eq!(tr("a**", Dialect::Extended).unwrap(), "(?:a*)*");
        assert_eq!(tr("a+?", Dialect::Extended).unwrap(), "(?:a+)?");
    }

    #[test]
    fn ere_leading_operator_is_dropped_with_warning() {
        let t = translate("*xyz", Dialect::Extended, false, false, 0).unwrap();
        assert_eq!(t.rust, "xyz");
        assert_eq!(t.warnings, vec!["warning: * at start of expression"]);
        assert!(tr("(*)b", Dialect::Extended).is_err());
        assert_eq!(tr("{1", Dialect::Extended).unwrap(), "1");
    }

    #[test]
    fn syntax_errors_use_glibc_messages() {
        let err = |p: &str, d| tr(p, d).err().unwrap_or_default();
        assert_eq!(err(r"\(a", Dialect::Basic), "Unmatched ( or \\(");
        assert_eq!(err(r"a\)", Dialect::Basic), "Unmatched ) or \\)");
        assert_eq!(
            err("[abc", Dialect::Extended),
            "Unmatched [, [^, [:, [., or [="
        );
        assert_eq!(
            err("[[:foo:]]", Dialect::Extended),
            "Invalid character class name"
        );
        assert_eq!(err("[z-a]", Dialect::Extended), "Invalid range end");
        assert_eq!(
            err("a{3,1}", Dialect::Extended),
            "Invalid content of \\{\\}"
        );
        assert_eq!(err(r"a\{1", Dialect::Basic), "Unmatched \\{");
        assert_eq!(err(r"\(a\)\2", Dialect::Basic), "Invalid back reference");
        assert_eq!(err("ab\\", Dialect::Basic), "Trailing backslash");
        assert_eq!(
            err("[à-ú]", Dialect::Extended),
            "Invalid collation character"
        );
        assert_eq!(
            err("b{40000}", Dialect::Extended),
            "Regular expression too big"
        );
        assert_eq!(
            err("[:space:]", Dialect::Extended),
            "character class syntax is [[:space:]], not [:space:]"
        );
    }

    #[test]
    fn literal_braces_and_parens_in_ere() {
        assert!(m("a{x}", Syntax::Extended).is_match("a{x}"));
        assert!(m("a)", Syntax::Extended).is_match("a)"));
        assert!(m("a{1,*}", Syntax::Extended).is_match("a{1,,,}"));
    }

    #[test]
    fn leftmost_longest_and_backrefs() {
        assert_eq!(
            m("ab|abcd", Syntax::Extended).find_at("abcd", 0),
            Some((0, 4))
        );
        assert_eq!(
            m(r"\(ab\)\1", Syntax::Basic).find_at("xabab", 0),
            Some((1, 5))
        );
        assert!(!m(r"\(ab\)\1", Syntax::Basic).is_match("abx"));
    }

    #[test]
    fn brackets_follow_posix() {
        assert!(m(r"[a\]]", Syntax::Basic).is_match("\\]"));
        assert!(m("[]a]", Syntax::Basic).is_match("]"));
        assert!(m("[[:alpha:]]", Syntax::Basic).is_match("é"));
        assert!(m("a[[.-.]--]c", Syntax::Extended).is_match("a-c"));
        assert!(m("a[\x01-\x03]?c", Syntax::Extended).is_match("a\x02c"));
    }
}
