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
    let payload = vec![b'x'; 64];
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
    let payload = vec![b'x'; 64];
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
