//! Multi-tenant stress for the vendored ZapCode `ts` (TM-TS-026).
//!
//! Many tenants share one process and a few worker threads. Hostile ones
//! (memory doubling, shared-string amplification, runaway loops, deep
//! recursion, abandoned calls) run next to well-behaved ones, round after
//! round. Each tenant must hit only its own limits, well-behaved output must
//! be exact, and the process must survive.
//!
//! Decision: sized to finish in seconds so it runs on every PR. Set
//! `BASHKIT_STRESS_ROUNDS` for a longer soak locally or in a nightly job.

#![cfg(feature = "typescript")]

use bashkit::{Bash, SessionLimits, TypeScriptLimits};
use std::time::Duration;

/// Largest `BASHKIT_STRESS_ROUNDS` accepted; also sizes the session exec()
/// headroom in `tenant` so the soak never fails on that cap instead.
const MAX_ROUNDS: usize = 1000;

/// Allocation budget large enough that `max_duration` is what stops a runaway
/// loop. ZapCode charges one allocation per VM stack push and never decrements
/// the counter (`zapcode_vendor/sandbox.rs`, `track_allocation`), so
/// `max_allocations` is really a per-run budget of pushes: a `while (true) {}`
/// spends about one per iteration and a release build burns the 1,000,000
/// default in a few milliseconds, while a debug build reaches a 300 ms
/// `max_duration` first. That made the release-mode nightly soak fail on a
/// tenant whose debug-mode PR run was green. This is ~100x the pushes a 300 ms
/// loop can make, so the clock binds in either profile.
const SPIN_ALLOCATIONS: usize = 1_000_000_000;

fn rounds() -> usize {
    std::env::var("BASHKIT_STRESS_ROUNDS")
        .ok()
        .map(|v| {
            v.parse::<usize>()
                .ok()
                .filter(|n| (1..=MAX_ROUNDS).contains(n))
                .unwrap_or_else(|| {
                    panic!("BASHKIT_STRESS_ROUNDS must be 1..={MAX_ROUNDS}, got {v:?}")
                })
        })
        .unwrap_or(3)
}

fn tenant(limit_mib: usize, max_ms: u64) -> Bash {
    Bash::builder()
        .typescript_with_limits(
            TypeScriptLimits::default()
                .max_memory(limit_mib * 1024 * 1024)
                .max_duration(Duration::from_millis(max_ms)),
        )
        .session_limits(SessionLimits::default().max_exec_calls(20 * MAX_ROUNDS as u64))
        .build()
}

/// Run `script` from a VFS file so template literals need no shell escaping.
async fn exec_ts(bash: &mut Bash, script: &str) -> bashkit::ExecResult {
    bash.exec(&format!(
        "cat > /tmp/t.ts <<'TSEOF'\n{script}\nTSEOF\nts /tmp/t.ts"
    ))
    .await
    .unwrap()
}

#[derive(Clone, Copy, Debug)]
enum Load {
    /// Computes and prints a known value.
    Good,
    /// Doubles a string until past the limit.
    Doubling,
    /// One big string shared many times, then joined.
    Amplify,
    /// Never terminates on its own.
    Spin,
    /// Unbounded recursion.
    Recurse,
}

const LOADS: [Load; 5] = [
    Load::Good,
    Load::Doubling,
    Load::Amplify,
    Load::Spin,
    Load::Recurse,
];

fn script(load: Load) -> &'static str {
    match load {
        Load::Good => "let t = 0; for (let i = 0; i < 20000; i++) { t += i * i; } console.log(t)",
        Load::Doubling => {
            "let s = 'x'.repeat(1e6); for (let i = 0; i < 30; i++) { s = `${s}${s}`; } console.log(s.length)"
        }
        Load::Amplify => {
            "const s = 'x'.repeat(2e6); const a = [s, s, s, s, s, s, s, s, s, s]; console.log(a.join('').length)"
        }
        Load::Spin => "while (true) {}",
        Load::Recurse => "function f(n) { return f(n + 1); } f(0)",
    }
}

