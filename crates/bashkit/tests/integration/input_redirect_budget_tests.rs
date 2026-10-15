//! Repeated numbered input redirects must charge the input budget:
//!
//! Each `N<file` / `N<&M` redirect materializes a retained `StreamData` clone
//! in the redirect list. Without a budget charge, `3</big 4</big ...`
//! duplicates the buffer once per fd with no aggregate or live-bytes
//! accounting, and superseded entries for the same fd pile up instead of
//! replacing each other.

use bashkit::{Bash, Error, ExecutionBudgetExceeded, ExecutionLimits, LimitExceeded};
use std::path::Path;

#[tokio::test]
async fn input_redirection_small_file_byte_parity() {
    let mut bash = Bash::new();
    bash.fs()
        .write_file(Path::new("/a"), b"a\xff\0z")
        .await
        .unwrap();
    for script in ["cat </a", "cat 3</a <&3", "cat 3</a 4<&3 <&4"] {
        let result = bash.exec(script).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout.as_bytes(), b"a\xff\0z");
        assert!(result.stderr.is_empty());
    }
}

#[tokio::test]
async fn input_redirection_readwrite_create_and_existing_file() {
    let mut bash = Bash::new();
    let result = bash.exec(": <>/new").await.unwrap();
    assert_eq!(result.exit_code, 0);
    assert!(
        bash.fs()
            .read_file(Path::new("/new"))
            .await
            .unwrap()
            .is_empty()
    );
    bash.fs().write_file(Path::new("/a"), b"ab").await.unwrap();
    let result = bash.exec("cat <>/a").await.unwrap();
    assert_eq!(result.exit_code, 0);
    assert_eq!(result.stdout.as_bytes(), b"ab");
    assert_eq!(bash.fs().read_file(Path::new("/a")).await.unwrap(), b"ab");
}

#[tokio::test]
async fn input_redirection_missing_file_remains_shell_failure() {
    let mut bash = Bash::new();
    let result = bash.exec("cat </missing; echo survived").await.unwrap();
    assert_eq!(result.exit_code, 0);
    assert_eq!(result.stdout, "survived\n");
    assert!(result.stderr.contains("No such file or directory"));
    assert!(!result.stderr.contains("io error:"));
}

#[tokio::test]
async fn input_redirection_tiny_mount_without_parent_entries() {
    let mounted = std::sync::Arc::new(bashkit::InMemoryFs::new());
    let fs = bashkit::MountableFs::new(std::sync::Arc::new(bashkit::InMemoryFs::new()));
    fs.mount("/mnt/input", mounted).unwrap();
    let mut bash = Bash::builder().fs(std::sync::Arc::new(fs)).build();
    bash.fs()
        .write_file(Path::new("/mnt/input/a"), b"ab")
        .await
        .unwrap();
    for (script, expected) in [
        ("cat </mnt/input/a", "ab"),
        ("cat <>/mnt/input/a", "ab"),
        (": <>/mnt/input/new", ""),
        ("cat </mnt/input/new", ""),
    ] {
        let result = bash.exec(script).await.unwrap();
        assert_eq!(result.exit_code, 0, "{}", result.stderr);
        assert_eq!(result.stdout.as_bytes(), expected.as_bytes());
    }
}

#[tokio::test]
async fn input_redirection_tiny_layered_mount() {
    for readonly in [false, true] {
        let lower = std::sync::Arc::new(bashkit::InMemoryFs::new());
        bashkit::FileSystem::write_file(&*lower, Path::new("/a"), b"ab")
            .await
            .unwrap();
        let overlay: std::sync::Arc<dyn bashkit::FileSystem> =
            std::sync::Arc::new(bashkit::OverlayFs::new(lower.clone()));
        let backend = if readonly {
            std::sync::Arc::new(bashkit::ReadOnlyFs::new(overlay))
                as std::sync::Arc<dyn bashkit::FileSystem>
        } else {
            overlay
        };
        let fs = bashkit::MountableFs::new(std::sync::Arc::new(bashkit::InMemoryFs::new()));
        fs.mount("/nested/data", backend).unwrap();
        let mut bash = Bash::builder().fs(std::sync::Arc::new(fs)).build();
        let read = bash.exec("cat </nested/data/a").await.unwrap();
        assert_eq!(read.exit_code, 0, "{}", read.stderr);
        assert_eq!(read.stdout, "ab");
        let opened = bash.exec("cat <>/nested/data/a").await.unwrap();
        assert_eq!(opened.exit_code, if readonly { 1 } else { 0 });
        if readonly {
            assert!(opened.stderr.contains("filesystem is read-only"));
            assert!(opened.stdout.is_empty());
        } else {
            assert_eq!(opened.stdout, "ab");
        }
        assert_eq!(
            bashkit::FileSystem::read_file(&*lower, Path::new("/a"))
                .await
                .unwrap(),
            b"ab"
        );
        let missing = bash
            .exec("cat </nested/data/missing; echo survived")
            .await
            .unwrap();
        assert_eq!(missing.stdout, "survived\n");
        assert!(missing.stderr.contains("No such file or directory"));
    }
}

#[tokio::test]
async fn input_redirection_tiny_rebased_namespace() {
    let source = std::sync::Arc::new(bashkit::InMemoryFs::new());
    bashkit::FileSystem::mkdir(&*source, Path::new("/project"), true)
        .await
        .unwrap();
    bashkit::FileSystem::write_file(&*source, Path::new("/project/a"), b"ab")
        .await
        .unwrap();
    let namespace = bashkit::NamespaceFs::builder()
        .mount_from(
            "/nested/data",
            source,
            "/project",
            bashkit::NamespaceAccess::ReadOnly,
        )
        .unwrap()
        .build();
    let mut bash = Bash::builder().fs(std::sync::Arc::new(namespace)).build();
    let read = bash.exec("cat </nested/data/a").await.unwrap();
    assert_eq!(read.exit_code, 0, "{}", read.stderr);
    assert_eq!(read.stdout, "ab");
    let opened = bash.exec("cat <>/nested/data/a").await.unwrap();
    assert_eq!(opened.exit_code, 1);
    assert!(!opened.stderr.is_empty());
    assert_eq!(
        bash.fs()
            .read_file(Path::new("/nested/data/a"))
            .await
            .unwrap(),
        b"ab"
    );
}

#[tokio::test]
async fn input_redirection_respects_symlink_configuration() {
    for follows in [false, true] {
        let mut bash = Bash::builder().follow_symlinks(follows).build();
        bash.fs().write_file(Path::new("/a"), b"ab").await.unwrap();
        bash.fs()
            .symlink(Path::new("/a"), Path::new("/link"))
            .await
            .unwrap();
        for script in ["cat </link", "cat <>/link"] {
            let result = bash.exec(script).await.unwrap();
            assert_eq!(result.exit_code, if follows { 0 } else { 1 });
            assert_eq!(
                result.stdout.as_bytes(),
                if follows { &b"ab"[..] } else { &[] }
            );
        }
    }
}

#[tokio::test]
async fn input_redirection_tiny_process_substitution() {
    let mut bash = Bash::new();
    for script in [
        "cat < <(printf 'x\\ny\\n')",
        "mapfile -t a < <(printf 'x\\ny\\n'); printf '%s\\n' \"${a[@]}\"",
    ] {
        let result = bash.exec(script).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout.as_bytes(), b"x\ny\n");
        assert!(result.stderr.is_empty(), "{}", result.stderr);
    }
}

#[tokio::test]
async fn input_redirection_tiny_here_inputs() {
    let mut bash = Bash::new();
    for script in ["cat <<<a", "cat <<EOF\na\nEOF"] {
        let result = bash.exec(script).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout.as_bytes(), b"a\n");
        assert!(result.stderr.is_empty());
    }
}

#[tokio::test]
/// Repeated numbered input redirects exceed the aggregate input budget.
async fn numbered_input_redirects_charge_aggregate_input_budget() {
    let payload = vec![b'x'; 32];
    let mut bash = Bash::builder()
        .limits(ExecutionLimits::new().max_aggregate_input_bytes(100))
        .build();
    bash.fs()
        .write_file(Path::new("/big"), &payload)
        .await
        .unwrap();

    let result = bash.exec(": 3</big 4</big").await;
    assert!(
        matches!(
            result,
            Err(Error::ResourceLimit(LimitExceeded::ExecutionBudget(
                ExecutionBudgetExceeded::InputBytes { .. }
            )))
        ),
        "expected aggregate input budget exhaustion, got {result:?}"
    );
}

