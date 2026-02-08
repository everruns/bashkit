//! od, xxd, and hexdump builtins - byte-level inspection tools

use async_trait::async_trait;

use super::{Builtin, Context};
use crate::error::Result;
use crate::interpreter::ExecResult;

/// The od builtin - dump files in octal and other formats.
///
/// Usage: od [-A RADIX] [-t TYPE] [-N COUNT] [-j SKIP] [FILE...]
///
/// Options:
///   -A RADIX   Address radix: d (decimal), o (octal, default), x (hex), n (none)
///   -t TYPE    Output type: o (octal, default), x (hex), d (decimal), c (char)
///   -N COUNT   Dump at most COUNT bytes
///   -j SKIP    Skip SKIP bytes from beginning
pub struct Od;

/// The xxd builtin - make a hexdump or do the reverse.
///
/// Usage: xxd [-l LEN] [-s OFFSET] [-c COLS] [-g GROUP] [-p] [FILE...]
///
/// Options:
///   -l LEN     Stop after LEN bytes
///   -s OFFSET  Start at OFFSET bytes
///   -c COLS    Bytes per line (default: 16)
///   -g GROUP   Bytes per group (default: 2)
///   -p         Plain hex dump (no offsets, no ASCII)
///   -r         Reverse: convert hexdump back to binary (not implemented)
pub struct Xxd;

/// The hexdump builtin - display file contents in hex.
///
/// Usage: hexdump [-C] [-n LENGTH] [-s OFFSET] [FILE...]
///
/// Options:
///   -C         Canonical hex+ASCII display
///   -n LENGTH  Interpret only LENGTH bytes
///   -s OFFSET  Skip OFFSET bytes from beginning
pub struct Hexdump;

// --- Od implementation ---

struct OdOptions {
    addr_radix: AddrRadix,
    output_type: OutputType,
    count: Option<usize>,
    skip: usize,
}

#[derive(Clone, Copy)]
enum AddrRadix {
    Octal,
    Decimal,
    Hex,
    None,
}

#[derive(Clone, Copy)]
enum OutputType {
    Octal,
    Hex,
    Decimal,
    Char,
}

fn parse_od_args(args: &[String]) -> std::result::Result<(OdOptions, Vec<String>), String> {
    let mut opts = OdOptions {
        addr_radix: AddrRadix::Octal,
        output_type: OutputType::Octal,
        count: None,
        skip: 0,
    };
    let mut files = Vec::new();
    let mut i = 0;

    while i < args.len() {
        let arg = &args[i];
        if arg == "-A" {
            i += 1;
            if i < args.len() {
                opts.addr_radix = match args[i].as_str() {
                    "d" => AddrRadix::Decimal,
                    "o" => AddrRadix::Octal,
                    "x" => AddrRadix::Hex,
                    "n" => AddrRadix::None,
                    other => return Err(format!("od: invalid address radix: '{}'", other)),
                };
            }
        } else if arg == "-t" {
            i += 1;
            if i < args.len() {
                opts.output_type = match args[i].chars().next() {
                    Some('o') => OutputType::Octal,
                    Some('x') => OutputType::Hex,
                    Some('d') => OutputType::Decimal,
                    Some('c') => OutputType::Char,
                    _ => return Err(format!("od: invalid output type: '{}'", args[i])),
                };
            }
        } else if arg == "-N" {
            i += 1;
            if i < args.len() {
                opts.count = Some(
                    args[i]
                        .parse()
                        .map_err(|_| format!("od: invalid count: '{}'", args[i]))?,
                );
            }
        } else if arg == "-j" {
            i += 1;
            if i < args.len() {
                opts.skip = args[i]
                    .parse()
                    .map_err(|_| format!("od: invalid skip: '{}'", args[i]))?;
            }
        } else if arg == "-x" {
            opts.output_type = OutputType::Hex;
        } else if arg == "-c" {
            opts.output_type = OutputType::Char;
        } else if arg == "-d" {
            opts.output_type = OutputType::Decimal;
        } else if !arg.starts_with('-') || arg == "-" {
            files.push(arg.clone());
        }
        i += 1;
    }

    Ok((opts, files))
}

