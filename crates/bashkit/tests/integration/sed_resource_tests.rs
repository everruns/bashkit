use bashkit::{Bash, ExecutionLimits};

#[tokio::test]
async fn repeated_read_stdin_stops_at_stdout_limit() {
    let limits = ExecutionLimits::new().max_stdout_bytes(64);
    let mut bash = Bash::builder().limits(limits).build();

    let result = bash
        .exec("printf 'a\\na\\na\\na\\na\\na\\na\\na\\n' | sed 'r /dev/stdin'")
        .await
        .unwrap();

    assert_eq!(result.exit_code, 1);
    assert!(result.stdout.is_empty());
    assert!(result.stderr.contains("configured stdout limit"));
}

#[tokio::test]
async fn read_stdin_preserves_output_below_limit() {
    let limits = ExecutionLimits::new().max_stdout_bytes(64);
    let mut bash = Bash::builder().limits(limits).build();

    let result = bash
        .exec("printf 'a\\nb\\n' | sed 'r /dev/stdin'")
        .await
        .unwrap();

    assert_eq!(result.exit_code, 0);
    assert_eq!(result.stdout, "a\na\nb\nb\na\nb\n");
    assert!(result.stderr.is_empty());
}