#[tokio::test]
/// A superseded numbered input fd keeps latest-wins behavior.
async fn superseded_numbered_input_fd_is_replaced() {
    let mut bash = Bash::builder().build();
    bash.fs()
        .write_file(Path::new("/a"), b"first")
        .await
        .unwrap();
    bash.fs()
        .write_file(Path::new("/b"), b"second")
        .await
        .unwrap();

    let result = bash.exec("cat 3</a 3</b <&3").await.unwrap();
    assert_eq!(result.stdout.text().unwrap(), "second");
}

#[tokio::test]
/// Retained numbered-fd buffers hold live-bytes leases while processed.
async fn numbered_input_redirects_hold_live_bytes_budget() {
    let payload = vec![b'x'; 32];
    let mut bash = Bash::builder()
        .limits(ExecutionLimits::new().max_live_intermediate_bytes(100))
        .build();
    bash.fs()
        .write_file(Path::new("/big"), &payload)
        .await
        .unwrap();

    let result = bash.exec(": 3</big 4</big").await;
    assert!(
        matches!(
            result,
            Err(Error::ResourceLimit(LimitExceeded::ExecutionBudget(
                ExecutionBudgetExceeded::LiveBytes { .. }
            )))
        ),
        "expected live-bytes budget exhaustion, got {result:?}"
    );
}

#[tokio::test]
/// Original attack shape: ':' with 1024 same-fd /dev/zero redirects trips
/// the aggregate budget on the first retained buffer instead of piling MiBs.
async fn original_shape_1024_dev_zero_trips_fast() {
    let mut bash = Bash::builder()
        .limits(ExecutionLimits::new().max_aggregate_input_bytes(65_536))
        .build();
    let script = format!(":{}", " 3</dev/zero".repeat(1024));
    let result = bash.exec(&script).await;
    assert!(
        matches!(
            result,
            Err(Error::ResourceLimit(LimitExceeded::ExecutionBudget(
                ExecutionBudgetExceeded::InputBytes { .. }
            )))
        ),
        "expected aggregate input budget exhaustion, got {result:?}"
    );
}

#[tokio::test]
/// Both backing buffers count: a 32B ASCII payload retains 64B, so a cap of
/// 63 trips on a single redirect while 64 succeeds.
async fn both_backing_buffers_admitted() {
    for (cap, ok) in [(71_u64, false), (72, true)] {
        let mut bash = Bash::builder()
            .limits(ExecutionLimits::new().max_aggregate_input_bytes(cap))
            .build();
        bash.fs()
            .write_file(Path::new("/big"), &[b'x'; 32])
            .await
            .unwrap();
        let result = bash.exec(": 3</big").await;
        assert_eq!(result.is_ok(), ok, "aggregate cap {cap}");
    }
    for (cap, ok) in [(63_u64, false), (64, true)] {
        let mut bash = Bash::builder()
            .limits(ExecutionLimits::new().max_live_intermediate_bytes(cap))
            .build();
        bash.fs()
            .write_file(Path::new("/big"), &[b'x'; 32])
            .await
            .unwrap();
        let result = bash.exec(": 3</big").await;
        assert_eq!(result.is_ok(), ok, "live cap {cap}");
    }
}

#[tokio::test]
/// Replacing a superseded same-fd entry releases its live lease: eight
/// same-fd repeats never hold more than two 64B buffers transiently (one
/// retained plus one incoming during replacement), so a 128B live cap
/// suffices where append-only leases would grow without bound.
async fn superseded_entry_releases_live_lease() {
    let mut bash = Bash::builder()
        .limits(ExecutionLimits::new().max_live_intermediate_bytes(128_u64))
        .build();
    bash.fs()
        .write_file(Path::new("/a"), &[b'y'; 32])
        .await
        .unwrap();
    let script = format!(":{}", " 3</a".repeat(8));
    let result = bash.exec(&script).await;
    assert!(
        result.is_ok(),
        "expected lease release on replace, got {result:?}"
    );
}

#[tokio::test]
/// Aggregate consumption is cumulative across replaces: the second same-fd
/// repeat exceeds a 100B aggregate cap even though live stays at 64B.
async fn cumulative_aggregate_survives_replace() {
    let mut bash = Bash::builder()
        .limits(ExecutionLimits::new().max_aggregate_input_bytes(100))
        .build();
    bash.fs()
        .write_file(Path::new("/a"), &[b'y'; 32])
        .await
        .unwrap();
    let result = bash.exec(": 3</a 3</a").await;
    assert!(
        matches!(
            result,
            Err(Error::ResourceLimit(LimitExceeded::ExecutionBudget(
                ExecutionBudgetExceeded::InputBytes { .. }
            )))
        ),
        "expected cumulative aggregate exhaustion, got {result:?}"
    );
}

#[tokio::test]
/// The DupInput target duplication itself is charged: with a 16B payload
/// (32B retained), a 47B aggregate cap admits the 15B script plus the source
/// fd (47B total) but the `4<&3` clone (79B) trips `InputBytes`. Controls
/// prove the script text and source fd fit on their own.
async fn dup_target_duplication_charges() {
    // Control 1: source fd alone fits (7B script + 32B retained = 39B).
    let mut bash = Bash::builder()
        .limits(ExecutionLimits::new().max_aggregate_input_bytes(39))
        .build();
    bash.fs()
        .write_file(Path::new("/big"), b"sixteen-bytesxx")
        .await
        .unwrap();
    assert!(bash.exec(": 3</big").await.is_ok());
    // Control 2: source plus dup fits under a 79B cap.
    let mut bash = Bash::builder()
        .limits(ExecutionLimits::new().max_aggregate_input_bytes(79))
        .build();
    bash.fs()
        .write_file(Path::new("/big"), b"sixteen-bytesxx")
        .await
        .unwrap();
    assert!(bash.exec(": 3</big 4<&3").await.is_ok());
    // The duplication itself trips: script (15B) + source (32B) fits 47B,
    // the target clone (+32B = 79B) does not.
    let mut bash = Bash::builder()
        .limits(ExecutionLimits::new().max_aggregate_input_bytes(47))
        .build();
    bash.fs()
        .write_file(Path::new("/big"), b"sixteen-bytesxx")
        .await
        .unwrap();
    let result = bash.exec(": 3</big 4<&3").await;
    assert!(
        matches!(
            result,
            Err(Error::ResourceLimit(LimitExceeded::ExecutionBudget(
                ExecutionBudgetExceeded::InputBytes { .. }
            )))
        ),
        "target duplication should trip InputBytes, got {result:?}"
    );
}

#[tokio::test]
/// ReadWrite numbered retention is charged: 10B script + 32B retained trips
/// a 31B aggregate cap with `InputBytes`.
async fn readwrite_numbered_retention_charges() {
    let mut bash = Bash::builder()
        .limits(ExecutionLimits::new().max_aggregate_input_bytes(31))
        .build();
    bash.fs()
        .write_file(Path::new("/big"), b"sixteen-bytesxx")
        .await
        .unwrap();
    let result = bash.exec(": 3<>/big").await;
    assert!(
        matches!(
            result,
            Err(Error::ResourceLimit(LimitExceeded::ExecutionBudget(
                ExecutionBudgetExceeded::InputBytes { .. }
            )))
        ),
        "ReadWrite retention should trip InputBytes, got {result:?}"
    );
}

#[tokio::test]
/// The session stays usable after a budget error: unrelated commands run.
async fn session_recovers_after_budget_error() {
    let mut bash = Bash::builder()
        .limits(ExecutionLimits::new().max_aggregate_input_bytes(100))
        .build();
    bash.fs()
        .write_file(Path::new("/big"), &[b'x'; 32])
        .await
        .unwrap();
    let tripped = bash.exec(": 3</big 4</big").await;
    assert!(
        tripped.is_err(),
        "expected the redirect to trip, got {tripped:?}"
    );
    let result = bash.exec("echo recovered").await.unwrap();
    assert_eq!(result.exit_code, 0);
    assert!(
        result.stdout.text().unwrap().contains("recovered"),
        "unexpected stdout: {:?}",
        result.stdout.text()
    );
}

#[tokio::test]
/// Finite bash parity: numbered-fd duplication chains resolve latest-wins.
async fn numbered_fd_duplication_chain() {
    let mut bash = Bash::builder().build();
    bash.fs()
        .write_file(Path::new("/a"), b"alpha")
        .await
        .unwrap();
    let result = bash.exec("cat 3</a 4<&3 <&4").await.unwrap();
    assert_eq!(result.stdout.text().unwrap(), "alpha");
}