fn format_od_addr(offset: usize, radix: AddrRadix) -> String {
    match radix {
        AddrRadix::Octal => format!("{:07o}", offset),
        AddrRadix::Decimal => format!("{:07}", offset),
        AddrRadix::Hex => format!("{:07x}", offset),
        AddrRadix::None => String::new(),
    }
}

fn format_od_byte(byte: u8, output_type: OutputType) -> String {
    match output_type {
        OutputType::Octal => format!(" {:03o}", byte),
        OutputType::Hex => format!(" {:02x}", byte),
        OutputType::Decimal => format!(" {:3}", byte),
        OutputType::Char => {
            let c = match byte {
                0 => "\\0".to_string(),
                7 => "\\a".to_string(),
                8 => "\\b".to_string(),
                9 => "\\t".to_string(),
                10 => "\\n".to_string(),
                11 => "\\v".to_string(),
                12 => "\\f".to_string(),
                13 => "\\r".to_string(),
                0x20..=0x7e => format!("  {}", byte as char),
                _ => format!(" {:03o}", byte),
            };
            format!(" {}", c.trim_start())
        }
    }
}

fn od_dump(data: &[u8], opts: &OdOptions) -> String {
    let bytes_per_line = 16;
    let mut output = String::new();

    let data = if opts.skip < data.len() {
        &data[opts.skip..]
    } else {
        &[]
    };

    let data = match opts.count {
        Some(n) => &data[..data.len().min(n)],
        None => data,
    };

    for (chunk_idx, chunk) in data.chunks(bytes_per_line).enumerate() {
        let offset = opts.skip + chunk_idx * bytes_per_line;
        let addr = format_od_addr(offset, opts.addr_radix);
        if !addr.is_empty() {
            output.push_str(&addr);
        }

        for byte in chunk {
            output.push_str(&format_od_byte(*byte, opts.output_type));
        }
        output.push('\n');
    }

    // Final address line
    if !data.is_empty() {
        let final_offset = opts.skip + data.len();
        let addr = format_od_addr(final_offset, opts.addr_radix);
        if !addr.is_empty() {
            output.push_str(&addr);
            output.push('\n');
        }
    }

    output
}

#[async_trait]
impl Builtin for Od {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let (opts, files) = match parse_od_args(ctx.args) {
            Ok(v) => v,
            Err(e) => return Ok(ExecResult::err(format!("{}\n", e), 1)),
        };

        let data = collect_input(ctx.stdin, &files, ctx.cwd, &ctx.fs).await?;
        let output = od_dump(&data, &opts);

        Ok(ExecResult::ok(output))
    }
}

// --- Xxd implementation ---

struct XxdOptions {
    length: Option<usize>,
    offset: usize,
    cols: usize,
    group: usize,
    plain: bool,
}

fn parse_xxd_args(args: &[String]) -> std::result::Result<(XxdOptions, Vec<String>), String> {
    let mut opts = XxdOptions {
        length: None,
        offset: 0,
        cols: 16,
        group: 2,
        plain: false,
    };
    let mut files = Vec::new();
    let mut i = 0;

    while i < args.len() {
        let arg = &args[i];
        if arg == "-l" {
            i += 1;
            if i < args.len() {
                opts.length = Some(
                    args[i]
                        .parse()
                        .map_err(|_| format!("xxd: invalid length: '{}'", args[i]))?,
                );
            }
        } else if arg == "-s" {
            i += 1;
            if i < args.len() {
                opts.offset = args[i]
                    .parse()
                    .map_err(|_| format!("xxd: invalid offset: '{}'", args[i]))?;
            }
        } else if arg == "-c" {
            i += 1;
            if i < args.len() {
                opts.cols = args[i]
                    .parse()
                    .map_err(|_| format!("xxd: invalid cols: '{}'", args[i]))?;
                if opts.cols == 0 {
                    opts.cols = 16;
                }
            }
        } else if arg == "-g" {
            i += 1;
            if i < args.len() {
                opts.group = args[i]
                    .parse()
                    .map_err(|_| format!("xxd: invalid group: '{}'", args[i]))?;
            }
        } else if arg == "-p" {
            opts.plain = true;
        } else if !arg.starts_with('-') || arg == "-" {
            files.push(arg.clone());
        }
        i += 1;
    }

    Ok((opts, files))
}

