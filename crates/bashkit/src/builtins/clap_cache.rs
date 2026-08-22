//! Cached `clap::Command` construction for the coreutils-ported builtins.
//!
//! # Decision: cache a *built* `Command`, clone per invocation
//!
//! Every ported builtin used to call `<util>_command()` (generated from
//! uutils' `uu_app()`) on each invocation, so a script running `cat` in a
//! loop reconstructed the whole arg tree N times. Measured on a 4-core Xeon
//! (`cargo bench --bench builtin_args`, plus a standalone clap micro-harness),
//! per invocation:
//!
//! | | `cat` (12 args) | `ls` (~60 args) |
//! |---|---|---|
//! | construct `Command` only | 1.1 µs | — |
//! | construct + `try_get_matches_from` | 7.3 µs | 44.7 µs |
//! | clone cached *unbuilt* + parse | 7.0 µs | 37.8 µs |
//! | clone cached *built* + parse | 6.5 µs | 35.5 µs |
//!
//! Two things follow, and both shaped this module:
//!
//! 1. **Construction is not the expensive half — parsing is.** Building `cat`'s
//!    tree costs 1.1 µs of the 7.3 µs round trip. Caching to avoid construction
//!    alone is worth ~4%, which is why this is not a plain `LazyLock<Command>`
//!    holding the output of `<util>_command()`.
//! 2. **Most of the parse cost is `_build_self`, and it is cacheable.** clap
//!    finalizes a `Command` (resolves groups, propagates settings, indexes
//!    args) on first parse, and skips that work when the `Built` flag is
//!    already set. Calling [`clap::Command::build`] once and cloning the
//!    finalized value carries the flag along, so each invocation pays only
//!    matching: −11% for `cat`, −20% for `ls`.
//!
//! The win scales with arg-surface size, so the large ports (`ls`, `stat`,
//! `od`) benefit most. `Command::clone` is *not* cheaper than
//! `<util>_command()` on its own (0.83 µs vs 1.1 µs, and the difference is
//! inside noise end-to-end) — the skipped `_build_self` is the entire point.
//!
//! `help_template` must be applied before `build()`, which is why the macro
//! owns both steps rather than leaving the template to callers.

/// GNU coreutils' help layout opens with the usage line; clap's default
/// template leads with the `about`. uutils handles this via uucore's
/// `localized_help_template`, which codegen drops because it pulls in Fluent
/// (see `knowledge/runtimes/coreutils-args-port.md`). Re-applied here so every
/// ported builtin renders GNU-equivalent help from one definition.
pub const GNU_HELP_TEMPLATE: &str = "Usage: {usage}\n{about}\n\n{all-args}\n";

/// Define a cached accessor for a generated `clap::Command`.
///
/// Expands to `fn $name() -> clap::Command` returning a clone of a
/// process-lifetime `Command` that has already had [`GNU_HELP_TEMPLATE`]
/// applied and [`clap::Command::build`] called on it.
///
/// ```ignore
/// cached_command!(cat_cmd, super::generated::cat_args::cat_command());
/// let matches = cat_cmd().try_get_matches_from(argv)?;
/// ```
macro_rules! cached_command {
    ($(#[$attr:meta])* $name:ident, $builder:expr) => {
        $(#[$attr])*
        fn $name() -> clap::Command {
            static CACHE: std::sync::LazyLock<clap::Command> = std::sync::LazyLock::new(|| {
                let mut cmd = $builder.help_template($crate::builtins::clap_cache::GNU_HELP_TEMPLATE);
                // Finalize once. Clones inherit the `Built` flag, so
                // `try_get_matches_from` skips `_build_self` per invocation.
                cmd.build();
                cmd
            });
            CACHE.clone()
        }
    };
}

pub(crate) use cached_command;

#[cfg(test)]
mod tests {
    use super::*;

    cached_command!(cat_cmd, crate::builtins::generated::cat_args::cat_command());

    /// A cached, pre-built command must parse identically to a freshly
    /// constructed one. Guards the `build()`-then-`clone()` shortcut against
    /// clap changing what `Built` implies.
    #[test]
    fn cached_command_parses_like_a_fresh_one() {
        let argv = ["cat", "-n", "-E", "/d/f"];
        let fresh = crate::builtins::generated::cat_args::cat_command()
            .help_template(GNU_HELP_TEMPLATE)
            .try_get_matches_from(argv)
            .expect("fresh parse");
        let cached = cat_cmd().try_get_matches_from(argv).expect("cached parse");

        assert_eq!(fresh.get_flag("number"), cached.get_flag("number"));
        assert_eq!(fresh.get_flag("show-ends"), cached.get_flag("show-ends"));
        assert_eq!(fresh.get_flag("show-tabs"), cached.get_flag("show-tabs"));
        let raw = |m: &clap::ArgMatches| {
            m.get_raw("file")
                .map(|v| v.map(|s| s.to_owned()).collect::<Vec<_>>())
        };
        assert_eq!(raw(&fresh), raw(&cached));
    }

    /// Repeated calls must not accumulate state — each clone parses from
    /// scratch, and a parse error on one call must not poison the next.
    #[test]
    fn cached_command_is_reusable_across_calls() {
        assert!(cat_cmd().try_get_matches_from(["cat", "--nope"]).is_err());
        let ok = cat_cmd()
            .try_get_matches_from(["cat", "-n", "/d/f"])
            .expect("parse after an earlier error");
        assert!(ok.get_flag("number"));
        // Same argv twice yields the same result (no residue on the cache).
        let again = cat_cmd()
            .try_get_matches_from(["cat", "-n", "/d/f"])
            .expect("second parse");
        assert!(again.get_flag("number"));
    }

    /// Help rendering must survive the pre-`build()` template application —
    /// the GNU layout leads with `Usage:`, clap's default does not.
    #[test]
    fn cached_command_renders_gnu_help_layout() {
        let err = cat_cmd()
            .try_get_matches_from(["cat", "--help"])
            .expect_err("--help exits via Err");
        let rendered = err.render().to_string();
        assert!(
            rendered.starts_with("Usage:"),
            "help must open with the usage line, got: {rendered}"
        );
    }
}
