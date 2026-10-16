//! Handwritten support shims for the vendored uucore `format` module.
//!
//! Keep uucore runtime hooks local and side-effect free: bashkit builtins
//! return structured `ExecResult`s, so generated formatting code must not
//! write diagnostics directly to host stderr or depend on uucore exit state.
//! `show_error!` messages are collected per call ([`collect_diagnostics`])
//! so `printf` can report bad numeric arguments the way bash does.

use std::ffi::{OsStr, OsString};

#[cfg(unix)]
use std::os::unix::ffi::OsStrExt;

#[derive(Debug)]
pub struct NonUtf8OsStrError {
    input_lossy_string: String,
}

impl NonUtf8OsStrError {
    #[cfg(test)]
    pub(crate) fn new_for_test(input_lossy_string: impl Into<String>) -> Self {
        Self {
            input_lossy_string: input_lossy_string.into(),
        }
    }
}

impl std::fmt::Display for NonUtf8OsStrError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use os_display::Quotable;
        let quoted = self.input_lossy_string.quote();
        write!(f, "invalid UTF-8 input {quoted}")
    }
}

impl std::error::Error for NonUtf8OsStrError {}

pub trait UError: std::error::Error {}

#[cfg_attr(any(unix, target_os = "wasi"), expect(clippy::unnecessary_wraps))]
pub fn os_str_as_bytes(os_string: &OsStr) -> Result<&[u8], NonUtf8OsStrError> {
    #[cfg(unix)]
    return Ok(os_string.as_bytes());

    #[cfg(target_os = "wasi")]
    return Ok(os_string.as_encoded_bytes());

    #[cfg(not(any(unix, target_os = "wasi")))]
    os_string
        .to_str()
        .ok_or_else(|| NonUtf8OsStrError {
            input_lossy_string: os_string.to_string_lossy().into_owned(),
        })
        .map(str::as_bytes)
}

#[allow(clippy::needless_pass_by_value)]
pub fn set_exit_code(_code: i32) {}

/// Render an [`std::io::Error`] the way GNU coreutils does: message only,
/// without Rust's ` (os error N)` suffix.
///
/// Mirrors `uucore::error::strip_errno`. Reimplemented here rather than
/// substituted onto some bashkit error type because the vendored
/// `format` module calls it purely to build a `Display` string, and
/// pulling in uucore's error module would drag its whole runtime.
pub fn strip_errno(err: &std::io::Error) -> String {
    let mut msg = err.to_string();
    if let Some(pos) = msg.find(" (os error ") {
        msg.truncate(pos);
    }
    msg
}

/// Stand-in for `uucore::quoting_style::QuotingStyle`, carrying only the
/// styles the vendored `format` module actually names.
///
/// Upstream renamed the `C_NO_QUOTES` associated constant to an `Escape`
/// variant; both map to uucore's `CQuoter` with `CQuotesEnforce::None` and
/// both are what `--quoting-style=escape` selects, so the rename carries no
/// behaviour change. Both spellings live here during the window where the
/// pinned `generated/format/` still says `C_NO_QUOTES` while the drift run
/// regenerates it against an upstream HEAD that says `Escape` — a shim that
/// only knew one of them would fail the drift build. Drop `C_NO_QUOTES` once
/// the pin has moved past the rename.
#[allow(non_camel_case_types)]
#[derive(Clone, Copy, Debug)]
pub enum QuotingStyle {
    /// Upstream's pre-rename `QuotingStyle::C_NO_QUOTES`.
    C_NO_QUOTES,
    /// Upstream's post-rename `QuotingStyle::Escape`. Same escaping.
    Escape,
    SHELL_ESCAPE,
}

/// Style the vendored `format` module quotes a rejected numeric argument
/// with before reporting it.
///
/// `printf` recovers the raw argument by re-quoting each candidate the same
/// way and prefix-matching the diagnostic
/// ([`crate::builtins::printf`]`::numeric_argument_diagnostics`), so the two
/// sides must agree. Naming the style once keeps them in step across an
/// upstream rename: the drift PR may only touch generated files, so it
/// cannot update `printf.rs` itself.
pub const DIAGNOSTIC_QUOTING_STYLE: QuotingStyle = QuotingStyle::Escape;

