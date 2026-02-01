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
const SESSION_COUNTS: &[usize] = &[10, 50, 100, 200];

/// Heavy workload: file creation, text processing with grep/awk/sed
const HEAVY_SCRIPT: &str = r#"
# Create a data file with 100 lines
for i in 1 2 3 4 5 6 7 8 9 10; do
    for j in 1 2 3 4 5 6 7 8 9 10; do
        echo "line $i$j: name=user$i value=$((i * j)) status=active"
    done
done > /tmp/data.txt

# grep: filter lines containing "value=2"
grep "value=2" /tmp/data.txt > /tmp/filtered.txt

# awk: extract and sum values
total=$(awk -F'=' '{sum += $3} END {print sum}' /tmp/data.txt)
echo "total: $total"

# sed: transform data
sed 's/status=active/status=PROCESSED/g' /tmp/data.txt > /tmp/processed.txt

# Count results
lines=$(cat /tmp/processed.txt | grep -c PROCESSED)
echo "processed: $lines lines"
"#;

/// Medium workload: file ops + grep
const MEDIUM_SCRIPT: &str = r#"
# Generate log-like data
for i in 1 2 3 4 5 6 7 8 9 10; do
    echo "2024-01-$i INFO Request processed in ${i}0ms"
    echo "2024-01-$i WARN Slow query detected"
    echo "2024-01-$i ERROR Connection timeout"
done > /tmp/log.txt

# Analyze logs
errors=$(grep -c ERROR /tmp/log.txt)
warns=$(grep -c WARN /tmp/log.txt)
echo "Errors: $errors, Warnings: $warns"

# Extract timing with awk
avg=$(awk '/processed/ {gsub(/ms/, "", $NF); sum += $NF; count++} END {print sum/count}' /tmp/log.txt)
echo "Avg response: ${avg}ms"
"#;

/// Light workload: simple arithmetic loop
const LIGHT_SCRIPT: &str = r#"
x=0
for i in 1 2 3 4 5; do
    x=$((x + i))
done
echo $x
"#;

/// Run script N times sequentially
async fn run_sequential(n: usize, script: &'static str) {
    for _ in 0..n {
        let mut bash = Bash::new();
        let _ = bash.exec(script).await;
    }
}

/// Run script N times in parallel
async fn run_parallel(n: usize, script: &'static str) {
    let handles: Vec<_> = (0..n)
        .map(|_| {
            tokio::spawn(async move {
                let mut bash = Bash::new();
                let _ = bash.exec(script).await;
            })
        })
        .collect();

    for handle in handles {
        let _ = handle.await;
    }
}

