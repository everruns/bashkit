//! Shared helpers for builtin commands.
//!
//! Builtins implement [`BuiltinHelper`] on their unit struct (e.g.
//! `impl BuiltinHelper for Echo { const NAME: &'static str = "echo"; }`)
//! and inherit consistent help/version handling and error formatting from
//! the trait's default methods.
//!
//! Error helpers produce real-shell-style messages (`{name}: {msg}\n`) and
//! use `Display`, never `Debug`, satisfying TM-INF-022.

use super::check_help_version;
use crate::interpreter::ExecResult;

/// bash `sh_single_quote`: wrap in single quotes, each `'` as `'\''`.
/// Used by listings meant to be re-read as input (`alias`, `trap`).
pub(crate) fn single_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// Build the error real GNU coreutils/util-linux print for an unrecognized
/// command-line option, so builtins reject typos the way the real tools do
/// instead of silently ignoring the bad flag.
///
/// `arg` is the offending token as the user wrote it. A `--long` token yields
/// `"{cmd}: unrecognized option '--long'"`; a `-xyz` token reports the first
/// character as `"{cmd}: invalid option -- 'x'"` (matching getopt, which fails
/// on the first unknown char in a bundle). `code` is the tool's exit status —
/// most coreutils use 1, a few (sort, grep, diff, patch) use 2.
pub(crate) fn invalid_option(cmd: &str, arg: &str, code: i32) -> ExecResult {
    let msg = if let Some(long) = arg.strip_prefix("--") {
        format!("{cmd}: unrecognized option '--{long}'\n")
    } else if let Some(short) = arg.strip_prefix('-') {
        let ch = short.chars().next().unwrap_or('-');
        format!("{cmd}: invalid option -- '{ch}'\n")
    } else {
        // Not option-shaped; report verbatim (rare).
        format!("{cmd}: invalid option -- '{arg}'\n")
    };
    ExecResult::err(msg, code)
}

/// Read a multi-valued clap arg as owned strings under either value type
/// clap may have stored for it.
///
/// Decision: the `generated/*_args.rs` surfaces are ported verbatim from
/// uutils, so whether an arg carries clap's default `String` parser or an
/// explicit `OsStringValueParser` is upstream's choice, and it flips
/// across revisions. `get_many::<T>` *panics* on a type mismatch, so a
/// consumer that names one type turns an upstream parser change into a
/// panic — and the drift PR is generated-only, so it cannot fix its own
/// consumers (`knowledge/runtimes/coreutils-args-port.md` § The drift PR
/// cannot fix its own consumers). Asking for both keeps the consumer
/// valid against the current pin and upstream HEAD at the same time,
/// which is the only state in which the weekly drift run can get past its
/// build and test steps.
///
/// Non-UTF-8 bytes are replaced, matching the `to_string_lossy` reads in
/// `cat`, `ls` and `truncate`.
pub(crate) fn string_values(matches: &clap::ArgMatches, id: &str) -> Vec<String> {
    if let Ok(Some(values)) = matches.try_get_many::<std::ffi::OsString>(id) {
        return values.map(|v| v.to_string_lossy().into_owned()).collect();
    }
    if let Ok(Some(values)) = matches.try_get_many::<String>(id) {
        return values.cloned().collect();
    }
    debug_assert_readable(matches, id, "string_values");
    Vec::new()
}

/// Single-valued counterpart of [`string_values`]; same rationale.
pub(crate) fn string_value(matches: &clap::ArgMatches, id: &str) -> Option<String> {
    if let Ok(Some(value)) = matches.try_get_one::<std::ffi::OsString>(id) {
        return Some(value.to_string_lossy().into_owned());
    }
    if let Ok(Some(value)) = matches.try_get_one::<String>(id) {
        return Some(value.clone());
    }
    debug_assert_readable(matches, id, "string_value");
    None
}

/// Guard the one hole the tolerant reads above open: a *third* value type.
///
/// Trying two types and giving up turns clap's loud panic into a silent
/// "no value", which for an arg the user did pass would mean the flag is
/// quietly ignored — worse than a panic, because nothing reports it. Only
/// `String` and `OsString` are expected (upstream flips between exactly
/// those two), so an arg that is present and readable as neither is a
/// porting bug, not user input. Firing only under `debug_assertions`
/// keeps the loud signal everywhere it is actionable — `cargo test`, and
/// so the weekly drift run's own test steps — while an embedder's release
/// build degrades instead of aborting.
fn debug_assert_readable(matches: &clap::ArgMatches, id: &str, caller: &str) {
    debug_assert!(
        !matches.try_contains_id(id).unwrap_or(false),
        "helpers::{caller}: arg `{id}` is present but is neither OsString \
         nor String; teach this helper the value type upstream now uses"
    );
}

/// Default helpers shared by builtin commands.
///
/// Each impl binds the command name to the type, removing the need to
/// repeat it in every `format!("{cmd}: ...")` call site. All methods are
/// associated functions; call them as `Self::err(...)` from inside
/// `impl Builtin for X` (where `Self = X`) once the trait is in scope.
pub(crate) trait BuiltinHelper {
    /// Canonical command name as it appears in error messages.
    const NAME: &'static str;

