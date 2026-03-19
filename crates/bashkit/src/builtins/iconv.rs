//! iconv builtin - character encoding conversion (virtual)
//!
//! Converts text between character encodings in a virtual environment.
//! Supports utf-8, ascii, latin1/iso-8859-1, utf-16.
//!
//! Usage:
//!   iconv -f UTF-8 -t LATIN1 file.txt
//!   echo "hello" | iconv -f UTF-8 -t ASCII
//!   iconv -l

use async_trait::async_trait;

use super::{Builtin, Context, resolve_path};
use crate::error::Result;
use crate::interpreter::ExecResult;

/// iconv builtin - character encoding conversion.
pub struct Iconv;

/// Normalize encoding name to canonical form.
fn normalize_encoding(name: &str) -> Option<&'static str> {
    match name.to_ascii_lowercase().replace('-', "").as_str() {
        "utf8" => Some("utf-8"),
        "ascii" | "usascii" => Some("ascii"),
        "latin1" | "iso88591" | "iso_88591" => Some("latin1"),
        "utf16" | "utf16le" => Some("utf-16"),
        "utf16be" => Some("utf-16be"),
        _ => None,
    }
}

const SUPPORTED_ENCODINGS: &[&str] = &[
    "ASCII",
    "ISO-8859-1",
    "LATIN1",
    "UTF-16",
    "UTF-16BE",
    "UTF-8",
];

/// Encode bytes from UTF-8 string into target encoding.
fn encode_to(input: &str, encoding: &str) -> std::result::Result<Vec<u8>, String> {
    match encoding {
        "utf-8" => Ok(input.as_bytes().to_vec()),
        "ascii" => {
            for (i, b) in input.bytes().enumerate() {
                if b > 127 {
                    return Err(format!(
                        "iconv: cannot convert character at byte {i} to ASCII\n"
                    ));
                }
            }
            Ok(input.as_bytes().to_vec())
        }
        "latin1" => {
            let mut out = Vec::with_capacity(input.len());
            for ch in input.chars() {
                let cp = ch as u32;
                if cp > 255 {
                    return Err(format!("iconv: cannot convert U+{cp:04X} to LATIN1\n"));
                }
                out.push(cp as u8);
            }
            Ok(out)
        }
        "utf-16" => {
            let mut out = Vec::new();
            // BOM little-endian
            out.extend_from_slice(&[0xFF, 0xFE]);
            for ch in input.chars() {
                let mut buf = [0u16; 2];
                let encoded = ch.encode_utf16(&mut buf);
                for u in encoded {
                    out.extend_from_slice(&u.to_le_bytes());
                }
            }
            Ok(out)
        }
        "utf-16be" => {
            let mut out = Vec::new();
            for ch in input.chars() {
                let mut buf = [0u16; 2];
                let encoded = ch.encode_utf16(&mut buf);
                for u in encoded {
                    out.extend_from_slice(&u.to_be_bytes());
                }
            }
            Ok(out)
        }
        _ => Err(format!("iconv: unsupported target encoding '{encoding}'\n")),
    }
}

/// Decode bytes from source encoding into UTF-8 string.
fn decode_from(input: &[u8], encoding: &str) -> std::result::Result<String, String> {
    match encoding {
        "utf-8" => String::from_utf8(input.to_vec())
            .map_err(|e| format!("iconv: invalid UTF-8 input: {e}\n")),
        "ascii" => {
            for (i, &b) in input.iter().enumerate() {
                if b > 127 {
                    return Err(format!(
                        "iconv: invalid ASCII byte 0x{b:02X} at position {i}\n"
                    ));
                }
            }
            // ASCII is a subset of UTF-8
            Ok(String::from_utf8(input.to_vec()).unwrap_or_default())
        }
        "latin1" => {
            // Each byte maps directly to a Unicode codepoint
            Ok(input.iter().map(|&b| b as char).collect())
        }
        "utf-16" => {
            if input.len() < 2 {
                return Err("iconv: UTF-16 input too short\n".to_string());
            }
            // Check BOM
            let (data, big_endian) = if input[0] == 0xFF && input[1] == 0xFE {
                (&input[2..], false)
            } else if input[0] == 0xFE && input[1] == 0xFF {
                (&input[2..], true)
            } else {
                // Default to little-endian (no BOM)
                (input, false)
            };
            if data.len() % 2 != 0 {
                return Err("iconv: UTF-16 input has odd byte count\n".to_string());
            }
            let units: Vec<u16> = data
                .chunks_exact(2)
                .map(|c| {
                    if big_endian {
                        u16::from_be_bytes([c[0], c[1]])
                    } else {
                        u16::from_le_bytes([c[0], c[1]])
                    }
                })
                .collect();
            String::from_utf16(&units).map_err(|e| format!("iconv: invalid UTF-16 input: {e}\n"))
        }
        "utf-16be" => {
            if !input.len().is_multiple_of(2) {
                return Err("iconv: UTF-16BE input has odd byte count\n".to_string());
            }
            let units: Vec<u16> = input
                .chunks_exact(2)
                .map(|c| u16::from_be_bytes([c[0], c[1]]))
                .collect();
            String::from_utf16(&units).map_err(|e| format!("iconv: invalid UTF-16BE input: {e}\n"))
        }
        _ => Err(format!("iconv: unsupported source encoding '{encoding}'\n")),
    }
}