/// Run N sessions with shared filesystem - each writes unique files
async fn run_parallel_shared_fs(n: usize) {
    let fs: Arc<dyn bashkit::FileSystem> = Arc::new(InMemoryFs::new());

    let handles: Vec<_> = (0..n)
        .map(|i| {
            let fs = Arc::clone(&fs);
            tokio::spawn(async move {
                let script = format!(
                    r#"
# Create unique data file for this session
for j in 1 2 3 4 5 6 7 8 9 10; do
    echo "session_{i} line_$j value=$((j * {i}))"
done > /tmp/session_{i}.txt

# Process with grep and awk
count=$(grep -c "session_{i}" /tmp/session_{i}.txt)
sum=$(awk -F= '{{s+=$2}} END {{print s}}' /tmp/session_{i}.txt)
echo "session {i}: $count lines, sum=$sum"
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

fn bench_workload_comparison(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();

    let mut group = c.benchmark_group("workload_types");
    group.sample_size(50); // Fewer samples for heavy workloads

    // Compare workload types at fixed session count
    let n = 50;
    group.throughput(Throughput::Elements(n as u64));

    group.bench_function("light_sequential", |b| {
        b.to_async(&rt).iter(|| run_sequential(n, LIGHT_SCRIPT));
    });

    group.bench_function("light_parallel", |b| {
        b.to_async(&rt).iter(|| run_parallel(n, LIGHT_SCRIPT));
    });

    group.bench_function("medium_sequential", |b| {
        b.to_async(&rt).iter(|| run_sequential(n, MEDIUM_SCRIPT));
    });

    group.bench_function("medium_parallel", |b| {
        b.to_async(&rt).iter(|| run_parallel(n, MEDIUM_SCRIPT));
    });

    group.bench_function("heavy_sequential", |b| {
        b.to_async(&rt).iter(|| run_sequential(n, HEAVY_SCRIPT));
    });

    group.bench_function("heavy_parallel", |b| {
        b.to_async(&rt).iter(|| run_parallel(n, HEAVY_SCRIPT));
    });

    group.finish();
}

fn bench_parallel_scaling(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();

    let mut group = c.benchmark_group("parallel_scaling");
    group.sample_size(30);

    for &n in SESSION_COUNTS {
        group.throughput(Throughput::Elements(n as u64));

        group.bench_with_input(BenchmarkId::new("medium_seq", n), &n, |b, &n| {
            b.to_async(&rt).iter(|| run_sequential(n, MEDIUM_SCRIPT));
        });

        group.bench_with_input(BenchmarkId::new("medium_par", n), &n, |b, &n| {
            b.to_async(&rt).iter(|| run_parallel(n, MEDIUM_SCRIPT));
        });

        group.bench_with_input(BenchmarkId::new("shared_fs", n), &n, |b, &n| {
            b.to_async(&rt).iter(|| run_parallel_shared_fs(n));
        });
    }

    group.finish();
}

fn bench_single_operations(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();

    c.bench_function("single_bash_new", |b| {
        b.iter(|| {
            let _ = Bash::new();
        });
    });

    c.bench_function("single_echo", |b| {
        b.to_async(&rt).iter(|| async {
            let mut bash = Bash::new();
            let _ = bash.exec("echo hello").await;
        });
    });

    c.bench_function("single_file_write_read", |b| {
        b.to_async(&rt).iter(|| async {
            let mut bash = Bash::new();
            let _ = bash
                .exec("echo 'test data' > /tmp/test.txt; cat /tmp/test.txt")
                .await;
        });
    });

    c.bench_function("single_grep", |b| {
        b.to_async(&rt).iter(|| async {
            let mut bash = Bash::new();
            let _ = bash
                .exec(
                    r#"
echo -e "foo\nbar\nbaz" > /tmp/t.txt
grep bar /tmp/t.txt
"#,
                )
                .await;
        });
    });

    c.bench_function("single_awk", |b| {
        b.to_async(&rt).iter(|| async {
            let mut bash = Bash::new();
            let _ = bash
                .exec(
                    r#"
echo -e "1 10\n2 20\n3 30" > /tmp/t.txt
awk '{sum += $2} END {print sum}' /tmp/t.txt
"#,
                )
                .await;
        });
    });

    c.bench_function("single_sed", |b| {
        b.to_async(&rt).iter(|| async {
            let mut bash = Bash::new();
            let _ = bash
                .exec(
                    r#"
echo "hello world" > /tmp/t.txt
sed 's/world/universe/' /tmp/t.txt
"#,
                )
                .await;
        });
    });

    c.bench_function("single_light_script", |b| {
        b.to_async(&rt).iter(|| async {
            let mut bash = Bash::new();
            let _ = bash.exec(LIGHT_SCRIPT).await;
        });
    });

    c.bench_function("single_medium_script", |b| {
        b.to_async(&rt).iter(|| async {
            let mut bash = Bash::new();
            let _ = bash.exec(MEDIUM_SCRIPT).await;
        });
    });

    c.bench_function("single_heavy_script", |b| {
        b.to_async(&rt).iter(|| async {
            let mut bash = Bash::new();
            let _ = bash.exec(HEAVY_SCRIPT).await;
        });
    });
}

/// Verify scripts execute correctly
#[test]
fn verify_light_script() {
    let rt = Runtime::new().unwrap();
    rt.block_on(async {
        let mut bash = Bash::new();
        let result = bash.exec(LIGHT_SCRIPT).await.unwrap();
        assert_eq!(result.stdout.trim(), "15", "1+2+3+4+5 = 15");
    });
}

#[test]
fn verify_medium_script() {
    let rt = Runtime::new().unwrap();
    rt.block_on(async {
        let mut bash = Bash::new();
        let result = bash.exec(MEDIUM_SCRIPT).await.unwrap();
        assert!(
            result.stdout.contains("Errors: 10"),
            "Should find 10 ERROR lines: {}",
            result.stdout
        );
        assert!(
            result.stdout.contains("Warnings: 10"),
            "Should find 10 WARN lines: {}",
            result.stdout
        );
    });
}

#[test]
fn verify_heavy_script() {
    let rt = Runtime::new().unwrap();
    rt.block_on(async {
        let mut bash = Bash::new();
        let result = bash.exec(HEAVY_SCRIPT).await.unwrap();
        assert!(
            result.stdout.contains("processed: 100 lines"),
            "Should process 100 lines: {}",
            result.stdout
        );
    });
}

criterion_group!(
    benches,
    bench_workload_comparison,
    bench_parallel_scaling,
    bench_single_operations
);
criterion_main!(benches);
