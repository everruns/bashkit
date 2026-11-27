//! Test-only helpers used by integration tests in `tests/*.rs` and the
//! cargo-fuzz targets in `fuzz/fuzz_targets/*.rs`.
//!
//! This module is `#[doc(hidden)]` because it isn't part of the supported
//! public API — it exists so external test/fuzz code can share the same
//! cross-tool invariants as the inline `#[cfg(test)]` modules without us
//! duplicating the banned-substring list and the canary plumbing.
//!
//! The invariants enforced here are documented in `knowledge/security/threat-model.md`:
//!  - **TM-INF-022** — no Rust Debug shapes in stderr
//!  - **TM-INF-016** — no host paths (`/rustc/`, `~/.cargo/registry/`,
//!    `target/debug/deps/`) in stderr
//!  - **TM-INF-013** — no host environment variables leak through
//!    builtins; verified via the canary mechanism: `fuzz_init()` sets a
//!    magic env var on the host process, and `assert_fuzz_invariants`
//!    asserts that magic value never appears in builtin stdout/stderr
//!    unless the executed script itself carries it (a coverage-guided
//!    fuzzer can forge the canary into its own input — see
//!    `assert_fuzz_invariants`).

mod checked_fs;
mod counting_fs;

pub use checked_fs::CheckedFs;
pub use counting_fs::{CallCounts, CountingFs};

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
    // -- Caught panics (TM-INT-001, TM-UNI-002) --
    // The interpreter catches builtin panics so the host stays up, but a
    // panic reachable from untrusted script text is still a bug; without
    // these the fuzz targets pass on every caught panic. Issue #2427.
    "internal error:",
    "builtin failed unexpectedly",
    // -- Host paths (TM-INF-016) --
    // Rust compiler internals leaked via panic backtraces.
    "/rustc/",
    // Cargo build artifacts — should never appear in user-facing stderr.
    "/.cargo/registry/",
    "target/debug/deps/",
    "target/release/deps/",
    "/.rustup/toolchains/",
];

/// Cap on per-call stderr length for the **strict** per-builtin path
/// ([`assert_no_leak`]). A hand-written `no_leak_*` case controls its own
/// input, so it has no reason to produce kilobytes of diagnostics: this stays
/// tight and catches "one bad regex generates 10 MB of library error spam"
/// regressions.
///
/// Do not confuse this with the bound on a *single* diagnostic. One
/// diagnostic staying small is a separate product invariant (L-ARITH-002,
/// `MAX_ARITHMETIC_DIAG_ECHO`) covered by
/// `tests/integration/diagnostic_prefix_tests.rs`.
pub const MAX_STDERR_BYTES: usize = 1024;

/// Cap on whole-execution stderr length for the **fuzz** path
/// ([`assert_fuzz_invariants`]), which is looser than [`MAX_STDERR_BYTES`] on
/// purpose.
///
/// A fuzz target inlines bytes it does not choose, and one that builds
/// `ExecutionLimits::max_stderr_bytes(N)` is declaring N bytes acceptable for
/// its scripts. The engine truncates only at that limit, so a harness cap
/// below N rejects legitimately chatty-but-bounded output instead of catching
/// anything. Sharing the strict 1024 figure did exactly that: Fuzz Testing run
/// 45932089002 failed `printf_fuzz`, whose `printf '%d %o %x'` cycles its
/// format over every operand and so emits one `invalid number` diagnostic per
/// operand — 34 non-numeric operands rendered 1,283 bytes, inside the 4096 the
/// target itself configures. Any script landing in 1024..=4096 failed by
/// construction, so the cap was unsatisfiable rather than strict. At the
/// largest configured ceiling the assertion says "the engine honored the
/// stderr limit it was given", which is the regression it was written for.
///
/// # Residual: targets that configure nothing
///
/// Fifteen of the 26 fuzz targets configure 4096 and three configure 1024; the
/// remaining eight take the 1 MiB default, which is 256x this cap. For those
/// the harness — not a declared budget — is the operative bound, so this is a
/// deliberate flood limit for them rather than a satisfiable-by-construction
/// one, and the same false positive can recur there: a legitimate >4 KiB
/// stderr the engine would permit still trips it. That is a narrow risk today
/// (none of the eight drives a per-operand diagnostic loop the way `printf`
/// does), and the fix when it happens is to give that target an explicit
/// `max_stderr_bytes` declaring its real budget, not to raise this constant
/// blindly.
pub const MAX_FUZZ_STDERR_BYTES: usize = 4096;

/// The largest `ExecutionLimits::max_stderr_bytes` any fuzz target explicitly
/// builds. Lowering [`MAX_FUZZ_STDERR_BYTES`] below it would make the
/// assertion unsatisfiable for those targets again, so the mismatch is a
/// compile error rather than a nightly-only failure. Raise this if a target
/// ever configures a larger budget.
const FUZZ_TARGET_STDERR_LIMIT: usize = 4096;
const _: () = assert!(
    MAX_FUZZ_STDERR_BYTES >= FUZZ_TARGET_STDERR_LIMIT,
    "MAX_FUZZ_STDERR_BYTES is tighter than the max_stderr_bytes the fuzz targets configure"
);

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

