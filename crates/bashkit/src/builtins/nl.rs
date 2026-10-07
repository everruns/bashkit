//! nl builtin command - number lines of files
//!
//! Decision: follows GNU nl's logical-page model. A line that is exactly the
//! section delimiter repeated 3/2/1 times starts a header/body/footer section,
//! prints as an empty line, and (unless `-p`) resets the line number. The `-l`
//! blank-line counter is not reset by section changes, as in GNU.

use async_trait::async_trait;
use regex::Regex;

use super::arg_parser::{OptArg, gnu_getopt};
use super::{Builtin, Context, MAX_FORMAT_WIDTH, read_text_file};
use crate::error::Result;
use crate::fs::vfs_join;
use crate::interpreter::ExecResult;

/// The nl builtin - number lines of files.
///
/// Usage: nl [OPTION]... [FILE...]
///
/// Options:
///   -b/-h/-f STYLE  Body/header/footer numbering: a, t, n, or pBRE
///   -d CC           Section delimiter (default `\:`)
///   -i INCR         Line number increment (default: 1)
///   -l NUMBER       Group of NUMBER empty lines counted as one
///   -n FORMAT       Number format: ln, rn (default), rz
///   -p              Do not reset line numbers at sections
///   -s SEP          Separator string between number and line (default: TAB)
///   -v START        Starting line number (default: 1)
///   -w WIDTH        Number width (default: 6)
pub struct Nl;

enum Style {
    All,
    NonEmpty,
    None,
    Regex(Regex),
}

#[derive(Clone, Copy)]
enum NumberFormat {
    LeftJustified,
    RightJustified,
    RightZero,
}

#[derive(Clone, Copy, PartialEq)]
enum Section {
    Header,
    Body,
    Footer,
}

struct NlOptions {
    body: Style,
    header: Style,
    footer: Style,
    delimiter: String,
    format: NumberFormat,
    separator: String,
    increment: i64,
    start: i64,
    width: usize,
    join_blank: u64,
    renumber: bool,
}

impl Default for NlOptions {
    fn default() -> Self {
        Self {
            body: Style::NonEmpty,
            header: Style::None,
            footer: Style::None,
            delimiter: "\\:".to_string(),
            format: NumberFormat::RightJustified,
            separator: "\t".to_string(),
            increment: 1,
            start: 1,
            width: 6,
            join_blank: 1,
            renumber: true,
        }
    }
}

fn parse_style(val: &str, what: &str) -> std::result::Result<Style, String> {
    match val {
        "a" => Ok(Style::All),
        "t" => Ok(Style::NonEmpty),
        "n" => Ok(Style::None),
        _ => {
            if let Some(pat) = val.strip_prefix('p') {
                let translated = super::sed::translate_posix_regex(pat, false);
                return super::search_common::build_regex(&translated)
                    .map(Style::Regex)
                    .map_err(|_| format!("nl: invalid regular expression: '{pat}'"));
            }
            Err(format!("nl: invalid {what} numbering style: '{val}'"))
        }
    }
}

fn parse_int(val: &str, what: &str) -> std::result::Result<i64, String> {
    val.trim_start()
        .parse()
        .map_err(|_| format!("nl: invalid {what}: '{val}'"))
}

