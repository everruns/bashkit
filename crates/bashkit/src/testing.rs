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

/// Full fuzz-invariant check. Combines [`assert_no_leak`] with the
/// host-canary check (TM-INF-013): the canary must not appear in
/// stdout or stderr.
///
/// Call this from cargo-fuzz targets and proptest cases — anywhere
/// random input runs through a builtin.
#[track_caller]
pub fn assert_fuzz_invariants(result: &ExecResult, ctx: &str, tool_banned: &[&str]) {
    assert_no_leak(result, ctx, tool_banned);
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
