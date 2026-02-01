//! Parallel execution benchmark for BashKit
//!
//! Tests: Can multiple Bash instances run in parallel?
//! Answer: YES - each Bash instance is independent. The Arc<dyn FileSystem>
//! allows shared filesystem access with Send + Sync bounds.
//!
//! Threading model:
//! - Single Bash instance: Sequential (uses &mut self)
//! - Multiple Bash instances: Parallel via tokio::spawn
//! - Filesystem: Thread-safe via Arc + RwLock

use bashkit::{Bash, InMemoryFs};
use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use std::sync::Arc;
use tokio::runtime::Runtime;

/// Number of parallel sessions to benchmark
const SESSION_COUNTS: &[usize] = &[10, 50, 100, 200, 500];

/// Simple script for benchmarking
const SIMPLE_SCRIPT: &str = r#"
x=0
for i in 1 2 3 4 5; do
    x=$((x + i))
done
echo $x
"#;

/// Run N bash sessions sequentially
async fn run_sequential(n: usize) {
    for _ in 0..n {
        let mut bash = Bash::new();
        let _ = bash.exec(SIMPLE_SCRIPT).await;
    }
}

/// Run N bash sessions in parallel using tokio::spawn
async fn run_parallel(n: usize) {
    let handles: Vec<_> = (0..n)
        .map(|_| {
            tokio::spawn(async move {
                let mut bash = Bash::new();
                let _ = bash.exec(SIMPLE_SCRIPT).await;
            })
        })
        .collect();

    for handle in handles {
        let _ = handle.await;
    }
}

/// Run N bash sessions in parallel with SHARED filesystem
/// This tests Arc's ability to share state across tasks
async fn run_parallel_shared_fs(n: usize) {
    let fs: Arc<dyn bashkit::FileSystem> = Arc::new(InMemoryFs::new());

    let handles: Vec<_> = (0..n)
        .map(|i| {
            let fs = Arc::clone(&fs);
            tokio::spawn(async move {
                // Each session writes to its own file to avoid conflicts
                let script = format!(
                    r#"
echo "session {i}" > /tmp/session_{i}.txt
cat /tmp/session_{i}.txt
"#
                );
                let mut bash = Bash::builder().fs(fs).build();
                let _ = bash.exec(&script).await;
            })
        })
        .collect();

    for handle in handles {
        let _ = handle.await;
    }
}

fn bench_parallel_vs_sequential(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();

    let mut group = c.benchmark_group("parallel_execution");

    for &n in SESSION_COUNTS {
        group.throughput(Throughput::Elements(n as u64));

        group.bench_with_input(BenchmarkId::new("sequential", n), &n, |b, &n| {
            b.to_async(&rt).iter(|| run_sequential(n));
        });

        group.bench_with_input(BenchmarkId::new("parallel", n), &n, |b, &n| {
            b.to_async(&rt).iter(|| run_parallel(n));
        });

        group.bench_with_input(BenchmarkId::new("parallel_shared_fs", n), &n, |b, &n| {
            b.to_async(&rt).iter(|| run_parallel_shared_fs(n));
        });
    }

    group.finish();
}

fn bench_single_session_overhead(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();

    c.bench_function("single_bash_new", |b| {
        b.iter(|| {
            let _ = Bash::new();
        });
    });

    c.bench_function("single_exec_echo", |b| {
        b.to_async(&rt).iter(|| async {
            let mut bash = Bash::new();
            let _ = bash.exec("echo hello").await;
        });
    });

    c.bench_function("single_exec_loop", |b| {
        b.to_async(&rt).iter(|| async {
            let mut bash = Bash::new();
            let _ = bash.exec(SIMPLE_SCRIPT).await;
        });
    });
}

/// Verify the benchmark script actually executes correctly
#[test]
fn verify_script_executes() {
    let rt = Runtime::new().unwrap();
    rt.block_on(async {
        let mut bash = Bash::new();
        let result = bash.exec(SIMPLE_SCRIPT).await.unwrap();
        // 1+2+3+4+5 = 15
        assert_eq!(
            result.stdout.trim(),
            "15",
            "Script must compute sum correctly"
        );
        assert_eq!(result.exit_code, 0);
    });
}

criterion_group!(
    benches,
    bench_parallel_vs_sequential,
    bench_single_session_overhead
);
criterion_main!(benches);
