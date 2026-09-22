//! POSIX BRE/ERE → `regex` crate translation, shared by every builtin that
//! accepts a POSIX regular expression.
//!
//! Important decisions (issues #2427, #2437):
//!
//! - There is exactly **one** translator. `sed` grew a correct one while `grep`
//!   kept a copy that handled only `( ) { }`, so `grep 'a+b'` and
//!   `sed 's/a+b/'` disagreed about whether `+` was an operator. Both now call
//!   [`translate`].
//! - Whether `+ ? | * ^ $` are operators or literals is **positional** in BRE,
//!   so the translator tracks "start of a branch" state rather than doing
//!   string replacements.
//! - Bracket expressions are copied by a dedicated scanner: inside `[...]`
//!   POSIX gives `\` no special meaning, while the `regex` crate treats `\`,
//!   `[`, `&&`, `~~` and `--` as syntax, so those are escaped on the way out.
//! - GNU's tools disagree about **invalid quantifiers**, so the caller picks:
//!   `grep` degrades them to literals (and drops a leading one with a warning
//!   on stderr) while `sed` rejects the expression. [`Syntax::lenient`]
//!   selects between the two; neither tool may silently match the wrong thing.

/// Which dialect to read the pattern as, and how forgiving to be.
#[derive(Clone, Copy)]
pub(crate) struct Syntax {
    /// `-E`/`-r`: read the pattern as POSIX ERE rather than BRE.
    pub(crate) extended: bool,
    /// Degrade an invalid quantifier to a literal instead of rejecting it.
    /// GNU grep does this; GNU sed does not.
    pub(crate) lenient: bool,
}

impl Syntax {
    pub(crate) fn new(extended: bool, lenient: bool) -> Self {
        Syntax { extended, lenient }
    }
}

/// A translated pattern plus any diagnostics a lenient caller should print.
pub(crate) struct Translated {
    pub(crate) regex: String,
    /// GNU grep's `warning: * at start of expression`, in order.
    pub(crate) warnings: Vec<String>,
}

