//! Command-line argument parsing for jq.
//!
//! Important decisions:
//!  - Index-based loop (not clap) — multi-arg flags like `--arg name value`
//!    and `--slurpfile name file` need to consume the following two positions
//!    explicitly. Combined short flags (`-rn`, `-snr`) are split per-char.
//!  - Unknown flags produce a usage error (exit 2). Real jq does the same;
//!    silent ignore would mask agent typos.
//!  - `--args` / `--jsonargs` switch the meaning of the remaining positional
//!    arguments: instead of files, they become string/JSON values stored
//!    in `$ARGS.positional`.

use crate::interpreter::ExecResult;

use super::convert::{MAX_JQ_JSON_DEPTH, check_json_depth};
use super::format::Indent;

/// Maximum number of positional `--args` / `--jsonargs` values per call.
/// jq has no documented cap; we apply one defensively to keep memory bounded.
pub(super) const MAX_ARGS_POSITIONAL: usize = 4096;

/// Parsed jq invocation. Fields mirror the documented jq options modulo
/// the few we explicitly do not implement (`--seq`, `--stream`, color flags).
pub(super) struct JqArgs<'a> {
    pub filter: &'a str,
    pub raw_input: bool,
    pub raw_output: bool,
    pub join_output: bool,
    pub compact_output: bool,
    pub null_input: bool,
    pub slurp: bool,
    pub sort_keys: bool,
    /// `-e` flag — set exit status from output.
    pub exit_status: bool,
    pub indent: Indent,
    pub file_args: Vec<&'a str>,
    /// Named string/JSON variables: `(name_with_$, value)`.
    pub var_bindings: Vec<(String, serde_json::Value)>,
    /// `--args` positional (string) / `--jsonargs` positional (json).
    pub positional_args: Vec<serde_json::Value>,
    /// Named args from `--arg`/`--argjson`, exposed via `$ARGS.named`.
    pub named_args: Vec<(String, serde_json::Value)>,
    /// `--slurpfile name file` and `--rawfile name file` requests, resolved
    /// after parsing.
    pub file_var_requests: Vec<FileVarRequest<'a>>,
}

#[derive(Debug)]
pub(super) struct FileVarRequest<'a> {
    pub name: String,
    pub path: &'a str,
    pub kind: FileVarKind,
}

#[derive(Debug, Clone, Copy)]
pub(super) enum FileVarKind {
    /// `--rawfile name file`: bind the file content as a raw string.
    Raw,
    /// `--slurpfile name file`: parse the file as a JSON stream and bind
    /// as an array of values.
    Slurp,
}

/// Outcome of argument parsing. Either a fully-resolved `JqArgs`, or an
/// `ExecResult` (with appropriate exit code) for short-circuit cases like
/// `--help`, `--version`, and unknown-flag rejection.
pub(super) enum ParseOutcome<'a> {
    Args(JqArgs<'a>),
    Done(ExecResult),
}

/// Real jq accepts `--indent` 0..=7. We cap defensively but match the same
/// range so familiar invocations work.
const MAX_JQ_INDENT: u8 = 7;

const HELP_TEXT: &str = "Usage: jq [OPTIONS...] FILTER [FILE...]\n\n\
    \tjq is a command-line JSON processor.\n\n\
    Options:\n\
    \t-c, --compact-output\tcompact instead of pretty-printed output\n\
    \t-r, --raw-output\toutput strings without escapes and quotes\n\
    \t-R, --raw-input\t\tread each line as string instead of JSON\n\
    \t-s, --slurp\t\tread entire input into a single array\n\
    \t-n, --null-input\tuse null as the single input value\n\
    \t-e, --exit-status\tset exit status code based on output\n\
    \t-S, --sort-keys\t\tsort object keys in output\n\
    \t-j, --join-output\tlike -r without trailing newline\n\
    \t--tab\t\t\tuse tabs for indentation\n\
    \t--indent N\t\tuse N spaces for indentation (0..7, default 2)\n\
    \t--arg name value\tset variable $name to string value\n\
    \t--argjson name value\tset variable $name to JSON value\n\
    \t--slurpfile name file\tbind $name to JSON values parsed from file\n\
    \t--rawfile name file\tbind $name to raw string contents of file\n\
    \t--args\t\t\tremaining args populate $ARGS.positional as strings\n\
    \t--jsonargs\t\tremaining args populate $ARGS.positional as JSON values\n\
    \t-V, --version\t\toutput version information and exit\n\
    \t-h, --help\t\toutput this help and exit\n";

