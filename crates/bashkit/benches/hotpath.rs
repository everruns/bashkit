//! Hot-path micro-benchmarks for the interpreter.
//!
//! Targets the specific code paths flagged in the perf analysis:
//!   - tight integer loops (set_variable + expand_variable)
//!   - command substitution (subshell state snapshot cost)
//!   - SHOPT flag checks (set -e, set -u, pipefail)
//!   - parameter expansion / attribute lookups
//!   - large pipelines
//!
//! Run with: `cargo bench --bench hotpath`
//! Save baseline: `cargo bench --bench hotpath -- --save-baseline before`
//! Compare:     `cargo bench --bench hotpath -- --baseline before`

use bashkit::Bash;
use criterion::{Criterion, criterion_group, criterion_main};
use tokio::runtime::Runtime;

fn run_script(rt: &Runtime, script: &str) {
    rt.block_on(async {
        let mut bash = Bash::new();
        let result = bash.exec(script).await.expect("exec failed");
        std::hint::black_box(result);
    });
}

fn bench_loops(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut g = c.benchmark_group("loops");
    g.bench_function("for_range_1k_arith", |b| {
        b.iter(|| {
            run_script(
                &rt,
                "n=0; for ((i=0; i<1000; i++)); do n=$((n+i)); done; echo $n",
            )
        })
    });
    g.bench_function("while_inc_1k", |b| {
        b.iter(|| {
            run_script(
                &rt,
                "i=0; while [[ $i -lt 1000 ]]; do i=$((i+1)); done; echo $i",
            )
        })
    });
    g.bench_function("for_list_100", |b| {
        b.iter(|| {
            run_script(
                &rt,
                "s=0; for x in $(seq 1 100); do s=$((s+x)); done; echo $s",
            )
        })
    });
    g.bench_function("nested_for_50x50", |b| {
        b.iter(|| {
            run_script(
                &rt,
                "n=0; for ((i=0;i<50;i++)); do for ((j=0;j<50;j++)); do n=$((n+1)); done; done; echo $n",
            )
        })
    });
    g.finish();
}

fn bench_variables(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut g = c.benchmark_group("variables");
    g.bench_function("assign_200", |b| {
        let mut script = String::new();
        for i in 0..200 {
            script.push_str(&format!("v{}={}\n", i, i));
        }
        script.push_str("echo done");
        b.iter(|| run_script(&rt, &script));
    });
    g.bench_function("read_200", |b| {
        let mut script = String::from("a=1\nb=2\nc=3\n");
        for _ in 0..200 {
            script.push_str("echo $a $b $c >/dev/null\n");
        }
        b.iter(|| run_script(&rt, &script));
    });
    g.bench_function("local_in_function", |b| {
        b.iter(|| {
            run_script(
                &rt,
                "f() { local a=1; local b=2; local c=3; echo $((a+b+c)); }; \
                 for i in $(seq 1 100); do f >/dev/null; done",
            )
        })
    });
    g.finish();
}

fn bench_cmdsubst(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut g = c.benchmark_group("cmdsubst");
    g.bench_function("subst_simple_100", |b| {
        let mut s = String::new();
        for _ in 0..100 {
            s.push_str("x=$(echo hello)\n");
        }
        b.iter(|| run_script(&rt, &s));
    });
    g.bench_function("subst_nested_3", |b| {
        b.iter(|| run_script(&rt, "echo $(echo $(echo $(echo deep)))"))
    });
    g.bench_function("subst_with_vars_50", |b| {
        let mut s = String::from("a=1\nb=2\nc=3\n");
        for _ in 0..50 {
            s.push_str("r=$(echo $a $b $c)\n");
        }
        b.iter(|| run_script(&rt, &s));
    });
    g.bench_function("subst_with_many_vars", |b| {
        let mut s = String::new();
        for i in 0..50 {
            s.push_str(&format!("v{}={}\n", i, i));
        }
        s.push_str("for i in $(seq 1 30); do r=$(echo done); done");
        b.iter(|| run_script(&rt, &s));
    });
    g.finish();
}

fn bench_shopt(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut g = c.benchmark_group("shopt");
    g.bench_function("strict_mode_1k_loop", |b| {
        b.iter(|| {
            run_script(
                &rt,
                "set -euo pipefail\nn=0; for ((i=0;i<1000;i++)); do n=$((n+i)); done; echo $n",
            )
        })
    });
    g.bench_function("plain_1k_loop", |b| {
        b.iter(|| {
            run_script(
                &rt,
                "n=0; for ((i=0;i<1000;i++)); do n=$((n+i)); done; echo $n",
            )
        })
    });
    g.finish();
}

fn bench_pipelines(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut g = c.benchmark_group("pipelines");
    g.bench_function("seq_grep_wc", |b| {
        b.iter(|| run_script(&rt, "seq 1 500 | grep 5 | wc -l"))
    });
    g.bench_function("seq_sort_uniq", |b| {
        b.iter(|| run_script(&rt, "seq 1 200 | sort | uniq | wc -l"))
    });
    g.finish();
}

fn bench_functions(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut g = c.benchmark_group("functions");
    g.bench_function("call_500", |b| {
        let s = "f() { echo $1; }; for i in $(seq 1 500); do f $i >/dev/null; done";
        b.iter(|| run_script(&rt, s));
    });
    g.bench_function("recursive_fib_10", |b| {
        let s = "fib() { if [[ $1 -le 1 ]]; then echo $1; else \
                 echo $(($(fib $(($1-1))) + $(fib $(($1-2))))); fi; }; fib 10";
        b.iter(|| run_script(&rt, s));
    });
    g.finish();
}

fn bench_param_exp(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut g = c.benchmark_group("param_exp");
    g.bench_function("default_op_500", |b| {
        let s = "x=hello\nfor i in $(seq 1 500); do y=${x:-default}; done; echo $y";
        b.iter(|| run_script(&rt, s));
    });
    g.bench_function("substring_500", |b| {
        let s = "x=helloworld\nfor i in $(seq 1 500); do y=${x:2:5}; done; echo $y";
        b.iter(|| run_script(&rt, s));
    });
    g.bench_function("uppercase_300", |b| {
        let s = "x=hello\nfor i in $(seq 1 300); do y=${x^^}; done; echo $y";
        b.iter(|| run_script(&rt, s));
    });
    g.finish();
}

fn bench_startup(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut g = c.benchmark_group("startup");
    g.bench_function("empty", |b| b.iter(|| run_script(&rt, "true")));
    g.bench_function("echo_hi", |b| b.iter(|| run_script(&rt, "echo hi")));
    g.bench_function("assign_echo", |b| {
        b.iter(|| run_script(&rt, "x=42; echo $x"))
    });
    g.finish();
}

criterion_group!(
    benches,
    bench_startup,
    bench_loops,
    bench_variables,
    bench_cmdsubst,
    bench_shopt,
    bench_pipelines,
    bench_functions,
    bench_param_exp,
);
criterion_main!(benches);
