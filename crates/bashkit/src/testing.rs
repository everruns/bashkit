//! Test-only helpers used by integration tests in `tests/*.rs` and the
//! cargo-fuzz targets in `fuzz/fuzz_targets/*.rs`.
//!
//! This module is `#[doc(hidden)]` because it isn't part of the supported
//! public API — it exists so external test/fuzz code can share the same
//! cross-tool invariants as the inline `#[cfg(test)]` modules without us
//! duplicating the banned-substring list and the canary plumbing.
//!
//! The invariants enforced here are documented in `specs/threat-model.md`:
//!  - **TM-INF-022** — no Rust Debug shapes in stderr
//!  - **TM-INF-016** — no host paths (`/rustc/`, `~/.cargo/registry/`,
//!    `target/debug/deps/`) in stderr
//!  - **TM-INF-013** — no host environment variables leak through
//!    builtins; verified via the canary mechanism: `fuzz_init()` sets a
//!    magic env var on the host process, and `assert_fuzz_invariants`
//!    asserts that magic value never appears in builtin stdout/stderr.

use crate::{Bash, ControlFlow, ExecResult};

/// Cross-tool banned substrings. Any of these in stderr means a leak —
/// either a Rust `Debug` formatter reached the agent (TM-INF-022) or a
/// host path/internal struct shape escaped sanitization (TM-INF-016).
/// Per-tool tests extend this with their own internals.
pub const UNIVERSAL_BANNED: &[&str] = &[
    // -- Debug shapes (TM-INF-022) --
    "File {",
    "path: ()",
    "Token(",
    "Tok::",
    "Undefined::",
    "Errors {",
    "Vec [",
    " { code:",
    "Some([",
    "Span {",
    "Range {",
    // -- Host paths (TM-INF-016) --
    // Rust compiler internals leaked via panic backtraces.
    "/rustc/",
    // Cargo build artifacts — should never appear in user-facing stderr.
    "/.cargo/registry/",
    "target/debug/deps/",
    "target/release/deps/",
    "/.rustup/toolchains/",
];

/// Cap on per-call stderr length. A single bad input must not flood
/// stderr beyond this — catches "one bad regex generates 10 MB of
/// library error spam" regressions.
pub const MAX_STDERR_BYTES: usize = 1024;

/// Magic value seeded into the host OS environment by `fuzz_init`. If
/// this string ever appears in builtin stdout or stderr, a builtin is
/// reading from `std::env::vars()` instead of the sandboxed `ctx.env`
/// (TM-INF-013 regression).
pub const FUZZ_HOST_CANARY: &str = "BASHKIT_FUZZ_HOST_CANARY_47a83bcf_DO_NOT_LEAK";

/// Idempotently seed the host OS environment with the canary value.
/// Must be called by every fuzz target before its first `Bash::exec`.
///
/// Uses `std::sync::Once` so the unsafe `set_var` runs exactly once,
/// before any worker threads spawn — sound under the Rust 2024 rules
/// that made `set_var` `unsafe`.
pub fn fuzz_init() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        // SAFETY: called exactly once at process start, before any
        // threads that read environment variables are spawned.
        unsafe {
            std::env::set_var("BASHKIT_FUZZ_HOST_SECRET", FUZZ_HOST_CANARY);
        }
    });
}

/// Execute a script under a fresh `Bash` and capture the result.
/// Both `Ok(ExecResult)` and the hard `Err(execution error)` paths are
/// normalized into `ExecResult` so callers don't have to branch.
pub async fn run(script: &str) -> ExecResult {
    let mut bash = Bash::new();
    bash.exec(script).await.unwrap_or_else(|e| ExecResult {
        stdout: String::new(),
        stderr: e.to_string(),
        exit_code: 1,
        control_flow: ControlFlow::None,
        ..Default::default()
    })
}

