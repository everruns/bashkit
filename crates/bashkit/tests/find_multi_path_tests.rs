//! Tests for find command with multiple paths, ensuring that:
//! - Results from valid paths are not discarded when another path is missing
//! - Errors for missing paths go to stderr
//! - Exit code is 1 when any path fails, 0 when all succeed

use bashkit::Bash;

#[tokio::test]
async fn find_multi_path_second_missing() {
    let mut bash = Bash::builder().build();

    // Create files in the first path
    bash.exec("mkdir -p /tmp/path_a/.director/memory")
        .await
        .unwrap();
    bash.exec("echo data > /tmp/path_a/.director/memory/MEMORY.md")
        .await
        .unwrap();

    // Find with first path valid, second path missing
    let result = bash
        .exec(
            "find /tmp/path_a/.director/memory/ /tmp/path_b/.director/memory/ -type f 2>/dev/null",
        )
        .await
        .unwrap();

    // Should still output results from the valid first path
    assert_eq!(result.exit_code, 1);
    assert!(
        result.stdout.contains("MEMORY.md"),
        "Expected MEMORY.md in stdout, got: {:?}",
        result.stdout
    );
}

#[tokio::test]
async fn find_multi_path_first_missing() {
    let mut bash = Bash::builder().build();

    // Create files in the second path
    bash.exec("mkdir -p /tmp/path_c/.director/memory")
        .await
        .unwrap();
    bash.exec("echo data > /tmp/path_c/.director/memory/notes.md")
        .await
        .unwrap();

    // Find with first path missing, second path valid
    let result = bash
        .exec("find /tmp/path_missing /tmp/path_c/.director/memory/ -type f 2>/dev/null")
        .await
        .unwrap();

    assert_eq!(result.exit_code, 1);
    assert!(
        result.stdout.contains("notes.md"),
        "Expected notes.md in stdout, got: {:?}",
        result.stdout
    );
}

#[tokio::test]
async fn find_multi_path_all_valid() {
    let mut bash = Bash::builder().build();

    bash.exec("mkdir -p /tmp/all_a && touch /tmp/all_a/f1.txt")
        .await
        .unwrap();
    bash.exec("mkdir -p /tmp/all_b && touch /tmp/all_b/f2.txt")
        .await
        .unwrap();

    let result = bash
        .exec("find /tmp/all_a /tmp/all_b -type f | sort")
        .await
        .unwrap();

    assert_eq!(result.exit_code, 0);
    assert!(result.stdout.contains("f1.txt"), "Missing f1.txt");
    assert!(result.stdout.contains("f2.txt"), "Missing f2.txt");
}

#[tokio::test]
async fn find_missing_path_reports_stderr() {
    let mut bash = Bash::builder().build();

    // Without 2>/dev/null, error should go to stderr
    let result = bash.exec("find /tmp/does_not_exist_xyz").await.unwrap();

    assert_eq!(result.exit_code, 1);
    assert!(
        result.stderr.contains("No such file or directory"),
        "Expected error in stderr, got: {:?}",
        result.stderr
    );
}

#[tokio::test]
async fn find_multi_path_both_missing() {
    let mut bash = Bash::builder().build();

    let result = bash
        .exec("find /tmp/nope_a /tmp/nope_b -type f 2>/dev/null")
        .await
        .unwrap();

    assert_eq!(result.exit_code, 1);
    assert!(
        result.stdout.is_empty(),
        "Expected no stdout, got: {:?}",
        result.stdout
    );
}

#[tokio::test]
async fn find_single_valid_path_exit_zero() {
    let mut bash = Bash::builder().build();

    bash.exec("mkdir -p /tmp/single && touch /tmp/single/ok.txt")
        .await
        .unwrap();

    let result = bash.exec("find /tmp/single -type f").await.unwrap();

    assert_eq!(result.exit_code, 0);
    assert!(result.stdout.contains("ok.txt"));
}

#[tokio::test]
async fn find_multi_path_mixed_has_exit_one() {
    let mut bash = Bash::builder().build();

    bash.exec("mkdir -p /tmp/mixed_ok && touch /tmp/mixed_ok/x.txt")
        .await
        .unwrap();

    // One valid path + one missing = exit code 1
    let result = bash
        .exec("find /tmp/mixed_ok /tmp/mixed_bad -type f 2>/dev/null")
        .await
        .unwrap();

    assert_eq!(result.exit_code, 1);
    assert!(
        result.stdout.contains("x.txt"),
        "Valid path results should still appear"
    );
}

#[tokio::test]
async fn find_exec_with_missing_path_preserves_error_and_exec_output() {
    let mut bash = Bash::builder().build();

    bash.exec("mkdir -p /tmp/find_exec_ok && touch /tmp/find_exec_ok/y.txt")
        .await
        .unwrap();

    let result = bash
        .exec("find /tmp/find_exec_ok /tmp/find_exec_missing -name '*.txt' -exec echo {} \\;")
        .await
        .unwrap();

    assert_eq!(result.exit_code, 1);
    assert!(
        result.stdout.contains("/tmp/find_exec_ok/y.txt"),
        "Expected -exec output for valid path, got: {:?}",
        result.stdout
    );
    assert!(
        result.stderr.contains("No such file or directory"),
        "Expected missing path error in stderr, got: {:?}",
        result.stderr
    );
}
