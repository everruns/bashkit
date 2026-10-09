//! Wasm coreutils benchmark: real uutils programs as WebAssembly guests
//! (`wasm-coreutils` feature) vs bashkit's native builtins.
//!
//! Dimensions:
//! - **Raw start**: the first wasm utility in a fresh OS process (engine +
//!   module load). One-time per process, so it is measured by the
//!   `wasm_coreutils_startup` example in new processes
//!   (`scripts/bench-wasm-coreutils.sh`), not here.
//! - **Per call** (`coreutils_call`): one command in a warm session, through
//!   the bashkit interpreter, native builtin vs `coreutils <util>`.
//! - **Throughput** (`coreutils_throughput`): the same commands over 1 MiB of
//!   input (guest execution speed on Pulley).
//! - **Per session** (`coreutils_session`): a new `Bash` per call, the
//!   multi-tenant request shape.
//! - **Parallel** (`coreutils_parallel`): N concurrent sessions, each one
//!   call, on a multi-thread runtime.
//!
//! Run: `cargo bench --bench wasm_coreutils --features wasm-coreutils` or
//! `just bench-wasm-coreutils` (saves results).

use bashkit::{Bash, SessionLimits, WasmCoreutil};
use std::path::Path;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use tokio::runtime::Runtime;

const IMPLS: &[&str] = &["native", "wasm"];
const PARALLEL: &[usize] = &[1, 16, 64];

/// Small-input commands; `{u}` becomes the utility (or `coreutils <util>`).
const CALLS: &[(&str, &str)] = &[
    ("echo", "{echo} hello"),
    ("cat_small", "{cat} /data/small.txt"),
    ("wc_small", "{wc} -l /data/small.txt"),
    ("sort_small", "{sort} /data/small.txt"),
    ("sha256sum_small", "{sha256sum} /data/small.txt"),
    (
        "pipeline",
        "{cat} /data/small.txt | {sort} -r | {head} -n 3",
    ),
];

/// 1 MiB-input commands.
const BIG: &[(&str, &str)] = &[
    ("cat_1m", "{cat} /data/big.txt"),
    ("wc_1m", "{wc} /data/big.txt"),
    ("sort_1m", "{sort} /data/big.txt"),
    ("sha256sum_1m", "{sha256sum} /data/big.txt"),
];

const BIG_BYTES: usize = 1 << 20;

fn render(template: &str, imp: &str) -> String {
    let mut out = template.to_string();
    for util in ["echo", "cat", "wc", "sort", "sha256sum", "head"] {
        let with = if imp == "wasm" {
            format!("coreutils {util}")
        } else {
            util.to_string()
        };
        out = out.replace(&format!("{{{util}}}"), &with);
    }
    out
}

fn small_text() -> String {
    (0..100)
        .map(|i| format!("line {}\n", (i * 37) % 101))
        .collect()
}

fn big_text() -> String {
    let mut s = String::with_capacity(BIG_BYTES);
    let mut i: u64 = 0;
    while s.len() < BIG_BYTES {
        i = i
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        s.push_str(&format!("{:016x} row\n", i));
    }
    s
}

fn make_bash(rt: &Runtime) -> Bash {
    let bash = Bash::builder()
        .wasm_coreutils()
        .session_limits(SessionLimits::unlimited())
        .build();
    let fs = bash.fs();
    rt.block_on(async {
        fs.mkdir(Path::new("/data"), true).await.unwrap();
        fs.write_file(Path::new("/data/small.txt"), small_text().as_bytes())
            .await
            .unwrap();
        fs.write_file(Path::new("/data/big.txt"), big_text().as_bytes())
            .await
            .unwrap();
    });
    bash
}

fn verify(rt: &Runtime) {
    // Native and wasm must agree before timing them.
    let mut bash = make_bash(rt);
    for (name, template) in CALLS.iter().chain(BIG) {
        let native = rt.block_on(bash.exec(&render(template, "native"))).unwrap();
        let wasm = rt.block_on(bash.exec(&render(template, "wasm"))).unwrap();
        assert_eq!(native.exit_code, 0, "native/{name}: {}", native.stderr);
        assert_eq!(wasm.exit_code, 0, "wasm/{name}: {}", wasm.stderr);
        assert_eq!(native.stdout, wasm.stdout, "{name}: outputs differ");
    }
}

fn bench_calls(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    WasmCoreutil::warm_up().unwrap();
    verify(&rt);
    let mut bash = make_bash(&rt);

    let mut group = c.benchmark_group("coreutils_call");
    for (name, template) in CALLS {
        for imp in IMPLS {
            let script = render(template, imp);
            group.bench_with_input(BenchmarkId::new(*imp, name), &script, |b, s| {
                b.iter(|| rt.block_on(bash.exec(s)).unwrap());
            });
        }
    }
    group.finish();

    let mut group = c.benchmark_group("coreutils_throughput");
    group.throughput(Throughput::Bytes(BIG_BYTES as u64));
    group.sample_size(20);
    for (name, template) in BIG {
        for imp in IMPLS {
            let script = render(template, imp);
            group.bench_with_input(BenchmarkId::new(*imp, name), &script, |b, s| {
                b.iter(|| rt.block_on(bash.exec(s)).unwrap());
            });
        }
    }
    group.finish();
}

fn bench_session(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    WasmCoreutil::warm_up().unwrap();
    let mut group = c.benchmark_group("coreutils_session");
    for imp in IMPLS {
        let script = render("{sort} -r <<< $'b\\na\\nc'", imp);
        group.bench_with_input(BenchmarkId::new(*imp, "new_bash_sort"), &script, |b, s| {
            b.iter(|| {
                rt.block_on(async {
                    let mut bash = Bash::builder().wasm_coreutils().build();
                    bash.exec(s).await.unwrap()
                })
            });
        });
    }
    group.finish();
}

fn bench_parallel(c: &mut Criterion) {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    WasmCoreutil::warm_up().unwrap();
    let mut group = c.benchmark_group("coreutils_parallel");
    group.sample_size(20);
    for imp in IMPLS {
        let script = render("{sort} -r <<< $'b\\na\\nc' | {wc} -l", imp);
        for n in PARALLEL {
            group.throughput(Throughput::Elements(*n as u64));
            group.bench_with_input(BenchmarkId::new(*imp, n), n, |b, &n| {
                b.iter(|| {
                    rt.block_on(async {
                        let mut handles = Vec::with_capacity(n);
                        for _ in 0..n {
                            let s = script.clone();
                            handles.push(tokio::spawn(async move {
                                let mut bash = Bash::builder().wasm_coreutils().build();
                                bash.exec(&s).await.unwrap()
                            }));
                        }
                        for h in handles {
                            h.await.unwrap();
                        }
                    })
                });
            });
        }
    }
    group.finish();
}

criterion_group!(benches, bench_calls, bench_session, bench_parallel);
criterion_main!(benches);
