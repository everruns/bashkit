//! Head and tail builtins - output first/last lines of input
//!
//! Decisions (#2447):
//! - Counts follow GNU coreutils: `-n -N` (head: all but the last N),
//!   `-n +N` (tail: start at N), multiplier suffixes (`1k`, `2MiB`, `3KB`,
//!   `b` = 512), and invalid counts fail with exit 1 instead of silently
//!   defaulting to 10.
//! - `head -c`/`tail -c` count bytes; `tail -c` supports the same signs.
//! - Selection works on raw bytes, so a missing final newline and non-UTF-8
//!   input pass through unchanged.
//! - A missing file is reported and skipped; the command then exits 1.

use async_trait::async_trait;

use super::{Builtin, Context};
use crate::error::Result;
use crate::fs::vfs_join;
use crate::interpreter::ExecResult;

/// Default number of lines to output
const DEFAULT_LINES: usize = 10;

/// What a count selects from the input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Selection {
    /// The first N units (`head -n N`).
    First(usize),
    /// Everything except the last N units (`head -n -N`).
    AllButLast(usize),
    /// The last N units (`tail -n N`).
    Last(usize),
    /// Everything from unit N on, 1-based (`tail -n +N`).
    From(usize),
}

/// Unit a selection counts in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Unit {
    Lines,
    Bytes,
}

/// Leading sign of a count value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Sign {
    None,
    Plus,
    Minus,
}

/// Parse a GNU count such as `10`, `-3`, `+2`, `1k`, `2MiB` or `5KB`.
fn parse_count(val: &str) -> Option<(Sign, usize)> {
    let (sign, rest) = match val.as_bytes().first() {
        Some(b'+') => (Sign::Plus, &val[1..]),
        Some(b'-') => (Sign::Minus, &val[1..]),
        _ => (Sign::None, val),
    };
    let digits_end = rest
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(rest.len());
    if digits_end == 0 {
        return None;
    }
    let multiplier = count_multiplier(&rest[digits_end..])?;
    // Values beyond usize mean "everything", as in GNU.
    let n = rest[..digits_end]
        .parse::<usize>()
        .unwrap_or(usize::MAX)
        .saturating_mul(multiplier);
    Some((sign, n))
}

/// Multiplier for a GNU size suffix (`K` = 1024, `KB` = 1000, `KiB` = 1024).
fn count_multiplier(suffix: &str) -> Option<usize> {
    if suffix.is_empty() {
        return Some(1);
    }
    if suffix == "b" {
        return Some(512);
    }
    let mut chars = suffix.chars();
    let letter = chars.next()?;
    let power = match letter {
        'k' | 'K' => 1,
        'M' => 2,
        'G' => 3,
        'T' => 4,
        'P' => 5,
        'E' => 6,
        _ => return None,
    };
    let base: usize = match chars.as_str() {
        "" | "iB" => 1024,
        "B" => 1000,
        _ => return None,
    };
    Some(base.saturating_pow(power))
}

/// Parse a `-n`/`-c` value into a selection for `cmd`.
#[allow(clippy::result_large_err)]
fn parse_selection(cmd: &str, val: &str, unit: Unit) -> std::result::Result<Selection, ExecResult> {
    let what = match unit {
        Unit::Lines => "lines",
        Unit::Bytes => "bytes",
    };
    let Some((sign, n)) = parse_count(val) else {
        return Err(ExecResult::err(
            format!("{cmd}: invalid number of {what}: '{val}'\n"),
            1,
        ));
    };
    Ok(match (cmd, sign) {
        ("head", Sign::Minus) => Selection::AllButLast(n),
        ("head", _) => Selection::First(n),
        (_, Sign::Plus) => Selection::From(n),
        _ => Selection::Last(n),
    })
}

/// Parsed head/tail command line.
struct Options {
    selection: Selection,
    unit: Unit,
    files: Vec<String>,
}

