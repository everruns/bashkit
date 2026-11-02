//! Tree-walking builtins over a simulated remote filesystem.
//!
//! Remote VFS backends (a database, an object store, a network share) pay
//! one round trip per `FileSystem` call, ~10 ms in production. What makes a
//! command slow there is its call count, not CPU. This bench loads a real
//! tree, the bashkit checkout itself (`git ls-files`, ~3.7k files), into a
//! `CountingFs` and:
//!
//! 1. prints, per command and backend, the call counts and the time they
//!    would cost at 10 ms per call (`--counts` or always, before timing);
//! 2. times each command with a simulated per-call latency
//!    (`BASHKIT_BENCH_LATENCY_US`, default 100 us) under criterion.
//!
//! Backends: `plain` (no index, `walk` = `read_dir` per directory), `index`
//! (`SearchCapable` content index), `index+walk` (index plus a one-call bulk
//! `walk`, like a path-prefix query).
//!
//! Run with: `cargo bench --bench remote_fs`
//! Counts only: `cargo bench --bench remote_fs -- --counts`

use bashkit::testing::{CallCounts, CountingFs};
use bashkit::{Bash, FileSystem, FsLimits};
use criterion::{BenchmarkId, Criterion, criterion_group};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::runtime::Runtime;

/// Per-call latency the production report is about.
const REMOTE_CALL_MS: f64 = 10.0;
/// Files above this are skipped (generated fixtures, lockfiles).
const MAX_FILE_BYTES: u64 = 1 << 20;

/// Commands under test, run with cwd `/workspace` (the checkout root).
const COMMANDS: &[&str] = &[
    // The production report's commands (no match in this checkout: worst case).
    "rg -l markkit crates",
    "grep -rln markkit crates",
    "find /workspace -path '*markkit*'",
    // A rare match and a common one.
    "rg -l SearchCapable crates",
    "rg -l --no-ignore SearchCapable crates",
    "grep -rl SearchCapable crates",
    "rg -c fn crates/bashkit/src/builtins",
    "find crates -type f -name '*.rs'",
    "find crates -ls",
    "ls -R crates",
    "du -s crates",
    "tree crates",
    "tar -cf /tmp/src.tar crates/bashkit/src/builtins",
    "cp -r crates/bashkit/src/builtins /tmp/builtins-copy",
];

#[derive(Clone, Copy)]
enum Backend {
    Plain,
    Index,
    IndexWalk,
}

impl Backend {
    const ALL: [Backend; 3] = [Backend::Plain, Backend::Index, Backend::IndexWalk];

    fn name(self) -> &'static str {
        match self {
            Backend::Plain => "plain",
            Backend::Index => "index",
            Backend::IndexWalk => "index+walk",
        }
    }

    fn fs(self) -> CountingFs {
        let fs = CountingFs::new();
        match self {
            Backend::Plain => fs,
            Backend::Index => fs.with_index(),
            Backend::IndexWalk => fs.with_index().with_native_walk(),
        }
    }
}

/// Repository root and its tracked files.
fn checkout_files() -> (PathBuf, Vec<String>) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(&root)
        .args(["ls-files", "-z"])
        .output()
        .expect("git ls-files (the bench needs a git checkout)");
    let files = out
        .stdout
        .split(|b| *b == 0)
        .filter(|f| !f.is_empty())
        .map(|f| String::from_utf8_lossy(f).into_owned())
        .collect();
    (root, files)
}