#[allow(clippy::result_large_err)]
fn parse_nl_args(args: &[String]) -> std::result::Result<(NlOptions, Vec<String>), ExecResult> {
    let (parsed, files) = gnu_getopt(
        "nl",
        args,
        "b:d:f:h:i:l:n:ps:v:w:",
        &[
            ("body-numbering", OptArg::Required, 'b'),
            ("section-delimiter", OptArg::Required, 'd'),
            ("footer-numbering", OptArg::Required, 'f'),
            ("header-numbering", OptArg::Required, 'h'),
            ("line-increment", OptArg::Required, 'i'),
            ("join-blank-lines", OptArg::Required, 'l'),
            ("number-format", OptArg::Required, 'n'),
            ("no-renumber", OptArg::No, 'p'),
            ("number-separator", OptArg::Required, 's'),
            ("starting-line-number", OptArg::Required, 'v'),
            ("number-width", OptArg::Required, 'w'),
        ],
        true,
        1,
    )?;
    let fail = |msg: String| ExecResult::err(format!("{msg}\n"), 1);
    let mut opts = NlOptions::default();
    for o in parsed {
        let val = o.value.unwrap_or_default();
        match o.key {
            'b' => opts.body = parse_style(&val, "body").map_err(fail)?,
            'h' => opts.header = parse_style(&val, "header").map_err(fail)?,
            'f' => opts.footer = parse_style(&val, "footer").map_err(fail)?,
            'd' => {
                // A single character keeps the default second character `:`.
                let mut chars = val.chars();
                opts.delimiter = match (chars.next(), chars.next()) {
                    (Some(c), None) => format!("{c}:"),
                    _ => val,
                };
            }
            'i' => opts.increment = parse_int(&val, "line number increment").map_err(fail)?,
            'l' => {
                opts.join_blank = match parse_int(&val, "line number of blank lines") {
                    Ok(n) if n > 0 => n as u64,
                    _ => {
                        return Err(fail(format!(
                            "nl: invalid line number of blank lines: '{val}'"
                        )));
                    }
                }
            }
            'n' => {
                opts.format = match val.as_str() {
                    "ln" => NumberFormat::LeftJustified,
                    "rn" => NumberFormat::RightJustified,
                    "rz" => NumberFormat::RightZero,
                    other => {
                        return Err(fail(format!(
                            "nl: invalid line numbering format: '{other}'"
                        )));
                    }
                }
            }
            'p' => opts.renumber = false,
            's' => opts.separator = val,
            'v' => opts.start = parse_int(&val, "starting line number").map_err(fail)?,
            _ => {
                let width = match parse_int(&val, "line number field width") {
                    Ok(n) if n > 0 => n as usize,
                    _ => {
                        return Err(fail(format!(
                            "nl: invalid line number field width: '{val}'"
                        )));
                    }
                };
                if width > MAX_FORMAT_WIDTH {
                    return Err(fail(format!(
                        "nl: line number field width {width} exceeds maximum ({MAX_FORMAT_WIDTH})"
                    )));
                }
                opts.width = width;
            }
        }
    }
    Ok((opts, files))
}

fn format_number(num: i64, format: NumberFormat, width: usize) -> String {
    match format {
        NumberFormat::LeftJustified => format!("{:<width$}", num, width = width),
        NumberFormat::RightJustified => format!("{:>width$}", num, width = width),
        NumberFormat::RightZero => {
            if num < 0 {
                format!("-{:0>w$}", num.unsigned_abs(), w = width.saturating_sub(1))
            } else {
                format!("{:0>width$}", num, width = width)
            }
        }
    }
}

/// Numbering state carried across all input files.
struct NlState {
    section: Section,
    line_no: i64,
    blank_lines: u64,
}

fn number_lines(text: &str, opts: &NlOptions, st: &mut NlState, out: &mut String) {
    let header = opts.delimiter.repeat(3);
    let body = opts.delimiter.repeat(2);
    let footer = &opts.delimiter;
    let no_number = " ".repeat(opts.width + opts.separator.len());

    for line in text.lines() {
        if !opts.delimiter.is_empty() {
            let next = if line == header {
                Some(Section::Header)
            } else if line == body {
                Some(Section::Body)
            } else if line == footer {
                Some(Section::Footer)
            } else {
                None
            };
            if let Some(section) = next {
                st.section = section;
                if opts.renumber {
                    st.line_no = opts.start;
                }
                out.push('\n');
                continue;
            }
        }
        let style = match st.section {
            Section::Header => &opts.header,
            Section::Body => &opts.body,
            Section::Footer => &opts.footer,
        };
        let number = match style {
            Style::All => {
                if opts.join_blank > 1 && line.is_empty() {
                    st.blank_lines += 1;
                    if st.blank_lines == opts.join_blank {
                        st.blank_lines = 0;
                        true
                    } else {
                        false
                    }
                } else {
                    st.blank_lines = 0;
                    true
                }
            }
            Style::NonEmpty => !line.is_empty(),
            Style::None => false,
            Style::Regex(re) => re.is_match(line),
        };
        if number {
            out.push_str(&format_number(st.line_no, opts.format, opts.width));
            out.push_str(&opts.separator);
            st.line_no = st.line_no.saturating_add(opts.increment);
        } else {
            out.push_str(&no_number);
        }
        out.push_str(line);
        out.push('\n');
    }
}