/// Parse head/tail arguments (`-n`, `-c`, obsolete `-NUM`, files).
#[allow(clippy::result_large_err)]
fn parse_args(cmd: &str, args: &[String]) -> std::result::Result<Options, ExecResult> {
    let default = if cmd == "head" {
        Selection::First(DEFAULT_LINES)
    } else {
        Selection::Last(DEFAULT_LINES)
    };
    let mut opts = Options {
        selection: default,
        unit: Unit::Lines,
        files: Vec::new(),
    };
    let mut p = super::arg_parser::ArgParser::new(args);

    while !p.is_done() {
        if let Some(val) = p.flag_value_opt("-n") {
            opts.selection = parse_selection(cmd, val, Unit::Lines)?;
            opts.unit = Unit::Lines;
        } else if let Some(val) = p.flag_value_opt("-c") {
            opts.selection = parse_selection(cmd, val, Unit::Bytes)?;
            opts.unit = Unit::Bytes;
        } else if let Some(arg) = p.current().filter(|a| a.starts_with('-')) {
            if let Some(num_str) = arg.strip_prefix('-')
                && let Ok(n) = num_str.parse::<usize>()
            {
                // Obsolete `-NUM` line-count form.
                opts.selection = if cmd == "head" {
                    Selection::First(n)
                } else {
                    Selection::Last(n)
                };
                opts.unit = Unit::Lines;
                p.advance();
            } else if arg == "-" {
                // "-" is the stdin operand.
                opts.files.push(arg.to_string());
                p.advance();
            } else if arg == "--" {
                // "--" ends options.
                p.advance();
            } else {
                // Unknown option-shaped token (e.g. -Q) → reject like GNU.
                return Err(super::invalid_option(cmd, arg, 1));
            }
        } else if let Some(arg) = p.positional() {
            opts.files.push(arg.to_string());
        }
    }

    Ok(opts)
}

/// Apply `selection` to `data`, counting `unit`s. Bytes pass through as-is.
fn select(data: &[u8], selection: Selection, unit: Unit) -> &[u8] {
    // Byte offset where each unit starts, plus the end of the data.
    let bounds: Vec<usize> = match unit {
        Unit::Bytes => return select_range(data, data.len(), |i| i, selection),
        Unit::Lines => std::iter::once(0)
            .chain(
                data.iter()
                    .enumerate()
                    .filter(|&(i, &b)| b == b'\n' && i + 1 < data.len())
                    .map(|(i, _)| i + 1),
            )
            .collect(),
    };
    let units = if data.is_empty() { 0 } else { bounds.len() };
    select_range(
        data,
        units,
        |i| bounds.get(i).copied().unwrap_or(data.len()),
        selection,
    )
}

/// Slice `data` to units `[start, end)` of `total`, mapping unit index to a
/// byte offset with `offset` (index `total` maps to the end of the data).
fn select_range(
    data: &[u8],
    total: usize,
    offset: impl Fn(usize) -> usize,
    selection: Selection,
) -> &[u8] {
    let (start, end) = match selection {
        Selection::First(n) => (0, n.min(total)),
        Selection::AllButLast(n) => (0, total.saturating_sub(n)),
        Selection::Last(n) => (total.saturating_sub(n), total),
        Selection::From(n) => (n.saturating_sub(1).min(total), total),
    };
    let byte = |i: usize| if i >= total { data.len() } else { offset(i) };
    &data[byte(start)..byte(end)]
}

/// Shared head/tail driver.
async fn run(cmd: &str, ctx: Context<'_>) -> Result<ExecResult> {
    let opts = match parse_args(cmd, ctx.args) {
        Ok(v) => v,
        Err(e) => return Ok(e),
    };

    if opts.files.is_empty() {
        let data = ctx.stdin.map(|s| s.as_bytes().to_vec()).unwrap_or_default();
        return Ok(ExecResult::ok_bytes(
            select(&data, opts.selection, opts.unit).to_vec(),
        ));
    }

    let mut out = Vec::new();
    let mut stderr = String::new();
    let multiple_files = opts.files.len() > 1;
    let mut printed_any = false;
    for file in &opts.files {
        let data = if file == "-" {
            ctx.stdin
                .as_ref()
                .map(|s| s.as_bytes().to_vec())
                .unwrap_or_default()
        } else {
            let path = if file.starts_with('/') {
                std::path::PathBuf::from(file)
            } else {
                vfs_join(ctx.cwd, file)
            };
            match ctx.fs.read_file(&path).await {
                Ok(content) => content,
                Err(e) => {
                    let reason = crate::error::io_error_reason(&e);
                    stderr.push_str(&format!("{cmd}: {file}: {reason}\n"));
                    continue;
                }
            }
        };
        if multiple_files {
            if printed_any {
                out.push(b'\n');
            }
            let name = if file == "-" { "standard input" } else { file };
            out.extend_from_slice(format!("==> {name} <==\n").as_bytes());
        }
        printed_any = true;
        out.extend_from_slice(select(&data, opts.selection, opts.unit));
    }

    let mut result = ExecResult::ok_bytes(out);
    if !stderr.is_empty() {
        result.stderr = stderr.into();
        result.exit_code = 1;
    }
    Ok(result)
}