    /// Standard `--help` / `--version` dispatch.
    ///
    /// Returns `Some(result)` when one of the flags is present; the
    /// caller should propagate it as `return Ok(r)`.
    fn check_help(args: &[String], help_text: &str, version: Option<&str>) -> Option<ExecResult> {
        check_help_version(args, help_text, version)
    }

    /// Format `{NAME}: {msg}\n` with the given exit code.
    fn err(msg: impl AsRef<str>, code: i32) -> ExecResult {
        ExecResult::err(format!("{}: {}\n", Self::NAME, msg.as_ref()), code)
    }

    /// Format `{NAME}: {path}: {msg}\n` for path-qualified errors.
    fn err_path(path: impl std::fmt::Display, msg: impl AsRef<str>, code: i32) -> ExecResult {
        ExecResult::err(
            format!("{}: {}: {}\n", Self::NAME, path, msg.as_ref()),
            code,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Demo;
    impl BuiltinHelper for Demo {
        const NAME: &'static str = "demo";
    }

    #[test]
    fn err_prefixes_name() {
        let r = Demo::err("boom", 2);
        assert_eq!(r.stderr, "demo: boom\n");
        assert_eq!(r.exit_code, 2);
    }

    #[test]
    fn err_path_includes_path() {
        let r = Demo::err_path("/etc/foo", "permission denied", 1);
        assert_eq!(r.stderr, "demo: /etc/foo: permission denied\n");
    }

    #[test]
    fn invalid_option_short() {
        let r = invalid_option("wc", "-Q", 1);
        assert_eq!(r.stderr, "wc: invalid option -- 'Q'\n");
        assert_eq!(r.exit_code, 1);
    }

    #[test]
    fn invalid_option_short_reports_first_char() {
        let r = invalid_option("sort", "-lQ", 2);
        assert_eq!(r.stderr, "sort: invalid option -- 'l'\n");
        assert_eq!(r.exit_code, 2);
    }

    #[test]
    fn invalid_option_long() {
        let r = invalid_option("wc", "--foo", 1);
        assert_eq!(r.stderr, "wc: unrecognized option '--foo'\n");
    }

    /// Two spellings of the same surface: clap's default `String` parser
    /// (what the pinned uutils rev generates) and an explicit
    /// `OsStringValueParser` (what upstream HEAD generates).
    fn arg_surface(os_string: bool) -> clap::Command {
        let many = clap::Arg::new("files")
            .num_args(0..)
            .action(clap::ArgAction::Append);
        let one = clap::Arg::new("width").long("width");
        let (many, one) = if os_string {
            (
                many.value_parser(clap::builder::OsStringValueParser::new()),
                one.value_parser(clap::builder::OsStringValueParser::new()),
            )
        } else {
            (many, one)
        };
        clap::Command::new("demo").arg(many).arg(one)
    }

    #[test]
    fn string_values_reads_either_value_parser() {
        for os_string in [false, true] {
            let matches = arg_surface(os_string)
                .try_get_matches_from(["demo", "a", "b"])
                .unwrap();
            assert_eq!(
                string_values(&matches, "files"),
                vec!["a".to_string(), "b".to_string()],
                "os_string={os_string}"
            );
        }
    }

    #[test]
    fn string_value_reads_either_value_parser() {
        for os_string in [false, true] {
            let matches = arg_surface(os_string)
                .try_get_matches_from(["demo", "--width", "16"])
                .unwrap();
            assert_eq!(
                string_value(&matches, "width").as_deref(),
                Some("16"),
                "os_string={os_string}"
            );
        }
    }

    #[test]
    fn string_reads_report_absent_args_as_empty() {
        for os_string in [false, true] {
            let matches = arg_surface(os_string)
                .try_get_matches_from(["demo"])
                .unwrap();
            assert!(string_values(&matches, "files").is_empty());
            assert_eq!(string_value(&matches, "width"), None);
        }
    }

    /// A value type neither read handles must not pass silently: a flag the
    /// user did supply would otherwise be dropped with no diagnostic.
    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "neither OsString nor String")]
    fn string_value_rejects_an_unhandled_value_type() {
        let matches = clap::Command::new("demo")
            .arg(
                clap::Arg::new("width")
                    .long("width")
                    .value_parser(clap::value_parser!(u64)),
            )
            .try_get_matches_from(["demo", "--width", "16"])
            .unwrap();
        let _ = string_value(&matches, "width");
    }

    /// An id that is not in the surface at all must not panic the way
    /// `get_many`/`get_one` would.
    #[test]
    fn string_reads_tolerate_unknown_ids() {
        let matches = arg_surface(true).try_get_matches_from(["demo"]).unwrap();
        assert!(string_values(&matches, "nope").is_empty());
        assert_eq!(string_value(&matches, "nope"), None);
    }

    #[test]
    fn check_help_delegates() {
        let args = vec!["--help".to_string()];
        let r = Demo::check_help(&args, "usage text\n", Some("demo 0.1"));
        assert!(r.is_some());
        assert_eq!(r.unwrap().stdout, "usage text\n");
    }
}