/// A throwaway `LLVM_PROFILE_FILE` path for a spawned child, outside the
/// directory `cargo tarpaulin` merges.
///
/// Any test that spawns the instrumented `bashkit` binary must set this on
/// the child. A child inherits the *parent's* profile-path template, so it
/// writes a profraw named after the parent test binary into
/// `target/tarpaulin/profraws/`, which tarpaulin then merges. Supervised
/// children are killed by design, and a SIGKILLed process never flushes its
/// profraw, so the merge step panicked inside `llvm_profparser`
/// (`unreachable!("llvm_profparsers works on complete data")`) and failed the
/// Coverage job at random — main runs 4588 and 4600, two days apart on
/// unrelated commits. Redirecting the child keeps truncated writes out of the
/// merge set, and stops the child's counters being misattributed to the
/// parent binary.
///
/// One directory per process, a unique file per call, removal left to the OS.
/// Deliberately not a cleanup-guarded temp dir: a killed child's foreign
/// descendant can still be writing here, so a per-call remove could fail for
/// reasons that have nothing to do with the test using it.
///
/// A test that needs to *assert* on the child's profraw (as a bounded-artifact
/// receipt, say) should set `LLVM_PROFILE_FILE` to its own path instead; the
/// point is only that the child never writes into tarpaulin's merge set.
// Host temp dir, not a VFS path: the host separator is correct here.
#[allow(clippy::disallowed_methods)]
pub fn child_profile_path() -> std::path::PathBuf {
    use std::sync::OnceLock;
    use std::sync::atomic::{AtomicU64, Ordering};
    static DIR: OnceLock<std::path::PathBuf> = OnceLock::new();
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let dir = DIR.get_or_init(|| {
        let dir =
            std::env::temp_dir().join(format!("bashkit-child-profraws-{}", std::process::id()));
        // Best-effort: if the directory cannot be created the child's write
        // simply fails, which is the outcome this redirect wants anyway.
        let _ = std::fs::create_dir_all(&dir);
        dir
    });
    dir.join(format!(
        "child-{}.profraw",
        NEXT.fetch_add(1, Ordering::Relaxed)
    ))
}

/// Execute a script under a fresh `Bash` and capture the result.
/// Both `Ok(ExecResult)` and the hard `Err(execution error)` paths are
/// normalized into `ExecResult` so callers don't have to branch.
pub async fn run(script: &str) -> ExecResult {
    let mut bash = Bash::new();
    bash.exec(script).await.unwrap_or_else(|e| ExecResult {
        stdout: crate::StreamData::new(),
        stderr: e.to_string().into(),
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
        stdout: crate::StreamData::new(),
        stderr: e.to_string().into(),
        exit_code: 1,
        control_flow: ControlFlow::None,
        ..Default::default()
    });
    assert_fuzz_invariants(&result, script, ctx, tool_banned);
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
///
/// The user-input slot can itself contain newlines, in which case a
/// one-line template renders across several lines — `glob_fuzz` run 219
/// produced `bash: { code:<(])\n: command not found` from a command name
/// ending in `\n`. So matching is span-based, not line-based: a `bash: `
/// or `ls: ` line that does not close on its own extends to the *first*
/// later line ending in a template suffix. Closing on the earliest
/// candidate keeps the span minimal, so a leak printed after a complete
/// diagnostic still survives; and a real leak cannot be swallowed
/// *inside* a span, because each shell diagnostic is formatted and
/// written as one string with nothing interleaved.
fn strip_real_shell_error_lines(stderr: &str) -> String {
    let lines: Vec<&str> = stderr.lines().collect();
    let mut kept: Vec<&str> = Vec::with_capacity(lines.len());
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        if is_real_shell_error_line(line) {
            i += 1;
            continue;
        }
        if starts_shell_error_template(line)
            && let Some(close) = lines[i + 1..]
                .iter()
                .position(|l| ends_shell_error_template(l))
        {
            i += close + 2;
            continue;
        }
        kept.push(line);
        i += 1;
    }
    kept.join("\n")
}

/// A stderr line that opens a real-shell diagnostic but does not close it,
/// so the user-input slot must run on into the following lines.
fn starts_shell_error_template(line: &str) -> bool {
    line.starts_with("bash: ") || line.starts_with("ls: cannot access ")
}

/// A stderr line that closes a real-shell diagnostic opened earlier.
fn ends_shell_error_template(line: &str) -> bool {
    SHELL_ERROR_SUFFIXES.iter().any(|suf| line.ends_with(suf))
}

/// True when the shell would echo `input` back into a diagnostic that
/// trips the leak detector — either a [`UNIVERSAL_BANNED`] shape or the
/// [`FUZZ_HOST_CANARY`].
///
/// Fuzz targets that inline raw bytes into scripts use this to skip such
/// inputs, keeping the leak detector strict for real internals. Checking
/// the raw bytes is not enough: the shell drops NUL bytes during word
/// expansion, so `\0{ code:` reaches stderr as ` { code:` and `</r\0ustc/`
/// as `/rustc/`. That transformation is what defeated `glob_fuzz`'s
/// original pre-filter in runs 218 and 219.
///
/// The canary belongs in this list because the detector hands libFuzzer
/// the bytes to forge it: `assert_fuzz_invariants` compares output
/// against [`FUZZ_HOST_CANARY`], that comparison is instrumented, and in
/// run 272 libFuzzer recovered a 32-byte `PersAutoDict` entry for it and
/// spliced the whole canary into an `arithmetic_fuzz` expression.
pub fn input_echo_would_trip(input: &str) -> bool {
    // Matching the raw bytes alone is not enough either: a diagnostic wraps
    // the slot in its own punctuation, so a banned shape can be spelled
    // across the join without appearing in the input at all. See
    // `DIAGNOSTIC_ECHO_PAD`.
    let (lead, trail) = DIAGNOSTIC_ECHO_PAD;
    // Mirror `diag_echo(expr.trim())`: the slot holds the trimmed text, so
    // trailing whitespace must not hide a join the render will still make.
    let rendered = format!("{lead}{}{trail}", input.trim());
    UNIVERSAL_BANNED
        .iter()
        .chain(std::iter::once(&FUZZ_HOST_CANARY))
        .any(|pat| {
            carries_through_expansion(input, pat) || carries_through_expansion(&rendered, pat)
        })
}