/// Run `script` on the caller-provided `Bash` instance and assert all
/// fuzz invariants. One-line replacement for the
/// `let _ = bash.exec(&script).await;` pattern in cargo-fuzz targets.
///
/// IMPORTANT: callers must NOT redirect stderr to `/dev/null` in the
/// script (`... 2>/dev/null`) — that throws away exactly what we want
/// to inspect.
pub async fn fuzz_exec(bash: &mut Bash, script: &str, ctx: &str, tool_banned: &[&str]) {
    let result = bash.exec(script).await.unwrap_or_else(|e| ExecResult {
        stdout: String::new(),
        stderr: e.to_string(),
        exit_code: 1,
        control_flow: ControlFlow::None,
        ..Default::default()
    });
    assert_fuzz_invariants(&result, ctx, tool_banned);
}

/// Assert that stderr is short and contains no banned Debug-shape or
/// host-path substring. Per-tool callers pass their own additional
/// banned list (env var names, prepended-source markers, etc.).
#[track_caller]
pub fn assert_no_leak(result: &ExecResult, ctx: &str, tool_banned: &[&str]) {
    let stderr = &result.stderr;
    assert!(
        stderr.len() <= MAX_STDERR_BYTES,
        "[{ctx}] stderr exceeds {MAX_STDERR_BYTES} bytes ({} bytes):\n---\n{stderr}\n---",
        stderr.len()
    );
    for pat in UNIVERSAL_BANNED.iter().chain(tool_banned.iter()) {
        assert!(
            !stderr.contains(pat),
            "[{ctx}] stderr leaks banned shape `{pat}`:\n---\n{stderr}\n---"
        );
    }
}

/// Lines fuzz/proptest targets inline arbitrary input bytes into shell
/// scripts, so bash and ls produce error messages that quote the input
/// verbatim — `bash: <cmd>: command not found`, `bash: <path>: No such
/// file or directory`, `ls: cannot access '<path>': …`. These are real
/// shell echoes of user input, not internal Debug leaks; if they happen
/// to contain a banned substring (e.g. user input `Tok"` becomes the
/// command name `Tok:`, which bash's `bash: %s: command not found`
/// formatter renders as `bash: Tok:: command not found`, accidentally
/// matching the parser-token shape `Tok::`), the leak detector must not
/// trip. This filter strips lines that match a recognized real-shell
/// error template before the banned-shape check; the byte-length cap
/// and the host-canary check still run on the unfiltered stderr so
/// flood and TM-INF-013 regressions are still caught.
fn strip_real_shell_error_lines(stderr: &str) -> String {
    let lines: Vec<&str> = stderr
        .lines()
        .filter(|line| !is_real_shell_error_line(line))
        .collect();
    lines.join("\n")
}

/// Recognize stderr lines that bash or ls produces verbatim from user
/// input. Conservative: only strips if the prefix is `bash: ` or `ls: `
/// AND the line ends with a known real-shell error suffix.
fn is_real_shell_error_line(line: &str) -> bool {
    const SHELL_ERROR_SUFFIXES: &[&str] = &[
        ": command not found",
        ": No such file or directory",
        ": Is a directory",
        ": Permission denied",
        ": cannot execute: required file not found",
        ": cannot execute binary file",
    ];
    if let Some(rest) = line.strip_prefix("bash: ") {
        if SHELL_ERROR_SUFFIXES.iter().any(|suf| rest.ends_with(suf)) {
            return true;
        }
        // Did-you-mean variant: `bash: <cmd>: command not found. Did you mean: ., :, [?`
        if rest.ends_with(". Did you mean: ., :, [?") {
            return true;
        }
        return false;
    }
    if let Some(rest) = line.strip_prefix("ls: ") {
        if rest.starts_with("cannot access ")
            && (rest.ends_with(": No such file or directory")
                || rest.ends_with(": Is a directory")
                || rest.ends_with(": Permission denied"))
        {
            return true;
        }
        return false;
    }
    false
}