/// The head builtin - output the first N lines or bytes of input.
///
/// Usage: head [-n [-]NUM | -c [-]NUM] [FILE...]
///
/// Options:
///   -n NUM   Output the first NUM lines (default: 10); -NUM: all but the last NUM
///   -c NUM   Output the first NUM bytes; -NUM: all but the last NUM
///   -NUM     Shorthand for -n NUM
pub struct Head;

#[async_trait]
impl Builtin for Head {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        if let Some(r) = super::check_help_version(
            ctx.args,
            "Usage: head [OPTION]... [FILE]...\nPrint the first 10 lines of each FILE to standard output.\n\n  -n [-]NUM\toutput the first NUM lines; with '-', all but the last NUM lines\n  -c [-]NUM\toutput the first NUM bytes; with '-', all but the last NUM bytes\n  -NUM\t\tshorthand for -n NUM\n  --help\tdisplay this help and exit\n  --version\toutput version information and exit\n\nNUM may have a multiplier suffix: b 512, kB 1000, K 1024, MB 1000*1000, M 1024*1024, and so on for G, T, P, E.\n",
            Some("head (bashkit) 0.1"),
        ) {
            return Ok(r);
        }
        run("head", ctx).await
    }
}

/// The tail builtin - output the last N lines or bytes of input.
///
/// Usage: tail [-n [+]NUM | -c [+]NUM] [FILE...]
///
/// Options:
///   -n NUM   Output the last NUM lines (default: 10)
///   -n +NUM  Output starting from line NUM (1-indexed)
///   -c NUM   Output the last NUM bytes; +NUM: starting from byte NUM
///   -NUM     Shorthand for -n NUM
pub struct Tail;

#[async_trait]
impl Builtin for Tail {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        if let Some(r) = super::check_help_version(
            ctx.args,
            "Usage: tail [OPTION]... [FILE]...\nPrint the last 10 lines of each FILE to standard output.\n\n  -n [+]NUM\toutput the last NUM lines; with '+', start at line NUM\n  -c [+]NUM\toutput the last NUM bytes; with '+', start at byte NUM\n  -NUM\t\tshorthand for -n NUM\n  --help\tdisplay this help and exit\n  --version\toutput version information and exit\n\nNUM may have a multiplier suffix: b 512, kB 1000, K 1024, MB 1000*1000, M 1024*1024, and so on for G, T, P, E.\n",
            Some("tail (bashkit) 0.1"),
        ) {
            return Ok(r);
        }
        run("tail", ctx).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Arc;

    use crate::fs::InMemoryFs;

    async fn run_head(args: &[&str], stdin: Option<&str>) -> ExecResult {
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
            stdin: crate::builtins::test_stream_opt(stdin),
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
        };

