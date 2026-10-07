//! jq's destructuring alternatives: `T as P1 ?// P2 | BODY`.
//!
//! Important decisions:
//!  - jaq does not parse `?//`, so the filter text is rewritten before it is
//!    compiled, the same way as `$__loc__` (see `loc.rs`):
//!
//!    ```text
//!    T as $alt | . as $dot | null as $v1 | ... |
//!      try ($alt as P1 | BODY)
//!      catch ($dot | $alt as P2 | BODY)
//!    ```
//!
//!    That is jq's rule: every variable of every pattern is bound (null when
//!    its pattern did not bind it), an error in a pattern or in BODY moves on
//!    to the next pattern, and the last pattern's error propagates.
//!  - BODY is copied once per pattern. The rewrite runs at most
//!    `MAX_REWRITES` times so nested alternatives cannot blow up the
//!    filter; anything left over fails to compile as before.
//!  - Strings and comments are masked before searching, so `?//` inside
//!    them is left alone. Forms the scanner cannot place (`reduce`/
//!    `foreach` with alternatives) are left unchanged.

use std::borrow::Cow;

const MAX_REWRITES: usize = 16;

/// Copy of `src` with string literals and comments blanked out (same
/// length; newlines kept), so structural searches only see code.
fn mask(src: &str) -> Vec<u8> {
    let b = src.as_bytes();
    let mut m = b.to_vec();
    let mut i = 0;
    let mut in_str = false;
    while i < b.len() {
        let c = b[i];
        if in_str {
            if c == b'\\' && i + 1 < b.len() {
                m[i] = b' ';
                m[i + 1] = b' ';
                i += 2;
                continue;
            }
            if c == b'"' {
                in_str = false;
            } else if c != b'\n' {
                m[i] = b' ';
            }
        } else if c == b'"' {
            in_str = true;
        } else if c == b'#' {
            while i < b.len() && b[i] != b'\n' {
                m[i] = b' ';
                i += 1;
            }
            continue;
        }
        i += 1;
    }
    m
}

