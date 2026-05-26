//! VFS / file-operation benchmarks.
//!
//! Today's bench coverage stops at trivial three-line `grep`/`awk`/`sed`
//! cases. This file fills in the workloads users actually hit:
//!
//!   - **Read throughput** — `cat`/`grep` over 1 KB, 1 MB, 50 MB files.
//!     Modeled on `sqlite.rs`'s backend-comparison shape (parametrized
//!     by file size instead of backend).
//!   - **Traversal** — `ls -R`, `find . -name`, and `for f in *` over a
//!     1000-file generated tree.
//!   - **`rg` workloads** — literal, regex, `--no-ignore`, `--multiline`,
//!     recursive over the same tree. Highest-churn builtin right now
//!     (~15 fixes in PRs #1742–#1767) with zero perf guard until this file.
//!   - **`grep -r` vs `rg`** at parity, so regressions in either show up.
//!   - **Glob expansion** — `echo **/*` (interpreter glob path, not a
//!     builtin) over the same tree.
//!
//! The seeded VFS is built once per bench group and shared via
//! `Bash::builder().fs(fs.clone()).build()` for each iteration — same
//! pattern `sqlite.rs` uses for shared-VFS parallel sessions. Iterations
//! pay one `Bash::new()` constant overhead; read paths are what we time.
//!
//! Run with: `cargo bench --bench file_ops`

use bashkit::{Bash, FileSystem, FsLimits, InMemoryFs};
use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use std::path::Path;
use std::sync::Arc;
use tokio::runtime::Runtime;

/// File sizes for read-throughput benches.
/// Tune the "large" entry down (e.g. to 5 MB) if running on memory-constrained
/// CI: criterion's default 100 samples × 50 MB = 5 GB of reads per bench.
const FILE_SIZES: &[(&str, usize)] = &[
    ("1KB", 1024),
    ("1MB", 1024 * 1024),
    ("50MB", 50 * 1024 * 1024),
];

/// Total files in the generated tree, fanned out across `TREE_DIRS` subdirs.
const TREE_FILES: usize = 1000;
const TREE_DIRS: usize = 10;
const TREE_ROOT: &str = "/work";

/// Build a fresh seeded FS containing one `/data/file.txt` of `size` bytes.
/// Pattern is `"abc\n"` repeated — predictable line count so we can
/// sanity-check `wc -l` from a `verify_*` test if needed.
fn seed_single_file(rt: &Runtime, size: usize) -> Arc<InMemoryFs> {
    // Default FsLimits caps individual file size at 10 MB; bench needs 50 MB.
    let fs = Arc::new(InMemoryFs::with_limits(FsLimits::unlimited()));
    rt.block_on(async {
        let fs_dyn: Arc<dyn FileSystem> = fs.clone();
        fs_dyn
            .mkdir(Path::new("/data"), true)
            .await
            .expect("mkdir /data");
        let mut buf = Vec::with_capacity(size);
        let chunk = b"abcdefghij\n"; // 11 bytes, includes newline
        while buf.len() + chunk.len() <= size {
            buf.extend_from_slice(chunk);
        }
        // Pad with zeros (no extra newlines) to hit `size` exactly.
        buf.resize(size, b'.');
        fs_dyn
            .write_file(Path::new("/data/file.txt"), &buf)
            .await
            .expect("write /data/file.txt");
    });
    fs
}

/// Build a 1000-file tree fanned across 10 subdirs. Each file gets the
/// same small body with a per-file marker, so `grep`/`rg` benches have
/// real content to match.
fn seed_tree(rt: &Runtime) -> Arc<InMemoryFs> {
    let fs = Arc::new(InMemoryFs::with_limits(FsLimits::unlimited()));
    rt.block_on(async {
        let fs_dyn: Arc<dyn FileSystem> = fs.clone();
        fs_dyn
            .mkdir(Path::new(TREE_ROOT), true)
            .await
            .expect("mkdir root");
        let per_dir = TREE_FILES / TREE_DIRS;
        for d in 0..TREE_DIRS {
            let dir = format!("{TREE_ROOT}/d{d:02}");
            fs_dyn
                .mkdir(Path::new(&dir), true)
                .await
                .expect("mkdir subdir");
            for f in 0..per_dir {
                let path = format!("{dir}/f{f:03}.txt");
                // ~6 lines per file; one in five carries "needle" so
                // recursive search has a non-trivial hit count.
                let mut body =
                    format!("alpha {d} {f}\nbeta {d} {f}\ngamma\ndelta\nepsilon\nzeta\n");
                if (d * per_dir + f).is_multiple_of(5) {
                    body.push_str("needle here\n");
                }
                fs_dyn
                    .write_file(Path::new(&path), body.as_bytes())
                    .await
                    .expect("write tree file");
            }
        }
    });
    fs
}