pub fn locale_aware_escape_name(input: &OsStr, style: QuotingStyle) -> OsString {
    match style {
        // No C escaping: the escaped value is only ever a matching key and
        // the text bashkit discards, never user-visible output (printf
        // reports the raw argument, as bash does).
        QuotingStyle::C_NO_QUOTES | QuotingStyle::Escape => input.to_os_string(),
        QuotingStyle::SHELL_ESCAPE => shell_quote(&input.to_string_lossy()).into(),
    }
}

/// Mirrors `uucore::quoting_style::locale_aware_shell_escape`, the shorthand
/// upstream extracted for `printf %q`. Shell-escapes and returns `String`.
pub fn locale_aware_shell_escape(input: impl AsRef<OsStr>) -> String {
    locale_aware_escape_name(input.as_ref(), QuotingStyle::SHELL_ESCAPE)
        .into_string()
        .unwrap_or_else(|os| os.to_string_lossy().into_owned())
}

/// Most diagnostics one [`collect_diagnostics`] call keeps (one per bad
/// argument; the cap bounds memory for huge argument lists).
const MAX_COLLECTED_DIAGNOSTICS: usize = 64;

std::thread_local! {
    /// `Some` while [`collect_diagnostics`] runs: uucore's `show_error!`
    /// messages land here instead of host stderr.
    static DIAGNOSTICS: std::cell::RefCell<Option<Vec<String>>> =
        const { std::cell::RefCell::new(None) };
}

/// Run `f` (synchronous, so the thread-local cannot leak across tasks) and
/// return the `show_error!` messages it produced, in order.
pub fn collect_diagnostics<R>(f: impl FnOnce() -> R) -> (R, Vec<String>) {
    DIAGNOSTICS.with(|d| *d.borrow_mut() = Some(Vec::new()));
    let result = f();
    let diags = DIAGNOSTICS
        .with(|d| d.borrow_mut().take())
        .unwrap_or_default();
    (result, diags)
}

/// Record one `show_error!` message when a collector is active.
pub fn record_diagnostic(msg: std::fmt::Arguments<'_>) {
    DIAGNOSTICS.with(|d| {
        if let Some(diags) = d.borrow_mut().as_mut()
            && diags.len() < MAX_COLLECTED_DIAGNOSTICS
        {
            diags.push(msg.to_string());
        }
    });
}

macro_rules! show_error {
    ($($arg:tt)*) => {{
        $crate::builtins::generated::format_support::record_diagnostic(format_args!($($arg)*));
    }};
}

macro_rules! show_warning {
    ($($arg:tt)*) => {{
        let _ = format_args!($($arg)*);
    }};
}

pub(crate) use show_error;
pub(crate) use show_warning;