/// Copy the checkout into a fresh backend under `/workspace`; `latency`
/// applies after seeding.
fn load(rt: &Runtime, backend: Backend, latency: Duration) -> (Arc<CountingFs>, usize) {
    let (root, files) = checkout_files();
    let fs = Arc::new(backend.fs().with_limits(FsLimits::unlimited()));
    let mut loaded = 0;
    rt.block_on(async {
        // `.git` marks the tree as a repository, so `.gitignore` applies.
        fs.mkdir(Path::new("/workspace/.git"), true).await.unwrap();
        for rel in &files {
            let host = root.join(rel);
            let Ok(meta) = std::fs::symlink_metadata(&host) else {
                continue;
            };
            if !meta.is_file() || meta.len() > MAX_FILE_BYTES {
                continue;
            }
            let Ok(content) = std::fs::read(&host) else {
                continue;
            };
            let path = PathBuf::from(format!("/workspace/{rel}"));
            if let Some(parent) = path.parent() {
                fs.mkdir(parent, true).await.unwrap();
            }
            fs.write_file(&path, &content).await.unwrap();
            loaded += 1;
        }
    });
    fs.set_latency(latency);
    (fs, loaded)
}

fn bash_for(fs: &Arc<CountingFs>) -> Bash {
    let fs_dyn: Arc<dyn FileSystem> = fs.clone();
    Bash::builder()
        .fs(fs_dyn)
        .cwd("/workspace")
        .limits(bashkit::ExecutionLimits::default().timeout(Duration::from_secs(600)))
        .build()
}

fn run_once(rt: &Runtime, fs: &Arc<CountingFs>, cmd: &str) -> (CallCounts, Duration) {
    let mut bash = bash_for(fs);
    rt.block_on(async {
        let _ = bash.exec("rm -rf /tmp/src.tar /tmp/builtins-copy").await;
    });
    fs.reset();
    let start = std::time::Instant::now();
    let r = rt.block_on(bash.exec(&format!("{cmd} > /dev/null")));
    let elapsed = start.elapsed();
    if let Ok(r) = &r
        && r.exit_code > 1
    {
        eprintln!("warning: `{cmd}` exited {}: {}", r.exit_code, r.stderr);
    }
    (fs.counts(), elapsed)
}

/// Markdown table of call counts and projected remote time.
fn print_counts(rt: &Runtime) {
    println!("\n## Round trips per command (bashkit checkout, cwd = repo root)\n");
    let mut header_done = false;
    for backend in Backend::ALL {
        let (fs, loaded) = load(rt, backend, Duration::ZERO);
        if !header_done {
            println!("{loaded} files loaded. Projected time = calls x {REMOTE_CALL_MS} ms.\n");
            println!(
                "| command | backend | calls | read_file | read_dir | stat+lstat+exists | walk | search | at 10 ms |"
            );
            println!("|---|---|---:|---:|---:|---:|---:|---:|---:|");
            header_done = true;
        }
        for cmd in COMMANDS {
            let (c, _) = run_once(rt, &fs, cmd);
            println!(
                "| `{cmd}` | {} | {} | {} | {} | {} | {} | {} | {:.1} s |",
                backend.name(),
                c.total(),
                c.read_file,
                c.read_dir,
                c.metadata(),
                c.walk,
                c.search,
                c.total() as f64 * REMOTE_CALL_MS / 1000.0
            );
        }
    }
    println!();
}

fn latency() -> Duration {
    let us = std::env::var("BASHKIT_BENCH_LATENCY_US")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(100);
    Duration::from_micros(us)
}

fn bench_remote(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let latency = latency();
    let mut group = c.benchmark_group(format!("remote_fs_{}us", latency.as_micros()));
    group.sample_size(10);
    group.measurement_time(Duration::from_secs(5));
    for backend in Backend::ALL {
        let (fs, _) = load(&rt, backend, latency);
        for cmd in COMMANDS {
            group.bench_with_input(BenchmarkId::new(backend.name(), cmd), cmd, |b, cmd| {
                b.iter(|| run_once(&rt, &fs, cmd))
            });
        }
    }
    group.finish();
}

criterion_group!(benches, bench_remote);

fn main() {
    let counts_only = std::env::args().any(|a| a == "--counts");
    let rt = Runtime::new().unwrap();
    print_counts(&rt);
    if counts_only {
        return;
    }
    benches();
    Criterion::default().configure_from_args().final_summary();
}
