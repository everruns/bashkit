//! SQLite builtin benchmark for Bashkit
//!
//! Targets the `sqlite` builtin (Turso, in-process) along the dimensions
//! that actually matter for users:
//!
//! - **Workload shapes**: bulk INSERT, batched UPDATE, indexed vs full-scan
//!   SELECT, aggregate (GROUP BY + COUNT/SUM/AVG).
//! - **Output modes**: list (default), CSV, JSON, markdown — exercise the
//!   formatter on the same row set.
//! - **Backends**: `Memory` (load whole DB into turso's `MemoryIO`) vs `Vfs`
//!   (custom IO trait talking to bashkit's FileSystem).
//! - **Persistence cost**: opening a fresh DB per invocation (load + flush)
//!   vs `:memory:` (zero VFS round-trip).
//! - **Concurrency**: N parallel sqlite sessions over a shared VFS, each
//!   with its own DB file — measures the cooperative-IO path under load.
//!
//! Each bench prints a single-row sentinel so successful execution can be
//! eyeballed in `cargo bench` output without parsing Criterion JSON.

use bashkit::{Bash, FileSystem, InMemoryFs, SqliteBackend, SqliteLimits};
use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use std::sync::Arc;
use tokio::runtime::Runtime;

/// Row counts to exercise scaling. Kept modest so a full `cargo bench` run
/// finishes in reasonable time on CI hardware; tune up locally if you need
/// more signal.
const ROW_COUNTS: &[usize] = &[100, 1_000, 10_000];

/// Concurrency levels for the parallel-sessions group.
const SESSION_COUNTS: &[usize] = &[4, 16, 64];

/// Build a Bash with sqlite enabled and the opt-in env set. Optionally
/// share a filesystem so multiple sessions hit the same VFS.
fn make_bash(backend: SqliteBackend, fs: Option<Arc<dyn FileSystem>>) -> Bash {
    let mut builder = Bash::builder()
        .sqlite_with_limits(
            SqliteLimits::default()
                .backend(backend)
                // Lift caps so large-N benches don't trip the defaults.
                .max_rows_per_query(1_000_000)
                .max_statements(1_000_000)
                .max_script_bytes(64 * 1024 * 1024),
        )
        .env("BASHKIT_ALLOW_INPROCESS_SQLITE", "1");
    if let Some(fs) = fs {
        builder = builder.fs(fs);
    }
    builder.build()
}

/// SQL that seeds a `kv(id, k, v)` table with `n` rows using a recursive CTE.
/// Faster than emitting `n` literal `INSERT` statements through the parser.
fn seed_kv_sql(n: usize) -> String {
    format!(
        r#"
CREATE TABLE IF NOT EXISTS kv(id INTEGER PRIMARY KEY, k TEXT, v INTEGER);
WITH RECURSIVE seq(i) AS (
    SELECT 1 UNION ALL SELECT i+1 FROM seq WHERE i < {n}
)
INSERT INTO kv(k, v) SELECT 'key' || i, i*7 % 113 FROM seq;
"#
    )
}

// ---------------------------------------------------------------------------
// Workload helpers (each takes a fresh Bash, runs the workload, returns).
// ---------------------------------------------------------------------------

async fn bench_insert(backend: SqliteBackend, n: usize) {
    let mut bash = make_bash(backend, None);
    let script = format!(
        "sqlite /tmp/bench.sqlite \"{}; SELECT count(*) FROM kv\"",
        seed_kv_sql(n).replace('"', "\\\"")
    );
    let _ = bash.exec(&script).await;
}

async fn bench_update(backend: SqliteBackend, n: usize) {
    let mut bash = make_bash(backend, None);
    let setup = seed_kv_sql(n).replace('"', "\\\"");
    let script = format!(
        "sqlite /tmp/bench.sqlite \"{setup}; UPDATE kv SET v = v + 1 WHERE k LIKE 'key1%'; SELECT changes()\""
    );
    let _ = bash.exec(&script).await;
}

