//! Virtual-env shim for clap-based ported builtins.
//!
//! uutils' `uu_app()` ships `Arg::env("FOO")` to default options like
//! `TIME_STYLE` / `BLOCK_SIZE` / `LS_COLORS` from the host process
//! environment. bashkit sandboxes scripts inside `ctx.env`, so codegen
//! strips `.env(...)` from the runtime Arg chain (TM-INF-024) and
//! instead emits a sidecar `<UTIL>_ENV_DEFAULTS: &[EnvDefault]` table
//! recording what was stripped.
//!
//! `apply_env_defaults` reads that table plus the caller's virtual
//! `ctx.env` and rewrites argv before clap sees it, emulating clap's
//! own "argv > env > default" precedence — but sourced from the
//! sandbox, never `std::env`. One bashkit-side seam, one audit point.

use std::collections::HashMap;
use std::ffi::OsString;

/// How an `Arg`'s value materialises on the command line. Mirrors the
/// shape of the clap chain we found `.env(...)` next to.
//
// `Multi` and (later) other variants are reserved for upcoming codegen
// emissions — current ported utils only exercise `Single`. Allow dead
// code so the surface stays committed without forcing a port that
// uses it on day one.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnvKind {
    /// `--<long> <value>`. The default for value-bearing options
    /// (uutils' `TIME_STYLE`, `TABSIZE`, `BLOCK_SIZE`, …).
    Single,
    /// `--<long>` with no value. uutils sets `.action(SetTrue)` for
    /// boolean env-defaulted toggles (none ported yet but reserved
    /// so codegen can emit them without shim changes). Truthy iff
    /// the env value is non-empty (matches clap's behaviour).
    Bool,
    /// Multi-value, comma-or-delim-separated. uutils uses
    /// `.value_delimiter(',')` for things like
    /// `LS_COLORS`-adjacent lists. We push `--<long>` once with the
    /// raw value; clap re-splits it via its own delimiter setting.
    Multi,
}

/// One stripped `.env(...)` annotation, harvested at codegen time.
///
/// Lives in `static`-friendly form so generated files can place these
/// in a `&'static [EnvDefault]` slice without any runtime allocation.
#[derive(Debug, Clone, Copy)]
pub struct EnvDefault {
    /// `Arg::new(arg_id)` — matches `ArgMatches::ids()` post-parse.
    /// Carried so callers / round-trip diagnostics can correlate a
    /// default back to its clap option even though the shim itself
    /// only needs `long`. Held intentionally; suppress the unused-field
    /// warning until a consumer reaches for it.
    #[allow(dead_code)]
    pub arg_id: &'static str,
    /// `Arg::long(long)` — the long flag we synthesise into argv.
    pub long: &'static str,
    /// The env-var name uutils originally pulled from `std::env`.
    /// We read this from `ctx.env` instead.
    pub env_var: &'static str,
    /// Drives the argv shape we synthesise.
    pub kind: EnvKind,
}

/// Inject env-sourced defaults into argv before handing it to clap.
///
/// Precedence (matches clap's own `Arg::env`):
///   1. argv (if user already passed `--<long>` or `--<long>=…`, leave it).
///   2. ctx.env[env_var] (if set, synthesise `--<long> <value>`).
///   3. clap's `.default_value(...)` (untouched).
///
/// `argv[0]` is the program name (clap convention); we never inspect or
/// modify it. Synthesised entries are appended *after* the existing
/// argv; clap doesn't care about positional order for flags. Positional
/// arguments are unaffected — none of the env-bound uutils args are
/// positional.
///
/// Empty env value = unset, intentionally:
///   - For `Single`/`Multi`, clap would reject an empty value via
///     `NonEmptyStringValueParser` anyway (uutils' default for these).
///   - For `Bool`, clap's own `Arg::env` matches "non-empty" as truthy.
pub fn apply_env_defaults(
    mut argv: Vec<OsString>,
    defaults: &[EnvDefault],
    env: &HashMap<String, String>,
) -> Vec<OsString> {
    for d in defaults {
        if argv_has_long(&argv, d.long) {
            continue;
        }
        let Some(val) = env.get(d.env_var) else {
            continue;
        };
        if val.is_empty() {
            continue;
        }
        let long_flag = format!("--{}", d.long);
        match d.kind {
            EnvKind::Bool => argv.push(OsString::from(long_flag)),
            EnvKind::Single | EnvKind::Multi => {
                argv.push(OsString::from(long_flag));
                argv.push(OsString::from(val));
            }
        }
    }
    argv
}

