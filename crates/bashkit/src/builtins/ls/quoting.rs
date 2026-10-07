//! `ls` name quoting styles (`-N`, `-b`, `-Q`, `--quoting-style=...`).
//!
//! Mirrors GNU quotearg for the styles agents hit in practice. Non-ASCII
//! text is treated as printable (a UTF-8 locale); only C0 controls and DEL
//! are escaped. `locale`/`clocale` styles are not implemented.

use std::borrow::Cow;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum QuotingStyle {
    #[default]
    Literal,
    Escape,
    C,
    Shell,
    ShellAlways,
    ShellEscape,
    ShellEscapeAlways,
}

impl QuotingStyle {
    pub(super) fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "literal" => Self::Literal,
            "escape" => Self::Escape,
            "c" | "c-maybe" => Self::C,
            "shell" => Self::Shell,
            "shell-always" => Self::ShellAlways,
            "shell-escape" => Self::ShellEscape,
            "shell-escape-always" => Self::ShellEscapeAlways,
            _ => return None,
        })
    }
}

/// Render `name` in `style`.
pub(super) fn quote_name(name: &str, style: QuotingStyle) -> Cow<'_, str> {
    match style {
        QuotingStyle::Literal => Cow::Borrowed(name),
        QuotingStyle::Escape => Cow::Owned(c_escape(name, true, false)),
        QuotingStyle::C => Cow::Owned(format!("\"{}\"", c_escape(name, false, true))),
        QuotingStyle::Shell => shell_quote(name, false, false),
        QuotingStyle::ShellAlways => shell_quote(name, true, false),
        QuotingStyle::ShellEscape => shell_quote(name, false, true),
        QuotingStyle::ShellEscapeAlways => shell_quote(name, true, true),
    }
}

fn is_control(c: char) -> bool {
    (c as u32) < 0x20 || c == '\x7f'
}

fn push_control_escape(out: &mut String, c: char) {
    match c {
        '\x07' => out.push_str("\\a"),
        '\x08' => out.push_str("\\b"),
        '\t' => out.push_str("\\t"),
        '\n' => out.push_str("\\n"),
        '\x0b' => out.push_str("\\v"),
        '\x0c' => out.push_str("\\f"),
        '\r' => out.push_str("\\r"),
        _ => out.push_str(&format!("\\{:03o}", c as u32)),
    }
}

fn c_escape(name: &str, escape_space: bool, escape_dquote: bool) -> String {
    let mut out = String::with_capacity(name.len());
    for c in name.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            ' ' if escape_space => out.push_str("\\ "),
            '"' if escape_dquote => out.push_str("\\\""),
            c if is_control(c) => push_control_escape(&mut out, c),
            c => out.push(c),
        }
    }
    out
}

/// Characters that force quoting in the `shell*` styles.
fn shell_special(c: char) -> bool {
    matches!(
        c,
        ' ' | '!'
            | '"'
            | '$'
            | '&'
            | '\''
            | '('
            | ')'
            | '*'
            | ';'
            | '<'
            | '>'
            | '?'
            | '['
            | '\\'
            | '^'
            | '`'
            | '{'
            | '|'
            | '}'
            | '='
    ) || is_control(c)
}

fn shell_quote(name: &str, always: bool, escape_controls: bool) -> Cow<'_, str> {
    let needs = name.chars().any(shell_special) || name.starts_with(['#', '~']);
    if !needs && !always {
        return Cow::Borrowed(name);
    }
    let has_control = name.chars().any(is_control);
    // GNU prefers "..." when `'` is the only awkward character.
    if name.contains('\'') && !has_control && !name.contains(['$', '`', '"', '\\', '!']) {
        return Cow::Owned(format!("\"{name}\""));
    }
    let mut out = String::with_capacity(name.len() + 2);
    out.push('\'');
    for c in name.chars() {
        if c == '\'' {
            out.push_str("'\\''");
        } else if escape_controls && is_control(c) {
            out.push_str("'$'");
            push_control_escape(&mut out, c);
            // Close `$'...'` and reopen the plain quote.
            out.push_str("''");
        } else {
            out.push(c);
        }
    }
    out.push('\'');
    // A trailing control escape leaves an empty `''` reopen; GNU drops it.
    if escape_controls && out.ends_with("''") && out.len() > 2 {
        out.truncate(out.len() - 2);
    }
    Cow::Owned(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn styles_match_gnu() {
        let cases: &[(&str, QuotingStyle, &str)] = &[
            ("a b", QuotingStyle::Escape, "a\\ b"),
            ("c\td", QuotingStyle::Escape, "c\\td"),
            ("z\u{1}", QuotingStyle::Escape, "z\\001"),
            ("q\"x", QuotingStyle::C, "\"q\\\"x\""),
            ("a b", QuotingStyle::C, "\"a b\""),
            ("a#b", QuotingStyle::Shell, "a#b"),
            ("#a", QuotingStyle::Shell, "'#a'"),
            ("x=1", QuotingStyle::Shell, "'x=1'"),
            ("it's", QuotingStyle::Shell, "\"it's\""),
            ("a$'b", QuotingStyle::Shell, "'a$'\\''b'"),
            ("a+b", QuotingStyle::ShellAlways, "'a+b'"),
            ("c\td", QuotingStyle::ShellEscape, "'c'$'\\t''d'"),
            ("z\u{1}", QuotingStyle::ShellEscape, "'z'$'\\001'"),
            ("e'\tf", QuotingStyle::ShellEscape, "'e'\\'''$'\\t''f'"),
            ("plain", QuotingStyle::ShellEscape, "plain"),
        ];
        for (name, style, want) in cases {
            assert_eq!(quote_name(name, *style), *want, "{name:?} {style:?}"); // debug-ok: test assertion message
        }
    }
}
