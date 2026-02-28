//! printf builtin - formatted output

use async_trait::async_trait;

use super::{Builtin, Context};
use crate::error::Result;
use crate::interpreter::ExecResult;

/// printf builtin - formatted string output
pub struct Printf;

#[async_trait]
impl Builtin for Printf {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        if ctx.args.is_empty() {
            return Ok(ExecResult::ok(String::new()));
        }

        let mut args_iter = ctx.args.iter();
        let mut var_name: Option<String> = None;

        // Check for -v varname flag
        let format = loop {
            match args_iter.next() {
                Some(arg) if arg == "-v" => {
                    if let Some(vname) = args_iter.next() {
                        var_name = Some(vname.clone());
                    }
                }
                Some(arg) => break arg.clone(),
                None => return Ok(ExecResult::ok(String::new())),
            }
        };

        let args: Vec<String> = args_iter.cloned().collect();
        let mut arg_index = 0;
        let mut output = String::new();

        // Bash printf repeats the format string until all args are consumed
        loop {
            let start_index = arg_index;
            output.push_str(&format_string(&format, &args, &mut arg_index));

            // If no args were consumed or we've used all args, stop
            if arg_index == start_index || arg_index >= args.len() {
                break;
            }
        }

        if let Some(name) = var_name {
            // -v: assign to variable instead of printing
            ctx.variables.insert(name, output);
            Ok(ExecResult::ok(String::new()))
        } else {
            Ok(ExecResult::ok(output))
        }
    }
}

/// Parsed format specification
struct FormatSpec {
    left_align: bool,
    zero_pad: bool,
    sign_plus: bool,
    width: Option<usize>,
    precision: Option<usize>,
}

impl FormatSpec {
    // chars.next().unwrap() is safe: only called after peek() confirms char exists
    #[allow(clippy::unwrap_used)]
    fn parse(spec: &str) -> Self {
        let mut left_align = false;
        let mut zero_pad = false;
        let mut sign_plus = false;
        let mut chars = spec.chars().peekable();

        // Parse flags
        while let Some(&c) = chars.peek() {
            match c {
                '-' => {
                    left_align = true;
                    chars.next();
                }
                '0' if !zero_pad && chars.clone().nth(1).is_some() => {
                    // Only treat as flag if followed by more chars (width)
                    zero_pad = true;
                    chars.next();
                }
                '+' => {
                    sign_plus = true;
                    chars.next();
                }
                ' ' | '#' => {
                    chars.next();
                }
                _ => break,
            }
        }

        // Parse width
        let mut width_str = String::new();
        while let Some(&c) = chars.peek() {
            if c.is_ascii_digit() {
                width_str.push(chars.next().unwrap());
            } else {
                break;
            }
        }
        let width = if width_str.is_empty() {
            None
        } else {
            width_str.parse().ok()
        };

        // Parse precision
        let precision = if chars.peek() == Some(&'.') {
            chars.next();
            let mut prec_str = String::new();
            while let Some(&c) = chars.peek() {
                if c.is_ascii_digit() {
                    prec_str.push(chars.next().unwrap());
                } else {
                    break;
                }
            }
            if prec_str.is_empty() {
                Some(0)
            } else {
                prec_str.parse().ok()
            }
        } else {
            None
        };

        Self {
            left_align,
            zero_pad,
            sign_plus,
            width,
            precision,
        }
    }

    /// Format an integer with the parsed spec
    fn format_int(&self, n: i64) -> String {
        let formatted = if self.sign_plus && n >= 0 {
            format!("+{}", n)
        } else {
            n.to_string()
        };

        self.apply_width(&formatted, true)
    }

    /// Format an unsigned integer with the parsed spec
    fn format_uint(&self, n: u64) -> String {
        let formatted = n.to_string();
        self.apply_width(&formatted, true)
    }

    /// Format a string with the parsed spec
    fn format_str(&self, s: &str) -> String {
        let s = if let Some(prec) = self.precision {
            &s[..s.len().min(prec)]
        } else {
            s
        };
        self.apply_width(s, false)
    }