/// Spin up a Bash sharing the pre-seeded VFS. One construction per iter
/// (constant overhead) — same pattern as `parallel_execution.rs::single_*`.
fn bash_with(fs: &Arc<InMemoryFs>) -> Bash {
    let fs_dyn: Arc<dyn FileSystem> = fs.clone();
    Bash::builder().fs(fs_dyn).build()
}

// ---------------------------------------------------------------------------
// VFS read throughput: cat + grep over 1 KB / 1 MB / 50 MB.
// ---------------------------------------------------------------------------

fn bench_read_throughput(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut g = c.benchmark_group("read_throughput");
    for (label, size) in FILE_SIZES {
        let fs = seed_single_file(&rt, *size);
        g.throughput(Throughput::Bytes(*size as u64));

        g.bench_with_input(BenchmarkId::new("cat", label), &fs, |b, fs| {
            b.to_async(&rt).iter(|| {
                let fs = fs.clone();
                async move {
                    let mut bash = bash_with(&fs);
                    let _ = bash.exec("cat /data/file.txt >/dev/null").await;
                }
            });
        });

        g.bench_with_input(BenchmarkId::new("grep_literal", label), &fs, |b, fs| {
            b.to_async(&rt).iter(|| {
                let fs = fs.clone();
                async move {
                    let mut bash = bash_with(&fs);
                    let _ = bash.exec("grep abcdef /data/file.txt >/dev/null").await;
                }
            });
        });

        g.bench_with_input(BenchmarkId::new("grep_count", label), &fs, |b, fs| {
            b.to_async(&rt).iter(|| {
                let fs = fs.clone();
                async move {
                    let mut bash = bash_with(&fs);
                    let _ = bash.exec("grep -c abc /data/file.txt >/dev/null").await;
                }
            });
        });
    }
    g.finish();
}

// ---------------------------------------------------------------------------
// Tree traversal: ls -R, find, glob expansion.
// ---------------------------------------------------------------------------

fn bench_traversal(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let fs = seed_tree(&rt);
    let mut g = c.benchmark_group("traversal");

    g.bench_function("ls_R_1k_files", |b| {
        b.to_async(&rt).iter(|| {
            let fs = fs.clone();
            async move {
                let mut bash = bash_with(&fs);
                let _ = bash.exec("ls -R /work >/dev/null").await;
            }
        });
    });

    g.bench_function("find_by_name_1k_files", |b| {
        b.to_async(&rt).iter(|| {
            let fs = fs.clone();
            async move {
                let mut bash = bash_with(&fs);
                let _ = bash.exec("find /work -name 'f001.txt' >/dev/null").await;
            }
        });
    });

    g.bench_function("find_no_filter_1k_files", |b| {
        b.to_async(&rt).iter(|| {
            let fs = fs.clone();
            async move {
                let mut bash = bash_with(&fs);
                let _ = bash.exec("find /work >/dev/null").await;
            }
        });
    });

    // Interpreter glob expansion (NOT a builtin). Two shapes:
    //   - shallow `*` glob (one directory level)
    //   - recursive `**/*` glob (globstar shopt)
    g.bench_function("glob_shallow_subdir", |b| {
        b.to_async(&rt).iter(|| {
            let fs = fs.clone();
            async move {
                let mut bash = bash_with(&fs);
                let _ = bash.exec("for f in /work/d00/*; do :; done").await;
            }
        });
    });

    g.bench_function("glob_globstar_1k_files", |b| {
        b.to_async(&rt).iter(|| {
            let fs = fs.clone();
            async move {
                let mut bash = bash_with(&fs);
                let _ = bash
                    .exec("shopt -s globstar; for f in /work/**/*; do :; done")
                    .await;
            }
        });
    });

    // Interpreter glob materialized into a single command's argv.
    g.bench_function("glob_echo_globstar", |b| {
        b.to_async(&rt).iter(|| {
            let fs = fs.clone();
            async move {
                let mut bash = bash_with(&fs);
                let _ = bash
                    .exec("shopt -s globstar; echo /work/**/* >/dev/null")
                    .await;
            }
        });
    });

    g.finish();
}

// ---------------------------------------------------------------------------
// rg workloads — literal, regex, --no-ignore, --multiline, recursive.
// ---------------------------------------------------------------------------

