//! `set -x` trace text, as bash 5.2 prints it.
//!
//! Important decisions:
//! - Words are quoted with bash's own rules (`print_cmd.c`
//!   `xtrace_print_word_list`, `lib/sh/shquote.c`): empty → `''`, a shell
//!   metacharacter → single quotes, a non-printable character → `$'...'`,
//!   anything else verbatim. Assignment values use the same rules except that
//!   an empty value stays empty (`+ v=`).
//! - The line prefix is PS4 after expansion, its first character repeated
//!   once per nesting level (`$(...)`, `eval`, `source`, trap handlers),
//!   like bash's `indirection_level_string`. Expansion lives in the
//!   interpreter (it is async); this module only formats.
//! - Trace lines are queued with the pending `$(...)` stderr, so a
//!   substitution's own trace (`++ echo sub`) comes before the line of the
//!   command that expanded it, and a command's trace is written before its
//!   redirections apply (`echo x 2>/dev/null` still traces).

use std::borrow::Cow;

/// bash `sh_contains_shell_metas`: does `s` need quoting to be re-read as
/// one word?
fn contains_shell_metas(s: &str) -> bool {
    let bytes = s.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        match b {
            b' ' | b'\t' | b'\n' | b'\'' | b'"' | b'\\' | b'|' | b'&' | b';' | b'(' | b')'
            | b'<' | b'>' | b'!' | b'{' | b'}' | b'*' | b'[' | b'?' | b']' | b'^' | b'$' | b'`' => {
                return true;
            }
            b'~' if i == 0 || bytes[i - 1] == b'=' || bytes[i - 1] == b':' => return true,
            b'#' if i == 0 => return true,
            _ => {}
        }
    }
    false
}

/// bash `ansic_shouldquote`: any non-printable character.
fn should_ansic_quote(s: &str) -> bool {
    s.chars().any(|c| c.is_control())
}

/// bash `sh_single_quote`.
fn single_quote(s: &str) -> String {
    if s == "'" {
        return "\\'".to_string();
    }
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for c in s.chars() {
        if c == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(c);
        }
    }
    out.push('\'');
    out
}

/// bash `ansic_quote`: `$'...'` with C escapes, octal for other controls.
fn ansic_quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 3);
    out.push_str("$'");
    for c in s.chars() {
        let esc = match c {
            '\x1b' => Some('E'),
            '\x07' => Some('a'),
            '\x0b' => Some('v'),
            '\x08' => Some('b'),
            '\x0c' => Some('f'),
            '\n' => Some('n'),
            '\r' => Some('r'),
            '\t' => Some('t'),
            '\\' | '\'' => Some(c),
            _ => None,
        };
        if let Some(e) = esc {
            out.push('\\');
            out.push(e);
        } else if c.is_control() {
            let mut buf = [0u8; 4];
            for b in c.encode_utf8(&mut buf).bytes() {
                out.push_str(&format!("\\{b:03o}"));
            }
        } else {
            out.push(c);
        }
    }
    out.push('\'');
    out
}

/// One traced word of a simple command.
pub(super) fn quote_word(s: &str) -> Cow<'_, str> {
    if s.is_empty() {
        Cow::Borrowed("''")
    } else {
        quote_value(s)
    }
}

/// An assignment value (`+ v=`, `+ v='a b'`): empty stays empty.
pub(super) fn quote_value(s: &str) -> Cow<'_, str> {
    if contains_shell_metas(s) {
        Cow::Owned(single_quote(s))
    } else if should_ansic_quote(s) {
        Cow::Owned(ansic_quote(s))
    } else {
        Cow::Borrowed(s)
    }
}

/// The trace prefix from an expanded PS4: its first character repeated
/// `level` times (at least once), then the rest. An empty PS4 gives none.
pub(super) fn prefix(ps4: &str, level: usize) -> String {
    let mut chars = ps4.chars();
    let Some(first) = chars.next() else {
        return String::new();
    };
    let mut out = String::with_capacity(ps4.len() + level);
    for _ in 0..level.max(1) {
        out.push(first);
    }
    out.push_str(chars.as_str());
    out
}

/// One `[[ ]]` primary as bash traces it (`xtrace_print_cond_term`):
/// operands unquoted, an empty one as `''`, a lone word as `-n word`.
pub(super) fn cond_term(invert: bool, args: &[String]) -> String {
    let show = |s: &str| -> String {
        if s.is_empty() {
            "''".to_string()
        } else {
            s.to_string()
        }
    };
    let mut out = String::from("[[ ");
    if invert {
        out.push_str("! ");
    }
    match args {
        [word] => {
            out.push_str("-n ");
            out.push_str(&show(word));
        }
        [op, arg] => {
            out.push_str(op);
            out.push(' ');
            out.push_str(&show(arg));
        }
        [lhs, op, rhs] => {
            out.push_str(&show(lhs));
            out.push(' ');
            out.push_str(op);
            out.push(' ');
            out.push_str(&show(rhs));
        }
        other => {
            let words: Vec<String> = other.iter().map(|w| show(w)).collect();
            out.push_str(&words.join(" "));
        }
    }
    out.push_str(" ]]");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_quote_like_bash() {
        assert_eq!(quote_word("plain"), "plain");
        assert_eq!(quote_word(""), "''");
        assert_eq!(quote_word("a b"), "'a b'");
        assert_eq!(quote_word("it's"), "'it'\\''s'");
        assert_eq!(quote_word("'"), "\\'");
        assert_eq!(quote_word("*"), "'*'");
        assert_eq!(quote_word("a#"), "a#");
        assert_eq!(quote_word("#a"), "'#a'");
        assert_eq!(quote_word("~a"), "'~a'");
        assert_eq!(quote_word("a~"), "a~");
        assert_eq!(quote_word("a=~b"), "'a=~b'");
        assert_eq!(quote_word("tab\there"), "'tab\there'");
        assert_eq!(quote_word("\x01x"), "$'\\001x'");
        assert_eq!(quote_word("\x1b"), "$'\\E'");
        assert_eq!(quote_word("é"), "é");
    }

    #[test]
    fn empty_value_stays_bare() {
        assert_eq!(quote_value(""), "");
        assert_eq!(quote_value("x y"), "'x y'");
    }

    #[test]
    fn prefix_repeats_first_char() {
        assert_eq!(prefix("+ ", 1), "+ ");
        assert_eq!(prefix("+ ", 3), "+++ ");
        assert_eq!(prefix("+ [3] ", 2), "++ [3] ");
        assert_eq!(prefix("", 2), "");
        assert_eq!(prefix("é>", 2), "éé>");
    }

    #[test]
    fn cond_terms() {
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert_eq!(cond_term(false, &s(&["abc"])), "[[ -n abc ]]");
        assert_eq!(cond_term(true, &s(&["-z", "abc"])), "[[ ! -z abc ]]");
        assert_eq!(cond_term(false, &s(&["", "==", "a b"])), "[[ '' == a b ]]");
    }
}
