//! base64 builtin command - encode/decode base64

use async_trait::async_trait;
use base64::Engine;

use super::arg_parser::OptArg;
use super::{Builtin, BuiltinHelper, Context};
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

impl BuiltinHelper for Base64 {
    const NAME: &'static str = "base64";
}

#[async_trait]
impl Builtin for Base64 {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        if let Some(r) = Self::check_help(
            ctx.args,
            "Usage: base64 [OPTION]... [FILE]\nBase64 encode or decode FILE, or standard input.\n\n  -d, --decode\tdecode data\n  -w COLS, --wrap=COLS\twrap encoded lines after COLS characters (default 76)\n  -i, --ignore-garbage\twhen decoding, ignore non-alphabet characters\n  --help\tdisplay this help and exit\n  --version\toutput version information and exit\n",
            Some("base64 (bashkit) 0.1"),
        ) {
            return Ok(r);
        }
        let (parsed, operands) = match super::arg_parser::gnu_getopt(
            "base64",
            ctx.args,
            "diw:",
            &[
                ("decode", OptArg::No, 'd'),
                ("ignore-garbage", OptArg::No, 'i'),
                ("wrap", OptArg::Required, 'w'),
            ],
            true,
            1,
        ) {
            Ok(v) => v,
            Err(e) => return Ok(e),
        };
        let mut decode = false;
        let mut ignore_garbage = false;
        let mut wrap = 76usize;
        for o in parsed {
            match o.key {
                'd' => decode = true,
                'i' => ignore_garbage = true,
                _ => {
                    let val = o.value.unwrap_or_default();
                    wrap = match val.parse() {
                        Ok(w) => w,
                        Err(_) => {
                            return Ok(Self::err(format!("invalid wrap size: '{val}'"), 1));
                        }
                    };
                }
            }
        }
        if operands.len() > 1 {
            return Ok(Self::err(format!("extra operand '{}'", operands[1]), 1));
        }
        let file = operands.into_iter().next();

        // Byte streams and files share the same exact representation.
        let input = if let Some(ref path) = file {
            if path == "-" {
                ctx.stdin_bytes().unwrap_or_default().to_vec()
            } else {
                let resolved = super::resolve_path(ctx.cwd, path);
                match ctx.fs.read_file(&resolved).await {
                    Ok(bytes) => bytes,
                    Err(_) => {
                        return Ok(Self::err_path(path, "No such file or directory", 1));
                    }
                }
            }
        } else {
            ctx.stdin_bytes().unwrap_or_default().to_vec()
        };

        if decode {
            // Decode: strip ASCII whitespace, then decode.
            let cleaned: Vec<u8> = input
                .into_iter()
                .filter(|byte| {
                    if ignore_garbage {
                        byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'/' | b'=')
                    } else {
                        !byte.is_ascii_whitespace()
                    }
                })
                .collect();
            match base64::engine::general_purpose::STANDARD.decode(&cleaned) {
                Ok(bytes) => Ok(ExecResult::ok_bytes(bytes)),
                Err(e) => Ok(Self::err(format!("invalid input: {e}"), 1)),
            }
        } else {
            // Encode exact input bytes, including trailing newlines.
            let encoded = base64::engine::general_purpose::STANDARD.encode(input);
            // GNU: a newline after every WRAP columns and after a final
            // partial line; `-w 0` never wraps and adds no newline.
            let output = if wrap > 0 {
                let mut wrapped = String::with_capacity(encoded.len() + encoded.len() / wrap + 1);
                for (i, ch) in encoded.chars().enumerate() {
                    if i > 0 && i % wrap == 0 {
                        wrapped.push('\n');
                    }
                    wrapped.push(ch);
                }
                if !encoded.is_empty() {
                    wrapped.push('\n');
                }
                wrapped
            } else {
                encoded
            };
            Ok(ExecResult::ok(output))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::{FileSystem, InMemoryFs};
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
    async fn test_encode_preserves_trailing_newline() {
        let result = run_base64(&["-w", "0"], Some("hello\n")).await;
        assert_eq!(result.stdout, "aGVsbG8K");
    }

    #[tokio::test]
    async fn test_encode_binary_file_preserves_bytes() {
        let args = vec!["-w".to_string(), "0".to_string(), "/blob".to_string()];
        let env = HashMap::new();
        let mut variables = HashMap::new();
        let mut cwd = PathBuf::from("/");
        let fs = Arc::new(InMemoryFs::new());
        fs.write_file(PathBuf::from("/blob").as_path(), &[0xff, b'\n', b'\n'])
            .await
            .unwrap();
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs, None);

        let result = Base64.execute(ctx).await.expect("base64 execute failed");

        assert_eq!(result.stdout, "/woK");
    }

    #[tokio::test]
    async fn test_decode_binary_preserves_stdout_bytes() {
        let result = run_base64(&["-d"], Some("AAH//kIAfw==")).await;
        assert_eq!(
            result.stdout.as_bytes(),
            &[0x00, 0x01, 0xff, 0xfe, b'B', 0x00, 0x7f]
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