/// Punctuation a real-shell diagnostic places immediately around an echoed
/// user-input slot, used to render the slot before matching it.
///
/// `Interpreter::arith_diag` renders
/// `bash: line N: {diag_echo(expr.trim())}: {msg}`, so the expression slot
/// carries `: ` on its left from the line prefix and `: ` on its right as
/// the message separator. A banned shape can therefore form across either
/// join while the input itself is clean: nightly fuzz run 295 submitted
/// `wZTok:`, whose diagnostic reads `bash: line 1: wZTok:: syntax error in
/// expression (error token is ":")` and spells the parser-token shape
/// `Tok::` out of the input's trailing colon plus the separator's. The
/// tokenizer's `Tok` enum is never formatted, and `Tok::` appears nowhere
/// in the script, so this is an echo and not a TM-INF-022 leak.
///
/// Padding both sides at once is the real rendering rather than a widening:
/// both characters come from the same template around the same slot. The
/// "error token" slot is rendered inside double quotes and no banned shape
/// contains `"`, so it needs no padding of its own.
const DIAGNOSTIC_ECHO_PAD: (&str, &str) = (": ", ": ");

/// Characters a parser between script text and output *deletes* from the data
/// it carries, so a needle can reach the output intact while the script spells
/// it only with one of these spliced in. See [`carries_through_expansion`].
///
///  - `\0` — shell word expansion drops NUL, turning `</r\0ustc/` into
///    `/rustc/` and `\0{ code:` into ` { code:`.
///  - `"`, `'`, `\` — shell quote removal drops them, and the `csv` builtin's
///    field parser drops `"` as its quote character.
const DROPPED_BY_PARSERS: &[char] = &['\0', '"', '\'', '\\'];

/// True when `text` carries `needle` into the output — literally, or after a
/// parser on the way drops one of [`DROPPED_BY_PARSERS`] out of it.
///
/// Comparing the raw bytes alone is what defeated `glob_fuzz`'s original
/// pre-filter in runs 218 and 219, which is why NUL removal was folded in. The
/// quote characters joined it for the same reason one step further down the
/// pipeline: Fuzz Testing run 45932089002 reported `csv_fuzz` leaking the host
/// canary from the input `BASHKIT_FUZZ_HOST_CANARY_47a83bcf_D"O_NOT_LEAK`.
/// `echo` passed that through verbatim, then `csv headers` removed the `"` as
/// a CSV quote and printed the canary whole, so the output carried a token the
/// script text never spelled. libFuzzer harvests the canary from this very
/// comparison (see [`input_echo_would_trip`]), so splicing a quote into it is
/// a forgery the detector armed, not a host-env read.
///
/// The `contains` guard keeps the allocation off the common path: this runs
/// once per pattern per fuzz iteration.
///
/// # This over-approximates, deliberately
///
/// The strip is unconditional and context-free, while a real parser drops
/// these characters only in particular positions: a `"` inside single quotes
/// is data the shell keeps, `\` is removed only in escape position, and the
/// csv quote rules are not the shell's. So a match can be added for a splice
/// no parser would actually have undone. Combined with attribution being
/// whole-script and whole-stderr, that means a fragment spliced in one part of
/// the script can excuse a genuine occurrence elsewhere in the same execution.
///
/// This is the same residual false negative [`script_explains`] documents, and
/// it is accepted for the same reasons: the miss is per-input rather than
/// per-bug, so the fuzzer reaches the same formatter through inputs that do
/// not carry the splice, and the static source scan plus the per-tool
/// [`assert_no_leak`] cases stay absolute. Erring toward over-approximation is
/// the right direction here — under-approximating turns the nightly red on the
/// detector's own dictionary, which is what runs 272, 304, 310 and 45932089002
/// each did.
fn carries_through_expansion(text: &str, needle: &str) -> bool {
    if text.contains(needle) {
        return true;
    }
    if text.contains(DROPPED_BY_PARSERS) {
        let stripped: String = text
            .chars()
            .filter(|c| !DROPPED_BY_PARSERS.contains(c))
            .collect();
        if stripped.contains(needle) {
            return true;
        }
    }
    false
}

