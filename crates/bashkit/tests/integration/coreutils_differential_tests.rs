//! Differential tests for ported coreutils builtins vs uutils binaries.
//!
//! For each fixture, we run the same `<util> <args>` (with the same stdin
//! and the same input files) through bashkit and through the matching
//! uutils binary, then assert byte-for-byte stdout parity. This closes
//! the "body drift" gap that `coreutils-args-drift.yml` cannot detect:
//! the args workflow only sees flag-signature changes; semantic
//! divergence inside `cat.rs` / `textrev.rs` is invisible to it.
//!
//! See `specs/coreutils-args-port.md` § Verification — Differential tests.
//!
//! Currently ported utils covered:
//! - cat: flags `-n`, `-b`, `-E`, `-A`, `-T`, `-v`, `-s`, `-ns`, `-bs`,
//!   stdin via `-`, multi-file, missing file (exit-code parity).
//! - tac: pipe + file inputs, trailing-newline edge cases. Currently
//!   unimplemented flags (`-b`, `-r`, `-s`) carry a `diff_reason` row so
//!   the table flips to a real assertion when they land.
//!
//! ## Skip policy
//!
//! Two skip gates, evaluated in order:
//!
//! 1. **Opt-in env gate** — `BASHKIT_RUN_COREUTILS_DIFF=1` must be set
//!    for the harness to attempt host comparison. The regular
//!    `cargo test --workspace` run leaves the gate off, so a body
//!    divergence between bashkit and uutils does not break unrelated
//!    test runs (the harness's *purpose* is to surface divergences;
//!    they are expected). The drift workflow
//!    (`coreutils-args-drift.yml`) sets the gate after rebuilding
//!    uutils from the pinned tree, so divergence surfaces in the same
//!    auto-PR as flag drift.
//! 2. **Binary presence** — when the gate is on, every fixture still
//!    passes with a notice if neither `uu_<util>` nor a `coreutils`
//!    multicall binary is on `$PATH`. Same pattern as
//!    `sqlite_differential_tests.rs`.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::OnceLock;

use bashkit::Bash;

/// One row in the differential corpus.
struct DiffFixture {
    /// Util name, e.g. `"cat"`.
    util: &'static str,
    /// Argv passed after the util name. Files inside `files` are
    /// referenced by their virtual path; bashkit sees them at the same
    /// path via the VFS, the host binary sees them inside a tempdir.
    args: &'static [&'static str],
    /// Optional stdin payload.
    stdin: Option<&'static [u8]>,
    /// (path, content) pairs. Materialized to disk for the host binary
    /// and into bashkit's VFS at the same path.
    files: &'static [(&'static str, &'static [u8])],
    /// When set, this row documents an intentional divergence (e.g. a
    /// flag bashkit accepts-and-errors that uutils implements). The
    /// fixture is run only against the host binary as a smoke probe;
    /// flip to a real `assert_matches` once parity is reached.
    diff_reason: Option<&'static str>,
}

/// Resolve the host invocation for a util. Returns `(program, prefix_args)`
/// where running `Command::new(program).args(prefix_args).args(util_args)`
/// invokes the matching uutils implementation.
///
/// Probe order:
/// 1. `uu_<util>` binary on PATH (preferred — matches the issue's
///    "uu_<util>" terminology and what the drift workflow builds).
/// 2. `coreutils` multicall binary — `coreutils <util> <args>`.
fn resolve_uutils(util: &str) -> Option<(PathBuf, Vec<String>)> {
    let direct = format!("uu_{util}");
    if which(&direct).is_some() {
        return Some((PathBuf::from(direct), Vec::new()));
    }
    if let Some(p) = which("coreutils") {
        return Some((p, vec![util.to_string()]));
    }
    None
}