    /// Apply width padding
    fn apply_width(&self, s: &str, is_numeric: bool) -> String {
        let width = match self.width {
            Some(w) => w,
            None => return s.to_string(),
        };

        if s.len() >= width {
            return s.to_string();
        }

        let pad_char = if self.zero_pad && is_numeric && !self.left_align {
            '0'
        } else {
            ' '
        };
        let padding = width - s.len();

        if self.left_align {
            format!("{}{}", s, " ".repeat(padding))
        } else if self.zero_pad && is_numeric && s.starts_with('-') {
            // Handle negative numbers: put minus before zeros
            format!("-{}{}", pad_char.to_string().repeat(padding), &s[1..])
        } else if self.zero_pad && is_numeric && s.starts_with('+') {
            // Handle explicit plus sign
            format!("+{}{}", pad_char.to_string().repeat(padding), &s[1..])
        } else {
            format!("{}{}", pad_char.to_string().repeat(padding), s)
        }
    }
}

/// Format a string using printf-style format specifiers
// chars.next().unwrap() is safe: only called after peek() confirms char exists
#[allow(clippy::unwrap_used)]
fn format_string(format: &str, args: &[String], arg_index: &mut usize) -> String {
    let mut output = String::new();
    let mut chars = format.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '\\' {
            // Handle escape sequences
            if let Some(next) = chars.next() {
                match next {
                    'n' => output.push('\n'),
                    't' => output.push('\t'),
                    'r' => output.push('\r'),
                    '\\' => output.push('\\'),
                    '"' => output.push('"'),
                    '\'' => output.push('\''),
                    '0' => {
                        // Octal escape sequence - \0, \0N, \0NN, \0NNN
                        let mut octal = String::from("0");
                        while let Some(&c) = chars.peek() {
                            if c.is_ascii_digit() && c != '8' && c != '9' && octal.len() < 4 {
                                octal.push(chars.next().unwrap());
                            } else {
                                break;
                            }
                        }
                        if let Ok(val) = u8::from_str_radix(&octal, 8) {
                            output.push(val as char);
                        }
                    }
                    'u' => {
                        // \uHHHH - 4-digit unicode escape
                        if let Some(c) = parse_unicode_escape(&mut chars, 4) {
                            output.push(c);
                        }
                    }
                    'U' => {
                        // \UHHHHHHHH - 8-digit unicode escape
                        if let Some(c) = parse_unicode_escape(&mut chars, 8) {
                            output.push(c);
                        }
                    }
                    _ => {
                        output.push('\\');
                        output.push(next);
                    }
                }
            } else {
                output.push('\\');
            }
        } else if ch == '%' {
            // Handle format specifiers
            if let Some(&next) = chars.peek() {
                if next == '%' {
                    chars.next();
                    output.push('%');
                    continue;
                }

                // Parse optional flags, width, precision
                let mut spec = String::new();
                while let Some(&c) = chars.peek() {
                    if c.is_ascii_digit()
                        || c == '-'
                        || c == '+'
                        || c == ' '
                        || c == '#'
                        || c == '.'
                    {
                        spec.push(chars.next().unwrap());
                    } else {
                        break;
                    }
                }

                let fmt_spec = FormatSpec::parse(&spec);

                // Get the format type
                if let Some(fmt_type) = chars.next() {
                    let arg = args.get(*arg_index).map(|s| s.as_str()).unwrap_or("");
                    *arg_index += 1;

                    match fmt_type {
                        's' => {
                            // String
                            output.push_str(&fmt_spec.format_str(arg));
                        }
                        'd' | 'i' => {
                            // Integer
                            if let Ok(n) = arg.parse::<i64>() {
                                output.push_str(&fmt_spec.format_int(n));
                            } else {
                                output.push_str(&fmt_spec.format_int(0));
                            }
                        }
                        'u' => {
                            // Unsigned integer
                            if let Ok(n) = arg.parse::<u64>() {
                                output.push_str(&fmt_spec.format_uint(n));
                            } else {
                                output.push_str(&fmt_spec.format_uint(0));
                            }
                        }
                        'o' => {
                            // Octal
                            if let Ok(n) = arg.parse::<u64>() {
                                let formatted = format!("{:o}", n);
                                output.push_str(&fmt_spec.apply_width(&formatted, true));
                            } else {
                                output.push_str(&fmt_spec.apply_width("0", true));
                            }
                        }
                        'x' => {
                            // Lowercase hex
                            if let Ok(n) = arg.parse::<u64>() {
                                let formatted = format!("{:x}", n);
                                output.push_str(&fmt_spec.apply_width(&formatted, true));
                            } else {
                                output.push_str(&fmt_spec.apply_width("0", true));
                            }
                        }
                        'X' => {
                            // Uppercase hex
                            if let Ok(n) = arg.parse::<u64>() {
                                let formatted = format!("{:X}", n);
                                output.push_str(&fmt_spec.apply_width(&formatted, true));
                            } else {
                                output.push_str(&fmt_spec.apply_width("0", true));
                            }
                        }
                        'f' | 'e' | 'E' | 'g' | 'G' => {
                            // Float
                            if let Ok(n) = arg.parse::<f64>() {
                                let formatted = if let Some(prec) = fmt_spec.precision {
                                    format!("{:.prec$}", n, prec = prec)
                                } else {
                                    format!("{}", n)
                                };
                                output.push_str(&fmt_spec.apply_width(&formatted, true));
                            } else {
                                output.push_str("0.0");
                            }
                        }
                        'c' => {
                            // Character
                            if let Some(c) = arg.chars().next() {
                                output.push(c);
                            }
                        }
                        'b' => {
                            // String with escape sequences
                            output.push_str(&expand_escapes(arg));
                        }
                        'q' => {
                            // Shell-quoted string safe for reuse
                            output.push_str(&shell_quote(arg));
                        }
                        _ => {
                            // Unknown format - output literally
                            output.push('%');
                            output.push_str(&spec);
                            output.push(fmt_type);
                            *arg_index -= 1; // Don't consume arg
                        }
                    }
                }
            } else {
                output.push('%');
            }
        } else {
            output.push(ch);
        }
    }

    output
}