async fn run(load: Load) {
    // The runaway loop ends on one of its own resource limits; the rest must
    // end on their own limit (memory, stack) or finish, even in a slow debug
    // build.
    let max_ms = if matches!(load, Load::Spin) {
        300
    } else {
        10_000
    };
    let mut bash = tenant(16, max_ms);
    let r = exec_ts(&mut bash, script(load)).await;
    match load {
        Load::Good => {
            assert_eq!(r.exit_code, 0, "{load:?} stderr: {}", r.stderr);
            assert_eq!(r.stdout, "2666466670000\n", "{load:?}");
        }
        Load::Doubling | Load::Amplify => {
            assert_ne!(r.exit_code, 0, "{load:?} stdout: {}", r.stdout);
            assert!(
                r.stderr.contains("limit exceeded"),
                "{load:?} stderr: {}",
                r.stderr
            );
        }
        Load::Spin => {
            // Whichever of the two limits the loop reaches first bounds it,
            // and which one that is depends on how fast the build is (see
            // SPIN_ALLOCATIONS). Both are correct bounded termination; the
            // two tests at the end of this file pin each limit on its own.
            assert_ne!(r.exit_code, 0, "{load:?} stdout: {}", r.stdout);
            assert!(
                r.stderr.contains("timed out") || r.stderr.contains("allocation limit exceeded"),
                "{load:?} stderr: {}",
                r.stderr
            );
        }
        Load::Recurse => {
            assert_ne!(r.exit_code, 0, "{load:?}");
            assert!(
                r.stderr.contains("stack overflow"),
                "{load:?} stderr: {}",
                r.stderr
            );
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ts_mixed_hostile_and_good_tenants_stay_isolated() {
    for _ in 0..rounds() {
        let mut handles = Vec::new();
        // 4 of each load = 20 tenants on 4 worker threads.
        for i in 0..20 {
            handles.push(tokio::spawn(run(LOADS[i % LOADS.len()])));
        }
        for h in handles {
            h.await.unwrap();
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ts_abandoned_calls_do_not_poison_the_worker_threads() {
    // Drop ts calls while they are suspended on a host call (caller-side
    // timeout), holding 4 MiB (each host call also snapshots the VM heap). If
    // dropping them corrupted the per-thread meter, later runs on the same
    // threads and session would fail or pass wrongly.
    const HOLD_AND_YIELD: &str = "const x = 'a'.repeat(4 * 1024 * 1024); while (true) { await writeFile('/tmp/spin', 'x'); }";
    const AFTER: &str = "const x = 'a'.repeat(9 * 1024 * 1024); console.log(x.length / 1048576)";
    for _ in 0..rounds() {
        let mut abandoned = Vec::new();
        for _ in 0..8 {
            abandoned.push(tokio::spawn(async {
                let mut bash = tenant(16, 5000);
                let script =
                    format!("cat > /tmp/h.ts <<'TSEOF'\n{HOLD_AND_YIELD}\nTSEOF\nts /tmp/h.ts");
                let outcome =
                    tokio::time::timeout(Duration::from_millis(50), bash.exec(&script)).await;
                assert!(
                    outcome.is_err(),
                    "call finished instead of being abandoned: {:?}",
                    outcome.map(|r| r.map(|r| (r.exit_code, r.stderr.to_string())))
                );
                let r = exec_ts(&mut bash, AFTER).await;
                assert_eq!(r.exit_code, 0, "after abandon stderr: {}", r.stderr);
                assert_eq!(r.stdout, "9\n");
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
                Load::Doubling
            };
            after.push(tokio::spawn(run(load)));
        }
        for h in after {
            h.await.unwrap();
        }
    }
}

#[tokio::test]
async fn ts_one_session_reuses_its_budget_across_many_calls() {
    // Keeps 10 of 16 MiB live per call, many times over. Memory charged in one
    // call must be released at its end, or later calls would trip the limit.
    let mut bash = tenant(16, 2000);
    for i in 0..(10 * rounds()) {
        let r = exec_ts(
            &mut bash,
            "const x = 'a'.repeat(9 * 1024 * 1024); console.log(x.length / 1048576)",
        )
        .await;
        assert_eq!(r.exit_code, 0, "call {i} stderr: {}", r.stderr);
        assert_eq!(r.stdout, "9\n", "call {i}");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ts_huge_single_allocations_are_refused_up_front() {
    // A request bigger than the machine has makes the global allocator fail,
    // and Rust aborts the whole process (every tenant). These must be refused
    // by size before allocating; run them next to a well-behaved tenant.
    let bombs = [
        "'a'.padStart(2 ** 40)",
        "'a'.padEnd(2 ** 40, 'xy')",
        "'a'.repeat(2 ** 40)",
        "'ab'.repeat(5e6).repeat(5e6)",
        "const s = 'x'.repeat(5e6); [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16].join(s)",
        "const s = 'x'.repeat(5e6); JSON.stringify([s, s, s, s, s, s, s, s, s, s, s, s, s, s, s, s])",
        "const s = 'x'.repeat(5e6); `${[s, s, s, s, s, s, s, s, s, s, s, s, s, s, s, s]}!`",
        "const s = 'x'.repeat(5e6); console.log(s, s, s, s, s, s, s, s, s, s, s, s, s, s, s, s)",
        "'x'.repeat(5e6).split('')",
        "'x'.repeat(1e5).replace(/x/g, 'y'.repeat(1e4))",
        "'x'.repeat(1e4).replaceAll('x', 'y'.repeat(1e5))",
    ];
    let mut handles = Vec::new();
    for code in bombs {
        handles.push(tokio::spawn(async move {
            let mut bash = tenant(64, 10_000);
            let r = exec_ts(&mut bash, code).await;
            assert_ne!(r.exit_code, 0, "{code}: stdout {}", r.stdout);
            assert!(
                r.stdout.len() < 1024,
                "{code}: printed {} bytes",
                r.stdout.len()
            );
        }));
    }
    handles.push(tokio::spawn(run(Load::Good)));
    for h in handles {
        h.await.unwrap();
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ts_host_call_loops_stop_at_the_wall_clock_limit() {
    // A loop of cheap host calls must still stop at max_duration (wall clock),
    // not only at VM time.
    let mut bash = tenant(16, 500);
    let started = std::time::Instant::now();
    let r = exec_ts(
        &mut bash,
        "while (true) { await writeFile('/tmp/spin', 'x'); }",
    )
    .await;
    let elapsed = started.elapsed();
    assert_eq!(r.exit_code, 124, "stderr: {}", r.stderr);
    assert!(
        elapsed < Duration::from_secs(2),
        "ran {elapsed:?} past a 500 ms limit"
    );
}

/// A pure runaway loop, with no host calls to carry the deadline check, stops
/// on `max_duration` once its allocation budget is not what binds (TM-TS-027).
#[tokio::test]
async fn ts_runaway_loop_stops_on_the_wall_clock() {
    let mut bash = Bash::builder()
        .typescript_with_limits(
            TypeScriptLimits::default()
                .max_memory(16 * 1024 * 1024)
                .max_duration(Duration::from_millis(300))
                .max_allocations(SPIN_ALLOCATIONS),
        )
        .build();
    let r = exec_ts(&mut bash, script(Load::Spin)).await;
    assert_eq!(r.exit_code, 124, "stderr: {}", r.stderr);
    assert!(r.stderr.contains("timed out"), "stderr: {}", r.stderr);
}

/// The allocation budget bounds the same loop on its own, well before a
/// generous `max_duration`, because the push counter never decreases.
#[tokio::test]
async fn ts_runaway_loop_stops_on_the_allocation_budget() {
    let mut bash = Bash::builder()
        .typescript_with_limits(
            TypeScriptLimits::default()
                .max_memory(16 * 1024 * 1024)
                .max_duration(Duration::from_secs(60))
                .max_allocations(50_000),
        )
        .build();
    let r = exec_ts(&mut bash, script(Load::Spin)).await;
    assert_eq!(r.exit_code, 1, "stderr: {}", r.stderr);
    assert!(
        r.stderr.contains("allocation limit exceeded"),
        "stderr: {}",
        r.stderr
    );
}
