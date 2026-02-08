//! strings builtin command - print printable strings from binary data

use async_trait::async_trait;

use super::{Builtin, Context};
use crate::error::Result;
use crate::interpreter::ExecResult;

/// The strings builtin - find printable strings in files.
///
/// Usage: strings [-n MIN] [-t FORMAT] [FILE...]
///
/// Options:
///   -n MIN     Minimum string length (default: 4)
///   -t FORMAT  Print offset: d (decimal), o (octal), x (hex)
///   -a         Scan entire file (default, for compatibility)
pub struct Strings;

struct StringsOptions {
    min_length: usize,
    offset_format: Option<OffsetFormat>,
}

#[derive(Clone, Copy)]
enum OffsetFormat {
    Decimal,
    Octal,
    Hex,
}

fn parse_strings_args(
    args: &[String],
) -> std::result::Result<(StringsOptions, Vec<String>), String> {
    let mut opts = StringsOptions {
        min_length: 4,
        offset_format: None,
    };
    let mut files = Vec::new();
    let mut i = 0;

    while i < args.len() {
        let arg = &args[i];
        if arg == "-n" {
            i += 1;
            if i < args.len() {
                opts.min_length = args[i].parse().map_err(|_| {
                    format!("strings: invalid minimum string length: '{}'", args[i])
                })?;
            }
        } else if let Some(val) = arg.strip_prefix("-n") {
            opts.min_length = val
                .parse()
                .map_err(|_| format!("strings: invalid minimum string length: '{}'", val))?;
        } else if arg == "-t" {
            i += 1;
            if i < args.len() {
                opts.offset_format = Some(match args[i].as_str() {
                    "d" => OffsetFormat::Decimal,
                    "o" => OffsetFormat::Octal,
                    "x" => OffsetFormat::Hex,
                    other => {
                        return Err(format!("strings: invalid radix for -t: '{}'", other));
                    }
                });
            }
        } else if arg == "-a" {
            // Default behavior, ignore
        } else if arg == "-" || !arg.starts_with('-') {
            files.push(arg.clone());
        } else if let Some(rest) = arg.strip_prefix('-') {
            // Try parsing as -NUM shorthand
            if let Ok(n) = rest.parse::<usize>() {
                opts.min_length = n;
            }
        }
        i += 1;
    }

    if opts.min_length == 0 {
        opts.min_length = 1;
    }

    Ok((opts, files))
}

fn extract_strings(data: &[u8], opts: &StringsOptions) -> String {
    let mut output = String::new();
    let mut current = String::new();
    let mut start_offset = 0;

    for (i, &byte) in data.iter().enumerate() {
        if (0x20..0x7f).contains(&byte) || byte == b'\t' {
            if current.is_empty() {
                start_offset = i;
            }
            current.push(byte as char);
        } else {
            if current.len() >= opts.min_length {
                if let Some(fmt) = opts.offset_format {
                    match fmt {
                        OffsetFormat::Decimal => {
                            output.push_str(&format!("{:>7} ", start_offset));
                        }
                        OffsetFormat::Octal => {
                            output.push_str(&format!("{:>7o} ", start_offset));
                        }
                        OffsetFormat::Hex => {
                            output.push_str(&format!("{:>7x} ", start_offset));
                        }
                    }
                }
                output.push_str(&current);
                output.push('\n');
            }
            current.clear();
        }
    }

    // Don't forget the last string
    if current.len() >= opts.min_length {
        if let Some(fmt) = opts.offset_format {
            match fmt {
                OffsetFormat::Decimal => {
                    output.push_str(&format!("{:>7} ", start_offset));
                }
                OffsetFormat::Octal => {
                    output.push_str(&format!("{:>7o} ", start_offset));
                }
                OffsetFormat::Hex => {
                    output.push_str(&format!("{:>7x} ", start_offset));
                }
            }
        }
        output.push_str(&current);
        output.push('\n');
    }

    output
}

#[async_trait]
impl Builtin for Strings {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let (opts, files) = match parse_strings_args(ctx.args) {
            Ok(v) => v,
            Err(e) => return Ok(ExecResult::err(format!("{}\n", e), 1)),
        };

        let mut output = String::new();

        if files.is_empty() {
            // Read from stdin (treat as bytes)
            if let Some(stdin) = ctx.stdin {
                output.push_str(&extract_strings(stdin.as_bytes(), &opts));
            }
        } else {
            for file in &files {
                if file == "-" {
                    if let Some(stdin) = ctx.stdin {
                        output.push_str(&extract_strings(stdin.as_bytes(), &opts));
                    }
                } else {
                    let path = if file.starts_with('/') {
                        std::path::PathBuf::from(file)
                    } else {
                        ctx.cwd.join(file)
                    };

                    match ctx.fs.read_file(&path).await {
                        Ok(content) => {
                            output.push_str(&extract_strings(&content, &opts));
                        }
                        Err(e) => {
                            return Ok(ExecResult::err(format!("strings: {}: {}\n", file, e), 1));
                        }
                    }
                }
            }
        }

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

