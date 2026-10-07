//! fnmatch-style pattern matching for `find -name`/`-path`, `glob` and `ls`.
//!
//! Decision: no FNM_PATHNAME / FNM_PERIOD, matching GNU find: `*` and `?`
//! match `/` and a leading `.`. Supports `[...]` (ranges, `!`/`^` negation,
//! POSIX classes) and backslash escapes.
//!
//! THREAT[TM-DOS-031]: `*` uses a single backtracking restore point, so the
//! worst case is O(value.len * pattern.len), never exponential.

/// Match `value` against shell `pattern` (case-sensitive).
pub(crate) fn glob_match(value: &str, pattern: &str) -> bool {
    fnmatch(value, pattern, false)
}

/// Match `value` against shell `pattern`, optionally ignoring ASCII/Unicode case.
pub(crate) fn fnmatch(value: &str, pattern: &str, nocase: bool) -> bool {
    let v: Vec<char> = value.chars().collect();
    let p: Vec<char> = pattern.chars().collect();
    let (mut vi, mut pi) = (0usize, 0usize);
    // (pattern index after the star, value index the star currently covers up to)
    let mut star: Option<(usize, usize)> = None;
    while vi < v.len() {
        if pi < p.len() {
            if p[pi] == '*' {
                while pi < p.len() && p[pi] == '*' {
                    pi += 1;
                }
                star = Some((pi, vi));
                continue;
            }
            if let Some(next) = match_one(&p, pi, v[vi], nocase) {
                pi = next;
                vi += 1;
                continue;
            }
        }
        match star {
            Some((sp, sv)) => {
                pi = sp;
                vi = sv + 1;
                star = Some((sp, sv + 1));
            }
            None => return false,
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

fn chars_eq(a: char, b: char, nocase: bool) -> bool {
    a == b || (nocase && a.to_lowercase().eq(b.to_lowercase()))
}

/// Match one non-star pattern token at `pi` against `c`; returns the index
/// after the token on success.
fn match_one(p: &[char], pi: usize, c: char, nocase: bool) -> Option<usize> {
    match p[pi] {
        '?' => Some(pi + 1),
        '[' => match match_bracket(p, pi, c, nocase) {
            Some((true, next)) => Some(next),
            Some((false, _)) => None,
            // No closing bracket: `[` is literal.
            None => chars_eq('[', c, nocase).then_some(pi + 1),
        },
        '\\' if pi + 1 < p.len() => chars_eq(p[pi + 1], c, nocase).then_some(pi + 2),
        lit => chars_eq(lit, c, nocase).then_some(pi + 1),
    }
}

fn class_matches(name: &str, c: char) -> Option<bool> {
    Some(match name {
        "alpha" => c.is_alphabetic(),
        "digit" => c.is_ascii_digit(),
        "alnum" => c.is_alphanumeric(),
        "upper" => c.is_uppercase(),
        "lower" => c.is_lowercase(),
        "space" => c.is_whitespace(),
        "blank" => c == ' ' || c == '\t',
        "punct" => c.is_ascii_punctuation(),
        "print" => !c.is_control(),
        "graph" => !c.is_control() && !c.is_whitespace(),
        "cntrl" => c.is_control(),
        "xdigit" => c.is_ascii_hexdigit(),
        _ => return None,
    })
}

/// Evaluate a bracket expression starting at `p[start] == '['`.
/// Returns `(matched, index_after_bracket)`, or None if unterminated.
fn match_bracket(p: &[char], start: usize, c: char, nocase: bool) -> Option<(bool, usize)> {
    let mut i = start + 1;
    let negate = matches!(p.get(i), Some('!' | '^'));
    if negate {
        i += 1;
    }
    let mut matched = false;
    let mut first = true;
    while i < p.len() {
        let ch = p[i];
        if ch == ']' && !first {
            return Some((matched != negate, i + 1));
        }
        first = false;
        if ch == '[' && p.get(i + 1) == Some(&':') {
            let rest: String = p[i + 2..].iter().collect();
            if let Some(end) = rest.find(":]") {
                let name = &rest[..end];
                if let Some(m) = class_matches(name, c) {
                    matched |= m
                        || (nocase
                            && (class_matches(name, c.to_ascii_lowercase()) == Some(true)
                                || class_matches(name, c.to_ascii_uppercase()) == Some(true)));
                    i += 2 + name.chars().count() + 2;
                    continue;
                }
            }
        }
        let (lo, mut next) = if ch == '\\' && i + 1 < p.len() {
            (p[i + 1], i + 2)
        } else {
            (ch, i + 1)
        };
        if p.get(next) == Some(&'-') && p.get(next + 1).is_some_and(|&e| e != ']') {
            let (hi, after) = if p[next + 1] == '\\' && next + 2 < p.len() {
                (p[next + 2], next + 3)
            } else {
                (p[next + 1], next + 2)
            };
            let in_range = |x: char| lo <= x && x <= hi;
            matched |= in_range(c)
                || (nocase && (c.to_lowercase().any(in_range) || c.to_uppercase().any(in_range)));
            next = after;
        } else {
            matched |= chars_eq(lo, c, nocase);
        }
        i = next;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basics() {
        assert!(fnmatch("file.txt", "*.txt", false));
        assert!(fnmatch(".hidden", "*", false));
        assert!(fnmatch("a/b/c", "a*c", false));
        assert!(!fnmatch("file.md", "*.txt", false));
        assert!(fnmatch("x", "?", false));
        assert!(!fnmatch("", "?", false));
    }

    #[test]
    fn brackets() {
        assert!(fnmatch("a1", "a[0-9]", false));
        assert!(!fnmatch("ab", "a[0-9]", false));
        assert!(fnmatch("ab", "a[!0-9]", false));
        assert!(fnmatch("a]", "a[]]", false));
        assert!(fnmatch("aZ", "a[[:upper:]]", false));
        assert!(fnmatch("[x", "[x", false));
        assert!(fnmatch("a-", "a[x-]", false));
    }

    #[test]
    fn escapes_and_case() {
        assert!(fnmatch("a*b", "a\\*b", false));
        assert!(!fnmatch("axb", "a\\*b", false));
        assert!(fnmatch("README.MD", "*.md", true));
        assert!(!fnmatch("README.MD", "*.md", false));
        assert!(fnmatch("B", "[a-c]", true));
    }

    #[test]
    fn many_stars_are_linear() {
        let value = "a".repeat(5000);
        let pattern = format!("{}b", "*a".repeat(50));
        assert!(!fnmatch(&value, &pattern, false));
    }
}
