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

    #[test]
    fn check_help_delegates() {
        let args = vec!["--help".to_string()];
        let r = Demo::check_help(&args, "usage text\n", Some("demo 0.1"));
        assert!(r.is_some());
        assert_eq!(r.unwrap().stdout, "usage text\n");
    }
}
