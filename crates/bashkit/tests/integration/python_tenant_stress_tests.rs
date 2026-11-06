//! Multi-tenant stress for the forked Monty `python3` (TM-PY-032, TM-PY-034).
//!
//! Many tenants share one process and a few worker threads. Hostile ones
//! (memory bombs, runaway loops, deep recursion, abandoned calls) run next to
//! well-behaved ones, round after round. Each tenant must hit only its own
//! limits, well-behaved output must be exact, and the process must survive.
//!
//! Decision: sized to finish in seconds so it runs on every PR. Set
//! `BASHKIT_STRESS_ROUNDS` for a longer soak locally or in a nightly job.

#![cfg(feature = "python")]

use bashkit::{Bash, PythonLimits};
use std::time::Duration;

fn rounds() -> usize {
    std::env::var("BASHKIT_STRESS_ROUNDS")
        .ok()
        .map(|v| {
            v.parse::<usize>()
                .ok()
                .filter(|n| (1..=1000).contains(n))
                .unwrap_or_else(|| panic!("BASHKIT_STRESS_ROUNDS must be 1..=1000, got {v:?}"))
        })
        .unwrap_or(3)
}

fn tenant(limit_mib: usize, max_ms: u64) -> Bash {
    Bash::builder()
        .python_with_limits(
            PythonLimits::default()
                .max_memory(limit_mib * 1024 * 1024)
                .max_duration(Duration::from_millis(max_ms))
                .max_recursion(200),
        )
        .env("BASHKIT_ALLOW_INPROCESS_PYTHON", "1")
        .build()
}

#[derive(Clone, Copy, Debug)]
enum Load {
    /// Computes and prints a known value.
    Good,
    /// One big up-front allocation over the limit.
    BigAlloc,
    /// Growth through many small objects over the limit.
    ManySmall,
    /// Never terminates on its own.
    Spin,
    /// Unbounded recursion.
    Recurse,
}

const LOADS: [Load; 5] = [
    Load::Good,
    Load::BigAlloc,
    Load::ManySmall,
    Load::Spin,
    Load::Recurse,
];

fn script(load: Load) -> &'static str {
    match load {
        Load::Good => "python3 -c \"print(sum(i * i for i in range(20000)))\"",
        Load::BigAlloc => "python3 -c \"x = 'a' * (64 * 1024 * 1024)\"",
        Load::ManySmall => "python3 -c \"x = [str(i) for i in range(3000000)]\"",
        Load::Spin => "python3 -c \"while True:\n    pass\"",
        Load::Recurse => "python3 -c \"def f(n):\n    return f(n + 1)\nf(0)\"",
    }
}