/// Returns `true` iff argv already specifies `--<long>` (with or
/// without an attached `=value`). Matches clap's own
/// "is-this-flag-already-set" check shape, modulo short flags — uutils
/// only env-defaults options that have a `.long(...)`, so we only check
/// the long form. Skips index 0 (program name).
fn argv_has_long(argv: &[OsString], long: &str) -> bool {
    let exact = format!("--{long}");
    let prefix = format!("--{long}=");
    argv.iter().skip(1).any(|a| {
        let Some(s) = a.to_str() else {
            return false;
        };
        s == exact || s.starts_with(&prefix)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(parts: &[&str]) -> Vec<OsString> {
        parts.iter().map(OsString::from).collect()
    }

    fn env(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect()
    }

    const TIME_STYLE: EnvDefault = EnvDefault {
        arg_id: "time-style",
        long: "time-style",
        env_var: "TIME_STYLE",
        kind: EnvKind::Single,
    };
    const TABSIZE: EnvDefault = EnvDefault {
        arg_id: "tab-size",
        long: "tabsize",
        env_var: "TABSIZE",
        kind: EnvKind::Single,
    };
    const COLOR_TOGGLE: EnvDefault = EnvDefault {
        arg_id: "color",
        long: "color",
        env_var: "CLICOLOR",
        kind: EnvKind::Bool,
    };

    #[test]
    fn single_value_env_synthesised_when_argv_silent() {
        let out = apply_env_defaults(
            argv(&["ls"]),
            &[TIME_STYLE],
            &env(&[("TIME_STYLE", "long-iso")]),
        );
        assert_eq!(out, argv(&["ls", "--time-style", "long-iso"]));
    }

    #[test]
    fn argv_long_overrides_env_default() {
        let out = apply_env_defaults(
            argv(&["ls", "--time-style", "iso"]),
            &[TIME_STYLE],
            &env(&[("TIME_STYLE", "long-iso")]),
        );
        // ctx.env value not appended; original argv preserved verbatim.
        assert_eq!(out, argv(&["ls", "--time-style", "iso"]));
    }

    #[test]
    fn argv_long_equals_form_overrides_env_default() {
        let out = apply_env_defaults(
            argv(&["ls", "--time-style=iso"]),
            &[TIME_STYLE],
            &env(&[("TIME_STYLE", "long-iso")]),
        );
        assert_eq!(out, argv(&["ls", "--time-style=iso"]));
    }

    #[test]
    fn missing_env_var_leaves_argv_alone() {
        let out = apply_env_defaults(argv(&["ls"]), &[TIME_STYLE], &env(&[]));
        assert_eq!(out, argv(&["ls"]));
    }

    #[test]
    fn empty_env_value_is_treated_as_unset() {
        // Matches clap::Arg::env behaviour: empty == not set for value
        // options (NonEmptyStringValueParser would reject anyway).
        let out = apply_env_defaults(argv(&["ls"]), &[TIME_STYLE], &env(&[("TIME_STYLE", "")]));
        assert_eq!(out, argv(&["ls"]));
    }

    #[test]
    fn bool_kind_appends_flag_only() {
        let out = apply_env_defaults(argv(&["ls"]), &[COLOR_TOGGLE], &env(&[("CLICOLOR", "1")]));
        assert_eq!(out, argv(&["ls", "--color"]));
    }

    #[test]
    fn multiple_defaults_processed_independently() {
        let out = apply_env_defaults(
            argv(&["ls"]),
            &[TIME_STYLE, TABSIZE],
            &env(&[("TIME_STYLE", "iso"), ("TABSIZE", "4")]),
        );
        assert_eq!(out, argv(&["ls", "--time-style", "iso", "--tabsize", "4"]));
    }

    #[test]
    fn does_not_read_std_env() {
        // Sanity: a var present on the host process but absent from
        // ctx.env must NOT leak through. Belt-and-braces against
        // future regressions of TM-INF-024; the fn signature already
        // forbids std::env access (no &std::env::Vars argument).
        // SAFETY: `std::env::set_var` is unsafe in 2024 edition; this
        // test reads its own var only, doesn't race with anything.
        unsafe { std::env::set_var("TIME_STYLE_HOST_LEAK_PROBE", "leaked") };
        let out = apply_env_defaults(
            argv(&["ls"]),
            &[EnvDefault {
                arg_id: "probe",
                long: "probe",
                env_var: "TIME_STYLE_HOST_LEAK_PROBE",
                kind: EnvKind::Single,
            }],
            &env(&[]), // empty virtual env
        );
        unsafe { std::env::remove_var("TIME_STYLE_HOST_LEAK_PROBE") };
        assert_eq!(out, argv(&["ls"]));
    }

    #[test]
    fn argv_program_name_is_never_matched_as_flag() {
        // If someone passes argv[0] = "--time-style" by mistake, we
        // must still inject the env default — argv[0] is the program
        // name slot per clap convention.
        let out = apply_env_defaults(
            argv(&["--time-style"]),
            &[TIME_STYLE],
            &env(&[("TIME_STYLE", "iso")]),
        );
        assert_eq!(out, argv(&["--time-style", "--time-style", "iso"]));
    }
}