    use crate::fs::{FileSystem, InMemoryFs};

    async fn run_strings(args: &[&str], stdin: Option<&str>) -> ExecResult {
        let fs = Arc::new(InMemoryFs::new());
        let mut variables = HashMap::new();
        let env = HashMap::new();
        let mut cwd = PathBuf::from("/");

        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs,
            stdin,
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
        };

        Strings.execute(ctx).await.unwrap()
    }

    async fn run_strings_with_fs(args: &[&str], files: &[(&str, &[u8])]) -> ExecResult {
        let fs = Arc::new(InMemoryFs::new());
        for (path, content) in files {
            fs.write_file(std::path::Path::new(path), content)
                .await
                .unwrap();
        }
        let mut variables = HashMap::new();
        let env = HashMap::new();
        let mut cwd = PathBuf::from("/");

        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs,
            stdin: None,
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
        };

        Strings.execute(ctx).await.unwrap()
    }

    #[tokio::test]
    async fn test_strings_basic() {
        let result = run_strings(&[], Some("hello world")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "hello world\n");
    }

    #[tokio::test]
    async fn test_strings_binary_data() {
        // Mix of binary and text
        let mut data = vec![0u8, 1, 2, 3];
        data.extend_from_slice(b"hello");
        data.extend_from_slice(&[0, 1, 2]);
        data.extend_from_slice(b"world");
        data.push(0);

        let result = run_strings_with_fs(&["/test.bin"], &[("/test.bin", &data)]).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "hello\nworld\n");
    }

    #[tokio::test]
    async fn test_strings_min_length() {
        let result = run_strings(&["-n", "8"], Some("hi there how are you")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "hi there how are you\n");
    }

    #[tokio::test]
    async fn test_strings_min_length_filter() {
        let mut data = Vec::new();
        data.extend_from_slice(b"ab");
        data.push(0);
        data.extend_from_slice(b"cdef");
        data.push(0);
        data.extend_from_slice(b"ghijklm");
        data.push(0);

        let result = run_strings_with_fs(&["-n", "4", "/test.bin"], &[("/test.bin", &data)]).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "cdef\nghijklm\n");
    }

    #[tokio::test]
    async fn test_strings_short_min() {
        let mut data = Vec::new();
        data.extend_from_slice(b"ab");
        data.push(0);
        data.extend_from_slice(b"cd");
        data.push(0);

        let result = run_strings_with_fs(&["-n", "2", "/test.bin"], &[("/test.bin", &data)]).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "ab\ncd\n");
    }

    #[tokio::test]
    async fn test_strings_offset_decimal() {
        let mut data = vec![0u8; 10];
        data.extend_from_slice(b"hello");
        data.push(0);

        let result = run_strings_with_fs(&["-t", "d", "/test.bin"], &[("/test.bin", &data)]).await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("10 hello"));
    }

    #[tokio::test]
    async fn test_strings_offset_hex() {
        let mut data = vec![0u8; 16];
        data.extend_from_slice(b"test");
        data.push(0);

        let result = run_strings_with_fs(&["-t", "x", "/test.bin"], &[("/test.bin", &data)]).await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("10 test"));
    }

    #[tokio::test]
    async fn test_strings_empty_input() {
        let result = run_strings(&[], Some("")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "");
    }

    #[tokio::test]
    async fn test_strings_all_binary() {
        let data = vec![0u8, 1, 2, 3, 4, 5];
        let result = run_strings_with_fs(&["/test.bin"], &[("/test.bin", &data)]).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "");
    }

    #[tokio::test]
    async fn test_strings_file_not_found() {
        let result = run_strings(&["/nonexistent"], None).await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("strings:"));
    }

    #[tokio::test]
    async fn test_strings_tab_is_printable() {
        let result = run_strings(&["-n", "1"], Some("a\tb")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "a\tb\n");
    }

    #[tokio::test]
    async fn test_strings_multiple_sequences() {
        let mut data = Vec::new();
        data.extend_from_slice(b"first");
        data.extend_from_slice(&[0, 0, 0]);
        data.extend_from_slice(b"second");
        data.extend_from_slice(&[0, 0]);
        data.extend_from_slice(b"third");

        let result = run_strings_with_fs(&["/test.bin"], &[("/test.bin", &data)]).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "first\nsecond\nthird\n");
    }

    #[tokio::test]
    async fn test_strings_invalid_min_length() {
        let result = run_strings(&["-n", "abc"], Some("test")).await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("invalid minimum string length"));
    }
}