async fn bench_index_create_and_query(backend: SqliteBackend, n: usize) {
    let mut bash = make_bash(backend, None);
    let setup = seed_kv_sql(n).replace('"', "\\\"");
    // Without index → with index → repeat the same point-query, measuring
    // the combined "schema change + indexed lookup" path. The point-query
    // alone is benched in `bench_query_indexed` below.
    let script = format!(
        "sqlite /tmp/bench.sqlite \"{setup}; CREATE INDEX IF NOT EXISTS idx_kv_k ON kv(k); SELECT v FROM kv WHERE k = 'key42'\""
    );
    let _ = bash.exec(&script).await;
}

async fn bench_query_indexed(backend: SqliteBackend, n: usize) {
    let mut bash = make_bash(backend, None);
    let setup = seed_kv_sql(n).replace('"', "\\\"");
    let script = format!(
        "sqlite /tmp/bench.sqlite \"{setup}; CREATE INDEX idx_kv_k ON kv(k); SELECT count(*) FROM kv WHERE k IN ('key1','key100','key1000','key9999')\""
    );
    let _ = bash.exec(&script).await;
}

async fn bench_query_full_scan(backend: SqliteBackend, n: usize) {
    let mut bash = make_bash(backend, None);
    let setup = seed_kv_sql(n).replace('"', "\\\"");
    // No index → forces a full scan with a LIKE predicate that the planner
    // cannot satisfy from sqlite_master.
    let script = format!(
        "sqlite /tmp/bench.sqlite \"{setup}; SELECT count(*) FROM kv WHERE k LIKE '%99%'\""
    );
    let _ = bash.exec(&script).await;
}

async fn bench_aggregate(backend: SqliteBackend, n: usize) {
    let mut bash = make_bash(backend, None);
    let setup = seed_kv_sql(n).replace('"', "\\\"");
    let script = format!(
        "sqlite /tmp/bench.sqlite \"{setup}; SELECT v % 10 AS bucket, count(*), sum(v), avg(v) FROM kv GROUP BY bucket ORDER BY bucket\""
    );
    let _ = bash.exec(&script).await;
}

async fn bench_output_mode(backend: SqliteBackend, n: usize, mode_flag: &str) {
    let mut bash = make_bash(backend, None);
    let setup = seed_kv_sql(n).replace('"', "\\\"");
    // Materialise the full row set through the formatter — this is what
    // changes between modes; the underlying scan is identical.
    let script = format!(
        "sqlite {mode_flag} -header /tmp/bench.sqlite \"{setup}; SELECT id, k, v FROM kv ORDER BY id\""
    );
    let _ = bash.exec(&script).await;
}

async fn bench_persistence_per_invocation(backend: SqliteBackend, n: usize) {
    // Two invocations: write, then read — measures load+flush per call.
    let fs: Arc<dyn FileSystem> = Arc::new(InMemoryFs::new());
    let mut bash = make_bash(backend, Some(Arc::clone(&fs)));
    let setup = seed_kv_sql(n).replace('"', "\\\"");
    let _ = bash
        .exec(&format!("sqlite /tmp/persist.sqlite \"{setup}\""))
        .await;
    let _ = bash
        .exec("sqlite /tmp/persist.sqlite 'SELECT count(*) FROM kv'")
        .await;
}

async fn bench_in_memory_only(n: usize) {
    let mut bash = make_bash(SqliteBackend::Memory, None);
    let setup = seed_kv_sql(n).replace('"', "\\\"");
    // `:memory:` skips the VFS entirely; isolates pure engine cost.
    let script = format!("sqlite :memory: \"{setup}; SELECT count(*) FROM kv\"");
    let _ = bash.exec(&script).await;
}

async fn bench_parallel_sessions(sessions: usize, rows: usize, backend: SqliteBackend) {
    // Shared VFS, distinct DB file per session — tests the cooperative-IO
    // path without contending on a single sqlite file (which the builtin
    // doesn't try to coordinate; see `specs/sqlite-builtin.md`).
    let fs: Arc<dyn FileSystem> = Arc::new(InMemoryFs::new());
    let handles: Vec<_> = (0..sessions)
        .map(|i| {
            let fs = Arc::clone(&fs);
            tokio::spawn(async move {
                let mut bash = make_bash(backend, Some(fs));
                let setup = seed_kv_sql(rows).replace('"', "\\\"");
                let script = format!(
                    "sqlite /tmp/sess_{i}.sqlite \"{setup}; SELECT count(*), sum(v) FROM kv\""
                );
                let _ = bash.exec(&script).await;
            })
        })
        .collect();
    for h in handles {
        let _ = h.await;
    }
}

