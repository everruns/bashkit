//! awk number formatting and `printf`/`sprintf`.
//!
//! Decisions (gawk 5 behavior, C `printf` semantics):
//! - `print`/string conversion of an integral number prints all its digits
//!   (`2^70` is `1180591620717411303424`), never through `OFMT`/`CONVFMT`.
//! - Infinities and NaN print as `+inf`, `-inf`, `+nan`, `-nan` in
//!   `print`, and as C does (`inf`, `-nan`, ...) through `printf` formats.
//! - `%d` of a value outside the 64-bit range prints its integral digits.
//! - `%c` of a number is the Unicode character with that code (UTF-8 locale).
//! - Width or precision above `MAX_FORMAT_WIDTH` is a fatal error (guard
//!   against `%999999999d` allocating gigabytes).

use super::value::Value;
use crate::builtins::MAX_FORMAT_WIDTH;

/// Convert a number to its awk string form, using `fmt` (`CONVFMT` or
/// `OFMT`) for non-integral values.
pub(super) fn format_number(n: f64, fmt: &str) -> String {
    if n.is_nan() {
        return if n.is_sign_negative() { "-nan" } else { "+nan" }.to_string();
    }
    if n.is_infinite() {
        return if n < 0.0 { "-inf" } else { "+inf" }.to_string();
    }
    // Integral values print as integers, so -0 prints as "0" (gawk).
    if n == n.trunc() {
        if n.abs() < 1e15 {
            return (n as i64).to_string();
        }
        return format!("{n:.0}");
    }
    sprintf(fmt, &[Value::Num(n)], "%.6g").unwrap_or_else(|_| format!("{n}"))
}

/// Error from `sprintf`: a fatal message (without the `fatal: ` prefix).
/// `located` messages are gawk's own and carry the source location.
#[derive(Debug)]
pub(super) struct FormatError {
    pub(super) msg: String,
    pub(super) located: bool,
}

#[derive(Default, Clone, Copy)]
struct Spec {
    left: bool,
    plus: bool,
    space: bool,
    alt: bool,
    zero: bool,
    width: usize,
    precision: Option<usize>,
}

