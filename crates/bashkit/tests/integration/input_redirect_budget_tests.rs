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

use crate::threat_model_tests::resource_exhaustion::{
    bashkit_cli_path, drain_capped, reap_with_grace,
};
use std::process::{Command, Stdio};
use std::time::Duration;

/// Address-space cap in KiB for the bash `ulimit -v` knob (Linux only).
const IR_AS_KIB: u64 = 1_572_864; // 1.5GiB
/// File-size backstop for the bash `ulimit -f` knob, in the shell's blocks.
/// MEASURED (not assumed): both /bin/bash and /opt/homebrew/bin/bash report
/// 1024-byte increments (`ulimit -f 1` reads back 1024 bytes via
/// resource.getrlimit), so 131072 blocks = 128MiB actual on measured builds.
/// The assertion below accepts exactly the historically attested unit set
/// {512B POSIX, 1024B measured bash} and fails closed on anything else.
const IR_FILE_BLOCKS: u64 = 131_072;
/// Active RSS cutoff in KiB (`ps -o rss=` reports KiB on Linux and macOS):
/// catches runaway growth far below RAM/disk exhaustion while legit runs
/// peak at O(MB). Policed by polling, not assumed from any cap.
const IR_RSS_CAP_KIB: u64 = 1_048_576; // 1GiB
/// Explicit allowance for one short instrumented run's coverage artifact.
const IR_ARTIFACT_BOUND: u64 = 64 * 1024 * 1024; // 64MiB

