// BASHKIT PATCH: bashkit-only module (not from upstream jaq-json).
//! jq's wording for value errors, so `try ... catch .` sees the same text as
//! jq (`string ("a") and number (1) cannot be added`).

use super::{Num, Val};
use jaq_core::{ops, Error};
use std::string::{String, ToString};

fn type_name(v: &Val) -> &'static str {
    match v {
        Val::Null => "null",
        Val::Bool(_) => "boolean",
        Val::Num(_) => "number",
        Val::BStr(_) | Val::TStr(_) => "string",
        Val::Arr(_) => "array",
        Val::Obj(_) => "object",
    }
}

/// Preview of a value for error messages, mirroring jq's 15-byte dump
/// buffer: longer dumps keep 11 bytes and end in `...`.
///
/// The cut is applied *while* rendering through a capped sink, so a huge
/// value never materializes its full dump as an intermediate string.
/// (Truncating after `v.to_string()` would still transiently allocate the
/// whole rendering outside the memory meter and the 64 KiB diagnostic cap.)
fn dump_trunc(v: &Val) -> String {
    /// `fmt::Write` sink that keeps at most `cap` bytes; excess is dropped.
    struct Capped {
        buf: String,
        cap: usize,
        over: bool,
    }
    impl std::fmt::Write for Capped {
        fn write_str(&mut self, s: &str) -> std::fmt::Result {
            if self.buf.len() >= self.cap {
                self.over = true;
                return Ok(());
            }
            let room = self.cap - self.buf.len();
            let mut end = s.len().min(room);
            while !s.is_char_boundary(end) {
                end -= 1;
            }
            self.buf.push_str(&s[..end]);
            if end < s.len() {
                self.over = true;
            }
            Ok(())
        }
    }
    use std::fmt::Write as _;
    // One byte past the 14-byte cut: enough to tell "fits" from "truncate",
    // so small-value output is byte-identical to the old truncation.
    let mut out = Capped { buf: String::new(), cap: 15, over: false };
    let _ = write!(out, "{v}");
    let mut s = out.buf;
    if !out.over && s.len() <= 14 {
        return s;
    }
    if s.len() > 14 {
        let mut end = 11;
        while !s.is_char_boundary(end) {
            end -= 1;
        }
        s.truncate(end);
        s.push_str("...");
    }
    s
}

fn described(v: &Val) -> String {
    std::format!("{} ({})", type_name(v), dump_trunc(v))
}

/// `l op r` failed; `zero` marks a zero divisor.
pub(crate) fn math(l: Val, op: ops::Math, r: Val, zero: bool) -> Error<Val> {
    let verb = match op {
        ops::Math::Add => "added",
        ops::Math::Sub => "subtracted",
        ops::Math::Mul => "multiplied",
        ops::Math::Div => "divided",
        ops::Math::Rem => "divided (remainder)",
    };
    let why = if zero { " because the divisor is zero" } else { "" };
    Error::str(std::format!(
        "{} and {} cannot be {verb}{why}",
        described(&l),
        described(&r)
    ))
}

/// `l[r]` is not defined.
pub(crate) fn index(l: Val, r: Val) -> Error<Val> {
    let right = match &r {
        // Bound the key preview: interpolating the raw string would embed
        // arbitrarily large input in the error text.
        Val::BStr(_) | Val::TStr(_) => std::format!("string {}", dump_trunc(&r)),
        other => type_name(other).to_string(),
    };
    Error::str(std::format!("Cannot index {} with {right}", type_name(&l)))
}

/// `.[]` over a value that is not an array or object.
pub(crate) fn iter(v: Val) -> Error<Val> {
    Error::str(std::format!("Cannot iterate over {}", described(&v)))
}

/// `{(k): v}` with a key that is not a string.
pub(crate) fn object_key(k: &Val) -> Error<Val> {
    Error::str(std::format!("Cannot use {} as object key", described(k)))
}

pub(crate) fn is_zero(v: &Val) -> bool {
    matches!(v, Val::Num(n) if n.as_f64() == 0.0)
}

/// jq's `%` works on integers: fractional operands are truncated
/// (saturating at the 64-bit range).
pub(crate) fn trunc_int(n: &Num) -> Num {
    if n.is_int() {
        return n.clone();
    }
    let f = n.as_f64();
    if f.is_nan() {
        return Num::Int(0);
    }
    Num::Int(f.trunc() as i64 as isize)
}

#[cfg(test)]
mod tests {
    use super::super::metered;
    use super::*;

    fn huge_array_key() -> Val {
        let body: Vec<Val> = (0..200_000).map(|i| Val::Num(Num::Int(i as isize))).collect();
        Val::Arr(metered(body))
    }

    #[test]
    fn object_key_error_stays_small_for_huge_key() {
        let err = object_key(&huge_array_key()).to_string();
        assert!(err.len() < 4096, "object-key error materialized {} bytes", err.len());
    }

    #[test]
    fn index_error_stays_small_for_huge_string() {
        let l = Val::Arr(metered(Vec::new()));
        let r = Val::utf8_str("y".repeat(200_000));
        let err = index(l, r).to_string();
        assert!(err.len() < 4096, "index error materialized {} bytes", err.len());
    }
}