/// C-style `sprintf` over awk values.
pub(super) fn sprintf(
    fmt: &str,
    args: &[Value],
    convfmt: &str,
) -> std::result::Result<String, FormatError> {
    let mut out = String::with_capacity(fmt.len());
    let bytes = fmt.as_bytes();
    let mut i = 0;
    let mut next_arg = 0;
    while i < bytes.len() {
        let Some(rel) = fmt[i..].find('%') else {
            out.push_str(&fmt[i..]);
            break;
        };
        out.push_str(&fmt[i..i + rel]);
        let start = i + rel;
        i = start + 1;
        if i >= bytes.len() {
            out.push('%');
            break;
        }
        if bytes[i] == b'%' {
            out.push('%');
            i += 1;
            continue;
        }
        // Positional argument: %N$
        let mut positional = None;
        {
            let mut j = i;
            while j < bytes.len() && bytes[j].is_ascii_digit() {
                j += 1;
            }
            if j > i && j < bytes.len() && bytes[j] == b'$' {
                let n: usize = fmt[i..j].parse().unwrap_or(0);
                if n == 0 {
                    return Err(FormatError {
                        msg: "positional argument zero is not allowed".to_string(),
                        located: true,
                    });
                }
                positional = Some(n - 1);
                i = j + 1;
            }
        }
        let mut spec = Spec::default();
        while i < bytes.len() {
            match bytes[i] {
                b'-' => spec.left = true,
                b'+' => spec.plus = true,
                b' ' => spec.space = true,
                b'#' => spec.alt = true,
                b'0' => spec.zero = true,
                b'\'' => {}
                _ => break,
            }
            i += 1;
        }
        let mut take_arg = |positional: Option<usize>| -> Option<&Value> {
            match positional {
                Some(p) => args.get(p),
                None => {
                    let v = args.get(next_arg);
                    next_arg += 1;
                    v
                }
            }
        };
        // Width.
        if i < bytes.len() && bytes[i] == b'*' {
            i += 1;
            let Some(v) = take_arg(None) else {
                return Err(not_enough(fmt, start));
            };
            let w = v.to_num();
            if w < 0.0 {
                spec.left = true;
            }
            spec.width = clamp(w.abs());
            check_cap("width", spec.width)?;
        } else {
            let s = i;
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
            if i > s {
                spec.width = fmt[s..i].parse::<usize>().unwrap_or(usize::MAX);
                check_cap("width", spec.width)?;
            }
        }
        // Precision.
        if i < bytes.len() && bytes[i] == b'.' {
            i += 1;
            if i < bytes.len() && bytes[i] == b'*' {
                i += 1;
                let Some(v) = take_arg(None) else {
                    return Err(not_enough(fmt, start));
                };
                let p = v.to_num();
                spec.precision = if p < 0.0 { None } else { Some(clamp(p)) };
                if let Some(p) = spec.precision {
                    check_cap("precision", p)?;
                }
            } else {
                let s = i;
                while i < bytes.len() && bytes[i].is_ascii_digit() {
                    i += 1;
                }
                let p = fmt[s..i].parse::<usize>().unwrap_or(usize::MAX);
                check_cap("precision", p)?;
                spec.precision = Some(p);
            }
        }
        // Length modifiers are accepted and ignored.
        while i < bytes.len() && matches!(bytes[i], b'h' | b'l' | b'L' | b'q' | b'j' | b'z' | b't')
        {
            i += 1;
        }
        let Some(conv) = fmt[i..].chars().next() else {
            // Incomplete spec at end: print it literally.
            out.push_str(&fmt[start..]);
            break;
        };
        i += conv.len_utf8();
        match conv {
            'd' | 'i' | 'o' | 'x' | 'X' | 'u' | 'c' | 's' | 'e' | 'E' | 'f' | 'F' | 'g' | 'G'
            | 'a' | 'A' => {}
            _ => {
                // Unknown conversion: gawk copies it through as text.
                out.push_str(&fmt[start..i]);
                continue;
            }
        }
        let Some(arg) = take_arg(positional) else {
            return Err(not_enough(fmt, start));
        };
        match conv {
            'c' => {
                let s = match arg {
                    Value::Num(n) | Value::StrNum(_, n) => {
                        let code = if n.is_finite() { *n as i64 } else { 0 };
                        match u32::try_from(code).ok().and_then(char::from_u32) {
                            Some(c) => c.to_string(),
                            None => char::from((code & 0xff) as u8).to_string(),
                        }
                    }
                    Value::Uninit => "\0".to_string(),
                    Value::Str(s) => s.chars().next().map(String::from).unwrap_or_default(),
                };
                pad(&mut out, &s, &spec, false);
            }
            's' => {
                let s = arg.to_str_fmt(convfmt);
                let s = match spec.precision {
                    Some(p) => match s.char_indices().nth(p) {
                        Some((idx, _)) => &s[..idx],
                        None => &s[..],
                    },
                    None => &s[..],
                };
                pad(&mut out, s, &spec, false);
            }
            'd' | 'i' => {
                let n = arg.to_num();
                if !n.is_finite() {
                    let s = special(n, false, &spec);
                    pad(&mut out, &s, &spec, false);
                    continue;
                }
                let t = n.trunc();
                let digits = if t.abs() < 9.2e18 {
                    (t.abs() as u64).to_string()
                } else {
                    format!("{:.0}", t.abs())
                };
                let sign = if t < 0.0 {
                    "-"
                } else if spec.plus {
                    "+"
                } else if spec.space {
                    " "
                } else {
                    ""
                };
                int_out(&mut out, sign, "", &digits, &spec);
            }
            'o' | 'x' | 'X' | 'u' => {
                let n = arg.to_num();
                if !n.is_finite() {
                    let s = special(n, false, &spec);
                    pad(&mut out, &s, &spec, false);
                    continue;
                }
                let t = n.trunc();
                let u: Option<u64> = if t < 0.0 {
                    if t >= -9.2e18 {
                        Some(t as i64 as u64)
                    } else {
                        None
                    }
                } else if t < 1.8e19 {
                    Some(t as u64)
                } else {
                    None
                };
                let Some(u) = u else {
                    // Out of range: gawk falls back to a float format.
                    let s = fmt_g(n, 6, false, false);
                    pad(&mut out, &s, &spec, false);
                    continue;
                };
                let (digits, prefix) = match conv {
                    'o' => {
                        let d = format!("{u:o}");
                        let p = if spec.alt && !d.starts_with('0') {
                            "0"
                        } else {
                            ""
                        };
                        (d, p)
                    }
                    'x' => (format!("{u:x}"), if spec.alt && u != 0 { "0x" } else { "" }),
                    'X' => (format!("{u:X}"), if spec.alt && u != 0 { "0X" } else { "" }),
                    _ => (u.to_string(), ""),
                };
                int_out(&mut out, "", prefix, &digits, &spec);
            }
            _ => {
                let n = arg.to_num();
                let upper = conv.is_ascii_uppercase();
                if !n.is_finite() {
                    let s = special(n, upper, &spec);
                    pad(&mut out, &s, &spec, false);
                    continue;
                }
                let prec = spec.precision.unwrap_or(6);
                let body = match conv.to_ascii_lowercase() {
                    'f' => {
                        let mut s = format!("{:.*}", prec, n.abs());
                        if spec.alt && prec == 0 {
                            s.push('.');
                        }
                        s
                    }
                    'e' => fmt_e(n.abs(), prec, spec.alt, upper),
                    'a' => fmt_e(n.abs(), prec, spec.alt, upper),
                    _ => fmt_g(n.abs(), prec, spec.alt, upper),
                };
                let sign = if n.is_sign_negative() && !(n == 0.0 && body_is_zero(&body)) {
                    "-"
                } else if spec.plus {
                    "+"
                } else if spec.space {
                    " "
                } else {
                    ""
                };
                // C prints -0.0 as "-0.000000"; keep that.
                let sign = if n == 0.0 && n.is_sign_negative() {
                    "-"
                } else {
                    sign
                };
                float_out(&mut out, sign, &body, &spec);
            }
        }
    }
    Ok(out)
}

