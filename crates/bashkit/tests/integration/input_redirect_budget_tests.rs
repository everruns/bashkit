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

/// Original shape under default limits in an owned bounded child: `:` plus
/// 1024 same-fd `/dev/zero` redirects is denied quickly by the aggregate
/// input budget (each 1MiB read retains ~2MiB against the 100MB default cap,
/// tripping after ~50 redirects) instead of piling ~2GiB of retained clones.
/// The harness asserts the exact denial: clean exit code 1 naming `aggregate
/// input bytes` (production retention admission, far above the ~7KiB script
/// text), which distinguishes budget denial from OOM/signal death (no exit
/// code) and from pre-fix success (exit 0). The 1536MiB RLIMIT_AS
/// additionally caps any regression to append-only retention. The child is
/// always reaped, killed first on wall-clock expiry. Unix-only: needs bash +
/// ulimit for enforcement.
#[cfg(unix)]
#[tokio::test]
async fn original_shape_bounded_child_default_limits() {
    let script = format!(":{}", " 3</dev/zero".repeat(1024));
    // Controlled sink for instrumented-coverage artifacts: under coverage
    // jobs the spawned CLI inherits LLVM_PROFILE_FILE and flushes a
    // multi-MiB profraw at exit, which must not share the producer tight
    // file cap (SIGXFSZ) nor pollute the workspace. Coverage stays enabled;
    // only the sink location is controlled, and the artifact size gets an
    // explicit bounded receipt below.
    let sink_dir = std::env::temp_dir().join(format!("bashkit-ir-child-{}", std::process::id()));
    std::fs::create_dir_all(&sink_dir).expect("artifact sink dir");
    let profile = sink_dir.join("child.profraw");
    let outcome = tokio::task::spawn_blocking(move || {
        let exe = std::env::current_exe().expect("test executable path");
        let cli = exe
            .ancestors()
            .flat_map(|dir| [dir.join(format!("bashkit{}", std::env::consts::EXE_SUFFIX))])
            .find(|path| path.is_file())
            .expect("build the CLI first: cargo build -p bashkit-cli");
        // rlimits apply to the launched CLI only. -f is an explicit 128MiB
        // artifact allowance for one short instrumented run (profraws are
        // O(MB)); the producer itself writes no files (VFS in-memory,
        // stdout/stderr piped), so anything near the cap is a regression.
        // Pipes are kernel-bounded, not FSIZE-capped: the poll loop
        // plus kill-on-deadline keeps collection finite either way.
        let launcher =
            "ulimit -v 1572864; ulimit -t 120; ulimit -s 8192; ulimit -f 2048; exec \"$@\"";
        let mut child = std::process::Command::new("bash")
            .args([
                "-c",
                launcher,
                "ir-child",
                &cli.to_string_lossy(),
                "-c",
                &script,
            ])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .env("LLVM_PROFILE_FILE", &profile)
            .spawn()
            .expect("spawn bounded child");
        let start = std::time::Instant::now();
        let deadline = std::time::Duration::from_secs(100);
        // wait_with_output runs only on an already-exited child, so
        // collection is limited to kernel pipe buffers; the deadline branch
        // kills first, then reaps. Either path returns promptly, and the
        // artifact sink is cleaned below.
        let (status, output, elapsed, overran) = loop {
            if let Some(status) = child.try_wait().expect("poll child") {
                let output = child.wait_with_output().expect("reap child");
                break (status, output, start.elapsed(), false);
            }
            if start.elapsed() > deadline {
                child.kill().expect("kill overrun child");
                let _ = child.wait();
                break (
                    {
                        use std::os::unix::process::ExitStatusExt;
                        std::process::ExitStatus::from_raw(9)
                    },
                    std::process::Output {
                        status: {
                            use std::os::unix::process::ExitStatusExt;
                            std::process::ExitStatus::from_raw(9)
                        },
                        stdout: Vec::new(),
                        stderr: b"wall-clock overrun".to_vec(),
                    },
                    start.elapsed(),
                    true,
                );
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        };
        // Artifact receipt (when coverage writes one) plus finite cleanup.
        const ARTIFACT_BOUND: u64 = 128 * 1024 * 1024;
        if let Ok(meta) = std::fs::metadata(&profile) {
            assert!(
                meta.len() < ARTIFACT_BOUND,
                "coverage artifact receipt: {} bytes exceeds {}",
                meta.len(),
                ARTIFACT_BOUND
            );
        }
        let _ = std::fs::remove_dir_all(sink_dir);
        (status, output, elapsed, overran)
    })
    .await
    .expect("child harness join");
    let (status, output, elapsed, overran) = outcome;
    assert!(
        !overran,
        "bounded child overran the wall clock; killed and reaped"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        status.code(),
        Some(1),
        // Signal death here means OOM kill, an artifact-cap trip (SIGXFSZ),
        // or a crash, never success; producer diagnostics included.
        "producer must fail cleanly, got {status:?} with stderr {stderr:?}",
    );
    assert!(
        stderr.contains("aggregate input bytes"),
        "denial must name the aggregate input budget phase, got: {stderr:?}"
    );
    assert!(output.stdout.len() < 4096, "producer output capped");
    assert!(
        elapsed < std::time::Duration::from_secs(60),
        "producer took {elapsed:?}"
    );
}
