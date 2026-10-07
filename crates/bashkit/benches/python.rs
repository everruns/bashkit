//! Python runtime benchmark: CPython (WASI, `cpython` feature) vs Monty
//! (`python` feature).
//!
//! Dimensions:
//! - **Raw start**: the first `python3` in a fresh OS process (Monty has no
//!   load step; CPython creates a wasmtime engine and maps the precompiled
//!   module). One-time per process, so it is measured by the `python_startup`
//!   example in new processes (`scripts/bench-python.sh`), not here.
//! - **Per call** (`python_call`): one `python3` invocation in a warm
//!   process, through the bashkit interpreter (parse, dispatch, instance,
//!   run, teardown) — what a script author observes.
//! - **Per session** (`python_session`): a new `Bash` per call, the
//!   multi-tenant request shape.
//! - **Compute** (`python_compute`): CPU-bound Python (interpreter speed).
//! - **Parallel** (`python_parallel`): N concurrent sessions, each one
//!   `python3` call, on a multi-thread runtime.
//!
//! Run: `cargo bench --bench python --features python,cpython` or
//! `just bench-python` (saves results).

use bashkit::{Bash, CPython, SessionLimits};
use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use tokio::runtime::Runtime;

const RUNTIMES: &[&str] = &["monty", "cpython"];
const PARALLEL: &[usize] = &[1, 16, 64];

fn make_bash(runtime: &str) -> Bash {
    // Warm sessions run thousands of calls; lift the per-session exec cap.
    let builder = Bash::builder().session_limits(SessionLimits::unlimited());
    match runtime {
        "monty" => builder
            .python()
            .env("BASHKIT_ALLOW_INPROCESS_PYTHON", "1")
            .build(),
        "cpython" => builder.cpython().build(),
        _ => unreachable!(),
    }
}

/// Scripts both runtimes accept, with identical output.
const CALLS: &[(&str, &str)] = &[
    ("pass", "python3 -c 'pass'"),
    ("print", "python3 -c 'print(1)'"),
    (
        "json_roundtrip",
        "python3 -c 'import json; print(json.dumps(json.loads(\"{\\\"a\\\": [1, 2, 3]}\")))'",
    ),
    (
        "file_write_read",
        "python3 -c 'from pathlib import Path; p = Path(\"/tmp/b.txt\"); p.write_text(\"x\" * 1000); print(len(p.read_text()))'",
    ),
];

const COMPUTE: &[(&str, &str)] = &[
    (
        "loop_sum_1e5",
        "python3 -c 'print(sum(i * i for i in range(100000)))'",
    ),
    (
        "fib_20",
        "python3 -c 'def fib(n):\n    return n if n < 2 else fib(n - 1) + fib(n - 2)\nprint(fib(20))'",
    ),
    (
        "string_build_1e4",
        "python3 -c 'print(len(\",\".join(str(i) for i in range(10000))))'",
    ),
];

fn verify(rt: &Runtime) {
    // Fail fast if a runtime cannot run the workloads, instead of timing errors.
    for runtime in RUNTIMES {
        for (name, script) in CALLS.iter().chain(COMPUTE) {
            let r = rt.block_on(async { make_bash(runtime).exec(script).await.unwrap() });
            assert_eq!(r.exit_code, 0, "{runtime}/{name}: {}", r.stderr);
        }
    }
}

fn bench_call(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    CPython::warm_up().unwrap();
    verify(&rt);
    let mut group = c.benchmark_group("python_call");
    for runtime in RUNTIMES {
        // One long-lived session; the lock is uncontended (iterations are sequential).
        let bash = tokio::sync::Mutex::new(make_bash(runtime));
        for (name, script) in CALLS {
            group.bench_with_input(BenchmarkId::new(*runtime, name), script, |b, script| {
                b.to_async(&rt)
                    .iter(|| async { bash.lock().await.exec(script).await.unwrap() });
            });
        }
    }
    group.finish();
}

fn bench_session(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    CPython::warm_up().unwrap();
    let mut group = c.benchmark_group("python_session");
    for runtime in RUNTIMES {
        group.bench_function(BenchmarkId::new(*runtime, "new_bash_print"), |b| {
            b.to_async(&rt).iter(|| async {
                make_bash(runtime)
                    .exec("python3 -c 'print(1)'")
                    .await
                    .unwrap()
            });
        });
    }
    group.finish();
}

fn bench_compute(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    CPython::warm_up().unwrap();
    let mut group = c.benchmark_group("python_compute");
    group.sample_size(10);
    for runtime in RUNTIMES {
        for (name, script) in COMPUTE {
            group.bench_with_input(BenchmarkId::new(*runtime, name), script, |b, script| {
                b.to_async(&rt)
                    .iter(|| async { make_bash(runtime).exec(script).await.unwrap() });
            });
        }
    }
    group.finish();
}

fn bench_parallel(c: &mut Criterion) {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    CPython::warm_up().unwrap();
    let mut group = c.benchmark_group("python_parallel");
    group.sample_size(10);
    for runtime in RUNTIMES {
        for &n in PARALLEL {
            group.throughput(Throughput::Elements(n as u64));
            group.bench_with_input(BenchmarkId::new(*runtime, n), &n, |b, &n| {
                b.to_async(&rt).iter(|| async move {
                    let tasks: Vec<_> = (0..n)
                        .map(|i| {
                            let runtime = *runtime;
                            tokio::spawn(async move {
                                let script = format!("python3 -c 'print({i} * 2)'");
                                let r = make_bash(runtime).exec(&script).await.unwrap();
                                assert_eq!(r.stdout, format!("{}\n", i * 2));
                            })
                        })
                        .collect();
                    for t in tasks {
                        t.await.unwrap();
                    }
                });
            });
        }
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_call,
    bench_session,
    bench_compute,
    bench_parallel
);
criterion_main!(benches);