#[async_trait]
impl Builtin for Iconv {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let mut from_enc: Option<String> = None;
        let mut to_enc: Option<String> = None;
        let mut file_arg: Option<String> = None;
        let mut list = false;

        let mut i = 0;
        while i < ctx.args.len() {
            match ctx.args[i].as_str() {
                "-l" | "--list" => {
                    list = true;
                }
                "-f" => {
                    i += 1;
                    if i >= ctx.args.len() {
                        return Ok(ExecResult::err(
                            "iconv: option '-f' requires an argument\n".to_string(),
                            1,
                        ));
                    }
                    from_enc = Some(ctx.args[i].clone());
                }
                "-t" => {
                    i += 1;
                    if i >= ctx.args.len() {
                        return Ok(ExecResult::err(
                            "iconv: option '-t' requires an argument\n".to_string(),
                            1,
                        ));
                    }
                    to_enc = Some(ctx.args[i].clone());
                }
                arg if arg.starts_with('-') => {
                    return Ok(ExecResult::err(
                        format!("iconv: unknown option '{arg}'\n"),
                        1,
                    ));
                }
                _ => {
                    file_arg = Some(ctx.args[i].clone());
                }
            }
            i += 1;
        }

        if list {
            let mut out = String::new();
            for enc in SUPPORTED_ENCODINGS {
                out.push_str(enc);
                out.push('\n');
            }
            return Ok(ExecResult::ok(out));
        }

        let from = match &from_enc {
            Some(f) => match normalize_encoding(f) {
                Some(e) => e,
                None => {
                    return Ok(ExecResult::err(
                        format!("iconv: unsupported encoding '{}'\n", f),
                        1,
                    ));
                }
            },
            None => {
                return Ok(ExecResult::err(
                    "iconv: missing source encoding (-f)\n".to_string(),
                    1,
                ));
            }
        };

        let to = match &to_enc {
            Some(t) => match normalize_encoding(t) {
                Some(e) => e,
                None => {
                    return Ok(ExecResult::err(
                        format!("iconv: unsupported encoding '{}'\n", t),
                        1,
                    ));
                }
            },
            None => {
                return Ok(ExecResult::err(
                    "iconv: missing target encoding (-t)\n".to_string(),
                    1,
                ));
            }
        };

        // Read input from file or stdin
        let input_bytes: Vec<u8> = if let Some(ref file) = file_arg {
            let path = resolve_path(ctx.cwd, file);
            match ctx.fs.read_file(&path).await {
                Ok(bytes) => bytes,
                Err(e) => return Ok(ExecResult::err(format!("iconv: {}: {e}\n", file), 1)),
            }
        } else if let Some(stdin) = ctx.stdin {
            stdin.as_bytes().to_vec()
        } else {
            return Ok(ExecResult::err(
                "iconv: no input (provide file argument or pipe stdin)\n".to_string(),
                1,
            ));
        };

        // Decode from source encoding to UTF-8 string
        let text = match decode_from(&input_bytes, from) {
            Ok(t) => t,
            Err(e) => return Ok(ExecResult::err(e, 1)),
        };

        // Encode from UTF-8 string to target encoding
        let output_bytes = match encode_to(&text, to) {
            Ok(b) => b,
            Err(e) => return Ok(ExecResult::err(e, 1)),
        };

