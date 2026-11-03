//! `Execution::output_file`: combined output tee into a VFS file with its own
//! byte cap, independent of the in-memory stdout/stderr caps.

use bashkit::{Bash, ExecutionLimits};
use std::path::Path;

#[tokio::test]
async fn output_file_keeps_output_beyond_stdout_cap_and_shell_state() {
    let limits = ExecutionLimits::default().max_stdout_bytes(1024);
    let mut bash = Bash::builder().limits(limits).build();
    let result = bash
        .execute("export KEPT=1; cd /tmp; seq 1 20000; echo tail-marker >&2")
        .capture(64)
        .output_file("/tmp/out.log", 10 * 1024 * 1024)
        .await
        .unwrap();
    assert_eq!(result.exit_code, 0);
    // In-memory delivery still honors the 1 KiB stdout cap.
    assert!(result.stdout_truncated);
    assert!(result.stdout.len() <= 64);
    let file = bash
        .fs()
        .read_file(Path::new("/tmp/out.log"))
        .await
        .unwrap();
    let text = String::from_utf8(file).unwrap();
    assert!(text.starts_with("1\n2\n3\n"));
    assert!(text.contains("\n20000\n"));
    assert!(text.contains("tail-marker\n"));
    assert!(text.len() > 100_000);
    // Runs in the current shell: no subshell, state carries over.
    let next = bash.exec("echo $KEPT $PWD").await.unwrap();
    assert_eq!(next.stdout, "1 /tmp\n");
}

#[tokio::test]
async fn output_file_respects_its_cap_and_truncates_previous_content() {
    let mut bash = Bash::new();
    bash.exec("echo stale-content-that-is-long > /tmp/out.log")
        .await
        .unwrap();
    let result = bash
        .execute("seq 1 1000; exit 3")
        .output_file("/tmp/out.log", 100)
        .await
        .unwrap();
    assert_eq!(result.exit_code, 3);
    let file = bash
        .fs()
        .read_file(Path::new("/tmp/out.log"))
        .await
        .unwrap();
    assert_eq!(file.len(), 100);
    assert!(file.starts_with(b"1\n2\n"));
}

#[tokio::test]
async fn output_file_is_bounded_by_fs_limits_without_failing_the_call() {
    let fs_limits = bashkit::FsLimits::new().max_file_size(1000);
    let fs = std::sync::Arc::new(bashkit::InMemoryFs::with_limits(fs_limits));
    let mut bash = Bash::builder().fs(fs).build();
    bash.exec("mkdir -p /tmp").await.unwrap();
    let result = bash
        .execute("seq 1 10000; echo ok")
        .output_file("/tmp/out.log", 1_000_000)
        .await
        .unwrap();
    assert_eq!(result.exit_code, 0);
    let file = bash
        .fs()
        .read_file(Path::new("/tmp/out.log"))
        .await
        .unwrap();
    assert!(file.len() <= 1000);
}

#[tokio::test]
async fn output_file_rejects_relative_and_missing_parent_paths() {
    let mut bash = Bash::new();
    assert!(
        bash.execute("echo hi")
            .output_file("out.log", 10)
            .await
            .is_err()
    );
    assert!(
        bash.execute("echo hi")
            .output_file("/missing/dir/out.log", 10)
            .await
            .is_err()
    );
    // Session stays usable.
    assert_eq!(bash.exec("echo ok").await.unwrap().stdout, "ok\n");
}