/// Quote a string for safe shell reuse (printf %q behavior).
///
/// Matches bash behavior:
/// - Empty string → `''`
/// - Safe strings (only alnum/`_`/`.`/`-`/`:`/`=`/`+`/`@`/`,`/`%`/`^`/`/`) → unquoted
/// - Strings with control chars (tab, newline, etc.) → `$'...'` quoting
/// - Other strings → backslash-escape individual special characters
fn shell_quote(s: &str) -> String {
    if s.is_empty() {
        return "''".to_string();
    }

    // Check if the string needs quoting at all
    let needs_quoting = s
        .chars()
        .any(|c| !c.is_ascii_alphanumeric() && !"_/.:-=+@,%^".contains(c));

    if !needs_quoting {
        return s.to_string();
    }

    // Check for control characters that require $'...' quoting
    let has_control = s.chars().any(|c| (c as u32) < 32 || c as u32 == 127);

    if has_control {
        // Use $'...' quoting
        let mut out = String::from("$'");
        for ch in s.chars() {
            match ch {
                '\'' => out.push_str("\\'"),
                '\\' => out.push_str("\\\\"),
                '\n' => out.push_str("\\n"),
                '\t' => out.push_str("\\t"),
                '\r' => out.push_str("\\r"),
                c if (c as u32) < 32 || c as u32 == 127 => {
                    out.push_str(&format!("\\x{:02x}", c as u32));
                }
                c => out.push(c),
            }
        }
        out.push('\'');
        out
    } else {
        // Backslash-escape individual special characters
        let mut out = String::new();
        for ch in s.chars() {
            if ch.is_ascii_alphanumeric() || "_/.:-=+@,%^".contains(ch) {
                out.push(ch);
            } else {
                out.push('\\');
                out.push(ch);
            }
        }
        out
    }
}

/// Expand escape sequences in a string
// chars.next().unwrap() is safe: only called after peek() confirms char exists
#[allow(clippy::unwrap_used)]
fn expand_escapes(s: &str) -> String {
    let mut output = String::new();
    let mut chars = s.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '\\' {
            if let Some(next) = chars.next() {
                match next {
                    'n' => output.push('\n'),
                    't' => output.push('\t'),
                    'r' => output.push('\r'),
                    '\\' => output.push('\\'),
                    '0' => {
                        // Octal escape sequence
                        let mut octal = String::from("0");
                        while let Some(&c) = chars.peek() {
                            if c.is_ascii_digit() && c != '8' && c != '9' && octal.len() < 4 {
                                octal.push(chars.next().unwrap());
                            } else {
                                break;
                            }
                        }
                        if let Ok(val) = u8::from_str_radix(&octal, 8) {
                            output.push(val as char);
                        }
                    }
                    'u' => {
                        // \uHHHH - 4-digit unicode escape
                        if let Some(c) = parse_unicode_escape(&mut chars, 4) {
                            output.push(c);
                        }
                    }
                    'U' => {
                        // \UHHHHHHHH - 8-digit unicode escape
                        if let Some(c) = parse_unicode_escape(&mut chars, 8) {
                            output.push(c);
                        }
                    }
                    _ => {
                        output.push('\\');
                        output.push(next);
                    }
                }
            } else {
                output.push('\\');
            }
        } else {
            output.push(ch);
        }
    }

    output
}