fn body_is_zero(body: &str) -> bool {
    body.bytes().all(|b| matches!(b, b'0' | b'.'))
}

fn clamp(v: f64) -> usize {
    if v.is_finite() && v < usize::MAX as f64 {
        v as usize
    } else {
        usize::MAX
    }
}

/// THREAT[TM-DOS-110]: refuse widths/precisions above `MAX_FORMAT_WIDTH`.
fn check_cap(what: &str, v: usize) -> std::result::Result<(), FormatError> {
    if v > MAX_FORMAT_WIDTH {
        let shown = if v == usize::MAX {
            "too large".to_string()
        } else {
            v.to_string()
        };
        return Err(FormatError {
            msg: format!("format {what} {shown} exceeds maximum ({MAX_FORMAT_WIDTH})"),
            located: false,
        });
    }
    Ok(())
}

fn not_enough(fmt: &str, at: usize) -> FormatError {
    FormatError {
        msg: format!(
            "not enough arguments to satisfy format string\n\t`{fmt}'\n\t{}^ ran out for this one",
            " ".repeat(at + 1)
        ),
        located: true,
    }
}

/// `inf`/`nan` through a numeric conversion.
fn special(n: f64, upper: bool, spec: &Spec) -> String {
    let word = if n.is_nan() { "nan" } else { "inf" };
    let word = if upper {
        word.to_ascii_uppercase()
    } else {
        word.to_string()
    };
    let sign = if n.is_sign_negative() {
        "-"
    } else if spec.plus {
        "+"
    } else if spec.space {
        " "
    } else {
        ""
    };
    format!("{sign}{word}")
}

