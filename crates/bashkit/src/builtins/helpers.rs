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
    fn check_help_delegates() {
        let args = vec!["--help".to_string()];
        let r = Demo::check_help(&args, "usage text\n", Some("demo 0.1"));
        assert!(r.is_some());
        assert_eq!(r.unwrap().stdout, "usage text\n");
    }
}
