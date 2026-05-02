//! fancy-regex backed implementations of jq's `matches`, `split_matches`,
//! and `split_` natives.
//!
//! Replaces jaq-std's `regex` crate backend so patterns with lookahead
//! `(?=...)`, lookbehind `(?<=...)`, atomic groups `(?>...)`, and
//! backreferences `\1` work — bringing jq closer to real jq's Oniguruma
//! engine without an FFI dependency. fancy-regex is a pure-Rust crate
//! that wraps `regex` for the simple cases and falls back to a VM for
//! features `regex` doesn't support.
//!
//! Important decisions:
//!  - Output shape is byte-for-byte the same as jaq-std's regex natives,
//!    so the user-facing `match`/`scan`/`test`/`capture`/`sub`/`gsub`
//!    filters (defined in compat.rs and jaq-std's defs.jq) consume our
//!    output unchanged.
//!  - Each capture object is `{offset, length, string, [name]}`. Offsets
//!    and lengths are CHARACTER counts (matches jq docs), even though
//!    fancy-regex returns byte indices.
//!  - Flags: `g` (global), `n` (ignore empty), `i` (case-insensitive),
//!    `m` (multi-line), `s` (dot-all), `x` (extended), `p` (= m+s).
//!    `l` (swap-greed) is silently no-op — fancy-regex does not expose it.
//!  - Each iteration of `captures_iter` returns `Result` because fancy-regex
//!    can fail at runtime (backtracking limits, etc.). We map those to a
//!    short jq-style error.

use fancy_regex::{Regex as FRegex, RegexBuilder as FRegexBuilder};
use jaq_core::native::{Filter, bome, v};
use jaq_core::{Cv, DataT, Error, RunPtr, ValR};
use jaq_std::ValT;

#[derive(Clone, Copy, Default)]
struct Flags {
    /// global search
    g: bool,
    /// ignore empty matches
    n: bool,
    /// case-insensitive
    i: bool,
    /// multi-line: ^/$ match line boundaries
    m: bool,
    /// single-line / dot-all: `.` matches `\n`
    s: bool,
    /// extended: ignore whitespace and `#` comments
    x: bool,
}

impl Flags {
    fn parse(s: &str) -> Result<Self, char> {
        let mut out = Self::default();
        for c in s.chars() {
            match c {
                'g' => out.g = true,
                'n' => out.n = true,
                'i' => out.i = true,
                'm' => out.m = true,
                's' => out.s = true,
                'x' => out.x = true,
                'l' => {} // swap_greed: not exposed by fancy-regex; silently no-op.
                'p' => {
                    out.m = true;
                    out.s = true;
                }
                c => return Err(c),
            }
        }
        Ok(out)
    }

    fn build(&self, pattern: &str) -> Result<FRegex, fancy_regex::Error> {
        let mut b = FRegexBuilder::new(pattern);
        b.case_insensitive(self.i)
            .multi_line(self.m)
            .dot_matches_new_line(self.s)
            .ignore_whitespace(self.x);
        b.build()
    }
}

/// Mapping from byte offset to character index. fancy-regex reports byte
/// offsets, but jq's `match`/`scan` output uses character indices.
struct ByteCharMap<'a> {
    text: &'a str,
    last_byte: usize,
    last_char: usize,
}

impl<'a> ByteCharMap<'a> {
    fn new(text: &'a str) -> Self {
        Self {
            text,
            last_byte: 0,
            last_char: 0,
        }
    }

    /// Convert a byte offset to a character index. Byte offsets must arrive
    /// in monotonically non-decreasing order across calls (which they do for
    /// regex iteration).
    fn byte_to_char(&mut self, byte_offset: usize) -> usize {
        if byte_offset < self.last_byte {
            // Defensive fallback: out-of-order call. Recompute from start.
            self.last_byte = 0;
            self.last_char = 0;
        }
        let slice = &self.text[self.last_byte..byte_offset];
        self.last_char += slice.chars().count();
        self.last_byte = byte_offset;
        self.last_char
    }
}

/// Internal regex engine. Mirrors the (split, match) flag tuple from
/// jaq-std::regex::regex.
fn run_regex<V: ValT>(
    text: &str,
    re: &FRegex,
    flags: Flags,
    sm: (bool, bool),
    sub: impl Fn(&str) -> V,
) -> Result<V, Error<V>> {
    let (mi, ma) = sm;
    let mut bc = ByteCharMap::new(text);
    let mut last_byte = 0usize;
    let mut out: Vec<V> = Vec::new();

    for cap_result in re.captures_iter(text) {
        let caps = cap_result.map_err(|e| Error::str(format_args!("regex error: {e}")))?;
        let whole = caps
            .get(0)
            .expect("captures always include the whole match at index 0");
        if flags.n && whole.range().is_empty() {
            continue;
        }
        if mi {
            out.push(sub(&text[last_byte..whole.start()]));
            last_byte = whole.end();
        }
        if ma {
            let names: Vec<Option<&str>> = re.capture_names().collect();
            let mut match_objs: Vec<V> = Vec::with_capacity(names.len());
            for (idx, name) in names.into_iter().enumerate() {
                let Some(m) = caps.get(idx) else {
                    // Optional group that didn't match — skip, matching jaq's
                    // filter_map behavior.
                    continue;
                };
                let offset = bc.byte_to_char(m.start());
                let length = m.as_str().chars().count();
                let mut fields: Vec<(V, V)> = vec![
                    (V::from(String::from("offset")), V::from(offset as isize)),
                    (V::from(String::from("length")), V::from(length as isize)),
                    (V::from(String::from("string")), sub(m.as_str())),
                ];
                if let Some(n) = name {
                    fields.push((V::from(String::from("name")), V::from(n.to_string())));
                }
                match_objs.push(V::from_map(fields)?);
            }
            // Collect into a Val Array (V impls FromIterator<V>).
            let arr: V = match_objs.into_iter().collect();
            out.push(arr);
        }
        if !flags.g {
            break;
        }
    }
    if mi {
        out.push(sub(&text[last_byte..]));
    }
    Ok(out.into_iter().collect())
}