const VERSION_TEXT: &str = "jq-1.8\n";

const UNKNOWN_OPTION_HINT: &str = "Use jq --help for help with command-line options.\n";

/// Parse `args` into either a fully-validated `JqArgs` or an early-exit
/// `ExecResult` (for help, version, or usage error).
pub(super) fn parse<'a>(args: &'a [String]) -> ParseOutcome<'a> {
    // Help / version short-circuit first so they work even after other flags.
    for arg in args {
        match arg.as_str() {
            "-h" | "--help" => return ParseOutcome::Done(ExecResult::ok(HELP_TEXT.to_string())),
            "-V" | "--version" => {
                return ParseOutcome::Done(ExecResult::ok(VERSION_TEXT.to_string()));
            }
            _ => {}
        }
    }

    let mut out = JqArgs {
        filter: ".",
        raw_input: false,
        raw_output: false,
        join_output: false,
        compact_output: false,
        null_input: false,
        slurp: false,
        sort_keys: false,
        exit_status: false,
        indent: Indent::Spaces(2),
        file_args: Vec::new(),
        var_bindings: Vec::new(),
        positional_args: Vec::new(),
        named_args: Vec::new(),
        file_var_requests: Vec::new(),
    };

    let mut found_filter = false;
    let mut positional_mode: Option<PositionalMode> = None;
    let mut i = 0;

    while i < args.len() {
        let arg = &args[i];

        // `--` ends option parsing. Whatever follows is filter (if not yet
        // found) or files / $ARGS.positional.
        if arg == "--" {
            i += 1;
            // After --, no more flags. Drain remaining args into the
            // appropriate positional bucket.
            while i < args.len() {
                let next = &args[i];
                if !found_filter {
                    out.filter = next;
                    found_filter = true;
                } else if let Some(mode) = positional_mode {
                    if let Err(e) = push_positional(&mut out, mode, next) {
                        return ParseOutcome::Done(*e);
                    }
                } else {
                    out.file_args.push(next);
                }
                i += 1;
            }
            continue;
        }

        // After --args / --jsonargs, all remaining non-flag args route to
        // $ARGS.positional (the filter, if not yet seen, still wins first).
        if let Some(mode) = positional_mode
            && !is_flag(arg)
        {
            if !found_filter {
                out.filter = arg;
                found_filter = true;
            } else if let Err(e) = push_positional(&mut out, mode, arg) {
                return ParseOutcome::Done(*e);
            }
            i += 1;
            continue;
        }

        match arg.as_str() {
            "--raw-output" => out.raw_output = true,
            "--raw-input" => out.raw_input = true,
            "--compact-output" => out.compact_output = true,
            "--null-input" => out.null_input = true,
            "--sort-keys" => out.sort_keys = true,
            "--slurp" => out.slurp = true,
            "--exit-status" => out.exit_status = true,
            "--tab" => out.indent = Indent::Tab,
            "--join-output" => out.join_output = true,
            "--ascii-output"
            | "-a"
            | "-C"
            | "-M"
            | "--color-output"
            | "--monochrome-output"
            | "--unbuffered" => {
                // Recognised but no-op: rendering is not TTY-aware in this
                // sandbox, and ASCII-only output isn't implemented yet.
            }
            "--indent" => match args.get(i + 1) {
                Some(n) => match n.parse::<u8>() {
                    Ok(n) if n <= MAX_JQ_INDENT => {
                        if !matches!(out.indent, Indent::Tab) {
                            out.indent = Indent::Spaces(n);
                        }
                        i += 2;
                        continue;
                    }
                    Ok(n) => {
                        return ParseOutcome::Done(usage_error(format!(
                            "jq: --indent must be 0..={MAX_JQ_INDENT}, got {n}"
                        )));
                    }
                    Err(_) => {
                        return ParseOutcome::Done(usage_error(format!(
                            "jq: --indent expected a number, got '{n}'"
                        )));
                    }
                },
                None => {
                    return ParseOutcome::Done(usage_error(
                        "jq: --indent requires an argument".into(),
                    ));
                }
            },
            "--arg" | "--argjson" => match (args.get(i + 1), args.get(i + 2)) {
                (Some(name), Some(value)) => {
                    let var = format!("${name}");
                    let v = if arg == "--arg" {
                        serde_json::Value::String(value.clone())
                    } else {
                        match serde_json::from_str::<serde_json::Value>(value) {
                            Ok(v) => {
                                if let Err(e) = check_json_depth(&v, MAX_JQ_JSON_DEPTH) {
                                    return ParseOutcome::Done(ExecResult::err(
                                        format!("{e}\n"),
                                        2,
                                    ));
                                }
                                v
                            }
                            Err(e) => {
                                return ParseOutcome::Done(usage_error(format!(
                                    "jq: invalid JSON for --argjson: {e}"
                                )));
                            }
                        }
                    };
                    out.var_bindings.push((var, v.clone()));
                    out.named_args.push((name.clone(), v));
                    i += 3;
                    continue;
                }
                _ => {
                    return ParseOutcome::Done(usage_error(format!(
                        "jq: {arg} requires two arguments"
                    )));
                }
            },
            "--slurpfile" | "--rawfile" => match (args.get(i + 1), args.get(i + 2)) {
                (Some(name), Some(path)) => {
                    let kind = if arg == "--slurpfile" {
                        FileVarKind::Slurp
                    } else {
                        FileVarKind::Raw
                    };
                    out.file_var_requests.push(FileVarRequest {
                        name: name.clone(),
                        path: path.as_str(),
                        kind,
                    });
                    i += 3;
                    continue;
                }
                _ => {
                    return ParseOutcome::Done(usage_error(format!(
                        "jq: {arg} requires two arguments"
                    )));
                }
            },
            "--args" => {
                positional_mode = Some(PositionalMode::Strings);
            }
            "--jsonargs" => {
                positional_mode = Some(PositionalMode::Json);
            }
            // Long flag we don't recognise: error out (matches real jq).
            s if s.starts_with("--") => {
                return ParseOutcome::Done(unknown_option(s));
            }
            // Short flag(s): may be combined like -rn, -sc, -snr.
            s if s.starts_with('-') && s.len() > 1 => {
                for ch in s[1..].chars() {
                    match ch {
                        'r' => out.raw_output = true,
                        'R' => out.raw_input = true,
                        'c' => out.compact_output = true,
                        'n' => out.null_input = true,
                        'S' => out.sort_keys = true,
                        's' => out.slurp = true,
                        'e' => out.exit_status = true,
                        'j' => out.join_output = true,
                        'a' | 'C' | 'M' => {} // ASCII / color / monochrome — accept silently
                        unknown => {
                            return ParseOutcome::Done(unknown_option(&format!("-{unknown}")));
                        }
                    }
                }
            }
            // Non-flag argument: filter if not yet seen, otherwise file
            // argument. Real jq accepts options anywhere on the line, so
            // we keep checking for flags even after the filter is set.
            _ => {
                if !found_filter {
                    out.filter = arg;
                    found_filter = true;
                } else {
                    out.file_args.push(arg);
                }
            }
        }
        i += 1;
    }

    ParseOutcome::Args(out)
}

