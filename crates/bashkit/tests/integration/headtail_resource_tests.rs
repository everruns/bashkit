//! TM-DOS-117: line selection must not allocate an offset per input line.

use std::path::Path;

use bashkit::{Bash, ExecutionLimits};

#[tokio::test]
async fn newline_dense_head_succeeds_with_tight_intermediate_budget() {
    let mut bash = Bash::builder()
        .limits(ExecutionLimits::new().max_live_intermediate_bytes(1_024))
        .build();
    bash.fs()
        .write_file(Path::new("/dense"), &vec![b'\n'; 1_000_000])
        .await
        .unwrap();

    let result = bash.exec("head -n 1 /dense").await.unwrap();

    assert_eq!(result.exit_code, 0);
    assert_eq!(result.stdout.as_bytes(), b"\n");
}
