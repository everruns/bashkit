//! jq's `$__loc__`: `{"file":"<top-level>","line":N}` for the line where it
//! appears in the filter.
//!
//! Important decisions:
//!  - jaq has no `$__loc__`, so the filter text is rewritten before it is
//!    compiled: each `$__loc__` in code becomes the object literal. A small
//!    scanner skips string literals (but not their `\(...)` interpolations)
//!    and comments, and counts lines.
//!  - `{$__loc__}` is jq's object shorthand for `{"__loc__": $__loc__}`; it
//!    is written out as that pair.

use std::borrow::Cow;

const TOKEN: &str = "$__loc__";

fn is_ident(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// Replace every `$__loc__` in filter code with its location object.
pub(super) fn expand_loc(src: &str) -> Cow<'_, str> {
    if !src.contains(TOKEN) {
        return Cow::Borrowed(src);
    }
    let b = src.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(src.len() + 64);
    // Open brackets: `{`, `[`, `(`, `"` (string), `I` (interpolation).
    let mut stack: Vec<u8> = Vec::new();
    let mut line = 1usize;
    let mut last_sig = 0u8;
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if stack.last() == Some(&b'"') {
            match c {
                b'\\' if b.get(i + 1) == Some(&b'(') => {
                    stack.push(b'I');
                    out.extend_from_slice(b"\\(");
                    last_sig = b'(';
                    i += 2;
                    continue;
                }
                b'\\' if i + 1 < b.len() => {
                    out.extend_from_slice(&b[i..i + 2]);
                    i += 2;
                    continue;
                }
                b'"' => {
                    stack.pop();
                    last_sig = b'"';
                }
                b'\n' => line += 1,
                _ => {}
            }
            out.push(c);
            i += 1;
            continue;
        }
        match c {
            b'#' => {
                let end = b[i..]
                    .iter()
                    .position(|&x| x == b'\n')
                    .map_or(b.len(), |p| i + p);
                out.extend_from_slice(&b[i..end]);
                i = end;
                continue;
            }
            b'"' | b'{' | b'[' | b'(' => stack.push(c),
            b'}' | b']' | b')' => {
                stack.pop();
            }
            b'\n' => line += 1,
            b'$' if src[i..].starts_with(TOKEN)
                && !b.get(i + TOKEN.len()).is_some_and(|&x| is_ident(x)) =>
            {
                let obj = format!("{{\"file\":\"<top-level>\",\"line\":{line}}}");
                let next_sig = b[i + TOKEN.len()..]
                    .iter()
                    .copied()
                    .find(|x| !x.is_ascii_whitespace())
                    .unwrap_or(0);
                let shorthand = stack.last() == Some(&b'{')
                    && matches!(last_sig, b'{' | b',')
                    && matches!(next_sig, b'}' | b',');
                if shorthand {
                    out.extend_from_slice(format!("\"__loc__\": {obj}").as_bytes());
                } else {
                    out.extend_from_slice(format!("({obj})").as_bytes());
                }
                last_sig = b')';
                i += TOKEN.len();
                continue;
            }
            _ => {}
        }
        out.push(c);
        if !c.is_ascii_whitespace() {
            last_sig = c;
        }
        i += 1;
    }
    // Only ASCII was inserted and input bytes were copied whole.
    Cow::Owned(String::from_utf8(out).unwrap_or_else(|_| src.to_string()))
}

#[cfg(test)]
mod tests {
    use super::expand_loc;

    #[test]
    fn replaces_code_occurrences_with_line() {
        assert_eq!(
            expand_loc("1\n| $__loc__.line"),
            "1\n| ({\"file\":\"<top-level>\",\"line\":2}).line"
        );
    }

    #[test]
    fn leaves_strings_comments_and_longer_names() {
        let src = "\"$__loc__\", $__loc__x # $__loc__";
        assert_eq!(expand_loc(src), src);
    }

    #[test]
    fn expands_inside_interpolation() {
        assert_eq!(
            expand_loc("\"\\($__loc__)\""),
            "\"\\(({\"file\":\"<top-level>\",\"line\":1}))\""
        );
    }

    #[test]
    fn object_shorthand_becomes_pair() {
        assert_eq!(
            expand_loc("{a: 1, $__loc__}"),
            "{a: 1, \"__loc__\": {\"file\":\"<top-level>\",\"line\":1}}"
        );
    }
}
