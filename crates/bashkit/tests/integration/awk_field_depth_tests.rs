//! THREAT[TM-DOS-027]: field-operand recursion must share AWK depth accounting.
//! Run potentially aborting programs only in a supervised child test process.
//!
//! The child is bounded by OS CPU/stack/core limits plus a parent-side wall
//! watchdog, output drain cap and peak-RSS watchdog. Memory containment is
//! deliberately RSS-based rather than an address-space cap (`ulimit -v`):
//! AddressSanitizer reserves ~14 TiB of *virtual* address space for its
//! shadow map at startup, so any `RLIMIT_AS` cap makes a sanitizer-
//! instrumented child die in `ReserveShadowMemoryRange` before it runs a
//! line of test code. That is what reddened the nightly ASAN job on run 257
//! while required CI, which is not instrumented, stayed green on the same
//! commit. RSS is also the better proxy for the runaway *allocation* this
//! bound exists to catch, and it is the convention the other bounded-child
//! harnesses already use (`BOUNDED_CLI_LIMITS` in `threat_model_tests`,
//! the probe harness in `bashkit-cli/tests/cli_oneshot.rs`).

use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

const CHILD: &str = "BASHKIT_AWK_FIELD_DEPTH_CHILD";
const FINITE_PROGRAM: &str = "BEGIN { $0=\"a b\"; i=0; print $++i, $!0, $+2, $-0 }";

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

/// Peak-RSS ceiling for the bounded child, enforced by the parent watchdog.
///
/// Generous enough for an AddressSanitizer build of the whole integration
/// binary (instrumentation plus redzones cost several times the native
/// footprint) and still orders of magnitude below the multi-gigabyte growth
/// an unbounded field-operand recursion would reach, so the bound stays
/// discriminating without being toolchain-specific.
#[cfg(unix)]
const RSS_CAP_KB: u64 = 2 * 1024 * 1024;

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

#[test]
#[cfg(unix)]
fn awk_field_depth_bounded() {
    // OS CPU/stack/core limits, continuous output drains, a wall watchdog and
    // the RSS watchdog below also contain a regression that aborts/hangs. No
    // address-space cap: see the module header -- it would kill an ASAN child
    // at startup. Growth is bounded by RSS_CAP_KB instead.
    let limits = "ulimit -t 10 && ulimit -s 8192 && ulimit -c 0 && exec \"$@\"";
    let mut command = Command::new("/bin/sh");
    command.args(["-c", limits, "awk-depth"]);
    let mut child = command
        .arg(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "awk_field_depth_tests::awk_field_depth_child",
            "--nocapture",
        ])
        .env(CHILD, "1")
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
        "child exceeded bounds (peak RSS {} KiB, cap {RSS_CAP_KB} KiB)",
        max_rss_kb.map_or("unavailable".to_owned(), |rss| rss.to_string())
    );
    // The loop leaves via `try_wait` before it re-checks the cap, so a child
    // that breached on its last poll and then exited on its own reaches here
    // with `stopped` false. Assert the peak so that still fails closed.
    assert!(
        max_rss_kb.is_none_or(|rss| rss <= RSS_CAP_KB),
        "peak child RSS {} KiB exceeds {RSS_CAP_KB} KiB bound",
        max_rss_kb.unwrap_or_default()
    );
    assert!(
        status.success(),
        "child status {status}: {}{}",
        String::from_utf8_lossy(&stdout),
        String::from_utf8_lossy(&stderr)
    );
    assert!(
        String::from_utf8_lossy(&stdout).contains("1 passed"),
        "child test did not run"
    );
}

#[test]
fn awk_field_depth_child() {
    if std::env::var_os(CHILD).is_none() {
        return;
    }
    // Keep the production API on a typical 2 MiB embedding thread stack.
    std::thread::Builder::new()
        .stack_size(2 * 1024 * 1024)
        .spawn(|| {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(async {
                    let mut bash = bashkit::Bash::new();
                    for token in ["!", "- ", "+ ", "! + - "] {
                        for prefix in ["$", "++$", "getline $", "$++$"] {
                            let program =
                                format!("BEGIN {{ print {prefix}{}1 }}", token.repeat(200));
                            for file in [false, true] {
                                let command = if file {
                                    bash.fs()
                                        .write_file(
                                            Path::new("/tmp/payload.awk"),
                                            program.as_bytes(),
                                        )
                                        .await
                                        .unwrap();
                                    "awk -f /tmp/payload.awk".to_owned()
                                } else {
                                    format!("awk '{program}'")
                                };
                                let result = bash.exec(&command).await.unwrap();
                                assert_eq!(
                                    result.exit_code, 2,
                                    "token={token}, prefix={prefix}, file={file}"
                                );
                                assert!(result.stdout.is_empty());
                                assert!(
                                    result.stderr.contains("nesting too deep"),
                                    "{}",
                                    result.stderr
                                );
                                assert!(result.stderr.len() <= 1024);
                                let recovered =
                                    bash.exec("awk 'BEGIN { print 42 }'").await.unwrap();
                                assert_eq!(recovered.exit_code, 0);
                                assert_eq!(recovered.stdout, "42\n");
                                assert!(recovered.stderr.is_empty());
                            }
                        }
                    }
                    // BEGIN block + print + expression + field consume four levels.
                    for (count, status) in [(95, 0), (96, 0), (97, 2)] {
                        for token in ["! ", "- ", "+ "] {
                            let program = format!(
                                "BEGIN {{ $0=\"one two\"; print ${}0 }}",
                                token.repeat(count)
                            );
                            let result = bash.exec(&format!("awk '{program}'")).await.unwrap();
                            assert_eq!(
                                result.exit_code, status,
                                "count={count}, token={token}: {}",
                                result.stderr
                            );
                            if status == 0 {
                                assert!(result.stderr.is_empty());
                                let expected = if token == "! " && count % 2 == 1 {
                                    "one\n"
                                } else {
                                    "one two\n"
                                };
                                assert_eq!(result.stdout, expected);
                            } else {
                                assert!(result.stderr.contains("101 levels, max 100"));
                            }
                        }
                    }
                    // Malformed operands remain syntax errors; recovery works afterwards.
                    let bad = bash.exec("awk 'BEGIN { print $! }'").await.unwrap();
                    assert_eq!(bad.exit_code, 1);
                    assert!(bad.stderr.contains("syntax error"));
                    let good = bash.exec(&format!("awk '{FINITE_PROGRAM}'")).await.unwrap();
                    assert_eq!(good.exit_code, 0);
                    assert_eq!(good.stdout, "a a b a b\n");
                    assert!(good.stderr.is_empty());
                });
        })
        .unwrap()
        .join()
        .unwrap();
}

#[tokio::test]
async fn finite_field_operands_match_gawk() {
    let oracle = match Command::new("gawk").arg(FINITE_PROGRAM).output() {
        Ok(output) => output,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
        Err(error) => panic!("run gawk oracle: {error}"),
    };
    assert!(oracle.status.success());

    let mut bash = bashkit::Bash::new();
    let result = bash.exec(&format!("awk '{FINITE_PROGRAM}'")).await.unwrap();
    assert_eq!(result.exit_code, oracle.status.code().unwrap());
    assert_eq!(result.stdout.as_bytes(), oracle.stdout);
    assert_eq!(result.stderr.as_bytes(), oracle.stderr);
}