        // For text-compatible encodings, output as string; otherwise raw bytes as lossy UTF-8
        let output = match to {
            "utf-8" | "ascii" => String::from_utf8_lossy(&output_bytes).to_string(),
            "latin1" => {
                // Each Latin1 byte maps directly to a Unicode codepoint
                output_bytes.iter().map(|&b| b as char).collect()
            }
            _ => {
                // Binary output - present as lossy string (VFS limitation)
                String::from_utf8_lossy(&output_bytes).to_string()
            }
        };

        Ok(ExecResult::ok(output))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Arc;

    use crate::fs::InMemoryFs;

    async fn run(args: &[&str], stdin: Option<&str>, fs: Option<Arc<InMemoryFs>>) -> ExecResult {
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let env = HashMap::new();
        let mut variables = HashMap::new();
        let mut cwd = PathBuf::from("/");
        let fs = fs.unwrap_or_else(|| Arc::new(InMemoryFs::new()));
        let fs_dyn = fs as Arc<dyn crate::fs::FileSystem>;
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs: fs_dyn,
            stdin,
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
            shell: None,
        };
        Iconv.execute(ctx).await.unwrap()
    }

    #[tokio::test]
    async fn test_list_encodings() {
        let r = run(&["-l"], None, None).await;
        assert_eq!(r.exit_code, 0);
        assert!(r.stdout.contains("UTF-8"));
        assert!(r.stdout.contains("ASCII"));
        assert!(r.stdout.contains("LATIN1"));
    }

    #[tokio::test]
    async fn test_utf8_to_ascii() {
        let r = run(&["-f", "UTF-8", "-t", "ASCII"], Some("hello"), None).await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "hello");
    }

    #[tokio::test]
    async fn test_utf8_to_ascii_fails_on_nonascii() {
        let r = run(&["-f", "UTF-8", "-t", "ASCII"], Some("caf\u{00e9}"), None).await;
        assert_eq!(r.exit_code, 1);
        assert!(r.stderr.contains("cannot convert"));
    }

    #[tokio::test]
    async fn test_utf8_to_latin1() {
        let r = run(&["-f", "UTF-8", "-t", "LATIN1"], Some("caf\u{00e9}"), None).await;
        assert_eq!(r.exit_code, 0);
        // Latin1 byte 0xe9 maps to Unicode U+00E9 (é), so output is valid UTF-8 "café"
        assert_eq!(r.stdout, "caf\u{00e9}");
    }

    #[tokio::test]
    async fn test_latin1_to_utf8() {
        let fs = Arc::new(InMemoryFs::new());
        // Write Latin1 bytes: "caf" + 0xe9 (e-acute)
        let latin1_bytes = vec![b'c', b'a', b'f', 0xe9];
        let path = std::path::Path::new("/test.txt");
        let fs_dyn = fs.clone() as Arc<dyn crate::fs::FileSystem>;
        fs_dyn.write_file(path, &latin1_bytes).await.unwrap();

        let r = run(
            &["-f", "LATIN1", "-t", "UTF-8", "/test.txt"],
            None,
            Some(fs),
        )
        .await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "caf\u{00e9}");
    }

    #[tokio::test]
    async fn test_missing_from_encoding() {
        let r = run(&["-t", "ASCII"], Some("hi"), None).await;
        assert_eq!(r.exit_code, 1);
        assert!(r.stderr.contains("missing source encoding"));
    }

    #[tokio::test]
    async fn test_missing_to_encoding() {
        let r = run(&["-f", "ASCII"], Some("hi"), None).await;
        assert_eq!(r.exit_code, 1);
        assert!(r.stderr.contains("missing target encoding"));
    }

    #[tokio::test]
    async fn test_unsupported_encoding() {
        let r = run(&["-f", "EBCDIC", "-t", "UTF-8"], Some("hi"), None).await;
        assert_eq!(r.exit_code, 1);
        assert!(r.stderr.contains("unsupported encoding"));
    }

    #[tokio::test]
    async fn test_no_input() {
        let r = run(&["-f", "UTF-8", "-t", "ASCII"], None, None).await;
        assert_eq!(r.exit_code, 1);
        assert!(r.stderr.contains("no input"));
    }

    #[tokio::test]
    async fn test_file_not_found() {
        let r = run(&["-f", "UTF-8", "-t", "ASCII", "/nope.txt"], None, None).await;
        assert_eq!(r.exit_code, 1);
        assert!(r.stderr.contains("/nope.txt"));
    }

    #[tokio::test]
    async fn test_identity_conversion() {
        let r = run(&["-f", "UTF-8", "-t", "UTF-8"], Some("hello world\n"), None).await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "hello world\n");
    }
}
