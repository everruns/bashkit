//! fmt builtin - fill and join paragraphs
//!
//! Important decision: the line breaker and paragraph splitter are vendored
//! from uutils/coreutils `fmt` (MIT, `linebreak.rs`, `parasplit.rs`), a
//! Knuth-Plass optimal-fit filler, not a greedy one. Bashkit is MIT, so GNU
//! `fmt.c` (GPL) must not be ported; uutils already targets GNU-compatible
//! output. Only option parsing and I/O are bashkit's. Width and goal follow
//! GNU's documented rules: goal defaults to 93% of the width, `-g` alone
//! sets the width to goal + 10, width is capped at 2500.

mod linebreak;
mod parasplit;

use async_trait::async_trait;

use super::{Builtin, BuiltinHelper, Context, resolve_path};
use crate::error::Result;
use crate::interpreter::ExecResult;

/// The fmt builtin.
///
/// Usage: fmt [-WIDTH] [OPTION]... [FILE]...
pub struct Fmt;

impl BuiltinHelper for Fmt {
    const NAME: &'static str = "fmt";
}

const USAGE: &str = "Usage: fmt [-WIDTH] [OPTION]... [FILE]...\nReformat each paragraph in the FILE(s), writing to standard output.\nThe option -WIDTH is an abbreviated form of --width=DIGITS.\n\nWith no FILE, or when FILE is -, read standard input.\n\n  -c, --crown-margin        preserve indentation of first two lines\n  -p, --prefix=STRING       reformat only lines beginning with STRING,\n                              reattaching the prefix to reformatted lines\n  -s, --split-only          split long lines, but do not refill\n  -t, --tagged-paragraph    indentation of first line different from second\n  -u, --uniform-spacing     one space between words, two after sentences\n  -w, --width=WIDTH         maximum line width (default of 75 columns)\n  -g, --goal=WIDTH          goal width (default of 93% of width)\n      --help                display this help and exit\n      --version             output version information and exit\n";

const DEFAULT_WIDTH: usize = 75;
const MAX_WIDTH: usize = 2500;

/// Input handed to the vendored paragraph splitter.
type FileOrStdReader = std::io::Cursor<Vec<u8>>;

/// Options read by the vendored `linebreak`/`parasplit` code.
struct FmtOptions {
    crown: bool,
    tagged: bool,
    mail: bool,
    split_only: bool,
    prefix: Option<String>,
    xprefix: bool,
    anti_prefix: Option<String>,
    xanti_prefix: bool,
    uniform: bool,
    quick: bool,
    width: usize,
    goal: usize,
    tabwidth: usize,
}

fn err(msg: String) -> ExecResult {
    ExecResult::err(format!("fmt: {msg}\n"), 1)
}

#[allow(clippy::result_large_err)]
/// GNU wording: bad digits vs. a number past `max`.
fn parse_width(value: &str, max: usize) -> std::result::Result<usize, ExecResult> {
    let digits = !value.is_empty() && value.bytes().all(|b| b.is_ascii_digit());
    match value.parse::<usize>() {
        Ok(w) if w <= max => Ok(w),
        Err(_) if digits => Err(err(format!(
            "invalid width: '{value}': Value too large for defined data type"
        ))),
        _ if digits => Err(err(format!(
            "invalid width: '{value}': Numerical result out of range"
        ))),
        _ => Err(err(format!("invalid width: '{value}'"))),
    }
}