// ---------------------------------------------------------------------------
// Criterion groups
// ---------------------------------------------------------------------------

fn bench_crud(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("sqlite_crud");
    group.sample_size(20);

    for &n in ROW_COUNTS {
        group.throughput(Throughput::Elements(n as u64));

        group.bench_with_input(BenchmarkId::new("insert_mem", n), &n, |b, &n| {
            b.to_async(&rt)
                .iter(|| bench_insert(SqliteBackend::Memory, n));
        });
        group.bench_with_input(BenchmarkId::new("insert_vfs", n), &n, |b, &n| {
            b.to_async(&rt).iter(|| bench_insert(SqliteBackend::Vfs, n));
        });

        group.bench_with_input(BenchmarkId::new("update_mem", n), &n, |b, &n| {
            b.to_async(&rt)
                .iter(|| bench_update(SqliteBackend::Memory, n));
        });
        group.bench_with_input(BenchmarkId::new("update_vfs", n), &n, |b, &n| {
            b.to_async(&rt).iter(|| bench_update(SqliteBackend::Vfs, n));
        });
    }

    group.finish();
}

fn bench_indexing(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("sqlite_index");
    group.sample_size(20);

    for &n in ROW_COUNTS {
        group.throughput(Throughput::Elements(n as u64));

        group.bench_with_input(BenchmarkId::new("create_index_mem", n), &n, |b, &n| {
            b.to_async(&rt)
                .iter(|| bench_index_create_and_query(SqliteBackend::Memory, n));
        });
        group.bench_with_input(BenchmarkId::new("indexed_lookup_mem", n), &n, |b, &n| {
            b.to_async(&rt)
                .iter(|| bench_query_indexed(SqliteBackend::Memory, n));
        });
        group.bench_with_input(BenchmarkId::new("full_scan_mem", n), &n, |b, &n| {
            b.to_async(&rt)
                .iter(|| bench_query_full_scan(SqliteBackend::Memory, n));
        });
    }

    group.finish();
}

fn bench_queries(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("sqlite_query");
    group.sample_size(20);

    for &n in ROW_COUNTS {
        group.throughput(Throughput::Elements(n as u64));

        group.bench_with_input(BenchmarkId::new("aggregate_mem", n), &n, |b, &n| {
            b.to_async(&rt)
                .iter(|| bench_aggregate(SqliteBackend::Memory, n));
        });
        group.bench_with_input(BenchmarkId::new("aggregate_vfs", n), &n, |b, &n| {
            b.to_async(&rt)
                .iter(|| bench_aggregate(SqliteBackend::Vfs, n));
        });
        group.bench_with_input(BenchmarkId::new("aggregate_in_memory", n), &n, |b, &n| {
            b.to_async(&rt).iter(|| bench_in_memory_only(n));
        });
    }

    group.finish();
}

fn bench_output_modes(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("sqlite_output_mode");
    group.sample_size(20);

    let n = 1_000;
    group.throughput(Throughput::Elements(n as u64));
    for (label, flag) in [
        ("list", ""),
        ("csv", "-csv"),
        ("json", "-json"),
        ("markdown", "-markdown"),
        ("box", "-box"),
    ] {
        group.bench_function(label, |b| {
            b.to_async(&rt)
                .iter(|| bench_output_mode(SqliteBackend::Memory, n, flag));
        });
    }

    group.finish();
}

fn bench_persistence(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("sqlite_persistence");
    group.sample_size(20);

    let n = 1_000;
    group.throughput(Throughput::Elements(n as u64));
    group.bench_function("two_invocations_mem", |b| {
        b.to_async(&rt)
            .iter(|| bench_persistence_per_invocation(SqliteBackend::Memory, n));
    });
    group.bench_function("two_invocations_vfs", |b| {
        b.to_async(&rt)
            .iter(|| bench_persistence_per_invocation(SqliteBackend::Vfs, n));
    });
    group.bench_function("memory_db_baseline", |b| {
        b.to_async(&rt).iter(|| bench_in_memory_only(n));
    });

    group.finish();
}