fn shell_quote(s: &str) -> String {
    if s.is_empty() {
        return "''".to_string();
    }

    // Under UTF-8, bash leaves printable non-ASCII characters as they are
    // (`printf %q 'com espaço'` is `com\ espaço`). Observed bash rules:
    // shell metacharacters (and `,`, `^` for brace/history expansion) are
    // always escaped; `#` only starts a comment, so only a leading one is;
    // `~` only expands at the start or after `=`/`:`, so only there.
    let needs_escape = |prev: Option<char>, c: char| match c {
        '#' => prev.is_none(),
        '~' => matches!(prev, None | Some('=' | ':')),
        c if c.is_ascii() => !(c.is_ascii_alphanumeric() || "_/.:-=+@%".contains(c)),
        c => c.is_control(),
    };
    let escapes = || {
        let mut prev = None;
        s.chars().map(move |c| {
            let e = needs_escape(prev, c);
            prev = Some(c);
            (c, e)
        })
    };
    if !escapes().any(|(_, e)| e) {
        return s.to_string();
    }

    let has_control = s.chars().any(char::is_control);
    if has_control {
        let mut out = String::from("$'");
        for ch in s.chars() {
            match ch {
                '\'' => out.push_str("\\'"),
                '\\' => out.push_str("\\\\"),
                // bash `ansic_quote`: named escapes, octal for the rest.
                '\x07' => out.push_str("\\a"),
                '\x08' => out.push_str("\\b"),
                '\x1b' => out.push_str("\\E"),
                '\x0c' => out.push_str("\\f"),
                '\n' => out.push_str("\\n"),
                '\t' => out.push_str("\\t"),
                '\r' => out.push_str("\\r"),
                '\x0b' => out.push_str("\\v"),
                c if c.is_control() => {
                    let mut buf = [0u8; 4];
                    for b in c.encode_utf8(&mut buf).bytes() {
                        out.push_str(&format!("\\{b:03o}"));
                    }
                }
                c => out.push(c),
            }
        }
        out.push('\'');
        return out;
    }

    let mut out = String::new();
    for (ch, escape) in escapes() {
        if escape {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Upstream renamed `C_NO_QUOTES` to the `Escape` variant; both select
    /// uucore's `CQuoter` with `CQuotesEnforce::None`, and both are what
    /// `--quoting-style=escape` resolves to. Pin the equivalence so the two
    /// spellings cannot drift apart while both are live.
    #[test]
    fn escape_and_c_no_quotes_escape_identically() {
        for input in ["", "abc", "a\tb", "a b", "quote'd", "\u{1b}[0m"] {
            let os = OsStr::new(input);
            assert_eq!(
                locale_aware_escape_name(os, QuotingStyle::C_NO_QUOTES),
                locale_aware_escape_name(os, QuotingStyle::Escape),
                "styles disagree on {input:?}" // debug-ok: assert-failure message
            );
        }
    }

    /// `locale_aware_shell_escape` is upstream's shorthand, not a second
    /// implementation: it must agree with the style it delegates to.
    #[test]
    fn shell_escape_shorthand_matches_escape_name() {
        for input in ["", "plain", "hello world", "a'b", "tab\there"] {
            assert_eq!(
                locale_aware_shell_escape(input),
                locale_aware_escape_name(OsStr::new(input), QuotingStyle::SHELL_ESCAPE)
                    .to_string_lossy(),
                "shorthand diverged on {input:?}" // debug-ok: assert-failure message
            );
        }
    }

    /// `printf` reverse-matches the vendored module's numeric diagnostics by
    /// re-quoting candidates with [`DIAGNOSTIC_QUOTING_STYLE`], so the style
    /// the generated code names must escape the same way. A drift PR may
    /// only touch generated files, so this scan is what makes an upstream
    /// switch to some other style fail loudly instead of silently dropping
    /// `printf: <arg>: invalid number`.
    #[test]
    fn generated_module_quotes_diagnostics_with_the_style_printf_expects() {
        const ARGUMENT_RS: &str = include_str!("format/argument.rs");

        let probe = OsStr::new("a\tb c'd");
        let expected = locale_aware_escape_name(probe, DIAGNOSTIC_QUOTING_STYLE);

        let named: Vec<&str> = ARGUMENT_RS
            .match_indices("QuotingStyle::")
            .map(|(i, _)| {
                let rest = &ARGUMENT_RS[i + "QuotingStyle::".len()..];
                let end = rest
                    .find(|c: char| !c.is_ascii_alphanumeric() && c != '_')
                    .unwrap_or(rest.len());
                &rest[..end]
            })
            .collect();
        assert!(
            !named.is_empty(),
            "generated/format/argument.rs no longer quotes its diagnostics; \
             printf's prefix match cannot work"
        );
        for name in named {
            let style = match name {
                "C_NO_QUOTES" => QuotingStyle::C_NO_QUOTES,
                "Escape" => QuotingStyle::Escape,
                "SHELL_ESCAPE" => QuotingStyle::SHELL_ESCAPE,
                other => panic!("generated code names unknown QuotingStyle::{other}"),
            };
            assert_eq!(
                locale_aware_escape_name(probe, style),
                expected,
                "generated code quotes with QuotingStyle::{name}, which escapes \
                 differently from DIAGNOSTIC_QUOTING_STYLE; update printf.rs"
            );
        }
    }
}
