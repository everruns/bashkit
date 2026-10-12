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
/// same-fd repeats hold only one 64B lease under a 64B live cap.
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
/// Every reachable cloning/retaining numbered-input branch charges: DupInput
/// clone and ReadWrite each trip a 31B aggregate cap (16B payload retains
/// 32B). (`3<<<` drops its fd in the parser and always feeds stdin, and
/// numbered heredoc bodies are parse-metered with no per-exec retention, so
/// those arms keep their charges as defense-in-depth.)
async fn all_numbered_input_branches_charge() {
    let cases = [": 3</big 4<&3", ": 3<>/big"];
    for script in cases {
        let mut bash = Bash::builder()
            .limits(ExecutionLimits::new().max_aggregate_input_bytes(31))
            .build();
        bash.fs()
            .write_file(Path::new("/big"), b"sixteen-bytesxx")
            .await
            .unwrap();
        let result = bash.exec(script).await;
        assert!(
            matches!(
                result,
                Err(Error::ResourceLimit(LimitExceeded::ExecutionBudget(
                    ExecutionBudgetExceeded::InputBytes { .. }
                )))
            ),
            "script {script:?} should trip the input budget, got {result:?}"
        );
    }
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