fn is_ident(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

fn word_at(m: &[u8], i: usize, w: &str) -> bool {
    let w = w.as_bytes();
    m[i..].starts_with(w)
        && (i == 0 || !is_ident(m[i - 1]) && m[i - 1] != b'$')
        && m.get(i + w.len()).is_none_or(|&c| !is_ident(c))
}

/// One rewrite of the first `?//`, or None when there is none to rewrite.
fn rewrite_once(src: &str, n: usize) -> Option<String> {
    let m = mask(src);
    let alt = (0..m.len().saturating_sub(2)).find(|&i| m[i..].starts_with(b"?//"))?;

    // Back to the `as` that starts the patterns, at the same depth.
    let mut depth = 0i32;
    let mut as_pos = None;
    let mut i = alt;
    while i > 0 {
        i -= 1;
        match m[i] {
            b')' | b']' | b'}' => depth += 1,
            b'(' | b'[' | b'{' => {
                depth -= 1;
                if depth < 0 {
                    return None;
                }
            }
            b'a' if depth == 0 && word_at(&m, i, "as") => {
                as_pos = Some(i);
                break;
            }
            _ => {}
        }
    }
    let as_pos = as_pos?;

    // Patterns, split on `?//`, up to the `|` that starts the body.
    let mut patterns = Vec::new();
    let mut start = as_pos + 2;
    let mut depth = 0i32;
    let mut i = start;
    let pipe = loop {
        let c = *m.get(i)?;
        match c {
            b'(' if depth == 0 => return None,
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => {
                depth -= 1;
                if depth < 0 {
                    return None;
                }
            }
            b'?' if depth == 0 && m[i..].starts_with(b"?//") => {
                patterns.push(src[start..i].trim());
                start = i + 3;
                i += 3;
                continue;
            }
            b'|' if depth == 0 && m.get(i + 1) != Some(&b'=') => {
                patterns.push(src[start..i].trim());
                break i;
            }
            _ => {}
        }
        i += 1;
    };
    if patterns.len() < 2 || patterns.iter().any(|p| p.is_empty()) {
        return None;
    }

    // The body runs to an unmatched closer, or `;`/`then`/`elif`/`else`/
    // `end`/`catch` outside any bracket or `if`.
    let body_start = pipe + 1;
    let mut depth = 0i32;
    let mut ifs = 0i32;
    let mut i = body_start;
    let body_end = loop {
        let Some(&c) = m.get(i) else { break m.len() };
        match c {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => {
                if depth == 0 {
                    break i;
                }
                depth -= 1;
            }
            b';' if depth == 0 && ifs == 0 => break i,
            b'i' if word_at(&m, i, "if") => ifs += 1,
            b'e' if word_at(&m, i, "end") => {
                if depth == 0 && ifs == 0 {
                    break i;
                }
                ifs -= 1;
            }
            _ if depth == 0
                && ifs == 0
                && ["then", "elif", "else", "catch"]
                    .iter()
                    .any(|w| word_at(&m, i, w)) =>
            {
                break i;
            }
            _ => {}
        }
        i += 1;
    };
    let body = &src[body_start..body_end];

    // Every `$name` in any pattern is bound, null by default.
    let mut vars: Vec<&str> = Vec::new();
    for p in &patterns {
        let pb = p.as_bytes();
        let pm = mask(p);
        let mut j = 0;
        while j < pb.len() {
            if pm[j] == b'$' {
                let e = (j + 1..pb.len())
                    .find(|&k| !is_ident(pb[k]))
                    .unwrap_or(pb.len());
                let v = &p[j..e];
                if v.len() > 1 && !vars.contains(&v) {
                    vars.push(v);
                }
                j = e;
            } else {
                j += 1;
            }
        }
    }

    let alt_var = format!("$__bk_alt{n}__");
    let dot_var = format!("$__bk_dot{n}__");
    let mut chain = format!("{alt_var} as {} |{body}", patterns[patterns.len() - 1]);
    for p in patterns[..patterns.len() - 1].iter().rev() {
        chain = format!("try ({alt_var} as {p} |{body}) catch ({dot_var} | {chain})");
    }
    let nulls: String = vars.iter().map(|v| format!("null as {v} | ")).collect();
    Some(format!(
        "{}as {alt_var} | . as {dot_var} | {nulls}({chain}){}",
        &src[..as_pos],
        &src[body_end..]
    ))
}

/// Rewrite every `as P1 ?// P2 | BODY` in the filter (see module docs).
pub(super) fn expand_alternatives(src: &str) -> Cow<'_, str> {
    if !src.contains("?//") {
        return Cow::Borrowed(src);
    }
    let mut cur = src.to_string();
    for n in 0..MAX_REWRITES {
        match rewrite_once(&cur, n) {
            Some(next) => cur = next,
            None => break,
        }
    }
    Cow::Owned(cur)
}

#[cfg(test)]
mod tests {
    use super::expand_alternatives;

    #[test]
    fn leaves_filters_without_alternatives() {
        assert_eq!(expand_alternatives(". as [$a] | $a"), ". as [$a] | $a");
        assert_eq!(expand_alternatives("\"?//\""), "\"?//\"");
    }

    #[test]
    fn rewrites_two_patterns() {
        let out = expand_alternatives(".[] as [$x, $y] ?// $x | [$x, $y]");
        assert_eq!(
            out,
            ".[] as $__bk_alt0__ | . as $__bk_dot0__ | null as $x | null as $y | \
             (try ($__bk_alt0__ as [$x, $y] | [$x, $y]) catch ($__bk_dot0__ | \
             $__bk_alt0__ as $x | [$x, $y]))"
        );
    }

    #[test]
    fn body_stops_at_closing_paren() {
        let out = expand_alternatives("(. as [$a] ?// $a | $a), 2");
        assert!(out.ends_with("| $a))), 2"), "{out}");
    }
}
