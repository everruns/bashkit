//! TM-DOS-117: end-to-end correctness of line selection over newline-dense
//! input.
//!
//! The memory property itself is asserted by `tests/headtail_allocation_tests.rs`,
//! which counts allocations: `headtail` takes no budget lease, so an
//! `ExecutionLimits` assertion here cannot observe the offset vector and would
//! pass with or without the fix. This case covers the other half — that
//! scanning for the boundary still returns the right bytes at scale.

use std::path::Path;

use bashkit::{Bash, ExecutionLimits};

#[tokio::test]
async fn newline_dense_line_selection_is_correct_at_scale() {
    let mut bash = Bash::builder()
        .limits(ExecutionLimits::new().max_live_intermediate_bytes(1_024))
        .build();
    bash.fs()
        .write_file(Path::new("/dense"), &vec![b'\n'; 1_000_000])
        .await
        .unwrap();

    let head = bash.exec("head -n 1 /dense").await.unwrap();
    assert_eq!(head.exit_code, 0);
    assert_eq!(head.stdout.as_bytes(), b"\n");

    let tail = bash.exec("tail -n 1 /dense").await.unwrap();
    assert_eq!(tail.exit_code, 0);
    assert_eq!(tail.stdout.as_bytes(), b"\n");
}
