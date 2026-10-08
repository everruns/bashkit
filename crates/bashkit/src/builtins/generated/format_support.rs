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

#[allow(non_camel_case_types)]
#[derive(Clone, Copy, Debug)]
pub enum QuotingStyle {
    C_NO_QUOTES,
    SHELL_ESCAPE,
}

pub fn locale_aware_escape_name(input: &OsStr, style: QuotingStyle) -> OsString {
    match style {
        QuotingStyle::C_NO_QUOTES => input.to_os_string(),
        QuotingStyle::SHELL_ESCAPE => shell_quote(&input.to_string_lossy()).into(),
    }
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
    // (`printf %q 'com espaço'` is `com\ espaço`).
    let safe = |c: char| {
        c.is_ascii_alphanumeric() || "_/.:-=+@,%^".contains(c) || (!c.is_ascii() && !c.is_control())
    };
    let needs_quoting = s.chars().any(|c| !safe(c));
    if !needs_quoting {
        return s.to_string();
    }

    let has_control = s.chars().any(char::is_control);
    if has_control {
        let mut out = String::from("$'");
        for ch in s.chars() {
            match ch {
                '\'' => out.push_str("\\'"),
                '\\' => out.push_str("\\\\"),
                '\n' => out.push_str("\\n"),
                '\t' => out.push_str("\\t"),
                '\r' => out.push_str("\\r"),
                c if (c as u32) < 32 || c as u32 == 127 => {
                    out.push_str(&format!("\\x{:02x}", c as u32));
                }
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
    for ch in s.chars() {
        if safe(ch) {
            out.push(ch);
        } else {
            out.push('\\');
            out.push(ch);
        }
    }
    out
}
