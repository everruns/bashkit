use bashkit::{Bash, ExecutionLimits};

#[tokio::test]
async fn shuf_range_head_count_does_not_materialize_full_range() {
    let limits = ExecutionLimits::new().max_stdout_bytes(64);
    let mut bash = Bash::builder().limits(limits).build();

    let result = bash
        .exec("shuf -i 1-18446744073709551615 -n 1")
        .await
        .unwrap();

    assert_eq!(result.exit_code, 0);
    assert!(!result.stdout_truncated);
    let value = result.stdout.trim().parse::<u64>().unwrap();
    assert!(value >= 1);
}

#[tokio::test]
async fn shuf_repeat_head_count_is_checked_before_output_allocation() {
    let limits = ExecutionLimits::new().max_stdout_bytes(16);
    let mut bash = Bash::builder().limits(limits).build();

    let result = bash.exec("shuf -r -e x -n 50").await.unwrap();

    assert_eq!(result.exit_code, 1);
    assert!(!result.stdout_truncated);
    assert!(result.stdout.is_empty());
    assert!(result.stderr.contains("output too large"));
}