/// Pad `s` to the spec width (spaces; zero padding is numeric only).
fn pad(out: &mut String, s: &str, spec: &Spec, _numeric: bool) {
    let len = s.chars().count();
    if len >= spec.width {
        out.push_str(s);
        return;
    }
    let fill = spec.width - len;
    if spec.left {
        out.push_str(s);
        out.extend(std::iter::repeat_n(' ', fill));
    } else {
        out.extend(std::iter::repeat_n(' ', fill));
        out.push_str(s);
    }
}

fn int_out(out: &mut String, sign: &str, prefix: &str, digits: &str, spec: &Spec) {
    let mut digits = digits.to_string();
    if let Some(p) = spec.precision {
        if p == 0 && digits == "0" {
            digits.clear();
        }
        if digits.len() < p {
            digits = "0".repeat(p - digits.len()) + &digits;
        }
    }
    let len = sign.len() + prefix.len() + digits.len();
    if spec.zero && !spec.left && spec.precision.is_none() && len < spec.width {
        out.push_str(sign);
        out.push_str(prefix);
        out.extend(std::iter::repeat_n('0', spec.width - len));
        out.push_str(&digits);
        return;
    }
    let s = format!("{sign}{prefix}{digits}");
    pad(out, &s, spec, true);
}

fn float_out(out: &mut String, sign: &str, body: &str, spec: &Spec) {
    let len = sign.len() + body.len();
    if spec.zero && !spec.left && len < spec.width {
        out.push_str(sign);
        out.extend(std::iter::repeat_n('0', spec.width - len));
        out.push_str(body);
        return;
    }
    let s = format!("{sign}{body}");
    pad(out, &s, spec, true);
}

/// `%e` body for a non-negative finite value.
fn fmt_e(n: f64, prec: usize, alt: bool, upper: bool) -> String {
    let s = format!("{:.*e}", prec, n);
    let (mant, exp) = s.split_once('e').unwrap_or((&s, "0"));
    let exp: i32 = exp.parse().unwrap_or(0);
    let mut mant = mant.to_string();
    if alt && prec == 0 {
        mant.push('.');
    }
    let e = if upper { 'E' } else { 'e' };
    let sign = if exp < 0 { '-' } else { '+' };
    format!("{mant}{e}{sign}{:02}", exp.abs())
}