fn xxd_dump(data: &[u8], opts: &XxdOptions) -> String {
    let mut output = String::new();

    let data = if opts.offset < data.len() {
        &data[opts.offset..]
    } else {
        &[]
    };

    let data = match opts.length {
        Some(n) => &data[..data.len().min(n)],
        None => data,
    };

    if opts.plain {
        for byte in data {
            output.push_str(&format!("{:02x}", byte));
        }
        if !data.is_empty() {
            output.push('\n');
        }
        return output;
    }

    for (chunk_idx, chunk) in data.chunks(opts.cols).enumerate() {
        let offset = opts.offset + chunk_idx * opts.cols;

        // Offset
        output.push_str(&format!("{:08x}: ", offset));

        // Hex bytes with grouping
        for (j, byte) in chunk.iter().enumerate() {
            if j > 0 && opts.group > 0 && j % opts.group == 0 {
                output.push(' ');
            }
            output.push_str(&format!("{:02x}", byte));
        }

        // Padding for short lines
        let missing = opts.cols - chunk.len();
        for k in 0..missing {
            if (chunk.len() + k) > 0 && opts.group > 0 && (chunk.len() + k) % opts.group == 0 {
                output.push(' ');
            }
            output.push_str("  ");
        }

        // ASCII representation
        output.push_str("  ");
        for byte in chunk {
            if *byte >= 0x20 && *byte < 0x7f {
                output.push(*byte as char);
            } else {
                output.push('.');
            }
        }

        output.push('\n');
    }

    output
}

#[async_trait]
impl Builtin for Xxd {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let (opts, files) = match parse_xxd_args(ctx.args) {
            Ok(v) => v,
            Err(e) => return Ok(ExecResult::err(format!("{}\n", e), 1)),
        };

        let data = collect_input(ctx.stdin, &files, ctx.cwd, &ctx.fs).await?;
        let output = xxd_dump(&data, &opts);

        Ok(ExecResult::ok(output))
    }
}

// --- Hexdump implementation ---

struct HexdumpOptions {
    canonical: bool,
    length: Option<usize>,
    offset: usize,
}

fn parse_hexdump_args(
    args: &[String],
) -> std::result::Result<(HexdumpOptions, Vec<String>), String> {
    let mut opts = HexdumpOptions {
        canonical: false,
        length: None,
        offset: 0,
    };
    let mut files = Vec::new();
    let mut i = 0;

    while i < args.len() {
        let arg = &args[i];
        if arg == "-C" {
            opts.canonical = true;
        } else if arg == "-n" {
            i += 1;
            if i < args.len() {
                opts.length = Some(
                    args[i]
                        .parse()
                        .map_err(|_| format!("hexdump: invalid length: '{}'", args[i]))?,
                );
            }
        } else if arg == "-s" {
            i += 1;
            if i < args.len() {
                opts.offset = args[i]
                    .parse()
                    .map_err(|_| format!("hexdump: invalid offset: '{}'", args[i]))?;
            }
        } else if !arg.starts_with('-') || arg == "-" {
            files.push(arg.clone());
        }
        i += 1;
    }

    Ok((opts, files))
}

fn hexdump_dump(data: &[u8], opts: &HexdumpOptions) -> String {
    let mut output = String::new();

    let data = if opts.offset < data.len() {
        &data[opts.offset..]
    } else {
        &[]
    };

    let data = match opts.length {
        Some(n) => &data[..data.len().min(n)],
        None => data,
    };

    if opts.canonical {
        // -C mode: hex+ASCII like `hexdump -C`
        for (chunk_idx, chunk) in data.chunks(16).enumerate() {
            let offset = opts.offset + chunk_idx * 16;
            output.push_str(&format!("{:08x}  ", offset));

            // First 8 bytes
            for j in 0..8 {
                if j < chunk.len() {
                    output.push_str(&format!("{:02x} ", chunk[j]));
                } else {
                    output.push_str("   ");
                }
            }
            output.push(' ');

            // Next 8 bytes
            for j in 8..16 {
                if j < chunk.len() {
                    output.push_str(&format!("{:02x} ", chunk[j]));
                } else {
                    output.push_str("   ");
                }
            }

            // ASCII
            output.push_str(" |");
            for byte in chunk {
                if *byte >= 0x20 && *byte < 0x7f {
                    output.push(*byte as char);
                } else {
                    output.push('.');
                }
            }
            output.push_str("|\n");
        }

        // Final offset
        if !data.is_empty() {
            let final_offset = opts.offset + data.len();
            output.push_str(&format!("{:08x}\n", final_offset));
        }
    } else {
        // Default mode: 16-bit hex words
        for (chunk_idx, chunk) in data.chunks(16).enumerate() {
            let offset = opts.offset + chunk_idx * 16;
            output.push_str(&format!("{:07x}", offset));

            for pair in chunk.chunks(2) {
                if pair.len() == 2 {
                    // Little-endian 16-bit word
                    let word = (pair[1] as u16) << 8 | pair[0] as u16;
                    output.push_str(&format!(" {:04x}", word));
                } else {
                    output.push_str(&format!(" {:04x}", pair[0] as u16));
                }
            }
            output.push('\n');
        }

        // Final offset
        if !data.is_empty() {
            let final_offset = opts.offset + data.len();
            output.push_str(&format!("{:07x}\n", final_offset));
        }
    }

    output
}

