//! Large parallel fan-out tests.
//!
//! A bashkit session is a plain heap object + tokio task — no per-session OS
//! process or thread (see `specs/parallel-execution.md`, L-PROC-003). These
//! tests confirm a large fan-out (1000 sessions) actually does real work and
//! produces correct output, rather than spawning and returning instantly
//! because every session errored out (e.g. hit a limit). The timing of this
//! fan-out is benchmarked separately in `benches/parallel_execution.rs`.

use bashkit::{Bash, FileSystem, InMemoryFs};
use std::sync::Arc;

/// 1000 parallel sessions, each with its own `Bash` instance but sharing one
/// `Arc<dyn FileSystem>`. Each session must succeed and compute the right sum.
#[tokio::test(flavor = "multi_thread")]
async fn thousand_parallel_sessions_do_real_work() {
    const N: usize = 1000;
    let fs: Arc<dyn FileSystem> = Arc::new(InMemoryFs::new());

    let handles: Vec<_> = (0..N)
        .map(|i| {
            let fs = Arc::clone(&fs);
            tokio::spawn(async move {
                // Write a unique file, then sum its values.
                // Expected sum = (1+2+...+10) * i = 55 * i.
                let script = format!(
                    r#"
for j in 1 2 3 4 5 6 7 8 9 10; do
    echo "value=$((j * {i}))"
done > /tmp/session_{i}.txt
awk -F= '{{s+=$2}} END {{print s}}' /tmp/session_{i}.txt
"#
                );
                let mut bash = Bash::builder().fs(fs).build();
                let result = bash.exec(&script).await.expect("session must succeed");
                (i, result.exit_code, result.stdout.trim().to_string())
            })
        })
        .collect();

    let mut completed = 0;
    for handle in handles {
        let (i, exit_code, stdout) = handle.await.expect("task must not panic");
        assert_eq!(exit_code, 0, "session {i} should exit 0");
        assert_eq!(stdout, (55 * i).to_string(), "session {i} wrong sum");
        completed += 1;
    }
    assert_eq!(completed, N, "all {N} sessions must complete");
}

/// Sessions sharing one filesystem must not corrupt each other's files: each
/// writes to a distinct path and reads back exactly what it wrote.
#[tokio::test(flavor = "multi_thread")]
async fn parallel_sessions_shared_fs_no_cross_contamination() {
    const N: usize = 500;
    let fs: Arc<dyn FileSystem> = Arc::new(InMemoryFs::new());

    let handles: Vec<_> = (0..N)
        .map(|i| {
            let fs = Arc::clone(&fs);
            tokio::spawn(async move {
                let script = format!("echo marker-{i} > /tmp/f_{i}.txt; cat /tmp/f_{i}.txt");
                let mut bash = Bash::builder().fs(fs).build();
                let out = bash.exec(&script).await.expect("session must succeed");
                (i, out.stdout.trim().to_string())
            })
        })
        .collect();

    for handle in handles {
        let (i, stdout) = handle.await.expect("task must not panic");
        assert_eq!(stdout, format!("marker-{i}"), "session {i} saw wrong file");
    }
}