/// Shared body for matches/split_matches/split_ — pops flags then pattern
/// from cv vars, runs the regex against cv input.
fn re_native<'a, D: DataT>(s: bool, m: bool, mut cv: Cv<'a, D>) -> ValR<D::V<'a>>
where
    D::V<'a>: ValT,
{
    let flags_v = cv.0.pop_var();
    let pat_v = cv.0.pop_var();

    let flag_bytes = flags_v.try_as_utf8_bytes()?;
    let flag_str = core::str::from_utf8(flag_bytes)
        .map_err(|_| Error::str(format_args!("invalid UTF-8 in regex flags")))?;
    let flags =
        Flags::parse(flag_str).map_err(|c| Error::str(format_args!("invalid regex flag: {c}")))?;

    let pat_bytes = pat_v.try_as_utf8_bytes()?;
    let pat_str = core::str::from_utf8(pat_bytes)
        .map_err(|_| Error::str(format_args!("invalid UTF-8 in regex pattern")))?;
    let re = flags
        .build(pat_str)
        .map_err(|e| Error::str(format_args!("invalid regex: {e}")))?;

    let in_bytes = cv.1.try_as_utf8_bytes()?;
    let text = core::str::from_utf8(in_bytes)
        .map_err(|_| Error::str(format_args!("invalid UTF-8 input to regex")))?;

    let input = cv.1.clone();
    let sub = move |x: &str| input.as_sub_str(x.as_bytes());

    run_regex::<D::V<'a>>(text, &re, flags, (s, m), sub)
}

/// Names of jaq-std natives we shadow. mod.rs filters these out before
/// chaining ours.
pub(super) const SHADOWED_NATIVE_NAMES: &[&str] = &["matches", "split_matches", "split_"];

/// Our fancy-regex backed natives. Drop-in replacement for jaq-std's
/// (filtered out by name in mod.rs). Uses non-capturing closures so each
/// element coerces cleanly to the HRTB `RunPtr<D>` fn pointer (mirrors
/// jaq-std's own pattern).
pub(super) fn funs<D: DataT>() -> Box<[Filter<RunPtr<D>>]>
where
    for<'a> D::V<'a>: ValT,
{
    Box::new([
        ("matches", v(2), |cv| bome(re_native(false, true, cv))),
        ("split_matches", v(2), |cv| bome(re_native(true, true, cv))),
        ("split_", v(2), |cv| bome(re_native(true, false, cv))),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_parse_known() {
        assert!(Flags::parse("gimsxn").is_ok());
        assert!(Flags::parse("p").is_ok());
        assert!(Flags::parse("l").is_ok()); // silently no-op
        assert!(Flags::parse("").is_ok());
    }

    #[test]
    fn flags_parse_unknown() {
        assert_eq!(Flags::parse("z").err(), Some('z'));
    }

    #[test]
    fn flags_p_implies_m_and_s() {
        let f = Flags::parse("p").unwrap();
        assert!(f.m);
        assert!(f.s);
    }

    #[test]
    fn build_simple_pattern() {
        let f = Flags::default();
        assert!(f.build("abc").is_ok());
    }

    #[test]
    fn build_lookahead() {
        let f = Flags::default();
        // (?=...) is a positive lookahead — `regex` rejects, fancy-regex accepts.
        assert!(f.build(r"foo(?=bar)").is_ok());
    }

    #[test]
    fn build_lookbehind() {
        let f = Flags::default();
        assert!(f.build(r"(?<=foo)bar").is_ok());
    }

    #[test]
    fn build_backreference() {
        let f = Flags::default();
        assert!(f.build(r"(\w+) \1").is_ok());
    }

    #[test]
    fn build_atomic_group() {
        let f = Flags::default();
        assert!(f.build(r"(?>abc|abd)d").is_ok());
    }

    #[test]
    fn build_invalid_returns_error() {
        let f = Flags::default();
        assert!(f.build(r"(unbalanced").is_err());
    }

    #[test]
    fn byte_to_char_ascii() {
        let mut bc = ByteCharMap::new("hello");
        assert_eq!(bc.byte_to_char(0), 0);
        assert_eq!(bc.byte_to_char(3), 3);
    }

    #[test]
    fn byte_to_char_multi_byte() {
        // "héllo": h=1B, é=2B, l=1B, l=1B, o=1B → byte 3 = char 2
        let mut bc = ByteCharMap::new("héllo");
        assert_eq!(bc.byte_to_char(0), 0);
        assert_eq!(bc.byte_to_char(3), 2); // start of second 'l'
    }

    #[test]
    fn byte_to_char_handles_out_of_order() {
        // Defensive — recomputes from start.
        let mut bc = ByteCharMap::new("héllo");
        let _ = bc.byte_to_char(3);
        assert_eq!(bc.byte_to_char(1), 1); // 'é' starts at byte 1, char 1
    }
}