#[tokio::test]
/// A 1MiB numbered-heredoc body stays bounded: the script-text charge covers
/// it once, it is not retained per-exec, and it must not double-trip a cap
/// above the script size. Guards against double-charge regressions.
async fn large_numbered_heredoc_stays_bounded() {
    let body = "z".repeat(1024 * 1024);
    let script = format!(": 3<<EOF\n{body}\nEOF");
    let mut bash = Bash::builder()
        .limits(ExecutionLimits::new().max_aggregate_input_bytes(1_572_864))
        .build();
    let result = bash.exec(&script).await.unwrap();
    assert_eq!(result.exit_code, 0);
}

#[tokio::test]
/// Stdin-clone live overlap: `cat 3</a <&3 4</b` simultaneously owns three
/// 64B buffers (fd-3 entry, stdin clone, fd-4 entry), so a 192B live cap
/// succeeds while 191B trips `LiveBytes` on the third admission.
async fn stdin_clone_live_overlap() {
    for (cap, ok) in [(192_u64, true), (191, false)] {
        let mut bash = Bash::builder()
            .limits(ExecutionLimits::new().max_live_intermediate_bytes(cap))
            .build();
        bash.fs()
            .write_file(Path::new("/a"), &[b'A'; 32])
            .await
            .unwrap();
        bash.fs()
            .write_file(Path::new("/b"), &[b'B'; 32])
            .await
            .unwrap();
        let result = bash.exec("cat 3</a <&3 4</b").await;
        if ok {
            let out = result.unwrap();
            assert_eq!(out.exit_code, 0);
            assert_eq!(out.stdout.text().unwrap(), "A".repeat(32));
        } else {
            assert!(
                matches!(
                    result,
                    Err(Error::ResourceLimit(LimitExceeded::ExecutionBudget(
                        ExecutionBudgetExceeded::LiveBytes { .. }
                    )))
                ),
                "live cap {cap} should trip LiveBytes, got {result:?}"
            );
        }
    }
}

#[tokio::test]
/// After a live-budget trip on a stdin-clone overlap, the session recovers
/// for commands within the remaining budget.
async fn stdin_clone_trip_recovers() {
    let mut bash = Bash::builder()
        .limits(ExecutionLimits::new().max_live_intermediate_bytes(191_u64))
        .build();
    bash.fs()
        .write_file(Path::new("/a"), &[b'A'; 32])
        .await
        .unwrap();
    bash.fs()
        .write_file(Path::new("/b"), &[b'B'; 32])
        .await
        .unwrap();
    let tripped = bash.exec("cat 3</a <&3 4</b").await;
    assert!(
        tripped.is_err(),
        "expected the overlap to trip, got {tripped:?}"
    );
    let result = bash.exec("echo recovered").await.unwrap();
    assert_eq!(result.exit_code, 0);
    assert!(result.stdout.text().unwrap().contains("recovered"));
}

// ---------- opt-in bounded runner extension for the input-redirect job ----------
// Reuses reviewed primitives (`drain_capped`, `reap_with_grace`) with a
// job-specific launcher. Shared defaults for other products are untouched.

#[cfg(not(target_arch = "wasm32"))]
use crate::threat_model_tests::resource_exhaustion::bashkit_cli_path;
#[cfg(not(target_arch = "wasm32"))]
use std::process::Stdio;
use std::time::Duration;
#[cfg(not(target_arch = "wasm32"))]
use tokio::process::Command;

/// File-size backstop for the bash `ulimit -f` knob, in the shell's blocks.
/// MEASURED (not assumed): both /bin/bash and /opt/homebrew/bin/bash report
/// 1024-byte increments (`ulimit -f 1` reads back 1024 bytes via
/// resource.getrlimit), so 131072 blocks = 128MiB actual on measured builds.
/// The assertion below accepts exactly the historically attested unit set
/// {512B POSIX, 1024B measured bash} and fails closed on anything else.
const IR_FILE_BLOCKS: u64 = 131_072;
/// Active RSS cutoff in KiB (`ps -o rss=` reports KiB on Linux and macOS):
/// Sampled observations trigger termination on exceedance. This is not a
/// hard OS memory cap or proof that no between-sample peak occurred.
#[cfg(not(target_arch = "wasm32"))]
const IR_RSS_CAP_KIB: u64 = 1_048_576; // 1GiB
/// Explicit allowance for one short instrumented run's coverage artifact.
#[cfg(not(target_arch = "wasm32"))]
const IR_ARTIFACT_BOUND: u64 = 64 * 1024 * 1024; // 64MiB
/// Per-stream kept bytes for concurrent diagnostic drains.
#[cfg(not(target_arch = "wasm32"))]
const PIPE_CAP: usize = 256 * 1024;

/// Opt-in launcher for the input-redirect job: shared CPU/stack caps plus a
/// file-size backstop, every `ulimit` fail-closed
/// (`|| exit 99`), with an extended readback receipt on stderr.
///
/// Virtual address space is deliberately not capped, matching the shared
/// sanitizer-compatible launcher contract. Memory fields record unsupported;
/// resident memory is sampled by the owned parent-side watchdog instead.
/// Raw values use the shell's own units (POSIX 512B blocks for `-f`; bash
/// KiB for `-s`; seconds for `-t`); `file_bytes` comes from
/// /proc byte-exact on Linux and is `unsupported` elsewhere, so unit
/// surprises fail closed at parse/assert time instead of converting
/// silently.
fn input_shape_launcher() -> String {
    format!(
        "ulimit -s 4096 || exit 99; \
         ulimit -t 10 || exit 99; \
         ulimit -f {fileblk} || exit 99; \
         mem_kb=unsupported; mem_bytes=unsupported; file_bytes=unsupported; \
         if [ \"$(uname)\" = Linux ] && [ -r /proc/self/limits ]; then \
         file_bytes=$(awk '/Max file size/ {{print $4}}' /proc/self/limits); \
         fi; \
         echo \"ir-limits: stack=$(ulimit -s) cpu=$(ulimit -t) \
         mem_kb=$mem_kb mem_bytes=$mem_bytes \
         fileblk=$(ulimit -Sf) file_bytes=$file_bytes\" >&2; \
         exec \"$@\"",
        fileblk = IR_FILE_BLOCKS,
    )
}

/// Parsed `ir-limits:` receipt. Raw fields are the shell's own units;
/// `*_bytes` are kernel byte counts (Linux) or `None` (explicitly
/// unsupported elsewhere).
#[derive(Debug)]
struct IrLimitsNote {
    stack_raw: String,
    cpu_raw: String,
    mem_kb_raw: String,
    mem_bytes: Option<u64>,
    fileblk_raw: String,
    file_bytes: Option<u64>,
}

fn parse_ir_limits_note(line: &str) -> Result<IrLimitsNote, String> {
    let body = line
        .strip_prefix("ir-limits:")
        .ok_or_else(|| format!("missing ir-limits prefix: {line:?}"))?;
    let get = |key: &str| -> Result<String, String> {
        body.split_whitespace()
            .find_map(|field| field.strip_prefix(&format!("{key}=")))
            .map(str::to_string)
            .ok_or_else(|| format!("missing field {key} in {line:?}"))
    };
    let parse_bytes = |key: &str, raw: &str| -> Result<Option<u64>, String> {
        if raw == "unsupported" {
            return Ok(None);
        }
        raw.parse::<u64>()
            .map(Some)
            .map_err(|_| format!("bad byte count {key}={raw:?} in {line:?}"))
    };
    let mem_bytes_raw = get("mem_bytes")?;
    let file_bytes_raw = get("file_bytes")?;
    Ok(IrLimitsNote {
        stack_raw: get("stack")?,
        cpu_raw: get("cpu")?,
        mem_kb_raw: get("mem_kb")?,
        mem_bytes: parse_bytes("mem_bytes", &mem_bytes_raw)?,
        fileblk_raw: get("fileblk")?,
        file_bytes: parse_bytes("file_bytes", &file_bytes_raw)?,
    })
}

/// Classified artifact-sink outcome: absence is normal off-coverage, a
/// present artifact must fit the explicit bound, anything else fails loudly
/// (never confused with absence).
#[derive(Debug)]
enum ArtifactCheck {
    Absent,
    Within(u64),
    Over(u64),
}