#[async_trait]
impl Builtin for Nl {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        if let Some(r) = super::check_help_version(
            ctx.args,
            "Usage: nl [OPTION]... [FILE]...\nNumber lines of files.\n\n  -b, --body-numbering=STYLE\tuse STYLE for numbering body lines\n  -d, --section-delimiter=CC\tuse CC for logical page delimiters\n  -f, --footer-numbering=STYLE\tuse STYLE for numbering footer lines\n  -h, --header-numbering=STYLE\tuse STYLE for numbering header lines\n  -i, --line-increment=NUMBER\tline number increment at each line\n  -l, --join-blank-lines=NUMBER\tgroup of NUMBER empty lines counted as one\n  -n, --number-format=FORMAT\tinsert line numbers according to FORMAT (ln, rn, rz)\n  -p, --no-renumber\tdo not reset line numbers for each section\n  -s, --number-separator=STRING\tadd STRING after (possible) line number\n  -v, --starting-line-number=NUMBER\tfirst line number for each section\n  -w, --number-width=NUMBER\tuse NUMBER columns for line numbers\n  --help\t\tdisplay this help and exit\n  --version\toutput version information and exit\n\nSTYLE is one of: a (all lines), t (nonempty lines), n (no lines), pBRE (lines matching BRE)\n",
            Some("nl (bashkit) 0.1"),
        ) {
            return Ok(r);
        }
        let (opts, mut files) = match parse_nl_args(ctx.args) {
            Ok(v) => v,
            Err(e) => return Ok(e),
        };
        if files.is_empty() {
            files.push("-".to_string());
        }

        let mut output = String::new();
        let mut state = NlState {
            section: Section::Body,
            line_no: opts.start,
            blank_lines: 0,
        };
        for file in &files {
            if file == "-" {
                if let Some(stdin) = ctx.stdin {
                    number_lines(stdin, &opts, &mut state, &mut output);
                }
                continue;
            }
            let path = if file.starts_with('/') {
                std::path::PathBuf::from(file)
            } else {
                vfs_join(ctx.cwd, file)
            };
            let text = match read_text_file(&*ctx.fs, &path, "nl").await {
                Ok(t) => t,
                Err(e) => return Ok(e),
            };
            number_lines(&text, &opts, &mut state, &mut output);
        }

        Ok(ExecResult::ok(output))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Arc;

    use crate::fs::{FileSystem, InMemoryFs};

    async fn run_nl(args: &[&str], stdin: Option<&str>) -> ExecResult {
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

        Nl.execute(ctx).await.unwrap()
    }

    async fn run_nl_with_fs(
        args: &[&str],
        stdin: Option<&str>,
        files: &[(&str, &[u8])],
    ) -> ExecResult {
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
            stdin: crate::builtins::test_stream_opt(stdin),
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
        };