fn is_flag(s: &str) -> bool {
    s.starts_with('-') && s.len() > 1
}

fn push_positional(
    out: &mut JqArgs<'_>,
    mode: PositionalMode,
    arg: &str,
) -> std::result::Result<(), Box<ExecResult>> {
    if out.positional_args.len() >= MAX_ARGS_POSITIONAL {
        return Err(Box::new(usage_error(format!(
            "jq: too many positional arguments (max {MAX_ARGS_POSITIONAL})"
        ))));
    }
    match mode {
        PositionalMode::Strings => {
            out.positional_args
                .push(serde_json::Value::String(arg.to_owned()));
        }
        PositionalMode::Json => match serde_json::from_str::<serde_json::Value>(arg) {
            Ok(v) => {
                if let Err(e) = check_json_depth(&v, MAX_JQ_JSON_DEPTH) {
                    return Err(Box::new(ExecResult::err(format!("{e}\n"), 2)));
                }
                out.positional_args.push(v);
            }
            Err(e) => {
                return Err(Box::new(usage_error(format!(
                    "jq: invalid JSON for --jsonargs: {e}"
                ))));
            }
        },
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum PositionalMode {
    Strings,
    Json,
}

fn usage_error(msg: String) -> ExecResult {
    ExecResult::err(format!("{msg}\n{UNKNOWN_OPTION_HINT}"), 2)
}

fn unknown_option(opt: &str) -> ExecResult {
    ExecResult::err(
        format!("jq: Unknown option {opt}\n{UNKNOWN_OPTION_HINT}"),
        2,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_strs(args: &[&str]) -> ParseOutcome<'static> {
        let v: Vec<String> = args.iter().map(|s| (*s).to_string()).collect();
        // SAFETY: tests only — leak the vec so the lifetime borrow checks.
        // (We're parsing a borrowed slice but tests need to inspect output.)
        let leaked: &'static [String] = Box::leak(v.into_boxed_slice());
        parse(leaked)
    }

    #[test]
    fn help_short_circuits() {
        match parse_strs(&["--help"]) {
            ParseOutcome::Done(r) => {
                assert_eq!(r.exit_code, 0);
                assert!(r.stdout.contains("Usage:"));
            }
            _ => panic!("expected Done"),
        }
    }

    #[test]
    fn version_short_circuits() {
        match parse_strs(&["-V"]) {
            ParseOutcome::Done(r) => {
                assert_eq!(r.exit_code, 0);
                assert!(r.stdout.starts_with("jq-"));
            }
            _ => panic!("expected Done"),
        }
    }

    #[test]
    fn unknown_long_flag_errors() {
        match parse_strs(&["--xyzzy", "."]) {
            ParseOutcome::Done(r) => {
                assert_eq!(r.exit_code, 2);
                assert!(r.stderr.contains("Unknown option --xyzzy"));
            }
            _ => panic!("expected Done with error"),
        }
    }

    #[test]
    fn unknown_short_flag_errors() {
        match parse_strs(&["-Z", "."]) {
            ParseOutcome::Done(r) => {
                assert_eq!(r.exit_code, 2);
                assert!(r.stderr.contains("Unknown option -Z"));
            }
            _ => panic!("expected Done with error"),
        }
    }

    #[test]
    fn combined_short_flags() {
        match parse_strs(&["-rn", "1+1"]) {
            ParseOutcome::Args(a) => {
                assert!(a.raw_output);
                assert!(a.null_input);
                assert_eq!(a.filter, "1+1");
            }
            _ => panic!("expected Args"),
        }
    }

    #[test]
    fn indent_parses_valid_value() {
        match parse_strs(&["--indent", "4", "."]) {
            ParseOutcome::Args(a) => assert!(matches!(a.indent, Indent::Spaces(4))),
            _ => panic!("expected Args"),
        }
    }

    #[test]
    fn indent_rejects_too_large() {
        match parse_strs(&["--indent", "9", "."]) {
            ParseOutcome::Done(r) => {
                assert_eq!(r.exit_code, 2);
                assert!(r.stderr.contains("--indent must be"));
            }
            _ => panic!("expected Done"),
        }
    }

    #[test]
    fn indent_rejects_non_numeric() {
        match parse_strs(&["--indent", "abc", "."]) {
            ParseOutcome::Done(r) => {
                assert_eq!(r.exit_code, 2);
                assert!(r.stderr.contains("expected a number"));
            }
            _ => panic!("expected Done"),
        }
    }

    #[test]
    fn arg_binds_string_var() {
        match parse_strs(&["--arg", "x", "hi", "."]) {
            ParseOutcome::Args(a) => {
                assert_eq!(a.var_bindings.len(), 1);
                assert_eq!(a.var_bindings[0].0, "$x");
                assert_eq!(a.var_bindings[0].1, serde_json::Value::String("hi".into()));
            }
            _ => panic!("expected Args"),
        }
    }

    #[test]
    fn argjson_binds_json_value() {
        match parse_strs(&["--argjson", "n", "42", "."]) {
            ParseOutcome::Args(a) => {
                assert_eq!(a.var_bindings[0].0, "$n");
                assert_eq!(a.var_bindings[0].1, serde_json::json!(42));
            }
            _ => panic!("expected Args"),
        }
    }

    #[test]
    fn argjson_invalid_returns_usage_error() {
        match parse_strs(&["--argjson", "x", "not json", "."]) {
            ParseOutcome::Done(r) => {
                assert_eq!(r.exit_code, 2);
                assert!(r.stderr.contains("invalid JSON"));
            }
            _ => panic!("expected Done"),
        }
    }

    #[test]
    fn slurpfile_records_request() {
        match parse_strs(&["--slurpfile", "data", "/x.json", "."]) {
            ParseOutcome::Args(a) => {
                assert_eq!(a.file_var_requests.len(), 1);
                assert_eq!(a.file_var_requests[0].name, "data");
                assert_eq!(a.file_var_requests[0].path, "/x.json");
                assert!(matches!(a.file_var_requests[0].kind, FileVarKind::Slurp));
            }
            _ => panic!("expected Args"),
        }
    }

    #[test]
    fn rawfile_records_request() {
        match parse_strs(&["--rawfile", "txt", "/x.txt", "."]) {
            ParseOutcome::Args(a) => {
                assert_eq!(a.file_var_requests[0].name, "txt");
                assert!(matches!(a.file_var_requests[0].kind, FileVarKind::Raw));
            }
            _ => panic!("expected Args"),
        }
    }

    #[test]
    fn args_strings_become_positional() {
        match parse_strs(&["-n", ".", "--args", "a", "b", "c"]) {
            ParseOutcome::Args(a) => {
                assert!(a.null_input);
                assert_eq!(a.positional_args.len(), 3);
                assert_eq!(a.positional_args[0], serde_json::json!("a"));
                assert_eq!(a.positional_args[2], serde_json::json!("c"));
                assert!(a.file_args.is_empty());
            }
            _ => panic!("expected Args"),
        }
    }

    #[test]
    fn jsonargs_become_positional_json() {
        match parse_strs(&["-n", ".", "--jsonargs", "1", "true", r#"{"a":1}"#]) {
            ParseOutcome::Args(a) => {
                assert_eq!(a.positional_args[0], serde_json::json!(1));
                assert_eq!(a.positional_args[1], serde_json::json!(true));
                assert_eq!(a.positional_args[2], serde_json::json!({"a":1}));
            }
            _ => panic!("expected Args"),
        }
    }

    #[test]
    fn jsonargs_invalid_returns_usage_error() {
        match parse_strs(&["-n", ".", "--jsonargs", "not json"]) {
            ParseOutcome::Done(r) => {
                assert_eq!(r.exit_code, 2);
                assert!(r.stderr.contains("--jsonargs"));
            }
            _ => panic!("expected Done"),
        }
    }

    #[test]
    fn double_dash_terminates_options() {
        match parse_strs(&["-n", "--", "1+1"]) {
            ParseOutcome::Args(a) => {
                assert!(a.null_input);
                assert_eq!(a.filter, "1+1");
            }
            _ => panic!("expected Args"),
        }
    }

    #[test]
    fn arg_does_not_swallow_filter() {
        match parse_strs(&["--arg", "x", "hello", "."]) {
            ParseOutcome::Args(a) => {
                assert_eq!(a.filter, ".");
                assert!(a.file_args.is_empty());
            }
            _ => panic!("expected Args"),
        }
    }

    #[test]
    fn ascii_output_silently_accepted() {
        match parse_strs(&["-a", "."]) {
            ParseOutcome::Args(a) => assert_eq!(a.filter, "."),
            _ => panic!("expected Args (-a should be accepted)"),
        }
    }

    #[test]
    fn color_flags_silently_accepted() {
        for flag in ["-C", "-M", "--color-output", "--monochrome-output"] {
            match parse_strs(&[flag, "."]) {
                ParseOutcome::Args(_) => {}
                _ => panic!("{flag} should be accepted"),
            }
        }
    }

    #[test]
    fn snr_combined_short_flags() {
        match parse_strs(&["-snr", "."]) {
            ParseOutcome::Args(a) => {
                assert!(a.slurp);
                assert!(a.null_input);
                assert!(a.raw_output);
            }
            _ => panic!("expected Args"),
        }
    }
}