fn check_artifact_bytes(profile: &std::path::Path, bound: u64) -> std::io::Result<ArtifactCheck> {
    match std::fs::metadata(profile) {
        Ok(meta) => Ok(if meta.len() < bound {
            ArtifactCheck::Within(meta.len())
        } else {
            ArtifactCheck::Over(meta.len())
        }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(ArtifactCheck::Absent),
        Err(e) => Err(e),
    }
}

/// Owned temp working dir. Ownership is established FIRST (the guard object
/// exists before any fallible use); every operation runs through the guard.
/// `cleanup` disarms only after removal succeeds, so a failed cleanup keeps
/// the guard armed and fails loudly instead of leaking silently. `Drop` is a
/// backstop for unwind paths only and documents its silent best-effort.
#[derive(Debug)]
struct TempWorkdir {
    path: Option<std::path::PathBuf>,
}

impl TempWorkdir {
    fn create(tag: &str) -> std::io::Result<Self> {
        let path = std::env::temp_dir().join(format!("bashkit-{tag}-{}", std::process::id()));
        // Exclusive creation: existence is NOT proof this invocation created
        // the directory. A collision rejects WITHOUT deleting so preexisting
        // contents (possibly another owner's) are always preserved.
        match std::fs::create_dir(&path) {
            Ok(()) => Ok(Self { path: Some(path) }),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Err(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                format!("work dir collision, refusing to adopt: {}", path.display()),
            )),
            Err(e) => Err(e),
        }
    }
    fn path(&self) -> &std::path::Path {
        self.path.as_ref().expect("workdir already cleaned")
    }

    fn cleanup(mut self) -> std::io::Result<()> {
        let path = self.path.as_ref().expect("workdir already cleaned");
        std::fs::remove_dir_all(path)?;
        self.path = None;
        Ok(())
    }
}

impl Drop for TempWorkdir {
    // Backstop only: the explicit `cleanup` path above is the truthful
    // receipt. A Drop-time failure has nowhere to report, hence best-effort;
    // normal paths never reach here armed.
    fn drop(&mut self) {
        if let Some(path) = self.path.take() {
            let _ = std::fs::remove_dir_all(path);
        }
    }
}

/// Parse `ps -o rss=` output (KiB on Linux and macOS) to a byte-exact...
/// KiB count. Anything else fails loudly: a policing loop must never parse
/// garbage as a small number.
fn parse_ps_rss(out: &str) -> Result<u64, String> {
    out.trim()
        .parse::<u64>()
        .map_err(|_| format!("unparseable ps rss output: {out:?}"))
}

/// Attested file-size unit factor: the byteproof must equal blocks times one
/// of the historically attested shell units (512B POSIX, 1024B measured
/// bash). Anything else fails closed instead of converting silently.
fn file_size_unit_factor(blocks: u64, bytes: u64) -> Result<u64, String> {
    for unit in [512u64, 1024u64] {
        if blocks.saturating_mul(unit) == bytes {
            return Ok(unit);
        }
    }
    Err(format!(
        "file byteproof {bytes} matches neither 512B nor 1024B units for {blocks} blocks"
    ))
}

/// Outcome of one opt-in bounded input-shape run: the reviewed verdict plus
/// job-specific receipts. Setup faults panic with context (fail-closed);
/// runtime observations are flags for the caller to assert on.
#[cfg(not(target_arch = "wasm32"))]
struct InputShapeRun {
    exit_code: Option<i32>,
    wall_limit_seconds: u64,
    stdout: String,
    stderr: String,
    timed_out: bool,
    oversized: bool,
    incomplete_readers: bool,
    sampler_complete: bool,
    lifecycle_error: Option<String>,
    read_error: bool,
    reaped: bool,
    limits: IrLimitsNote,
    artifact: ArtifactCheck,
    leftovers: Vec<String>,
    file_unit: Option<u64>,
    rss_pid: u32,
    rss_samples: u32,
    peak_rss_kb: u64,
    rss_exceeded: bool,
    exited_fast: bool,
}

/// Sample resident-set size (KiB) of `pid` via `ps` (`rss` is KiB on
/// Linux and macOS). Any failure is loud: blind policing is worse than none,
/// and the caller kills first on doubt.
/// Bounded async drain: keeps at most `cap` bytes, counts the rest without
/// storing. Cancellation-safe per `AsyncReadExt::read` (locked tokio
/// 1.53.2): dropping the future abandons only unread bytes, so a deadline
/// timeout followed by kill+reap stays finite. Never collects unbounded
/// `read_to_end`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StreamNotice {
    Excess,
    ReadError,
}

async fn ir_drain_capped_async<R>(
    pipe: &mut R,
    cap: usize,
    mut notice: Option<tokio::sync::mpsc::Sender<StreamNotice>>,
) -> (Vec<u8>, usize, bool)
where
    R: tokio::io::AsyncRead + Unpin,
{
    use tokio::io::AsyncReadExt;
    let mut kept = Vec::new();
    let mut total = 0usize;
    let mut had_error = false;
    let mut chunk = [0u8; 8192];
    loop {
        match pipe.read(&mut chunk).await {
            Ok(0) => break,
            Ok(n) => {
                let room = cap.saturating_sub(kept.len()).min(n);
                kept.extend_from_slice(&chunk[..room]);
                total = total.saturating_add(n);
                // One notice per stream; the shared channel has two slots.
                // Notify before EOF while continuing bounded capture.
                if total > cap
                    && let Some(sender) = notice.take()
                    && sender.try_send(StreamNotice::Excess).is_err()
                {
                    had_error = true;
                    break;
                }
            }
            Err(_) => {
                had_error = true;
                if let Some(sender) = notice.take() {
                    // A closed receiver cannot turn a failed read into success.
                    let _ = sender.try_send(StreamNotice::ReadError);
                }
                break;
            }
        }
    }
    let kept_len = kept.len();
    (kept, total.saturating_sub(kept_len), had_error)
}

#[derive(Debug)]
enum ReapOutcome<T> {
    Reaped(T),
    Failed(std::io::Error),
    TimedOut,
}

async fn reap_before<T>(
    future: impl Future<Output = std::io::Result<T>>,
    deadline: tokio::time::Instant,
) -> ReapOutcome<T> {
    match tokio::time::timeout_at(deadline, future).await {
        Ok(Ok(status)) => ReapOutcome::Reaped(status),
        Ok(Err(error)) => ReapOutcome::Failed(error),
        Err(_) => ReapOutcome::TimedOut,
    }
}

#[derive(Debug)]
struct SampleError {
    message: String,
    cleanup_complete: bool,
}

/// One lifecycle owns wait, captures and the optional RSS sampler. Errors
/// request kill once, then retain all futures through finite completion.
/// Drop remains a best-effort backstop, never a completion receipt.
#[cfg(not(target_arch = "wasm32"))]
struct ChildCompletion {
    status: Option<std::process::ExitStatus>,
    out: (Vec<u8>, usize, bool),
    err: (Vec<u8>, usize, bool),
    reaped: bool,
    readers_complete: bool,
    sampler_complete: bool,
    timed_out: bool,
    failure: Option<String>,
    rss_samples: u32,
    peak_rss_kb: u64,
    rss_exceeded: bool,
    exited_fast: bool,
}