        Nl.execute(ctx).await.unwrap()
    }

    #[tokio::test]
    async fn test_nl_basic() {
        let result = run_nl(&[], Some("hello\nworld\n")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "     1\thello\n     2\tworld\n");
    }

    #[tokio::test]
    async fn test_nl_default_skips_empty() {
        let result = run_nl(&[], Some("hello\n\nworld\n")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "     1\thello\n       \n     2\tworld\n");
    }

    #[tokio::test]
    async fn test_nl_all_lines() {
        let result = run_nl(&["-b", "a"], Some("hello\n\nworld\n")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "     1\thello\n     2\t\n     3\tworld\n");
    }

    #[tokio::test]
    async fn test_nl_no_numbering() {
        let result = run_nl(&["-b", "n"], Some("hello\nworld\n")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "       hello\n       world\n");
    }

    #[tokio::test]
    async fn test_nl_left_justified() {
        let result = run_nl(&["-n", "ln"], Some("hello\nworld\n")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "1     \thello\n2     \tworld\n");
    }

    #[tokio::test]
    async fn test_nl_right_zero() {
        let result = run_nl(&["-n", "rz"], Some("hello\nworld\n")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "000001\thello\n000002\tworld\n");
    }

    #[tokio::test]
    async fn test_nl_custom_separator() {
        let result = run_nl(&["-s", ": "], Some("hello\nworld\n")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "     1: hello\n     2: world\n");
    }

    #[tokio::test]
    async fn test_nl_custom_increment() {
        let result = run_nl(&["-i", "2"], Some("a\nb\nc\n")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "     1\ta\n     3\tb\n     5\tc\n");
    }

    #[tokio::test]
    async fn test_nl_custom_start() {
        let result = run_nl(&["-v", "10"], Some("a\nb\nc\n")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "    10\ta\n    11\tb\n    12\tc\n");
    }

    #[tokio::test]
    async fn test_nl_custom_width() {
        let result = run_nl(&["-w", "3"], Some("a\nb\n")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "  1\ta\n  2\tb\n");
    }

    #[tokio::test]
    async fn test_nl_rejects_excessive_width() {
        let too_wide = (MAX_FORMAT_WIDTH + 1).to_string();
        let result = run_nl(&["-w", &too_wide], Some("a\n")).await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("exceeds maximum"));
    }

    #[tokio::test]
    async fn test_nl_combined_options() {
        let result = run_nl(
            &[
                "-b", "a", "-n", "rz", "-w", "4", "-s", " ", "-v", "5", "-i", "3",
            ],
            Some("x\n\ny\n"),
        )
        .await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "0005 x\n0008 \n0011 y\n");
    }

    #[tokio::test]
    async fn test_nl_empty_input() {
        let result = run_nl(&[], Some("")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "");
    }

    #[tokio::test]
    async fn test_nl_no_stdin() {
        let result = run_nl(&[], None).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "");
    }

    #[tokio::test]
    async fn test_nl_from_file() {
        let result =
            run_nl_with_fs(&["/test.txt"], None, &[("/test.txt", b"one\ntwo\nthree\n")]).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "     1\tone\n     2\ttwo\n     3\tthree\n");
    }

    #[tokio::test]
    async fn test_nl_file_not_found() {
        let result = run_nl(&["/nonexistent"], None).await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("nl:"));
    }

    #[tokio::test]
    async fn test_nl_invalid_body_type() {
        let result = run_nl(&["-b", "x"], Some("test\n")).await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("invalid body numbering style"));
    }

    #[tokio::test]
    async fn test_nl_invalid_format() {
        let result = run_nl(&["-n", "xx"], Some("test\n")).await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("invalid line numbering format"));
    }

    #[tokio::test]
    async fn test_nl_single_line() {
        let result = run_nl(&[], Some("hello\n")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "     1\thello\n");
    }

    #[tokio::test]
    async fn test_nl_stdin_dash() {
        let result = run_nl(&["-"], Some("hello\nworld\n")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "     1\thello\n     2\tworld\n");
    }

    #[tokio::test]
    async fn test_nl_multiple_files() {
        let result = run_nl_with_fs(
            &["/a.txt", "/b.txt"],
            None,
            &[("/a.txt", b"one\ntwo\n"), ("/b.txt", b"three\nfour\n")],
        )
        .await;
        assert_eq!(result.exit_code, 0);
        // Line numbers continue across files
        assert_eq!(
            result.stdout,
            "     1\tone\n     2\ttwo\n     3\tthree\n     4\tfour\n"
        );
    }

    #[tokio::test]
    async fn test_nl_attached_args() {
        // Test -ba, -nrz, -w4 (attached value form)
        let result = run_nl(&["-ba", "-nrz", "-w4"], Some("x\ny\n")).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "0001\tx\n0002\ty\n");
    }

    #[tokio::test]
    async fn test_nl_rejects_unknown_option() {
        let result = run_nl(&["-Q"], Some("a\n")).await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("invalid option -- 'Q'"));
    }

    #[tokio::test]
    async fn test_nl_sections_reset_numbers() {
        let input = "\\:\\:\\:\nh\n\\:\\:\nb1\nb2\n\\:\nf\n";
        let result = run_nl(&["-ha", "-fa"], Some(input)).await;
        assert_eq!(result.exit_code, 0);
        assert_eq!(
            result.stdout,
            "\n     1\th\n\n     1\tb1\n     2\tb2\n\n     1\tf\n"
        );
    }

    #[tokio::test]
    async fn test_nl_no_renumber() {
        let result = run_nl(&["-p"], Some("a\n\\:\\:\nb\n")).await;
        assert_eq!(result.stdout, "     1\ta\n\n     2\tb\n");
    }

    #[tokio::test]
    async fn test_nl_regex_style() {
        let result = run_nl(&["-bp^x"], Some("xa\nb\nxc\n")).await;
        assert_eq!(result.stdout, "     1\txa\n       b\n     2\txc\n");
    }

    #[tokio::test]
    async fn test_nl_join_blank_lines() {
        let result = run_nl(&["-ba", "-l2"], Some("\n\n\n")).await;
        assert_eq!(result.stdout, "       \n     1\t\n       \n");
    }

    #[tokio::test]
    async fn test_nl_invalid_join_count() {
        let result = run_nl(&["-l", "0"], Some("a\n")).await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("invalid line number of blank lines"));
    }
}
