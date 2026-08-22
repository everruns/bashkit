//! Per-invocation argument-parsing cost for builtins.
//!
//! Motivation: every ported coreutils builtin rebuilds its whole `clap::Command`
//! on each invocation (`cat_command()` in `builtins/generated/cat_args.rs` is
//! called from `builtins/cat.rs` per exec, and the same holds for `ls`, `od`,
//! `stat`, …). Real scripts call these in loops, so the arg tree is constructed
//! N times for N invocations. This bench exists to answer one question: is that
//! construction cost visible next to the interpreter and the builtin body, or
//! is it noise?
//!
//! Decision: measured end-to-end through the public `Bash::exec` API, not
//! against `cat_command()` directly. `mod builtins` is private in `lib.rs`, and
//! widening the public surface for a bench is worse than reading the cost as a
//! difference between comparable scripts. The numbers are therefore read as
//! deltas, not absolutes:
//!
//!   - `clap_vs_handrolled`: `cat` / `ls` (clap-parsed) against `echo` /
//!     `printf` (hand-parsed) and `:` (no args at all). The `:` row is the
//!     loop-and-dispatch floor; everything above it is arg handling plus body.
//!   - `arg_surface_size`: `cat` (12 args) against `ls` (~60 args) on inputs
//!     sized so the bodies do near-identical work. clap's per-parse cost scales
//!     with the number of declared args while the body does not, so this delta
//!     tracks arg-surface cost.
//!   - `flag_count`: same builtin, more flags on the command line. Costs ~0.45
//!     µs per extra flag — note this measures only the *matching* of additional
//!     argv entries, NOT the fixed per-parse cost, which dominates (see below).
//!
//! Each bench runs `INVOCATIONS` calls inside one script and declares
//! `Throughput::Elements(INVOCATIONS)`, so criterion reports per-invocation
//! time directly and the one-time `Bash::new()` cost amortizes away.
//!
//! What this measured, and what came of it: clap costs ~10 µs per `cat`
//! invocation and ~73% of an `ls` invocation. The expensive part is *parsing*,
//! not constructing the `Command` (construction is ~1.1 µs of `cat`'s 7.3 µs
//! round trip) — but most of that parse cost is clap's one-time `_build_self`,
//! which `builtins::clap_cache` now hoists into a cached pre-built `Command`.
//! Worth −15% on `ls` end-to-end, −2% on `cat`. Keep this bench as the guard:
//! a regression here means the cache stopped being hit.
//!
//! Run with: `cargo bench --bench builtin_args`
//! Save baseline: `cargo bench --bench builtin_args -- --save-baseline before`
//! Compare:      `cargo bench --bench builtin_args -- --baseline before`
//!
//! First-run results belong in `crates/bashkit/benches/results/` as
//! `criterion-builtin_args-<moniker>-<timestamp>.md` (see AGENTS.md → Benches).

use bashkit::{Bash, FileSystem, InMemoryFs};
use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use std::path::Path;
use std::sync::Arc;
use tokio::runtime::Runtime;

/// Invocations per iteration. Large enough that `Bash::new()` and parse of the
/// surrounding loop amortize out, small enough that a 100-sample criterion run
/// stays under a minute per bench.
const INVOCATIONS: u64 = 500;

/// Seed a VFS with one tiny file in a single-entry directory.
///
/// Both are deliberately minimal: a 4-byte file so `cat`'s body is one small
/// read, and a directory holding exactly that file so `ls`'s body is one small
/// readdir. Keeping the bodies trivial is what makes the `cat` vs `ls` delta
/// readable as arg-surface cost rather than as I/O.
fn seed(rt: &Runtime) -> Arc<InMemoryFs> {
    let fs = Arc::new(InMemoryFs::new());
    rt.block_on(async {
        let fs_dyn: Arc<dyn FileSystem> = fs.clone();
        fs_dyn.mkdir(Path::new("/d"), true).await.expect("mkdir /d");
        fs_dyn
            .write_file(Path::new("/d/f"), b"abc\n")
            .await
            .expect("write /d/f");
    });
    fs
}