#[cfg(not(target_arch = "wasm32"))]
async fn complete_owned_child(
    child: &mut OwnedChild,
    cap: usize,
    deadline: tokio::time::Instant,
    sample_pid: Option<u32>,
    grace: Duration,
) -> ChildCompletion {
    let (mut out_pipe, mut err_pipe) = child.take_pipes();
    let missing_pipe = out_pipe.is_none() || err_pipe.is_none();
    let (notice_tx, mut notices) = tokio::sync::mpsc::channel(2);
    let err_notice = notice_tx.clone();
    let mut out_fut = std::pin::pin!(async {
        match out_pipe.as_mut() {
            Some(pipe) => ir_drain_capped_async(pipe, cap, Some(notice_tx)).await,
            None => (Vec::new(), 0, true),
        }
    });
    let mut err_fut = std::pin::pin!(async {
        match err_pipe.as_mut() {
            Some(pipe) => ir_drain_capped_async(pipe, cap, Some(err_notice)).await,
            None => (Vec::new(), 0, true),
        }
    });
    let mut completion = ChildCompletion {
        status: None,
        out: (Vec::new(), 0, false),
        err: (Vec::new(), 0, false),
        reaped: false,
        readers_complete: false,
        sampler_complete: false,
        timed_out: false,
        failure: missing_pipe.then(|| "child capture pipe missing".into()),
        rss_samples: 0,
        peak_rss_kb: 0,
        rss_exceeded: false,
        exited_fast: false,
    };
    let mut stop_requested = missing_pipe;
    let mut kill_sent = false;
    let mut wait_failed = false;
    let mut out_done = false;
    let mut err_done = false;
    let mut notices_open = true;
    let mut sample_fut: Option<SampleFuture> = None;
    let mut ticker = tokio::time::interval_at(
        tokio::time::Instant::now() + Duration::from_millis(50),
        Duration::from_millis(500),
    );
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let finish_deadline = deadline + grace;
    let mut sampler_cleanup_complete = true;
    loop {
        if stop_requested && !kill_sent && completion.status.is_none() {
            kill_sent = true;
            if let Err(error) = child.start_kill() {
                completion
                    .failure
                    .get_or_insert_with(|| format!("kill child: {error}"));
            }
        }
        if (completion.status.is_some() || wait_failed)
            && out_done
            && err_done
            && sample_fut.is_none()
        {
            break;
        }
        tokio::select! {
            biased;
            _ = tokio::time::sleep_until(finish_deadline) => {
                completion.failure.get_or_insert_with(|| "child completion timed out".into());
                // Send before leaving the owner loop, including a failed wait.
                if !kill_sent && completion.status.is_none()
                    && let Err(error) = child.start_kill()
                {
                    completion.failure.get_or_insert_with(|| format!("kill child: {error}"));
                }
                break;
            }
            _ = tokio::time::sleep_until(deadline),
                if !stop_requested && completion.status.is_none() =>
            {
                completion.timed_out = true;
                stop_requested = true;
            }
            result = child.wait(), if completion.status.is_none() && !wait_failed => {
                match result {
                    Ok(status) => {
                        completion.status = Some(status);
                        completion.reaped = true;
                        completion.exited_fast = completion.rss_samples == 0;
                    }
                    Err(error) => {
                        completion.failure = Some(format!("wait child: {error}"));
                        wait_failed = true;
                        stop_requested = true;
                    }
                }
            }
            notice = notices.recv(), if notices_open => {
                match notice {
                    Some(notice) => {
                        completion.failure.get_or_insert_with(|| format!("child stream: {notice:?}"));
                        stop_requested = true;
                    }
                    None => notices_open = false,
                }
            }
            out = &mut out_fut, if !out_done => {
                if out.1 > 0 || out.2 {
                    completion.failure.get_or_insert_with(|| "stdout capture failed".into());
                    stop_requested = true;
                }
                completion.out = out;
                out_done = true;
            }
            err = &mut err_fut, if !err_done => {
                if err.1 > 0 || err.2 {
                    completion.failure.get_or_insert_with(|| "stderr capture failed".into());
                    stop_requested = true;
                }
                completion.err = err;
                err_done = true;
            }
            res = poll_owned_sample(&mut sample_fut), if sample_fut.is_some() => {
                sample_fut = None;
                match res {
                    Ok(rss) => {
                        completion.rss_samples += 1;
                        completion.peak_rss_kb = completion.peak_rss_kb.max(rss);
                        if rss > IR_RSS_CAP_KIB {
                            completion.rss_exceeded = true;
                            stop_requested = true;
                        }
                    }
                    Err(error) => {
                        if !error.cleanup_complete {
                            sampler_cleanup_complete = false;
                            completion.failure = Some(error.message);
                            stop_requested = true;
                        } else if completion.status.is_none() {
                            match child.try_wait() {
                                Ok(Some(status)) => {
                                    completion.status = Some(status);
                                    completion.reaped = true;
                                    completion.exited_fast = completion.rss_samples == 0;
                                }
                                Ok(None) => {
                                    completion.failure = Some(error.message);
                                    stop_requested = true;
                                }
                                Err(poll_error) => {
                                    completion.failure = Some(format!("poll child: {poll_error}; {}", error.message));
                                    stop_requested = true;
                                    wait_failed = true;
                                }
                            }
                        }
                    }
                }
            }
            _ = ticker.tick(),
                if sample_pid.is_some() && !stop_requested
                    && completion.status.is_none() && sample_fut.is_none() =>
            {
                sample_fut = Some(Box::pin(sample_child_rss(sample_pid.expect("sampling enabled"))));
            }
        }
    }
    completion.readers_complete = out_done && err_done;
    match finish_owned_sample(&mut sample_fut, finish_deadline).await {
        Ok(Some(rss)) => {
            completion.rss_samples += 1;
            completion.peak_rss_kb = completion.peak_rss_kb.max(rss);
            completion.rss_exceeded |= rss > IR_RSS_CAP_KIB;
        }
        Ok(None) => {}
        Err(error) => {
            sampler_cleanup_complete &= error.cleanup_complete;
            if !error.cleanup_complete {
                completion.failure.get_or_insert(error.message);
            }
        }
    }
    completion.sampler_complete = sample_fut.is_none() && sampler_cleanup_complete;
    if completion.status.is_none() {
        match reap_before(
            child.wait(),
            tokio::time::Instant::now() + grace.min(Duration::from_secs(5)),
        )
        .await
        {
            ReapOutcome::Reaped(status) => {
                completion.status = Some(status);
                completion.reaped = true;
            }
            ReapOutcome::Failed(error) => {
                completion
                    .failure
                    .get_or_insert_with(|| format!("reap child: {error}"));
            }
            ReapOutcome::TimedOut => {
                completion
                    .failure
                    .get_or_insert_with(|| "reap child timed out".into());
            }
        }
    }
    completion
}

/// RSS sampling uses the same finite owned lifecycle as the main child.
/// Sampling failure and incomplete reap/capture are distinct verdicts.
#[cfg(not(target_arch = "wasm32"))]
async fn sample_child_rss(pid: u32) -> Result<u64, SampleError> {
    let ps = Command::new("ps")
        .arg("-o")
        .arg("rss=")
        .arg("-p")
        .arg(pid.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|error| SampleError {
            message: format!("spawn ps for rss of {pid}: {error}"),
            cleanup_complete: true, // Spawn failed; no child exists.
        })?;
    let mut ps = OwnedChild::new(ps);
    let capture = complete_owned_child(
        &mut ps,
        4096,
        tokio::time::Instant::now() + Duration::from_secs(5),
        None,
        Duration::from_secs(2),
    )
    .await;
    let cleanup_complete = capture.reaped && capture.readers_complete && capture.sampler_complete;
    if !cleanup_complete
        || capture.timed_out
        || capture.failure.is_some()
        || !capture.status.is_some_and(|status| status.success())
    {
        return Err(SampleError {
            message: format!(
                "ps rss of {pid} failed: {} (reaped={}, captures={})",
                capture.failure.as_deref().unwrap_or("unsuccessful sample"),
                capture.reaped,
                capture.readers_complete
            ),
            cleanup_complete,
        });
    }
    parse_ps_rss(&String::from_utf8_lossy(&capture.out.0)).map_err(|message| SampleError {
        message,
        cleanup_complete,
    })
}
type SampleFuture = std::pin::Pin<Box<dyn Future<Output = Result<u64, SampleError>>>>;

async fn poll_owned_sample(slot: &mut Option<SampleFuture>) -> Result<u64, SampleError> {
    // Losing a select branch must not destroy the pending sampler it borrows.
    slot.as_mut().expect("armed with sampler").as_mut().await
}

async fn finish_owned_sample(
    slot: &mut Option<SampleFuture>,
    deadline: tokio::time::Instant,
) -> Result<Option<u64>, SampleError> {
    if slot.is_none() {
        return Ok(None);
    }
    match tokio::time::timeout_at(deadline, poll_owned_sample(slot)).await {
        Ok(result) => {
            *slot = None;
            result.map(Some)
        }
        Err(_) => Err(SampleError {
            message: "owned sampler completion timed out".into(),
            cleanup_complete: false,
        }),
    }
}

#[tokio::test]
async fn ir_helper_reap_outcomes() {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(1);
    assert!(matches!(
        reap_before(std::future::ready(Ok(7_u8)), deadline).await,
        ReapOutcome::Reaped(7)
    ));
    match reap_before(
        std::future::ready(Err::<u8, _>(std::io::Error::other("injected"))),
        deadline,
    )
    .await
    {
        ReapOutcome::Failed(error) => assert_eq!(error.kind(), std::io::ErrorKind::Other),
        outcome => panic!("failed wait must remain an error: {outcome:?}"),
    }
    assert!(matches!(
        reap_before(
            std::future::pending::<std::io::Result<u8>>(),
            tokio::time::Instant::now()
        )
        .await,
        ReapOutcome::TimedOut
    ));
}