/// Translate a POSIX BRE/ERE into `regex` crate syntax.
pub(crate) fn translate(pattern: &str, syntax: Syntax) -> Translated {
    let extended = syntax.extended;
    let chars: Vec<char> = pattern.chars().collect();
    let mut out = String::with_capacity(pattern.len() + 8);
    let mut warnings = Vec::new();
    let mut i = 0;
    // True when the next token begins an alternation branch: `^` anchors there
    // and (BRE only) a `*` there is an ordinary character.
    let mut branch_start = true;
    // True just after a leading `^`. GNU grep warns about a quantifier there
    // (it has nothing to repeat but the anchor) yet keeps it, so `^*a` still
    // matches the same text; only a quantifier at a true branch start is
    // dropped. Conflating the two would change what the pattern matches.
    let mut after_anchor = false;

    while i < chars.len() {
        let ch = chars[i];
        let anchor_carry = after_anchor;
        after_anchor = false;
        let after_anchor_here = anchor_carry;
        match ch {
            '\\' if i + 1 < chars.len() => {
                let next = chars[i + 1];
                i += 2;
                match next {
                    // `\{` only opens an interval when a valid one follows;
                    // GNU grep otherwise reads it as a brace, GNU sed rejects it.
                    '{' if !extended && syntax.lenient && !opens_interval(&chars, i - 2, false) => {
                        out.push_str("\\{");
                        branch_start = false;
                    }
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
                let anchored = extended || branch_start;
                if anchored {
                    out.push('^');
                } else {
                    out.push_str("\\^");
                }
                after_anchor = anchored && extended && branch_start;
                i += 1;
                // `^*` keeps `*` literal in BRE.
                branch_start = !extended;
                continue;
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
            '*' | '+' | '?'
                if extended && syntax.lenient && (branch_start || after_anchor_here) =>
            {
                warnings.push(format!("{ch} at start of expression"));
                if branch_start {
                    // Nothing at all to repeat: GNU drops it.
                    i += 1;
                } else {
                    // Repeating the anchor is accepted; only warn.
                    out.push(ch);
                    i += 1;
                }
            }
            '{' if extended && syntax.lenient => {
                match interval_end(&chars, i, true) {
                    // A valid `{n,m}` is copied through whole, so the closing
                    // brace never reaches the orphan-brace arm below.
                    Some(end) => {
                        out.extend(&chars[i..=end]);
                        i = end + 1;
                    }
                    None => {
                        out.push_str("\\{");
                        i += 1;
                    }
                }
                branch_start = false;
            }
            // A `}` with no interval to close is an ordinary character.
            '}' if extended && syntax.lenient => {
                out.push_str("\\}");
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

    Translated {
        regex: out,
        warnings,
    }
}

/// Does a POSIX interval (`{n}`, `{n,}`, `{n,m}`) start here? `at` indexes the
/// backslash of `\{` in BRE, or the `{` itself in ERE.
fn opens_interval(chars: &[char], at: usize, extended: bool) -> bool {
    interval_end(chars, at, extended).is_some()
}

/// Index of the interval's final character (`}` in ERE, the `}` of `\}` in
/// BRE), or `None` when no valid interval starts at `at`.
fn interval_end(chars: &[char], at: usize, extended: bool) -> Option<usize> {
    let mut i = at + if extended { 1 } else { 2 };
    let digits = |i: &mut usize| {
        let start = *i;
        while chars.get(*i).is_some_and(|c| c.is_ascii_digit()) {
            *i += 1;
        }
        *i > start
    };
    if !digits(&mut i) {
        return None;
    }
    if chars.get(i) == Some(&',') {
        i += 1;
        digits(&mut i);
    }
    if extended {
        (chars.get(i) == Some(&'}')).then_some(i)
    } else {
        (chars.get(i) == Some(&'\\') && chars.get(i + 1) == Some(&'}')).then_some(i + 1)
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn bre(p: &str) -> String {
        translate(p, Syntax::new(false, false)).regex
    }
    fn ere(p: &str) -> String {
        translate(p, Syntax::new(true, false)).regex
    }
    fn lenient(p: &str, extended: bool) -> Translated {
        translate(p, Syntax::new(extended, true))
    }

    #[test]
    fn bre_operators_need_a_backslash() {
        assert_eq!(bre("a+b"), "a\\+b");
        assert_eq!(bre("a?b"), "a\\?b");
        assert_eq!(bre("a|b"), "a\\|b");
        assert_eq!(bre("a\\+b"), "a+b");
        assert_eq!(bre("a\\?b"), "a?b");
        assert_eq!(bre("a\\|b"), "a|b");
        assert_eq!(bre("a(b)"), "a\\(b\\)");
        assert_eq!(bre("\\(a\\)"), "(a)");
    }

    #[test]
    fn bre_anchors_and_star_are_positional() {
        assert_eq!(bre("^a$"), "^a$");
        assert_eq!(bre("a^b"), "a\\^b");
        assert_eq!(bre("a$b"), "a\\$b");
        assert_eq!(bre("*a"), "\\*a");
        assert_eq!(bre("^*a"), "^\\*a");
        assert_eq!(bre("a*"), "a*");
        // `$` still anchors before `\)` and `\|`.
        assert_eq!(bre("\\(a$\\)"), "(a$)");
    }

    #[test]
    fn ere_keeps_its_operators() {
        assert_eq!(ere("a+b"), "a+b");
        assert_eq!(ere("(a|b)+"), "(a|b)+");
        assert_eq!(ere("a\\+b"), "a\\+b");
    }

    #[test]
    fn bracket_expressions_keep_posix_meaning() {
        // `\` is an ordinary member inside a bracket expression.
        assert_eq!(bre("[\\]"), "[\\\\]");
        assert_eq!(bre("[]a]"), "[\\]a]");
        assert_eq!(bre("[^]a]"), "[^\\]a]");
        assert_eq!(bre("[[:alpha:]]"), "[[:alpha:]]");
        // `&&`, `~~` and `[` are regex-crate class syntax, not POSIX.
        assert_eq!(bre("[a&&b]"), "[a\\&\\&b]");
        assert_eq!(bre("[a[b]"), "[a\\[b]");
    }

    #[test]
    fn gnu_escapes_are_translated() {
        assert_eq!(bre("\\<ab\\>"), "\\bab\\b");
        assert_eq!(bre("\\`a\\'"), "\\Aa\\z");
        assert_eq!(bre("\\w\\s"), "\\w\\s");
        assert_eq!(bre("\\n"), "\\\n");
    }

    #[test]
    fn intervals_are_recognised_in_both_dialects() {
        assert_eq!(bre("a\\{2\\}"), "a{2}");
        assert_eq!(bre("a\\{2,\\}"), "a{2,}");
        assert_eq!(bre("a\\{2,3\\}"), "a{2,3}");
        assert_eq!(ere("a{2}"), "a{2}");
        assert_eq!(ere("a{2,3}"), "a{2,3}");
    }

    /// GNU grep degrades an invalid quantifier; GNU sed rejects it. The strict
    /// caller must keep producing something the engine will refuse.
    #[test]
    fn lenient_degrades_what_strict_leaves_to_the_engine() {
        // A brace that opens no valid interval is an ordinary character.
        assert_eq!(lenient("a{b", true).regex, "a\\{b");
        assert_eq!(lenient("{}", true).regex, "\\{\\}");
        assert_eq!(lenient("a\\{", false).regex, "a\\{");
        // ... but a valid interval still compiles as one.
        assert_eq!(lenient("a{2}b", true).regex, "a{2}b");
        assert_eq!(lenient("a\\{2\\}b", false).regex, "a{2}b");
        // Strict leaves the invalid forms for the engine to reject.
        assert_eq!(ere("a{b"), "a{b");
        assert_eq!(bre("a\\{"), "a{");
    }

    #[test]
    fn lenient_drops_a_quantifier_with_nothing_to_repeat() {
        let t = lenient("*a", true);
        assert_eq!(t.regex, "a");
        assert_eq!(t.warnings, ["* at start of expression"]);

        let t = lenient("+", true);
        assert_eq!(t.regex, "");
        assert_eq!(t.warnings, ["+ at start of expression"]);

        // After `^` the quantifier has the anchor to repeat: GNU warns but
        // keeps it, so the pattern goes on matching the same text.
        let t = lenient("^*a", true);
        assert_eq!(t.regex, "^*a");
        assert_eq!(t.warnings, ["* at start of expression"]);

        // A quantifier that does have something to repeat is silent.
        assert!(lenient("a*", true).warnings.is_empty());
        assert!(lenient("^ab", true).warnings.is_empty());
    }

    #[test]
    fn lenient_bre_never_needs_a_warning() {
        // In BRE these are already ordinary characters, so nothing is dropped.
        for p in ["*a", "+", "?", "^*a"] {
            assert!(lenient(p, false).warnings.is_empty(), "{p}");
        }
    }
}