#[async_trait]
impl Builtin for Hexdump {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let (opts, files) = match parse_hexdump_args(ctx.args) {
            Ok(v) => v,
            Err(e) => return Ok(ExecResult::err(format!("{}\n", e), 1)),
        };

        let data = collect_input(ctx.stdin, &files, ctx.cwd, &ctx.fs).await?;
        let output = hexdump_dump(&data, &opts);

        Ok(ExecResult::ok(output))
    }
}

// --- Shared helpers ---

async fn collect_input(
    stdin: Option<&str>,
    files: &[String],
    cwd: &std::path::Path,
    fs: &std::sync::Arc<dyn crate::fs::FileSystem>,
) -> Result<Vec<u8>> {
    let mut data = Vec::new();

    if files.is_empty() {
        if let Some(stdin) = stdin {
            data.extend_from_slice(stdin.as_bytes());
        }
    } else {
        for file in files {
            if file == "-" {
                if let Some(stdin) = stdin {
                    data.extend_from_slice(stdin.as_bytes());
                }
            } else {
                let path = if file.starts_with('/') {
                    std::path::PathBuf::from(file)
                } else {
                    cwd.join(file)
                };

                let content = fs
                    .read_file(&path)
                    .await
                    .map_err(|e| crate::error::Error::Internal(format!("{}: {}", file, e)))?;
                data.extend_from_slice(&content);
            }
        }
    }

    Ok(data)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Arc;

    use crate::fs::{FileSystem, InMemoryFs};

    async fn run_od(args: &[&str], stdin: Option<&str>) -> ExecResult {
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

        Od.execute(ctx).await.unwrap()
    }

    async fn run_xxd(args: &[&str], stdin: Option<&str>) -> ExecResult {
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

        Xxd.execute(ctx).await.unwrap()
    }

    async fn run_hexdump(args: &[&str], stdin: Option<&str>) -> ExecResult {
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

        Hexdump.execute(ctx).await.unwrap()
    }

    async fn run_od_with_fs(args: &[&str], files: &[(&str, &[u8])]) -> ExecResult {
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

        Od.execute(ctx).await.unwrap()
    }

    // --- Od tests ---

    #[tokio::test]
    async fn test_od_basic() {
        let result = run_od(&[], Some("AB")).await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("101")); // 'A' = octal 101
        assert!(result.stdout.contains("102")); // 'B' = octal 102
    }

    #[tokio::test]
    async fn test_od_hex() {
        let result = run_od(&["-t", "x"], Some("AB")).await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("41")); // 'A' = 0x41
        assert!(result.stdout.contains("42")); // 'B' = 0x42
    }

    #[tokio::test]
    async fn test_od_decimal() {
        let result = run_od(&["-t", "d"], Some("A")).await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains(" 65")); // 'A' = 65
    }

    #[tokio::test]
    async fn test_od_char() {
        let result = run_od(&["-t", "c"], Some("A\n")).await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("A"));
        assert!(result.stdout.contains("\\n"));
    }

    #[tokio::test]
    async fn test_od_hex_addr() {
        let result = run_od(&["-A", "x"], Some("test")).await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.starts_with("0000000"));
    }

    #[tokio::test]
    async fn test_od_no_addr() {
        let result = run_od(&["-A", "n", "-t", "x"], Some("AB")).await;
        assert_eq!(result.exit_code, 0);
        assert!(!result.stdout.starts_with("0"));
        assert!(result.stdout.contains("41"));
    }

    #[tokio::test]
    async fn test_od_count() {
        let result = run_od(&["-N", "2", "-t", "x"], Some("ABCD")).await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("41"));
        assert!(result.stdout.contains("42"));
        assert!(!result.stdout.contains("43"));
    }

    #[tokio::test]
    async fn test_od_skip() {
        let result = run_od(&["-j", "2", "-t", "x"], Some("ABCD")).await;
        assert_eq!(result.exit_code, 0);
        assert!(!result.stdout.contains(" 41"));
        assert!(result.stdout.contains("43"));
    }

    #[tokio::test]
    async fn test_od_empty_input() {
        let result = run_od(&[], Some("")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "");
    }

    #[tokio::test]
    async fn test_od_from_file() {
        let result =
            run_od_with_fs(&["-t", "x", "/test.bin"], &[("/test.bin", &[0x41, 0x42])]).await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("41"));
    }

    // --- Xxd tests ---

    #[tokio::test]
    async fn test_xxd_basic() {
        let result = run_xxd(&[], Some("Hello")).await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("00000000:"));
        assert!(result.stdout.contains("4865 6c6c 6f"));
        assert!(result.stdout.contains("Hello"));
    }

    #[tokio::test]
    async fn test_xxd_plain() {
        let result = run_xxd(&["-p"], Some("Hi")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "4869\n");
    }

    #[tokio::test]
    async fn test_xxd_length() {
        let result = run_xxd(&["-l", "3", "-p"], Some("Hello World")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "48656c\n");
    }

    #[tokio::test]
    async fn test_xxd_offset() {
        let result = run_xxd(&["-s", "2", "-p"], Some("Hello")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "6c6c6f\n");
    }

    #[tokio::test]
    async fn test_xxd_cols() {
        let result = run_xxd(&["-c", "4"], Some("ABCDEFGH")).await;
        assert_eq!(result.exit_code, 0);
        let lines: Vec<&str> = result.stdout.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("00000000:"));
        assert!(lines[1].contains("00000004:"));
    }

    #[tokio::test]
    async fn test_xxd_empty() {
        let result = run_xxd(&[], Some("")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "");
    }

    #[tokio::test]
    async fn test_xxd_group() {
        let result = run_xxd(&["-g", "1"], Some("AB")).await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("41 42"));
    }

    #[tokio::test]
    async fn test_xxd_non_printable() {
        let result = run_xxd(&["-p"], Some("\x00\x01\x02")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "000102\n");
    }

    // --- Hexdump tests ---

    #[tokio::test]
    async fn test_hexdump_default() {
        let result = run_hexdump(&[], Some("AB")).await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("4241")); // Little-endian
    }

    #[tokio::test]
    async fn test_hexdump_canonical() {
        let result = run_hexdump(&["-C"], Some("Hello")).await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("48 65 6c 6c 6f"));
        assert!(result.stdout.contains("|Hello|"));
    }

    #[tokio::test]
    async fn test_hexdump_canonical_non_printable() {
        let result = run_hexdump(&["-C"], Some("\x00\x01\x02")).await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("00 01 02"));
        assert!(result.stdout.contains("|...|"));
    }

    #[tokio::test]
    async fn test_hexdump_length() {
        let result = run_hexdump(&["-C", "-n", "3"], Some("Hello World")).await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("48 65 6c"));
        assert!(!result.stdout.contains("6f")); // 'o' should not be there
    }

    #[tokio::test]
    async fn test_hexdump_offset() {
        let result = run_hexdump(&["-C", "-s", "2"], Some("Hello")).await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("6c 6c 6f"));
    }

    #[tokio::test]
    async fn test_hexdump_empty() {
        let result = run_hexdump(&[], Some("")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "");
    }

    #[tokio::test]
    async fn test_hexdump_canonical_final_offset() {
        let result = run_hexdump(&["-C"], Some("AB")).await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("00000002")); // final offset
    }
}
