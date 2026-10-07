//! echo builtin command

use async_trait::async_trait;

use super::{Builtin, BuiltinHelper, Context};
use crate::error::Result;
use crate::interpreter::ExecResult;

/// The echo builtin command.
pub struct Echo;

impl BuiltinHelper for Echo {
    const NAME: &'static str = "echo";
}

#[async_trait]
impl Builtin for Echo {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        if let Some(r) = Self::check_help(
            ctx.args,
            "Usage: echo [SHORT-OPTION]... [STRING]...\n  or:  echo LONG-OPTION\nEcho the STRING(s) to standard output.\n\n  -n\tdo not output the trailing newline\n  -e\tenable interpretation of backslash escapes\n  -E\tdisable interpretation of backslash escapes (default)\n  --help\tdisplay this help and exit\n  --version\toutput version information and exit\n",
            Some("echo (bashkit) 0.1"),
        ) {
            return Ok(r);
        }
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
        let mut output: Vec<u8> = Vec::new();

        for (i, arg) in remaining.iter().enumerate() {
            if i > 0 {
                output.push(b' ');
            }

            if interpret_escapes {
                // `\c` stops all further output, the newline included.
                if interpret_escape_sequences(arg, &mut output) {
                    return Ok(ExecResult::ok_bytes(output));
                }
            } else {
                output.extend_from_slice(arg.as_bytes());
            }
        }

        if add_newline {
            output.push(b'\n');
        }

        Ok(ExecResult::ok_bytes(output))
    }
}

/// Expand bash `echo -e` escapes into `out` (octal/hex escapes are raw
/// bytes). Returns `true` when `\c` asked to stop all output.
fn interpret_escape_sequences(s: &str, out: &mut Vec<u8>) -> bool {
    let mut chars = s.chars().peekable();
    let mut buf = [0u8; 4];

    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
            continue;
        }
        let byte = match chars.next() {
            Some('n') => b'\n',
            Some('t') => b'\t',
            Some('r') => b'\r',
            Some('\\') => b'\\',
            Some('a') => 0x07,
            Some('b') => 0x08,
            Some('e') | Some('E') => 0x1b,
            Some('f') => 0x0c,
            Some('v') => 0x0b,
            Some('c') => return true,
            Some('0') => {
                // \0nnn: zero to three octal digits after the 0.
                let mut value = 0u32;
                for _ in 0..3 {
                    match chars.peek().and_then(|d| d.to_digit(8)) {
                        Some(d) => {
                            value = value * 8 + d;
                            chars.next();
                        }
                        None => break,
                    }
                }
                (value & 0xff) as u8
            }
            Some(c @ ('x' | 'u' | 'U')) => {
                let max = match c {
                    'x' => 2,
                    'u' => 4,
                    _ => 8,
                };
                let mut value = 0u32;
                let mut digits = 0;
                while digits < max {
                    match chars.peek().and_then(|d| d.to_digit(16)) {
                        Some(d) => {
                            value = value * 16 + d;
                            chars.next();
                            digits += 1;
                        }
                        None => break,
                    }
                }
                if digits == 0 {
                    // No hex digits: the escape stays as written.
                    out.push(b'\\');
                    out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
                    continue;
                }
                if c == 'x' {
                    value as u8
                } else {
                    let ch = char::from_u32(value).unwrap_or('\u{fffd}');
                    out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                    continue;
                }
            }
            Some(other) => {
                out.push(b'\\');
                out.extend_from_slice(other.encode_utf8(&mut buf).as_bytes());
                continue;
            }
            None => b'\\',
        };
        out.push(byte);
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn esc(s: &str) -> (Vec<u8>, bool) {
        let mut out = Vec::new();
        let stop = interpret_escape_sequences(s, &mut out);
        (out, stop)
    }

    #[test]
    fn test_escape_sequences() {
        assert_eq!(esc("hello\\nworld").0, b"hello\nworld");
        assert_eq!(esc("tab\\there").0, b"tab\there");
        assert_eq!(esc("\\\\backslash").0, b"\\backslash");
        // Unknown escapes and \x without digits stay as written.
        assert_eq!(esc("\\z\\xg\\").0, b"\\z\\xg\\");
        // Octal/hex escapes are raw bytes, not Latin-1 characters.
        assert_eq!(esc("\\0377\\xff").0, vec![0xff, 0xff]);
        assert_eq!(esc("a\\cb"), (b"a".to_vec(), true));
        assert_eq!(esc("\\u00e9").0, "é".as_bytes());
    }
}