fn which(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

fn uutils_available_for(util: &str) -> bool {
    static CACHE: OnceLock<std::sync::Mutex<std::collections::HashMap<String, bool>>> =
        OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    let mut map = cache.lock().unwrap();
    *map.entry(util.to_string())
        .or_insert_with(|| resolve_uutils(util).is_some())
}

struct HostOutput {
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    exit_code: i32,
}

/// Materialize fixture files to a tempdir and run the host uutils binary
/// with `args` rewritten so virtual paths point inside the tempdir.
fn run_host(fx: &DiffFixture) -> HostOutput {
    let (program, prefix) =
        resolve_uutils(fx.util).expect("resolve_uutils must be checked before run_host");

    let dir = tempfile::tempdir().expect("tempdir");
    let dir_path = dir.path();

    for (vpath, body) in fx.files {
        let on_disk = host_path_for(dir_path, vpath);
        if let Some(parent) = on_disk.parent() {
            std::fs::create_dir_all(parent).expect("create fixture parent");
        }
        std::fs::write(&on_disk, body).expect("write fixture file");
    }

    let mapped_args: Vec<String> = fx
        .args
        .iter()
        .map(|a| {
            if a.starts_with('/') && fx.files.iter().any(|(p, _)| p == a) {
                host_path_for(dir_path, a).to_string_lossy().into_owned()
            } else {
                (*a).to_string()
            }
        })
        .collect();

    let mut cmd = Command::new(&program);
    cmd.args(&prefix);
    cmd.args(&mapped_args);
    cmd.env("LC_ALL", "C");
    cmd.stdin(Stdio::piped());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    let mut child = cmd.spawn().expect("spawn uutils binary");
    if let Some(payload) = fx.stdin {
        child
            .stdin
            .as_mut()
            .unwrap()
            .write_all(payload)
            .expect("feed host stdin");
    }
    drop(child.stdin.take());
    let out = child.wait_with_output().expect("wait host");
    HostOutput {
        stdout: out.stdout,
        stderr: out.stderr,
        exit_code: out.status.code().unwrap_or(-1),
    }
}

fn host_path_for(root: &std::path::Path, vpath: &str) -> PathBuf {
    let stripped = vpath.trim_start_matches('/');
    root.join(stripped)
}

// VFS path used when stdin payload has no trailing newline and can't use heredoc.
const STDIN_VPATH: &str = "/__bk_diff_stdin__";

async fn run_bashkit(fx: &DiffFixture) -> (String, String, i32) {
    let mut builder = Bash::builder();
    for (vpath, body) in fx.files {
        let text = std::str::from_utf8(body).expect(
            "fixture body must be utf-8 — bashkit's mount_text takes String. \
             Use only utf-8 fixtures for now; binary fixtures need mount_bytes.",
        );
        builder = builder.mount_text(*vpath, text.to_string());
    }

    // Heredoc requires the delimiter to appear on its own line. When stdin has
    // no trailing newline the marker ends up concatenated with the last content
    // byte (e.g. "three__BK_DIFF_EOF__"), which bash does not treat as the
    // delimiter — the heredoc content then includes the marker as literal text,
    // corrupting the input. Mount such payloads as a VFS file and use a stdin
    // redirect instead so the exact bytes are preserved.
    let use_file_redirect = fx.stdin.is_some_and(|p| !p.ends_with(b"\n"));
    if use_file_redirect {
        let payload_str =
            std::str::from_utf8(fx.stdin.unwrap()).expect("stdin must be utf-8 in current harness");
        builder = builder.mount_text(STDIN_VPATH, payload_str.to_string());
    }

    let mut bash = builder.build();

    let argv: Vec<String> = std::iter::once(fx.util.to_string())
        .chain(fx.args.iter().map(|s| (*s).to_string()))
        .collect();
    let line = argv
        .iter()
        .map(|a| shell_quote(a))
        .collect::<Vec<_>>()
        .join(" ");

    let cmd = match fx.stdin {
        Some(payload) if payload.ends_with(b"\n") => format!(
            "{line} <<'__BK_DIFF_EOF__'\n{}__BK_DIFF_EOF__\n",
            std::str::from_utf8(payload).expect("stdin must be utf-8 in current harness"),
        ),
        Some(_) => format!("{line} < {STDIN_VPATH}"),
        None => line,
    };

    let r = bash.exec(&cmd).await.expect("bashkit exec");
    (r.stdout, r.stderr, r.exit_code)
}

fn shell_quote(s: &str) -> String {
    if !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '/' | '.' | ':' | '='))
    {
        return s.to_string();
    }
    let escaped = s.replace('\'', "'\\''");
    format!("'{escaped}'")
}

/// Check the opt-in gate. Returns `true` when the harness should run
/// against the host. See module-level "Skip policy" docs.
fn diff_harness_enabled() -> bool {
    std::env::var("BASHKIT_RUN_COREUTILS_DIFF").is_ok_and(|v| v == "1")
}