/// Wrap `body` in a loop of `INVOCATIONS` iterations.
///
/// `for ((...))` rather than `for x in $(seq ...)`: the C-style loop keeps the
/// measured work to the builtin under test, with no command substitution and no
/// `seq` invocation in the middle of the thing we are timing.
fn loop_script(body: &str) -> String {
    format!("for ((i=0; i<{INVOCATIONS}; i++)); do {body}; done")
}

fn run(rt: &Runtime, fs: &Arc<InMemoryFs>, script: &str) {
    rt.block_on(async {
        let mut bash = Bash::builder().fs(fs.clone()).build();
        let result = bash.exec(script).await.expect("exec failed");
        std::hint::black_box(result);
    });
}

/// Register one `INVOCATIONS`-sized loop bench in `g`.
fn bench_loop(
    g: &mut criterion::BenchmarkGroup<'_, criterion::measurement::WallTime>,
    rt: &Runtime,
    fs: &Arc<InMemoryFs>,
    name: &str,
    body: &str,
) {
    let script = loop_script(body);
    g.bench_function(name, |b| b.iter(|| run(rt, fs, &script)));
}

/// clap-parsed builtins against hand-parsed ones and against the empty floor.
fn bench_clap_vs_handrolled(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let fs = seed(&rt);
    let mut g = c.benchmark_group("builtin_args/clap_vs_handrolled");
    g.throughput(Throughput::Elements(INVOCATIONS));

    // Floor: loop + dispatch only, no arguments, no body.
    bench_loop(&mut g, &rt, &fs, "colon_noop", ":");
    // Hand-parsed builtins, one argument each.
    bench_loop(&mut g, &rt, &fs, "echo_handrolled", "echo x > /dev/null");
    bench_loop(
        &mut g,
        &rt,
        &fs,
        "printf_handrolled",
        "printf x > /dev/null",
    );
    // clap-parsed builtins, one argument each.
    bench_loop(&mut g, &rt, &fs, "cat_clap", "cat /d/f > /dev/null");
    bench_loop(&mut g, &rt, &fs, "ls_clap", "ls /d > /dev/null");

    g.finish();
}

/// Small vs large clap arg surface, bodies held near-constant.
///
/// `cat` declares ~12 args, `ls` ~60. If `Command` construction dominates, this
/// gap tracks the arg count; if it does not, the two rows land close together
/// and caching the built `Command` is not worth doing.
fn bench_arg_surface_size(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let fs = seed(&rt);
    let mut g = c.benchmark_group("builtin_args/arg_surface_size");
    g.throughput(Throughput::Elements(INVOCATIONS));

    bench_loop(&mut g, &rt, &fs, "cat_12_args", "cat /d/f > /dev/null");
    bench_loop(&mut g, &rt, &fs, "ls_60_args", "ls /d > /dev/null");

    g.finish();
}

/// Same builtin, growing flag count on the command line.
///
/// Construction cost is identical across these rows, so any spread is match /
/// value-parse cost. Splitting the two matters for the fix: caching a built
/// `Command` helps only the construction half.
fn bench_flag_count(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let fs = seed(&rt);
    let mut g = c.benchmark_group("builtin_args/flag_count");
    g.throughput(Throughput::Elements(INVOCATIONS));

    bench_loop(&mut g, &rt, &fs, "cat_0_flags", "cat /d/f > /dev/null");
    bench_loop(&mut g, &rt, &fs, "cat_1_flag", "cat -n /d/f > /dev/null");
    bench_loop(
        &mut g,
        &rt,
        &fs,
        "cat_3_flags",
        "cat -n -E -T /d/f > /dev/null",
    );

    g.finish();
}

criterion_group!(
    benches,
    bench_clap_vs_handrolled,
    bench_arg_surface_size,
    bench_flag_count
);
criterion_main!(benches);