/// `%g` body for a non-negative finite value.
fn fmt_g(n: f64, prec: usize, alt: bool, upper: bool) -> String {
    let p = if prec == 0 { 1 } else { prec };
    if n == 0.0 {
        let mut s = "0".to_string();
        if alt {
            s.push('.');
            s.push_str(&"0".repeat(p - 1));
        }
        return s;
    }
    // Exponent after rounding to p significant digits.
    let e_repr = format!("{:.*e}", p - 1, n);
    let x: i32 = e_repr
        .split_once('e')
        .and_then(|(_, e)| e.parse().ok())
        .unwrap_or(0);
    let mut s = if x < -4 || x >= p as i32 {
        fmt_e(n, p - 1, alt, upper)
    } else {
        let decimals = (p as i32 - 1 - x).max(0) as usize;
        let mut s = format!("{:.*}", decimals, n);
        if alt && !s.contains('.') {
            s.push('.');
        }
        s
    };
    if !alt {
        // Strip trailing zeros of the fraction (before any exponent).
        let (mant, exp) = match s.find(['e', 'E']) {
            Some(pos) => (s[..pos].to_string(), s[pos..].to_string()),
            None => (s.clone(), String::new()),
        };
        if mant.contains('.') {
            let m = mant.trim_end_matches('0').trim_end_matches('.');
            s = format!("{m}{exp}");
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(fmt: &str, args: &[Value]) -> String {
        sprintf(fmt, args, "%.6g").unwrap()
    }
    fn n(x: f64) -> Value {
        Value::Num(x)
    }
    fn s(x: &str) -> Value {
        Value::Str(x.to_string())
    }

    #[test]
    fn number_to_string() {
        assert_eq!(format_number(3.0, "%.6g"), "3");
        assert_eq!(format_number(1.23456789, "%.6g"), "1.23457");
        assert_eq!(format_number(1e16, "%.6g"), "10000000000000000");
        assert_eq!(
            format_number(2f64.powi(70), "%.6g"),
            "1180591620717411303424"
        );
        assert_eq!(format_number(0.1, "%.2f"), "0.10");
        assert_eq!(format_number(1e-5, "%.6g"), "1e-05");
        assert_eq!(format_number(f64::INFINITY, "%.6g"), "+inf");
        assert_eq!(format_number(-f64::NAN, "%.6g"), "-nan");
        assert_eq!(format_number(-0.0, "%.6g"), "0");
    }

    #[test]
    fn conversions() {
        assert_eq!(f("%c %c", &[n(65.0), s("hello")]), "A h");
        assert_eq!(f("%c%c", &[n(225.0), n(8364.0)]), "á€");
        assert_eq!(f("%d %x %X", &[n(10.0), n(255.0), n(255.0)]), "10 ff FF");
        assert_eq!(f("%e", &[n(1234.5)]), "1.234500e+03");
        assert_eq!(f("%E", &[n(0.000123)]), "1.230000E-04");
        assert_eq!(f("%g %g", &[n(1234567.0), n(0.0001)]), "1.23457e+06 0.0001");
        assert_eq!(
            f(
                "%g %g %g %g %.3g",
                &[n(1e5), n(1e6), n(1e-4), n(1e-5), n(1.23456)]
            ),
            "100000 1e+06 0.0001 1e-05 1.23"
        );
        assert_eq!(f("%5.1f|%-5s|", &[n(1.23456), s("ab")]), "  1.2|ab   |");
    }

    #[test]
    fn flags_and_star() {
        assert_eq!(
            f(
                "%05d|%+d|% d|%05.1f|%-5d|%#o|%#x",
                &[n(42.0), n(42.0), n(42.0), n(1.23), n(7.0), n(8.0), n(255.0)]
            ),
            "00042|+42| 42|001.2|7    |010|0xff"
        );
        assert_eq!(
            f(
                "%*d|%-*s|%.*f",
                &[n(5.0), n(42.0), n(4.0), s("ab"), n(2.0), n(1.23456)]
            ),
            "   42|ab  |1.23"
        );
        assert_eq!(f("%2$s %1$s", &[s("world"), s("hello")]), "hello world");
    }

    #[test]
    fn integers() {
        assert_eq!(f("%d", &[n(2f64.powi(64))]), "18446744073709551616");
        assert_eq!(
            f("%d", &[n(-9223372036854775808.0)]),
            "-9223372036854775808"
        );
        assert_eq!(f("%d %d", &[s("12abc"), s("abc")]), "12 0");
        assert_eq!(f("%x", &[n(-1.0)]), "ffffffffffffffff");
        assert_eq!(f("%.3d", &[n(7.0)]), "007");
        assert_eq!(f("%d", &[n(-3.9)]), "-3");
    }

    #[test]
    fn not_enough_args() {
        let e = sprintf("%s %s\n", &[s("a")], "%.6g").unwrap_err();
        assert!(e.msg.contains("not enough arguments"), "{}", e.msg);
        assert!(
            e.msg.ends_with("\t    ^ ran out for this one"),
            "{:?}", // debug-ok: assert-failure message
            e.msg
        );
    }

    #[test]
    fn special_values() {
        assert_eq!(f("%f %d", &[n(f64::INFINITY), n(-f64::NAN)]), "inf -nan");
        assert_eq!(f("%5.1f%%", &[n(99.25)]), " 99.2%");
    }

    #[test]
    fn huge_width_is_an_error() {
        let e = sprintf("%10001s", &[s("x")], "%.6g").unwrap_err();
        assert_eq!(e.msg, "format width 10001 exceeds maximum (10000)");
        let e = sprintf("%.10001s", &[s("x")], "%.6g").unwrap_err();
        assert_eq!(e.msg, "format precision 10001 exceeds maximum (10000)");
        assert_eq!(f("%10000s", &[s("x")]).len(), 10000);
    }
}