async fn assert_matches(fx: &DiffFixture) {
    if !diff_harness_enabled() {
        eprintln!(
            "skip: BASHKIT_RUN_COREUTILS_DIFF not set; harness is opt-in for `{u} {a:?}`",
            u = fx.util,
            a = fx.args,
        );
        return;
    }
    if !uutils_available_for(fx.util) {
        eprintln!(
            "skip: no uu_{u} or coreutils multicall on PATH for fixture `{u} {a:?}`",
            u = fx.util,
            a = fx.args,
        );
        return;
    }

    if let Some(reason) = fx.diff_reason {
        let host = run_host(fx);
        eprintln!(
            "diff_reason `{reason}` — host {u} args {args:?}: exit={code}",
            u = fx.util,
            args = fx.args,
            code = host.exit_code,
        );
        return;
    }

    let host = run_host(fx);
    let host_stdout = String::from_utf8_lossy(&host.stdout).into_owned();
    let host_stderr = String::from_utf8_lossy(&host.stderr).into_owned();

    let (bk_stdout, bk_stderr, bk_exit) = run_bashkit(fx).await;

    pretty_assertions::assert_eq!(
        bk_stdout,
        host_stdout,
        "stdout mismatch: util={u} args={args:?}",
        u = fx.util,
        args = fx.args,
    );
    assert_eq!(
        bk_exit,
        host.exit_code,
        "exit-code mismatch: util={u} args={args:?}\nhost stderr={host_stderr:?}\nbashkit stderr={bk_stderr:?}",
        u = fx.util,
        args = fx.args,
    );
    assert_eq!(
        bk_stderr.is_empty(),
        host_stderr.is_empty(),
        "stderr presence diverged: util={u} args={args:?}\nhost stderr={host_stderr:?}\nbashkit stderr={bk_stderr:?}",
        u = fx.util,
        args = fx.args,
    );
}

// ---------------------------------------------------------------------------
// cat fixtures
// ---------------------------------------------------------------------------

const CAT_THREE_LINES: &[u8] = b"alpha\nbeta\ngamma\n";
const CAT_BLANKS: &[u8] = b"x\n\ny\n";
const CAT_TABS_AND_CTRL: &[u8] = b"a\tb\nc\x01d\n";

#[tokio::test]
async fn cat_empty_input() {
    assert_matches(&DiffFixture {
        util: "cat",
        args: &["/in/empty.txt"],
        stdin: None,
        files: &[("/in/empty.txt", b"")],
        diff_reason: None,
    })
    .await;
}

#[tokio::test]
async fn cat_single_file_default() {
    assert_matches(&DiffFixture {
        util: "cat",
        args: &["/in/three.txt"],
        stdin: None,
        files: &[("/in/three.txt", CAT_THREE_LINES)],
        diff_reason: None,
    })
    .await;
}

#[tokio::test]
async fn cat_multiple_files() {
    assert_matches(&DiffFixture {
        util: "cat",
        args: &["/in/a.txt", "/in/b.txt"],
        stdin: None,
        files: &[("/in/a.txt", b"first\n"), ("/in/b.txt", b"second\n")],
        diff_reason: None,
    })
    .await;
}

#[tokio::test]
async fn cat_stdin_dash() {
    assert_matches(&DiffFixture {
        util: "cat",
        args: &["-"],
        stdin: Some(b"piped\nlines\n"),
        files: &[],
        diff_reason: None,
    })
    .await;
}

#[tokio::test]
async fn cat_number_all_lines() {
    assert_matches(&DiffFixture {
        util: "cat",
        args: &["-n", "/in/blanks.txt"],
        stdin: None,
        files: &[("/in/blanks.txt", CAT_BLANKS)],
        diff_reason: None,
    })
    .await;
}

#[tokio::test]
async fn cat_number_nonblank() {
    assert_matches(&DiffFixture {
        util: "cat",
        args: &["-b", "/in/blanks.txt"],
        stdin: None,
        files: &[("/in/blanks.txt", CAT_BLANKS)],
        diff_reason: None,
    })
    .await;
}

#[tokio::test]
async fn cat_show_ends() {
    assert_matches(&DiffFixture {
        util: "cat",
        args: &["-E", "/in/three.txt"],
        stdin: None,
        files: &[("/in/three.txt", CAT_THREE_LINES)],
        diff_reason: None,
    })
    .await;
}

#[tokio::test]
async fn cat_show_tabs() {
    assert_matches(&DiffFixture {
        util: "cat",
        args: &["-T", "/in/tabs.txt"],
        stdin: None,
        files: &[("/in/tabs.txt", CAT_TABS_AND_CTRL)],
        diff_reason: None,
    })
    .await;
}

#[tokio::test]
async fn cat_show_nonprinting_v() {
    assert_matches(&DiffFixture {
        util: "cat",
        args: &["-v", "/in/tabs.txt"],
        stdin: None,
        files: &[("/in/tabs.txt", CAT_TABS_AND_CTRL)],
        diff_reason: None,
    })
    .await;
}