#[allow(clippy::result_large_err)]
/// Parse arguments into options and file operands.
fn parse_args(args: &[String]) -> std::result::Result<(FmtOptions, Vec<&str>), ExecResult> {
    let mut opts = FmtOptions {
        crown: false,
        tagged: false,
        mail: false,
        split_only: false,
        prefix: None,
        xprefix: false,
        anti_prefix: None,
        xanti_prefix: false,
        uniform: false,
        quick: false,
        width: DEFAULT_WIDTH,
        goal: 0,
        tabwidth: 8,
    };
    let mut width: Option<usize> = None;
    let mut goal: Option<String> = None;
    let mut files = Vec::new();
    let mut i = 0;
    // Obsolete `fmt -WIDTH`, first argument only.
    if let Some(digits) = args.first().and_then(|a| a.strip_prefix('-'))
        && digits.starts_with(|c: char| c.is_ascii_digit())
    {
        width = Some(parse_width(digits, MAX_WIDTH)?);
        i = 1;
    }
    let mut only_files = false;
    while i < args.len() {
        let a = args[i].as_str();
        i += 1;
        if only_files || a == "-" || !a.starts_with('-') {
            files.push(a);
            continue;
        }
        if a == "--" {
            only_files = true;
            continue;
        }
        if let Some(long) = a.strip_prefix("--") {
            let (name, inline) = match long.split_once('=') {
                Some((n, v)) => (n, Some(v.to_string())),
                None => (long, None),
            };
            let takes_value = matches!(
                name,
                "width" | "goal" | "prefix" | "skip-prefix" | "tab-width"
            );
            let value = if takes_value {
                match inline {
                    Some(v) => Some(v),
                    None => {
                        let v = args.get(i).cloned().ok_or_else(|| {
                            err(format!("option '--{name}' requires an argument"))
                        })?;
                        i += 1;
                        Some(v)
                    }
                }
            } else {
                None
            };
            let value = value.unwrap_or_default();
            match name {
                "crown-margin" => opts.crown = true,
                "tagged-paragraph" => opts.tagged = true,
                "preserve-headers" => opts.mail = true,
                "split-only" => opts.split_only = true,
                "uniform-spacing" => opts.uniform = true,
                "exact-prefix" => opts.xprefix = true,
                "exact-skip-prefix" => opts.xanti_prefix = true,
                "quick" => opts.quick = true,
                "width" => width = Some(parse_width(&value, MAX_WIDTH)?),
                "goal" => goal = Some(value),
                "prefix" => opts.prefix = Some(value),
                "skip-prefix" => opts.anti_prefix = Some(value),
                "tab-width" => opts.tabwidth = parse_tab_width(&value)?,
                _ => return Err(super::invalid_option("fmt", a, 1)),
            }
            continue;
        }
        // Short option cluster; a value option takes the rest or the next arg.
        let bytes = a.as_bytes();
        let mut j = 1;
        while j < bytes.len() {
            let f = bytes[j];
            match f {
                b'c' => opts.crown = true,
                b't' => opts.tagged = true,
                b'm' => opts.mail = true,
                b's' => opts.split_only = true,
                b'u' => opts.uniform = true,
                b'x' => opts.xprefix = true,
                b'X' => opts.xanti_prefix = true,
                b'q' => opts.quick = true,
                b'w' | b'g' | b'p' | b'P' | b'T' => {
                    let value = if j + 1 < bytes.len() {
                        a[j + 1..].to_string()
                    } else {
                        let v = args.get(i).cloned().ok_or_else(|| {
                            err(format!("option requires an argument -- '{}'", f as char))
                        })?;
                        i += 1;
                        v
                    };
                    match f {
                        b'w' => width = Some(parse_width(&value, MAX_WIDTH)?),
                        b'g' => goal = Some(value),
                        b'p' => opts.prefix = Some(value),
                        b'P' => opts.anti_prefix = Some(value),
                        _ => opts.tabwidth = parse_tab_width(&value)?,
                    }
                    break;
                }
                _ => return Err(super::invalid_option("fmt", a, 1)),
            }
            j += 1;
        }
    }

    // Mode precedence as in uutils: -s wins over -c, -c over -t.
    if opts.crown {
        opts.tagged = false;
    }
    if opts.split_only {
        opts.crown = false;
        opts.tagged = false;
    }
    opts.width = width.unwrap_or(DEFAULT_WIDTH);
    opts.goal = match goal {
        Some(g) => {
            let g = parse_width(&g, opts.width)?;
            if width.is_none() {
                opts.width = g + 10;
            }
            g
        }
        None => opts.width * 187 / 200,
    };
    if files.is_empty() {
        files.push("-");
    }
    Ok((opts, files))
}