/// True when `script` itself spells `pat`, so a diagnostic that quotes any
/// slice of the script can reproduce `pat` with no internal formatter having
/// run.
///
/// This is the banned-shape twin of the canary rule in
/// [`assert_fuzz_invariants`], and it exists for the same reason: the
/// detector arms the fuzzer against itself. Every `contains(pat)` below is
/// instrumented, so libFuzzer harvests [`UNIVERSAL_BANNED`] into its
/// dictionary and splices the shapes back into its own input. The targets
/// that inline raw bytes then echo them, and the detector reports its own
/// dictionary as a leak.
///
/// [`input_echo_would_trip`] gates that at the input layer, but it is opt-in
/// and only 6 of 25 targets called it, so nightly fuzz run 304 went red on
/// five at once: `envsubst_fuzz` and `archive_fuzz` on `Undefined::`,
/// `csv_fuzz` and `tomlq_fuzz` on `/.rustup/toolchains/`, and `printf_fuzz`
/// on `builtin failed unexpectedly`. Each echo came from a *builtin*
/// diagnostic (`tomlq: path '<input>' not found`) rather than one of the
/// real-shell templates [`strip_real_shell_error_lines`] knows, and
/// enumerating every builtin's format string does not scale. Attribution
/// does: the harness applies it to every caller, so a new target cannot
/// forget it.
///
/// # Spelling a shape across the template join
///
/// The echoed slot is an arbitrary *substring* of the script, and the
/// diagnostic wraps it in its own punctuation, so a banned shape can be
/// spelled as `(tail of the template prefix) ++ (script text) ++ (head of the
/// template suffix)` while appearing nowhere in the script. Padding the whole
/// script with [`DIAGNOSTIC_ECHO_PAD`] only sees a join at the script's own
/// ends, so nightly fuzz run 310 went red on two targets at once: `awk_fuzz`
/// on a program of `{ code:…`, which `awk: cmd. line:1: {program}` renders as
/// ` { code:`, and `printf_fuzz` on an operand of `{ code:`, which
/// `printf: {arg}: invalid number` renders the same way. Both joins are
/// interior, so both scripts looked unexplained.
///
/// So the decomposition is applied to the *pattern* instead: drop a leading
/// fragment the template's lead could have supplied and a trailing fragment
/// its trail could have supplied, then ask whether the remaining core is
/// script text. Both pads are two bytes, so this is at most nine `contains`
/// calls, and it loosens only the banned shapes that begin with a space or
/// end in a colon (` { code:`, `Tok::`, `Undefined::`, `internal error:`) —
/// every other shape has no fragment to drop and stays absolute. Those four
/// need it: `printf '%d' 'internal error'` really does render
/// `printf: internal error: invalid number` with no panic behind it.
///
/// An empty core is never accepted. A shape made purely of template text
/// would mean a builtin's own format string spells it, which is a real
/// TM-INF-022 bug and not an echo, so it must keep failing.
///
/// # Residual false negative
///
/// Attribution is whole-script and whole-stderr: once the script spells a
/// shape, *every* occurrence of it in that stderr is excused, so a genuine
/// leak of the same shape in the same execution is missed. Unlike the canary
/// — a random per-run token — these are short dictionary strings, so the
/// collision rate is not negligible.
///
/// Scoping the excuse to occurrences that sit next to quoted script text was
/// considered and rejected on evidence: a diagnostic supplies its own
/// punctuation around the slot, so the neighbouring bytes are template text,
/// not script text. `printf '%d' 'Undefined::'` — the minimized form of one
/// of the five run-304 failures — renders
/// `bash: line 1: printf: Undefined::: invalid number`, where the shape is
/// framed by `printf: ` and `: invalid number` while the script frames it
/// with `'`. An adjacency rule finds no common run beyond the shape itself
/// and reports a leak, so it would re-break `printf_fuzz` (and `yq`, whose
/// `yq: /home/user/<slot>: No such file or directory` prefixes the slot with
/// a host path). The run-304 units pass an adjacency test only because their
/// surrounding bytes happened to come from the input.
///
/// What remains uncovered is narrow because the miss is per-input, not
/// per-bug: carrying the shape is a feature of the input, not a precondition
/// of the leak, so the fuzzer reaches the same formatter through inputs that
/// do not carry it, and the static scan
/// (`builtins::tests::no_debug_fmt_in_builtin_source`) plus the per-tool
/// [`assert_no_leak`] cases are absolute either way. The one case genuinely
/// hidden is a formatter whose leak is *triggered* by parsing the shape.
fn script_explains(script: &str, pat: &str) -> bool {
    let (lead, trail) = DIAGNOSTIC_ECHO_PAD;
    for drop_lead in pad_suffixes(lead) {
        let Some(rest) = pat.strip_prefix(drop_lead) else {
            continue;
        };
        for drop_trail in pad_prefixes(trail) {
            let Some(core) = rest.strip_suffix(drop_trail) else {
                continue;
            };
            if !core.is_empty() && carries_through_expansion(script, core) {
                return true;
            }
        }
    }
    false
}

