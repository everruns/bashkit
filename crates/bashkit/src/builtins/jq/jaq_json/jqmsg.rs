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

/// jq dumps an operand into a 15-byte buffer: longer dumps keep 11 bytes
/// and end in `...`.
fn dump_trunc(v: &Val) -> String {
    let mut s = String::new();
    // Render at most a little past the cut, whatever the value's size.
    for c in v.to_string().chars() {
        s.push(c);
        if s.len() > 14 {
            break;
        }
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
        Val::BStr(_) | Val::TStr(_) => std::format!("string {}", r),
        other => type_name(other).to_string(),
    };
    Error::str(std::format!("Cannot index {} with {right}", type_name(&l)))
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
