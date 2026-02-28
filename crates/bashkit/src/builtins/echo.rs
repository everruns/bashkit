//! echo builtin command

use async_trait::async_trait;

use super::{Builtin, Context};
use crate::error::Result;
use crate::interpreter::ExecResult;

/// The echo builtin command.
pub struct Echo;

#[async_trait]
impl Builtin for Echo {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let mut output = String::new();
        let mut add_newline = true;
        let mut interpret_escapes = false;
        let mut args_iter = ctx.args.iter().peekable();

        // Parse options - support combined flags like -en, -ne, -neE
        while let Some(arg) = args_iter.peek() {
            let arg_str = arg.as_str();
            if arg_str.starts_with('-') && arg_str.len() > 1 && !arg_str.starts_with("--") {
                let mut is_valid_option = true;
                // Check if all characters after '-' are valid options
                for c in arg_str[1..].chars() {
                    if !matches!(c, 'n' | 'e' | 'E') {
                        is_valid_option = false;
                        break;
                    }
                }
                if is_valid_option {
                    // Process each flag character
                    for c in arg_str[1..].chars() {
                        match c {
                            'n' => add_newline = false,
                            'e' => interpret_escapes = true,
                            'E' => interpret_escapes = false,
                            _ => {}
                        }
                    }
                    args_iter.next();
                } else {
                    break;
                }
            } else {
                break;
            }
        }

        // Collect remaining arguments
        let remaining: Vec<&String> = args_iter.collect();

        for (i, arg) in remaining.iter().enumerate() {
            if i > 0 {
                output.push(' ');
            }

            if interpret_escapes {
                output.push_str(&interpret_escape_sequences(arg));
            } else {
                output.push_str(arg);
            }
        }

        if add_newline {
            output.push('\n');
        }

        Ok(ExecResult::ok(output))
    }
}

// to_digit().unwrap() is safe: only called after is_ascii_hexdigit() check
#[allow(clippy::unwrap_used)]
fn interpret_escape_sequences(s: &str) -> String {
    let mut result = String::new();
    let mut chars = s.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '\\' {
            match chars.next() {
                Some('n') => result.push('\n'),
                Some('t') => result.push('\t'),
                Some('r') => result.push('\r'),
                Some('\\') => result.push('\\'),
                Some('a') => result.push('\x07'), // bell
                Some('b') => result.push('\x08'), // backspace
                Some('f') => result.push('\x0c'), // form feed
                Some('v') => result.push('\x0b'), // vertical tab
                Some('0') => {
                    // Octal escape \0nnn
                    let mut value = 0u8;
                    for _ in 0..3 {
                        if let Some(&digit) = chars.peek() {
                            if ('0'..='7').contains(&digit) {
                                value = value * 8 + (digit as u8 - b'0');
                                chars.next();
                            } else {
                                break;
                            }
                        }
                    }
                    result.push(value as char);
                }
                Some('x') => {
                    // Hex escape \xHH
                    let mut value = 0u8;
                    for _ in 0..2 {
                        if let Some(&digit) = chars.peek() {
                            if digit.is_ascii_hexdigit() {
                                value = value * 16 + digit.to_digit(16).unwrap() as u8;
                                chars.next();
                            } else {
                                break;
                            }
                        }
                    }
                    result.push(value as char);
                }
                Some('c') => {
                    // Stop output
                    break;
                }
                Some(other) => {
                    result.push('\\');
                    result.push(other);
                }
                None => result.push('\\'),
            }
        } else {
            result.push(ch);
        }
    }

    result
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn test_escape_sequences() {
        assert_eq!(interpret_escape_sequences("hello\\nworld"), "hello\nworld");
        assert_eq!(interpret_escape_sequences("tab\\there"), "tab\there");
        assert_eq!(interpret_escape_sequences("\\\\backslash"), "\\backslash");
    }
}
