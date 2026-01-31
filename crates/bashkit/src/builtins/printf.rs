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

        let format = &ctx.args[0];
        let args = &ctx.args[1..];
        let mut arg_index = 0;

        let output = format_string(format, args, &mut arg_index);
        Ok(ExecResult::ok(output))
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

                // Get the format type
                if let Some(fmt_type) = chars.next() {
                    let arg = args.get(*arg_index).map(|s| s.as_str()).unwrap_or("");
                    *arg_index += 1;

                    match fmt_type {
                        's' => {
                            // String
                            output.push_str(arg);
                        }
                        'd' | 'i' => {
                            // Integer
                            if let Ok(n) = arg.parse::<i64>() {
                                output.push_str(&n.to_string());
                            } else {
                                output.push('0');
                            }
                        }
                        'u' => {
                            // Unsigned integer
                            if let Ok(n) = arg.parse::<u64>() {
                                output.push_str(&n.to_string());
                            } else {
                                output.push('0');
                            }
                        }
                        'o' => {
                            // Octal
                            if let Ok(n) = arg.parse::<u64>() {
                                output.push_str(&format!("{:o}", n));
                            } else {
                                output.push('0');
                            }
                        }
                        'x' => {
                            // Lowercase hex
                            if let Ok(n) = arg.parse::<u64>() {
                                output.push_str(&format!("{:x}", n));
                            } else {
                                output.push('0');
                            }
                        }
                        'X' => {
                            // Uppercase hex
                            if let Ok(n) = arg.parse::<u64>() {
                                output.push_str(&format!("{:X}", n));
                            } else {
                                output.push('0');
                            }
                        }
                        'f' | 'e' | 'E' | 'g' | 'G' => {
                            // Float
                            if let Ok(n) = arg.parse::<f64>() {
                                output.push_str(&format!("{}", n));
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