        Head.execute(ctx).await.unwrap()
    }

    async fn run_tail(args: &[&str], stdin: Option<&str>) -> ExecResult {
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
            stdin: crate::builtins::test_stream_opt(stdin),
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
        };

        Tail.execute(ctx).await.unwrap()
    }

    #[tokio::test]
    async fn test_head_default() {
        let input = "1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n11\n12\n";
        let result = run_head(&[], Some(input)).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n");
    }

    #[tokio::test]
    async fn test_head_n_flag() {
        let input = "a\nb\nc\nd\ne\n";
        let result = run_head(&["-n", "3"], Some(input)).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "a\nb\nc\n");
    }

    #[tokio::test]
    async fn test_head_shorthand() {
        let input = "a\nb\nc\nd\ne\n";
        let result = run_head(&["-2"], Some(input)).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "a\nb\n");
    }

    #[tokio::test]
    async fn test_head_invalid_option() {
        let result = run_head(&["-Q"], Some("a\nb\n")).await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("invalid option -- 'Q'"));
    }

    #[tokio::test]
    async fn test_head_c_multibyte_stdin_respects_byte_limit() {
        let result = run_head(&["-c", "1"], Some("éX")).await;
        assert_eq!(result.exit_code, 0);
        assert!(
            result.stdout.len() <= 1,
            "head -c 1 must not emit more than 1 byte for UTF-8 stdin"
        );

        let result = run_head(&["-c", "2"], Some("éX")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "é");
        assert_eq!(result.stdout.len(), 2);
    }

    #[tokio::test]
    async fn test_head_n_negative_all_but_last() {
        let result = run_head(&["-n", "-1"], Some("1\n2\n3\n")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "1\n2\n");

        let result = run_head(&["-n-2"], Some("1\n2\n3\n")).await;
        assert_eq!(result.stdout, "1\n");

        let result = run_head(&["-n", "-9"], Some("1\n2\n3\n")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "");
    }

    #[tokio::test]
    async fn test_head_c_negative_all_but_last() {
        let result = run_head(&["-c", "-2"], Some("abcdef")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "abcd");

        let result = run_head(&["-c", "-10"], Some("abc")).await;
        assert_eq!(result.stdout, "");
    }

    #[tokio::test]
    async fn test_head_invalid_count() {
        let result = run_head(&["-n", "abc"], Some("a\n")).await;
        assert_eq!(result.exit_code, 1);
        assert_eq!(result.stdout, "");
        assert!(result.stderr.contains("invalid number of lines: 'abc'"));

        let result = run_head(&["-c", "-x"], Some("a\n")).await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("invalid number of bytes: '-x'"));
    }

    #[test]
    fn test_parse_count_suffixes_and_signs() {
        assert_eq!(parse_count("10"), Some((Sign::None, 10)));
        assert_eq!(parse_count("-3"), Some((Sign::Minus, 3)));
        assert_eq!(parse_count("+2"), Some((Sign::Plus, 2)));
        assert_eq!(parse_count("1k"), Some((Sign::None, 1024)));
        assert_eq!(parse_count("2KB"), Some((Sign::None, 2000)));
        assert_eq!(parse_count("1MiB"), Some((Sign::None, 1 << 20)));
        assert_eq!(parse_count("3b"), Some((Sign::None, 1536)));
        assert_eq!(parse_count("1E"), Some((Sign::None, 1 << 60)));
        assert_eq!(
            parse_count("99999999999999999999999"),
            Some((Sign::None, usize::MAX))
        );
        for bad in ["", "-", "+", "abc", "3x", "1KiBB", "k", "1 k", "--1"] {
            assert_eq!(parse_count(bad), None, "{bad} must be rejected");
        }
    }

    #[tokio::test]
    async fn test_tail_c_and_signs() {
        let result = run_tail(&["-c", "2"], Some("abcdef")).await;
        assert_eq!(result.stdout, "ef");
        let result = run_tail(&["-c", "+3"], Some("abcdef")).await;
        assert_eq!(result.stdout, "cdef");
        let result = run_tail(&["-n", "-2"], Some("1\n2\n3\n")).await;
        assert_eq!(result.stdout, "2\n3\n");
        let result = run_tail(&["-n", "x"], Some("1\n")).await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("tail: invalid number of lines: 'x'"));
    }

    #[tokio::test]
    async fn test_head_tail_preserve_missing_final_newline() {
        let result = run_head(&["-n", "5"], Some("a\nb")).await;
        assert_eq!(result.stdout, "a\nb");
        let result = run_tail(&["-n", "1"], Some("a\nb")).await;
        assert_eq!(result.stdout, "b");
    }

    #[tokio::test]
    async fn test_tail_default() {
        let input = "1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n11\n12\n";
        let result = run_tail(&[], Some(input)).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "3\n4\n5\n6\n7\n8\n9\n10\n11\n12\n");
    }

    #[tokio::test]
    async fn test_tail_n_flag() {
        let input = "a\nb\nc\nd\ne\n";
        let result = run_tail(&["-n", "3"], Some(input)).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "c\nd\ne\n");
    }

    #[tokio::test]
    async fn test_tail_shorthand() {
        let input = "a\nb\nc\nd\ne\n";
        let result = run_tail(&["-2"], Some(input)).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "d\ne\n");
    }

    #[tokio::test]
    async fn test_tail_invalid_option() {
        let result = run_tail(&["-Q"], Some("a\nb\n")).await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("invalid option -- 'Q'"));
    }

    #[tokio::test]
    async fn test_head_empty_input() {
        let result = run_head(&[], Some("")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "");
    }

    #[tokio::test]
    async fn test_tail_empty_input() {
        let result = run_tail(&[], Some("")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "");
    }

    #[tokio::test]
    async fn test_head_fewer_lines_than_requested() {
        let input = "a\nb\n";
        let result = run_head(&["-n", "10"], Some(input)).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "a\nb\n");
    }

    #[tokio::test]
    async fn test_tail_fewer_lines_than_requested() {
        let input = "a\nb\n";
        let result = run_tail(&["-n", "10"], Some(input)).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "a\nb\n");
    }

    #[tokio::test]
    async fn test_tail_plus_n_from_start() {
        let input = "header\nline1\nline2\nline3\n";
        let result = run_tail(&["-n", "+2"], Some(input)).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "line1\nline2\nline3\n");
    }

    #[tokio::test]
    async fn test_tail_plus_1_all_lines() {
        let input = "a\nb\nc\n";
        let result = run_tail(&["-n", "+1"], Some(input)).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "a\nb\nc\n");
    }
}