#[tokio::test]
async fn cat_show_all_uppercase_a() {
    assert_matches(&DiffFixture {
        util: "cat",
        args: &["-A", "/in/tabs.txt"],
        stdin: None,
        files: &[("/in/tabs.txt", CAT_TABS_AND_CTRL)],
        diff_reason: None,
    })
    .await;
}

#[tokio::test]
async fn cat_squeeze_blank() {
    assert_matches(&DiffFixture {
        util: "cat",
        args: &["-s", "/in/squeezable.txt"],
        stdin: None,
        files: &[("/in/squeezable.txt", b"a\n\n\n\nb\n")],
        diff_reason: None,
    })
    .await;
}

#[tokio::test]
async fn cat_number_and_squeeze_combined() {
    assert_matches(&DiffFixture {
        util: "cat",
        args: &["-ns", "/in/blanks.txt"],
        stdin: None,
        files: &[("/in/blanks.txt", CAT_BLANKS)],
        diff_reason: None,
    })
    .await;
}

#[tokio::test]
async fn cat_number_nonblank_and_squeeze_combined() {
    assert_matches(&DiffFixture {
        util: "cat",
        args: &["-bs", "/in/blanks.txt"],
        stdin: None,
        files: &[("/in/blanks.txt", CAT_BLANKS)],
        diff_reason: None,
    })
    .await;
}

#[tokio::test]
async fn cat_missing_file_exit_code_parity() {
    // No fixture mounted at this path. Both engines should exit non-zero
    // and emit a stderr message; the harness asserts stderr-presence
    // parity, not byte equality of the message itself (uutils and
    // bashkit phrase the error differently and that's acceptable).
    assert_matches(&DiffFixture {
        util: "cat",
        args: &["/in/does-not-exist.txt"],
        stdin: None,
        files: &[],
        diff_reason: None,
    })
    .await;
}

#[tokio::test]
async fn cat_no_trailing_newline_preserved() {
    assert_matches(&DiffFixture {
        util: "cat",
        args: &["/in/no-newline.txt"],
        stdin: None,
        files: &[("/in/no-newline.txt", b"no-final-nl")],
        diff_reason: None,
    })
    .await;
}

// ---------------------------------------------------------------------------
// tac fixtures
// ---------------------------------------------------------------------------

#[tokio::test]
async fn tac_file_input() {
    assert_matches(&DiffFixture {
        util: "tac",
        args: &["/in/three.txt"],
        stdin: None,
        files: &[("/in/three.txt", CAT_THREE_LINES)],
        diff_reason: None,
    })
    .await;
}

#[tokio::test]
async fn tac_pipe_input() {
    assert_matches(&DiffFixture {
        util: "tac",
        args: &[],
        stdin: Some(b"one\ntwo\nthree\n"),
        files: &[],
        diff_reason: None,
    })
    .await;
}

#[tokio::test]
async fn tac_no_trailing_newline() {
    assert_matches(&DiffFixture {
        util: "tac",
        args: &[],
        stdin: Some(b"one\ntwo\nthree"),
        files: &[],
        diff_reason: None,
    })
    .await;
}

// ---------------------------------------------------------------------------
// Documented divergences — these rows exist so the corpus shape is set.
// They run host-side only and skip the bashkit-vs-host equality check.
// Flip to `diff_reason: None` once bashkit closes the gap.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn tac_before_flag_not_yet_implemented() {
    assert_matches(&DiffFixture {
        util: "tac",
        args: &["-b"],
        stdin: Some(b":one:two:three"),
        files: &[],
        diff_reason: Some("tac -b: parser-accepted-but-unimplemented in bashkit"),
    })
    .await;
}

#[tokio::test]
async fn tac_separator_flag_not_yet_implemented() {
    assert_matches(&DiffFixture {
        util: "tac",
        args: &["-s", ":"],
        stdin: Some(b"a:b:c"),
        files: &[],
        diff_reason: Some("tac -s: parser-accepted-but-unimplemented in bashkit"),
    })
    .await;
}

#[tokio::test]
async fn tac_regex_flag_not_yet_implemented() {
    assert_matches(&DiffFixture {
        util: "tac",
        args: &["-r", "-s", r"[:.]"],
        stdin: Some(b"a:b.c"),
        files: &[],
        diff_reason: Some("tac -r: parser-accepted-but-unimplemented in bashkit"),
    })
    .await;
}
