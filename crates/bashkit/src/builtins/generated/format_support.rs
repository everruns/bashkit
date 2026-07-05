//! Handwritten support shims for the vendored uucore `format` module.
//!
//! Keep uucore runtime hooks local and side-effect free: bashkit builtins
//! return structured `ExecResult`s, so generated formatting code must not
//! write diagnostics directly to host stderr or depend on uucore exit state.

use std::ffi::{OsStr, OsString};

#[cfg(unix)]
use std::os::unix::ffi::OsStrExt;
#[cfg(target_os = "wasi")]
use std::os::wasi::ffi::OsStrExt;

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
    #[cfg(any(unix, target_os = "wasi"))]
    return Ok(os_string.as_bytes());

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

macro_rules! show_error {
    ($($arg:tt)*) => {{
        let _ = format_args!($($arg)*);
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

    let needs_quoting = s
        .chars()
        .any(|c| !c.is_ascii_alphanumeric() && !"_/.:-=+@,%^".contains(c));
    if !needs_quoting {
        return s.to_string();
    }

    let has_control = s.chars().any(|c| (c as u32) < 32 || c as u32 == 127);
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
                c => out.push(c),
            }
        }
        out.push('\'');
        return out;
    }

    let mut out = String::new();
    for ch in s.chars() {
        if ch.is_ascii_alphanumeric() || "_/.:-=+@,%^".contains(ch) {
            out.push(ch);
        } else {
            out.push('\\');
            out.push(ch);
        }
    }
    out
}