/// Parse a unicode escape sequence (\uHHHH or \UHHHHHHHH) from a char iterator.
/// `max_digits` is 4 for \u and 8 for \U.
// chars.next().unwrap() is safe: only called after peek() confirms char exists
#[allow(clippy::unwrap_used)]
fn parse_unicode_escape(
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
    max_digits: usize,
) -> Option<char> {
    let mut hex = String::new();
    for _ in 0..max_digits {
        if let Some(&c) = chars.peek() {
            if c.is_ascii_hexdigit() {
                hex.push(chars.next().unwrap());
            } else {
                break;
            }
        } else {
            break;
        }
    }
    if hex.is_empty() {
        return None;
    }
    u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_zero_padding() {
        let args = vec!["42".to_string()];
        let mut idx = 0;
        assert_eq!(format_string("%05d", &args, &mut idx), "00042");
    }

    #[test]
    fn test_zero_padding_negative() {
        let args = vec!["-42".to_string()];
        let mut idx = 0;
        assert_eq!(format_string("%06d", &args, &mut idx), "-00042");
    }

    #[test]
    fn test_width_without_zero() {
        let args = vec!["42".to_string()];
        let mut idx = 0;
        assert_eq!(format_string("%5d", &args, &mut idx), "   42");
    }

    #[test]
    fn test_left_align() {
        let args = vec!["42".to_string()];
        let mut idx = 0;
        assert_eq!(format_string("%-5d", &args, &mut idx), "42   ");
    }

    #[test]
    fn test_string_width() {
        let args = vec!["hi".to_string()];
        let mut idx = 0;
        assert_eq!(format_string("%5s", &args, &mut idx), "   hi");
    }

    #[test]
    fn test_string_left_align() {
        let args = vec!["hi".to_string()];
        let mut idx = 0;
        assert_eq!(format_string("%-5s", &args, &mut idx), "hi   ");
    }

    #[test]
    fn test_precision_float() {
        let args = vec!["3.14159".to_string()];
        let mut idx = 0;
        assert_eq!(format_string("%.2f", &args, &mut idx), "3.14");
    }

    #[test]
    fn test_width_and_precision() {
        let args = vec!["3.14".to_string()];
        let mut idx = 0;
        assert_eq!(format_string("%8.2f", &args, &mut idx), "    3.14");
    }

    #[test]
    fn test_hex_zero_padding() {
        let args = vec!["255".to_string()];
        let mut idx = 0;
        assert_eq!(format_string("%04x", &args, &mut idx), "00ff");
    }

    #[test]
    fn test_unicode_escape_u() {
        // \u03bc -> μ (Greek small letter mu)
        let args = vec![];
        let mut idx = 0;
        assert_eq!(format_string("\\u03bc", &args, &mut idx), "\u{03bc}");
    }

    #[test]
    fn test_unicode_escape_big_u() {
        // \U000003bc -> μ
        let args = vec![];
        let mut idx = 0;
        assert_eq!(format_string("\\U000003bc", &args, &mut idx), "\u{03bc}");
    }

    #[test]
    fn test_unicode_escape_ascii() {
        // \u0041 -> A
        let args = vec![];
        let mut idx = 0;
        assert_eq!(
            format_string("\\u0041\\u0042\\u0043", &args, &mut idx),
            "ABC"
        );
    }

    #[test]
    fn test_unicode_escape_in_expand() {
        // %b format also handles \u escapes
        assert_eq!(expand_escapes("\\u03bc"), "\u{03bc}");
        assert_eq!(expand_escapes("\\U000003bc"), "\u{03bc}");
    }
}