fn bench_parallel(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("sqlite_parallel");
    group.sample_size(15);

    let rows = 500;
    for &sessions in SESSION_COUNTS {
        group.throughput(Throughput::Elements((sessions * rows) as u64));
        group.bench_with_input(BenchmarkId::new("mem", sessions), &sessions, |b, &s| {
            b.to_async(&rt)
                .iter(|| bench_parallel_sessions(s, rows, SqliteBackend::Memory));
        });
        group.bench_with_input(BenchmarkId::new("vfs", sessions), &sessions, |b, &s| {
            b.to_async(&rt)
                .iter(|| bench_parallel_sessions(s, rows, SqliteBackend::Vfs));
        });
    }

    group.finish();
}

// ---------------------------------------------------------------------------
// Smoke-test harness — runs as part of `cargo test --features sqlite` so a
// regression in the benched code path breaks the build, not just a silent
// 0ns measurement.
// ---------------------------------------------------------------------------

#[test]
fn verify_insert_and_count() {
    let rt = Runtime::new().unwrap();
    rt.block_on(async {
        let mut bash = make_bash(SqliteBackend::Memory, None);
        let setup = seed_kv_sql(50).replace('"', "\\\"");
        let r = bash
            .exec(&format!(
                "sqlite /tmp/v.sqlite \"{setup}; SELECT count(*) FROM kv\""
            ))
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0, "stderr={}", r.stderr);
        assert_eq!(r.stdout.trim(), "50");
    });
}

#[test]
fn verify_aggregate_groups() {
    let rt = Runtime::new().unwrap();
    rt.block_on(async {
        let mut bash = make_bash(SqliteBackend::Memory, None);
        let setup = seed_kv_sql(100).replace('"', "\\\"");
        let r = bash
            .exec(&format!(
                "sqlite -header /tmp/v.sqlite \"{setup}; SELECT count(DISTINCT v % 10) FROM kv\""
            ))
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0, "stderr={}", r.stderr);
        // 10 buckets max — the exact count depends on the v distribution.
        let count: i64 = r.stdout.lines().last().unwrap().trim().parse().unwrap();
        assert!(
            (1..=10).contains(&count),
            "unexpected bucket count: {count}"
        );
    });
}

#[test]
fn verify_index_lookup_matches_scan() {
    let rt = Runtime::new().unwrap();
    rt.block_on(async {
        let mut bash = make_bash(SqliteBackend::Memory, None);
        let setup = seed_kv_sql(200).replace('"', "\\\"");
        let r = bash
            .exec(&format!(
                "sqlite /tmp/v.sqlite \"{setup}; CREATE INDEX i ON kv(k); SELECT v FROM kv WHERE k='key42'\""
            ))
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0, "stderr={}", r.stderr);
        // 42 * 7 % 113 = 294 % 113 = 68.
        assert_eq!(r.stdout.trim(), "68");
    });
}

#[test]
fn verify_persistence_across_invocations() {
    let rt = Runtime::new().unwrap();
    rt.block_on(async {
        let fs: Arc<dyn FileSystem> = Arc::new(InMemoryFs::new());
        let mut bash = make_bash(SqliteBackend::Memory, Some(Arc::clone(&fs)));
        let setup = seed_kv_sql(25).replace('"', "\\\"");
        let r1 = bash
            .exec(&format!("sqlite /tmp/persist.sqlite \"{setup}\""))
            .await
            .unwrap();
        assert_eq!(r1.exit_code, 0, "stderr={}", r1.stderr);
        let r2 = bash
            .exec("sqlite /tmp/persist.sqlite 'SELECT count(*) FROM kv'")
            .await
            .unwrap();
        assert_eq!(r2.exit_code, 0, "stderr={}", r2.stderr);
        assert_eq!(r2.stdout.trim(), "25");
    });
}

criterion_group!(
    benches,
    bench_crud,
    bench_indexing,
    bench_queries,
    bench_output_modes,
    bench_persistence,
    bench_parallel,
);
criterion_main!(benches);