#[tokio::test]
async fn ir_helper_live_excess_notice() {
    let (mut writer, mut reader) = tokio::io::duplex(64);
    tokio::io::AsyncWriteExt::write_all(&mut writer, &[1_u8; 32])
        .await
        .unwrap();
    let (tx, mut rx) = tokio::sync::mpsc::channel(2);
    let mut drain = std::pin::pin!(ir_drain_capped_async(&mut reader, 16, Some(tx)));
    let notice = tokio::time::timeout(Duration::from_secs(1), async {
        tokio::select! {
            notice = rx.recv() => notice,
            _ = &mut drain => panic!("writer is open; capture must remain pending"),
        }
    })
    .await
    .unwrap();
    assert_eq!(notice, Some(StreamNotice::Excess));
    drop(writer);
    let (kept, truncated, error) = drain.await;
    assert_eq!(kept, vec![1_u8; 16]);
    assert_eq!(truncated, 16);
    assert!(!error);
}

#[tokio::test]
async fn ir_helper_live_error_notice() {
    let (tx, mut rx) = tokio::sync::mpsc::channel(2);
    let mut reader = FailOnce { failed: false };
    let (notice, (kept, truncated, error)) =
        tokio::join!(rx.recv(), ir_drain_capped_async(&mut reader, 16, Some(tx)),);
    assert_eq!(notice, Some(StreamNotice::ReadError));
    assert!(kept.is_empty() && truncated == 0 && error);
}

#[tokio::test]
async fn ir_helper_terminal_sampler_completion() {
    let (tx, rx) = tokio::sync::oneshot::channel::<u64>();
    let mut slot: Option<SampleFuture> = Some(Box::pin(async move {
        rx.await.map_err(|error| SampleError {
            message: error.to_string(),
            cleanup_complete: true,
        })
    }));
    let deadline = tokio::time::Instant::now() + Duration::from_secs(1);
    let (result, ()) = tokio::join!(finish_owned_sample(&mut slot, deadline), async {
        tx.send(7).unwrap();
    },);
    assert_eq!(result.unwrap(), Some(7));
    assert!(slot.is_none());
    slot = Some(Box::pin(std::future::pending()));
    let error = finish_owned_sample(&mut slot, tokio::time::Instant::now())
        .await
        .unwrap_err();
    assert!(!error.cleanup_complete);
    assert_eq!(error.message, "owned sampler completion timed out");
    assert!(
        slot.is_some(),
        "timeout cannot claim completion or discard the owned future"
    );
}

#[cfg(not(target_arch = "wasm32"))]
async fn run_input_shape_bounded(script: &str, wall_secs: u64, tag: &str) -> InputShapeRun {
    let sink = TempWorkdir::create(&format!("{tag}-sink")).expect("artifact sink setup");
    let work = TempWorkdir::create(&format!("{tag}-work")).expect("producer workdir setup");
    let profile = sink.path().join("child.profraw");
    let cli = bashkit_cli_path();
    let mut cmd = Command::new("bash");
    cmd.arg("-c")
        .arg(input_shape_launcher())
        .arg("ir-child")
        .arg(&cli)
        .arg("-c")
        .arg(script)
        .env("LLVM_PROFILE_FILE", &profile)
        .current_dir(work.path())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        // Backstop only: SIGKILL on drop if the explicit kill+reap below
        // never runs. Not proof of anything; receipts are.
        .kill_on_drop(true);
    let mut child = OwnedChild::new(cmd.spawn().expect("spawn bounded child"));
    let rss_pid = child.id();
    let completion = complete_owned_child(
        &mut child,
        PIPE_CAP,
        tokio::time::Instant::now() + Duration::from_secs(wall_secs),
        Some(rss_pid),
        Duration::from_secs(15),
    )
    .await;
    let (out_kept, out_trunc, out_err) = completion.out;
    let (err_kept, err_trunc, err_err) = completion.err;
    // Receipt first: the launcher note proves the caps ran; without it the
    // run is untrusted regardless of exit code.
    let stderr_full = String::from_utf8_lossy(&err_kept).into_owned();
    let (note, rest) = stderr_full
        .split_once('\n')
        .expect("launcher receipt missing: empty stderr");
    let limits = parse_ir_limits_note(note).unwrap_or_else(|e| panic!("bad launcher receipt: {e}"));
    let stderr = rest.to_string();
    #[cfg(target_os = "linux")]
    let file_unit = {
        let blocks: u64 = limits
            .fileblk_raw
            .parse()
            .expect("fileblk receipt must be numeric");
        let bytes = limits.file_bytes.expect("file byteproof required on Linux");
        Some(file_size_unit_factor(blocks, bytes).unwrap_or_else(|e| panic!("file unit cap: {e}")))
    };
    #[cfg(not(target_os = "linux"))]
    let file_unit = None;
    let artifact = check_artifact_bytes(&profile, IR_ARTIFACT_BOUND)
        .unwrap_or_else(|e| panic!("cannot stat artifact sink: {e:?}"));
    // Inventory before cleanup: any producer file fails closed by naming it;
    // entry errors are loud, never filtered away.
    let mut leftovers = Vec::new();
    for entry in std::fs::read_dir(work.path())
        .unwrap_or_else(|e| panic!("cannot inventory work dir: {e:?}"))
    {
        match entry {
            Ok(e) => leftovers.push(e.file_name().to_string_lossy().into_owned()),
            Err(e) => panic!("cannot inventory work dir entry: {e:?}"),
        }
    }
    sink.cleanup().expect("artifact sink cleanup");
    work.cleanup().expect("work dir cleanup");
    InputShapeRun {
        exit_code: completion.status.and_then(|s| s.code()),
        wall_limit_seconds: wall_secs,
        stdout: String::from_utf8_lossy(&out_kept).into_owned(),
        stderr,
        timed_out: completion.timed_out,
        oversized: out_trunc > 0 || err_trunc > 0,
        incomplete_readers: !completion.readers_complete,
        sampler_complete: completion.sampler_complete,
        lifecycle_error: completion.failure,
        read_error: out_err || err_err,
        reaped: completion.reaped,
        limits,
        artifact,
        leftovers,
        file_unit,
        rss_pid,
        rss_samples: completion.rss_samples,
        peak_rss_kb: completion.peak_rss_kb,
        rss_exceeded: completion.rss_exceeded,
        exited_fast: completion.exited_fast,
    }
}

/// Effective-limits assertions shared by bounded input-shape runs: exact raw
/// round-trips everywhere; kernel byte proofs on Linux, explicit unsupported
/// markers elsewhere (fail-closed on any unit surprise).
/// Effective-bounds assertions shared by bounded input-shape runs: exact raw
/// round-trips everywhere; kernel byte proofs on Linux (including the
/// shell-unit factor for `-f`); explicit unsupported markers elsewhere
/// (fail-closed on any unit surprise); RSS policing receipts (a real pid,
/// no observed exceedance, sampled maximum under the cutoff, samples taken unless the child
/// exited before the first poll).
#[cfg(not(target_arch = "wasm32"))]
fn assert_effective_bounds(r: &InputShapeRun) {
    use std::io::Write as _;

    let (artifact_class, artifact_bytes) = match &r.artifact {
        ArtifactCheck::Absent => ("absent", None),
        ArtifactCheck::Within(bytes) => ("within", Some(*bytes)),
        ArtifactCheck::Over(bytes) => ("over", Some(*bytes)),
    };
    let receipt = serde_json::json!({
        "producer_exit_code": r.exit_code,
        "wall_limit_seconds": r.wall_limit_seconds,
        "per_stream_capture_limit_bytes": PIPE_CAP,
        "stack_kib": r.limits.stack_raw,
        "cpu_seconds": r.limits.cpu_raw,
        "address_space_kib": r.limits.mem_kb_raw,
        "address_space_bytes": r.limits.mem_bytes,
        "address_space_enforced_by_helper": false,
        "memory_guard": "sampled-rss",
        "rss_limit_kib": IR_RSS_CAP_KIB,
        "file_blocks": r.limits.fileblk_raw,
        "file_bytes": r.limits.file_bytes,
        "file_unit_bytes": r.file_unit,
        "artifact_class": artifact_class,
        "artifact_bytes": artifact_bytes,
        "artifact_limit_bytes": IR_ARTIFACT_BOUND,
        "reaped": r.reaped,
        "readers_complete": !r.incomplete_readers,
        "sampler_complete": r.sampler_complete,
        "lifecycle_error": r.lifecycle_error,
        "read_error": r.read_error,
        "timed_out": r.timed_out,
        "oversized": r.oversized,
        "leftover_count": r.leftovers.len(),
        "rss_samples": r.rss_samples,
        "sampled_peak_rss_kib": r.peak_rss_kb,
        "rss_exceeded": r.rss_exceeded,
    });
    // Direct stderr preserves the measured receipt in successful CI logs too.
    writeln!(std::io::stderr().lock(), "input-shape-receipt: {receipt}")
        .expect("write bounded input receipt");
    assert!(r.sampler_complete, "owned sampler completion unknown");
    assert!(
        r.lifecycle_error.is_none(),
        "child lifecycle failed: {:?}",
        r.lifecycle_error
    );
    let limits = &r.limits;
    assert_eq!(limits.stack_raw, "4096", "stack cap round-trip");
    assert_eq!(limits.cpu_raw, "10", "cpu cap round-trip");
    assert_eq!(
        limits.mem_kb_raw, "unsupported",
        "no AS enforcement by helper"
    );
    assert_eq!(limits.mem_bytes, None, "no AS byteproof claimed");
    assert_eq!(
        limits.fileblk_raw,
        IR_FILE_BLOCKS.to_string(),
        "file cap round-trip"
    );
    #[cfg(target_os = "linux")]
    {
        assert!(
            r.file_unit.is_some(),
            "file unit factor must be attested on Linux"
        );
    }
    #[cfg(not(target_os = "linux"))]
    {
        assert_eq!(limits.file_bytes, None, "no file byteproof here");
        assert_eq!(r.file_unit, None, "no file unit attested here");
    }
    assert_ne!(r.rss_pid, 0, "RSS policing must attach to a real pid");
    assert!(
        !r.rss_exceeded,
        "RSS cutoff tripped at {} KiB",
        r.peak_rss_kb
    );
    assert!(
        r.peak_rss_kb < IR_RSS_CAP_KIB,
        "sampled peak RSS {} KiB over cutoff",
        r.peak_rss_kb
    );
    assert!(
        r.rss_samples >= 1 || r.exited_fast,
        "RSS policing never sampled a live child"
    );
}

