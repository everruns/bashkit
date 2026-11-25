//! Shared supervised-child launcher for tests whose failure mode can abort the
//! process (native stack overflow, runaway recursion or allocation).
//!
//! Re-runs one `#[test]` of this binary in a child, bounded by OS CPU/stack/
//! core limits plus a parent-side wall watchdog, output drain cap and peak-RSS
//! watchdog. A regression then fails one test with a readable status instead
//! of killing the whole integration binary.
//!
//! Memory containment is deliberately RSS-based rather than an address-space
//! cap (`ulimit -v`): AddressSanitizer reserves ~14 TiB of *virtual* address
//! space for its shadow map at startup, so any `RLIMIT_AS` cap makes a
//! sanitizer-instrumented child die in `ReserveShadowMemoryRange` before it
//! runs a line of test code. That is what reddened the nightly ASAN job on run
//! 257 while required CI, which is not instrumented, stayed green on the same
//! commit (see `bounded_child_launcher_scan_tests`).
//!
//! Usage: a parent test calls [`run`] with an env var name and the exact test
//! path of a child test; the child test returns immediately unless that env
//! var is set, then runs its body on [`EMBEDDING_STACK`] via [`on_small_stack`].

// The launcher is unix-only; the child helpers are shared by every platform.
#![cfg_attr(not(unix), allow(dead_code, unused_imports))]

use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// Typical embedding thread stack (Rust's spawned-thread default). Child
/// bodies run here so a native-stack regression shows up at production size.
pub const EMBEDDING_STACK: usize = 2 * 1024 * 1024;

/// Peak-RSS ceiling for the bounded child, enforced by the parent watchdog.
///
/// Generous enough for an AddressSanitizer build of the whole integration
/// binary (instrumentation plus redzones cost several times the native
/// footprint) and still orders of magnitude below the multi-gigabyte growth
/// an unbounded recursion would reach, so the bound stays discriminating
/// without being toolchain-specific.
#[cfg(unix)]
const RSS_CAP_KB: u64 = 2 * 1024 * 1024;

fn drain(
    mut stream: impl Read + Send + 'static,
    exceeded: Arc<AtomicBool>,
) -> std::thread::JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        let mut output = Vec::new();
        let mut chunk = [0; 4096];
        loop {
            let n = stream.read(&mut chunk).expect("read child output");
            if n == 0 {
                return output;
            }
            if output.len() + n > 65536 {
                exceeded.store(true, Ordering::Relaxed);
            } else {
                output.extend_from_slice(&chunk[..n]);
            }
        }
    })
}

/// Best-effort peak-RSS readback for a live child via unix `ps`.
///
/// `None` only means this poll found no live PID; the CPU/stack/wall/output
/// bounds hold regardless of whether the numeric readback is available.
#[cfg(unix)]
fn sample_rss_kb(pid: u32) -> Option<u64> {
    let out = Command::new("ps")
        .args(["-o", "rss=", "-p", &pid.to_string()])
        .output()
        .ok()?;
    String::from_utf8(out.stdout)
        .ok()?
        .trim()
        .parse::<u64>()
        .ok()
}

/// Run `exact_test` of this binary in a bounded child with `env` set, and
/// assert it stayed in bounds, exited successfully and actually ran.
#[cfg(unix)]
pub fn run(label: &str, env: &str, exact_test: &str) {
    // OS CPU/stack/core limits, continuous output drains, a wall watchdog and
    // the RSS watchdog below also contain a regression that aborts/hangs. No
    // address-space cap: see the module header -- it would kill an ASAN child
    // at startup. Growth is bounded by RSS_CAP_KB instead.
    let limits = "ulimit -t 10 && ulimit -s 8192 && ulimit -c 0 && exec \"$@\"";
    let mut child = Command::new("/bin/sh")
        .args(["-c", limits, label])
        .arg(std::env::current_exe().unwrap())
        .args(["--exact", exact_test, "--nocapture", "--test-threads=1"])
        .env(env, "1")
        // A child that dies on a signal never flushes its coverage profile;
        // keep it out of tarpaulin's merge set.
        .env("LLVM_PROFILE_FILE", bashkit::testing::child_profile_path())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn bounded child");
    let exceeded = Arc::new(AtomicBool::new(false));
    let child_id = child.id();
    let stdout = drain(child.stdout.take().unwrap(), exceeded.clone());
    let stderr = drain(child.stderr.take().unwrap(), exceeded.clone());
    let start = Instant::now();
    let mut stopped = false;
    let mut max_rss_kb: Option<u64> = None;
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        // The child has not been reaped yet, so it is alive for this readback.
        if let Some(rss) = sample_rss_kb(child_id) {
            max_rss_kb = Some(max_rss_kb.unwrap_or(0).max(rss));
        }
        let over_rss = max_rss_kb.is_some_and(|rss| rss > RSS_CAP_KB);
        if exceeded.load(Ordering::Relaxed) || over_rss || start.elapsed() > Duration::from_secs(20)
        {
            stopped = true;
            child.kill().unwrap();
            break child.wait().unwrap();
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let stdout = stdout.join().unwrap();
    let stderr = stderr.join().unwrap();
    assert!(
        !stopped && !exceeded.load(Ordering::Relaxed),
        "{label}: child exceeded bounds (peak RSS {} KiB, cap {RSS_CAP_KB} KiB)",
        max_rss_kb.map_or("unavailable".to_owned(), |rss| rss.to_string())
    );
    // The loop leaves via `try_wait` before it re-checks the cap, so a child
    // that breached on its last poll and then exited on its own reaches here
    // with `stopped` false. Assert the peak so that still fails closed.
    assert!(
        max_rss_kb.is_none_or(|rss| rss <= RSS_CAP_KB),
        "{label}: peak child RSS {} KiB exceeds {RSS_CAP_KB} KiB bound",
        max_rss_kb.unwrap_or_default()
    );
    assert!(
        status.success(),
        "{label}: child status {status}: {}{}",
        String::from_utf8_lossy(&stdout),
        String::from_utf8_lossy(&stderr)
    );
    assert!(
        String::from_utf8_lossy(&stdout).contains("1 passed"),
        "{label}: child test did not run"
    );
}

/// Whether this process is the supervised child selected by `env`.
pub fn is_child(env: &str) -> bool {
    std::env::var_os(env).is_some()
}

/// Run an async body on a fresh current-thread runtime, on a thread with the
/// typical embedding stack size.
pub fn on_small_stack<F, Fut>(body: F)
where
    F: FnOnce() -> Fut + Send + 'static,
    Fut: std::future::Future<Output = ()>,
{
    std::thread::Builder::new()
        .stack_size(EMBEDDING_STACK)
        .spawn(move || {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(body())
        })
        .unwrap()
        .join()
        .unwrap();
}
