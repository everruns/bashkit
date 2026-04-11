//! base64 builtin command - encode/decode base64

use async_trait::async_trait;
use base64::Engine;

use super::arg_parser::ArgParser;
use super::{Builtin, Context, read_text_file};
use crate::error::Result;
use crate::interpreter::ExecResult;

/// The base64 builtin command.
///
/// Usage: base64 [-d|--decode] [-w COLS|--wrap=COLS] [FILE]
///
/// Options:
///   -d, --decode    Decode base64 input
///   -w COLS         Wrap encoded lines after COLS characters (default: 76, 0 = no wrap)
pub struct Base64;

#[async_trait]
impl Builtin for Base64 {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        if let Some(r) = super::check_help_version(
            ctx.args,
            "Usage: base64 [OPTION]... [FILE]\nBase64 encode or decode FILE, or standard input.\n\n  -d, --decode\tdecode data\n  -w COLS, --wrap=COLS\twrap encoded lines after COLS characters (default 76)\n  -i, --ignore-garbage\twhen decoding, ignore non-alphabet characters\n  --help\tdisplay this help and exit\n  --version\toutput version information and exit\n",
            Some("base64 (bashkit) 0.1"),
        ) {
            return Ok(r);
        }
        let mut decode = false;
        let mut wrap = 76usize;
        let mut file: Option<String> = None;

        let mut parser = ArgParser::new(ctx.args);
        while !parser.is_done() {
            if parser.flag_any(&["-d", "--decode"]) {
                decode = true;
            } else if let Some(val) = parser.current().and_then(|s| s.strip_prefix("--wrap=")) {
                wrap = val.parse().unwrap_or(76);
                parser.advance();
            } else if let Some(val) = match parser.flag_value("-w", "base64") {
                Ok(v) => v,
                Err(e) => return Ok(ExecResult::err(format!("{e}\n"), 1)),
            } {
                wrap = val.parse().unwrap_or(76);
            } else if parser.flag_any(&["-i", "--ignore-garbage"]) {
                // silently accept
            } else if parser.is_flag() {
                if let Some(s) = parser.positional() {
                    let msg = format!("base64: invalid option -- '{}'\n", &s[1..]);
                    return Ok(ExecResult::err(msg, 1));
                }
            } else if let Some(arg) = parser.positional() {
                file = Some(arg.to_string());
            }
        }

        // Get input: from file, stdin, or empty
        let input = if let Some(ref path) = file {
            if path == "-" {
                ctx.stdin.unwrap_or("").to_string()
            } else {
                let resolved = super::resolve_path(ctx.cwd, path);
                match read_text_file(ctx.fs.as_ref(), &resolved, "base64").await {
                    Ok(text) => text,
                    Err(_) => {
                        return Ok(ExecResult::err(
                            format!("base64: {}: No such file or directory\n", path),
                            1,
                        ));
                    }
                }
            }
        } else {
            ctx.stdin.unwrap_or("").to_string()
        };

        if decode {
            // Decode: strip whitespace, then decode
            let cleaned: String = input.chars().filter(|c| !c.is_whitespace()).collect();
            match base64::engine::general_purpose::STANDARD.decode(&cleaned) {
                Ok(bytes) => {
                    // Output raw bytes as string (lossy for non-UTF8)
                    let output = String::from_utf8_lossy(&bytes).to_string();
                    Ok(ExecResult::ok(output))
                }
                Err(e) => Ok(ExecResult::err(format!("base64: invalid input: {e}\n"), 1)),
            }
        } else {
            // Encode
            let encoded =
                base64::engine::general_purpose::STANDARD.encode(input.trim_end_matches('\n'));
            let output = if wrap > 0 {
                // Wrap at specified column width
                let mut wrapped = String::new();
                for (i, ch) in encoded.chars().enumerate() {
                    if i > 0 && i % wrap == 0 {
                        wrapped.push('\n');
                    }
                    wrapped.push(ch);
                }
                wrapped.push('\n');
                wrapped
            } else {
                format!("{encoded}\n")
            };
            Ok(ExecResult::ok(output))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::InMemoryFs;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Arc;

    async fn run_base64(args: &[&str], stdin: Option<&str>) -> ExecResult {
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let env = HashMap::new();
        let mut variables = HashMap::new();
        let mut cwd = PathBuf::from("/");
        let fs = Arc::new(InMemoryFs::new()) as Arc<dyn crate::fs::FileSystem>;
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs, stdin);
        Base64.execute(ctx).await.expect("base64 execute failed")
    }

    #[tokio::test]
    async fn test_encode_basic() {
        let result = run_base64(&[], Some("hello world")).await;
        assert_eq!(result.stdout.trim(), "aGVsbG8gd29ybGQ=");
    }

    #[tokio::test]
    async fn test_decode_basic() {
        let result = run_base64(&["-d"], Some("aGVsbG8gd29ybGQ=")).await;
        assert_eq!(result.stdout, "hello world");
    }

    #[tokio::test]
    async fn test_decode_long_flag() {
        let result = run_base64(&["--decode"], Some("aGVsbG8gd29ybGQ=")).await;
        assert_eq!(result.stdout, "hello world");
    }

    #[tokio::test]
    async fn test_wrap_zero() {
        // Long input that would normally wrap
        let input = "a]".repeat(50);
        let result = run_base64(&["-w", "0"], Some(&input)).await;
        // Should be single line (no internal newlines except trailing)
        assert!(
            !result.stdout.trim().contains('\n'),
            "should not wrap with -w 0"
        );
    }

    #[tokio::test]
    async fn test_decode_invalid() {
        let result = run_base64(&["-d"], Some("!!!not-base64!!!")).await;
        assert_ne!(result.exit_code, 0);
        assert!(result.stderr.contains("invalid input"));
    }

    #[tokio::test]
    async fn test_roundtrip() {
        let original = "The quick brown fox jumps over the lazy dog";
        let encoded = run_base64(&["-w", "0"], Some(original)).await;
        let decoded = run_base64(&["-d"], Some(encoded.stdout.trim())).await;
        assert_eq!(decoded.stdout, original);
    }
}