/// Original shape under default limits in the opt-in bounded runner: `:`
/// plus 1024 same-fd `/dev/zero` redirects is denied quickly by the
/// aggregate input budget (each 1MiB read retains ~2MiB against the 100MB
/// default cap, tripping after ~50 redirects) instead of piling ~2GiB of
/// retained clones. Asserts the exact denial: clean exit code 1 naming
/// `aggregate input bytes` (production retention admission, far above the
/// ~7KiB script text), distinguishing budget denial from OOM/signal death
/// (no exit code) and from pre-fix success (exit 0).
/// NOT EXECUTED in this environment (hostile-fixture ban); kept as the
/// blocked-verification target with full harness assertions intact.
#[cfg(all(unix, not(target_arch = "wasm32")))]
#[tokio::test]
async fn original_shape_bounded_child_default_limits() {
    let r =
        run_input_shape_bounded(&format!(":{}", " 3</dev/zero".repeat(1024)), 90, "ir-1024").await;
    assert!(!r.timed_out, "bounded child overran its deadline");
    assert!(!r.oversized, "bounded child output exceeded pipe caps");
    assert!(
        !r.incomplete_readers,
        "bounded child left readers unfinished"
    );
    assert!(!r.read_error, "bounded child pipe read failed");
    assert!(r.reaped, "bounded child was not reaped");
    assert_effective_bounds(&r);
    assert!(
        matches!(r.artifact, ArtifactCheck::Absent | ArtifactCheck::Within(_)),
        "artifact outside explicit allowance: {:?}",
        r.artifact
    );
    assert!(r.leftovers.is_empty(), "producer files: {:?}", r.leftovers);
    assert_eq!(
        r.exit_code,
        Some(1),
        "producer must fail cleanly, got {:?} with stderr {:?}",
        r.exit_code,
        r.stderr
    );
    assert!(
        r.stderr.contains("aggregate input bytes"),
        "denial must name the aggregate input budget phase, got: {:?}",
        r.stderr
    );
    assert!(r.stdout.is_empty(), "producer must print nothing");
}

#[test]
fn ir_helper_launcher_fragments() {
    let text = input_shape_launcher();
    for fragment in [
        "ulimit -s 4096 || exit 99",
        "ulimit -t 10 || exit 99",
        "ulimit -f 131072 || exit 99",
        "mem_kb=unsupported; mem_bytes=unsupported",
        "ir-limits:",
        "mem_bytes=$mem_bytes",
        "file_bytes=$file_bytes",
        "exec \"$@\"",
    ] {
        assert!(
            text.contains(fragment),
            "launcher must contain {fragment:?}"
        );
    }
}

#[test]
fn ir_helper_parse_receipt() {
    let full = parse_ir_limits_note("ir-limits: stack=4096 cpu=10 mem_kb=unsupported mem_bytes=unsupported fileblk=131072 file_bytes=134217728").unwrap();
    assert_eq!(full.stack_raw, "4096");
    assert_eq!(full.cpu_raw, "10");
    assert_eq!(full.mem_kb_raw, "unsupported");
    assert_eq!(full.mem_bytes, None);
    assert_eq!(full.fileblk_raw, "131072");
    assert_eq!(full.file_bytes, Some(134217728));
    let unsupported = parse_ir_limits_note("ir-limits: stack=4096 cpu=10 mem_kb=unsupported mem_bytes=unsupported fileblk=131072 file_bytes=unsupported").unwrap();
    assert_eq!(unsupported.mem_bytes, None);
    assert_eq!(unsupported.file_bytes, None);
    assert!(parse_ir_limits_note("bogus line").is_err());
    assert!(parse_ir_limits_note("ir-limits: stack=4096 cpu=10").is_err());
    assert!(parse_ir_limits_note("ir-limits: stack=4096 cpu=10 mem_kb=unsupported mem_bytes=unsupported fileblk=131072 file_bytes=nope").is_err());
}

#[test]
fn ir_helper_artifact_classes() {
    let dir = TempWorkdir::create("ir-artifact-probe").expect("probe dir");
    let small = dir.path().join("small.profraw");
    std::fs::write(&small, [7u8; 32]).expect("probe file");
    assert!(matches!(
        check_artifact_bytes(&small, 64).expect("stat small"),
        ArtifactCheck::Within(32)
    ));
    assert!(matches!(
        check_artifact_bytes(&small, 33).expect("stat small"),
        ArtifactCheck::Within(32)
    ));
    assert!(matches!(
        check_artifact_bytes(&small, 32).expect("stat small"),
        ArtifactCheck::Over(32)
    ));
    assert!(matches!(
        check_artifact_bytes(&dir.path().join("absent"), 64).expect("stat absent"),
        ArtifactCheck::Absent
    ));
    assert!(check_artifact_bytes(std::path::Path::new("\0"), 64).is_err());
    dir.cleanup().expect("probe cleanup");
}

#[test]
fn ir_helper_workdir_lifecycle() {
    let dir = TempWorkdir::create("ir-lifecycle-probe").expect("probe dir");
    let path = dir.path().to_path_buf();
    assert!(path.is_dir());
    std::fs::write(path.join("x"), b"y").expect("probe file");
    dir.cleanup().expect("probe cleanup");
    assert!(!path.exists(), "cleanup must remove the dir");
}

#[test]
fn ir_helper_workdir_drop_cleans() {
    let path = {
        let dir = TempWorkdir::create("ir-drop-probe").expect("probe dir");
        dir.path().to_path_buf()
    };
    assert!(!path.exists(), "drop must remove the dir");
}

/// Finite normal child completion through the full opt-in wrapper with an
/// ordinary harmless script: proves the harness itself works end to end.
/// NOT vulnerability proof: `true` exercises no budget path.
#[cfg(all(unix, not(target_arch = "wasm32")))]
#[tokio::test]
async fn ir_helper_normal_child_completes() {
    let r = run_input_shape_bounded("true", 30, "ir-true").await;
    assert!(
        !r.timed_out && !r.oversized && !r.incomplete_readers && !r.read_error && r.reaped,
        "harness lifecycle must complete cleanly"
    );
    assert_effective_bounds(&r);
    assert_eq!(r.exit_code, Some(0));
    assert!(r.stdout.is_empty() && r.stderr.is_empty());
    assert!(r.leftovers.is_empty());
    assert!(matches!(
        r.artifact,
        ArtifactCheck::Absent | ArtifactCheck::Within(_)
    ));
}

