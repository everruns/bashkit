//! Load test: many concurrent CPython (wasm) sessions in one process.
//!
//! For each concurrency level N, starts N tenants at once on a multi-thread
//! runtime. Every tenant gets its own `Bash` (own VFS) and runs 4 (or
//! `CPYTHON_LOAD_CALLS`)
//! `python3` invocations that write, read back and verify tenant-specific
//! data, so cross-tenant leakage or a wrong answer counts as a failure.
//! Reports throughput, per-call latency percentiles and peak RSS.
//!
//! Run: cargo run --release --example cpython_load --features cpython -- 1 16 64 256 1024

use bashkit::{Bash, CPython};
use std::time::{Duration, Instant};

/// Calls per tenant; override with `CPYTHON_LOAD_CALLS`.
fn calls() -> usize {
    std::env::var("CPYTHON_LOAD_CALLS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(4)
}

async fn tenant(id: usize) -> Result<Vec<Duration>, String> {
    let mut bash = Bash::builder().cpython().build();
    let mut latencies = Vec::with_capacity(calls());
    for call in 0..calls() {
        let script = format!(
            "python3 -c 'import json, os\n\
             p = \"/state.json\"\n\
             s = json.load(open(p)) if os.path.exists(p) else {{\"id\": {id}, \"n\": 0}}\n\
             assert s[\"id\"] == {id}, s\n\
             s[\"n\"] += 1\n\
             json.dump(s, open(p, \"w\"))\n\
             print(s[\"id\"], s[\"n\"])'"
        );
        let t = Instant::now();
        let r = bash.exec(&script).await.map_err(|e| e.to_string())?;
        latencies.push(t.elapsed());
        let want = format!("{id} {}\n", call + 1);
        if r.exit_code != 0 || r.stdout != want.as_str() {
            return Err(format!(
                "tenant {id} call {call}: exit {} stdout {:?} stderr {:?}",
                r.exit_code,
                r.stdout.to_string(),
                r.stderr.to_string()
            ));
        }
    }
    Ok(latencies)
}

fn peak_rss_mb() -> Option<f64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let line = status.lines().find(|l| l.starts_with("VmHWM:"))?;
    let kb: f64 = line.split_whitespace().nth(1)?.parse().ok()?;
    Some(kb / 1024.0)
}

fn pct(sorted: &[Duration], p: f64) -> f64 {
    let idx = ((sorted.len() as f64 - 1.0) * p).round() as usize;
    sorted[idx].as_secs_f64() * 1e3
}

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    let levels: Vec<usize> = std::env::args()
        .skip(1)
        .map(|a| a.parse().expect("concurrency level"))
        .collect();
    let levels = if levels.is_empty() {
        vec![1, 16, 64, 256]
    } else {
        levels
    };
    CPython::warm_up().expect("load CPython");
    println!(
        "| Tenants | Calls | Wall (s) | Calls/s | p50 (ms) | p99 (ms) | max (ms) | Failures | Peak RSS (MB) |"
    );
    println!(
        "|---------|-------|----------|---------|----------|----------|----------|----------|---------------|"
    );
    let mut failed = false;
    for n in levels {
        let start = Instant::now();
        let handles: Vec<_> = (0..n).map(|id| tokio::spawn(tenant(id))).collect();
        let mut all = Vec::with_capacity(n * calls());
        let mut failures = 0;
        for h in handles {
            match h.await.expect("tenant task") {
                Ok(l) => all.extend(l),
                Err(e) => {
                    failures += 1;
                    eprintln!("{e}");
                }
            }
        }
        let wall = start.elapsed().as_secs_f64();
        all.sort();
        failed |= failures > 0 || all.is_empty();
        println!(
            "| {n} | {} | {wall:.2} | {:.0} | {:.2} | {:.2} | {:.2} | {failures} | {} |",
            all.len(),
            all.len() as f64 / wall,
            pct(&all, 0.5),
            pct(&all, 0.99),
            pct(&all, 1.0),
            peak_rss_mb().map_or("n/a".to_string(), |m| format!("{m:.0}")),
        );
    }
    if failed {
        std::process::exit(1);
    }
}