/// Every suffix of `pad`, longest first and the empty one last: what a
/// diagnostic's template prefix can contribute immediately left of an echoed
/// operand. See [`script_explains`].
fn pad_suffixes(pad: &'static str) -> impl Iterator<Item = &'static str> {
    pad.char_indices()
        .map(move |(i, _)| &pad[i..])
        .chain(std::iter::once(""))
}

/// Every prefix of `pad`, shortest first and the empty one last: what a
/// diagnostic's template suffix can contribute immediately right of an echoed
/// operand. See [`script_explains`].
fn pad_prefixes(pad: &'static str) -> impl Iterator<Item = &'static str> {
    pad.char_indices()
        .map(move |(i, c)| &pad[..i + c.len_utf8()])
        .chain(std::iter::once(""))
}

/// Trailing halves of the real-shell diagnostic templates that quote user
/// input. A line ending in one of these closes such a diagnostic.
const SHELL_ERROR_SUFFIXES: &[&str] = &[
    ": command not found",
    ": No such file or directory",
    ": Is a directory",
    ": Permission denied",
    ": cannot execute: required file not found",
    ": cannot execute binary file",
    // Remaining errno templates that `io_error_reason` can emit
    // for `bash: <redirect target>: <strerror>`.
    ": Not a directory",
    ": File exists",
    ": Operation not supported",
    // Fixed bashkit redirection refusals. Like the errno templates they
    // quote the user-supplied redirect target verbatim, so a target
    // containing a banned shape is an echo, not an internal leak.
    ": filesystem redirection disabled",
    ": cannot overwrite existing file",
    ": filesystem is read-only",
];

/// Recognize stderr lines that bash, ls, or a uutils clap CLI produces
/// verbatim from user input. Conservative: each branch matches a fixed
/// real-shell error template that quotes input.
fn is_real_shell_error_line(line: &str) -> bool {
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
    is_clap_error_chrome_line(line)
}

/// Recognize the four-line clap CLI error template that uutils builtins
/// (`ls`, `cat`, `truncate`, …) emit when a user passes an unknown flag.
/// Each line quotes the offending argument verbatim, so a fuzz input
/// containing `/rustc/` becomes
/// `error: unexpected argument '--i{fi/rustc/fi{{RRi' found` and trips
/// the banned-shape check even though no internal Debug formatter ran.
///
/// Patterns are anchored on clap-specific chrome that does not occur in
/// real Debug leaks: the literal `unexpected argument '`, the leading
/// `  tip: to pass '`, the exact `Usage: ` prefix, and the
/// `For more information, try '` help footer.
fn is_clap_error_chrome_line(line: &str) -> bool {
    // `error: unexpected argument '<input>' found`
    // `error: invalid value '<input>' for '<flag>': <reason>`
    // `error: the argument '<flag>' cannot be used with '<flag>'`
    // `error: unrecognized subcommand '<input>'`
    if let Some(rest) = line.strip_prefix("error: ") {
        const CLAP_ERROR_FRAGMENTS: &[&str] = &[
            "unexpected argument '",
            "the argument '",
            "unrecognized subcommand '",
            "the following required arguments were not provided",
            "a value is required for '",
            "equal sign is needed when assigning values to '",
        ];
        if CLAP_ERROR_FRAGMENTS.iter().any(|frag| rest.contains(frag)) {
            return true;
        }
        return false;
    }
    // `  tip: to pass '<input>' as a value, use '-- <input>'`
    if let Some(rest) = line.strip_prefix("  tip: ") {
        if rest.starts_with("to pass '") && rest.contains("' as a value, use '") {
            return true;
        }
        return false;
    }
    // `Usage: <cmd> [OPTION]... [FILE]...` — user input does not appear
    // in the Usage line itself, but it follows the error/tip block and
    // we strip it for completeness so the leftover stderr is a clean
    // signal of real internal leaks.
    if let Some(rest) = line.strip_prefix("Usage: ") {
        if rest.contains(" [") || rest.ends_with(" --help") || rest.ends_with(" --version") {
            return true;
        }
        return false;
    }
    // `For more information, try '--help'.` /
    // `For more information, try '<cmd> --help'.`
    line.starts_with("For more information, try '") && line.ends_with("'.")
}

/// Full fuzz-invariant check. Like [`assert_no_leak`] but tolerates
/// real-shell-style error lines (which echo user input verbatim), skips a
/// banned shape the `script` itself spells (see `script_explains`), and
/// adds the host-canary check (TM-INF-013): the canary must not appear
/// in stdout or stderr unless `script` accounts for it.
///
/// The stderr byte cap is absolute and applies either way — attribution
/// excuses an echoed shape, never a flood.
///
/// `script` is the script that produced `result`. The canary check is
/// attribution-based rather than absolute because the detector arms the
/// fuzzer against itself: the `contains(FUZZ_HOST_CANARY)` comparisons
/// below are instrumented, so libFuzzer can learn the canary and splice
/// it into its own input (run 272), and any builtin that echoes input
/// verbatim then reproduces it. A canary the script already carries is
/// that echo; a canary the script never mentions can only have come
/// from `std::env`, which is the regression this guards.
///
/// Call this from cargo-fuzz targets and proptest cases — anywhere
/// random input runs through a builtin.
#[track_caller]
pub fn assert_fuzz_invariants(result: &ExecResult, script: &str, ctx: &str, tool_banned: &[&str]) {
    let stderr = &result.stderr;
    assert!(
        stderr.len() <= MAX_FUZZ_STDERR_BYTES,
        "[{ctx}] stderr exceeds {MAX_FUZZ_STDERR_BYTES} bytes ({} bytes):\n---\n{stderr}\n---",
        stderr.len()
    );
    let stripped = strip_real_shell_error_lines(stderr);
    for pat in UNIVERSAL_BANNED.iter().chain(tool_banned.iter()) {
        // Only a shape the script cannot explain is an internal leak. See
        // `script_explains` for why this is attribution and not an absolute.
        if script_explains(script, pat) {
            continue;
        }
        assert!(
            !stripped.contains(pat),
            "[{ctx}] stderr leaks banned shape `{pat}` (after stripping shell echoes):\n\
             ---raw stderr---\n{stderr}\n---stripped---\n{stripped}\n---"
        );
    }
    // Only an occurrence the script cannot explain is a host-env read.
    if !carries_through_expansion(script, FUZZ_HOST_CANARY) {
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
}

/// Clip `s` to at most `max` bytes for a failure message.
///
/// Floors the cut to a character boundary: callers pass a fixed byte budget
/// and the strings here are arbitrary fuzz output, so slicing at `max`
/// directly panics whenever that offset lands inside a multi-byte character —
/// turning a clear assertion message into an unrelated backtrace on exactly
/// the path that reports a canary leak.
fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut end = max;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}...<truncated>", &s[..end])
}

/// Decode one CPython HTTP bridge request record (the bytes the guest hands
/// to `bashkit.http_request`) and assert what the host guarantees before
/// dispatch: allowed method, bounded URL and headers, no CR/LF/NUL in
/// values, no host-owned headers (`Host`, framing, hop-by-hop). Never
/// panics on malformed input. Used by the `cpython_http_fuzz` target.
#[cfg(feature = "cpython")]
pub fn check_cpython_http_request(data: &[u8]) {
    crate::builtins::check_http_request_invariants(data);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn child_profile_paths_are_unique_and_outside_the_merge_set() {
        let a = child_profile_path();
        let b = child_profile_path();
        assert_ne!(a, b, "each child needs its own profraw path");
        for p in [&a, &b] {
            let text = p.to_string_lossy();
            // The whole point: never inside the directory tarpaulin merges.
            assert!(
                !text.contains("tarpaulin"),
                "child profraw must not land in tarpaulin's merge set: {text}"
            );
            assert!(text.ends_with(".profraw"), "{text}");
        }
        assert_eq!(a.parent(), b.parent(), "one directory per process");
        assert!(
            a.parent().expect("parent dir").is_dir(),
            "the sink directory is created up front"
        );
    }

    #[test]
    fn canary_in_the_input_is_counted_as_an_echo() {
        // Run 272: libFuzzer learned the canary from the detector's own
        // `contains` and spliced it into the expression. The pre-filter must
        // treat it like any other self-inflicted echo.
        let input = format!("1+{FUZZ_HOST_CANARY}");
        assert!(input_echo_would_trip(&input));
        // NUL-split, as the shell will rejoin it during word expansion.
        let (head, tail) = FUZZ_HOST_CANARY.split_at(10);
        assert!(input_echo_would_trip(&format!("{head}\0{tail}")));
    }

    #[test]
    fn canary_echoed_from_the_script_is_not_a_leak() {
        let result = ExecResult {
            stderr: format!("bash: {FUZZ_HOST_CANARY}: syntax error").into(),
            ..Default::default()
        };
        // The script put the canary there, so this is an echo, not a host read.
        assert_fuzz_invariants(
            &result,
            &format!("echo $(({FUZZ_HOST_CANARY}))"),
            "canary_echo",
            &[],
        );
    }

    #[test]
    #[should_panic(expected = "FUZZ canary leaked into stderr")]
    fn canary_absent_from_the_script_is_still_a_leak() {
        let result = ExecResult {
            stderr: format!("env: {FUZZ_HOST_CANARY}").into(),
            ..Default::default()
        };
        assert_fuzz_invariants(&result, "env", "canary_leak", &[]);
    }

    #[test]
    #[should_panic(expected = "FUZZ canary leaked into stdout")]
    fn canary_absent_from_the_script_is_still_a_leak_on_stdout() {
        let result = ExecResult {
            stdout: FUZZ_HOST_CANARY.into(),
            ..Default::default()
        };
        assert_fuzz_invariants(&result, "env", "canary_leak_stdout", &[]);
    }

    #[test]
    fn banned_shape_the_script_spells_is_an_echo() {
        // Nightly fuzz run 304: `envsubst_fuzz` and `archive_fuzz` inlined an
        // input holding the literal `Undefined::`, the builtin echoed it back,
        // and the detector called its own dictionary entry a Debug leak. No
        // real-shell template wraps the echo, so `strip_real_shell_error_lines`
        // cannot see it; attribution against the script can.
        let result = ExecResult {
            stderr: "\0Undefined::\0\0\0~?".into(),
            ..Default::default()
        };
        assert_fuzz_invariants(
            &result,
            "echo '\0Undefined::\0' | envsubst",
            "script_echo",
            &[],
        );
    }

    #[test]
    fn banned_shape_echoed_by_a_builtin_diagnostic_is_not_a_leak() {
        // Same run, `tomlq_fuzz`: the builtin quotes the user-supplied path
        // verbatim, and the path was `/.rustup/toolchains/`. Also `printf_fuzz`,
        // whose `invalid number` diagnostic quoted `builtin failed unexpectedly`
        // straight out of the input.
        let result = ExecResult {
            stderr: "tomlq: path '/.rustup/toolchains/x' not found\n".into(),
            ..Default::default()
        };
        assert_fuzz_invariants(
            &result,
            "tomlq '/.rustup/toolchains/x' f.toml",
            "script_echo_tomlq",
            &[],
        );

        let result = ExecResult {
            stderr: "bash: line 1: printf:  )builtin failed unexpectedlyT: invalid number".into(),
            ..Default::default()
        };
        assert_fuzz_invariants(
            &result,
            "printf '%d' ' )builtin failed unexpectedlyT'",
            "script_echo_printf",
            &[],
        );
    }

    #[test]
    fn banned_shape_the_script_spells_across_a_nul_is_an_echo() {
        // The shell drops NUL during word expansion, so a script carrying
        // `/.rustup/tool\0chains/` renders the banned shape on stderr while
        // no literal match exists in the script. Same transformation
        // `input_echo_would_trip` already accounts for.
        let result = ExecResult {
            stderr: "'\n_/.rustup/toolchains/\0\0".into(),
            ..Default::default()
        };
        assert_fuzz_invariants(
            &result,
            "csvlook '/.rustup/tool\0chains/'",
            "script_echo_nul",
            &[],
        );
    }

    #[test]
    fn banned_shape_spelled_across_the_template_lead_is_an_echo() {
        // `printf: {arg}: invalid number` prefixes the operand with `: `, so
        // an operand of `{ code:` spells ` { code:` while the script holds
        // only `{ code:`. Nightly fuzz run 310 (`printf_fuzz`, `awk_fuzz`).
        let result = ExecResult {
            stderr: "bash: line 1: printf: { code:: invalid number".into(),
            ..Default::default()
        };
        assert_fuzz_invariants(&result, "printf '%d' '{ code:'", "join_lead", &[]);
    }

    #[test]
    fn banned_shape_spelled_across_the_template_trail_is_an_echo() {
        // Run 295: `wZTok:` renders `bash: line 1: wZTok:: syntax error …`,
        // where the message separator's colon completes `Tok::`.
        let result = ExecResult {
            stderr: "bash: line 1: wZTok:: syntax error in expression".into(),
            ..Default::default()
        };
        assert_fuzz_invariants(&result, "echo $((wZTok:))", "join_trail", &[]);
    }

    #[test]
    fn banned_shape_spelled_across_both_template_joins_is_an_echo() {
        // Both halves at once: an operand of `{ code` renders
        // `printf: { code: invalid number`, whose ` { code:` is the lead's
        // space plus the operand plus the separator's colon.
        let result = ExecResult {
            stderr: "bash: line 1: printf: { code: invalid number".into(),
            ..Default::default()
        };
        assert_fuzz_invariants(&result, "printf '%d' '{ code'", "join_both", &[]);
    }

    #[test]
    #[should_panic(expected = "stderr leaks banned shape ` { code:`")]
    fn template_join_attribution_still_needs_the_core_in_the_script() {
        // Decomposition only forgives the pad fragments. A script that spells
        // none of the cores (` { code:`, `{ code:`, ` { code`, `{ code`) is
        // still unexplained, so a Debug shape there is a real leak.
        let result = ExecResult {
            stderr: "parse failed: { code: 3 } at offset 1".into(),
            ..Default::default()
        };
        assert_fuzz_invariants(&result, "jq '.a' f.json", "join_unexplained", &[]);
    }

    #[test]
    #[should_panic(expected = "stderr leaks banned shape `: `")]
    fn a_shape_made_only_of_template_text_is_still_a_leak() {
        // An empty core would mean the shape is entirely the diagnostic's own
        // punctuation — a format string spelling it, not an echo — so it must
        // not be excused. Exercised through the per-tool list, since no
        // universal shape is a substring of the pad.
        let result = ExecResult {
            stderr: "tool: boom".into(),
            ..Default::default()
        };
        assert_fuzz_invariants(&result, "tool run", "empty_core", &[": "]);
    }

    #[test]
    #[should_panic(expected = "stderr leaks banned shape `Undefined::`")]
    fn banned_shape_absent_from_the_script_is_still_a_leak() {
        // The point of the attribution rule: a shape the script never spells
        // can only have come from an internal formatter (TM-INF-022).
        let result = ExecResult {
            stderr: "unexpected Undefined::Ident at offset 3".into(),
            ..Default::default()
        };
        assert_fuzz_invariants(&result, "jq '.a' f.json", "real_leak", &[]);
    }

    #[test]
    #[should_panic(expected = "stderr leaks banned shape `/rustc/`")]
    fn host_path_absent_from_the_script_is_still_a_leak() {
        let result = ExecResult {
            stderr: "panic at /rustc/abc/library/core/src/option.rs:1".into(),
            ..Default::default()
        };
        assert_fuzz_invariants(&result, "yq '.a' f.yaml", "real_host_path", &[]);
    }

    #[test]
    #[should_panic(expected = "stderr leaks banned shape `Mapping {`")]
    fn tool_banned_shape_absent_from_the_script_is_still_a_leak() {
        // Attribution applies to the per-tool list too, and must not weaken it
        // when the script is clean.
        let result = ExecResult {
            stderr: "Mapping { keys: 2 }".into(),
            ..Default::default()
        };
        assert_fuzz_invariants(&result, "yq '.a' f.yaml", "real_tool_leak", &["Mapping {"]);
    }

    #[test]
    fn attribution_does_not_relax_the_stderr_cap() {
        // A script that spells a banned shape still may not flood stderr.
        // Sized off the constant so raising the cap cannot silently turn this
        // into a no-op, the way the literal `repeat(200)` did.
        const SHAPE: &str = "Undefined::";
        let result = ExecResult {
            stderr: SHAPE.repeat(MAX_FUZZ_STDERR_BYTES / SHAPE.len() + 1).into(),
            ..Default::default()
        };
        assert!(result.stderr.len() > MAX_FUZZ_STDERR_BYTES);
        let err = std::panic::catch_unwind(|| {
            assert_fuzz_invariants(&result, "echo Undefined::", "cap_still_on", &[]);
        })
        .expect_err("the byte cap must still fire");
        let msg = err
            .downcast_ref::<String>()
            .map(String::as_str)
            .unwrap_or_default();
        assert!(msg.contains("stderr exceeds"), "{msg}");
    }

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

    #[test]
    fn strip_removes_command_not_found_split_across_lines() {
        // Regression, glob_fuzz run 219 (crash-2ac9af1dcb0bb66e7849b347828a8236e12fdda3,
        // bytes `\n \0{ code:<(])\n`). The command name itself contains a
        // newline, so bash's one-line `bash: %s: command not found` template
        // renders across two lines and the line-based matcher saw neither
        // half as a template.
        let s = "bash: { code:<(])\n: command not found\n";
        assert_eq!(strip_real_shell_error_lines(s), "");
    }

    #[test]
    fn strip_span_stops_at_the_first_closing_line() {
        // The span must close on the earliest line ending in a template
        // suffix, so a real leak printed afterwards still survives.
        let s = "bash: foo: command not found\nFile { code: 1 }\n";
        let stripped = strip_real_shell_error_lines(s);
        assert!(stripped.contains("File {"), "stripped: {stripped:?}");
    }

    #[test]
    fn strip_keeps_unclosed_bash_prefix_span() {
        // A `bash: ` line with no closing template anywhere after it is not
        // a diagnostic — it must not swallow the rest of stderr.
        let s = "bash: partial Span {\nTok::Ident\n";
        let stripped = strip_real_shell_error_lines(s);
        assert!(stripped.contains("Span {"), "stripped: {stripped:?}");
        assert!(stripped.contains("Tok::"), "stripped: {stripped:?}");
    }

    #[test]
    fn input_echo_would_trip_sees_the_diagnostic_separator() {
        // Run 295: `wZTok:` carries only `Tok:`; the arithmetic diagnostic
        // renders `{expr}: {msg}` and the separator's colon completes the
        // banned `Tok::`. The gate has to render the slot, not match raw.
        assert!(!"wZTok:".contains("Tok::"));
        assert!(input_echo_would_trip("wZTok:"));
        // The slot holds `expr.trim()`, so padding is measured after trimming.
        assert!(input_echo_would_trip("wZTok:  "));
        // The leading `: ` of `bash: line N: ` completes a space-prefixed
        // shape the same way (glob_fuzz's ` { code:` class).
        assert!(input_echo_would_trip("{ code:x"));
        // Still narrow: one character short of the shape keeps running.
        assert!(!input_echo_would_trip("wZTo:"));
        assert!(!input_echo_would_trip("1+2*3"));
    }

    #[test]
    fn input_echo_would_trip_sees_through_nul_stripping() {
        // Both glob_fuzz crashes hinged on this: the raw bytes carry a NUL
        // that hides the banned shape from a literal `contains` check, and
        // the shell drops it during expansion.
        assert!(input_echo_would_trip("\n \0{ code:<(])\n"));
        assert!(input_echo_would_trip("</r\0ustc/"));
        assert!(input_echo_would_trip("plain /rustc/ path"));
        assert!(!input_echo_would_trip("ls -la /tmp/*.txt"));
    }

    #[test]
    fn strip_removes_clap_unexpected_argument_block() {
        // From a real glob_fuzz failure on main: input bytes contained
        // `--i{fi/rustc/fi{{RRi`; uutils `ls` (clap) echoed it back in
        // its standard four-line error template. The banned shape
        // `/rustc/` came from the user input, not an internal formatter.
        let s = "error: unexpected argument '--i{fi/rustc/fi{{RRi' found\n\
                 \n\
                 \x20\x20tip: to pass '--i{fi/rustc/fi{{RRi' as a value, use '-- --i{fi/rustc/fi{{RRi'\n\
                 \n\
                 Usage: ls [OPTION]... [FILE]...\n\
                 \n\
                 For more information, try '--help'.\n";
        let stripped = strip_real_shell_error_lines(s);
        assert!(!stripped.contains("/rustc/"), "stripped: {stripped:?}");
        assert!(
            !stripped.contains("unexpected argument"),
            "stripped: {stripped:?}"
        );
        assert!(!stripped.contains("Usage:"), "stripped: {stripped:?}");
        assert!(
            !stripped.contains("For more information"),
            "stripped: {stripped:?}"
        );
    }

    #[test]
    fn strip_keeps_clap_invalid_value_line() {
        let s = "error: invalid value 'Span {abc' for '--width <N>': not a number\n";
        let stripped = strip_real_shell_error_lines(s);
        assert!(stripped.contains("Span {"), "stripped: {stripped:?}");
    }

    #[test]
    fn strip_keeps_unrelated_error_prefix_lines() {
        // `error: ` lines that are not clap chrome must remain — e.g.
        // a real internal error with a Debug shape glued on.
        let s = "error: parser failed: Tok::Ident\n";
        let stripped = strip_real_shell_error_lines(s);
        assert!(stripped.contains("Tok::"), "stripped: {stripped:?}");
    }

    #[test]
    fn strip_keeps_usage_lookalikes() {
        // A line that begins with `Usage:` but lacks the clap shape
        // (no bracketed option block, no --help/--version trailer)
        // must stay — defense against masking real leaks.
        let s = "Usage: see Span { for details\n";
        let stripped = strip_real_shell_error_lines(s);
        assert!(stripped.contains("Span {"), "stripped: {stripped:?}");
    }

    /// Fuzz Testing run 45932089002, `csv_fuzz`: the input carried the canary
    /// with a `"` spliced into it, `echo` passed it through verbatim, and
    /// `csv headers` removed the quote as a CSV quote character — so the
    /// canary reached stdout whole while the script text never spelled it.
    /// End-to-end on purpose: the quote removal belongs to the csv builtin,
    /// so a hand-built `ExecResult` would not prove the path exists.
    #[tokio::test]
    async fn canary_forged_with_a_quote_the_csv_parser_drops_is_not_a_leak() {
        let (head, tail) = FUZZ_HOST_CANARY.split_at(FUZZ_HOST_CANARY.len() - 10);
        let forged = format!("{head}\"{tail}");
        assert!(
            !forged.contains(FUZZ_HOST_CANARY),
            "forgery must not be literal"
        );
        let script = format!("echo '{}' | csv headers", forged.replace('\'', "'\\''"));
        let result = run(&script).await;
        // Precondition: the builtin really does reconstitute the canary.
        assert!(
            result.stdout.contains(FUZZ_HOST_CANARY),
            "csv no longer drops the quote, so this regression is unreachable: {:?}",
            result.stdout.to_string()
        );
        // The script explains it, so this is a forgery and not a host-env read.
        assert_fuzz_invariants(&result, &script, "canary_quote_forge", &[]);
    }

    #[test]
    #[should_panic(expected = "FUZZ canary leaked into stdout")]
    fn a_script_with_quotes_that_cannot_spell_the_canary_still_leaks() {
        // Dropping quote characters must not blanket-excuse any script that
        // merely contains one: this script quotes something else entirely.
        let result = ExecResult {
            stdout: FUZZ_HOST_CANARY.into(),
            ..Default::default()
        };
        assert_fuzz_invariants(
            &result,
            "echo '\"unrelated\"' | csv headers",
            "canary_leak_quoted",
            &[],
        );
    }

    /// Fuzz Testing run 45932089002, `printf_fuzz`: `printf '%d %o %x'` cycles
    /// its format over every operand, so N non-numeric operands emit N
    /// `invalid number` diagnostics. Thirty-four of them came to 1,283 bytes —
    /// bounded, legitimate output well inside the `max_stderr_bytes(4096)` the
    /// target configures, yet over the harness's old 1 KiB cap.
    #[test]
    fn per_operand_diagnostics_within_the_engine_limit_are_not_a_flood() {
        let line = "bash: line 1: printf:  : invalid number\n";
        let stderr = line.repeat(33);
        assert!(
            (1024..=4096).contains(&stderr.len()),
            "fixture must land in the band the old cap rejected: {} bytes",
            stderr.len()
        );
        let result = ExecResult {
            stderr: stderr.into(),
            ..Default::default()
        };
        assert_fuzz_invariants(
            &result,
            "printf '%d %o %x' ' ' ' ' ' '",
            "stderr_per_operand",
            &[],
        );
    }

    #[test]
    fn truncate_cuts_on_a_char_boundary() {
        // The canary assertions hand `truncate` arbitrary fuzz output with a
        // fixed 512-byte budget, so a cut landing inside a multi-byte
        // character used to panic while *reporting* a leak.
        let s = "é".repeat(400); // 2 bytes each; 512 splits the 256th
        assert!(!s.is_char_boundary(512 - 1) || !s.is_char_boundary(511));
        let out = truncate(&s, 511);
        assert!(out.ends_with("...<truncated>"));
        assert!(out.len() < s.len());
        // Multi-byte at the very first boundary, and the no-op path.
        assert_eq!(truncate("ée", 1), "...<truncated>");
        assert_eq!(truncate("abc", 3), "abc");
    }

    #[test]
    #[should_panic(expected = "stderr exceeds")]
    fn stderr_past_the_strict_cap_is_still_a_flood() {
        // The fuzz path's cap moved; the strict path's did not go away.
        let result = ExecResult {
            stderr: "x".repeat(MAX_STDERR_BYTES + 1).into(),
            ..Default::default()
        };
        assert_no_leak(&result, "stderr_flood", &[]);
    }

    #[test]
    #[should_panic(expected = "stderr exceeds")]
    fn loosening_the_fuzz_cap_did_not_loosen_the_strict_path() {
        // Regression for the review finding on this change: both paths read one
        // shared constant, so raising it for the fuzz targets silently let a
        // builtin emit 1-4 KiB of Debug spam past `assert_no_leak`, which the
        // threat model states is absolute. Sized between the two caps, so it
        // fails if they are ever collapsed back into one.
        const { assert!(MAX_STDERR_BYTES < MAX_FUZZ_STDERR_BYTES) }
        let result = ExecResult {
            stderr: "x".repeat(MAX_FUZZ_STDERR_BYTES).into(),
            ..Default::default()
        };
        assert_no_leak(&result, "strict_cap_unrelaxed", &[]);
    }
}