/// Opt-in launcher for the input-redirect job: shared CPU/stack caps plus an
/// address-space cap and a file-size backstop, every `ulimit` fail-closed
/// (`|| exit 99`), with an extended readback receipt on stderr.
///
/// The AS cap is Linux-only (`ulimit -v` is refused elsewhere); other
/// platforms record `mem=unsupported` truthfully instead of pretending.
/// Raw values use the shell's own units (POSIX 512B blocks for `-f`; bash
/// KiB for `-s`/`-v`; seconds for `-t`); `mem_bytes`/`file_bytes` come from
/// /proc byte-exact on Linux and are `unsupported` elsewhere, so unit
/// surprises fail closed at parse/assert time instead of converting
/// silently.
fn input_shape_launcher() -> String {
    format!(
        "ulimit -s 4096 || exit 99; \
         ulimit -t 10 || exit 99; \
         ulimit -f {fileblk} || exit 99; \
         mem_kb=unsupported; mem_bytes=unsupported; file_bytes=unsupported; \
         if [ \"$(uname)\" = Linux ] && [ -r /proc/self/limits ]; then \
         ulimit -v {askib} || exit 99; \
         mem_kb=$(ulimit -Sv); \
         mem_bytes=$(awk '/Max address space/ {{print $4}}' /proc/self/limits); \
         file_bytes=$(awk '/Max file size/ {{print $4}}' /proc/self/limits); \
         fi; \
         echo \"ir-limits: stack=$(ulimit -s) cpu=$(ulimit -t) \
         mem_kb=$mem_kb mem_bytes=$mem_bytes \
         fileblk=$(ulimit -Sf) file_bytes=$file_bytes\" >&2; \
         exec \"$@\"",
        fileblk = IR_FILE_BLOCKS,
        askib = IR_AS_KIB,
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
struct InputShapeRun {
    exit_code: Option<i32>,
    stdout: String,
    stderr: String,
    timed_out: bool,
    oversized: bool,
    incomplete_readers: bool,
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
fn sample_child_rss(pid: u32) -> Result<u64, String> {
    // The sampler itself is bounded (5s): `ps` must never block past the
    // wall deadline. Kill and reap on expiry, loud error either way.
    let mut ps = std::process::Command::new("ps")
        .arg("-o")
        .arg("rss=")
        .arg("-p")
        .arg(pid.to_string())
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("spawn ps for rss of {pid}: {e:?}"))?;
    let ps_deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        match ps.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => {
                if std::time::Instant::now() >= ps_deadline {
                    ps.kill()
                        .map_err(|e| format!("kill stuck ps for rss of {pid}: {e:?}"))?;
                    let _ = ps.wait();
                    return Err(format!("ps rss of {pid} timed out"));
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            Err(e) => return Err(format!("poll ps for rss of {pid}: {e:?}")),
        }
    }
    let out = ps
        .wait_with_output()
        .map_err(|e| format!("reap ps for rss of {pid}: {e:?}"))?;
    if !out.status.success() {
        return Err(format!(
            "ps rss of {pid} failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    parse_ps_rss(&String::from_utf8_lossy(&out.stdout))
}

/// Join a pipe-reader thread before `deadline`. On time, returns its
/// receipts with `true`. Past the deadline, one final blocking join
/// completes the original reader: pipes hit EOF once the reaped child is
/// gone and our shapes spawn no grandchildren, so this cannot hang on a live
/// pipe; it is still flagged `false` so the verdict fails closed. The
/// handle is never forgotten or dropped unfinished.
fn ir_join_capped(
    handle: std::thread::JoinHandle<(Vec<u8>, bool, bool)>,
    deadline: std::time::Instant,
) -> (Vec<u8>, bool, bool, bool) {
    while !handle.is_finished() {
        if std::time::Instant::now() >= deadline {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let finished = handle.is_finished();
    let (kept, oversized, read_error) = handle.join().expect("reader thread join");
    (kept, oversized, read_error, finished)
}

fn run_input_shape_bounded(script: &str, wall_secs: u64, tag: &str) -> InputShapeRun {
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
        .stderr(Stdio::piped());
    let mut child = OwnedChild::new(cmd.spawn().expect("spawn bounded child"));
    let rss_pid = child.id();
    // Pipes are taken before the first poll so the concurrent readers below
    // own them outright; the child handle keeps only pid and lifecycle.
    let (out_pipe, err_pipe) = child.take_pipes();
    let mut out_pipe = out_pipe.expect("take child stdout");
    let mut err_pipe = err_pipe.expect("take child stderr");
    let out_reader = std::thread::Builder::new()
        .name("ir-child-stdout".into())
        .spawn(move || drain_capped(&mut out_pipe))
        .expect("spawn stdout reader");
    let err_reader = std::thread::Builder::new()
        .name("ir-child-stderr".into())
        .spawn(move || drain_capped(&mut err_pipe))
        .expect("spawn stderr reader");
    let start = std::time::Instant::now();
    let deadline = start + Duration::from_secs(wall_secs);
    let mut ticks = 0u64;
    let mut rss_samples = 0u32;
    let mut peak_rss_kb = 0u64;
    let mut rss_exceeded = false;
    let mut exited_fast = false;
    let mut overran = false;
    // try_wait reaps the zombie it reports, so the exited branch needs no
    // further wait; killed branches go through the reviewed grace reap,
    // never an open wait.
    let mut final_status: Option<std::process::ExitStatus> = None;
    let mut reaped = false;
    loop {
        if let Some(status) = child.try_wait().expect("poll child") {
            if rss_samples == 0 {
                exited_fast = true;
            }
            final_status = Some(status);
            reaped = true;
            break;
        }
        ticks += 1;
        if ticks == 1 || ticks.is_multiple_of(10) {
            match sample_child_rss(rss_pid) {
                Ok(rss) => {
                    rss_samples += 1;
                    peak_rss_kb = peak_rss_kb.max(rss);
                    if rss > IR_RSS_CAP_KIB {
                        child.kill().expect("kill RSS overrun child");
                        rss_exceeded = true;
                        break;
                    }
                }
                Err(e) => {
                    // The pid may have vanished between try_wait and ps;
                    // re-check before treating this as a policing fault.
                    if let Some(status) = child.try_wait().expect("re-poll child") {
                        if rss_samples == 0 {
                            exited_fast = true;
                        }
                        final_status = Some(status);
                        reaped = true;
                        break;
                    }
                    panic!("RSS policing failed on live child {rss_pid}: {e}");
                }
            }
        }
        if std::time::Instant::now() >= deadline {
            child.kill().expect("kill overrun child");
            overran = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    if final_status.is_none() {
        let mut poll = || child.try_wait();
        match reap_with_grace(&mut poll, Duration::from_secs(5)) {
            Some(status) => {
                final_status = Some(status);
                reaped = true;
            }
            None => {
                reaped = false;
            }
        }
    }
    // Finite reader completion shares the absolute budget: pipes hit EOF
    // once the reaped child is gone. See ir_join_capped on unfinished.
    let join_deadline = start + Duration::from_secs(wall_secs + 15);
    let (out_kept, out_over, out_err, out_done) = ir_join_capped(out_reader, join_deadline);
    let (err_kept, err_over, err_err, err_done) = ir_join_capped(err_reader, join_deadline);
    let incomplete_readers = !(out_done && err_done);
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
        exit_code: final_status.and_then(|s| s.code()),
        stdout: String::from_utf8_lossy(&out_kept).into_owned(),
        stderr,
        timed_out: overran,
        oversized: out_over || err_over,
        incomplete_readers,
        read_error: out_err || err_err,
        reaped,
        limits,
        artifact,
        leftovers,
        file_unit,
        rss_pid,
        rss_samples,
        peak_rss_kb,
        rss_exceeded,
        exited_fast,
    }
}

/// Effective-limits assertions shared by bounded input-shape runs: exact raw
/// round-trips everywhere; kernel byte proofs on Linux, explicit unsupported
/// markers elsewhere (fail-closed on any unit surprise).
/// Effective-bounds assertions shared by bounded input-shape runs: exact raw
/// round-trips everywhere; kernel byte proofs on Linux (including the
/// shell-unit factor for `-f`); explicit unsupported markers elsewhere
/// (fail-closed on any unit surprise); RSS policing receipts (a real pid,
/// no exceedance, peak under the cutoff, samples taken unless the child
/// exited before the first poll).
fn assert_effective_bounds(r: &InputShapeRun) {
    let limits = &r.limits;
    assert_eq!(limits.stack_raw, "4096", "stack cap round-trip");
    assert_eq!(limits.cpu_raw, "10", "cpu cap round-trip");
    assert_eq!(
        limits.fileblk_raw,
        IR_FILE_BLOCKS.to_string(),
        "file cap round-trip"
    );
    #[cfg(target_os = "linux")]
    {
        assert_eq!(
            limits.mem_kb_raw,
            IR_AS_KIB.to_string(),
            "AS cap round-trip"
        );
        assert_eq!(limits.mem_bytes, Some(IR_AS_KIB * 1024), "AS bytes proof");
        assert!(
            r.file_unit.is_some(),
            "file unit factor must be attested on Linux"
        );
    }
    #[cfg(not(target_os = "linux"))]
    {
        assert_eq!(limits.mem_kb_raw, "unsupported", "no AS enforcement here");
        assert_eq!(limits.mem_bytes, None, "no AS byteproof here");
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
        "peak RSS {} KiB over cutoff",
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
#[cfg(unix)]
#[test]
fn original_shape_bounded_child_default_limits() {
    let r = run_input_shape_bounded(&format!(":{}", " 3</dev/zero".repeat(1024)), 90, "ir-1024");
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
        "ulimit -v 1572864 || exit 99",
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
    let full = parse_ir_limits_note("ir-limits: stack=4096 cpu=10 mem_kb=1572864 mem_bytes=1610612736 fileblk=131072 file_bytes=134217728").unwrap();
    assert_eq!(full.stack_raw, "4096");
    assert_eq!(full.cpu_raw, "10");
    assert_eq!(full.mem_kb_raw, "1572864");
    assert_eq!(full.mem_bytes, Some(1610612736));
    assert_eq!(full.fileblk_raw, "131072");
    assert_eq!(full.file_bytes, Some(134217728));
    let unsupported = parse_ir_limits_note("ir-limits: stack=4096 cpu=10 mem_kb=unsupported mem_bytes=unsupported fileblk=131072 file_bytes=unsupported").unwrap();
    assert_eq!(unsupported.mem_bytes, None);
    assert_eq!(unsupported.file_bytes, None);
    assert!(parse_ir_limits_note("bogus line").is_err());
    assert!(parse_ir_limits_note("ir-limits: stack=4096 cpu=10").is_err());
    assert!(parse_ir_limits_note("ir-limits: stack=4096 cpu=10 mem_kb=1572864 mem_bytes=1610612736 fileblk=131072 file_bytes=nope").is_err());
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
#[cfg(unix)]
#[test]
fn ir_helper_normal_child_completes() {
    let r = run_input_shape_bounded("true", 30, "ir-true");
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
/// deleting (possibly another owner's contents).
#[test]
fn ir_helper_workdir_collision_preserves() {
    let tag = "ir-collision-probe";
    let path = std::env::temp_dir().join(format!("bashkit-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).expect("plant collision dir");
    std::fs::write(path.join("precious"), b"precious").expect("plant collision file");
    let err = TempWorkdir::create(tag).expect_err("collision must fail");
    assert_eq!(err.kind(), std::io::ErrorKind::AlreadyExists);
    assert_eq!(
        std::fs::read(path.join("precious")).expect("preexisting contents preserved"),
        b"precious"
    );
    std::fs::remove_dir_all(&path).expect("remove planted dir");
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
struct OwnedChild {
    inner: Option<std::process::Child>,
}

impl OwnedChild {
    fn new(child: std::process::Child) -> Self {
        Self { inner: Some(child) }
    }

    fn id(&self) -> u32 {
        self.inner.as_ref().expect("child already released").id()
    }

    fn take_pipes(
        &mut self,
    ) -> (
        Option<std::process::ChildStdout>,
        Option<std::process::ChildStderr>,
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

    fn kill(&mut self) -> std::io::Result<()> {
        self.inner.as_mut().expect("child already released").kill()
    }
}

impl Drop for OwnedChild {
    fn drop(&mut self) {
        if let Some(child) = self.inner.as_mut() {
            let _ = child.kill();
        }
    }
}