/// Full fuzz-invariant check. Like [`assert_no_leak`] but tolerates
/// real-shell-style error lines (which echo user input verbatim) and
/// adds the host-canary check (TM-INF-013): the canary must not appear
/// in stdout or stderr.
///
/// Call this from cargo-fuzz targets and proptest cases — anywhere
/// random input runs through a builtin.
#[track_caller]
pub fn assert_fuzz_invariants(result: &ExecResult, ctx: &str, tool_banned: &[&str]) {
    let stderr = &result.stderr;
    assert!(
        stderr.len() <= MAX_STDERR_BYTES,
        "[{ctx}] stderr exceeds {MAX_STDERR_BYTES} bytes ({} bytes):\n---\n{stderr}\n---",
        stderr.len()
    );
    let stripped = strip_real_shell_error_lines(stderr);
    for pat in UNIVERSAL_BANNED.iter().chain(tool_banned.iter()) {
        assert!(
            !stripped.contains(pat),
            "[{ctx}] stderr leaks banned shape `{pat}` (after stripping shell echoes):\n\
             ---raw stderr---\n{stderr}\n---stripped---\n{stripped}\n---"
        );
    }
    assert!(
        !result.stdout.contains(FUZZ_HOST_CANARY),
        "[{ctx}] FUZZ canary leaked into stdout (TM-INF-013 regression — \
         a builtin is reading host env). stdout:\n---\n{}\n---",
        truncate(&result.stdout, 512)
    );
    assert!(
        !result.stderr.contains(FUZZ_HOST_CANARY),
        "[{ctx}] FUZZ canary leaked into stderr (TM-INF-013 regression — \
         a builtin is reading host env). stderr:\n---\n{}\n---",
        truncate(&result.stderr, 512)
    );
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}...<truncated>", &s[..max.min(s.len())])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_keeps_unrelated_lines() {
        let s = "warning: something\nthread panicked at lib.rs:1\n";
        assert_eq!(
            strip_real_shell_error_lines(s),
            "warning: something\nthread panicked at lib.rs:1"
        );
    }

    #[test]
    fn strip_removes_command_not_found() {
        // From a real glob_fuzz failure — the input ended with `Tok"`,
        // bash formatted `bash: <cmd>: command not found`, and the
        // separator `:` after `Tok` formed the banned `Tok::` substring.
        let s = "bash: Tok:: command not found\n";
        assert_eq!(strip_real_shell_error_lines(s), "");
    }

    #[test]
    fn strip_removes_no_such_file() {
        // From a real arithmetic_fuzz failure — input contained
        // `/.rustup/toolchains/` literally, bash echoed it back.
        let s = "bash: /.rustup/toolchains/gww: No such file or directory\n";
        assert_eq!(strip_real_shell_error_lines(s), "");
    }

    #[test]
    fn strip_removes_did_you_mean_variant() {
        let s = "bash: : command not found. Did you mean: ., :, [?\n";
        assert_eq!(strip_real_shell_error_lines(s), "");
    }

    #[test]
    fn strip_removes_ls_cannot_access() {
        // From #1621 — input contained `Span {`, ls echoed it back.
        let s = "ls: cannot access '/tmp/==(Span {(;': No such file or directory\n";
        assert_eq!(strip_real_shell_error_lines(s), "");
    }

    #[test]
    fn strip_keeps_internal_panic_lines() {
        // A real internal Debug leak that doesn't match the shell
        // template must NOT be stripped — otherwise the leak detector
        // would silently pass real regressions.
        let s = "thread 'fuzz' panicked at parse.rs:42:\nFile { code: \"oops\", path: () }\n";
        let stripped = strip_real_shell_error_lines(s);
        assert!(stripped.contains("File {"), "stripped: {stripped:?}");
        assert!(stripped.contains("path: ()"), "stripped: {stripped:?}");
    }

    #[test]
    fn strip_keeps_partial_matches() {
        // Lines that look like shell errors but don't match the exact
        // template must remain — defense in depth against accidentally
        // masking real leaks.
        let s = "bash: something weird Span { not at end\n\
                 some-other-tool: Tok:: blah\n";
        let stripped = strip_real_shell_error_lines(s);
        assert!(stripped.contains("Span {"));
        assert!(stripped.contains("Tok::"));
    }

    #[test]
    fn strip_handles_multiline_mixed() {
        let s = "bash: foo: command not found\n\
                 bash: /tmp/Span {bar: No such file or directory\n\
                 thread panicked at runtime.rs:1\n\
                 ls: cannot access 'baz': No such file or directory\n";
        let stripped = strip_real_shell_error_lines(s);
        assert!(!stripped.contains("command not found"));
        assert!(!stripped.contains("/tmp/Span {"));
        assert!(!stripped.contains("cannot access"));
        assert!(stripped.contains("thread panicked"));
    }
}
