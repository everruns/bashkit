//! printf builtin - formatted output

// Format parsing uses chars().next().unwrap() after peek() confirms character exists
#![allow(clippy::unwrap_used)]

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

        let format = &ctx.args[0];
        let args = &ctx.args[1..];
        let mut arg_index = 0;

        let output = format_string(format, args, &mut arg_index);
        Ok(ExecResult::ok(output))
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
                        // Octal escape sequence
                        let mut octal = String::new();
                        while let Some(&c) = chars.peek() {
                            if c.is_ascii_digit() && c != '8' && c != '9' && octal.len() < 3 {
                                octal.push(chars.next().unwrap());
                            } else {
                                break;
                            }
                        }
                        if let Ok(val) = u8::from_str_radix(&octal, 8) {
                            output.push(val as char);
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

/// Expand escape sequences in a string
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
}
