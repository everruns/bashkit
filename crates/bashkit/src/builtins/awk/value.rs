//! awk values: numbers, strings, strnums and the uninitialized value.
//!
//! Decisions (POSIX awk "Expressions in awk", gawk 5 behavior):
//! - Input-derived text (fields, `getline` vars, `split` elements, `ARGV`,
//!   `ENVIRON`, `-v`/operand assignments) is a *strnum*: it compares
//!   numerically when it looks like a number, as a string otherwise.
//! - String to number uses the C `strtod` prefix rule (`"12abc"` is 12,
//!   `"0x1A"` is 0: hex is only recognized in program source).
//! - Number to string: integral values print as integers, others through
//!   `CONVFMT` (or `OFMT` for `print`).
//! - String comparison is bytewise (C locale collation).

use std::cmp::Ordering;

use super::format::format_number;

#[derive(Debug, Clone, PartialEq)]
pub(super) enum Value {
    Num(f64),
    Str(String),
    /// Input text that looks numeric: compares as a number, prints as text.
    StrNum(String, f64),
    Uninit,
}

impl Value {
    /// Classify input-derived text as strnum or plain string.
    pub(super) fn from_input(s: String) -> Value {
        match looks_numeric(&s) {
            Some(n) => Value::StrNum(s, n),
            None => Value::Str(s),
        }
    }

    pub(super) fn to_num(&self) -> f64 {
        match self {
            Value::Num(n) | Value::StrNum(_, n) => *n,
            Value::Str(s) => str_to_num(s),
            Value::Uninit => 0.0,
        }
    }

    /// String form using `CONVFMT` for non-integral numbers.
    pub(super) fn to_str_fmt(&self, convfmt: &str) -> String {
        match self {
            Value::Num(n) => format_number(*n, convfmt),
            Value::Str(s) | Value::StrNum(s, _) => s.clone(),
            Value::Uninit => String::new(),
        }
    }

    pub(super) fn to_bool(&self) -> bool {
        match self {
            Value::Num(n) => *n != 0.0,
            Value::StrNum(_, n) => *n != 0.0,
            Value::Str(s) => !s.is_empty(),
            Value::Uninit => false,
        }
    }

    /// Bytes this value keeps on the heap (TM-DOS-110 accounting).
    pub(super) fn heap_bytes(&self) -> usize {
        match self {
            Value::Str(s) | Value::StrNum(s, _) => s.len(),
            _ => 0,
        }
    }

    pub(super) fn is_numeric(&self) -> bool {
        matches!(self, Value::Num(_) | Value::StrNum(..) | Value::Uninit)
    }

    /// `typeof()` name.
    pub(super) fn type_name(&self) -> &'static str {
        match self {
            Value::Num(_) => "number",
            Value::Str(_) => "string",
            Value::StrNum(..) => "strnum",
            Value::Uninit => "untyped",
        }
    }
}

/// Compare two scalars by the POSIX rules: numeric when both are numeric
/// (numbers, numeric-looking input, uninitialized), string otherwise.
pub(super) fn compare(a: &Value, b: &Value, convfmt: &str) -> Ordering {
    if a.is_numeric() && b.is_numeric() {
        let (x, y) = (a.to_num(), b.to_num());
        // NaN compares unequal to everything; treat as equal-to-nothing.
        return x.partial_cmp(&y).unwrap_or(if x.is_nan() && y.is_nan() {
            Ordering::Equal
        } else if x.is_nan() {
            Ordering::Greater
        } else {
            Ordering::Less
        });
    }
    let (x, y) = (a.to_str_fmt(convfmt), b.to_str_fmt(convfmt));
    x.as_bytes().cmp(y.as_bytes())
}

/// Length of the longest prefix of `s` that `strtod` would consume, with
/// its value. Leading blanks are skipped. Returns `(0, 0.0)` for none.
pub(super) fn strtod_prefix(s: &str) -> (usize, f64) {
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() && matches!(b[i], b' ' | b'\t' | b'\n' | b'\r' | b'\x0b' | b'\x0c') {
        i += 1;
    }
    let start = i;
    if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
        i += 1;
    }
    // gawk accepts +inf/-inf/+nan/-nan only with an explicit sign.
    if i > start {
        let rest = &s[i..];
        for (word, val) in [("inf", f64::INFINITY), ("nan", f64::NAN)] {
            if rest.len() >= 3 && rest[..3].eq_ignore_ascii_case(word) {
                let neg = b[start] == b'-';
                let mut len = i + 3;
                if word == "inf" && rest.len() >= 8 && rest[..8].eq_ignore_ascii_case("infinity") {
                    len = i + 8;
                }
                let v = if neg { -val } else { val };
                return (len, v);
            }
        }
    }
    let digits_start = i;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    let mut mantissa_digits = i - digits_start;
    if i < b.len() && b[i] == b'.' {
        let dot = i;
        i += 1;
        let frac_start = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        mantissa_digits += i - frac_start;
        if mantissa_digits == 0 {
            i = dot;
        }
    }
    if mantissa_digits == 0 {
        return (0, 0.0);
    }
    if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
        let mut j = i + 1;
        if j < b.len() && (b[j] == b'+' || b[j] == b'-') {
            j += 1;
        }
        let exp_start = j;
        while j < b.len() && b[j].is_ascii_digit() {
            j += 1;
        }
        if j > exp_start {
            i = j;
        }
    }
    let v = s[start..i].parse::<f64>().unwrap_or(0.0);
    (i, v)
}

/// awk string-to-number: the `strtod` prefix, 0 when there is none.
pub(super) fn str_to_num(s: &str) -> f64 {
    strtod_prefix(s).1
}

/// `Some(n)` when the whole of `s` (blanks around allowed) is a number.
pub(super) fn looks_numeric(s: &str) -> Option<f64> {
    let (len, v) = strtod_prefix(s);
    if len == 0 {
        return None;
    }
    if s[len..]
        .bytes()
        .all(|c| matches!(c, b' ' | b'\t' | b'\n' | b'\r' | b'\x0b' | b'\x0c'))
    {
        Some(v)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strtod_prefix_rules() {
        assert_eq!(str_to_num("12abc"), 12.0);
        assert_eq!(str_to_num("  -3.5e2x"), -350.0);
        assert_eq!(str_to_num("0x1A"), 0.0);
        assert_eq!(str_to_num(".5"), 0.5);
        assert_eq!(str_to_num("."), 0.0);
        assert_eq!(str_to_num("1e"), 1.0);
        assert_eq!(str_to_num("inf"), 0.0);
        assert_eq!(str_to_num("-inf"), f64::NEG_INFINITY);
        assert!(str_to_num("+nan").is_nan());
    }

    #[test]
    fn strnum_classification() {
        assert!(looks_numeric(" 10 ").is_some());
        assert!(looks_numeric("10a").is_none());
        assert!(looks_numeric("").is_none());
        assert!(looks_numeric("007").is_some());
    }

    #[test]
    fn comparison_rules() {
        let n = |x| Value::Num(x);
        let s = |x: &str| Value::Str(x.into());
        let i = |x: &str| Value::from_input(x.into());
        assert_eq!(compare(&i("10"), &i("9"), "%.6g"), Ordering::Greater);
        assert_eq!(compare(&s("10"), &s("9"), "%.6g"), Ordering::Less);
        assert_eq!(compare(&i("10"), &n(9.0), "%.6g"), Ordering::Greater);
        assert_eq!(compare(&Value::Uninit, &n(0.0), "%.6g"), Ordering::Equal);
        assert_eq!(compare(&Value::Uninit, &s(""), "%.6g"), Ordering::Equal);
    }
}