async fn run(load: Load) {
    // Only the runaway loop should end on time; the rest must end on their
    // own limit (memory, recursion) or finish, even in a slow debug build.
    let max_ms = if matches!(load, Load::Spin) {
        300
    } else {
        10_000
    };
    let mut bash = tenant(16, max_ms);
    let r = bash.exec(script(load)).await.unwrap();
    match load {
        Load::Good => {
            assert_eq!(r.exit_code, 0, "{load:?} stderr: {}", r.stderr);
            assert_eq!(r.stdout, "2666466670000\n", "{load:?}");
        }
        Load::BigAlloc | Load::ManySmall => {
            assert_ne!(r.exit_code, 0, "{load:?} stdout: {}", r.stdout);
            assert!(
                r.stderr.contains("MemoryError"),
                "{load:?} stderr: {}",
                r.stderr
            );
        }
        Load::Spin => {
            assert_eq!(r.exit_code, 124, "{load:?} stderr: {}", r.stderr);
        }
        Load::Recurse => {
            assert_ne!(r.exit_code, 0, "{load:?}");
            assert!(
                r.stderr.contains("RecursionError"),
                "{load:?} stderr: {}",
                r.stderr
            );
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn mixed_hostile_and_good_tenants_stay_isolated() {
    for _ in 0..rounds() {
        let mut handles = Vec::new();
        // 4 of each load = 20 tenants on 4 worker threads.
        for i in 0..20 {
            let load = LOADS[i % LOADS.len()];
            handles.push(tokio::spawn(run(load)));
        }
        for h in handles {
            h.await.unwrap();
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn abandoned_calls_do_not_poison_the_worker_threads() {
    // Drop python3 calls while they are suspended between VM steps (caller-side
    // timeout). The VM only yields at host calls, so the script loops on file
    // writes while holding 8 MiB. Dropping it frees that memory outside any
    // metering window. If that corrupted the per-thread meter, or charged a
    // dead budget, later VMs on the same threads (and the same session) would
    // fail or pass wrongly.
    const HOLD_AND_YIELD: &str = "python3 -c \"from pathlib import Path\nx = 'a' * (8 * 1024 * 1024)\np = Path('/tmp/spin')\nwhile True:\n    p.write_text('x')\"";
    for _ in 0..rounds() {
        let mut abandoned = Vec::new();
        for _ in 0..8 {
            abandoned.push(tokio::spawn(async {
                let mut bash = tenant(16, 5000);
                let dropped =
                    tokio::time::timeout(Duration::from_millis(50), bash.exec(HOLD_AND_YIELD))
                        .await
                        .is_err();
                assert!(dropped, "call finished instead of being abandoned");
                // Same session right after: its full budget must be available.
                let r = bash
                    .exec("python3 -c \"x = 'a' * (10 * 1024 * 1024)\nprint(len(x) // 1048576)\"")
                    .await
                    .unwrap();
                assert_eq!(r.exit_code, 0, "after abandon stderr: {}", r.stderr);
                assert_eq!(r.stdout, "10\n");
            }));
        }
        for h in abandoned {
            h.await.unwrap();
        }
        let mut after = Vec::new();
        for i in 0..8 {
            let load = if i % 2 == 0 {
                Load::Good
            } else {
                Load::BigAlloc
            };
            after.push(tokio::spawn(run(load)));
        }
        for h in after {
            h.await.unwrap();
        }
    }
}

#[tokio::test]
async fn one_session_reuses_its_budget_across_many_calls() {
    // A tenant that keeps 10 of its 16 MiB live per call, many times over. If
    // memory charged in one call were not released at its end, later calls in
    // the same session would trip MemoryError.
    let mut bash = tenant(16, 2000);
    for i in 0..(10 * rounds()) {
        let r = bash
            .exec("python3 -c \"x = 'a' * (10 * 1024 * 1024)\nprint(len(x) // 1048576)\"")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0, "call {i} stderr: {}", r.stderr);
        assert_eq!(r.stdout, "10\n", "call {i}");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn huge_single_allocations_are_refused_up_front() {
    // A request bigger than the machine has would make the global allocator
    // fail, and Rust aborts the whole process (every tenant) on allocation
    // failure. Monty must refuse these by size before allocating; run them
    // concurrently next to a well-behaved tenant to show the process survives.
    let bombs = [
        "x = 'a' * (10**12)",
        "x = b'a' * (10**12)",
        "x = [0] * (10**11)",
        "x = (1,) * (10**11)",
        "x = 'ab' * (2**62)",
        "x = 'x'.ljust(10**12)",
        "x = 'x'.zfill(10**12)",
        "x = 'a'.center(10**12)",
        "x = ' '.join(['a'] * 10**10)",
        "x = 10 ** (10**8)",
        "x = 2 << (10**10)",
        "x = list(range(10**11))",
        "x = '{:>1000000000000}'.format(1)",
    ];
    let mut handles = Vec::new();
    for code in bombs {
        handles.push(tokio::spawn(async move {
            let mut bash = tenant(64, 10_000);
            let r = bash.exec(&format!("python3 -c \"{code}\"")).await.unwrap();
            assert_ne!(r.exit_code, 0, "{code}: stdout {}", r.stdout);
            assert!(
                r.stderr.contains("MemoryError"),
                "{code}: stderr {}",
                r.stderr
            );
        }));
    }
    handles.push(tokio::spawn(run(Load::Good)));
    for h in handles {
        h.await.unwrap();
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn host_call_loops_stop_at_the_wall_clock_limit() {
    // Monty's clock pauses while the VM waits on the host, so a loop of cheap
    // file writes used to run ~10x past max_duration and never yield the
    // worker thread. The limit must bound wall-clock time.
    let mut bash = tenant(16, 500);
    let started = std::time::Instant::now();
    let r = bash
        .exec("python3 -c \"from pathlib import Path\np = Path('/tmp/spin')\nwhile True:\n    p.write_text('x')\"")
        .await
        .unwrap();
    let elapsed = started.elapsed();
    assert_eq!(r.exit_code, 124, "stderr: {}", r.stderr);
    assert!(
        elapsed < Duration::from_secs(2),
        "ran {elapsed:?} past a 500 ms limit"
    );
}