#[allow(clippy::result_large_err)]
fn parse_tab_width(value: &str) -> std::result::Result<usize, ExecResult> {
    value
        .parse::<usize>()
        .map(|t| t.max(1))
        .map_err(|_| err(format!("invalid tab width: '{value}'")))
}

/// Format one input into `out` with the vendored engine.
fn format_input(opts: &FmtOptions, input: Vec<u8>, out: &mut Vec<u8>) {
    let mut reader: FileOrStdReader = std::io::Cursor::new(input);
    for para in parasplit::ParagraphStream::new(opts, &mut reader) {
        match para {
            Err(line) => {
                out.extend_from_slice(&line);
                out.push(b'\n');
            }
            // Writes to a Vec never fail.
            Ok(para) => {
                let _ = linebreak::break_lines(&para, opts, out);
            }
        }
    }
}

#[async_trait]
impl Builtin for Fmt {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        if let Some(r) = Self::check_help(ctx.args, USAGE, Some("fmt (bashkit) 0.1")) {
            return Ok(r);
        }
        let (opts, files) = match parse_args(ctx.args) {
            Ok(v) => v,
            Err(e) => return Ok(e),
        };
        let mut out = Vec::new();
        let mut stderr = String::new();
        let mut exit_code = 0;
        for file in files {
            let input: Vec<u8> = if file == "-" {
                ctx.stdin_bytes().map(<[u8]>::to_vec).unwrap_or_default()
            } else {
                let path = resolve_path(ctx.cwd, file);
                match ctx.fs.read_file(&path).await {
                    Ok(bytes) => bytes,
                    Err(e) => {
                        stderr.push_str(&format!(
                            "fmt: cannot open '{file}' for reading: {}\n",
                            crate::error::io_error_reason(&e)
                        ));
                        exit_code = 1;
                        continue;
                    }
                }
            };
            ctx.consume_budget_work(1 + (input.len() / 1024) as u64)?;
            format_input(&opts, input, &mut out);
        }
        let mut result = ExecResult::with_code(crate::StreamData::from(out), exit_code);
        result.stderr = stderr.into();
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fmt(args: &[&str], input: &str) -> String {
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let Ok((opts, _)) = parse_args(&args) else {
            panic!("valid args");
        };
        let mut out = Vec::new();
        format_input(&opts, input.as_bytes().to_vec(), &mut out);
        String::from_utf8(out).unwrap()
    }

    #[test]
    fn joins_short_lines() {
        assert_eq!(fmt(&[], "a b\nc d\n"), "a b c d\n");
    }

    #[test]
    fn keeps_blank_line_paragraphs() {
        assert_eq!(fmt(&[], "a\nb\n\nc\n"), "a b\n\nc\n");
    }

    #[test]
    fn two_spaces_after_sentence_end_at_line_end() {
        assert_eq!(
            fmt(&[], "Hello world.\nNext line.\n"),
            "Hello world.  Next line.\n"
        );
    }

    #[test]
    fn long_word_stays_whole() {
        let word = "x".repeat(100);
        assert_eq!(
            fmt(&["-w", "20"], &format!("{word} y\n")),
            format!("{word}\ny\n")
        );
    }

    #[test]
    fn no_trailing_newline_input() {
        assert_eq!(fmt(&[], "a b"), "a b\n");
    }

    #[test]
    fn empty_input() {
        assert_eq!(fmt(&[], ""), "");
    }

    #[test]
    fn width_rules() {
        let args = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let (o, _) = parse_args(&args(&[])).ok().unwrap();
        assert_eq!((o.width, o.goal), (75, 70));
        let (o, _) = parse_args(&args(&["-g", "30"])).ok().unwrap();
        assert_eq!((o.width, o.goal), (40, 30));
        let (o, _) = parse_args(&args(&["-50"])).ok().unwrap();
        assert_eq!((o.width, o.goal), (50, 46));
        assert!(parse_args(&args(&["-w", "3000"])).is_err());
        assert!(parse_args(&args(&["-w", "20", "-g", "30"])).is_err());
        assert!(parse_args(&args(&["-w", "abc"])).is_err());
        assert!(parse_args(&args(&["-z"])).is_err());
    }
}