#[test]
fn ir_helper_file_unit_cases() {
    // Measured shell units: 1024B increments on tested bash builds, 512B POSIX.
    assert_eq!(file_size_unit_factor(131072, 131072 * 1024), Ok(1024));
    assert_eq!(file_size_unit_factor(131072, 131072 * 512), Ok(512));
    assert!(file_size_unit_factor(131072, 131072 * 1000).is_err());
    assert!(file_size_unit_factor(131072, 0).is_err());
}
/// A preexisting dir must fail creation AND keep every byte: existence is
/// not proof this invocation created it, so a collision rejects without
/// deleting (possibly another owner's contents). The fixture first
/// exclusively creates its own root; a foreign preexisting path would fail
/// loudly at plant time instead of being adopted or removed.
#[test]
fn ir_helper_workdir_collision_preserves() {
    let tag = "ir-collision-probe";
    let path = std::env::temp_dir().join(format!("bashkit-{tag}-{}", std::process::id()));
    match std::fs::create_dir(&path) {
        Ok(()) => {}
        Err(e) => panic!("fixture root busy, clean {path:?} manually: {e:?}"),
    }
    std::fs::write(path.join("precious"), b"precious").expect("plant collision file");
    let err = TempWorkdir::create(tag).expect_err("collision must fail");
    assert_eq!(err.kind(), std::io::ErrorKind::AlreadyExists);
    assert_eq!(
        std::fs::read(path.join("precious")).expect("preexisting contents preserved"),
        b"precious"
    );
    std::fs::remove_dir_all(&path).expect("remove owned planted dir");
    assert!(!path.exists());
}

#[test]
fn ir_helper_ps_parse_cases() {
    assert_eq!(parse_ps_rss("  1234\n"), Ok(1234));
    assert_eq!(parse_ps_rss("0"), Ok(0));
    assert!(parse_ps_rss("").is_err());
    assert!(parse_ps_rss("abc").is_err());
    assert!(parse_ps_rss("12 34").is_err());
}

/// Owned child handle: established immediately after spawn, before any
/// fallible pipe/thread/poll operation. Every later expect runs with this
/// owner alive, so unwind paths always have a defined backstop. Normal
/// paths reap explicitly (see `reap_with_grace` callers) and then drop the
/// guard; Drop kills best-effort (documented backstop, never the primary
/// path) but never waits, so Drop cannot block.
/// Owned child handle: established immediately after spawn, before any
/// fallible pipe/poll operation. Every later expect runs with this owner
/// alive, so unwind paths always have a defined backstop. Normal paths reap
/// explicitly with a finite deadline and then drop the guard; Drop sends
/// SIGKILL best-effort via sync `start_kill` (documented backstop, never the
/// primary path) but never waits, so Drop cannot block.
#[cfg(not(target_arch = "wasm32"))]
struct OwnedChild {
    inner: Option<tokio::process::Child>,
}

#[cfg(not(target_arch = "wasm32"))]
impl OwnedChild {
    fn new(child: tokio::process::Child) -> Self {
        Self { inner: Some(child) }
    }

    fn id(&self) -> u32 {
        self.inner
            .as_ref()
            .expect("child already released")
            .id()
            .expect("child has pid")
    }

    fn take_pipes(
        &mut self,
    ) -> (
        Option<tokio::process::ChildStdout>,
        Option<tokio::process::ChildStderr>,
    ) {
        let inner = self.inner.as_mut().expect("child already released");
        (inner.stdout.take(), inner.stderr.take())
    }

    fn try_wait(&mut self) -> std::io::Result<Option<std::process::ExitStatus>> {
        self.inner
            .as_mut()
            .expect("child already released")
            .try_wait()
    }

    fn start_kill(&mut self) -> std::io::Result<()> {
        self.inner
            .as_mut()
            .expect("child already released")
            .start_kill()
    }

    async fn wait(&mut self) -> std::io::Result<std::process::ExitStatus> {
        self.inner
            .as_mut()
            .expect("child already released")
            .wait()
            .await
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Drop for OwnedChild {
    fn drop(&mut self) {
        if let Some(child) = self.inner.as_mut() {
            let _ = child.start_kill();
        }
    }
}

/// In-memory async drain contract: 100B through a duplex pair with a 16B
/// cap keeps 16, counts 84 truncated, no error. Harmless synthetic fixture:
/// proves the bounded-drain primitive admits before growth without any
/// child, pipe, or resource pressure.
#[tokio::test]
async fn ir_helper_async_drain_bounded() {
    let (mut writer, mut reader) = tokio::io::duplex(256);
    tokio::io::AsyncWriteExt::write_all(&mut writer, &[9u8; 100])
        .await
        .expect("fill duplex");
    drop(writer);
    let (kept, trunc, err) = ir_drain_capped_async(&mut reader, 16, None).await;
    assert_eq!(kept.len(), 16);
    assert_eq!(trunc, 84);
    assert!(!err);
}

/// Injected I/O error contract: a reader failing on first poll reports
/// had_error instead of hanging or misattributing bytes.
struct FailOnce {
    failed: bool,
}

impl tokio::io::AsyncRead for FailOnce {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
        _buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        if self.failed {
            std::task::Poll::Ready(Ok(()))
        } else {
            self.get_mut().failed = true;
            std::task::Poll::Ready(Err(std::io::Error::other("injected")))
        }
    }
}

#[tokio::test]
async fn ir_helper_async_drain_error() {
    let mut reader = FailOnce { failed: false };
    let (kept, trunc, err) = ir_drain_capped_async(&mut reader, 16, None).await;
    assert!(kept.is_empty());
    assert_eq!(trunc, 0);
    assert!(err);
}

/// Deterministic timer contract: a ready timer completes under deadline; a
/// pending future does not. Proves timeout_at behaves before trusting it
/// for kill/reap/drain bounds. No wall-clock flakiness: margins are 500x.
#[tokio::test]
async fn ir_helper_timeout_contract() {
    use std::time::Duration;
    let ok = tokio::time::timeout_at(
        tokio::time::Instant::now() + Duration::from_secs(5),
        tokio::time::sleep(Duration::from_millis(10)),
    )
    .await;
    assert!(ok.is_ok());
    let timed_out = tokio::time::timeout_at(
        tokio::time::Instant::now() + Duration::from_millis(10),
        std::future::pending::<()>(),
    )
    .await;
    assert!(timed_out.is_err());
}

/// Scheduling contract: a release-gated sampler, two 32B duplex drains, and
/// a timer must ALL complete concurrently in one select! scope. The release
/// fires only after both drains finish. Poll the wrapper's actual owned-slot
/// helper so losing a selection cannot destroy that release path. 64B total fixture
/// data, 2s outer bound, fully deterministic (no timing margins asserted,
/// only completion).
#[tokio::test]
async fn ir_helper_concurrent_schedule() {
    use std::time::Duration;
    let (tx, rx) = tokio::sync::oneshot::channel::<u32>();
    let (mut w1, mut r1) = tokio::io::duplex(64);
    let (mut w2, mut r2) = tokio::io::duplex(64);
    tokio::io::AsyncWriteExt::write_all(&mut w1, &[1u8; 32])
        .await
        .expect("fill one");
    tokio::io::AsyncWriteExt::write_all(&mut w2, &[2u8; 32])
        .await
        .expect("fill two");
    drop((w1, w2));
    let mut sampler: Option<SampleFuture> = Some(Box::pin(async move {
        rx.await.map(u64::from).map_err(|error| SampleError {
            message: error.to_string(),
            cleanup_complete: true,
        })
    }));
    let mut d1 = std::pin::pin!(ir_drain_capped_async(&mut r1, 64, None));
    let mut d2 = std::pin::pin!(ir_drain_capped_async(&mut r2, 64, None));
    let mut tx_opt = Some(tx);
    let mut got = 0u64;
    let mut k1 = Vec::new();
    let mut k2 = Vec::new();
    let mut sampler_done = false;
    let mut d1_done = false;
    let mut d2_done = false;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    let finished = tokio::time::timeout_at(deadline, async {
        loop {
            if sampler_done && d1_done && d2_done {
                break;
            }
            tokio::select! {
                biased;
                v = poll_owned_sample(&mut sampler), if sampler.is_some() => {
                    got = v.expect("sampler completed");
                    sampler = None;
                    sampler_done = true;
                }
                _ = tokio::time::sleep(Duration::from_millis(5)) => {
                    if d1_done && d2_done && let Some(tx) = tx_opt.take() {
                        let _ = tx.send(7);
                    }
                }
                r = &mut d1, if !d1_done => {
                    let (kept, trunc, err) = r;
                    k1 = kept;
                    assert_eq!(trunc, 0);
                    assert!(!err);
                    d1_done = true;
                }
                r = &mut d2, if !d2_done => {
                    let (kept, trunc, err) = r;
                    k2 = kept;
                    assert_eq!(trunc, 0);
                    assert!(!err);
                    d2_done = true;
                }
            }
        }
    })
    .await;
    assert!(finished.is_ok(), "concurrent schedule must finish fast");
    assert_eq!(got, 7);
    assert_eq!(k1, vec![1u8; 32]);
    assert_eq!(k2, vec![2u8; 32]);
}
