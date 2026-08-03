//! Snapshot history benchmark: what a per-message snapshot actually costs.
//!
//! Measures the two questions that decide whether storing a snapshot per
//! conversation turn is affordable:
//!
//! 1. **Time** — encode and restore latency for the v1 JSON format, the v2
//!    packed container, and an incremental commit against a warm store.
//! 2. **Size** — bytes on the wire for the same state in each format, and the
//!    marginal cost of one more commit after a small edit.
//!
//! The size table is printed once at startup, since criterion only reports
//! time. Results belong in `crates/bashkit/benches/results/` as
//! `criterion-snapshot-history-<moniker>-<timestamp>.md`.

use bashkit::{Bash, CheckoutPolicy, CommitOptions, ObjectId, SnapshotOptions};
use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use std::collections::HashMap;
use tokio::runtime::Runtime;

type Store = HashMap<ObjectId, Vec<u8>>;

/// Workspace sizes, in files. A conversational agent's sandbox usually sits at
/// the low end; the large case shows how the cost scales.
const FILE_COUNTS: &[usize] = &[10, 100, 500];

/// Build a workspace with `files` text files plus one large file, so both the
/// inline and chunked paths are exercised.
fn workspace(rt: &Runtime, files: usize) -> Bash {
    let mut bash = Bash::builder()
        .limits(bashkit::ExecutionLimits::new().max_commands(1_000_000))
        .build();
    rt.block_on(async {
        bash.exec("mkdir -p /w/src /w/docs").await.unwrap();
        for i in 0..files {
            bash.exec(&format!(
                "echo 'module {i}: the quick brown fox jumps over the lazy dog' > /w/src/f{i}.txt"
            ))
            .await
            .unwrap();
        }
        // One file big enough to chunk, so chunk-level dedup is measured too.
        bash.exec("seq 1 20000 > /w/docs/big.txt").await.unwrap();
    });
    bash
}

/// Seed a store with a full commit and return it alongside the commit id.
fn warm_store(bash: &Bash) -> Store {
    let packed = bash.commit(CommitOptions::new()).unwrap();
    packed.into_objects().collect()
}

/// Print the size comparison the timing benchmarks cannot show.
fn report_sizes(rt: &Runtime) {
    println!("\n=== snapshot size comparison ===");
    println!(
        "{:>6} | {:>10} | {:>10} | {:>7} | {:>14}",
        "files", "v1 json", "v2 packed", "ratio", "incremental"
    );
    println!(
        "{:->6}-+-{:->10}-+-{:->10}-+-{:->7}-+-{:->14}",
        "", "", "", "", ""
    );

    for &files in FILE_COUNTS {
        let mut bash = workspace(rt, files);

        let v1 = bash
            .snapshot_state(SnapshotOptions::default())
            .to_bytes()
            .unwrap()
            .len();
        let v2 = bash.snapshot().unwrap().len();

        // Cost of the next commit after touching a single file — the number
        // that decides whether per-message history is affordable.
        let store = warm_store(&bash);
        rt.block_on(async {
            bash.exec("echo 'edited' >> /w/src/f0.txt").await.unwrap();
        });
        let incremental = bash
            .commit(CommitOptions::new().have(store.keys()))
            .unwrap()
            .stored_bytes();

        println!(
            "{files:>6} | {v1:>10} | {v2:>10} | {:>6.2}x | {incremental:>14}",
            v1 as f64 / v2 as f64
        );
    }
    println!();
}

fn bench_capture(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    report_sizes(&rt);

    let mut group = c.benchmark_group("snapshot_capture");
    for &files in FILE_COUNTS {
        let bash = workspace(&rt, files);
        group.throughput(Throughput::Elements(files as u64));

        group.bench_with_input(BenchmarkId::new("v1_json", files), &files, |b, _| {
            b.iter(|| {
                bash.snapshot_state(SnapshotOptions::default())
                    .to_bytes()
                    .unwrap()
            });
        });

        group.bench_with_input(BenchmarkId::new("v2_packed", files), &files, |b, _| {
            b.iter(|| bash.snapshot().unwrap());
        });

        // The steady-state case for session history: a warm store, so only
        // changed content is encoded.
        let store = warm_store(&bash);
        group.bench_with_input(BenchmarkId::new("v2_incremental", files), &files, |b, _| {
            b.iter(|| {
                bash.commit(CommitOptions::new().have(store.keys()))
                    .unwrap()
            });
        });
    }
    group.finish();
}

fn bench_restore(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("snapshot_restore");

    for &files in FILE_COUNTS {
        let bash = workspace(&rt, files);
        group.throughput(Throughput::Elements(files as u64));

        let v1 = bash
            .snapshot_state(SnapshotOptions::default())
            .to_bytes()
            .unwrap();
        group.bench_with_input(BenchmarkId::new("v1_json", files), &files, |b, _| {
            b.iter(|| {
                let mut target = Bash::new();
                target
                    .restore_snapshot_with_policy(&v1, CheckoutPolicy::Force)
                    .unwrap();
            });
        });

        let v2 = bash.snapshot().unwrap();
        group.bench_with_input(BenchmarkId::new("v2_packed", files), &files, |b, _| {
            b.iter(|| {
                let mut target = Bash::new();
                target
                    .restore_snapshot_with_policy(&v2, CheckoutPolicy::Force)
                    .unwrap();
            });
        });

        // Checkout from a store, which is how a fork or a rewind is served.
        let store = warm_store(&bash);
        let id = bash.commit(CommitOptions::new()).unwrap().id();
        group.bench_with_input(BenchmarkId::new("v2_checkout", files), &files, |b, _| {
            b.iter(|| {
                let mut target = Bash::new();
                target.checkout(id, &store, CheckoutPolicy::Force).unwrap();
            });
        });
    }
    group.finish();
}

fn bench_history_ops(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("snapshot_history_ops");

    let mut bash = workspace(&rt, 100);
    let mut store = Store::new();
    let first = {
        let packed = bash.commit(CommitOptions::new()).unwrap();
        let id = packed.id();
        store.extend(packed.into_objects());
        id
    };
    rt.block_on(async {
        bash.exec("echo more >> /w/src/f1.txt; rm /w/src/f2.txt; echo new > /w/src/added.txt")
            .await
            .unwrap();
    });
    let second = {
        let packed = bash
            .commit(CommitOptions::new().parent(first).have(store.keys()))
            .unwrap();
        let id = packed.id();
        store.extend(packed.into_objects());
        id
    };

    // Diff compares content addresses, so it never reads file content.
    group.bench_function("diff", |b| {
        b.iter(|| bashkit::SnapshotGraph::diff(first, second, &store).unwrap());
    });
    group.bench_function("plan_checkout_warm", |b| {
        b.iter(|| bashkit::SnapshotGraph::plan_checkout(second, &store).unwrap());
    });
    group.bench_function("reachable", |b| {
        b.iter(|| bashkit::SnapshotGraph::reachable(second, &store).unwrap());
    });

    group.finish();
}

criterion_group!(benches, bench_capture, bench_restore, bench_history_ops);
criterion_main!(benches);