fn bench_rg(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let fs = seed_tree(&rt);
    let mut g = c.benchmark_group("rg");

    g.bench_function("literal_recursive", |b| {
        b.to_async(&rt).iter(|| {
            let fs = fs.clone();
            async move {
                let mut bash = bash_with(&fs);
                let _ = bash.exec("rg needle /work >/dev/null").await;
            }
        });
    });

    g.bench_function("regex_recursive", |b| {
        b.to_async(&rt).iter(|| {
            let fs = fs.clone();
            async move {
                let mut bash = bash_with(&fs);
                let _ = bash.exec(r"rg '\balpha \d+ \d+\b' /work >/dev/null").await;
            }
        });
    });

    // --no-ignore: forces traversal to ignore .gitignore-style rules.
    // No ignore files in the seeded tree, but the flag still exercises
    // a different code path through the ignore stack.
    g.bench_function("no_ignore_recursive", |b| {
        b.to_async(&rt).iter(|| {
            let fs = fs.clone();
            async move {
                let mut bash = bash_with(&fs);
                let _ = bash.exec("rg --no-ignore needle /work >/dev/null").await;
            }
        });
    });

    g.bench_function("multiline_recursive", |b| {
        b.to_async(&rt).iter(|| {
            let fs = fs.clone();
            async move {
                let mut bash = bash_with(&fs);
                let _ = bash
                    .exec(r"rg --multiline 'alpha.*\n.*beta' /work >/dev/null")
                    .await;
            }
        });
    });

    // Single-file rg — separates per-file overhead from traversal cost.
    g.bench_function("literal_single_file", |b| {
        b.to_async(&rt).iter(|| {
            let fs = fs.clone();
            async move {
                let mut bash = bash_with(&fs);
                let _ = bash.exec("rg needle /work/d00/f000.txt >/dev/null").await;
            }
        });
    });

    g.finish();
}

// ---------------------------------------------------------------------------
// grep -r vs rg parity — same query, same tree, different builtins.
// Regressions in either show up as a divergence on the same chart.
// ---------------------------------------------------------------------------

fn bench_grep_rg_parity(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let fs = seed_tree(&rt);
    let mut g = c.benchmark_group("grep_vs_rg");

    g.bench_function("grep_r_literal", |b| {
        b.to_async(&rt).iter(|| {
            let fs = fs.clone();
            async move {
                let mut bash = bash_with(&fs);
                let _ = bash.exec("grep -r needle /work >/dev/null").await;
            }
        });
    });

    g.bench_function("rg_literal", |b| {
        b.to_async(&rt).iter(|| {
            let fs = fs.clone();
            async move {
                let mut bash = bash_with(&fs);
                let _ = bash.exec("rg needle /work >/dev/null").await;
            }
        });
    });

    g.bench_function("grep_r_regex", |b| {
        b.to_async(&rt).iter(|| {
            let fs = fs.clone();
            async move {
                let mut bash = bash_with(&fs);
                let _ = bash
                    .exec(r"grep -rE 'alpha [0-9]+ [0-9]+' /work >/dev/null")
                    .await;
            }
        });
    });

    g.bench_function("rg_regex", |b| {
        b.to_async(&rt).iter(|| {
            let fs = fs.clone();
            async move {
                let mut bash = bash_with(&fs);
                let _ = bash
                    .exec(r"rg 'alpha [0-9]+ [0-9]+' /work >/dev/null")
                    .await;
            }
        });
    });

    g.finish();
}

// ---------------------------------------------------------------------------
// Sanity checks (compile-time-cheap, runs under `cargo test`).
// ---------------------------------------------------------------------------

#[test]
fn verify_seed_single_file() {
    let rt = Runtime::new().unwrap();
    let fs = seed_single_file(&rt, 1024);
    rt.block_on(async {
        let mut bash = bash_with(&fs);
        let res = bash.exec("wc -c </data/file.txt").await.expect("exec wc");
        assert_eq!(res.stdout.trim(), "1024", "expected 1024 bytes");
    });
}

#[test]
fn verify_seed_tree() {
    let rt = Runtime::new().unwrap();
    let fs = seed_tree(&rt);
    rt.block_on(async {
        let mut bash = bash_with(&fs);
        let res = bash
            .exec("find /work -type f | wc -l")
            .await
            .expect("exec find");
        assert_eq!(
            res.stdout.trim(),
            TREE_FILES.to_string(),
            "expected {TREE_FILES} files"
        );
    });
}

criterion_group!(
    benches,
    bench_read_throughput,
    bench_traversal,
    bench_rg,
    bench_grep_rg_parity,
);
criterion_main!(benches);
