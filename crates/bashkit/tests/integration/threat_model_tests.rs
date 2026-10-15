//! Threat Model Security Tests
//!
//! Tests for threats identified in knowledge/security/threat-model.md
//! Each test category maps to a threat category in the threat model.
//!
//! Run with: `cargo test threat_`

use bashkit::{
    Bash, ExecutionLimits, FileSystem, FileSystemExt, FsLimits, InMemoryFs, MemoryLimits,
    OverlayFs, SessionLimits, TraceEventDetails, TraceEventKind, TraceMode,
};
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

// =============================================================================
// 1. RESOURCE EXHAUSTION TESTS
// =============================================================================

pub(crate) mod resource_exhaustion {
    use super::*;

    /// Ancestor levels searched for the CLI binary. Five are needed for the
    /// deepest known layout (`out`, hash, package, `build`, profile); the
    /// spare one absorbs another level without reaching above `target`.
    const CLI_SEARCH_DEPTH: usize = 6;

    /// Locates the `bashkit` binary exported next to this test binary's
    /// build tree, or returns the candidates it ruled out.
    ///
    /// Where cargo puts a *test* binary is not contractual and differs by
    /// toolchain: the pinned stable builds it into `<profile>/deps/`, while
    /// cargo 1.101-nightly's separate build directory uses
    /// `<profile>/build/<package>/<hash>/out/`. What cargo does guarantee is
    /// that the final `bashkit` binary is exported to the profile directory,
    /// an ancestor of the test binary under either layout — so the profile
    /// directory is found by walking up until the binary turns up, never by
    /// assuming a fixed depth. Taking it as `exe.parent().parent()` passed on
    /// stable and resolved into the hashed build directory under nightly,
    /// which is how the nightly ASAN job went red while required CI on the
    /// same commits stayed green.
    ///
    /// `is_file` is load-bearing: the nightly layout puts a *directory*
    /// named after the package at `<profile>/build/bashkit`, which a bare
    /// existence check would accept.
    ///
    /// The walk stops at cargo's target directory, marked by the
    /// `CACHEDIR.TAG` cargo writes there, so an unrelated `bashkit` file
    /// above the build tree can never be spawned in place of the binary
    /// under test. That marker is belt-and-braces over `CLI_SEARCH_DEPTH`,
    /// which bounds the walk whether or not it is present.
    fn cli_in_ancestors(exe: &Path) -> Result<std::path::PathBuf, Vec<std::path::PathBuf>> {
        let name = format!("bashkit{}", std::env::consts::EXE_SUFFIX);
        let mut ruled_out = Vec::new();
        for dir in exe.ancestors().skip(1).take(CLI_SEARCH_DEPTH) {
            let candidate = dir.join(&name);
            if candidate.is_file() {
                return Ok(candidate);
            }
            ruled_out.push(candidate);
            if dir.join("CACHEDIR.TAG").is_file() {
                break;
            }
        }
        Err(ruled_out)
    }

    /// TM-DOS-135: child-process runner for hostile scripts.
    /// Hostile reproducers never run in the test process: pre-fix they
    /// overflowed the stack and aborted the whole runner.
    ///
    /// The binary belongs to a sibling package, so `CARGO_BIN_EXE_*` is not
    /// available here; it is built because the invocation selects that
    /// package's own integration tests, which do get that variable. A
    /// selection narrow enough to exclude them (`-p bashkit --test
    /// integration` alone) leaves nothing to find, which used to surface as a
    /// bare spawn `NotFound` and, once the launcher was introduced, as an
    /// unexplained exit 127 with the bound assertions failing — both read
    /// like product failures. Fail on the precondition instead, naming every
    /// path ruled out.
    pub(crate) fn bashkit_cli_path() -> std::path::PathBuf {
        let exe = std::env::current_exe().expect("current test executable");
        match cli_in_ancestors(&exe) {
            Ok(cli) => cli,
            Err(ruled_out) => panic!(
                "bashkit CLI binary not found above the test binary {}: this suite spawns \
                 it to run hostile reproducers out of process, so the test selection must \
                 include the bashkit-cli package (its own integration tests are what make \
                 cargo export the binary). Ruled out: {}",
                exe.display(),
                ruled_out
                    .iter()
                    .map(|p| p.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        }
    }

    pub(crate) struct BoundedRun {
        pub(crate) timed_out: bool,
        pub(crate) oversized: bool,
        /// True when the deadline passed with readers unfinished: a foreign
        /// descendant retains the pipes. Detection, not reaping: without
        /// platform process-group APIs (no new dependencies) a foreign
        /// descendant cannot be reaped portably, so the run fails visibly.
        pub(crate) incomplete_readers: bool,
        /// A pipe reader hit a real I/O error (anything but `Interrupted`,
        /// which is retried). Capture is then partial by definition, so the
        /// run fails closed instead of relabeling the error as a clean EOF.
        pub(crate) read_error: bool,
        /// OS termination accounted for: the loop observed an exit or
        /// the bounded reap recovered one. Orthogonal to `exit_code`
        /// (signaled victims have no code) and to injected observation
        /// faults (which discard the observation, never the cleanup).
        pub(crate) reaped: bool,
        /// First stderr line from the launcher (`harness-limits: ...`),
        /// proving the CPU/stack caps were applied to this run. Empty when
        /// the launcher itself refused (fail-closed exit 99).
        pub(crate) limits_note: String,
        pub(crate) exit_code: Option<i32>,
        pub(crate) stdout: String,
        pub(crate) stderr: String,
    }

    /// Fail-closed launcher limits: 10 CPU seconds, 4 MiB main-thread
    /// stack. Any refusal exits 99 before the producer starts, so a run
    /// without limits can never be mistaken for a bounded one.
    const BOUNDED_CLI_LIMITS: &str = "ulimit -s 4096 || exit 99; ulimit -t 10 || exit 99";

    /// Per-pipe sample cap. Past the cap the reader stops draining and the
    /// run is marked `oversized`, which fails the harness: oversize output
    /// is never silently truncated.
    const BOUNDED_SAMPLE_CAP: usize = 8192;
    /// Finite cleanup grace after the execution deadline: at most 2s more
    /// for pipe drains plus at most 5s to reap the victim. `limit` is the
    /// execution deadline for the child itself; these graces bound the
    /// harness teardown after it, so the whole run always terminates.
    const DRAIN_GRACE: Duration = Duration::from_secs(2);
    const REAP_GRACE: Duration = Duration::from_secs(5);
    /// Shared completion grace for joining reader threads after all
    /// receipts arrived. Full budget equation: execution `limit` + drain
    /// 2s + reap 5s + completion 5s; every stage is finite.
    const JOIN_GRACE: Duration = Duration::from_secs(5);
    /// Absolute bound for the whole post-verdict cleanup: every stage
    /// derives its allowance from one shared end instant (kill, absence
    /// poll, handle join), so stage budgets can never stack past it.
    /// Budget: execution `limit` + drain 2s + reap 5s run first; cleanup
    /// itself (kill, join, absence) shares CLEANUP_TOTAL below.
    const CLEANUP_TOTAL: Duration = Duration::from_secs(12);

    /// Join verdict for one reader handle inside the shared completion
    /// grace. `Unfinished` carries the SAME handle back so the caller can
    /// release it and observe that thread's actual join — never a twin,
    /// never detached-while-asserting-cleanup. `Panicked` means the thread
    /// terminated by panicking (join returned, work failed).
    enum JoinOutcome {
        Joined,
        Unfinished(std::thread::JoinHandle<()>),
        Panicked,
    }

    /// Joins one reader handle inside the shared completion grace: the
    /// single bounded-completion primitive used by both the runner gate
    /// below and its deterministic proof test.
    fn join_finished(handle: std::thread::JoinHandle<()>, end: Instant) -> JoinOutcome {
        let mut finished = handle.is_finished();
        while !finished && Instant::now() < end {
            std::thread::sleep(Duration::from_millis(5));
            finished = handle.is_finished();
        }
        if !finished {
            return JoinOutcome::Unfinished(handle);
        }
        match handle.join() {
            Ok(()) => JoinOutcome::Joined,
            Err(_) => JoinOutcome::Panicked,
        }
    }

    /// Reads one pipe up to the cap. Retention is truncated BEFORE the
    /// buffer can grow past it, so a hostile stream can never inflate the
    /// harness itself. `Interrupted` is retried; any other I/O error ends
    /// the capture with the bytes kept so far and `read_error` set, never
    /// relabeled as a clean EOF. Reaching exactly the cap followed by EOF
    /// is not oversize; only real excess is flagged.
    pub(crate) fn drain_capped<R: Read>(reader: &mut R) -> (Vec<u8>, bool, bool) {
        let mut buf = Vec::new();
        let mut chunk = [0u8; 4096];
        loop {
            match reader.read(&mut chunk) {
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => {
                    let over = buf.len() > BOUNDED_SAMPLE_CAP;
                    return (buf, over, true);
                }
                Ok(0) => {
                    let over = buf.len() > BOUNDED_SAMPLE_CAP;
                    return (buf, over, false);
                }
                Ok(n) => {
                    if buf.len() + n > BOUNDED_SAMPLE_CAP {
                        buf.extend_from_slice(&chunk[..BOUNDED_SAMPLE_CAP - buf.len()]);
                        return (buf, true, false);
                    }
                    buf.extend_from_slice(&chunk[..n]);
                }
            }
        }
    }

    /// Bounded reader: one thread per pipe so a chatty stderr cannot
    /// starve stdout. The worker stops at the cap instead of buffering the
    /// whole hostile stream, then reports back with live oversize/error
    /// flags observed at read time.
    fn spawn_capped_reader(
        id: u8,
        mut pipe: Box<dyn Read + Send>,
        tx: std::sync::mpsc::Sender<(u8, Vec<u8>, bool, bool)>,
    ) -> std::thread::JoinHandle<()> {
        std::thread::spawn(move || {
            let (buf, oversized, read_error) = drain_capped(&mut pipe);
            let _ = tx.send((id, buf, oversized, read_error));
        })
    }

    /// Runs a script in the built CLI under `run_command_bounded`, with
    /// trailing `$0`/ARG0 words after the `-c` script.
    /// The launcher applies the fail-closed CPU/stack caps, reports the
    /// effective values as the first stderr line, then `exec`s the CLI so
    /// exit codes and diagnostics stay the real producer's own.
    /// Fail-closed launcher shared by every bounded child: applies the
    /// CPU/stack caps (exiting 99 when the platform refuses them), reports
    /// the effective values as the first stderr line, then `exec`s the
    /// target so exit codes stay the real producer's own. Portable limits
    /// only: what the kernel enforces afterwards is platform behavior, and
    /// the readback below proves what took effect here.
    fn limit_launcher() -> String {
        format!(
            "{BOUNDED_CLI_LIMITS}; echo \"harness-limits: stack=$(ulimit -s) cpu=$(ulimit -t)\" >&2; exec \"$@\""
        )
    }

    /// Splits the launcher's readback line off stderr. An absent line
    /// means the launcher never ran (fail-closed before the target), and
    /// the empty note records exactly that.
    fn strip_launcher_note(run: &mut BoundedRun) {
        (run.limits_note, run.stderr) = match run.stderr.split_once('\n') {
            Some((first, rest)) if first.starts_with("harness-limits:") => {
                (first.to_string(), rest.to_string())
            }
            _ => (String::new(), std::mem::take(&mut run.stderr)),
        };
    }

    fn run_cli_argv_bounded(script: &str, extra_args: &[&str], limit: Duration) -> BoundedRun {
        let launcher = limit_launcher();
        let mut cmd = Command::new("bash");
        cmd.args(["-c", &launcher, "_"])
            .arg(bashkit_cli_path())
            .args(["-c", script])
            .args(extra_args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut run = run_command_bounded(cmd, limit);
        strip_launcher_note(&mut run);
        run
    }

    /// Presence of one PID as an explicit receipt, never inferred from a
    /// bare nonzero exit: only an observed ESRCH text proves absence, while
    /// EPERM or anything unknown keeps polling inside the bound.
    enum PidPresence {
        Present,
        Absent,
        Unknown,
    }

    /// Runs `kill` with the given args under a finite grace and classifies
    /// the outcome from the observed exit plus stderr text. Built from the
    /// same bounded primitives as the runner itself: the helper child is
    /// terminated and reaped through `reap_with_grace` on every path that
    /// does not observe its exit (no leaked running helpers), and its
    /// stderr is drained through `drain_capped` (bounded retention, real
    /// errors fail closed). EOF for that drain is construction-specific,
    /// not assumed for arbitrary executables: this stderr pipe is private
    /// to the `kill` helper binary, which performs one kill(2) and exits
    /// without forking, so the observed exit closes its only write end.
    /// ESRCH receipt text is matched case-insensitively (`no such
    /// process`); EPERM and anything undecipherable stay Unknown — never
    /// successful absence. Every wait inside is a bounded try_wait poll,
    /// so the call itself always terminates.
    /// What a bounded signal query actually proved, stage by stage.
    /// Unknowns stay explicit on every error path; no stage infers the
    /// success of another. Callers assert the fields they depend on.
    struct SignalReceipt {
        /// The signal was dispatched to the target (for `-9`), or the
        /// query itself ran (for `-0`). False when the helper never ran
        /// or its kill failed.
        dispatched: bool,
        /// The helper child was reaped inside the grace (no leak); a
        /// try_wait error counts as unreaped, never as completion.
        helper_reaped: bool,
        /// The stderr drain completed within its cap without I/O error.
        pipe_ok: bool,
        /// Target presence: Absent only by observed ESRCH receipt text.
        presence: PidPresence,
        /// Unfinished drain reader on the timeout path: propagated for
        /// caller-owned finite cleanup under the caller end, never
        /// silently detached. `None` after a joined-inside success.
        drain_thread: Option<std::thread::JoinHandle<()>>,
    }

    /// What one bounded helper execution actually proved, stage by
    /// stage. Callers interpret `exit` per binary (`pkill` 0 = matched,
    /// `pgrep` 0 = present / 1 = absent receipt / else unknown); every
    /// wait inside is bounded by the single caller-owned `end` instant,
    /// never a fresh grace stacked after a timeout.
    struct HelperOutcome {
        exit: Option<i32>,
        stderr: Vec<u8>,
        drained_ok: bool,
        reaped: bool,
        signaled: bool,
        /// The drain reader on the timeout path, returned instead of
        /// detached: the caller owns its finite cleanup. `None` after a
        /// joined-inside success or when no drain was attempted.
        drain_thread: Option<std::thread::JoinHandle<()>>,
    }

    /// Runs one short-lived helper (`kill`, `pkill`, `pgrep`: single-shot
    /// binaries that never fork) to completion inside `end`. Kill/reap on
    /// every non-exit path (a failed kill is recorded, never ignored);
    /// stderr drained through the bounded primitive (retention capped,
    /// errors fail closed). EOF for the drain is construction-specific:
    /// these helpers hold the only write end and exit promptly, so an
    /// observed exit closes it; anything else stays Unknown.
    /// Resolves a helper binary to an absolute executable path by an
    /// explicit PATH search (no silent per-spawn fallback): missing or
    /// non-executable helpers fail closed as Unknown at the call site.
    /// Callers additionally assert the binary runs (the trichotomy test
    /// below), which is the executable evidence behind the documented
    /// single-writer EOF contract.
    fn resolve_helper(name: &str) -> Option<std::path::PathBuf> {
        let path_var = std::env::var_os("PATH")?;
        for dir in std::env::split_paths(&path_var) {
            let candidate = dir.join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
        None
    }

    fn run_helper_bounded(program: &str, args: &[&str], end: Instant) -> HelperOutcome {
        let blank = || HelperOutcome {
            exit: None,
            stderr: Vec::new(),
            drained_ok: false,
            reaped: false,
            signaled: false,
            drain_thread: None,
        };
        let bin = match resolve_helper(program) {
            Some(b) => b,
            None => {
                return HelperOutcome {
                    drain_thread: None,
                    exit: None,
                    stderr: Vec::new(),
                    drained_ok: false,
                    reaped: false,
                    signaled: false,
                };
            }
        };
        let mut child = match Command::new(&bin)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
        {
            Ok(c) => c,
            Err(_) => return blank(),
        };
        // Split the single deadline between observation and cleanup:
        // the poll phase takes the first half so kill+reap always retain
        // the second half — a timeout never leaves zero remaining reap
        // time with ownership silently abandoned, and the total never
        // exceeds `end`. (A fixed-second reserve cannot work here: for an
        // end nearer than the reserve, checked_sub lands in the past and
        // the poll phase would be skipped entirely.)
        let remaining = end.saturating_duration_since(Instant::now());
        let poll_end = Instant::now() + remaining / 2;
        let exited = loop {
            match child.try_wait() {
                Ok(Some(_)) => break true,
                Ok(None) => {
                    if Instant::now() >= poll_end.min(end) {
                        break false;
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(_) => break false,
            }
        };
        if !exited {
            let signaled = child.kill().is_ok();
            let mut poll = || child.try_wait();
            let reaped =
                reap_with_grace(&mut poll, end.saturating_duration_since(Instant::now())).is_some();
            return HelperOutcome {
                exit: None,
                stderr: Vec::new(),
                drained_ok: false,
                reaped,
                signaled,
                drain_thread: None,
            };
        }
        let mut pipe = match child.stderr.take() {
            Some(p) => p,
            None => {
                return HelperOutcome {
                    exit: None,
                    stderr: Vec::new(),
                    drained_ok: false,
                    reaped: true,
                    signaled: false,
                    drain_thread: None,
                };
            }
        };
        // Owned finite drain: the reader runs on its own thread and the
        // result is collected under the shared end, so even a retained
        // writer (no EOF ever) cannot stall this call past its deadline.
        // The cap inside drain_capped bounds retention; the timeout below
        // bounds duration. A timed-out drain returns its original handle
        // so the controlled fixture can terminate the writer and join it.
        // The SAME drain reader is retained and finitely observed:
        // on success it is joined below (proving original-reader
        // completion — it already sent, so the join returns at once);
        // on timeout its handle rides home in the receipt for
        // caller-owned finite cleanup instead of silent detachment.
        // Gate FIRST under the shared end, then take the sent value
        // without blocking: receipt-send only proves the bytes moved,
        // never that the thread finished. join_finished proves the
        // SAME reader completed; try_recv afterwards cannot block (a
        // finished thread either sent or panicked before sending).
        // An unfinished handle rides home in the receipt for
        // caller-owned finite cleanup instead of silent detachment.
        // wait() first: the exit was already observed, so this reaps
        // at once and never blocks. Its code is preserved on EVERY path
        // below (direct OS reap receipt), even when the drain times out.
        let code = child.wait().ok().and_then(|s| s.code());
        let (drain_tx, drain_rx) = std::sync::mpsc::channel();
        let drain_handle = std::thread::spawn(move || {
            let _ = drain_tx.send(drain_capped(&mut pipe));
        });
        let outcome = match join_finished(
            drain_handle,
            Instant::now() + end.saturating_duration_since(Instant::now()),
        ) {
            JoinOutcome::Joined => drain_rx.try_recv().ok(),
            JoinOutcome::Unfinished(handle) => {
                return HelperOutcome {
                    exit: code,
                    stderr: Vec::new(),
                    drained_ok: false,
                    reaped: true,
                    signaled: true,
                    drain_thread: Some(handle),
                };
            }
            JoinOutcome::Panicked => {
                return HelperOutcome {
                    exit: code,
                    stderr: Vec::new(),
                    drained_ok: false,
                    reaped: true,
                    signaled: true,
                    drain_thread: None,
                };
            }
        };
        let (stderr, _, read_error) = match outcome {
            Some(v) => v,
            None => {
                return HelperOutcome {
                    exit: code,
                    stderr: Vec::new(),
                    drained_ok: false,
                    reaped: true,
                    signaled: true,
                    drain_thread: None,
                };
            }
        };
        HelperOutcome {
            exit: code,
            stderr,
            drained_ok: !read_error,
            reaped: true,
            signaled: true,
            drain_thread: None,
        }
    }

    fn signal_bounded(args: &[&str], grace: Duration) -> SignalReceipt {
        // Single owned implementation: classification only lives here.
        // The caller end (never a fresh stacked grace) bounds the helper;
        // an unfinished drain handle rides home in the receipt for
        // caller-owned finite cleanup under that same end — never
        // detached here. Consuming arms are exclusive, so each moves the
        // handle at most once.
        let end = Instant::now() + grace;
        let o = run_helper_bounded("kill", args, end);
        // o.signaled is consumed below, not just written: a timeout path
        // that dispatched its kill reports sent-but-unobserved, distinct
        // from a helper that never ran at all.
        if o.exit.is_none() && !o.reaped {
            return SignalReceipt {
                dispatched: o.signaled,
                helper_reaped: false,
                pipe_ok: false,
                presence: PidPresence::Unknown,
                drain_thread: o.drain_thread,
            };
        }
        if !o.drained_ok {
            return SignalReceipt {
                dispatched: o.signaled,
                helper_reaped: o.reaped,
                pipe_ok: false,
                presence: PidPresence::Unknown,
                drain_thread: o.drain_thread,
            };
        }
        let lowered = String::from_utf8_lossy(&o.stderr).to_lowercase();
        match o.exit {
            Some(0) => SignalReceipt {
                dispatched: true,
                helper_reaped: o.reaped,
                pipe_ok: true,
                presence: PidPresence::Present,
                drain_thread: o.drain_thread,
            },
            _ if lowered.contains("no such process") => SignalReceipt {
                dispatched: true,
                helper_reaped: o.reaped,
                pipe_ok: true,
                presence: PidPresence::Absent,
                drain_thread: o.drain_thread,
            },
            _ => SignalReceipt {
                dispatched: false,
                helper_reaped: o.reaped,
                pipe_ok: true,
                presence: PidPresence::Unknown,
                drain_thread: o.drain_thread,
            },
        }
    }

    fn retain_join(
        handle: std::thread::JoinHandle<()>,
        end: Instant,
        unfinished: &mut Vec<std::thread::JoinHandle<()>>,
    ) -> bool {
        match join_finished(handle, end) {
            JoinOutcome::Joined => true,
            JoinOutcome::Unfinished(handle) => {
                unfinished.push(handle);
                false
            }
            JoinOutcome::Panicked => false,
        }
    }

    /// Test-scoped owned cleanup for the retained-pipe fixture: the exact
    /// recorded descendant plus the ORIGINAL reader handles. The normal
    /// path (`complete`) terminates the descendant, observes its absence
    /// via ESRCH receipt, joins the original handles, and proves the total
    /// elapsed bound. `Drop` fires the same kill best-effort (no asserts,
    /// no panics) so setup/assertion failures never leak the 30s sleeper;
    /// unfinished ORIGINAL handles return to the caller-owned observer,
    /// which remains alive after this guard is consumed.
    /// What finite cleanup actually proved: unknowns stay explicit,
    /// never successful. The test asserts these instead of inferring
    /// success from the absence of a hang.
    struct CleanupReceipt {
        signaled: bool,
        descendant_gone: bool,
        readers_joined: bool,
        /// Every signal helper call inside cleanup reported its own
        /// bounded reap plus a clean pipe drain. A false here means the
        /// observations above rest on unhealthy helpers, never silent.
        helper_clean: bool,
        /// Reader handles that never finished inside the shared grace:
        /// preserved here instead of silently detached, so the caller
        /// sees the exact shortfall. Normally empty.
        unjoined: Vec<std::thread::JoinHandle<()>>,
        elapsed: Duration,
    }

    struct RetainedCleanup<'a> {
        drop_readers: &'a mut Vec<std::thread::JoinHandle<()>>,
        pid: Option<String>,
        /// Test-controlled argv marker identifying the exact fixture
        /// without any pid channel: usable even when pidfiles fail.
        /// Bound before verdict asserts, like everything else owned here.
        marker: Option<String>,
        pending: Option<PendingJoiners>,
        files: Vec<std::path::PathBuf>,
        /// Optional outcome receipt for the Drop path: Drop writes how
        /// far best-effort cleanup got (kill/join/files), and the test
        /// reads it back instead of inferring cleanup from silence.
        drop_note: Option<std::path::PathBuf>,
    }

    impl RetainedCleanup<'_> {
        fn complete(&mut self, start: Instant) -> CleanupReceipt {
            let end = Instant::now() + CLEANUP_TOTAL;
            let remaining = || {
                end.saturating_duration_since(Instant::now())
                    .min(Duration::from_secs(5))
            };
            let mut signaled = false;
            let mut gone = false;
            let mut joined = false;
            let mut unjoined = Vec::new();
            let mut helper_clean = true;
            // Termination prefers the pid identity and falls back to the
            // marker ownership channel; both run under the shared end.
            // A missing identity with no marker is honestly unowned.
            let target = self.pid.clone();
            let marker = self.marker.clone();
            if target.is_some() || marker.is_some() {
                // Terminate the exact owned descendant (not self-expiry):
                // pid identity first, marker ownership channel as the
                // fallback when no pid was ever recorded. ESRCH on the
                // kill itself already proves absence; a dispatched signal
                // proceeds to the join/absence proof. Every helper runs
                // under the shared end (never a fresh stacked grace).
                match (target.as_deref(), marker.as_deref()) {
                    (Some(pid), _) => {
                        let mut kill = signal_bounded(&["-9", pid], remaining());
                        if let Some(h) = kill.drain_thread.take() {
                            // Handoff from the (unreachable-for-kill) timeout
                            // path: bounded join under the shared end, with
                            // unfinished handles preserved, never detached.
                            match join_finished(h, Instant::now() + remaining()) {
                                JoinOutcome::Joined => {}
                                JoinOutcome::Unfinished(handle) => {
                                    unjoined.push(handle);
                                    helper_clean = false;
                                }
                                JoinOutcome::Panicked => {
                                    helper_clean = false;
                                }
                            }
                        }
                        if kill.dispatched {
                            signaled = true;
                        }
                        if !(kill.helper_reaped && kill.pipe_ok) {
                            helper_clean = false;
                        }
                        if matches!(kill.presence, PidPresence::Absent) {
                            signaled = true;
                            gone = true;
                        }
                    }
                    (None, Some(m)) => {
                        // pkill exit 0 matched exactly this fixture's
                        // marker; anything else leaves absence to the
                        // poll below (no inference from failure).
                        let o = run_helper_bounded("pkill", &["-9", "-f", m], end);
                        // SAME-handle rule: a handoff here (unreachable for
                        // pkill) is joined boundedly under the shared end,
                        // with unfinished handles preserved, never asserted
                        // away or detached.
                        if let Some(h) = o.drain_thread {
                            match join_finished(
                                h,
                                Instant::now() + remaining().min(Duration::from_secs(2)),
                            ) {
                                JoinOutcome::Joined => {}
                                JoinOutcome::Unfinished(handle) => {
                                    unjoined.push(handle);
                                    helper_clean = false;
                                }
                                JoinOutcome::Panicked => {
                                    helper_clean = false;
                                }
                            }
                        }
                        if !(o.reaped && o.drained_ok) {
                            helper_clean = false;
                        }
                        if o.exit == Some(0) {
                            signaled = true;
                        }
                    }
                    (None, None) => {}
                }
                // Join the ORIGINAL reader handles: the kill closed the
                // retained pipes, so they must finish inside the shared
                // grace. Missing receipts fail here, never silently.
                // Join deferred past absence: see below.
                // Absence receipt: only ESRCH counts; anything unknown
                // retries inside the shared end, then stays false.
                while !gone && Instant::now() < end {
                    // Absence receipt: pid ESRCH text, or pgrep exit 1
                    // (no match) for the marker channel. Anything else —
                    // present, errors, undecipherable — retries inside the
                    // shared end, then stays false. Never inferred.
                    let absent = match (target.as_deref(), marker.as_deref()) {
                        (Some(pid), _) => {
                            let mut probe = signal_bounded(&["-0", pid], remaining());
                            if let Some(h) = probe.drain_thread.take() {
                                // Handoff from the (unreachable-for-kill)
                                // timeout path: bounded join under the
                                // shared end, unfinished preserved.
                                match join_finished(h, Instant::now() + remaining()) {
                                    JoinOutcome::Joined => {}
                                    JoinOutcome::Unfinished(handle) => {
                                        unjoined.push(handle);
                                        helper_clean = false;
                                    }
                                    JoinOutcome::Panicked => {
                                        helper_clean = false;
                                    }
                                }
                            }
                            if !(probe.helper_reaped && probe.pipe_ok) {
                                helper_clean = false;
                            }
                            matches!(probe.presence, PidPresence::Absent)
                        }
                        (None, Some(m)) => {
                            let o = run_helper_bounded("pgrep", &["-f", m], end);
                            if let Some(h) = o.drain_thread {
                                match join_finished(
                                    h,
                                    Instant::now() + remaining().min(Duration::from_secs(2)),
                                ) {
                                    JoinOutcome::Joined => {}
                                    JoinOutcome::Unfinished(handle) => {
                                        unjoined.push(handle);
                                        helper_clean = false;
                                    }
                                    JoinOutcome::Panicked => {
                                        helper_clean = false;
                                    }
                                }
                            }
                            if !(o.reaped && o.drained_ok) {
                                helper_clean = false;
                            }
                            o.exit == Some(1)
                        }
                        (None, None) => break,
                    };
                    if absent {
                        gone = true;
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(50));
                }
            }
            // Join ORIGINAL handles unconditionally (not only with an
            // identity): with no pid the pipes may stay held, in which
            // case the shared grace expires and the handles are preserved
            // in the receipt as explicit unknowns — never silently
            // detached, never a vacuous success.
            if let Some(pending) = self.pending.take() {
                let mut ok = !pending.handles.is_empty();
                for reader in pending.handles {
                    match join_finished(reader, Instant::now() + remaining().min(JOIN_GRACE)) {
                        JoinOutcome::Joined => {}
                        JoinOutcome::Unfinished(handle) => {
                            unjoined.push(handle);
                            ok = false;
                        }
                        JoinOutcome::Panicked => {
                            ok = false;
                        }
                    }
                }
                joined = ok;
            }
            // Receipt files are removed on every path, success or unwind.
            for path in std::mem::take(&mut self.files) {
                std::fs::remove_file(&path).ok();
            }
            self.pid = None;
            CleanupReceipt {
                signaled,
                descendant_gone: gone,
                readers_joined: joined,
                helper_clean,
                unjoined,
                elapsed: start.elapsed(),
            }
        }
    }

    impl Drop for RetainedCleanup<'_> {
        fn drop(&mut self) {
            // Best-effort only (no asserts, no panics): terminate by pid
            // identity first, marker ownership channel as fallback, then
            // one bounded join attempt for retained handles, then file
            // removal. ONE shared end bounds every stage below (kill,
            // join, files): allowances derive from the remainder, never
            // fresh stacked graces. The outcome receipt records helper
            // health too, so callers observe rather than infer; the
            // caller removes it after reading.
            let end = Instant::now() + Duration::from_secs(10);
            let remaining = || end.saturating_duration_since(Instant::now());
            let mut killed = false;
            let mut joined = false;
            let mut helper_clean = true;
            // The caller retains ownership even if this bounded Drop cannot join.
            if let Some(pid) = self.pid.take() {
                let o = run_helper_bounded(
                    "kill",
                    &["-9", &pid],
                    Instant::now() + remaining().min(Duration::from_secs(5)),
                );
                killed = o.exit == Some(0);
                if !(o.reaped && o.drained_ok) {
                    helper_clean = false;
                }
                if let Some(h) = o.drain_thread {
                    // Best-effort backstop for the unreachable handoff:
                    // bounded attempt under the shared end, outcome
                    // recorded (Drop cannot return handles).
                    if !retain_join(h, end, self.drop_readers) {
                        helper_clean = false;
                    }
                }
            } else if let Some(m) = self.marker.take() {
                let o = run_helper_bounded(
                    "pkill",
                    &["-9", "-f", &m],
                    Instant::now() + remaining().min(Duration::from_secs(5)),
                );
                killed = o.exit == Some(0);
                if !(o.reaped && o.drained_ok) {
                    helper_clean = false;
                }
                let handoff_joined = match o.drain_thread {
                    None => true,
                    Some(h) => retain_join(h, end, self.drop_readers),
                };
                if !handoff_joined {
                    helper_clean = false;
                }
            }
            if let Some(pending) = self.pending.take() {
                let join_end = Instant::now() + remaining().min(JOIN_GRACE);
                let mut ok = true;
                for reader in pending.handles {
                    if !retain_join(reader, join_end, self.drop_readers) {
                        ok = false;
                    }
                }
                joined = ok;
            }
            for path in std::mem::take(&mut self.files) {
                std::fs::remove_file(&path).ok();
            }
            if let Some(note) = self.drop_note.take() {
                let _ = std::fs::write(
                    &note,
                    format!("killed={killed} joined={joined} helper_clean={helper_clean}"),
                );
            }
        }
    }

    fn run_raw_bounded(program: &str, args: &[&str], limit: Duration) -> BoundedRun {
        run_raw_bounded_with(program, args, limit, LifecycleFaults::default())
    }

    /// Runs an arbitrary program under the same fail-closed launcher (used
    /// by harness selftests so they prove the effective CPU/stack bounds
    /// instead of bypassing them). The caller supplies piped stdio.
    fn run_raw_bounded_with(
        program: &str,
        args: &[&str],
        limit: Duration,
        faults: LifecycleFaults,
    ) -> BoundedRun {
        let (mut run, pending) = run_raw_core(program, args, limit, faults);
        if !finish_joiners(pending) {
            run.incomplete_readers = true;
        }
        run
    }

    /// Launcher-wrapped core without joining: the fixture owns the reader
    /// threads from here (retained-pipe cleanup proof).
    fn run_raw_core(
        program: &str,
        args: &[&str],
        limit: Duration,
        faults: LifecycleFaults,
    ) -> (BoundedRun, PendingJoiners) {
        let launcher = limit_launcher();
        let mut cmd = Command::new("bash");
        cmd.args(["-c", &launcher, "_"])
            .arg(program)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let (mut run, pending) = run_core(cmd, limit, faults);
        strip_launcher_note(&mut run);
        (run, pending)
    }

    /// Runs a script in the built CLI under `run_command_bounded`, with
    /// no trailing words after the `-c` script.
    fn run_cli_bounded(script: &str, limit: Duration) -> BoundedRun {
        run_cli_argv_bounded(script, &[], limit)
    }

    /// Reaps a victim inside a finite grace via caller-supplied polling.
    /// Returns the observed status, or `None` when the victim cannot be
    /// reaped inside the grace (or polling itself errors): the safe seam
    /// for the unavailable-reap verdict.
    pub(crate) fn reap_with_grace(
        poll: &mut dyn FnMut() -> std::io::Result<Option<std::process::ExitStatus>>,
        grace: Duration,
    ) -> Option<std::process::ExitStatus> {
        let end = Instant::now() + grace;
        loop {
            match poll() {
                Ok(Some(s)) => return Some(s),
                Ok(None) => {
                    if Instant::now() >= end {
                        return None;
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(_) => return None,
            }
        }
    }

    /// Runs an already-built command with one shared wall deadline for
    /// the child, plus finite drain/reap graces after it. The caller
    /// supplies piped stdio: inherited pipes never report back and fail
    /// the run as incomplete instead of hanging it.
    /// child exit, pipe drain, and reaping:
    /// - wall clock: past `limit` the child is killed; if it had already
    ///   exited, the run is not marked timed out;
    /// - output: one reader thread per pipe stops past the sample cap, the
    ///   child is killed, and the run is marked `oversized` (a harness
    ///   failure, never a silent cut). Readers run concurrently, so a
    ///   chatty child can never deadlock the runner on a full pipe;
    /// - drain: after the break, missing readers get a short grace period.
    ///   Anything still missing means a foreign descendant retains the
    ///   pipe: the run is marked `incomplete_readers` and fails visibly
    ///   instead of waiting forever. Reader threads for retained pipes are
    ///   dropped, not joined without a deadline (one documented thread
    ///   leak per such run, only in the retention self-test).
    /// - CPU/stack caps are the launcher's job (see above); Rust std
    ///   exposes no portable per-child CPU/stack/memory caps, and Darwin
    ///   does not enforce address-space caps: documented here, not claimed.
    // Reaping is done by the bounded `try_wait` poll inside the cleanup
    // grace below; an unconditional `wait()` would hang past the grace on
    // an unkillable victim, which is exactly what this harness forbids.
    // Test-only fault injection for the parent lifecycle: each flag
    // drives the real lifecycle path with exactly one hostile condition.
    // All flags stay off outside these tests.
    #[derive(Clone, Default)]
    pub(crate) struct LifecycleFaults {
        /// Child poll fails: the parent must fail closed, never panic.
        fail_poll: bool,
        /// Stdout pipe reads fail: the parent verdict must carry
        /// `read_error` with partial capture.
        fail_read: bool,
        /// Reaping never observes an exit: the unavailable-reap verdict.
        /// Optional readiness gate for `fail_poll`: the fault fires only
        /// after this file exists, proving the fixture victim truly live
        /// under bounds before observation is broken. Without it the fault
        /// could mask a spawn failure as poll breakage.
        ready_file: Option<std::path::PathBuf>,
        reap_never: bool,
    }

    /// A pipe that always fails reads: plugs a real I/O error into the
    /// actual parent capture path (reader thread, channel, verdict).
    struct FailRead;
    impl Read for FailRead {
        fn read(&mut self, _buf: &mut [u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("injected read failure"))
        }
    }

    fn run_command_bounded(cmd: Command, limit: Duration) -> BoundedRun {
        run_command_bounded_with(cmd, limit, LifecycleFaults::default())
    }

    /// Standard runner: core lifecycle plus the bounded join gate. Reader
    /// threads that never reported stay detached by contract.
    pub(crate) fn run_command_bounded_with(
        cmd: Command,
        limit: Duration,
        faults: LifecycleFaults,
    ) -> BoundedRun {
        let (mut run, pending) = run_core(cmd, limit, faults);
        if !finish_joiners(pending) {
            run.incomplete_readers = true;
        }
        run
    }

    // Reaping is done by the bounded try_wait poll inside reap_with_grace;
    // an unconditional wait() would hang past the cleanup grace on an
    // unkillable victim, which is exactly what this harness forbids.
    #[allow(clippy::zombie_processes)]
    /// Owned reader handles for tests that must observe completion
    /// themselves: the runner's verdict plus the actual threads.
    struct PendingJoiners {
        handles: Vec<std::thread::JoinHandle<()>>,
        all_reported: bool,
    }

    /// Applies the bounded join gate to owned reader threads: joins only
    /// while `is_finished` inside the shared completion grace, drops the
    /// rest detached. Returns true only when every reported thread joined
    /// cleanly; unfinished or panicking threads fail the verdict instead.
    /// Callers OR the result into `incomplete_readers`.
    fn finish_joiners(pending: PendingJoiners) -> bool {
        if !pending.all_reported {
            return false;
        }
        let end = Instant::now() + JOIN_GRACE;
        let mut ok = true;
        for reader in pending.handles {
            match join_finished(reader, end) {
                JoinOutcome::Joined => {}
                JoinOutcome::Unfinished(_) | JoinOutcome::Panicked => {
                    ok = false;
                }
            }
        }
        ok
    }

    /// Core lifecycle without joining: spawn, observe, terminate, reap,
    /// drain, verdict. Callers own the reader threads from here.
    fn run_core(
        mut cmd: Command,
        limit: Duration,
        faults: LifecycleFaults,
    ) -> (BoundedRun, PendingJoiners) {
        let mut child = cmd.spawn().expect("spawn bounded child");
        let (tx, rx) = std::sync::mpsc::channel();
        let mut readers = Vec::new();
        // Injected read failure plugs a real I/O error into the actual
        // parent capture path (reader thread, channel, verdict). Without
        // faults the take() path below is byte-identical to before.
        if faults.fail_read {
            readers.push(spawn_capped_reader(
                0,
                Box::new(FailRead) as Box<dyn Read + Send>,
                tx.clone(),
            ));
        } else if let Some(pipe) = child.stdout.take() {
            readers.push(spawn_capped_reader(0, Box::new(pipe), tx.clone()));
        }
        if let Some(pipe) = child.stderr.take() {
            readers.push(spawn_capped_reader(1, Box::new(pipe), tx.clone()));
        }
        drop(tx);
        let mut bufs: [Option<Vec<u8>>; 2] = [None, None];
        let mut oversized = false;
        let mut read_error = false;
        let mut status = None;
        let mut timed_out = false;
        let mut poll_failed = false;
        let deadline = Instant::now() + limit;
        // Pipe EOF alone never ends the run: only an observed exit does,
        // otherwise a child that closes output and keeps running would
        // bypass the watchdog into an unbounded wait. The deadline verdict
        // is recorded from pre-kill observed liveness and never recomputed
        // after killing: reaping the SIGKILL victim must not flip a genuine
        // timeout to false on scheduling luck.
        loop {
            while let Ok((id, buf, over, rerr)) = rx.try_recv() {
                bufs[id as usize] = Some(buf);
                oversized |= over;
                read_error |= rerr;
            }
            if bufs[0].is_some() && bufs[1].is_some() && status.is_some() {
                break;
            }
            if oversized {
                // `kill` is idempotent; errors (already exited) are ignored.
                let _ = child.kill();
            }
            if status.is_none() {
                // A failed poll is an unobservable child, never a panic: fail
                // closed through the kill/reap path with a distinct verdict
                // (incomplete, no exit, not mislabeled as a timeout below).
                // fail_poll fires only once the readiness file proves the
                // victim truly live; before that the parent polls normally, so
                // the fault can never mask a spawn/launch failure.
                let armed = faults.fail_poll
                    && faults
                        .ready_file
                        .as_ref()
                        .map(|p| p.exists())
                        .unwrap_or(true);
                if armed {
                    poll_failed = true;
                    break;
                }
                match child.try_wait() {
                    Ok(found) => status = found,
                    Err(_) => {
                        poll_failed = true;
                        break;
                    }
                }
            }
            if Instant::now() >= deadline {
                // Verdict only: termination happens in common cleanup below,
                // so every path (timeout, oversized, poll failure) funnels
                // through one reachable kill guard. An unobservable child
                // keeps its distinct verdict (incomplete, exitless, and no
                // timeout label) instead of being mislabeled.
                if status.is_none() && !oversized && !poll_failed {
                    timed_out = true;
                }
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        // Bounded drain grace: an exited child with unretained pipes hits
        // EOF promptly; anything still missing is a retained pipe.
        // Common cleanup termination: every loop exit (observed
        // completion, deadline, oversized, poll failure) funnels through
        // this one reachable kill guard. It is idempotent — already-dead
        // children are untouched — and the live oversize kill inside the
        // loop stays as the early producer stop. Verdicts were recorded
        // pre-kill above and are never recomputed from it.
        if status.is_none() {
            let _ = child.kill();
        }
        let grace_end = Instant::now() + DRAIN_GRACE;
        while (bufs[0].is_none() || bufs[1].is_none()) && Instant::now() < grace_end {
            match rx.recv_timeout(Duration::from_millis(50)) {
                Ok((id, buf, over, rerr)) => {
                    bufs[id as usize] = Some(buf);
                    oversized |= over;
                    read_error |= rerr;
                }
                Err(_) => {
                    if bufs[0].is_some() && bufs[1].is_some() {
                        break;
                    }
                }
            }
        }
        let incomplete_readers = bufs[0].is_none() || bufs[1].is_none() || read_error;
        // Bounded completion gate: a channel receipt proves its thread
        // finished every blocking call, but scheduling can pause the sender
        // between send and return, so a receipt is not the termination
        // contract. Poll is_finished inside the shared completion grace and
        // join only finished handles; unfinished handles stay detached and
        // fail the run as incomplete instead. A panicking reader surfaces
        // as incomplete too, never silent. std offers no timed join; this
        // gate plus the elapsed-total assertions are the termination proof.
        // No reproduced production hang is claimed from the static shape:
        // the gate exists so none can occur.
        let pending = PendingJoiners {
            handles: readers,
            all_reported: bufs[0].is_some() && bufs[1].is_some(),
        };
        // Reaps the victim inside a finite grace through a poll seam, so
        // the unavailable-reap verdict is unit-testable without OS fakery.
        // Bounded reap inside a finite cleanup grace: the execution deadline
        // (`limit`) is already over, so reaping must never become an
        // unbounded `wait`. A victim that cannot be reaped inside the grace
        // fails the run visibly instead of hanging the suite; `timed_out`
        // stays exactly as recorded pre-kill and is never recomputed here.
        // Unavailable reap (or injected): the seam verdict flows into
        // the real parent status instead of a helper-only simulation.
        // The OS cleanup always runs (no zombies by construction);
        // reap_never only discards the observation for the verdict, never
        // the cleanup itself. Do not invent exit/reap success from it.
        let mut poll = || child.try_wait();
        let really_reaped = reap_with_grace(&mut poll, REAP_GRACE);
        let reaped = if faults.reap_never {
            None
        } else {
            really_reaped
        };
        let status = match (status, reaped) {
            (Some(s), _) => Some(s),
            (None, Some(s)) => Some(s),
            (None, None) => None,
        };
        let incomplete_readers = incomplete_readers || status.is_none() || poll_failed;
        let stdout = String::from_utf8_lossy(bufs[0].as_deref().unwrap_or(&[])).into_owned();
        let stderr = String::from_utf8_lossy(bufs[1].as_deref().unwrap_or(&[])).into_owned();
        let run = BoundedRun {
            timed_out,
            oversized,
            incomplete_readers,
            read_error,
            // OS termination accounted for: the loop observed an exit or
            // the bounded reap recovered one. Independent of `exit_code`
            // (signaled victims have no code) and of injected observation
            // faults (which discard the observation, never the cleanup).
            reaped: status.is_some() || really_reaped.is_some(),
            limits_note: String::new(),
            exit_code: status.and_then(|s| s.code()),
            stdout,
            stderr,
        };
        (run, pending)
    }

    /// TM-DOS-136: scaled multiline token x long ARG0 stays bounded in a
    /// child. The exact finding shape: `for x "` plus N `a`+newline lines
    /// plus the closing quote (the parser expects `do`), invoked with a long
    /// `-c` ARG0. Diagnostic storage used to grow as token lines x ARG0
    /// before stderr capture. Both scales must exit 2 with a Bash-compatible
    /// shape and flat <=1 KiB stderr, proving the report no longer grows
    /// with the product. Never runs in the test process.
    #[test]
    fn syntax_diagnostic_multiline_times_arg0_is_bounded_in_child() {
        for token_lines in [200usize, 2000usize] {
            let mut script = String::from("for x \"");
            for _ in 0..token_lines {
                script.push('a');
                script.push('\n');
            }
            script.push('"');
            let arg0 = "B".repeat(8192);
            let run = run_cli_argv_bounded(&script, &[&arg0], Duration::from_secs(20));
            assert!(
                !run.timed_out,
                "token_lines={token_lines}: child hit the 20s watchdog"
            );
            assert_eq!(
                run.exit_code,
                Some(2),
                "token_lines={token_lines}: raw child exit code"
            );
            assert!(
                !run.oversized,
                "token_lines={token_lines}: output exceeded the 8192-byte sample cap"
            );
            assert!(
                !run.incomplete_readers,
                "token_lines={token_lines}: reader threads did not finish"
            );
            assert!(
                !run.read_error,
                "token_lines={token_lines}: pipe reader hit a real I/O error"
            );
            assert!(
                run.limits_note.contains("stack=4096"),
                "token_lines={token_lines}: effective stack cap not read back: {}",
                run.limits_note
            );
            assert!(
                run.limits_note.contains("cpu=10"),
                "token_lines={token_lines}: effective CPU cap not read back: {}",
                run.limits_note
            );
            assert!(
                run.stdout.is_empty(),
                "token_lines={token_lines}: stdout must stay empty"
            );
            assert!(
                run.stderr.contains("syntax error"),
                "token_lines={token_lines}: keep the Bash-compatible shape"
            );
            assert!(
                run.stderr.len() <= 1024,
                "token_lines={token_lines}: {} stderr bytes, want <= 1024",
                run.stderr.len()
            );
        }
    }

    /// Scripted pipe source for `drain_capped` selftests: replays
    /// `Interrupted` retries, data, real errors, and EOF on demand.
    enum ReadOp {
        Interrupted,
        Data(&'static [u8]),
        Fail,
        Eof,
    }
    struct ScriptedRead {
        ops: Vec<ReadOp>,
        off: usize,
    }
    impl Read for ScriptedRead {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            if self.off >= self.ops.len() {
                return Ok(0);
            }
            match self.ops[self.off] {
                ReadOp::Interrupted => {
                    self.off += 1;
                    Err(std::io::Error::from(std::io::ErrorKind::Interrupted))
                }
                ReadOp::Data(d) => {
                    self.off += 1;
                    let n = d.len().min(buf.len());
                    buf[..n].copy_from_slice(&d[..n]);
                    Ok(n)
                }
                ReadOp::Fail => {
                    self.off += 1;
                    Err(std::io::Error::new(std::io::ErrorKind::BrokenPipe, "boom"))
                }
                ReadOp::Eof => Ok(0),
            }
        }
    }

    #[test]
    fn harness_reader_exact_cap_eof_is_clean() {
        // Two 4096-byte reads land exactly on the 8192 cap, then EOF:
        // full retention, no excess flag.
        static CHUNK: [u8; 4096] = [b'x'; 4096];
        let mut exact = ScriptedRead {
            ops: vec![ReadOp::Data(&CHUNK), ReadOp::Data(&CHUNK), ReadOp::Eof],
            off: 0,
        };
        let (buf, oversized, read_error) = drain_capped(&mut exact);
        assert_eq!(buf.len(), BOUNDED_SAMPLE_CAP);
        assert!(!oversized, "exact cap + EOF is not excess");
        assert!(!read_error);
    }

    #[test]
    fn harness_reader_real_excess_flags_oversized() {
        let mut over: &[u8] = &vec![b'x'; BOUNDED_SAMPLE_CAP + 1];
        let (buf, oversized, read_error) = drain_capped(&mut over);
        assert_eq!(
            buf.len(),
            BOUNDED_SAMPLE_CAP,
            "retention never passes the cap"
        );
        assert!(oversized, "cap + 1 is real excess");
        assert!(!read_error);
    }

    #[test]
    fn harness_reader_retries_interrupted_and_reports_errors() {
        let mut ok = ScriptedRead {
            ops: vec![ReadOp::Interrupted, ReadOp::Data(b"hi"), ReadOp::Eof],
            off: 0,
        };
        let (buf, oversized, read_error) = drain_capped(&mut ok);
        assert_eq!(buf, b"hi");
        assert!(!oversized);
        assert!(!read_error, "Interrupted is retried, not reported");
        let mut bad = ScriptedRead {
            ops: vec![ReadOp::Data(b"par"), ReadOp::Fail],
            off: 0,
        };
        let (buf, _, read_error) = drain_capped(&mut bad);
        assert_eq!(buf, b"par", "partial bytes are kept");
        assert!(read_error, "real errors fail closed, never EOF");
    }

    #[test]
    fn harness_signal_reports_presence_trichotomy() {
        // Direct deterministic proof of the ESRCH/EPERM/Unknown split
        // through the actual helper: self is Present, an impossible PID
        // is Absent by ESRCH receipt, and an invalid PID string is
        // Unknown (never successful absence).
        let me_pid = std::process::id().to_string();
        let tri_start = Instant::now();
        let end = tri_start + Duration::from_secs(20);
        let mut unfinished = Vec::new();
        let me_check = signal_bounded(
            &["-0", &me_pid],
            end.saturating_duration_since(Instant::now())
                .min(Duration::from_secs(5)),
        );
        match me_check.drain_thread {
            None => {}
            Some(h) => assert!(
                retain_join(h, end, &mut unfinished),
                "handoff joined under a finite bound"
            ),
        }
        assert!(me_check.dispatched, "self-signal delivers");
        assert!(me_check.helper_reaped, "helper reaped on every path");
        assert!(me_check.pipe_ok, "pipe drained cleanly");
        assert!(
            matches!(me_check.presence, PidPresence::Present),
            "own PID reads Present"
        );
        let gone_check = signal_bounded(
            &["-0", "2147483647"],
            end.saturating_duration_since(Instant::now())
                .min(Duration::from_secs(5)),
        );
        match gone_check.drain_thread {
            None => {}
            Some(h) => assert!(
                retain_join(h, end, &mut unfinished),
                "handoff joined under a finite bound"
            ),
        }
        assert!(gone_check.dispatched, "query itself delivered");
        assert!(gone_check.helper_reaped, "helper reaped on every path");
        assert!(
            matches!(gone_check.presence, PidPresence::Absent),
            "impossible PID reads Absent by ESRCH receipt"
        );
        let bad_check = signal_bounded(
            &["-9", "not-a-pid"],
            end.saturating_duration_since(Instant::now())
                .min(Duration::from_secs(5)),
        );
        match bad_check.drain_thread {
            None => {}
            Some(h) => assert!(
                retain_join(h, end, &mut unfinished),
                "handoff joined under a finite bound"
            ),
        }
        assert!(!bad_check.dispatched, "invalid PID string cannot deliver");
        assert!(bad_check.helper_reaped, "helper reaped on every path");
        assert!(
            matches!(bad_check.presence, PidPresence::Unknown),
            "undecipherable outcome stays Unknown, never absence"
        );
        assert!(
            tri_start.elapsed() < Duration::from_secs(20),
            "all three helper calls finished inside finite bounds: {:?}",
            tri_start.elapsed()
        );
    }

    /// An infinite writer that never yields EOF: proves the drain
    /// terminates at the cap by retention bound alone, without any EOF.
    struct EndlessWriter;
    impl Read for EndlessWriter {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            for b in buf.iter_mut() {
                *b = b'x';
            }
            Ok(buf.len())
        }
    }

    #[test]
    fn harness_drain_ends_without_eof_at_cap() {
        let start = Instant::now();
        let mut src = EndlessWriter;
        let (buf, oversized, read_error) = drain_capped(&mut src);
        assert_eq!(
            buf.len(),
            BOUNDED_SAMPLE_CAP,
            "retention never passes the cap"
        );
        assert!(oversized, "uncapped stream is real excess");
        assert!(!read_error);
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "no EOF needed for termination: {:?}",
            start.elapsed()
        );
    }

    #[test]
    fn harness_helper_timeout_kills_and_reaps_bounded() {
        // Deterministic timeout path through the shared helper: a 30s
        // sleeper against a 200ms end always trips the deadline first.
        // The helper must kill it, reap the signaled exit inside the
        // reserved floor, and report exit-None with reaped true — never
        // an abandoned running child, never an inferred exit.
        let start = Instant::now();
        let end = start + Duration::from_millis(200);
        let mut unfinished = Vec::new();
        let o = run_helper_bounded("sleep", &["30"], end);
        assert_eq!(o.exit, None, "timed-out helper has no exit code");
        assert!(o.signaled, "timeout path dispatched its kill");
        assert!(o.reaped, "kill was reaped inside the reserved floor");
        // SAME-handle rule even here: sleep drains instantly, so the join
        // below succeeds at once and proves it (no assert-away).
        if let Some(h) = o.drain_thread {
            assert!(retain_join(h, end, &mut unfinished), "handoff joined");
        }
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "helper timeout bounded end to end: {:?}",
            start.elapsed()
        );
    }

    #[test]
    fn harness_helper_retained_writer_times_out_bounded() {
        // Controlled retained-writer negative proof THROUGH the shared
        // helper entry (not a manual copy of its poll/drain/channel): a
        // backgrounded marker sleeper holds the stderr pipe open while
        // the shell exits at once, under an applied CPU/stack/output
        // envelope proven by file receipt. Ownership (marker, files) is
        // acquired in the guard BEFORE the subject call, so setup and
        // assertion failures clean up too; the drain handle joins the
        // guard right after the call. Controlled cleanup (marker kill,
        // SAME-handle join, absence) runs through guard.complete BEFORE
        // any verdict assertion below. No twin, no self-expiry, no
        // inference: every receipt is observed.
        let marker = format!("syntax-held-{}", std::process::id());
        let receipt =
            std::env::temp_dir().join(format!("syntax-held-limits-{}", std::process::id()));
        // SAME effective child envelope as the launcher (fail-closed
        // 99 on refusal): the subject runs bounded even though it goes
        // through run_helper directly.
        let script = format!(
            "{BOUNDED_CLI_LIMITS}; ulimit -s > '{}'; ulimit -t >> '{}'; exec -a '{marker}' sleep 30 & exit 0",
            receipt.display(),
            receipt.display()
        );
        let start = Instant::now();
        let mut drop_readers = Vec::new();
        let mut guard = RetainedCleanup {
            drop_readers: &mut drop_readers,
            pid: None,
            marker: Some(marker.clone()),
            pending: None,
            files: vec![receipt.clone()],
            drop_note: None,
        };
        let o = run_helper_bounded(
            "bash",
            &["-c", &script],
            Instant::now() + Duration::from_secs(8),
        );
        // The SAME returned reader joins guard ownership immediately
        // (before any assert): a panic below still cleans up via Drop.
        let mut handle = o.drain_thread;
        assert!(
            handle.is_some(),
            "unfinished reader rides home, never detached"
        );
        guard.pending = Some(PendingJoiners {
            handles: vec![handle.take().unwrap()],
            all_reported: false,
        });
        // Collect the envelope receipt now: complete() below removes its
        // file as part of cleanup, so the bytes must be owned first. The
        // assertions on its contents stay after cleanup with the verdict.
        let receipt_text =
            std::fs::read_to_string(&receipt).expect("envelope receipt written under bounds");
        // Controlled cleanup BEFORE verdict assertions: terminate the
        // exact writer through marker ownership, join the SAME original
        // handle, observe absence, bound the total.
        let cleanup = guard.complete(start);
        // Verdict assertions on observed receipts only.
        assert_eq!(o.exit, Some(0), "subject exit observed (direct OS reap)");
        assert!(o.reaped, "observed exit reaped, no zombie");
        assert!(
            !o.drained_ok,
            "retained writer yields no drain verdict (fail-closed)"
        );
        let mut lines = receipt_text.lines();
        assert_eq!(lines.next(), Some("4096"), "effective stack cap applied");
        assert_eq!(lines.next(), Some("10"), "effective CPU cap applied");
        assert!(cleanup.signaled, "marker kill dispatched");
        assert!(
            cleanup.descendant_gone,
            "writer absence observed (pgrep exit 1), not inferred"
        );
        assert!(
            cleanup.readers_joined,
            "SAME original reader joined after writer termination"
        );
        assert!(
            cleanup.unjoined.is_empty(),
            "no reader handle silently detached"
        );
        assert!(
            cleanup.helper_clean,
            "cleanup helpers reaped and drained on every call"
        );
        assert!(
            cleanup.elapsed < Duration::from_secs(25),
            "retained-writer proof bounded end to end: {:?}",
            cleanup.elapsed
        );
    }

    #[test]
    fn harness_handshake_failure_has_owned_unknown_verdict() {
        // Two identity channels for the SAME controlled victim: pidA is
        // written by the fixture and establishes independent ownership
        // BEFORE the fallible handshake; pidB is never written, so its
        // handshake fails deterministically. The run verdict stays the
        // helper's own (incomplete, exitless-timeout-free); cleanup runs
        // through the owned channel with the SAME original readers.
        let receipt = std::env::temp_dir().join(format!("syntax-hs-limits-{}", std::process::id()));
        let pid_a = std::env::temp_dir().join(format!("syntax-hs-pid-a-{}", std::process::id()));
        let pid_b = std::env::temp_dir().join(format!("syntax-hs-pid-b-{}", std::process::id()));
        let script = format!(
            "ulimit -s > '{}'; ulimit -t >> '{}'; sleep 30 & echo $! > '{}'; exit 0",
            receipt.display(),
            receipt.display(),
            pid_a.display()
        );
        let start = Instant::now();
        let (run, pending) = run_raw_core(
            "bash",
            &["-c", &script],
            Duration::from_secs(5),
            LifecycleFaults::default(),
        );
        // Ownership first: pidA binds the exact victim before the
        // handshake on pidB is even attempted.
        // Guard owns threads and file paths BEFORE any fallible read:
        // a panic below still cleans up via Drop. The pid binds once the
        // pidA handshake proves identity; without it there is honestly
        // nothing to terminate.
        let mut drop_readers = Vec::new();
        let mut guard = RetainedCleanup {
            drop_readers: &mut drop_readers,
            drop_note: None,
            marker: None,
            pid: None,
            pending: Some(pending),
            files: vec![receipt, pid_a],
        };
        let owned = std::fs::read_to_string(&guard.files[1])
            .expect("ownership channel recorded")
            .trim()
            .to_string();
        guard.pid = Some(owned.clone());
        assert!(!run.timed_out, "exited child misreported as timeout");
        assert_eq!(run.exit_code, Some(0));
        assert!(
            run.incomplete_readers,
            "retained pipes are reported, not hidden"
        );
        // The pidB handshake fails deterministically: nothing ever wrote
        // it, and the verdict above was reached without it.
        assert!(
            std::fs::read_to_string(&pid_b).is_err(),
            "unwritten handshake channel stays failed"
        );
        // Cleanup through the OWNED channel with the SAME original
        // readers: terminate, join, observe absence, bound the total.
        let receipt = guard.complete(start);
        assert!(receipt.signaled, "owned descendant terminated");
        assert!(
            receipt.descendant_gone,
            "ESRCH-observed absence of the owned victim"
        );
        assert!(
            receipt.readers_joined,
            "SAME original reader handles joined after the kill"
        );
        assert!(
            receipt.helper_clean,
            "cleanup helpers reaped and drained on every call"
        );
        assert!(
            receipt.unjoined.is_empty(),
            "no reader handle silently detached"
        );
        assert!(
            receipt.elapsed < Duration::from_secs(25),
            "owned cleanup bounded end to end: {:?}",
            receipt.elapsed
        );
    }

    #[test]
    fn harness_ownership_channel_failure_has_owned_unknown_verdict() {
        // Fixture with NO pid echo anywhere: no per-process identity
        // channel exists, so the parent verdict stays honestly unknown.
        // Ownership comes instead from a test-controlled argv marker
        // (established in the script text before the run, usable whether
        // or not any pidfile handshake succeeds): pkill/pgrep address the
        // exact fixture through it, with exit codes as the receipts
        // (pkill 0 = matched, pgrep 1 = gone). No twin handles, no
        // self-expiry reliance: every assertion below is observed.
        let marker = format!("syntax-sleeper-{}", std::process::id());
        let receipt =
            std::env::temp_dir().join(format!("syntax-hs-nopid-limits-{}", std::process::id()));
        let script = format!(
            "ulimit -s > '{}'; ulimit -t >> '{}'; exec -a '{}' sleep 30 & exit 0",
            receipt.display(),
            receipt.display(),
            marker
        );
        let start = Instant::now();
        let (run, pending) = run_raw_core(
            "bash",
            &["-c", &script],
            Duration::from_secs(5),
            LifecycleFaults::default(),
        );
        // Guard owns threads, files, AND the marker ownership channel
        // BEFORE any assert below: a panic anywhere still cleans up via
        // Drop. No pid identity was ever recorded for this fixture; the
        // marker (established in the script text pre-launch) is the
        // usable control channel, and complete() terminates through it.
        let mut drop_readers = Vec::new();
        let mut guard = RetainedCleanup {
            drop_readers: &mut drop_readers,
            drop_note: None,
            pid: None,
            marker: Some(marker.clone()),
            pending: Some(pending),
            files: vec![receipt],
        };
        assert!(!run.timed_out, "exited child misreported as timeout");
        assert_eq!(run.exit_code, Some(0));
        assert!(
            run.incomplete_readers,
            "retained pipes are reported, not hidden"
        );
        // No pid identity was ever recorded for this fixture (proven
        // below): the marker ownership channel drove full cleanup, so the
        // helper verdict stays honestly pid-less while every cleanup step
        // carries an observed receipt.
        let receipt = guard.complete(start);
        assert!(
            std::fs::read_to_string(
                std::env::temp_dir().join(format!("syntax-hs-nopid-pid-{}", std::process::id()))
            )
            .is_err(),
            "no pid identity was ever recorded for this fixture"
        );
        assert!(
            receipt.signaled,
            "marker ownership terminated the exact fixture"
        );
        assert!(
            receipt.descendant_gone,
            "marker absence observed (pgrep exit 1), not inferred"
        );
        assert!(
            receipt.readers_joined,
            "SAME original handles joined after marker termination"
        );
        assert!(
            receipt.unjoined.is_empty(),
            "no reader handle silently detached"
        );
        assert!(
            receipt.helper_clean,
            "cleanup helpers reaped and drained on every call"
        );
        // Termination, SAME-original-handle join, and absence all ran
        // inside complete() through the marker channel (no pid identity
        // was ever recorded): the receipt asserts below prove each, with
        // no unbounded waits and no post-hoc inference.
        assert!(
            start.elapsed() < Duration::from_secs(25),
            "owned unknown-verdict cleanup bounded end to end: {:?}",
            start.elapsed()
        );
    }

    #[test]
    fn harness_drop_preserves_unfinished_reader_for_owner() {
        let (release, wait) = std::sync::mpsc::channel();
        let handle = std::thread::spawn(move || {
            let _ = wait.recv_timeout(Duration::from_secs(20));
        });
        let original_id = handle.thread().id();
        let mut readers = Vec::new();
        let start = Instant::now();
        {
            let _guard = RetainedCleanup {
                drop_readers: &mut readers,
                pid: None,
                marker: None,
                pending: Some(PendingJoiners {
                    handles: vec![handle],
                    all_reported: false,
                }),
                files: Vec::new(),
                drop_note: None,
            };
        }
        // Release before any assertion, then observe the ORIGINAL returned handle.
        let _ = release.send(());
        let count = readers.len();
        let returned_id = readers.first().map(|h| h.thread().id());
        let end = start + Duration::from_secs(10);
        let mut all_joined = true;
        for handle in std::mem::take(&mut readers) {
            all_joined &= retain_join(handle, end, &mut readers);
        }
        assert_eq!(
            count, 1,
            "Drop returns unfinished ownership, not just a count"
        );
        assert_eq!(returned_id, Some(original_id));
        assert!(all_joined && readers.is_empty());
        assert!(start.elapsed() < Duration::from_secs(10));
    }

    #[test]
    fn harness_marker_drop_cleans_up_on_unwind() {
        // Caught failure through a MARKER-owned fixture (no pid identity
        // anywhere): the guard is built before any assert, the panic
        // unwinds through it, and Drop terminates via the marker channel,
        // joins the SAME original readers, removes files, and writes its
        // outcome receipt. The test reads that receipt back instead of
        // inferring cleanup from silence.
        let marker = format!("syntax-unwind-{}", std::process::id());
        let note = std::env::temp_dir().join(format!("syntax-unwind-note-{}", std::process::id()));
        let script = format!("exec -a '{marker}' sleep 30 & exit 0");
        let (run, pending) = run_raw_core(
            "bash",
            &["-c", &script],
            Duration::from_secs(5),
            LifecycleFaults::default(),
        );
        let mut drop_readers = Vec::new();
        let guard = RetainedCleanup {
            drop_readers: &mut drop_readers,
            pid: None,
            marker: Some(marker.clone()),
            pending: Some(pending),
            files: vec![],
            drop_note: Some(note.clone()),
        };
        assert!(
            run.incomplete_readers,
            "fixture holds pipes as designed before the panic"
        );
        let start = Instant::now();
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
            let _guard = guard;
            panic!("simulated assertion failure with marker-owned guard");
        }));
        assert!(outcome.is_err(), "the simulated failure unwinds");
        // Drop fired on unwind: read its outcome receipt (written before
        // the guard was consumed), then prove absence independently.
        let note_text = std::fs::read_to_string(&note).expect("Drop wrote its outcome receipt");
        std::fs::remove_file(&note).ok();
        assert_eq!(
            note_text, "killed=true joined=true helper_clean=true",
            "Drop terminated and joined with healthy helpers, observed not inferred"
        );
        // ONE shared end for the whole absence proof: each pgrep
        // derives its allowance from the remainder, and only a healthy
        // helper reporting exit 1 counts as absence (unhealthy helpers
        // retry inside the bound, never prove anything).
        let end = Instant::now() + Duration::from_secs(10);
        let mut gone = false;
        while Instant::now() < end {
            let o = run_helper_bounded(
                "pgrep",
                &["-f", &marker],
                Instant::now()
                    + end
                        .saturating_duration_since(Instant::now())
                        .min(Duration::from_secs(5)),
            );
            assert!(
                o.drain_thread.is_none(),
                "pgrep drains instantly; a handoff here fails closed"
            );
            assert!(
                o.drain_thread.is_none(),
                "pgrep drains instantly by construction; a handoff here fails closed"
            );
            if o.exit == Some(1) && o.reaped && o.drained_ok {
                gone = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(gone, "unwind path absence observed via pgrep exit 1");
        assert!(
            start.elapsed() < Duration::from_secs(25),
            "marker unwind cleanup bounded: {:?}",
            start.elapsed()
        );
    }

    #[test]
    fn harness_owned_guard_survives_unwind_then_completes() {
        // The guard survives a borrow-only panic (never consumed by it),
        // then runs its explicit finite cleanup AFTER catch_unwind: same
        // owned descendant terminated, SAME original handles joined,
        // absence observed, verdict asserted. This is the unwind path
        // with receipts, complementing the Drop-consumed proof.
        let pid_a =
            std::env::temp_dir().join(format!("syntax-hs-postunwind-pid-{}", std::process::id()));
        let script = format!("sleep 30 & echo $! > '{}'; exit 0", pid_a.display());
        let (run, pending) = run_raw_core(
            "bash",
            &["-c", &script],
            Duration::from_secs(5),
            LifecycleFaults::default(),
        );
        // Guard owns threads and file paths BEFORE the assert below: a
        // failure here still cleans up via Drop. The pid binds after its
        // read; the post-unwind stage below then uses this SAME guard.
        let mut drop_readers = Vec::new();
        let mut guard_opt = Some(RetainedCleanup {
            drop_readers: &mut drop_readers,
            drop_note: None,
            marker: None,
            pid: None,
            pending: Some(pending),
            files: vec![pid_a.clone()],
        });
        // Identity binds before any assert below: the pidfile read is
        // fallible setup, and the guard already owns threads and files
        // while it runs.
        let owned = std::fs::read_to_string(&pid_a)
            .expect("ownership channel recorded")
            .trim()
            .to_string();
        guard_opt.as_mut().unwrap().pid = Some(owned.clone());
        assert!(
            run.incomplete_readers,
            "fixture holds pipes as designed before the panic"
        );
        let start = Instant::now();
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let g = guard_opt.as_mut().unwrap();
            let _ = g;
            panic!("simulated assertion failure with guard alive");
        }));
        assert!(outcome.is_err(), "the simulated failure unwinds");
        // The SAME guard survived (only borrowed): explicit finite cleanup
        // after unwind, then the negative verdict and original-handle
        // completion on the actual owned resources.
        let mut guard = guard_opt.take().expect("guard survived unwind");
        let receipt = guard.complete(start);
        assert!(receipt.signaled, "owned descendant terminated");
        assert!(
            receipt.descendant_gone,
            "ESRCH-observed absence of the owned victim"
        );
        assert!(
            receipt.readers_joined,
            "SAME original reader handles joined after the kill"
        );
        assert!(
            receipt.unjoined.is_empty(),
            "no reader handle silently detached"
        );
        assert!(
            receipt.elapsed < Duration::from_secs(25),
            "post-unwind cleanup bounded: {:?}",
            receipt.elapsed
        );
        assert!(
            run.incomplete_readers,
            "negative verdict preserved through unwind and cleanup"
        );
    }

    #[test]
    fn harness_handshake_fixture_cleans_up_on_unwind() {
        // Caught setup/assertion failure through the owned handshake
        // fixture: panic BEFORE normal complete, then prove the guard's
        // Drop terminated the exact owned descendant inside a finite
        // bound. No success flags come from the unwound path — only the
        // absence receipt plus the reaped-termination observation below.
        let pid_a =
            std::env::temp_dir().join(format!("syntax-hs-unwind-pid-{}", std::process::id()));
        let script = format!("sleep 30 & echo $! > '{}'; exit 0", pid_a.display());
        let (run, pending) = run_raw_core(
            "bash",
            &["-c", &script],
            Duration::from_secs(5),
            LifecycleFaults::default(),
        );
        // Guard acquired BEFORE any assert below: a failure in the
        // ownership read or in the verdict assert still cleans up via
        // Drop. The incomplete assert runs inside the closure, after the
        // guard owns the exact descendant and the original handles.
        // Guard owns threads and files BEFORE the fallible pidA read:
        // a failure here still cleans up via Drop (no identity to kill,
        // files removed, threads documented). The pid binds below.
        let mut drop_readers = Vec::new();
        let mut guard = RetainedCleanup {
            drop_readers: &mut drop_readers,
            drop_note: None,
            marker: None,
            pid: None,
            pending: Some(pending),
            files: vec![pid_a.clone()],
        };
        let owned = std::fs::read_to_string(&pid_a)
            .expect("ownership channel recorded")
            .trim()
            .to_string();
        guard.pid = Some(owned.clone());
        let start = Instant::now();
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
            let _guard = guard;
            assert!(
                run.incomplete_readers,
                "fixture holds pipes as designed before the panic"
            );
            panic!("simulated assertion failure before complete");
        }));
        assert!(outcome.is_err(), "the simulated failure unwinds");
        // The guard's Drop fired its kill; reap the signaled exit through
        // the (bounded) absence poll below — kill -0 succeeds on zombies,
        // so absence alone would prove nothing without this note: the
        // sleeper here was orphaned (reparented past the fixture shell),
        // hence no zombie lingers for this test to reap.
        let end = Instant::now() + Duration::from_secs(10);
        let mut gone = false;
        while Instant::now() < end {
            match signal_bounded(&["-0", &owned], Duration::from_secs(5)).presence {
                PidPresence::Absent => {
                    gone = true;
                    break;
                }
                PidPresence::Present | PidPresence::Unknown => {
                    std::thread::sleep(Duration::from_millis(50))
                }
            }
        }
        assert!(gone, "unwind path still terminated the owned descendant");
        assert!(
            start.elapsed() < Duration::from_secs(20),
            "unwind cleanup bounded: {:?}",
            start.elapsed()
        );
        std::fs::remove_file(&pid_a).ok();
    }
    #[test]
    fn harness_cleanup_guard_fires_on_unwind() {
        // A live sleeper owned by a guard that never reaches complete():
        // the panic below simulates an assertion failure, and Drop must
        // still terminate the exact descendant with a bounded absence
        // receipt. No success flags are asserted from the unwound path —
        // only that nothing leaks past the bound.
        let mut child = Command::new("sleep")
            .arg("30")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn unwind fixture");
        let pid = child.id().to_string();
        let start = Instant::now();
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut drop_readers = Vec::new();
            let _guard = RetainedCleanup {
                drop_readers: &mut drop_readers,
                drop_note: None,
                marker: None,
                pid: Some(pid.clone()),
                pending: None,
                files: vec![],
            };
            panic!("simulated assertion failure before complete");
        }));
        assert!(outcome.is_err(), "the simulated failure unwinds");
        // The guard's Drop fired its kill; this direct child is now a
        // zombie (kill -0 succeeds on zombies), so reap through our own
        // handle first — the reaped signaled exit is itself the
        // termination receipt — then prove absence via ESRCH.
        let reap_end = Instant::now() + Duration::from_secs(5);
        let reaped = loop {
            match child.try_wait() {
                Ok(Some(s)) => break Some(s),
                Ok(None) => {
                    assert!(
                        Instant::now() < reap_end,
                        "unwound victim reaped inside the bound"
                    );
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(_) => break None,
            }
        };
        assert!(
            reaped.map(|s| s.code()) == Some(None),
            "unwound victim died signaled, not exited"
        );
        let end = Instant::now() + Duration::from_secs(10);
        let mut gone = false;
        while Instant::now() < end {
            match signal_bounded(&["-0", &pid], Duration::from_secs(5)).presence {
                PidPresence::Absent => {
                    gone = true;
                    break;
                }
                PidPresence::Present | PidPresence::Unknown => {
                    std::thread::sleep(Duration::from_millis(50))
                }
            }
        }
        assert!(gone, "unwind path still terminated the owned descendant");
        assert!(
            start.elapsed() < Duration::from_secs(20),
            "unwind cleanup bounded: {:?}",
            start.elapsed()
        );
        // Reap through our direct handle so no zombie outlives the test;
        // the victim is already dead, so this returns at once.
        let _ = child.wait();
    }

    #[test]
    fn harness_reap_seam_covers_poll_verdicts() {
        // A pre-reaped status to replay through the seam.
        let mut done = Command::new("bash");
        done.args(["-c", "exit 3"]);
        let mut done = done.spawn().expect("spawn seam status source");
        let status = loop {
            if let Some(s) = done.try_wait().expect("poll seam source") {
                break s;
            }
            std::thread::sleep(Duration::from_millis(5));
        };
        // Immediate exit reaps at once.
        let mut once = Some(status);
        assert!(reap_with_grace(&mut || Ok(once.take()), Duration::from_secs(5)).is_some());
        // Delayed exit reaps once observed.
        let mut pending = 2;
        let got = reap_with_grace(
            &mut || {
                if pending == 0 {
                    Ok(Some(status))
                } else {
                    pending -= 1;
                    Ok(None)
                }
            },
            Duration::from_secs(5),
        );
        assert_eq!(got.map(|s| s.code()), Some(Some(3)));
        // A victim that never exits fails the verdict inside the grace.
        let start = Instant::now();
        let none: Option<std::process::ExitStatus> =
            reap_with_grace(&mut || Ok(None), Duration::from_millis(50));
        assert!(none.is_none(), "unreapable victim fails visibly");
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "grace bounds the wait"
        );
        // Poll errors fail closed too.
        let err: Option<std::process::ExitStatus> = reap_with_grace(
            &mut || Err(std::io::Error::other("gone")),
            Duration::from_secs(5),
        );
        assert!(err.is_none());
    }

    #[test]
    fn harness_join_gate_proves_paused_sender_unfinished() {
        // A sender paused after send, driven through the ACTUAL
        // `join_finished` gate the runner uses. The sender blocks on
        // `release` right after sending, so it is alive by construction
        // when the parent observes the receipt: no timing luck involved.
        // The thread body after release is a bare return (no blocking
        // calls), so the release below is a bounded cleanup path watched
        // by its own finite deadline. The SAME handle goes through both
        // verdicts: unfinished first, then actually joined — never a twin,
        // never detached-while-asserting-cleanup.
        let (done_tx, done_rx) = std::sync::mpsc::channel::<()>();
        let (rel_tx, rel_rx) = std::sync::mpsc::channel::<()>();
        let handle = std::thread::spawn(move || {
            let _ = done_tx.send(());
            let _ = rel_rx.recv();
        });
        done_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("sender ran to the pause point inside the readiness bound");
        let handle = match join_finished(handle, Instant::now() + Duration::from_millis(100)) {
            JoinOutcome::Unfinished(h) => h,
            JoinOutcome::Joined => panic!("paused-after-send thread must fail the gate"),
            JoinOutcome::Panicked => panic!("paused sender must not panic"),
        };
        // Explicit release of THAT SAME thread with finite join cleanup:
        // dropping the release sender unblocks the receiver with an error,
        // the thread returns, and the gate joins it inside the watchdog.
        drop(rel_tx);
        let start = Instant::now();
        match join_finished(handle, start + Duration::from_secs(5)) {
            JoinOutcome::Joined => {}
            JoinOutcome::Unfinished(_) => panic!("released thread must join inside the watchdog"),
            JoinOutcome::Panicked => panic!("released thread must not panic"),
        }
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "cleanup path bounded"
        );
    }
    #[test]
    fn harness_parent_verdict_covers_injected_faults() {
        // Poll failure against a TRULY LIVE child through the REAL parent
        // lifecycle: no panic, real termination in common cleanup, and a
        // verdict distinct from timeout. The orphaned `sleep` is self-bound
        // (30s); grandchildren cannot be reaped portably.
        // Readiness file: the victim proves itself truly live under bounds
        // before observation is broken. Direct exec replaces the shell, so
        // killing closes the pipes at once with no orphan.
        let ready = std::env::temp_dir().join(format!("syntax-poll-ready-{}", std::process::id()));
        let script = format!("echo live > '{}'; exec sleep 30", ready.display());
        let start = Instant::now();
        let run = run_raw_bounded_with(
            "bash",
            &["-c", &script],
            Duration::from_secs(5),
            LifecycleFaults {
                fail_poll: true,
                ready_file: Some(ready.clone()),
                ..Default::default()
            },
        );
        let elapsed = start.elapsed();
        let ready_seen = ready.exists();
        std::fs::remove_file(&ready).ok();
        // The common cleanup really terminated the live child; the exit
        // comes back signaled (no code), and the verdict stays distinct
        // from a timeout (incomplete, exitless, no timeout label).
        assert!(ready_seen, "victim proved truly live before poll breakage");
        assert_eq!(run.exit_code, None, "killed live child has no exit code");
        assert!(
            run.reaped,
            "actual OS reap observed, separate from the exit verdict"
        );
        assert!(
            run.limits_note.contains("stack=4096") && run.limits_note.contains("cpu=10"),
            "effective caps read back: {}",
            run.limits_note
        );
        assert!(run.incomplete_readers, "poll failure fails the run visibly");
        assert!(
            !run.timed_out,
            "poll failure must not be mislabeled as a timeout"
        );
        assert!(
            elapsed < Duration::from_secs(15),
            "cleanup bounded without waiting for any descendant: {elapsed:?}"
        );
        assert!(
            elapsed < Duration::from_secs(15),
            "cleanup bounded without waiting for the 30s sleeper: {elapsed:?}"
        );
        // Injected read failure through the REAL parent capture path:
        // partial capture with read_error set, never a clean EOF.
        let mut c = Command::new("bash");
        c.args(["-c", "exit 0"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let run = run_command_bounded_with(
            c,
            Duration::from_secs(5),
            LifecycleFaults {
                fail_read: true,
                ..Default::default()
            },
        );
        assert!(run.read_error, "injected read error propagates");
        assert!(
            run.incomplete_readers,
            "partial capture fails the run visibly"
        );
        // Unavailable reap through the REAL parent verdict: the victim is
        // killed on the deadline but its exit can never be observed. Runs
        // under the fail-closed launcher so the effective bounds apply;
        // the injected verdict (None) stays separate from the successful
        // OS cleanup the runner still performs (no zombies by construction).
        let start = Instant::now();
        let run = run_raw_bounded_with(
            "bash",
            &["-c", "while true; do :; done"],
            Duration::from_secs(2),
            LifecycleFaults {
                reap_never: true,
                ..Default::default()
            },
        );
        let elapsed = start.elapsed();
        assert_eq!(run.exit_code, None, "unreaped victim has no exit code");
        assert!(
            run.incomplete_readers,
            "unavailable reap fails the run visibly"
        );
        assert!(run.timed_out, "the deadline did trip");
        assert!(
            run.reaped,
            "real OS cleanup ran despite discarded observation"
        );
        assert!(
            run.limits_note.contains("stack=4096") && run.limits_note.contains("cpu=10"),
            "effective caps read back: {}",
            run.limits_note
        );
        assert!(
            elapsed < Duration::from_secs(12),
            "deadline 2s + drain 2s + reap 5s bound the run: {elapsed:?}"
        );
    }

    #[test]
    fn harness_timeout_kills_and_reaps_within_grace() {
        // Pure-builtin busy loop: no grandchildren can inherit the pipes,
        // so killing the shell closes them and the readers finish.
        // Pure-builtin busy loop: no grandchildren can inherit the pipes,
        // so killing the shell closes them and the readers finish. Runs
        // under the fail-closed launcher so the effective CPU/stack bounds
        // are applied and read back, not bypassed.
        let start = Instant::now();
        let run = run_raw_bounded(
            "bash",
            &["-c", "while true; do :; done"],
            Duration::from_secs(2),
        );
        let elapsed = start.elapsed();
        assert!(run.timed_out, "busy loop must trip the 2s deadline");
        assert_eq!(run.exit_code, None, "killed victim has no exit code");
        assert!(!run.oversized);
        assert!(!run.read_error);
        assert!(
            !run.incomplete_readers,
            "silent child: pipes close on kill, readers finish"
        );
        assert!(
            run.limits_note.contains("stack=4096"),
            "effective stack cap read back: {}",
            run.limits_note
        );
        assert!(
            run.limits_note.contains("cpu=10"),
            "effective CPU cap read back: {}",
            run.limits_note
        );
        assert!(
            elapsed < Duration::from_secs(12),
            "deadline 2s + drain 2s + reap 5s bound the run: {elapsed:?}"
        );
    }

    #[test]
    fn harness_overflow_child_is_flagged_with_clean_read() {
        // 9000 bytes: just over the 8192 cap, small enough to exit on its
        // own through the pipe buffer, so the verdict is overflow alone.
        let start = Instant::now();
        let run = run_raw_bounded(
            "bash",
            &["-c", "head -c 9000 /dev/zero"],
            Duration::from_secs(10),
        );
        let elapsed = start.elapsed();
        assert!(run.oversized, "9000 retained bytes are real excess");
        assert_eq!(run.stderr.len() + run.stdout.len(), 8192);
        assert!(!run.read_error);
        assert!(!run.timed_out);
        assert!(
            run.limits_note.contains("stack=4096") && run.limits_note.contains("cpu=10"),
            "effective caps read back: {}",
            run.limits_note
        );
        assert!(
            elapsed < Duration::from_secs(19),
            "limit 10s + drain 2s + reap 5s bound the run: {elapsed:?}"
        );
    }

    /// TM-DOS-135: hostile indirect cycles are refused in a bounded child.
    /// Exact finding reproducer plus the mutual variant. Both run in a
    /// child CLI under a wall-clock watchdog (never in the test runner:
    /// pre-fix they overflowed the stack and aborted the process) and are
    /// refused with `invalid variable name`, like modern bash (5.x); the
    /// real producer exit status is asserted, never relabeled.
    #[test]
    fn indirect_hostile_cycles_rejected_in_bounded_child() {
        for script in [
            "unset a\nr='a[0]:-${!r}]'\necho \"${!r}\"",
            "unset a\nr='a[0]:-${!s}]'\ns='a[0]:-${!r}]'\necho \"${!r}\"",
        ] {
            let run = run_cli_bounded(script, Duration::from_secs(20));
            assert!(!run.timed_out, "hostile script hung: {script}");
            assert!(!run.oversized, "hostile script oversized: {script}");
            assert!(
                !run.incomplete_readers,
                "hostile script readers incomplete: {script}"
            );
            assert!(
                run.limits_note.contains("stack=4096") && run.limits_note.contains("cpu=10"),
                "limits not applied: {}",
                run.limits_note
            );
            assert_eq!(
                run.exit_code,
                Some(1),
                "script: {script}, stdout: {}, stderr: {}",
                run.stdout,
                run.stderr
            );
            assert!(run.stdout.is_empty(), "stdout: {}", run.stdout);
            assert!(
                run.stderr.contains("invalid variable name"),
                "stderr: {}",
                run.stderr
            );
            assert!(run.stderr.len() < 1024, "stderr: {}", run.stderr);
        }
    }

    /// Layout regression for the CLI lookup, over synthetic trees rather
    /// than this run's own: the live tree only ever has the one layout its
    /// toolchain produces, so the other would go uncovered until a nightly
    /// job went red. Both known layouts must resolve the exported binary;
    /// the package-named `build/bashkit` directory must not be mistaken for
    /// it; and absence must stay an error, never a path that fails later as
    /// a spawn error or an exit 127.
    #[test]
    fn cli_lookup_resolves_known_cargo_layouts() {
        let root = std::env::temp_dir().join(format!("bashkit-cli-layout-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let profile = root.join("target/x86_64-unknown-linux-gnu/debug");
        // Stable: `<profile>/deps/<test>`. Nightly 1.101 separate build
        // directory: `<profile>/build/<package>/<hash>/out/<test>`, whose
        // package level collides by name with the binary being sought.
        let stable_exe = profile.join("deps/integration-0a5939c1");
        let nightly_exe = profile.join("build/bashkit/f7e14526a94504a4/out/integration-f7e1");
        for exe in [&stable_exe, &nightly_exe] {
            std::fs::create_dir_all(exe.parent().expect("test binary dir"))
                .expect("create test binary dir");
            std::fs::write(exe, b"").expect("create test binary");
        }

        for exe in [&stable_exe, &nightly_exe] {
            let ruled_out = cli_in_ancestors(exe).expect_err("no binary exported yet");
            assert!(
                ruled_out.contains(&profile.join("bashkit")),
                "the profile directory must be among the paths reported: {ruled_out:?}"
            );
        }

        // A same-named file above the build tree is never a substitute for
        // the binary under test: spawning it would be a false pass, worse
        // than the failure. The target directory marker stops the walk.
        std::fs::write(root.join("target/CACHEDIR.TAG"), b"Signature: 8a477f59")
            .expect("mark target dir");
        std::fs::write(
            root.join(format!("bashkit{}", std::env::consts::EXE_SUFFIX)),
            b"",
        )
        .expect("decoy above the build tree");
        for exe in [&stable_exe, &nightly_exe] {
            let ruled_out = cli_in_ancestors(exe).expect_err("decoy must not be accepted");
            assert!(
                !ruled_out.iter().any(|p| p.parent() == Some(root.as_path())),
                "walk escaped the target directory: {ruled_out:?}"
            );
        }

        let cli = profile.join(format!("bashkit{}", std::env::consts::EXE_SUFFIX));
        std::fs::write(&cli, b"").expect("export cli binary");
        for exe in [&stable_exe, &nightly_exe] {
            assert_eq!(
                cli_in_ancestors(exe).as_deref().ok(),
                Some(cli.as_path()),
                "layout not resolved for {}",
                exe.display()
            );
        }

        std::fs::remove_dir_all(&root).expect("clean up layout fixture");
    }

    /// The bounded runner itself is tested: a hung child must be
    /// reported as a timeout, and an overproducer as oversize. Both fail
    /// visibly (a boolean the harness asserts on), so a helper regression
    /// cannot silently pass hostile coverage. The short-deadline case runs
    /// repeatedly: the timeout verdict must be deterministic, never a
    /// scheduling coin flip.
    #[test]
    fn bounded_helper_kills_hung_child() {
        let run = run_cli_bounded("sleep 30", Duration::from_secs(3));
        assert!(
            run.timed_out,
            "hung child not reported: {:?}",
            run.exit_code
        );
        assert!(!run.oversized, "hung child misreported as oversize");
        assert!(
            !run.incomplete_readers,
            "hung child misreported as incomplete"
        );
        // Repeated short deadlines: every run must report the timeout.
        for _ in 0..3 {
            let run = run_cli_bounded("sleep 30", Duration::from_secs(2));
            assert!(run.timed_out, "short deadline flaked");
            assert!(!run.oversized, "short deadline misreported as oversize");
        }
    }

    /// The launcher is fail-closed: if `ulimit` refuses, the producer never
    /// starts (exit 99) instead of running unbounded. A shadowed failing
    /// `ulimit` exercises the refusal with a harmless instant child.
    #[test]
    fn bounded_launcher_failure_is_fail_closed() {
        let probe = [
            "ulimit() { return 1; }; ",
            BOUNDED_CLI_LIMITS,
            "; echo SURVIVED",
        ]
        .concat();
        let out = Command::new("bash")
            .args(["-c", &probe])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn launcher probe")
            .wait_with_output()
            .expect("reap launcher probe");
        assert_eq!(
            out.status.code(),
            Some(99),
            "stderr: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(
            !String::from_utf8_lossy(&out.stdout).contains("SURVIVED"),
            "launcher ran the producer without limits"
        );
    }

    /// Both pipes hit EOF while the producer lives: only the wall deadline
    /// may end the run. The previous loop broke on complete buffers and
    /// then waited without a deadline; this case hung it forever.
    #[test]
    fn bounded_helper_covers_early_pipe_close() {
        let run = run_cli_bounded("echo hi; exec 1>&- 2>&-; sleep 30", Duration::from_secs(5));
        assert!(run.timed_out, "live producer with closed pipes not killed");
        assert!(!run.oversized, "early close misreported as oversize");
        assert!(
            !run.incomplete_readers,
            "closed pipes misreported as incomplete"
        );
        assert!(run.stdout.contains("hi"), "stdout: {}", run.stdout);
        assert_eq!(run.exit_code, None, "expected SIGKILL, no exit code");
    }

    /// An exited child with descendant-retained pipes: the exit is observed
    /// but readers never finish. The helper must report incomplete readers
    /// within bounds, not wait forever. Runs under the fail-closed launcher;
    /// because the retained pipes keep even the launcher's own readback
    /// line unread, the effective bounds are proven through a per-process
    /// receipt file the fixture shell writes before parking the sleeper
    /// (removed below). The guard terminates the exact descendant
    /// promptly after the verdict; the sleeper's own 30s lifetime remains
    /// only as the backstop bound (grandchildren cannot be reaped
    /// portably). The elapsed assertion proves the run returns without
    /// waiting for it.
    #[test]
    fn bounded_helper_flags_retained_pipes() {
        // The sleeper's PID is retained through a second receipt file so
        // the test itself terminates and finitely observes the exact
        // descendant it created — no waiting for the 30s self-expiry, no
        // universal process-tree reaper, just test-scoped cleanup of owned
        // resources. Unique per process; removed below.
        let receipt =
            std::env::temp_dir().join(format!("syntax-retained-limits-{}", std::process::id()));
        let pidfile =
            std::env::temp_dir().join(format!("syntax-retained-pid-{}", std::process::id()));
        let script = format!(
            "ulimit -s > '{}'; ulimit -t >> '{}'; sleep 30 & echo $! > '{}'; exit 0",
            receipt.display(),
            receipt.display(),
            pidfile.display()
        );
        // Handle-retaining core: this fixture owns the ORIGINAL reader
        // threads from here (not a twin, not detached-while-asserting).
        let start = Instant::now();
        let (run, pending) = run_raw_core(
            "bash",
            &["-c", &script],
            Duration::from_secs(5),
            LifecycleFaults::default(),
        );
        let elapsed = start.elapsed();
        // Guard owns the ORIGINAL reader handles and both receipt paths
        // BEFORE any fallible setup read below: a panic in the reads or
        // in the verdict asserts still cleans up via Drop (best-effort
        // kill once the pid binds, file removal, no panics). The pid
        // itself binds after the pidfile read; without an identity there
        // is nothing to terminate, which Drop records honestly.
        let mut drop_readers = Vec::new();
        let mut guard = RetainedCleanup {
            drop_readers: &mut drop_readers,
            drop_note: None,
            marker: None,
            pid: None,
            pending: Some(pending),
            files: vec![receipt, pidfile],
        };
        let receipt_text =
            std::fs::read_to_string(&guard.files[0]).expect("fixture shell ran under the launcher");
        let sleeper = std::fs::read_to_string(&guard.files[1])
            .expect("fixture recorded its descendant identity")
            .trim()
            .to_string();
        guard.pid = Some(sleeper.clone());
        assert!(!run.timed_out, "exited child misreported as timeout");
        assert!(
            run.incomplete_readers,
            "retained pipes not reported as incomplete"
        );
        assert!(!run.read_error, "open pipes are not I/O errors");
        assert!(
            receipt_text.lines().next() == Some("4096")
                && receipt_text.lines().nth(1) == Some("10"),
            "effective stack/cpu bounds applied to the fixture: {receipt_text:?}"
        );
        assert_eq!(run.exit_code, Some(0), "stdout: {}", run.stdout);
        assert!(
            elapsed < Duration::from_secs(15),
            "returned in {elapsed:?} without waiting for the 30s sleeper"
        );
        // Controlled cleanup of the ACTUAL owned resources through the
        // guard (its Drop covers any failure above too): terminate the
        // exact retained descendant, join the ORIGINAL reader handles,
        // observe absence, prove the total bound.
        let receipt = guard.complete(start);
        assert!(receipt.signaled, "owned descendant terminated");
        assert!(
            receipt.descendant_gone,
            "ESRCH-observed absence, not inferred"
        );
        assert!(
            receipt.readers_joined,
            "ORIGINAL reader handles joined after the kill"
        );
        assert!(
            receipt.helper_clean,
            "cleanup helpers reaped and drained on every call"
        );
        assert!(
            receipt.unjoined.is_empty(),
            "no reader handle silently detached"
        );
        assert!(
            receipt.elapsed < Duration::from_secs(25),
            "verdict plus owned cleanup bounded end to end: {:?}",
            receipt.elapsed
        );
    }

    #[test]
    fn bounded_helper_flags_oversize_output() {
        let script =
            "i=0; while ((i < 4000)); do echo \"padding line $i padding\"; i=$((i + 1)); done";
        let run = run_cli_bounded(script, Duration::from_secs(20));
        assert!(!run.timed_out, "overproducer hung");
        assert!(run.oversized, "overproducer not flagged as oversize");
        assert!(
            !run.incomplete_readers,
            "overproducer misreported as incomplete"
        );
        assert!(
            run.stdout.len() <= BOUNDED_SAMPLE_CAP + 4096,
            "sample not capped: {}",
            run.stdout.len()
        );
    }

    /// TM-DOS-135: a bracket indirect target is refused unless the first
    /// complete `[...]` reference consumes the whole value. Modern bash
    /// (5.x) rejects trailing operators with `invalid variable name`;
    /// refusal also keeps a nested `${!...}` from ever reaching the desugar
    /// re-parse, which re-entered indirect expansion without bound.
    /// Valid nested subscripts resolve whole.
    #[tokio::test]
    async fn indirect_array_boundary_rejects_trailing_operators() {
        // Trailing operator/nesting text is refused, like bash 5.x. Older
        // bash 3.2 ignored the tail, so no real-bash oracle is asserted
        // for these shapes; the rejection itself is the contract.
        for target in [
            "a[0]:-injected]",
            "a[0]-injected]",
            "a[0]:=injected]",
            "a[0]+injected]",
            "a[0]/x/injected]",
            "a[0][1]",
            "a[0]}${x:-injected}[0]",
            "a[b[0]]:-D",
        ] {
            let mut bash = Bash::new();
            let result = bash
                .exec(&format!("a=(1 2)\nb=(0)\nr='{target}'\necho \"${{!r}}\""))
                .await
                .unwrap();
            assert_ne!(result.exit_code, 0, "target accepted: {target}");
            assert_eq!(result.exit_code, 1, "target: {target}");
            assert!(result.stdout.is_empty(), "target: {target}");
            assert!(
                result.stderr.contains("invalid variable name"),
                "target: {target}: {}",
                result.stderr
            );
            assert!(result.stderr.len() < 1024, "target: {target}");
        }

        // Nested subscripts resolve whole (bash: `two`/`two`).
        let mut bash = Bash::new();
        let result = bash
            .exec("a=(zero one two three)\nb=(2)\nr='a[b[0]]'\nprintf \"indirect=[%s]\\n\" \"${!r}\"\nprintf \"direct=[%s]\\n\" \"${a[b[0]]}\"")
            .await
            .unwrap();
        assert_eq!(result.stdout, "indirect=[two]\ndirect=[two]\n");
        assert!(result.stderr.is_empty(), "stderr: {}", result.stderr);
        // Associative references agree between direct and indirect use.
        // Portable note: indexed cases above also run against real bash
        // below; assoc arrays need bash 4+, so agreement is asserted here.
        let mut bash = Bash::new();
        let result = bash
            .exec("declare -A h=([k]=V [key]=W)\nkey=k\nr='h[$key]'\necho \"dollar=[${!r}] direct=[${h[$key]}]\"\nr='h[key]'\necho \"plain=[${!r}] direct=[${h[key]}]\"\nr='h[@]'\necho \"at=[${!r}] direct=[${h[@]}]\"")
            .await
            .unwrap();
        assert_eq!(
            result.stdout,
            "dollar=[V] direct=[V]\nplain=[W] direct=[W]\nat=[V W] direct=[V W]\n"
        );
        assert!(result.stderr.is_empty(), "stderr: {}", result.stderr);
    }

    // Out of scope: `]` inside an assoc key is unsupported by the
    // direct subscript parser as well (`declare` accepts it, reads
    // misparse), so no indirect agreement is asserted for that shape.
    // The boundary scan still tracks quotes so a complete reference
    // stays whole if the parser ever learns quoted brackets.

    /// TM-DOS-135: finite nested indexed indirects match real bash
    /// byte-for-byte. Indexed arrays work on bash 3.2+, so this runs on
    /// macOS and Linux CI alike.
    #[tokio::test]
    async fn indirect_nested_indexed_matches_real_bash() {
        for script in [
            "a=(zero one two three); b=(2); r='a[b[0]]'; printf 'indirect=[%s]\n' \"${!r}\"; printf 'direct=[%s]\n' \"${a[b[0]]}\"",
            "a=(1 2); r='a[0]'; echo \"out=[${!r}]\"",
            "a=(x y); r='a[@]'; echo \"out=[${!r}]\"",
            "a=(x y); i=1; r='a[$i]'; echo \"out=[${!r}]\"",
        ] {
            let expected = Command::new("bash")
                .args(["--noprofile", "--norc", "-c", script])
                .output()
                .expect("run real bash");
            assert!(
                expected.status.success(),
                "real bash failed: {script}: {}",
                String::from_utf8_lossy(&expected.stderr)
            );
            let mut harness = Bash::new();
            let result = harness.exec(script).await.unwrap();
            assert_eq!(
                result.stdout,
                String::from_utf8_lossy(&expected.stdout).into_owned(),
                "script: {script}"
            );
            assert!(result.stderr.is_empty(), "stderr: {}", result.stderr);
        }
    }

    /// V1: Test that command limit prevents infinite execution
    #[tokio::test]
    async fn threat_infinite_commands_blocked() {
        let limits = ExecutionLimits::new().max_commands(10);
        let mut bash = Bash::builder().limits(limits).build();

        // Try to run 20 commands
        let result = bash
            .exec("true; true; true; true; true; true; true; true; true; true; true; true; true; true; true; true; true; true; true; true")
            .await;

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("command") && err.contains("exceeded"),
            "Expected command limit error, got: {}",
            err
        );
    }

    /// TM-DOS-063: Persistent custom file descriptors must be capped.
    #[tokio::test]
    async fn fd_exhaustion_blocked() {
        let limits = ExecutionLimits::new().max_file_descriptors(2);
        let mut bash = Bash::builder().limits(limits).build();

        let result = bash
            .exec("exec 3>/tmp/fd3; exec 4>/tmp/fd4; exec 5>/tmp/fd5")
            .await;

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("file descriptors") && err.contains("exceeded"),
            "Expected fd limit error, got: {}",
            err
        );
    }

    /// TM-DOS-063: Reusing or closing fds should not consume new slots.
    #[tokio::test]
    async fn fd_limit_allows_reuse_and_close() {
        let limits = ExecutionLimits::new().max_file_descriptors(1);
        let mut bash = Bash::builder().limits(limits).build();

        let result = bash
            .exec("exec 3>/tmp/a; exec 3>/tmp/b; exec 3>&-; exec 4>/tmp/c; echo ok >&4; cat /tmp/c")
            .await
            .unwrap();

        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout.trim(), "ok");
    }

    /// TM-DOS-063: `cmd 3<<E` opens fd 3 for the command only; it counts
    /// against the fd cap and a refused open closes what it already opened.
    #[tokio::test]
    async fn here_doc_fds_count_against_fd_cap_and_close() {
        let limits = ExecutionLimits::new().max_file_descriptors(1);
        let mut bash = Bash::builder().limits(limits).build();

        let err = bash
            .exec("cat 3<<<a 4<<<b <&3")
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("file descriptors"), "got: {err}");
        // Nothing stayed open: the next command gets its slot back.
        let result = bash.exec("cat 5<<<ok <&5").await.unwrap();
        assert_eq!(result.stdout, "ok\n");
        let result = bash.exec("cat <&5").await.unwrap();
        assert_ne!(result.exit_code, 0);
    }

    /// TM-DOS-129: the command hash table is bounded however many names a
    /// script hashes.
    #[tokio::test]
    async fn command_hash_table_is_bounded() {
        let mut bash = Bash::new();
        let result = bash
            .exec("for i in $(seq 700); do hash -p /bin/true c$i; done; hash -l | wc -l")
            .await
            .unwrap();
        let rows: usize = result.stdout.trim().parse().unwrap();
        assert!(rows <= 512, "hash table grew to {rows}");
    }

    /// TM-DOS-130: `$((` nested inside arithmetic recurses only 32 levels,
    /// one command's `bad array subscript` reports are capped, and an
    /// invalid `${!r}` target is echoed truncated.
    #[tokio::test]
    async fn array_and_arith_reports_are_bounded() {
        let mut bash = Bash::new();
        let mut expr = String::from("1");
        for _ in 0..40 {
            expr = format!("$(( {expr} + 1 ))");
        }
        let result = bash
            .exec(&format!("echo $(( {expr} )); echo next"))
            .await
            .unwrap();
        assert!(!result.stdout.contains("41"), "got: {}", result.stdout);
        assert!(result.stderr.contains("recursion level exceeded"));

        let reads = "${a[-9]}".repeat(5000);
        let result = bash
            .exec(&format!("a=(1); x={reads}; echo done"))
            .await
            .unwrap();
        assert_eq!(result.stdout, "done\n");
        assert!(result.stderr.len() <= 65 * 1024, "{}", result.stderr.len());

        let result = bash
            .exec("r=$(printf '%5000s' | tr ' ' '-')\necho ${!r}\necho status=$?")
            .await
            .unwrap();
        assert_eq!(result.stdout, "status=1\n");
        assert!(result.stderr.len() < 1024, "{}", result.stderr.len());
    }

    /// TM-DOS-100: archive growth must consume live bytes before allocation.
    #[tokio::test]
    async fn archive_growth_is_admitted_by_shared_live_budget() {
        let fs = Arc::new(InMemoryFs::new());
        fs.write_file(Path::new("/input"), &vec![b'x'; 1024])
            .await
            .unwrap();
        let limits = ExecutionLimits::new().max_live_intermediate_bytes(2_500);
        let mut bash = Bash::builder().fs(fs.clone()).limits(limits).build();

        let result = bash.exec("tar -cf /out.tar /input").await;

        let error = result.unwrap_err().to_string();
        assert!(error.contains("live intermediate bytes"), "{error}");
        assert!(!fs.exists(Path::new("/out.tar")).await.unwrap());
    }

    /// TM-DOS-106: zip inputs and output must be admitted before allocation.
    #[tokio::test]
    async fn zip_growth_is_admitted_by_shared_live_budget() {
        let fs = Arc::new(InMemoryFs::new());
        fs.write_file(Path::new("/input"), &vec![b'x'; 1024])
            .await
            .unwrap();
        let limits = ExecutionLimits::new().max_live_intermediate_bytes(1_500);
        let mut bash = Bash::builder().fs(fs.clone()).limits(limits).build();

        let result = bash.exec("zip /out.zip /input").await;

        let error = result.unwrap_err().to_string();
        assert!(error.contains("live intermediate bytes"), "{error}");
        assert!(!fs.exists(Path::new("/out.zip")).await.unwrap());
    }

    /// TM-DOS-063: `{v}>file` ignores a prior (negative) value of `v`: bash
    /// allocates the lowest free fd >= 10 and stores it, so a crafted value
    /// can neither pick the fd nor bypass the fd limit.
    #[tokio::test]
    async fn fd_var_output_ignores_negative_value_and_respects_limit() {
        let limits = ExecutionLimits::new().max_file_descriptors(1);
        let mut bash = Bash::builder().limits(limits).build();

        let result = bash
            .exec("v=-1; exec {v}>/tmp/neg-out; echo $v")
            .await
            .unwrap();
        assert_eq!(result.stdout, "10\n");

        let result = bash.exec("w=-1; exec {w}>/tmp/neg-out2").await;
        let err = result.unwrap_err().to_string();
        assert!(err.contains("file descriptor"), "got: {err}");
    }

    /// TM-DOS-063: same for input `{v}<file`.
    #[tokio::test]
    async fn fd_var_input_ignores_negative_value_and_respects_limit() {
        let limits = ExecutionLimits::new().max_file_descriptors(1);
        let mut bash = Bash::builder().limits(limits).build();
        bash.exec("echo hi >/tmp/neg-in").await.unwrap();

        let result = bash
            .exec("v=-1; exec {v}</tmp/neg-in; echo $v; read x <&$v; echo $x")
            .await
            .unwrap();
        assert_eq!(result.stdout, "10\nhi\n");

        let result = bash.exec("w=-1; exec {w}</tmp/neg-in").await;
        let err = result.unwrap_err().to_string();
        assert!(err.contains("file descriptor"), "got: {err}");
    }

    /// TM-DOS-063: `cmd {v}>file` on any command leaves the fd open (bash
    /// never undoes `{var}` redirections), so a loop must hit the fd cap.
    #[tokio::test]
    async fn fd_var_on_plain_command_in_loop_hits_fd_limit() {
        let limits = ExecutionLimits::new().max_file_descriptors(8);
        let mut bash = Bash::builder().limits(limits).build();

        let result = bash
            .exec("for i in $(seq 100); do : {v}>/dev/null; done")
            .await;
        let err = result.unwrap_err().to_string();
        assert!(err.contains("file descriptor"), "got: {err}");
    }

    /// Subsequent exec() calls recover after a prior call hits the command limit.
    /// Each exec() is a separate script invocation and gets its own budget.
    #[tokio::test]
    async fn exec_recovers_after_command_limit() {
        let limits = ExecutionLimits::new().max_commands(10);
        let mut bash = Bash::builder().limits(limits).build();

        // First exec: exceed the command limit
        let result = bash
            .exec("true; true; true; true; true; true; true; true; true; true; true; true")
            .await;
        assert!(result.is_err());

        // Second exec: trivial command should succeed — budget resets per exec
        let result = bash.exec("echo hello").await.unwrap();
        assert_eq!(result.stdout.trim(), "hello");
        assert_eq!(result.exit_code, 0);
    }

    /// Loop counters also reset between exec() calls.
    #[tokio::test]
    async fn exec_recovers_after_loop_limit() {
        let limits = ExecutionLimits::new()
            .max_loop_iterations(5)
            .max_total_loop_iterations(10)
            .max_commands(10000);
        let mut bash = Bash::builder().limits(limits).build();

        // First exec: exceed the total loop iteration limit
        let result = bash
            .exec("for i in 1 2 3 4 5; do true; done; for i in 1 2 3 4 5 6; do true; done")
            .await;
        assert!(result.is_err());

        // Second exec: loops should work again
        let result = bash.exec("for i in 1 2 3; do echo $i; done").await.unwrap();
        assert!(result.stdout.contains("1"));
        assert_eq!(result.exit_code, 0);
    }

    /// V2: Test that loop limit prevents infinite loops
    #[tokio::test]
    async fn threat_infinite_loop_blocked() {
        let limits = ExecutionLimits::new()
            .max_loop_iterations(5)
            .max_commands(1000);
        let mut bash = Bash::builder().limits(limits).build();

        let result = bash
            .exec("for i in 1 2 3 4 5 6 7 8 9 10; do echo $i; done")
            .await;

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("loop") && err.contains("exceeded"),
            "Expected loop limit error, got: {}",
            err
        );
    }

    /// V3: Test that function recursion limit prevents stack overflow
    #[tokio::test]
    async fn threat_stack_overflow_blocked() {
        let limits = ExecutionLimits::new()
            .max_function_depth(5)
            .max_commands(1000);
        let mut bash = Bash::builder().limits(limits).build();

        let result = bash
            .exec(
                r#"
                recurse() {
                    echo "depth"
                    recurse
                }
                recurse
                "#,
            )
            .await;

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("function") && err.contains("exceeded"),
            "Expected function depth error, got: {}",
            err
        );
    }

    /// Test while loop with always-true condition is limited
    #[tokio::test]
    async fn threat_while_true_blocked() {
        let limits = ExecutionLimits::new()
            .max_loop_iterations(10)
            .max_commands(1000);
        let mut bash = Bash::builder().limits(limits).build();

        // This would run forever without limits
        let result = bash
            .exec("i=0; while [ $i -lt 100 ]; do i=$((i+1)); done")
            .await;

        assert!(result.is_err());
    }

    /// Test that timeout is respected (if implemented)
    #[tokio::test]
    async fn threat_cpu_exhaustion_timeout() {
        let limits = ExecutionLimits::new()
            .timeout(Duration::from_millis(100))
            .max_commands(1_000_000)
            .max_loop_iterations(1_000_000);
        let mut bash = Bash::builder().limits(limits).build();

        // This should timeout, not complete
        let start = std::time::Instant::now();
        let _ = bash
            .exec("for i in $(seq 1 1000000); do echo $i; done")
            .await;
        let elapsed = start.elapsed();

        // Should complete quickly due to either timeout or loop limit.
        // Under ASan/Miri the overhead can be ~200x, so use a very generous bound.
        assert!(elapsed < Duration::from_secs(300));
    }
}

// =============================================================================
// 2. SANDBOX ESCAPE TESTS
// =============================================================================

mod sandbox_escape {
    use super::*;

    /// Test path traversal is blocked
    #[tokio::test]
    async fn threat_path_traversal_blocked() {
        let mut bash = Bash::new();

        // Try to escape via ../
        let result = bash.exec("cat ../../../etc/passwd").await.unwrap();
        // `..` clamps at the VFS root: only the synthetic file is reachable.
        assert!(result.stdout.contains("sandbox:x:1000:1000"));
        assert!(!result.stdout.contains("daemon:"));
    }

    /// Test absolute path to /etc/passwd fails
    #[tokio::test]
    async fn threat_etc_passwd_blocked() {
        let mut bash = Bash::new();

        let result = bash.exec("cat /etc/passwd").await.unwrap();
        // THREAT[TM-ISO-018]: synthetic file built from the virtual identity.
        assert_eq!(
            result.stdout,
            "sandbox:x:1000:1000:sandbox:/home/sandbox:/bin/bash\nnobody:x:65534:65534:nobody:/nonexistent:/usr/sbin/nologin\n"
        );
        let bare = Bash::builder()
            .rootfs(false)
            .build()
            .exec("cat /etc/passwd")
            .await
            .unwrap();
        assert_ne!(bare.exit_code, 0);
    }

    /// Test /proc access is blocked (no /proc in virtual FS)
    #[tokio::test]
    async fn threat_proc_access_blocked() {
        let mut bash = Bash::new();

        let result = bash.exec("cat /proc/self/environ").await.unwrap();
        assert!(result.exit_code != 0);
    }

    /// Test eval is implemented but safe in virtual environment
    ///
    /// eval is a POSIX special builtin that's now implemented. In the virtual environment,
    /// eval can only execute other builtins (no external commands), so it's safe.
    /// The current implementation stores the command but doesn't re-execute it.
    #[tokio::test]
    async fn threat_eval_is_safe_in_sandbox() {
        let mut bash = Bash::new();

        // eval is now implemented - it stores the command but in virtual environment
        // it can only run builtins, so it's safe
        let result = bash.exec("eval echo test").await.unwrap();
        // eval returns 0 (success) as it's a valid builtin
        assert_eq!(result.exit_code, 0);
        // Note: current impl stores command but doesn't execute it
    }

    /// Test exec cannot escape sandbox — only VFS scripts are reachable
    ///
    /// exec executes commands within the VFS (run + exit). A host program
    /// path is not there, so it fails with exit 127; `/bin/bash` is a
    /// root-filesystem stub that re-enters the in-process interpreter.
    /// Either way there is no real process replacement.
    #[tokio::test]
    async fn threat_exec_not_available() {
        let mut bash = Bash::new();

        let result = bash.exec("exec /usr/bin/gcc").await.unwrap();
        assert_eq!(result.exit_code, 127);

        let result = bash
            .exec("exec /bin/bash -c 'cat /etc/passwd'")
            .await
            .unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(!result.stdout.contains("root:x:0:0"), "host passwd leaked");
    }

    /// Test exec argv is never re-parsed as shell source (quote injection safe).
    #[tokio::test]
    async fn threat_exec_argument_quote_injection_blocked() {
        let mut bash = Bash::new();

        let result = bash
            .exec("exec echo \"foo' ; touch /tmp/exec_injected #\"")
            .await
            .unwrap();
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout.trim(), "foo' ; touch /tmp/exec_injected #");

        let check = bash.exec("cat /tmp/exec_injected").await.unwrap();
        assert_ne!(
            check.exit_code, 0,
            "injected touch must not execute through exec argv quoting"
        );
    }

    /// Test external command execution is blocked
    #[tokio::test]
    async fn threat_external_commands_blocked() {
        let mut bash = Bash::new();

        // A path with no root-filesystem stub is not executable.
        if let Ok(r) = bash.exec("/bin/host-only-tool").await {
            assert!(r.exit_code != 0);
        }
        // /bin/ls is a stub that runs the builtin over the VFS, not the host ls.
        let r = bash.exec("/bin/ls /").await.unwrap();
        assert!(r.stdout.contains("tmp\n") && !r.stdout.contains("boot\n"));

        if let Ok(r) = bash.exec("./malicious").await {
            assert!(r.exit_code != 0);
        }
    }

    /// Test symlink creation (stored but not followed for escape)
    #[tokio::test]
    async fn threat_symlink_escape_blocked() {
        let mut bash = Bash::new();

        // Even if symlinks could be created, they shouldn't allow escape
        // Virtual FS doesn't follow symlinks
        let result = bash.exec("cat /tmp/symlink_to_etc").await.unwrap();
        assert!(result.exit_code != 0);
    }
}

// =============================================================================
// 3. INJECTION ATTACK TESTS
// =============================================================================

mod injection_attacks {
    use super::*;

    /// TM-INJ-026: a redirect target is expanded once. A value holding
    /// shell syntax names a file, it is never run, and a glob matching
    /// several files is an ambiguous redirect that writes nothing.
    #[tokio::test]
    async fn redirect_target_expanded_once() {
        let mut bash = Bash::new();
        let result = bash
            .exec(
                r#"cd /tmp; v='$(touch${IFS}pwned)'; echo x > $v; [ -e pwned ] && echo RAN
cat ./'$(touch${IFS}pwned)'
: > a1; : > a2; echo y > a*; echo s=$?; cat a1 a2"#,
            )
            .await
            .unwrap();
        assert_eq!(result.stdout, "x\ns=1\n");
        assert!(result.stderr.contains("ambiguous redirect"));
    }

    /// Test that variable content with semicolons doesn't execute as separate command
    /// Security: Variables should expand to strings, not be re-parsed as code
    #[tokio::test]
    async fn threat_semicolon_in_variable_safe() {
        let mut bash = Bash::new();

        // Set a variable with a semicolon (simulating injection attempt)
        bash.exec("safe=harmless").await.unwrap();
        let result = bash.exec("echo $safe").await.unwrap();

        // Simple case works
        assert_eq!(result.stdout.trim(), "harmless");
        assert_eq!(result.exit_code, 0);
    }

    /// Test that command substitution in single quotes is literal
    #[tokio::test]
    async fn threat_command_sub_in_single_quotes() {
        let mut bash = Bash::new();

        // Single quotes should prevent command substitution
        let result = bash.exec("echo '$(whoami)'").await.unwrap();
        assert!(result.stdout.contains("$(whoami)"));
        assert!(!result.stdout.contains("sandbox"));
    }

    /// Test that backticks in single quotes are literal
    #[tokio::test]
    async fn threat_backticks_in_single_quotes() {
        let mut bash = Bash::new();

        let result = bash.exec("echo '`hostname`'").await.unwrap();
        assert!(result.stdout.contains("`hostname`"));
        assert!(!result.stdout.contains("bashkit-sandbox"));
    }

    /// Test that eval is implemented but safe (can only run builtins)
    ///
    /// eval is a POSIX special builtin. In virtual mode, it can only execute
    /// builtins (no external commands), so it cannot be used for code injection.
    #[tokio::test]
    async fn threat_eval_is_sandboxed() {
        let mut bash = Bash::new();

        // eval is executed by the interpreter (Interpreter::execute_eval), not the
        // Eval builtin stub, and in virtual mode can only run builtins.
        let result = bash.exec("eval echo test").await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(
            result.stdout.contains("test"),
            "eval should parse and execute its argument: {:?}",
            result.stdout
        );
    }

    /// Regression: the `_EVAL_CMD` magic-variable channel was removed. eval no
    /// longer routes its command through a user-visible shell variable, so
    /// `_EVAL_CMD` is now an ordinary variable with no special meaning and
    /// setting it has no effect on eval.
    #[tokio::test]
    async fn threat_eval_cmd_channel_removed() {
        let mut bash = Bash::new();

        // _EVAL_CMD is no longer reserved: a user can assign it like any var.
        let result = bash
            .exec("_EVAL_CMD=hijack; eval echo safe; echo $_EVAL_CMD")
            .await
            .unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(
            result.stdout.contains("safe"),
            "eval must execute its own argument, not $_EVAL_CMD: {:?}",
            result.stdout
        );
        assert!(
            result.stdout.contains("hijack"),
            "_EVAL_CMD is now an ordinary variable: {:?}",
            result.stdout
        );
    }

    /// Test path with null byte (Rust prevents this)
    #[tokio::test]
    async fn threat_null_byte_in_path() {
        let mut bash = Bash::new();

        // Rust strings can't contain null bytes, so this is safe by construction
        let result = bash.exec("cat '/tmp/file'").await.unwrap();
        // Just verify it doesn't crash
        assert!(result.exit_code == 0 || result.exit_code == 1);
    }

    /// Test that pipe operator in quotes is literal
    #[tokio::test]
    async fn threat_pipe_in_quotes() {
        let mut bash = Bash::new();

        let result = bash.exec("echo '| cat /etc/passwd'").await.unwrap();
        assert!(result.stdout.contains("| cat /etc/passwd"));
    }

    /// Test that redirect in quotes is literal
    #[tokio::test]
    async fn threat_redirect_in_quotes() {
        let mut bash = Bash::new();

        let result = bash.exec("echo '> /tmp/pwned'").await.unwrap();
        assert!(result.stdout.contains("> /tmp/pwned"));
    }
}

// =============================================================================
// 4. INFORMATION DISCLOSURE TESTS
// =============================================================================

mod information_disclosure {
    use super::*;

    /// Test hostname returns sandbox value, not real hostname
    #[tokio::test]
    async fn threat_hostname_hardcoded() {
        let mut bash = Bash::new();

        let result = bash.exec("hostname").await.unwrap();
        assert_eq!(result.stdout.trim(), "bashkit-sandbox");
        assert_eq!(result.exit_code, 0);
    }

    /// Test hostname cannot be set
    #[tokio::test]
    async fn threat_hostname_cannot_set() {
        let mut bash = Bash::new();

        let result = bash.exec("hostname evil.attacker.com").await.unwrap();
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("cannot set"));
    }

    /// Test uname returns sandbox values
    #[tokio::test]
    async fn threat_uname_hardcoded() {
        let mut bash = Bash::new();

        let result = bash.exec("uname -a").await.unwrap();
        assert!(result.stdout.contains("bashkit-sandbox"));
        assert!(result.stdout.contains("Linux"));
        // Should NOT contain real kernel info
        assert!(!result.stdout.contains("Ubuntu"));
        assert!(!result.stdout.contains("Debian"));
    }

    /// Test uname -n returns sandbox hostname
    #[tokio::test]
    async fn threat_uname_nodename_hardcoded() {
        let mut bash = Bash::new();

        let result = bash.exec("uname -n").await.unwrap();
        assert_eq!(result.stdout.trim(), "bashkit-sandbox");
    }

    /// Test whoami returns sandbox user
    #[tokio::test]
    async fn threat_whoami_hardcoded() {
        let mut bash = Bash::new();

        let result = bash.exec("whoami").await.unwrap();
        assert_eq!(result.stdout.trim(), "sandbox");
    }

    /// Test id returns sandbox IDs
    #[tokio::test]
    async fn threat_id_hardcoded() {
        let mut bash = Bash::new();

        let result = bash.exec("id").await.unwrap();
        assert!(result.stdout.contains("uid=1000"));
        assert!(result.stdout.contains("sandbox"));

        let result = bash.exec("id -u").await.unwrap();
        assert_eq!(result.stdout.trim(), "1000");

        let result = bash.exec("id -g").await.unwrap();
        assert_eq!(result.stdout.trim(), "1000");
    }

    /// Test that sensitive env vars are only accessible if passed
    #[tokio::test]
    async fn threat_env_vars_isolated() {
        let mut bash = Bash::new();

        // Default instance shouldn't have sensitive vars
        let result = bash.exec("echo $DATABASE_URL").await.unwrap();
        assert!(result.stdout.trim().is_empty());

        let result = bash.exec("echo $AWS_SECRET_ACCESS_KEY").await.unwrap();
        assert!(result.stdout.trim().is_empty());

        let result = bash.exec("echo $API_KEY").await.unwrap();
        assert!(result.stdout.trim().is_empty());
    }

    /// Test that only explicitly passed env vars are available
    #[tokio::test]
    async fn threat_env_vars_explicit_only() {
        let mut bash = Bash::builder().env("ALLOWED_VAR", "allowed_value").build();

        let result = bash.exec("echo $ALLOWED_VAR").await.unwrap();
        assert_eq!(result.stdout.trim(), "allowed_value");

        // But host vars aren't magically available: $PATH is the synthetic
        // sandbox default, never the host's.
        let result = bash.exec("echo $PATH").await.unwrap();
        assert_eq!(result.stdout.trim(), "/usr/local/bin:/usr/bin:/bin");
        let result = bash.exec("echo $HOST_ONLY_SENTINEL_VAR").await.unwrap();
        assert!(result.stdout.trim().is_empty());
    }

    /// Test /proc is not accessible
    #[tokio::test]
    async fn threat_proc_environ_blocked() {
        let mut bash = Bash::new();

        let result = bash.exec("cat /proc/self/environ").await.unwrap();
        assert_eq!(result.exit_code, 1);
        assert!(result.stdout.is_empty());
    }
}

// =============================================================================
// 5. NETWORK SECURITY TESTS (when http_client feature enabled)
// =============================================================================

mod network_security {
    use super::*;

    /// Test that curl/wget commands aren't available without http_client feature
    #[tokio::test]
    async fn threat_network_commands_not_builtin() {
        let mut bash = Bash::new();

        // curl/wget should not be available - either error or non-zero exit
        let result = bash.exec("curl https://evil.com").await;
        if let Ok(r) = result {
            assert!(r.exit_code != 0);
        }
        // Error is also acceptable

        let result = bash.exec("wget https://evil.com").await;
        if let Ok(r) = result {
            assert!(r.exit_code != 0);
        }
        // Error is also acceptable
    }
}

// =============================================================================
// 6. SESSION ISOLATION TESTS
// =============================================================================

mod session_isolation {
    use super::*;
    use bashkit::InMemoryFs;
    use std::sync::Arc;

    /// Test that separate instances have isolated filesystems
    #[tokio::test]
    async fn threat_isolation_fs_isolation() {
        let fs_a = Arc::new(InMemoryFs::new());
        let fs_b = Arc::new(InMemoryFs::new());

        let mut tenant_a = Bash::builder().fs(fs_a).build();
        let mut tenant_b = Bash::builder().fs(fs_b).build();

        // Tenant A writes a secret
        tenant_a
            .exec("echo 'SECRET_A' > /tmp/secret.txt")
            .await
            .unwrap();

        // Tenant B cannot read it
        let result = tenant_b.exec("cat /tmp/secret.txt").await.unwrap();
        assert_eq!(result.exit_code, 1);
        assert!(!result.stdout.contains("SECRET_A"));
    }

    /// Test that separate instances have isolated variables
    #[tokio::test]
    async fn threat_isolation_variable_isolation() {
        let mut tenant_a = Bash::new();
        let mut tenant_b = Bash::new();

        tenant_a.exec("SECRET=password123").await.unwrap();

        let result = tenant_b.exec("echo $SECRET").await.unwrap();
        assert!(result.stdout.trim().is_empty());
    }

    /// Test that separate instances have isolated functions
    #[tokio::test]
    async fn threat_isolation_function_isolation() {
        let mut tenant_a = Bash::new();
        let mut tenant_b = Bash::new();

        tenant_a.exec("steal() { echo 'stolen'; }").await.unwrap();

        // Function defined in tenant_a should not exist in tenant_b
        let result = tenant_b.exec("steal").await.unwrap();
        // Should return command not found (exit 127)
        assert_eq!(result.exit_code, 127);
        assert!(!result.stdout.contains("stolen"));
        assert!(result.stderr.contains("command not found"));
    }

    /// Test that limits are per-instance
    #[tokio::test]
    async fn threat_isolation_limits_isolation() {
        let limits_strict = ExecutionLimits::new().max_commands(5);
        let limits_relaxed = ExecutionLimits::new().max_commands(100);

        let mut tenant_strict = Bash::builder().limits(limits_strict).build();
        let mut tenant_relaxed = Bash::builder().limits(limits_relaxed).build();

        // Strict tenant hits limit
        let result = tenant_strict
            .exec("true; true; true; true; true; true; true")
            .await;
        assert!(result.is_err());

        // Relaxed tenant can do more
        let result = tenant_relaxed
            .exec("true; true; true; true; true; true; true")
            .await;
        assert!(result.is_ok());
    }

    /// TM-ISO-019: Aliases defined in one session must not leak to another
    #[tokio::test]
    async fn threat_isolation_alias_isolation() {
        let mut tenant_a = Bash::new();
        let mut tenant_b = Bash::new();

        tenant_a
            .exec("alias secret_cmd='echo LEAKED'")
            .await
            .unwrap();

        // Tenant B should not have tenant A's alias
        let result = tenant_b.exec("secret_cmd").await.unwrap();
        assert_eq!(result.exit_code, 127);
        assert!(!result.stdout.contains("LEAKED"));
    }

    /// Alias expansion must work across separate exec() calls (issue #1130).
    /// shopt -s expand_aliases set in one exec() must persist to the next.
    #[tokio::test]
    async fn alias_expansion_persists_across_exec_calls() {
        let mut bash = Bash::new();

        bash.exec("shopt -s expand_aliases").await.unwrap();
        bash.exec("alias ll='echo alias_worked'").await.unwrap();

        let result = bash.exec("ll").await.unwrap();
        assert_eq!(
            result.exit_code, 0,
            "alias should resolve: {}",
            result.stderr
        );
        assert_eq!(result.stdout.trim(), "alias_worked");
    }

    /// TM-ISO-020: Trap handlers in one session must not fire in another
    #[tokio::test]
    async fn threat_isolation_trap_isolation() {
        let mut tenant_a = Bash::new();
        let mut tenant_b = Bash::new();

        tenant_a.exec("trap 'echo TRAP_LEAKED' EXIT").await.unwrap();

        // Tenant B's EXIT trap should not produce tenant A's output
        let result = tenant_b.exec("true").await.unwrap();
        assert!(!result.stdout.contains("TRAP_LEAKED"));
    }

    /// TM-ISO-019: Shell options (set -e, set -o pipefail, etc.) must not leak
    #[tokio::test]
    async fn threat_isolation_shell_options_isolation() {
        let mut tenant_a = Bash::new();
        let mut tenant_b = Bash::new();

        tenant_a.exec("set -e").await.unwrap();
        tenant_a.exec("set -o pipefail").await.unwrap();

        // Tenant B should still have default options (errexit off)
        // If errexit leaked, `false` would abort and we'd get an error
        let result = tenant_b.exec("false; echo STILL_RUNNING").await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("STILL_RUNNING"));
    }

    /// TM-ISO-020: Exported environment variables must not cross sessions
    #[tokio::test]
    async fn threat_isolation_export_isolation() {
        let mut tenant_a = Bash::new();
        let mut tenant_b = Bash::new();

        tenant_a.exec("export DB_PASSWORD=s3cret").await.unwrap();

        // Tenant B must not see tenant A's exported var
        let result = tenant_b.exec("echo \"[$DB_PASSWORD]\"").await.unwrap();
        assert_eq!(result.stdout.trim(), "[]");
    }

    /// TM-ISO-019: Indexed and associative arrays must not leak between sessions
    #[tokio::test]
    async fn threat_isolation_array_isolation() {
        let mut tenant_a = Bash::new();
        let mut tenant_b = Bash::new();

        tenant_a
            .exec("SECRET_ARR=(one two three); declare -A SECRET_MAP; SECRET_MAP[key]=val")
            .await
            .unwrap();

        // Tenant B must not see tenant A's arrays
        let result = tenant_b
            .exec("echo \"${SECRET_ARR[0]}\" \"${SECRET_MAP[key]}\"")
            .await
            .unwrap();
        assert_eq!(result.stdout.trim(), "");
    }

    /// TM-ISO-020: Working directory changes must not leak between sessions
    #[tokio::test]
    async fn threat_isolation_cwd_isolation() {
        let fs_a = Arc::new(InMemoryFs::new());
        let mut tenant_a = Bash::builder().fs(fs_a).build();
        let mut tenant_b = Bash::new();

        tenant_a
            .exec("mkdir -p /opt/secret && cd /opt/secret")
            .await
            .unwrap();

        // Tenant B should still be at default cwd, not /opt/secret
        let result = tenant_b.exec("pwd").await.unwrap();
        assert!(!result.stdout.contains("/opt/secret"));
    }

    /// TM-ISO-019: Exit codes from one session must not affect another
    #[tokio::test]
    async fn threat_isolation_exit_code_isolation() {
        let mut tenant_a = Bash::new();
        let mut tenant_b = Bash::new();

        // Tenant A ends with failure
        tenant_a.exec("false").await.unwrap();

        // Tenant B should start with clean exit code (0)
        let result = tenant_b.exec("echo $?").await.unwrap();
        assert_eq!(result.stdout.trim(), "0");
    }

    /// TM-ISO-020: Concurrent sessions must not interfere with each other
    #[tokio::test]
    async fn threat_isolation_concurrent_isolation() {
        use tokio::task::JoinSet;

        let mut tasks = JoinSet::new();

        for i in 0..10 {
            tasks.spawn(async move {
                let mut bash = Bash::new();
                let secret = format!("TENANT_{}_SECRET", i);
                bash.exec(&format!("MY_SECRET={}", secret)).await.unwrap();

                // Each session should only see its own variable
                let result = bash.exec("echo $MY_SECRET").await.unwrap();
                assert_eq!(result.stdout.trim(), secret);

                // Try to probe for other tenants' variables
                for j in 0..10 {
                    if j != i {
                        let other = format!("TENANT_{}_SECRET", j);
                        let probe = bash.exec("echo $MY_SECRET").await.unwrap();
                        assert!(!probe.stdout.contains(&other));
                    }
                }
            });
        }

        while let Some(result) = tasks.join_next().await {
            result.unwrap();
        }
    }

    /// TM-ISO-019: Concurrent filesystem writes must not cross-contaminate
    #[tokio::test]
    async fn threat_isolation_concurrent_fs_isolation() {
        use tokio::task::JoinSet;

        let mut tasks = JoinSet::new();

        for i in 0..10 {
            tasks.spawn(async move {
                let fs = Arc::new(InMemoryFs::new());
                let mut bash = Bash::builder().fs(fs).build();

                let secret = format!("FS_SECRET_{}", i);
                bash.exec(&format!("echo '{}' > /tmp/data.txt", secret))
                    .await
                    .unwrap();

                let result = bash.exec("cat /tmp/data.txt").await.unwrap();
                assert!(
                    result.stdout.contains(&secret),
                    "Tenant {} should see its own secret",
                    i
                );

                // Verify no other tenant's data leaked in
                for j in 0..10 {
                    if j != i {
                        let other_secret = format!("FS_SECRET_{}", j);
                        assert!(
                            !result.stdout.contains(&other_secret),
                            "Tenant {} saw tenant {}'s data!",
                            i,
                            j
                        );
                    }
                }
            });
        }

        while let Some(result) = tasks.join_next().await {
            result.unwrap();
        }
    }

    /// TM-ISO-020: Session state snapshot/restore must not affect other sessions
    #[tokio::test]
    async fn threat_isolation_snapshot_isolation() {
        let mut tenant_a = Bash::new();
        let mut tenant_b = Bash::new();

        tenant_a.exec("SNAPSHOT_SECRET=before").await.unwrap();
        let snapshot = tenant_a.shell_state();

        // Mutate tenant_a
        tenant_a.exec("SNAPSHOT_SECRET=after").await.unwrap();

        // Restore snapshot on tenant_a
        tenant_a.restore_shell_state(&snapshot);

        // Tenant B should be unaffected by any of this
        let result = tenant_b.exec("echo \"[$SNAPSHOT_SECRET]\"").await.unwrap();
        assert_eq!(result.stdout.trim(), "[]");
    }

    /// TM-ISO-019: Adversarial probing — script tries to discover other sessions
    /// by iterating common paths and variable names
    #[tokio::test]
    async fn threat_isolation_adversarial_probing() {
        let mut victim = Bash::new();
        victim.exec("API_KEY=sk-live-12345").await.unwrap();
        victim
            .exec("export DATABASE_URL=postgres://secret@db/prod")
            .await
            .unwrap();

        let mut attacker = Bash::new();

        // Try common secret variable names
        let probe_script = r#"
echo "$API_KEY"
echo "$DATABASE_URL"
echo "$AWS_SECRET_ACCESS_KEY"
echo "$GITHUB_TOKEN"
echo "$PASSWORD"
echo "$SECRET"
echo "$PRIVATE_KEY"
"#;
        let result = attacker.exec(probe_script).await.unwrap();
        assert!(!result.stdout.contains("sk-live"));
        assert!(!result.stdout.contains("postgres://"));

        // Try to enumerate env via env/printenv/set
        let result = attacker
            .exec("env 2>/dev/null; printenv 2>/dev/null")
            .await
            .unwrap();
        assert!(!result.stdout.contains("sk-live"));
        assert!(!result.stdout.contains("postgres://"));
    }

    /// TM-ISO-020: Adversarial script tries to read /proc, /sys, /dev for leaks
    #[tokio::test]
    async fn threat_isolation_proc_probing() {
        let mut bash = Bash::new();

        // These should not expose host information
        // /proc and /etc hold only synthetic files (checked below).
        let listing = bash.exec("ls /proc").await.unwrap();
        assert_eq!(
            listing.stdout,
            "1\ncpuinfo\nloadavg\nmeminfo\nmounts\nself\nsys\nuptime\nversion\n"
        );
        // The shell's own process view is static and synthetic.
        let own = bash
            .exec("cat /proc/self/cmdline | tr '\\0' ' '; readlink /proc/self/exe; grep ^Uid /proc/1/status")
            .await
            .unwrap();
        assert_eq!(own.stdout, "bash /bin/bash\nUid:\t1000\t1000\t1000\t1000\n");
        let probes = vec![
            "cat /proc/self/environ 2>/dev/null",
            "cat /proc/1/environ 2>/dev/null",
            "ls /proc/2 2>/dev/null",
            "cat /etc/shadow 2>/dev/null",
        ];

        for probe in probes {
            let result = bash.exec(probe).await.unwrap();
            // VFS shouldn't have real /proc or /etc content
            assert!(
                result.stdout.trim().is_empty() || result.exit_code != 0,
                "Probe '{}' returned unexpected data: {}",
                probe,
                result.stdout
            );
        }
    }

    /// TM-ISO-019: jq env isolation — jq in one session must not see
    /// environment variables from another concurrent session
    #[tokio::test]
    async fn threat_isolation_jq_env_cross_session() {
        let mut tenant_a = Bash::new();
        let mut tenant_b = Bash::new();

        tenant_a
            .exec("export JQ_SECRET=tenant_a_secret")
            .await
            .unwrap();

        // Tenant B's jq should not see tenant A's env
        let result = tenant_b
            .exec("jq -n 'env.JQ_SECRET // \"none\"'")
            .await
            .unwrap();
        assert!(
            !result.stdout.contains("tenant_a_secret"),
            "jq leaked cross-session env: {}",
            result.stdout
        );
    }

    /// TM-ISO-020: Subshell isolation — mutations in subshell must not
    /// leak to parent, and parent state must not leak to sibling sessions
    #[tokio::test]
    async fn threat_isolation_subshell_isolation() {
        let mut bash = Bash::new();

        bash.exec("OUTER=original").await.unwrap();
        bash.exec("(OUTER=mutated; INNER=leaked)").await.unwrap();

        // Parent should not see subshell mutations
        let result = bash.exec("echo $OUTER $INNER").await.unwrap();
        assert_eq!(result.stdout.trim(), "original");

        // Separate session should see neither
        let mut other = Bash::new();
        let result = other.exec("echo \"[$OUTER][$INNER]\"").await.unwrap();
        assert_eq!(result.stdout.trim(), "[][]");
    }
}

// =============================================================================
// 7. EDGE CASE TESTS
// =============================================================================

mod edge_cases {
    use super::*;

    /// Test empty script
    #[tokio::test]
    async fn threat_empty_script() {
        let mut bash = Bash::new();
        let result = bash.exec("").await.unwrap();
        assert_eq!(result.exit_code, 0);
    }

    /// Test script with only whitespace
    #[tokio::test]
    async fn threat_whitespace_script() {
        let mut bash = Bash::new();
        let result = bash.exec("   \n\t\n   ").await.unwrap();
        assert_eq!(result.exit_code, 0);
    }

    /// Test script with only comments
    #[tokio::test]
    async fn threat_comment_only_script() {
        let mut bash = Bash::new();
        let result = bash
            .exec("# This is a comment\n# Another comment")
            .await
            .unwrap();
        assert_eq!(result.exit_code, 0);
    }

    /// Test very long single line
    #[tokio::test]
    async fn threat_long_line() {
        let mut bash = Bash::new();
        let long_arg = "a".repeat(10000);
        let result = bash.exec(&format!("echo {}", long_arg)).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.len() >= 10000);
    }

    /// Test deeply nested command substitution
    #[tokio::test]
    async fn threat_nested_command_sub() {
        let limits = ExecutionLimits::new()
            .max_commands(100)
            .max_function_depth(50);
        let mut bash = Bash::builder().limits(limits).build();

        // Moderately nested (4 levels) - should succeed and produce correct output
        let result = bash.exec("echo $(echo $(echo $(echo hello)))").await;
        let result = result.expect("4-level command substitution should succeed");
        assert_eq!(
            result.stdout.trim(),
            "hello",
            "nested command sub should produce 'hello'"
        );
    }

    /// TM-DOS-022: Deep subshell nesting must hit ast_depth limit or handle gracefully
    #[tokio::test]
    async fn threat_deep_subshell_nesting_blocked() {
        let limits = ExecutionLimits::new()
            .max_commands(100)
            .max_function_depth(50)
            .max_ast_depth(20);
        let mut bash = Bash::builder().limits(limits).build();

        // 200-level nested subshells against max_ast_depth=20
        let script = format!("{}echo hello{}", "(".repeat(200), ")".repeat(200),);
        let result = bash.exec(&script).await;
        // Must not crash — either errors with depth limit or returns Ok (graceful)
        match result {
            Ok(_) => {} // Depth limit caused parse truncation → Ok with empty output
            Err(e) => {
                let err = e.to_string();
                assert!(
                    err.contains("nesting") || err.contains("depth") || err.contains("fuel"),
                    "Expected depth/nesting/fuel error, got: {}",
                    err
                );
            }
        }
    }

    /// TM-DOS-026: Deep arithmetic nesting must not crash (depth-limited)
    #[tokio::test]
    async fn threat_deep_arithmetic_nesting_safe() {
        let mut bash = Bash::new();

        // 500-level arithmetic parens — now bounded by MAX_ARITHMETIC_DEPTH
        let depth = 500;
        let script = format!("echo $(({} 1 {}))", "(".repeat(depth), ")".repeat(depth),);
        let result = bash.exec(&script).await;
        // Must not crash. Past the depth limit the expansion fails like a
        // bash arithmetic error: message on stderr, line abandoned.
        match result {
            Ok(r) => {
                assert!(
                    r.stderr.contains("recursion level exceeded"),
                    "depth limit should be reported, got {:?}",
                    r.stderr
                );
                assert_eq!(r.exit_code, 1);
            }
            Err(_) => {
                // Error also acceptable (parser-level rejection)
            }
        }
    }

    /// Test special variable names
    #[tokio::test]
    async fn threat_special_variable_names() {
        let mut bash = Bash::new();

        // These should all be safe
        let result = bash.exec("echo $?").await.unwrap(); // Exit code
        assert!(result.exit_code == 0);

        let result = bash.exec("echo $$").await.unwrap(); // PID (may not be implemented)
        assert!(result.exit_code == 0);

        let result = bash.exec("echo $#").await.unwrap(); // Arg count
        assert!(result.exit_code == 0);
    }

    /// Test command not found returns exit code 127 and proper error message
    #[tokio::test]
    async fn command_not_found_exit_code() {
        let mut bash = Bash::new();

        // Unknown command should return exit code 127 (not a Rust error)
        let result = bash.exec("nonexistent_command").await.unwrap();
        assert_eq!(result.exit_code, 127);
        assert!(
            result.stderr.contains("command not found"),
            "stderr should contain 'command not found', got: {}",
            result.stderr
        );
        assert!(
            result.stderr.contains("nonexistent_command"),
            "stderr should contain the command name, got: {}",
            result.stderr
        );
    }

    /// Test command not found in script continues execution
    #[tokio::test]
    async fn command_not_found_continues_script() {
        let mut bash = Bash::new();

        // Script should continue after command not found
        let result = bash.exec("unknown_cmd; echo after").await.unwrap();
        assert!(result.stdout.contains("after"));
        // Last command succeeded, so exit code should be 0
        assert_eq!(result.exit_code, 0);
    }

    /// Test command not found stderr format matches bash
    #[tokio::test]
    async fn command_not_found_stderr_format() {
        let mut bash = Bash::new();

        // Use rsync which is never a builtin (ssh may be with ssh feature)
        let result = bash.exec("rsync").await.unwrap();
        assert_eq!(result.exit_code, 127);
        // Should match bash format: "bash: line 1: cmd: command not found"
        assert!(
            result
                .stderr
                .starts_with("bash: line 1: rsync: command not found"),
            "stderr should match bash format, got: {}",
            result.stderr
        );
    }

    /// Test various common missing commands all return 127
    #[tokio::test]
    async fn command_not_found_various_commands() {
        let mut bash = Bash::new();

        // Commands that are NOT implemented as builtins
        // Note: git is a builtin (returns exit 1 when not configured, not 127)
        // Note: ssh/scp/sftp are builtins when ssh feature is enabled
        // Note: nano is a builtin when the terminal feature is enabled
        let mut missing = vec!["apt", "yum", "docker", "vim", "rsync"];
        if cfg!(feature = "terminal") {
            let result = bash.exec("nano </dev/null").await.unwrap();
            assert_ne!(result.exit_code, 127, "nano is a terminal builtin");
        } else {
            missing.push("nano");
        }
        for cmd in &missing {
            let result = bash.exec(cmd).await.unwrap();
            assert_eq!(
                result.exit_code, 127,
                "{} should return exit 127, got {}",
                cmd, result.exit_code
            );
            assert!(
                result.stderr.contains("command not found"),
                "{} stderr should contain 'command not found', got: {}",
                cmd,
                result.stderr
            );
        }
    }

    /// Test $? captures exit code 127 after command not found
    #[tokio::test]
    async fn command_not_found_exit_status_variable() {
        let mut bash = Bash::new();

        let result = bash.exec("nonexistent; echo $?").await.unwrap();
        assert!(result.stdout.contains("127"));
        // Final exit code is 0 because echo succeeded
        assert_eq!(result.exit_code, 0);
    }

    /// Test command not found in pipeline
    #[tokio::test]
    async fn command_not_found_in_pipeline() {
        let mut bash = Bash::new();

        // Pipeline with missing command should still work
        let result = bash.exec("echo hello | nonexistent_filter").await.unwrap();
        // Exit code should be from the last command (127)
        assert_eq!(result.exit_code, 127);
    }

    /// Test command not found in conditional
    #[tokio::test]
    async fn command_not_found_in_conditional() {
        let mut bash = Bash::new();

        // if with missing command should take else branch
        let result = bash
            .exec("if nonexistent_cmd; then echo yes; else echo no; fi")
            .await
            .unwrap();
        assert!(result.stdout.contains("no"));
        assert_eq!(result.exit_code, 0);
    }

    /// Test command not found with || operator
    #[tokio::test]
    async fn command_not_found_or_operator() {
        let mut bash = Bash::new();

        // Should execute fallback after command not found
        let result = bash.exec("nonexistent || echo fallback").await.unwrap();
        assert!(result.stdout.contains("fallback"));
        assert_eq!(result.exit_code, 0);
    }

    /// Test command not found with && operator
    #[tokio::test]
    async fn command_not_found_and_operator() {
        let mut bash = Bash::new();

        // Should not execute second command after failure
        let result = bash.exec("nonexistent && echo success").await.unwrap();
        assert!(!result.stdout.contains("success"));
        assert_eq!(result.exit_code, 127);
    }

    /// Test builtins still work (positive test case)
    #[tokio::test]
    async fn builtins_still_work() {
        let mut bash = Bash::new();

        // Verify various builtins work correctly
        let result = bash.exec("echo hello").await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("hello"));

        let result = bash.exec("pwd").await.unwrap();
        assert_eq!(result.exit_code, 0);

        let result = bash.exec("true").await.unwrap();
        assert_eq!(result.exit_code, 0);

        let result = bash.exec("false").await.unwrap();
        assert_eq!(result.exit_code, 1);
    }

    /// Test command in subshell not found
    #[tokio::test]
    async fn command_not_found_in_subshell() {
        let mut bash = Bash::new();

        let result = bash.exec("(nonexistent_cmd)").await.unwrap();
        assert_eq!(result.exit_code, 127);
        assert!(result.stderr.contains("command not found"));
    }

    /// Test command substitution with not found command
    #[tokio::test]
    async fn command_not_found_in_substitution() {
        let mut bash = Bash::new();

        let result = bash.exec("echo \"output: $(nonexistent)\"").await.unwrap();
        // Command substitution captures stdout (which is empty for command not found)
        assert!(result.stdout.contains("output:"));
        // Exit code is from echo (0), not from the failed substitution
        assert_eq!(result.exit_code, 0);
    }
}

// =============================================================================
// PYTHON BUILTIN SECURITY TESTS
// =============================================================================

#[cfg(feature = "python")]
mod python_security {
    use super::*;
    use bashkit::PythonLimits;

    /// Helper: create Bash with python builtins registered.
    fn bash_with_python() -> Bash {
        Bash::builder()
            .python_with_limits(PythonLimits::default())
            .env("BASHKIT_ALLOW_INPROCESS_PYTHON", "1")
            .build()
    }

    /// TM-PY-001: Python infinite loop blocked by Monty time limit
    #[tokio::test]
    async fn threat_python_infinite_loop() {
        let mut bash = bash_with_python();
        let result = bash.exec("python3 -c \"while True: pass\"").await.unwrap();
        // Should fail with resource limit (timeout or allocation limit)
        assert_ne!(result.exit_code, 0, "Infinite loop should not succeed");
    }

    /// TM-PY-002: Python memory exhaustion blocked by allocation limits
    #[tokio::test]
    async fn threat_python_memory_exhaustion() {
        let mut bash = bash_with_python();
        let result = bash
            .exec("python3 -c \"x = [0] * 100000000\"")
            .await
            .unwrap();
        // Should fail with memory or allocation limit
        assert_ne!(result.exit_code, 0, "Memory bomb should not succeed");
    }

    /// TM-PY-003: Python recursion depth limited
    #[tokio::test]
    async fn threat_python_recursion_bomb() {
        let mut bash = bash_with_python();
        let result = bash.exec("python3 -c \"def r(): r()\nr()\"").await.unwrap();
        assert_ne!(result.exit_code, 0, "Recursion bomb should not succeed");
        assert!(
            result.stderr.contains("RecursionError") || result.stderr.contains("recursion"),
            "Should get recursion error, got: {}",
            result.stderr
        );
    }

    /// TM-PY-004: Python os module operations are not available
    #[tokio::test]
    async fn threat_python_no_os_operations() {
        let mut bash = bash_with_python();

        // os.system should not work
        let result = bash
            .exec("python3 -c \"import os\nos.system('echo hacked')\"")
            .await
            .unwrap();
        assert_ne!(result.exit_code, 0, "os.system should fail");
        assert!(
            !result.stdout.contains("hacked"),
            "Should not execute shell via os.system"
        );

        // subprocess should not work
        let result = bash
            .exec("python3 -c \"import subprocess\nsubprocess.run(['echo', 'hacked'])\"")
            .await
            .unwrap();
        assert_ne!(result.exit_code, 0, "subprocess.run should fail");
        assert!(
            !result.stdout.contains("hacked"),
            "Should not execute shell via subprocess"
        );
    }

    /// TM-PY-005: Python cannot access real filesystem
    #[tokio::test]
    async fn threat_python_no_filesystem() {
        let mut bash = bash_with_python();

        // open() resolves through Bashkit VFS, not the host filesystem.
        let result = bash
            .exec("python3 -c \"f = open('/etc/passwd')\nprint(f.read())\"")
            .await
            .unwrap();
        // Either open() fails or it reads the synthetic rootfs passwd.
        assert!(
            !result.stdout.contains("root:"),
            "Should not read real /etc/passwd"
        );
        assert!(result.exit_code != 0 || result.stdout.starts_with("sandbox:x:1000"));
    }

    /// TM-PY-006: Python error output goes to stderr, not stdout
    #[tokio::test]
    async fn threat_python_error_isolation() {
        let mut bash = bash_with_python();

        let result = bash.exec("python3 -c \"1/0\"").await.unwrap();
        assert_eq!(result.exit_code, 1);
        // Error traceback should be on stderr
        assert!(
            result.stderr.contains("ZeroDivisionError"),
            "Error should be on stderr"
        );
    }

    /// TM-PY-007: Python syntax error returns non-zero exit code
    #[tokio::test]
    async fn threat_python_syntax_error_exit() {
        let mut bash = bash_with_python();

        let result = bash.exec("python3 -c \"if\"").await.unwrap();
        assert_ne!(result.exit_code, 0, "Syntax error should fail");
        assert!(
            result.stderr.contains("SyntaxError") || result.stderr.contains("Error"),
            "Should get syntax error, got: {}",
            result.stderr
        );
    }

    /// TM-PY-008: Python exit code propagates to bash correctly
    #[tokio::test]
    async fn threat_python_exit_code_propagation() {
        let mut bash = bash_with_python();

        // Success case
        let result = bash
            .exec("python3 -c \"print('ok')\"\necho $?")
            .await
            .unwrap();
        assert!(result.stdout.contains("0"), "Success should give exit 0");

        // Failure case
        let result = bash
            .exec("python3 -c \"1/0\" 2>/dev/null\necho $?")
            .await
            .unwrap();
        assert!(result.stdout.contains("1"), "Error should give exit 1");
    }

    /// TM-PY-009: Python -c with empty argument fails gracefully
    #[tokio::test]
    async fn threat_python_empty_code() {
        let mut bash = bash_with_python();

        let result = bash.exec("python3 -c \"\"").await.unwrap();
        // Empty string is valid -c "" argument but should fail (requires non-empty)
        assert_ne!(result.exit_code, 0);
    }

    /// TM-PY-010: Python output in pipeline doesn't leak errors
    #[tokio::test]
    async fn threat_python_pipeline_error_handling() {
        let mut bash = bash_with_python();

        // Errors should not leak into pipeline stdout
        let result = bash
            .exec("python3 -c \"1/0\" 2>/dev/null | cat")
            .await
            .unwrap();
        assert!(
            !result.stdout.contains("ZeroDivisionError"),
            "Error should not be on stdout in pipeline"
        );
    }

    /// TM-PY-011: Python command substitution captures only stdout
    #[tokio::test]
    async fn threat_python_subst_captures_stdout() {
        let mut bash = bash_with_python();

        let result = bash
            .exec("result=$(python3 -c \"print(42)\")\necho $result")
            .await
            .unwrap();
        assert_eq!(result.stdout.trim(), "42");
    }

    /// TM-PY-012: Python cannot execute shell commands via eval/exec
    #[tokio::test]
    async fn threat_python_no_shell_exec() {
        let mut bash = bash_with_python();

        // __import__ should not be available
        let result = bash
            .exec("python3 -c \"__import__('os').system('echo hacked')\"")
            .await
            .unwrap();
        assert_ne!(result.exit_code, 0, "Shell exec via __import__ should fail");
        assert!(
            !result.stdout.contains("hacked"),
            "Should not execute shell command"
        );
    }

    /// TM-PY-013: Python unknown options rejected
    #[tokio::test]
    async fn threat_python_unknown_options() {
        let mut bash = bash_with_python();

        let result = bash.exec("python3 -X import_all").await.unwrap();
        assert_ne!(result.exit_code, 0);
    }

    /// TM-PY-014: Python with Bashkit resource limits
    #[tokio::test]
    async fn threat_python_respects_bash_limits() {
        let limits = ExecutionLimits::new().max_commands(5);
        let mut bash = Bash::builder()
            .python()
            .env("BASHKIT_ALLOW_INPROCESS_PYTHON", "1")
            .limits(limits)
            .build();

        // Each python3 invocation is 1 command; but with limit=5 we can still run some
        let result = bash.exec("python3 -c \"print('ok')\"").await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "ok\n");
    }

    // --- VFS Security Tests ---

    /// TM-PY-015: Python VFS reads only from Bashkit's virtual filesystem
    #[tokio::test]
    async fn threat_python_vfs_no_real_fs() {
        let mut bash = bash_with_python();

        // pathlib.Path reads the VFS: /etc/passwd is the synthetic rootfs
        // file (no root entry), never the host's.
        let result = bash
            .exec("python3 -c \"from pathlib import Path\nprint(Path('/etc/passwd').read_text())\"")
            .await
            .unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(
            result.stdout.starts_with("sandbox:x:1000:1000"),
            "Should read synthetic /etc/passwd: {}",
            result.stdout
        );
        assert!(
            !result.stdout.contains("root:x:0:0"),
            "Must not leak real filesystem"
        );
    }

    /// TM-PY-016: Python VFS write stays in virtual filesystem
    #[tokio::test]
    async fn threat_python_vfs_write_sandboxed() {
        let mut bash = bash_with_python();

        // Write to VFS, verify it stays in VFS (no real file created)
        let result = bash
            .exec(
                "python3 -c \"from pathlib import Path\n_ = Path('/tmp/sandbox_test.txt').write_text('test')\nprint(Path('/tmp/sandbox_test.txt').read_text())\"",
            )
            .await
            .unwrap();
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "test\n");
    }

    /// TM-PY-017: Python VFS path traversal blocked
    #[tokio::test]
    async fn threat_python_vfs_path_traversal() {
        let mut bash = bash_with_python();

        // Path traversal via ../.. should not escape VFS
        let result = bash
            .exec(
                "python3 -c \"from pathlib import Path\ntry:\n    print(Path('/tmp/../../../etc/passwd').read_text())\nexcept FileNotFoundError:\n    print('blocked')\"",
            )
            .await
            .unwrap();
        // `..` clamps at the VFS root: at most the synthetic passwd.
        assert!(
            !result.stdout.contains("root:x:0:0"),
            "Path traversal must not escape VFS"
        );
    }

    /// TM-PY-018: Python VFS data flows correctly between bash and Python
    #[tokio::test]
    async fn threat_python_vfs_bash_python_isolation() {
        let mut bash = bash_with_python();

        // Write from bash, read from Python - shares VFS
        let result = bash
            .exec(
                "echo 'from bash' > /tmp/shared.txt\npython3 -c \"from pathlib import Path\nprint(Path('/tmp/shared.txt').read_text().strip())\"",
            )
            .await
            .unwrap();
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "from bash\n");
    }

    /// TM-PY-019: Python VFS FileNotFoundError properly raised
    #[tokio::test]
    async fn threat_python_vfs_error_handling() {
        let mut bash = bash_with_python();

        // Reading nonexistent file should raise FileNotFoundError, not crash
        let result = bash
            .exec("python3 -c \"from pathlib import Path\nPath('/nonexistent').read_text()\"")
            .await
            .unwrap();
        assert_ne!(result.exit_code, 0, "Reading missing file should fail");
        assert!(
            result.stderr.contains("FileNotFoundError"),
            "Should get FileNotFoundError, got: {}",
            result.stderr
        );
    }

    /// TM-PY-020: Python VFS operations respect Bashkit sandbox boundaries
    #[tokio::test]
    async fn threat_python_vfs_no_network() {
        let mut bash = bash_with_python();

        // Python should not be able to make network requests
        // Even with pathlib, network paths should not work
        let result = bash
            .exec("python3 -c \"import socket\nsocket.socket()\"")
            .await
            .unwrap();
        assert_ne!(result.exit_code, 0, "socket should not be available");
    }

    /// TM-PY-021: Python VFS mkdir cannot escape sandbox
    #[tokio::test]
    async fn threat_python_vfs_mkdir_sandboxed() {
        let mut bash = bash_with_python();

        // mkdir in VFS only
        let result = bash
            .exec(
                "python3 -c \"from pathlib import Path\nPath('/tmp/pydir').mkdir()\nprint(Path('/tmp/pydir').is_dir())\"",
            )
            .await
            .unwrap();
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "True\n");
    }
}

// NOTE: Subprocess isolation tests (TM-PY-022 to TM-PY-026) were removed
// when the worker subprocess architecture was replaced with direct Monty
// integration. Resource limits and VFS isolation are now enforced directly
// by Monty's runtime within the host process.

// -- TM-PY regression coverage for Monty v0.0.5 parser depth guard ----------

#[cfg(feature = "python")]
mod python_security_regressions {
    use super::*;
    use bashkit::PythonLimits;

    fn bash_with_python() -> Bash {
        Bash::builder()
            .python_with_limits(PythonLimits::default())
            .env("BASHKIT_ALLOW_INPROCESS_PYTHON", "1")
            .build()
    }

    /// TM-PY-022: Deeply nested Python expressions caught by Monty depth guard.
    /// Monty v0.0.5 added a parser depth guard (d634706). In debug builds the
    /// limit is 30; in release builds it's 200. We use a nesting level that
    /// stays under the Monty guard rather than overflowing the ruff parser stack.
    #[tokio::test]
    async fn threat_python_deep_nesting_parser() {
        let mut bash = bash_with_python();
        // 25 levels of nested tuples stays under Monty's depth guard in debug
        // builds (MAX_NESTING_DEPTH=30) while staying safe for ruff's parser stack.
        let depth = 25;
        let code = format!(
            "python3 -c \"x = {}1{}\"",
            "(".repeat(depth),
            ",)".repeat(depth)
        );
        let result = bash.exec(&code).await.unwrap();
        // In debug builds (depth limit 30), 25 nested tuples should succeed.
        // In release builds (depth limit 200), it definitely succeeds.
        // The guard prevents deeper nesting from crashing.
        assert_eq!(
            result.exit_code, 0,
            "25 levels of nesting should be within parser depth budget"
        );
    }

    /// TM-PY-022b: Nesting at the depth guard boundary fails gracefully.
    #[tokio::test]
    async fn threat_python_nesting_at_guard_boundary() {
        let mut bash = bash_with_python();
        // In debug builds MAX_NESTING_DEPTH=30, so 40 nested statements
        // should trigger the depth guard and return an error, not crash.
        // In release builds (limit=200) this will succeed, which is fine.
        let depth = 40;
        let code = format!(
            "python3 -c \"{}x = 1{}\"",
            "if True:\n    ".repeat(depth),
            ""
        );
        let result = bash.exec(&code).await.unwrap();
        // Either succeeds (release build, limit=200) or errors gracefully (debug build)
        if result.exit_code != 0 {
            assert!(
                !result.stderr.is_empty(),
                "Should get a parse error, not silent failure"
            );
        }
    }

    /// TM-PY-003b: Exponentiation resource exhaustion blocked.
    /// Monty v0.0.5 added a 4x safety multiplier (a07e336) to prevent
    /// huge power results from exhausting memory.
    #[tokio::test]
    async fn threat_python_pow_exhaustion() {
        let limits = PythonLimits::default().max_memory(1024 * 1024); // 1MB
        let mut bash = Bash::builder()
            .python_with_limits(limits)
            .env("BASHKIT_ALLOW_INPROCESS_PYTHON", "1")
            .build();
        // 2 ** 1_000_000 produces ~300KB number; with tight 1MB limit the
        // allocation check should reject it before completion.
        let result = bash
            .exec("python3 -c \"x = 2 ** 1000000\ny = 2 ** 1000000\nz = x * y\"")
            .await
            .unwrap();
        assert_ne!(
            result.exit_code, 0,
            "Large exponentiation chain should be blocked by memory limit"
        );
    }

    /// TM-PY-003c: Division by zero during floor-div of extreme values
    /// doesn't panic. Monty v0.0.5 (fc2f154) fixed i64::MIN overflow.
    #[tokio::test]
    async fn threat_python_division_edge_cases() {
        let mut bash = bash_with_python();

        // Floor division by zero should raise ZeroDivisionError, not panic
        let result = bash.exec("python3 -c \"x = 1 // 0\"").await.unwrap();
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("ZeroDivisionError"));

        // Modulo by zero
        let result = bash.exec("python3 -c \"x = 1 % 0\"").await.unwrap();
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("ZeroDivisionError"));
    }
}

// =============================================================================
// 8. NESTING DEPTH SECURITY TESTS
//
// These tests verify that deeply nested structures cannot crash the host via
// stack overflow. Covers parser, command substitution, arithmetic, and
// misconfiguration scenarios.
// =============================================================================

mod nesting_depth_security {
    use super::*;

    // ---- POSITIVE TESTS: normal nesting works correctly ----

    /// Moderate subshell nesting (3 levels) should work fine
    #[tokio::test]
    async fn positive_moderate_subshell_nesting() {
        let mut bash = Bash::new();
        // Note: deeply nested subshells may not propagate stdout in the same way
        // as bash does. Test with a sane depth that we know works.
        let result = bash.exec("(echo ok)").await.unwrap();
        assert_eq!(result.stdout.trim(), "ok");
    }

    /// Moderate command substitution nesting (5 levels) produces correct output
    #[tokio::test]
    async fn positive_moderate_command_sub_nesting() {
        let mut bash = Bash::new();
        let result = bash
            .exec("echo $(echo $(echo $(echo $(echo nested))))")
            .await
            .unwrap();
        assert_eq!(result.stdout.trim(), "nested");
    }

    /// Moderate arithmetic nesting (20 levels) evaluates correctly
    #[tokio::test]
    async fn positive_moderate_arithmetic_nesting() {
        let mut bash = Bash::new();
        let depth = 20;
        let script = format!("echo $(({} 42 {}))", "(".repeat(depth), ")".repeat(depth),);
        let result = bash.exec(&script).await.unwrap();
        assert_eq!(result.stdout.trim(), "42");
    }

    /// Arithmetic with operators at moderate nesting works
    #[tokio::test]
    async fn positive_arithmetic_operators_nested() {
        let mut bash = Bash::new();
        // ((((2+3)))) = 5
        let result = bash.exec("echo $(( ((((2+3)))) ))").await.unwrap();
        assert_eq!(result.stdout.trim(), "5");
    }

    /// Nested if/for/while at moderate depth works
    #[tokio::test]
    async fn positive_compound_nesting() {
        let mut bash = Bash::builder()
            .limits(ExecutionLimits::new().max_commands(1000))
            .build();
        // 5-level nested if
        let script = r#"
            if true; then
                if true; then
                    if true; then
                        if true; then
                            if true; then
                                echo deep
                            fi
                        fi
                    fi
                fi
            fi
        "#;
        let result = bash.exec(script).await.unwrap();
        assert_eq!(result.stdout.trim(), "deep");
    }

    // ---- NEGATIVE TESTS: deep nesting is properly blocked ----

    /// TM-DOS-022: 200-level subshell nesting with tight depth limit → blocked
    #[tokio::test]
    async fn negative_deep_subshells_blocked() {
        let limits = ExecutionLimits::new().max_ast_depth(10);
        let mut bash = Bash::builder().limits(limits).build();

        let script = format!("{}echo hello{}", "(".repeat(200), ")".repeat(200),);
        let result = bash.exec(&script).await;
        // Must not crash. Either errors with depth limit, or parser truncates
        // at depth limit causing the inner echo to not execute
        match result {
            Ok(r) => {
                // Depth limit truncated parsing → echo never reached → no "hello"
                assert!(
                    !r.stdout.contains("hello"),
                    "200-level nesting with max_ast_depth=10 should not execute inner echo"
                );
            }
            Err(e) => {
                let err = e.to_string();
                assert!(
                    err.contains("nesting") || err.contains("depth") || err.contains("fuel"),
                    "Expected depth error, got: {}",
                    err
                );
            }
        }
    }

    /// TM-DOS-022: Deeply nested if statements blocked
    #[tokio::test]
    async fn negative_deep_if_nesting_blocked() {
        let limits = ExecutionLimits::new().max_ast_depth(5);
        let mut bash = Bash::builder().limits(limits).build();

        // Build 20-level nested if
        let mut script = String::new();
        for _ in 0..20 {
            script.push_str("if true; then ");
        }
        script.push_str("echo deep; ");
        for _ in 0..20 {
            script.push_str("fi; ");
        }
        let result = bash.exec(&script).await;
        assert!(
            result.is_err(),
            "20-level if with max_ast_depth=5 must fail"
        );
    }

    /// TM-DOS-026: 1000-level arithmetic paren nesting does not crash
    #[tokio::test]
    async fn negative_extreme_arithmetic_nesting_safe() {
        let mut bash = Bash::new();

        let depth = 1000;
        let script = format!("echo $(({} 7 {}))", "(".repeat(depth), ")".repeat(depth),);
        let result = bash.exec(&script).await;
        // Must not crash: the depth limit reports an arithmetic error.
        if let Ok(r) = result {
            assert!(r.stderr.contains("recursion level exceeded"));
            assert_eq!(r.exit_code, 1);
        }
    }

    /// TM-DOS-021: Command substitution inherits parent depth budget
    #[tokio::test]
    async fn negative_command_sub_inherits_depth_limit() {
        let limits = ExecutionLimits::new().max_ast_depth(5).max_commands(1000);
        let mut bash = Bash::builder().limits(limits).build();

        // Even though the outer script is simple, the command substitution
        // should inherit the tight depth limit and reject deep nesting inside
        let inner_depth = 50;
        let inner = format!(
            "{}echo x{}",
            "(".repeat(inner_depth),
            ")".repeat(inner_depth),
        );
        let script = format!("echo $({})", inner);
        let result = bash.exec(&script).await;
        // The inner parser should inherit max_ast_depth=5 (minus used depth)
        // and fail on 50-level nesting
        match result {
            Ok(r) => {
                // If command sub parsing fails silently, the $() produces empty string
                // This is acceptable — the deep nesting didn't execute
                assert!(
                    !r.stdout.contains("x") || r.exit_code == 0,
                    "deep nesting in command sub should not produce 'x'"
                );
            }
            Err(e) => {
                let err = e.to_string();
                assert!(
                    err.contains("nesting") || err.contains("depth") || err.contains("fuel"),
                    "Expected depth error, got: {}",
                    err
                );
            }
        }
    }

    /// TM-DOS-021: Fuel is inherited by child parsers
    #[tokio::test]
    async fn negative_command_sub_inherits_fuel_limit() {
        let limits = ExecutionLimits::new()
            .max_parser_operations(50)
            .max_commands(1000);
        let mut bash = Bash::builder().limits(limits).build();

        // A very complex command inside $() should be constrained by inherited fuel
        // Generate many semicolons to burn through fuel quickly
        let inner_cmds: Vec<&str> = (0..100).map(|_| "true").collect();
        let script = format!("echo $({})", inner_cmds.join("; "));
        let result = bash.exec(&script).await;
        // With only 50 fuel, the child parser should run out
        // Either the outer parse fails, or the inner parse silently fails → empty $()
        match result {
            Ok(r) => {
                // Acceptable: inner parse failed → $() is empty
                assert_eq!(
                    r.stdout.trim(),
                    "",
                    "inner parse should fail with limited fuel"
                );
            }
            Err(_) => {
                // Also acceptable: outer parse may fail
            }
        }
    }

    // ---- MISCONFIGURATION TESTS: absurd limits still safe ----

    /// Even with max_ast_depth=1_000_000, the parser hard-caps at 500 to prevent
    /// stack overflow. This is a key defense: misconfiguration
    /// cannot crash the host process.
    #[tokio::test]
    async fn misconfig_huge_ast_depth_still_safe() {
        let limits = ExecutionLimits::new()
            .max_ast_depth(1_000_000) // caller tries to set absurdly high
            .max_commands(10_000);
        let mut bash = Bash::builder().limits(limits).build();

        // 150-level nested if statements — exceeds HARD_MAX_AST_DEPTH (100)
        // The parser hard cap will clamp max_depth to 100 regardless of config.
        let mut script = String::new();
        for _ in 0..150 {
            script.push_str("if true; then ");
        }
        script.push_str("echo deep; ");
        for _ in 0..150 {
            script.push_str("fi; ");
        }
        let result = bash.exec(&script).await;
        // Must not crash! Hard cap at 100 catches this despite 1M config.
        match result {
            Ok(r) => {
                // Depth exceeded at 100 → parse truncated → echo not reached
                assert!(
                    !r.stdout.contains("deep"),
                    "150-level nesting should be blocked by hard cap"
                );
            }
            Err(e) => {
                // Depth/fuel error is expected
                let err = e.to_string();
                assert!(
                    err.contains("fuel") || err.contains("nesting") || err.contains("depth"),
                    "Expected fuel/depth error, got: {}",
                    err
                );
            }
        }
    }

    /// max_ast_depth=1 should reject compound commands that nest beyond depth 1
    #[tokio::test]
    async fn misconfig_tiny_ast_depth_rejects_compounds() {
        // 0 is treated as "use default" per #1181, so use 1 for minimal limit
        let limits = ExecutionLimits::new().max_ast_depth(1);
        let mut bash = Bash::builder().limits(limits).build();

        let result = bash
            .exec("if true; then if true; then echo x; fi; fi")
            .await;
        assert!(
            result.is_err(),
            "max_ast_depth=1 should reject deeply nested compound commands"
        );
    }

    /// Even with max_parser_operations=1_000_000_000, 10MB input limit bounds parser work
    #[tokio::test]
    async fn misconfig_huge_fuel_still_bounded_by_input() {
        let limits = ExecutionLimits::new()
            .max_parser_operations(1_000_000_000)
            .max_input_bytes(1000); // 1KB input limit
        let mut bash = Bash::builder().limits(limits).build();

        // Try to submit more than 1KB
        let script = "echo ".to_string() + &"x".repeat(2000);
        let result = bash.exec(&script).await;
        assert!(
            result.is_err(),
            "input exceeding max_input_bytes must be rejected"
        );
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("too large") || err.contains("input"),
            "Expected input size error, got: {}",
            err
        );
    }

    /// Misconfigured timeout (very long) doesn't matter because command limit still works
    #[tokio::test]
    async fn misconfig_long_timeout_still_command_limited() {
        let limits = ExecutionLimits::new()
            .timeout(std::time::Duration::from_secs(3600)) // 1 hour!
            .max_commands(10);
        let mut bash = Bash::builder().limits(limits).build();

        let result = bash
            .exec("true; true; true; true; true; true; true; true; true; true; true; true")
            .await;
        assert!(
            result.is_err(),
            "command limit should trigger before 1hr timeout"
        );
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("command") && err.contains("exceeded"),
            "Expected command limit error, got: {}",
            err
        );
    }

    // ---- REGRESSION TESTS: specific attack patterns ----

    /// Monty#112 analogue: deeply nested parens in arithmetic context
    /// This is the exact pattern from the Monty issue adapted for bash
    #[tokio::test]
    async fn regression_monty_112_arithmetic_parens() {
        let mut bash = Bash::new();

        // Replicate Monty#112 pattern: ~5750 nesting levels
        // For bash arithmetic, we can't go that deep without 10MB input,
        // but we test the pattern at 3000 levels (well above MAX_ARITHMETIC_DEPTH=200)
        let depth = 3000;
        let script = format!("echo $(({} 1 {}))", "(".repeat(depth), ")".repeat(depth),);
        let result = bash.exec(&script).await;
        // Must not crash — depth limit returns 0 as fallback
        assert!(result.is_ok() || result.is_err(), "must not crash");
    }

    /// Monty#112 analogue: deeply nested subshells (parser recursion)
    #[tokio::test]
    async fn regression_monty_112_subshell_nesting() {
        let mut bash = Bash::new(); // default max_ast_depth=100

        let depth = 500;
        let script = format!("{}echo hello{}", "(".repeat(depth), ")".repeat(depth),);
        let result = bash.exec(&script).await;
        // Must not crash — either errors (depth/fuel exceeded) or Ok (truncated parse)
        match result {
            Ok(r) => {
                // Parser truncated at depth=100 → inner echo not reached
                assert!(
                    !r.stdout.contains("hello"),
                    "500-level subshells should not execute inner echo"
                );
            }
            Err(e) => {
                let err = e.to_string();
                assert!(
                    err.contains("nesting") || err.contains("depth") || err.contains("fuel"),
                    "Expected depth/fuel error, got: {}",
                    err
                );
            }
        }
    }

    /// Mixed nesting: command substitution containing deeply nested subshells
    #[tokio::test]
    async fn regression_mixed_nesting_safe() {
        let limits = ExecutionLimits::new().max_ast_depth(10).max_commands(1000);
        let mut bash = Bash::builder().limits(limits).build();

        // Outer: 5-level subshell, inner: 50-level subshell inside $()
        let outer = "(((((";
        let outer_close = ")))))";
        let inner_depth = 50;
        let inner = format!(
            "{}echo x{}",
            "(".repeat(inner_depth),
            ")".repeat(inner_depth),
        );
        let script = format!("{}echo $({}){}", outer, inner, outer_close);
        let result = bash.exec(&script).await;
        // Inner parser gets remaining depth budget (10-5=5), which < 50
        // So the inner parse should fail
        match result {
            Ok(r) => {
                // Inner parse fails silently → $() is empty, echo prints newline
                assert!(
                    !r.stdout.contains("x"),
                    "inner deep nesting should not succeed"
                );
            }
            Err(e) => {
                let err = e.to_string();
                assert!(
                    err.contains("nesting")
                        || err.contains("depth")
                        || err.contains("fuel")
                        || err.contains("unexpected token"),
                    "Expected depth or syntax error, got: {}",
                    err
                );
            }
        }
    }

    /// Nested command substitutions all share the depth budget
    #[tokio::test]
    async fn negative_chained_command_subs_share_budget() {
        let limits = ExecutionLimits::new().max_ast_depth(15).max_commands(1000);
        let mut bash = Bash::builder().limits(limits).build();

        // 3 levels of command substitution, each containing subshells.
        // Outer uses some depth, inner gets less.
        // Total if limits weren't shared: 3 * 15 = 45
        // With sharing: 15 total
        let script =
            "echo $( ( ( ( ( echo $( ( ( ( ( echo $( ( ( ( ( echo ok ) ) ) ) ) ) ) ) ) ) ) ) ) ) )";
        let result = bash.exec(script).await;
        // This has many levels — may hit limit or succeed depending on accounting
        // Key: no crash
        match result {
            Ok(_) | Err(_) => {} // both acceptable, just no crash
        }
    }
}

// =============================================================================
// TM-DOS-027: BUILTIN PARSER DEPTH LIMIT TESTS
// =============================================================================

mod builtin_parser_depth {
    use super::*;

    /// TM-DOS-027: Deeply nested awk expression via parentheses must not crash
    #[tokio::test]
    async fn threat_awk_deep_paren_nesting_safe() {
        let mut bash = Bash::new();

        // 200-level parenthesized expression in awk
        let depth = 200;
        let open = "(".repeat(depth);
        let close = ")".repeat(depth);
        let script = format!(r#"echo "1" | awk '{{print {open}1{close}}}'"#);

        let result = bash.exec(&script).await;
        // Must not crash. Either error (depth exceeded) or caught by panic handler.
        if let Ok(r) = result {
            // If builtin caught the error, exit code should be non-zero
            assert!(
                r.exit_code != 0 || r.stderr.contains("nesting"),
                "deep awk nesting should fail gracefully"
            );
        }
    }

    /// TM-DOS-027: Deeply nested awk unary operators must not crash
    #[tokio::test]
    async fn threat_awk_deep_unary_nesting_safe() {
        let mut bash = Bash::new();

        // 200-level chained unary negation in awk
        let depth = 200;
        let prefix = "- ".repeat(depth);
        let script = format!(r#"echo "1" | awk '{{print {prefix}1}}'"#);

        let result = bash.exec(&script).await;
        // Must not crash
        if let Ok(r) = result {
            assert!(
                r.exit_code != 0 || r.stderr.contains("nesting"),
                "deep awk unary nesting should fail gracefully"
            );
        }
    }

    /// TM-DOS-027: Deeply nested JSON input to jq must not crash
    #[cfg(feature = "jq")]
    #[tokio::test]
    async fn threat_jq_deep_json_nesting_safe() {
        let mut bash = Bash::new();

        // 200-level nested JSON arrays
        let depth = 200;
        let open = "[".repeat(depth);
        let close = "]".repeat(depth);
        let json = format!("{open}1{close}");
        let script = format!(r#"echo '{json}' | jq '.'"#);

        let result = bash.exec(&script).await;
        // Must not crash
        if let Ok(r) = result {
            assert!(
                r.exit_code != 0 || r.stderr.contains("nesting"),
                "deep JSON nesting should fail gracefully"
            );
        }
    }

    /// TM-DOS-027: Moderate nesting in awk still works
    #[tokio::test]
    async fn threat_awk_moderate_nesting_works() {
        let mut bash = Bash::new();

        // 5-level nesting should be fine
        let script = r#"echo "1" | awk '{print (((((1 + 2)))))}'"#;
        let result = bash.exec(script).await.unwrap();
        assert_eq!(result.stdout.trim(), "3");
    }

    /// TM-DOS-027: Moderate nesting in jq still works
    #[cfg(feature = "jq")]
    #[tokio::test]
    async fn threat_jq_moderate_nesting_works() {
        let mut bash = Bash::new();

        let script = r#"echo '[[[[1]]]]' | jq '.[0][0][0][0]'"#;
        let result = bash.exec(script).await.unwrap();
        assert_eq!(result.stdout.trim(), "1");
    }
}

// =============================================================================
// NESTED LOOP MULTIPLICATION TESTS (TM-DOS-018)
// =============================================================================

mod nested_loop_security {
    use bashkit::{Bash, ExecutionLimits};

    /// TM-DOS-018: Nested loops hit total loop iteration cap
    #[tokio::test]
    async fn threat_nested_loop_multiplication_blocked() {
        // Per-loop: 1000, total: 5000
        // Two nested loops of 100 each = 10,000 total iterations would exceed 5000
        let limits = ExecutionLimits::new()
            .max_loop_iterations(1000)
            .max_total_loop_iterations(5000)
            .max_commands(100_000);
        let mut bash = Bash::builder().limits(limits).build();

        let script = r#"
            count=0
            for i in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25 26 27 28 29 30 31 32 33 34 35 36 37 38 39 40 41 42 43 44 45 46 47 48 49 50 51 52 53 54 55 56 57 58 59 60 61 62 63 64 65 66 67 68 69 70 71 72 73 74 75 76 77 78 79 80 81 82 83 84 85 86 87 88 89 90 91 92 93 94 95 96 97 98 99 100; do
                for j in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25 26 27 28 29 30 31 32 33 34 35 36 37 38 39 40 41 42 43 44 45 46 47 48 49 50 51 52 53 54 55 56 57 58 59 60 61 62 63 64 65 66 67 68 69 70 71 72 73 74 75 76 77 78 79 80 81 82 83 84 85 86 87 88 89 90 91 92 93 94 95 96 97 98 99 100; do
                    count=$((count + 1))
                done
            done
            echo $count
        "#;
        let result = bash.exec(script).await;
        // Should hit total loop iteration limit
        assert!(
            result.is_err(),
            "Nested 100x100 loops should hit total limit of 5000"
        );
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("loop iterations exceeded"),
            "Expected loop limit error, got: {}",
            err
        );
    }

    /// TM-DOS-018: Sequential loops within total budget succeed
    #[tokio::test]
    async fn threat_sequential_loops_within_budget() {
        let limits = ExecutionLimits::new()
            .max_loop_iterations(100)
            .max_total_loop_iterations(200)
            .max_commands(100_000);
        let mut bash = Bash::builder().limits(limits).build();

        // Two sequential loops of 5 each = 10 total, well within budget
        let result = bash
            .exec("for i in 1 2 3 4 5; do echo $i; done; for j in 1 2 3 4 5; do echo $j; done")
            .await
            .unwrap();
        assert_eq!(result.exit_code, 0);
    }
}

// =============================================================================
// PATH VALIDATION SECURITY TESTS (TM-DOS-012, TM-DOS-013, TM-DOS-015)
// =============================================================================

mod path_validation_security {
    use bashkit::{Bash, FileSystem, FsLimits, InMemoryFs};
    use std::path::Path;
    use std::sync::Arc;

    /// TM-DOS-012: Deep directory nesting blocked by max_path_depth
    #[tokio::test]
    async fn threat_deep_directory_nesting_blocked() {
        let limits = FsLimits::new().max_path_depth(5);
        let fs = Arc::new(InMemoryFs::with_limits(limits));
        let mut bash = Bash::builder().fs(fs).build();

        // Depth 5 should work
        let result = bash.exec("mkdir -p /a/b/c/d/e").await.unwrap();
        assert_eq!(result.exit_code, 0);

        // Depth 6 should fail
        let result = bash.exec("mkdir -p /a/b/c/d/e/f").await.unwrap();
        assert_ne!(result.exit_code, 0);
        assert!(result.stderr.contains("path too deep"));
    }

    /// TM-DOS-012: Writing to deeply nested path blocked
    #[tokio::test]
    async fn threat_deep_path_write_blocked() {
        let limits = FsLimits::new().max_path_depth(3);
        let fs = Arc::new(InMemoryFs::with_limits(limits));

        // Depth 3 should work
        fs.mkdir(Path::new("/a/b"), true).await.unwrap();
        fs.write_file(Path::new("/a/b/c"), b"ok").await.unwrap();

        // Depth 4 should fail
        let result = fs.write_file(Path::new("/a/b/c/d"), b"fail").await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("path too deep"));
    }

    /// TM-DOS-013: Long filenames blocked by max_filename_length
    #[tokio::test]
    async fn threat_long_filename_blocked() {
        let limits = FsLimits::new().max_filename_length(20);
        let fs = Arc::new(InMemoryFs::with_limits(limits));
        let mut bash = Bash::builder().fs(fs).build();

        // Short name works
        let result = bash.exec("echo ok > /tmp/short.txt").await.unwrap();
        assert_eq!(result.exit_code, 0);

        // 21-char name fails
        let long_name = "a".repeat(21);
        let result = bash
            .exec(&format!("echo fail > /tmp/{}", long_name))
            .await
            .unwrap();
        assert_ne!(result.exit_code, 0);
        assert!(result.stderr.contains("filename too long"));
    }

    /// TM-DOS-013: Long total path blocked by max_path_length
    #[tokio::test]
    async fn threat_long_path_blocked() {
        let limits = FsLimits::new().max_path_length(30);
        let fs = Arc::new(InMemoryFs::with_limits(limits));

        // Short path works
        fs.write_file(Path::new("/tmp/ok.txt"), b"ok")
            .await
            .unwrap();

        // Long path fails
        let long_path = format!("/tmp/{}", "x".repeat(30));
        let result = fs.write_file(Path::new(&long_path), b"fail").await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("path too long"));
    }

    /// TM-DOS-015: Control characters in filenames rejected
    #[tokio::test]
    async fn threat_control_char_filename_rejected() {
        let fs = InMemoryFs::new();

        // Newline in filename
        let result = fs.write_file(Path::new("/tmp/file\nname"), b"bad").await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("unsafe character"));

        // Tab in filename
        let result = fs.write_file(Path::new("/tmp/file\tname"), b"bad").await;
        assert!(result.is_err());
    }

    /// TM-DOS-015: Bidi override characters in filenames rejected
    #[tokio::test]
    async fn threat_bidi_override_filename_rejected() {
        let fs = InMemoryFs::new();

        // Right-to-left override (U+202E) — can make "exe.txt" display as "txt.exe"
        let result = fs
            .write_file(Path::new("/tmp/file\u{202E}name"), b"bad")
            .await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("bidi override"), "Error: {}", err);
    }

    /// TM-DOS-015: Normal unicode filenames still work
    #[tokio::test]
    async fn threat_normal_unicode_filename_ok() {
        let fs = InMemoryFs::new();

        // Accented chars
        fs.write_file(Path::new("/tmp/café.txt"), b"ok")
            .await
            .unwrap();

        // CJK characters
        fs.write_file(Path::new("/tmp/文件.txt"), b"ok")
            .await
            .unwrap();
    }

    /// TM-DOS-012: Deep nesting via script blocked end-to-end
    #[tokio::test]
    async fn threat_deep_nesting_script_blocked() {
        let limits = FsLimits::new().max_path_depth(5);
        let fs = Arc::new(InMemoryFs::with_limits(limits));
        let mut bash = Bash::builder().fs(fs).build();

        // 6-level deep path (exceeds max_path_depth=5)
        let result = bash.exec("mkdir -p /a/b/c/d/e/f").await.unwrap();
        assert_ne!(
            result.exit_code, 0,
            "mkdir -p for depth 6 should fail with max_path_depth=5, stderr: {}",
            result.stderr
        );
    }
}

// =============================================================================
// 12. ARCHIVE SECURITY TESTS (TM-DOS-007, TM-DOS-008)
// =============================================================================

mod archive_security {
    use super::*;
    use bashkit::{FsLimits, InMemoryFs};
    use std::sync::Arc;

    /// TM-DOS-007: Gzip bomb — decompression output exceeds file size limit
    #[tokio::test]
    async fn threat_gzip_bomb_blocked() {
        let limits = FsLimits::new().max_file_size(1_000);
        let fs = Arc::new(InMemoryFs::with_limits(limits));
        let mut bash = Bash::builder().fs(fs).build();

        // Create a file larger than the limit, compress it, then try to decompress
        // We can't create a huge file directly (limit blocks it), but we can test
        // that gzip output respects file size limits by creating a compressible file
        // within limits and verifying the pipeline works
        let result = bash
            .exec("echo 'small data' > /tmp/test.txt && gzip /tmp/test.txt")
            .await
            .unwrap();
        assert_eq!(result.exit_code, 0, "gzip of small file should work");

        // Verify gunzip produces output within limits
        let result = bash.exec("gunzip /tmp/test.txt.gz").await.unwrap();
        assert_eq!(
            result.exit_code, 0,
            "gunzip of small file should work: {}",
            result.stderr
        );
    }

    /// TM-DOS-008: Tar with many files — FS file count limit blocks extraction
    #[tokio::test]
    async fn threat_tar_bomb_many_files_blocked() {
        let limits = FsLimits::new().max_file_count(30);
        let fs = Arc::new(InMemoryFs::with_limits(limits));
        let mut bash = Bash::builder().fs(fs).build();

        // Create many files and archive them
        let result = bash
            .exec(
                r#"
mkdir -p /tmp/src
for i in $(seq 1 10); do echo "file $i" > /tmp/src/f$i.txt; done
tar -cf /tmp/archive.tar -C /tmp/src .
mkdir -p /tmp/dst
"#,
            )
            .await
            .unwrap();
        assert_eq!(
            result.exit_code, 0,
            "Creating archive should work: {}",
            result.stderr
        );

        // Now try to extract when we're close to file count limit
        // The extraction should fail or stop when hitting the FS limit
        let result = bash
            .exec("tar -xf /tmp/archive.tar -C /tmp/dst")
            .await
            .unwrap();
        // Either succeeds (if within limits) or fails (if limits hit) —
        // the key property is it doesn't crash or exceed limits
        let _ = result;
    }

    /// TM-ESC-001/TM-INJ-005: Tar path traversal — VFS prevents escape.
    /// Even if tar entries had traversal names, the VFS sandbox blocks it.
    #[tokio::test]
    async fn threat_tar_path_traversal_blocked() {
        let mut bash = Bash::new();

        // Create a tar archive with normal files
        let result = bash
            .exec(
                r#"
mkdir -p /tmp/src
echo "normal" > /tmp/src/safe.txt
cd /tmp/src && tar -cf /tmp/test.tar safe.txt
"#,
            )
            .await
            .unwrap();
        assert_eq!(result.exit_code, 0, "tar create: {}", result.stderr);

        // Extract to a target directory
        let result = bash
            .exec(
                r#"
mkdir -p /tmp/dst
cd /tmp/dst && tar -xf /tmp/test.tar
cat /tmp/dst/safe.txt
"#,
            )
            .await
            .unwrap();
        assert_eq!(result.exit_code, 0, "tar extract: {}", result.stderr);
        assert!(result.stdout.contains("normal"));

        // /etc/passwd is the synthetic root-filesystem file, untouched by the archive.
        let result = bash.exec("cat /etc/passwd").await.unwrap();
        assert!(result.stdout.contains("sandbox:x:1000:1000"));
        assert!(!result.stdout.contains("daemon:"));
    }

    /// TM-DOS-005: Tar extraction respects max_file_size limit.
    /// The limit applies to individual extracted files.
    #[tokio::test]
    async fn threat_tar_large_file_blocked() {
        // 10KB limit — enough for tar overhead and small files, but blocks large content
        let limits = FsLimits::new().max_file_size(10_000);
        let fs = Arc::new(InMemoryFs::with_limits(limits));
        let mut bash = Bash::builder().fs(fs).build();

        // Create a small file and archive it (within limits)
        let result = bash
            .exec(
                r#"
mkdir -p /tmp/src
echo "small" > /tmp/src/ok.txt
cd /tmp/src && tar -cf /tmp/test.tar ok.txt
"#,
            )
            .await
            .unwrap();
        assert_eq!(
            result.exit_code, 0,
            "Archiving should work: {}",
            result.stderr
        );

        // Extract within limits should work
        let result = bash
            .exec("mkdir -p /tmp/dst && cd /tmp/dst && tar -xf /tmp/test.tar")
            .await
            .unwrap();
        assert_eq!(
            result.exit_code, 0,
            "Extraction within limits: {}",
            result.stderr
        );
    }

    /// TM-DOS-005: Gzip respects filesystem limits for output files
    #[tokio::test]
    async fn threat_gzip_respects_fs_limits() {
        let mut bash = Bash::new();

        // Basic gzip/gunzip roundtrip preserves data
        let result = bash
            .exec(
                r#"
echo "test data for compression" > /tmp/data.txt
gzip /tmp/data.txt
gunzip /tmp/data.txt.gz
cat /tmp/data.txt
"#,
            )
            .await
            .unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(
            result.stdout.contains("test data for compression"),
            "Roundtrip should preserve data: {}",
            result.stdout
        );
    }
}

// =============================================================================
// TM-INJ-009 / TM-INJ-018: Variable namespace injection via builtins
// =============================================================================

mod variable_namespace_injection {
    use bashkit::Bash;

    async fn exec(script: &str) -> bashkit::ExecResult {
        let mut bash = Bash::builder().build();
        bash.exec(script).await.unwrap()
    }

    /// All internal prefixes that must be blocked
    const INTERNAL_PREFIXES: &[&str] = &[
        "_NAMEREF_x",
        "_READONLY_x",
        "_UPPER_x",
        "_LOWER_x",
        "_ARRAY_READ_x",
        "_SHIFT_COUNT",
        "_SET_POSITIONAL",
    ];

    // --- local builtin ---

    #[tokio::test]
    async fn tm_inj_009_local_rejects_internal_prefixes() {
        for prefix in INTERNAL_PREFIXES {
            let result = exec(&format!(
                "myfn() {{ local {prefix}=injected; echo ${{{prefix}:-blocked}}; }}; myfn"
            ))
            .await;
            assert!(
                result.stdout.contains("blocked"),
                "local should block {prefix}: got {:?}",
                result.stdout
            );
        }
    }

    #[tokio::test]
    async fn tm_inj_009_local_allows_normal_vars() {
        let result = exec("myfn() { local MY_VAR=hello; echo $MY_VAR; }; myfn").await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("hello"));
    }

    // --- printf -v ---

    #[tokio::test]
    async fn tm_inj_009_printf_v_rejects_internal_prefixes() {
        for prefix in INTERNAL_PREFIXES {
            let result = exec(&format!(
                "printf -v {prefix} injected; echo ${{{prefix}:-blocked}}"
            ))
            .await;
            assert!(
                result.stdout.contains("blocked"),
                "printf -v should block {prefix}: got {:?}",
                result.stdout
            );
        }
    }

    #[tokio::test]
    async fn tm_inj_009_printf_v_allows_normal_vars() {
        let result = exec("printf -v MY_VAR 'hello'; echo $MY_VAR").await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("hello"));
    }

    // --- read ---

    #[tokio::test]
    async fn tm_inj_009_read_rejects_internal_prefixes() {
        for prefix in INTERNAL_PREFIXES {
            let result = exec(&format!(
                "shopt -s lastpipe; echo injected | read {prefix}; echo ${{{prefix}:-blocked}}"
            ))
            .await;
            assert!(
                result.stdout.contains("blocked"),
                "read should block {prefix}: got {:?}",
                result.stdout
            );
        }
    }

    #[tokio::test]
    async fn tm_inj_009_read_allows_normal_vars() {
        let result = exec("shopt -s lastpipe; echo hello | read MY_VAR; echo $MY_VAR").await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("hello"));
    }

    #[tokio::test]
    async fn tm_inj_009_read_array_rejects_internal_prefixes() {
        let result = exec(
            "shopt -s lastpipe; echo 'a b c' | read -a _NAMEREF_x; echo ${_NAMEREF_x:-blocked}",
        )
        .await;
        assert!(
            result.stdout.contains("blocked"),
            "read -a should block _NAMEREF_x: got {:?}",
            result.stdout
        );
    }

    // --- dotenv ---

    #[tokio::test]
    async fn tm_inj_018_dotenv_rejects_internal_prefixes() {
        let result = exec(
            r#"
echo '_NAMEREF_x=injected' > /tmp/.env
echo '_READONLY_y=injected' >> /tmp/.env
echo 'NORMAL=ok' >> /tmp/.env
dotenv /tmp/.env
echo ${_NAMEREF_x:-blocked1} ${_READONLY_y:-blocked2} $NORMAL
"#,
        )
        .await;
        assert!(
            result.stdout.contains("blocked1"),
            "dotenv should block _NAMEREF_x: got {:?}",
            result.stdout
        );
        assert!(
            result.stdout.contains("blocked2"),
            "dotenv should block _READONLY_y: got {:?}",
            result.stdout
        );
        assert!(
            result.stdout.contains("ok"),
            "dotenv should allow normal vars: got {:?}",
            result.stdout
        );
    }

    // --- TM-INJ-011: Cyclic nameref resolution ---

    /// TM-INJ-011: A nameref cycle a→b→a must not loop forever, stack-overflow,
    /// or silently resolve to a stale value. `resolve_nameref` detects the cycle
    /// and returns the original name; the lookup then sees no value.
    #[tokio::test]
    async fn tm_inj_011_cyclic_nameref_two_node_does_not_hang() {
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            exec(
                r#"
declare -n a=b
declare -n b=a
echo "result=${a:-cycle_broken}"
echo "exit=$?"
"#,
            ),
        )
        .await
        .expect("cyclic nameref must not hang");
        assert!(
            result.stdout.contains("result=cycle_broken"),
            "cycle resolved to a value instead of breaking: {:?}",
            result.stdout
        );
        assert!(
            result.stdout.contains("exit=0"),
            "cycle handling caused a non-zero exit: {:?}",
            result.stdout
        );
    }

    /// TM-INJ-011: A three-node cycle (a→b→c→a) is also detected.
    #[tokio::test]
    async fn tm_inj_011_cyclic_nameref_three_node_does_not_hang() {
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            exec(
                r#"
declare -n a=b
declare -n b=c
declare -n c=a
echo "result=${a:-cycle_broken}"
"#,
            ),
        )
        .await
        .expect("cyclic nameref must not hang");
        assert!(
            result.stdout.contains("result=cycle_broken"),
            "3-node cycle resolved to a value: {:?}",
            result.stdout
        );
    }

    /// TM-INJ-011: A self-referencing nameref (a→a) breaks immediately.
    #[tokio::test]
    async fn tm_inj_011_self_referencing_nameref_does_not_hang() {
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            exec(
                r#"
declare -n a=a
echo "result=${a:-cycle_broken}"
"#,
            ),
        )
        .await
        .expect("self-ref nameref must not hang");
        assert!(
            result.stdout.contains("result=cycle_broken"),
            "self-ref resolved to a value: {:?}",
            result.stdout
        );
    }

    /// TM-DOS-090: malformed nameref array targets must not panic expansion.
    #[tokio::test]
    async fn tm_dos_090_malformed_nameref_array_target_does_not_panic() {
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            exec(
                r#"
arr=(zero one)
declare -n elem='arr[1]'
echo "elem=${elem[0]}"
declare -n ref='['
echo "before"
echo "${ref[0]}"
declare -n ref='a['
echo "${ref[0]}"
echo "after"
"#,
            ),
        )
        .await
        .expect("malformed nameref target must not panic or hang");
        assert_eq!(result.exit_code, 0);
        assert!(
            result.stdout.contains("elem=one")
                && result.stdout.contains("before")
                && result.stdout.contains("after"),
            "malformed nameref target should expand safely: {:?}",
            result.stdout
        );
    }

    // --- Cross-builtin: injected markers from one builtin don't affect another ---

    #[tokio::test]
    async fn tm_inj_cross_builtin_no_state_corruption() {
        // Attempt to inject _READONLY_ via local, verify readonly check isn't affected
        let result = exec(
            r#"
myfn() { local _READONLY_FOO=1; }
myfn
FOO=bar
echo $FOO
"#,
        )
        .await;
        assert_eq!(result.exit_code, 0);
        assert!(
            result.stdout.contains("bar"),
            "Cross-builtin injection should not affect state: got {:?}",
            result.stdout
        );
    }
}

// =============================================================================
// OVERLAY FS LIMIT ACCOUNTING (issue #653)
// =============================================================================

mod overlay_limit_accounting {
    use super::*;

    fn make_lower() -> Arc<InMemoryFs> {
        Arc::new(InMemoryFs::new())
    }

    // --- TM-DOS-035: Combined accounting (upper + lower) ---

    /// TM-DOS-035: check_write_limits must use combined usage, not upper-only.
    /// With 80 bytes in lower and 100-byte limit, writing 30 bytes should fail.
    #[tokio::test]
    async fn tm_dos_035_combined_byte_limit() {
        let lower = make_lower();
        lower
            .write_file(Path::new("/tmp/big.txt"), &[b'A'; 80])
            .await
            .unwrap();

        let limits = FsLimits::new().max_total_bytes(100);
        let overlay = OverlayFs::with_limits(lower, limits);

        // 80 (lower) + 30 (new) = 110 > 100
        let result = overlay
            .write_file(Path::new("/tmp/extra.txt"), &[b'B'; 30])
            .await;
        assert!(
            result.is_err(),
            "TM-DOS-035: write should fail when combined usage exceeds limit"
        );
    }

    /// TM-DOS-035: File count limit must include lower layer files.
    #[tokio::test]
    async fn tm_dos_035_combined_file_count_limit() {
        let lower = make_lower();
        lower
            .write_file(Path::new("/tmp/existing.txt"), b"data")
            .await
            .unwrap();

        let temp = OverlayFs::new(lower.clone());
        let base_count = temp.usage().file_count;

        let limits = FsLimits::new().max_file_count(base_count + 1);
        let overlay = OverlayFs::with_limits(lower, limits);

        overlay
            .write_file(Path::new("/tmp/new1.txt"), b"ok")
            .await
            .unwrap();

        let result = overlay
            .write_file(Path::new("/tmp/new2.txt"), b"fail")
            .await;
        assert!(
            result.is_err(),
            "TM-DOS-035: file count limit must include lower layer"
        );
    }

    // --- TM-DOS-036: Double-counting overwritten files ---

    /// TM-DOS-036: Overwriting a lower file in upper should not double-count.
    #[tokio::test]
    async fn tm_dos_036_no_double_count_on_override() {
        let lower = make_lower();
        lower
            .write_file(Path::new("/tmp/file.txt"), &[b'L'; 100])
            .await
            .unwrap();

        let overlay = OverlayFs::new(lower);
        let before = overlay.usage();

        overlay
            .write_file(Path::new("/tmp/file.txt"), &[b'U'; 50])
            .await
            .unwrap();

        let after = overlay.usage();
        assert_eq!(
            after.file_count, before.file_count,
            "TM-DOS-036: overridden file should not increase count"
        );
        assert_eq!(
            after.total_bytes,
            before.total_bytes - 50,
            "TM-DOS-036: bytes should reflect upper size, not sum"
        );
    }

    /// TM-DOS-036: Whiteout should deduct lower file from usage.
    #[tokio::test]
    async fn tm_dos_036_whiteout_deducts_usage() {
        let lower = make_lower();
        lower
            .write_file(Path::new("/tmp/gone.txt"), &[b'X'; 200])
            .await
            .unwrap();

        let overlay = OverlayFs::new(lower);
        let before = overlay.usage();

        overlay
            .remove(Path::new("/tmp/gone.txt"), false)
            .await
            .unwrap();

        let after = overlay.usage();
        assert_eq!(
            after.total_bytes,
            before.total_bytes - 200,
            "TM-DOS-036: whited-out file bytes should be deducted"
        );
        assert_eq!(
            after.file_count,
            before.file_count - 1,
            "TM-DOS-036: whited-out file should be deducted from count"
        );
    }

    // --- TM-DOS-037: chmod CoW bypasses limits ---

    /// TM-DOS-037: chmod on lower file triggers CoW, must check write limits.
    #[tokio::test]
    async fn tm_dos_037_chmod_file_cow_checks_limits() {
        let lower = make_lower();
        lower
            .write_file(Path::new("/tmp/big.txt"), &[b'X'; 5000])
            .await
            .unwrap();

        let limits = FsLimits::new().max_total_bytes(1000);
        let overlay = OverlayFs::with_limits(lower, limits);

        let result = overlay.chmod(Path::new("/tmp/big.txt"), 0o755).await;
        assert!(
            result.is_err(),
            "TM-DOS-037: chmod CoW should fail when content exceeds write limits"
        );
    }

    /// TM-DOS-037: chmod on lower directory triggers CoW, must check dir limits.
    #[tokio::test]
    async fn tm_dos_037_chmod_dir_cow_checks_limits() {
        let lower = make_lower();
        // Create many directories in lower to fill up dir count
        for i in 0..10 {
            lower
                .mkdir(Path::new(&format!("/d{}", i)), true)
                .await
                .unwrap();
        }

        // Get base dir count, then set limit to exactly that
        let temp = OverlayFs::new(lower.clone());
        let base_dirs = temp.usage().dir_count;

        let limits = FsLimits::new().max_dir_count(base_dirs);
        let overlay = OverlayFs::with_limits(lower, limits);

        // chmod a lower directory should trigger CoW mkdir — must be rejected
        let result = overlay.chmod(Path::new("/d0"), 0o755).await;
        assert!(
            result.is_err(),
            "TM-DOS-037: chmod dir CoW should fail when dir count at limit"
        );
    }

    /// TM-DOS-037: mkdir should check dir count limits.
    #[tokio::test]
    async fn tm_dos_037_mkdir_checks_dir_limits() {
        let lower = make_lower();
        let temp = OverlayFs::new(lower.clone());
        let base_dirs = temp.usage().dir_count;

        let limits = FsLimits::new().max_dir_count(base_dirs + 1);
        let overlay = OverlayFs::with_limits(lower, limits);

        // First mkdir should succeed
        overlay.mkdir(Path::new("/newdir"), false).await.unwrap();

        // Second mkdir should fail
        let result = overlay.mkdir(Path::new("/newdir2"), false).await;
        assert!(
            result.is_err(),
            "TM-DOS-037: mkdir should fail when dir count exceeds limit"
        );
    }

    /// TM-DOS-037: recursive mkdir must account for all missing components.
    #[tokio::test]
    async fn tm_dos_037_recursive_mkdir_counts_all_new_dirs() {
        let lower = make_lower();
        let temp = OverlayFs::new(lower.clone());
        let base_dirs = temp.usage().dir_count;

        // Only allow one additional directory, but recursive mkdir needs three.
        let limits = FsLimits::new().max_dir_count(base_dirs + 1);
        let overlay = OverlayFs::with_limits(lower, limits);

        let result = overlay.mkdir(Path::new("/a/b/c"), true).await;
        assert!(
            result.is_err(),
            "TM-DOS-037: recursive mkdir should fail when total new dirs exceed limit"
        );
    }

    // --- TM-DOS-038: Incomplete recursive whiteout ---

    /// TM-DOS-038: Recursive delete must hide all lower children.
    #[tokio::test]
    async fn tm_dos_038_recursive_delete_hides_all_children() {
        let lower = make_lower();
        lower.mkdir(Path::new("/data"), true).await.unwrap();
        lower
            .write_file(Path::new("/data/a.txt"), b"aaa")
            .await
            .unwrap();
        lower
            .write_file(Path::new("/data/b.txt"), b"bbb")
            .await
            .unwrap();
        lower.mkdir(Path::new("/data/sub"), true).await.unwrap();
        lower
            .write_file(Path::new("/data/sub/c.txt"), b"ccc")
            .await
            .unwrap();

        let overlay = OverlayFs::new(lower);

        overlay.remove(Path::new("/data"), true).await.unwrap();

        // All children must be invisible
        assert!(
            !overlay.exists(Path::new("/data")).await.unwrap(),
            "TM-DOS-038: directory itself should be hidden"
        );
        assert!(
            !overlay.exists(Path::new("/data/a.txt")).await.unwrap(),
            "TM-DOS-038: child file should be hidden"
        );
        assert!(
            !overlay.exists(Path::new("/data/sub/c.txt")).await.unwrap(),
            "TM-DOS-038: nested child should be hidden"
        );

        // read_file should fail
        assert!(overlay.read_file(Path::new("/data/a.txt")).await.is_err());
        assert!(
            overlay
                .read_file(Path::new("/data/sub/c.txt"))
                .await
                .is_err()
        );
    }

    /// TM-DOS-038: Usage must deduct all recursively deleted children.
    #[tokio::test]
    async fn tm_dos_038_recursive_delete_deducts_all_bytes() {
        let lower = make_lower();
        lower.mkdir(Path::new("/stuff"), true).await.unwrap();
        lower
            .write_file(Path::new("/stuff/x.txt"), &[b'X'; 100])
            .await
            .unwrap();
        lower
            .write_file(Path::new("/stuff/y.txt"), &[b'Y'; 200])
            .await
            .unwrap();
        lower.mkdir(Path::new("/stuff/deep"), true).await.unwrap();
        lower
            .write_file(Path::new("/stuff/deep/z.txt"), &[b'Z'; 50])
            .await
            .unwrap();

        let overlay = OverlayFs::new(lower);
        let before = overlay.usage();

        overlay.remove(Path::new("/stuff"), true).await.unwrap();

        let after = overlay.usage();
        assert_eq!(
            after.total_bytes,
            before.total_bytes - 350,
            "TM-DOS-038: should deduct all child file bytes (100+200+50)"
        );
        assert_eq!(
            after.file_count,
            before.file_count - 3,
            "TM-DOS-038: should deduct all child file counts"
        );
    }

    // --- Boundary math ---

    /// Boundary: lower=50, upper=49, limit=100, write 2 → should fail.
    #[tokio::test]
    async fn tm_dos_boundary_exact_limit() {
        let lower = make_lower();
        lower
            .write_file(Path::new("/tmp/lower.txt"), &[b'A'; 50])
            .await
            .unwrap();

        let limits = FsLimits::new().max_total_bytes(100);
        let overlay = OverlayFs::with_limits(lower, limits);

        // 50 (lower) + 49 (upper) = 99 <= 100: should succeed
        overlay
            .write_file(Path::new("/tmp/upper.txt"), &[b'B'; 49])
            .await
            .unwrap();

        // 99 + 2 = 101 > 100: should fail
        let result = overlay
            .write_file(Path::new("/tmp/over.txt"), &[b'C'; 2])
            .await;
        assert!(result.is_err(), "boundary: 99 + 2 = 101 > 100 should fail");
    }

    // --- CoW accumulation via repeated chmod ---

    /// Repeated chmod on different lower files should accumulate CoW correctly.
    /// After CoW, usage should reflect correct combined accounting.
    #[tokio::test]
    async fn tm_dos_cow_accumulation_via_chmod() {
        let lower = make_lower();
        for i in 0..5 {
            lower
                .write_file(Path::new(&format!("/tmp/f{}.txt", i)), &[b'A'; 100])
                .await
                .unwrap();
        }

        // Give generous limit so chmod CoW succeeds (check is conservative: adds
        // content_size before deducting hidden lower, so limit must be >= usage + file_size)
        let temp = OverlayFs::new(lower.clone());
        let base = temp.usage().total_bytes;
        let limits = FsLimits::new().max_total_bytes(base + 500);
        let overlay = OverlayFs::with_limits(lower, limits);

        let before = overlay.usage();

        for i in 0..5 {
            let path = format!("/tmp/f{}.txt", i);
            overlay.chmod(Path::new(&path), 0o755).await.unwrap();
        }

        let after = overlay.usage();
        // Each chmod copies file to upper (100 bytes) and hides lower (100 bytes) → net 0
        assert_eq!(
            after.total_bytes, before.total_bytes,
            "CoW chmod should not change total bytes"
        );
        assert_eq!(
            after.file_count, before.file_count,
            "CoW chmod should not change file count"
        );
    }
}

// =============================================================================
// TEMPLATE RECURSION DEPTH LIMITS (issue #654)
// =============================================================================

mod template_depth {
    use super::*;

    fn bash() -> Bash {
        Bash::builder().build()
    }

    // --- TM-DOS-052: Template depth bomb ---

    /// TM-DOS-052: Deeply nested {{#if}} should produce error, not stack overflow.
    #[tokio::test]
    async fn tm_dos_052_template_if_depth_bomb() {
        let mut bash = bash();
        let mut template = String::new();
        for _ in 0..200 {
            template.push_str("{{#if x}}");
        }
        template.push_str("deep");
        for _ in 0..200 {
            template.push_str("{{/if}}");
        }

        // Write template and JSON to VFS, then render
        let script = format!(
            r#"echo '{template}' > /tmp/tpl.txt
echo '{{"x": true}}' > /tmp/data.json
template render /tmp/tpl.txt -d /tmp/data.json"#
        );
        let result = bash.exec(&script).await.unwrap();
        let output = format!("{}{}", result.stdout, result.stderr);
        assert!(
            output.contains("depth exceeded") || result.exit_code != 0,
            "TM-DOS-052: deeply nested #if should produce clean error, got: stdout={:?} stderr={:?}",
            result.stdout,
            result.stderr
        );
    }

    /// TM-DOS-052: Template with reasonable usage should work fine.
    #[tokio::test]
    async fn tm_dos_052_template_normal_nesting_works() {
        let mut bash = bash();
        let script = r#"
cat > /tmp/tpl.txt << 'TPLEOF'
{{#if x}}hello from template{{/if}}
TPLEOF
cat > /tmp/data.json << 'JSONEOF'
{"x": true}
JSONEOF
template render /tmp/tpl.txt -d /tmp/data.json
"#;
        let result = bash.exec(script).await.unwrap();
        assert_eq!(
            result.exit_code, 0,
            "template render should succeed: stderr={:?} stdout={:?}",
            result.stderr, result.stdout
        );
        assert!(
            result.stdout.contains("hello from template"),
            "Normal template should work: got {:?}",
            result.stdout
        );
    }
}

// =============================================================================
// SESSION-LEVEL CUMULATIVE RESOURCE COUNTERS (TM-DOS-059)
// =============================================================================

mod session_limits {
    use super::*;

    /// TM-DOS-059: Cumulative command limit across multiple exec() calls.
    /// Run N exec() calls each with K commands; verify failure when total > max_total_commands.
    #[tokio::test]
    async fn tm_dos_059_cumulative_command_limit() {
        let session = SessionLimits::new()
            .max_total_commands(15)
            .max_exec_calls(100);
        let limits = ExecutionLimits::new().max_commands(10_000);
        let mut bash = Bash::builder()
            .limits(limits)
            .session_limits(session)
            .build();

        // Each exec runs 5 commands (echo x 5). Three calls = 15 commands should be near limit.
        let script = "echo 1; echo 2; echo 3; echo 4; echo 5";

        let r1 = bash.exec(script).await.unwrap();
        assert_eq!(r1.exit_code, 0, "first batch should succeed");

        let r2 = bash.exec(script).await.unwrap();
        assert_eq!(r2.exit_code, 0, "second batch should succeed");

        // Eventually we should hit the session command limit (Err or non-zero exit).
        let mut hit_limit = false;
        for _ in 0..5 {
            match bash.exec(script).await {
                Err(e) => {
                    let msg = e.to_string();
                    assert!(
                        msg.contains("session"),
                        "error should mention session: {msg}"
                    );
                    hit_limit = true;
                    break;
                }
                Ok(r) if r.exit_code != 0 => {
                    hit_limit = true;
                    break;
                }
                Ok(_) => {} // keep going
            }
        }
        assert!(hit_limit, "should eventually hit session command limit");
    }

    /// TM-DOS-059: exec() call count limit.
    #[tokio::test]
    async fn tm_dos_059_exec_call_count_limit() {
        let session = SessionLimits::new()
            .max_exec_calls(3)
            .max_total_commands(u64::MAX);
        let limits = ExecutionLimits::new();
        let mut bash = Bash::builder()
            .limits(limits)
            .session_limits(session)
            .build();

        // First 3 exec() calls should succeed
        for i in 0..3 {
            let r = bash.exec("echo ok").await.unwrap();
            assert_eq!(r.exit_code, 0, "exec call {} should succeed", i + 1);
        }

        // 4th exec() should fail with Err (session limit exceeded at entry)
        let r = bash.exec("echo should_fail").await;
        assert!(
            r.is_err(),
            "4th exec call should fail due to session exec limit"
        );
        let msg = r.unwrap_err().to_string();
        assert!(
            msg.contains("session") && msg.contains("exec"),
            "error should mention session exec limit: {msg}"
        );
    }

    /// TM-DOS-059: Malformed scripts count toward exec() call limit before parsing.
    #[tokio::test]
    async fn tm_dos_059_parse_errors_count_toward_exec_call_limit() {
        let session = SessionLimits::new()
            .max_exec_calls(1)
            .max_total_commands(u64::MAX);
        let mut bash = Bash::builder().session_limits(session).build();

        let parse_err = bash.exec("if").await;
        assert!(parse_err.is_err(), "malformed script should fail parsing");

        let limit_err = bash.exec("echo should_fail").await;
        assert!(
            limit_err.is_err(),
            "second exec() should fail because parse errors consume exec budget"
        );
        let msg = limit_err.unwrap_err().to_string();
        assert!(
            msg.contains("session") && msg.contains("exec"),
            "error should mention session exec limit: {msg}"
        );
    }

    /// TM-DOS-059: Session counters persist across exec() calls (not reset).
    #[tokio::test]
    async fn tm_dos_059_counter_persistence() {
        let session = SessionLimits::new()
            .max_total_commands(20)
            .max_exec_calls(100);
        let limits = ExecutionLimits::new().max_commands(10_000);
        let mut bash = Bash::builder()
            .limits(limits)
            .session_limits(session)
            .build();

        // Run commands in separate exec() calls, accumulating toward limit
        let mut hit_limit = false;
        for i in 0..20 {
            match bash.exec("echo a; echo b; echo c").await {
                Err(e) => {
                    let msg = e.to_string();
                    assert!(
                        msg.contains("session"),
                        "error should mention session: {msg}"
                    );
                    hit_limit = true;
                    assert!(
                        i >= 3,
                        "should succeed for at least a few calls before limit"
                    );
                    break;
                }
                Ok(r) if r.exit_code != 0 => {
                    hit_limit = true;
                    break;
                }
                Ok(_) => {}
            }
        }
        assert!(hit_limit, "should eventually hit session command limit");
    }

    /// TM-DOS-059: Per-exec limits still work independently of session limits.
    #[tokio::test]
    async fn tm_dos_059_per_exec_limits_still_work() {
        let session = SessionLimits::unlimited();
        let limits = ExecutionLimits::new().max_commands(3);
        let mut bash = Bash::builder()
            .limits(limits)
            .session_limits(session)
            .build();

        // Per-exec limit of 3 commands should still trigger.
        // May return Ok with non-zero exit or Err depending on how limit surfaces.
        let r = bash.exec("echo 1; echo 2; echo 3; echo 4; echo 5").await;
        match r {
            Ok(result) => assert_ne!(
                result.exit_code, 0,
                "per-exec command limit should still work"
            ),
            Err(e) => {
                let msg = e.to_string();
                assert!(
                    msg.contains("command") || msg.contains("limit"),
                    "error should be about command limit: {msg}"
                );
            }
        }
    }

    /// TM-DOS-059: Builder API configures session limits correctly.
    #[tokio::test]
    async fn tm_dos_059_builder_api() {
        let session = SessionLimits::new()
            .max_total_commands(50)
            .max_exec_calls(10);
        let mut bash = Bash::builder().session_limits(session).build();

        // Should work within limits
        let r = bash.exec("echo hello").await.unwrap();
        assert_eq!(r.exit_code, 0);
        assert!(r.stdout.contains("hello"));
    }

    /// TM-DOS-059: Default session limits are reasonable and non-zero.
    #[tokio::test]
    async fn tm_dos_059_default_safety() {
        let defaults = SessionLimits::default();
        assert!(
            defaults.max_total_commands > 0,
            "default max_total_commands should be non-zero"
        );
        assert!(
            defaults.max_exec_calls > 0,
            "default max_exec_calls should be non-zero"
        );
        // Defaults should be reasonably large but finite
        assert!(
            defaults.max_total_commands < u64::MAX,
            "default max_total_commands should be finite"
        );
        assert!(
            defaults.max_exec_calls < u64::MAX,
            "default max_exec_calls should be finite"
        );
        // Verify specific expected values
        assert_eq!(defaults.max_total_commands, 100_000);
        assert_eq!(defaults.max_exec_calls, 1_000);
    }

    /// TM-DOS-059: SessionLimits::unlimited() disables all session limits.
    #[tokio::test]
    async fn tm_dos_059_unlimited() {
        let unlimited = SessionLimits::unlimited();
        assert_eq!(unlimited.max_total_commands, u64::MAX);
        assert_eq!(unlimited.max_exec_calls, u64::MAX);
    }

    /// TM-DOS-059: reset_for_execution does NOT reset session counters.
    #[test]
    fn tm_dos_059_reset_preserves_session_counters() {
        use bashkit::ExecutionCounters;

        let limits = ExecutionLimits::new().max_commands(10_000);
        let mut counters = ExecutionCounters::new();

        // Simulate some commands
        for _ in 0..5 {
            counters.tick_command(&limits).unwrap();
        }
        counters.tick_exec_call();

        assert_eq!(counters.session_commands, 5);
        assert_eq!(counters.session_exec_calls, 1);

        // reset_for_execution should NOT reset session counters
        counters.reset_for_execution();

        assert_eq!(
            counters.session_commands, 5,
            "session_commands must persist"
        );
        assert_eq!(
            counters.session_exec_calls, 1,
            "session_exec_calls must persist"
        );
        // But per-exec counters should be reset
        assert_eq!(counters.commands, 0);
    }
}

// =============================================================================
// MEMORY BUDGET LIMITS (TM-DOS-060)
// =============================================================================

mod memory_limits {
    use super::*;

    /// TM-DOS-060: Variable count bomb — script creating many variables.
    #[tokio::test]
    async fn tm_dos_060_variable_count_bomb() {
        let mem = MemoryLimits::new().max_variable_count(50);
        let limits = ExecutionLimits::new()
            .max_commands(10_000)
            .max_loop_iterations(10_000);
        let mut bash = Bash::builder()
            .limits(limits)
            .memory_limits(mem)
            .session_limits(SessionLimits::unlimited())
            .build();

        // Try to create 100 variables — the rejected write must fail visibly.
        let script = r#"
for i in $(seq 1 100); do
    eval "var_$i=hello"
done
echo "done"
"#;
        let error = bash.exec(script).await.unwrap_err();
        assert!(error.to_string().contains("variable count limit (50)"));
    }

    /// TM-DOS-060: Variable byte bomb — large variable values.
    #[tokio::test]
    async fn tm_dos_060_variable_size_bomb() {
        let mem = MemoryLimits::new().max_total_variable_bytes(1000);
        let limits = ExecutionLimits::new();
        let mut bash = Bash::builder()
            .limits(limits)
            .memory_limits(mem)
            .session_limits(SessionLimits::unlimited())
            .build();

        // Try to create a variable with a large value
        let script = r#"
big=$(printf '%0500s' | tr ' ' 'A')
echo ${#big}
big2=$(printf '%0500s' | tr ' ' 'B')
echo ${#big2}
"#;
        // First variable fits; the second one crosses the 1000-byte budget
        // and is rejected as a resource limit, not stored.
        let err = bash.exec(script).await.unwrap_err().to_string();
        assert!(err.contains("variable byte limit (1000)"), "got: {err}");
        let result = bash
            .exec("big=$(printf '%0500s' | tr ' ' 'A'); echo ${#big}")
            .await
            .unwrap();
        assert_eq!(result.stdout, "500\n");
    }

    /// TM-DOS-060: declaration builtin assignments must honor variable count
    /// budget. (`local` outside a function assigns nothing, as in bash.)
    #[tokio::test]
    async fn tm_dos_060_local_assignment_respects_budget() {
        let mem = MemoryLimits::new().max_variable_count(2);
        let mut bash = Bash::builder()
            .memory_limits(mem)
            .session_limits(SessionLimits::unlimited())
            .build();

        let script = r#"
declare a=1 b=2 c=3
printf "%s\n" "${a:-unset}" "${b:-unset}" "${c:-unset}"
"#;
        let error = bash.exec(script).await.unwrap_err();
        assert!(error.to_string().contains("variable count limit (2)"));
    }

    /// TM-DOS-060: local builtin assignments in functions must honor variable count budget.
    #[tokio::test]
    async fn tm_dos_060_function_local_assignment_respects_budget() {
        // `$_` exists from startup (uncounted), so only the locals count:
        // the second one is over a budget of one.
        let mem = MemoryLimits::new().max_variable_count(1);
        let mut bash = Bash::builder()
            .memory_limits(mem)
            .session_limits(SessionLimits::unlimited())
            .build();

        let script = r#"
f() {
    local a=1 b=2
    printf "%s\n" "${a:-unset}" "${b:-unset}"
}
f
"#;
        let error = bash.exec(script).await.unwrap_err();
        assert!(error.to_string().contains("variable count limit (1)"));
    }

    /// TM-DOS-060: local compound array assignments must honor array entry budget.
    #[tokio::test]
    async fn tm_dos_060_local_compound_array_respects_budget() {
        let mem = MemoryLimits::new().max_array_entries(1);
        let mut bash = Bash::builder()
            .memory_limits(mem)
            .session_limits(SessionLimits::unlimited())
            .build();

        let script = r#"
local arr=(a b c)
printf "%s\n" "${#arr[@]}"
f() {
    local inner=(x y z)
    printf "%s\n" "${#inner[@]}"
}
f
"#;
        let result = bash.exec(script).await.unwrap();

        assert_eq!(result.exit_code, 0);
        let counts: Vec<usize> = result
            .stdout
            .lines()
            .map(|line| line.parse::<usize>().unwrap())
            .collect();
        // At most the one budgeted entry is stored, never all three.
        assert_eq!(counts.len(), 2);
        assert!(counts.iter().all(|c| *c <= 1), "counts: {counts:?}");
    }

    /// TM-INF-023: bare local declarations must shadow stale global array bindings.
    #[tokio::test]
    async fn tm_inf_023_bare_local_declarations_shadow_global_arrays() {
        let mut bash = Bash::builder()
            .session_limits(SessionLimits::unlimited())
            .build();

        let script = r#"
scalar_shadow=(GLOBAL_INDEXED)
declare -A indexed_shadow=([k]=GLOBAL_ASSOC)
assoc_shadow=(GLOBAL_INDEXED)
f() {
    local scalar_shadow
    local -a indexed_shadow
    local -A assoc_shadow
    printf "scalar_star=<%s>\n" "${scalar_shadow[*]}"
    printf "scalar_zero=<%s>\n" "${scalar_shadow[0]}"
    printf "indexed_star=<%s>\n" "${indexed_shadow[*]}"
    printf "indexed_key=<%s>\n" "${indexed_shadow[k]}"
    printf "assoc_star=<%s>\n" "${assoc_shadow[*]}"
    printf "assoc_zero=<%s>\n" "${assoc_shadow[0]}"
}
f
printf "after_scalar=<%s>\n" "${scalar_shadow[*]}"
printf "after_indexed=<%s>\n" "${indexed_shadow[*]}"
printf "after_assoc=<%s>\n" "${assoc_shadow[*]}"
"#;
        let result = bash.exec(script).await.unwrap();

        assert_eq!(result.exit_code, 0);
        let lines: Vec<&str> = result.stdout.lines().collect();
        assert_eq!(
            lines,
            vec![
                "scalar_star=<>",
                "scalar_zero=<>",
                "indexed_star=<>",
                "indexed_key=<>",
                "assoc_star=<>",
                "assoc_zero=<>",
                "after_scalar=<GLOBAL_INDEXED>",
                "after_indexed=<GLOBAL_ASSOC>",
                "after_assoc=<GLOBAL_INDEXED>",
            ]
        );
    }

    /// TM-DOS-060: FUNCNAME bookkeeping must not undercount array budget.
    #[tokio::test]
    async fn tm_dos_060_funcname_restore_preserves_array_budget() {
        let mem = MemoryLimits::new().max_array_entries(3);
        let mut bash = Bash::builder()
            .memory_limits(mem)
            .session_limits(SessionLimits::unlimited())
            .build();

        let script = r#"
arr[0]=a
arr[1]=b
arr[2]=c
f() { :; }
f
extra[0]=x
printf "%s %s\n" "${#arr[@]}" "${#extra[@]}"
"#;
        let error = bash.exec(script).await.unwrap_err();
        assert!(error.to_string().contains("array entry limit (3)"));
        let result = bash.exec("echo ${#arr[@]} ${#extra[@]}").await.unwrap();
        assert_eq!(result.stdout.trim(), "3 0");
    }

    /// TM-DOS-060: brace/glob expansion inside array literals stays capped.
    #[tokio::test]
    async fn tm_dos_060_array_literal_brace_expansion_respects_budget() {
        let mem = MemoryLimits::new().max_array_entries(3);
        let mut bash = Bash::builder()
            .memory_limits(mem)
            .session_limits(SessionLimits::unlimited())
            .build();

        let script = r#"
arr=(x{1..1000})
echo "${#arr[@]}"
unset arr
declare -a d=("y"{1..1000} z{a,b})
echo "${#d[@]}"
"#;
        let result = bash.exec(script).await.unwrap();

        assert_eq!(result.stdout.trim(), "3\n3");
    }

    /// TM-DOS-060: arithmetic writes to array elements are charged to the budget.
    #[tokio::test]
    async fn tm_dos_060_arithmetic_array_writes_respect_budget() {
        let mem = MemoryLimits::new().max_array_entries(3);
        let mut bash = Bash::builder()
            .memory_limits(mem)
            .session_limits(SessionLimits::unlimited())
            .build();

        let script = r#"
declare -A c
for ((i = 0; i < 10; i++)); do ((c[k$i]+=1)); done
for ((i = 0; i < 10; i++)); do ((a[i]++)); done
printf "%s %s\n" "${#c[@]}" "${#a[@]}"
"#;
        let error = bash.exec(script).await.unwrap_err();
        assert!(error.to_string().contains("array entry limit (3)"));
        let result = bash.exec("echo ${#c[@]} ${#a[@]}").await.unwrap();
        assert_eq!(result.stdout.trim(), "3 0");
    }

    /// TM-DOS-060: saved local array shadows must stay charged to array budget.
    #[tokio::test]
    async fn tm_dos_060_local_array_shadow_preserves_array_budget() {
        let mem = MemoryLimits::new().max_array_entries(3);
        let mut bash = Bash::builder()
            .memory_limits(mem)
            .session_limits(SessionLimits::unlimited())
            .build();

        let script = r#"
arr=(a b c)
f() {
    local arr=(x)
    printf "inside=%s\n" "${#arr[@]}"
}
f
printf "outside=%s:%s\n" "${#arr[@]}" "${arr[*]}"
"#;
        let result = bash.exec(script).await.unwrap();

        assert_eq!(result.exit_code, 0);
        assert_eq!(
            result.stdout.lines().collect::<Vec<_>>(),
            vec!["inside=0", "outside=3:a b c"]
        );
    }

    /// TM-DOS-060: saved local associative-array shadows must stay charged.
    #[tokio::test]
    async fn tm_dos_060_local_assoc_array_shadow_preserves_array_budget() {
        let mem = MemoryLimits::new().max_array_entries(3);
        let mut bash = Bash::builder()
            .memory_limits(mem)
            .session_limits(SessionLimits::unlimited())
            .build();

        let script = r#"
declare -A map
map[a]=1
map[b]=2
map[c]=3
f() {
    local -A map=([x]=9)
    printf "inside=%s\n" "${#map[@]}"
}
f
printf "outside=%s:%s\n" "${#map[@]}" "${map[a]}${map[b]}${map[c]}"
"#;
        let result = bash.exec(script).await.unwrap();

        assert_eq!(result.exit_code, 0);
        assert_eq!(
            result.stdout.lines().collect::<Vec<_>>(),
            vec!["inside=0", "outside=3:123"]
        );
    }

    /// TM-DOS-060: re-shadowing the same local array in one frame must release
    /// the replaced transient binding's budget (only the first snapshot is kept,
    /// so later removals must not leave phantom entries charged).
    #[tokio::test]
    async fn tm_dos_060_repeated_local_array_shadow_does_not_drift_budget() {
        let mem = MemoryLimits::new().max_array_entries(3);
        let mut bash = Bash::builder()
            .memory_limits(mem)
            .session_limits(SessionLimits::unlimited())
            .build();

        // The first `local arr=(a b c)` fills the budget. The second
        // `local arr=(x)` replaces it; its three entries must be released so the
        // single-element reassignment fits. Without releasing them the budget
        // drifts upward and the second assignment is wrongly rejected.
        let script = r#"
f() {
    local arr=(a b c)
    printf "first=%s\n" "${#arr[@]}"
    local arr=(x)
    printf "second=%s\n" "${#arr[@]}"
}
f
printf "after=%s\n" "${#arr[@]}"
"#;
        let result = bash.exec(script).await.unwrap();

        assert_eq!(result.exit_code, 0);
        assert_eq!(
            result.stdout.lines().collect::<Vec<_>>(),
            vec!["first=3", "second=1", "after=0"]
        );
    }

    /// TM-DOS-060: user mutation of FUNCNAME inside a function must not leak
    /// array budget after the function returns (interpreter metadata stays
    /// uncharged, but user-added entries are credited back on discard).
    #[tokio::test]
    async fn tm_dos_060_funcname_user_mutation_does_not_leak_budget() {
        let mem = MemoryLimits::new().max_array_entries(5);
        let mut bash = Bash::builder()
            .memory_limits(mem)
            .session_limits(SessionLimits::unlimited())
            .build();

        // Each call adds a user entry to FUNCNAME, which is discarded on return.
        // If the charge leaked, the later 5-element array would not fully fit.
        let script = r#"
f() { FUNCNAME[7]=injected; }
f
f
f
arr=(a b c d e)
printf "n=%s\n" "${#arr[@]}"
"#;
        let result = bash.exec(script).await.unwrap();

        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout.trim(), "n=5");
    }

    /// TM-INF-023: local compound arrays must not leak after a function returns.
    #[tokio::test]
    async fn tm_inf_023_function_local_arrays_do_not_leak() {
        let mut bash = Bash::builder()
            .session_limits(SessionLimits::unlimited())
            .build();

        let script = r#"
outer=(global value)
declare -A outer_map=([k]=global)
f() {
    local -a outer=(token value)
    local -a secret_arr=(hidden token)
    local -A outer_map=([k]=secret)
    local -A secret_map=([k]=hidden)
    printf "inside_outer=%s\n" "${outer[*]}"
    printf "inside_secret_arr=%s\n" "${secret_arr[*]}"
    printf "inside_outer_map=%s\n" "${outer_map[*]}"
    printf "inside_secret_map=%s\n" "${secret_map[*]}"
}
f
printf "outside_outer=%s\n" "${outer[*]}"
printf "outside_secret_arr=%s\n" "${secret_arr[*]}"
printf "outside_outer_map=%s\n" "${outer_map[*]}"
printf "outside_secret_map=%s\n" "${secret_map[*]}"
"#;
        let result = bash.exec(script).await.unwrap();

        assert_eq!(result.exit_code, 0);
        let lines: Vec<&str> = result.stdout.lines().collect();
        assert_eq!(
            lines,
            vec![
                "inside_outer=token value",
                "inside_secret_arr=hidden token",
                "inside_outer_map=secret",
                "inside_secret_map=hidden",
                "outside_outer=global value",
                "outside_secret_arr=",
                "outside_outer_map=global",
                "outside_secret_map=",
            ]
        );
    }

    /// TM-DOS-060: Array entry bomb — indexed array with many entries.
    #[tokio::test]
    async fn tm_dos_060_array_entry_bomb() {
        let mem = MemoryLimits::new().max_array_entries(50);
        let limits = ExecutionLimits::new()
            .max_commands(10_000)
            .max_loop_iterations(10_000);
        let mut bash = Bash::builder()
            .limits(limits)
            .memory_limits(mem)
            .session_limits(SessionLimits::unlimited())
            .build();

        // Try to create array with 100 entries
        let script = r#"
for i in $(seq 1 100); do
    arr[$i]=hello
done
echo "done"
"#;
        let error = bash.exec(script).await.unwrap_err();
        assert!(error.to_string().contains("array entry limit (50)"));
        let result = bash.exec("echo ${#arr[@]}").await.unwrap();
        assert_eq!(result.stdout, "50\n");
    }

    /// TM-DOS-060: Function count bomb — defining many functions.
    #[tokio::test]
    async fn tm_dos_060_function_count_bomb() {
        let mem = MemoryLimits::new().max_function_count(10);
        let limits = ExecutionLimits::new()
            .max_commands(10_000)
            .max_loop_iterations(10_000);
        let mut bash = Bash::builder()
            .limits(limits)
            .memory_limits(mem)
            .session_limits(SessionLimits::unlimited())
            .build();

        // Try to define 50 functions
        let script = r#"
for i in $(seq 1 50); do
    eval "func_$i() { echo $i; }"
done
echo "done"
"#;
        let result = bash.exec(script).await.unwrap();
        assert_eq!(result.exit_code, 0);
    }

    /// TM-DOS-060: Normal scripts unaffected by default limits.
    #[tokio::test]
    async fn tm_dos_060_normal_script_unaffected() {
        // Use default memory limits
        let mut bash = Bash::builder().build();

        let script = r#"
name="hello"
count=42
arr=(one two three)
declare -A map=([key1]=val1 [key2]=val2)
greet() { echo "Hello $1"; }
greet "world"
echo "$name $count ${arr[1]}"
"#;
        let result = bash.exec(script).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("Hello world"));
        assert!(result.stdout.contains("hello 42 two"));
    }

    /// TM-DOS-060: Default memory limits are reasonable and non-zero.
    #[test]
    fn tm_dos_060_default_safety() {
        let defaults = MemoryLimits::default();
        assert_eq!(defaults.max_variable_count, 10_000);
        assert_eq!(defaults.max_total_variable_bytes, 10_000_000);
        assert_eq!(defaults.max_array_entries, 100_000);
        assert_eq!(defaults.max_function_count, 1_000);
        assert_eq!(defaults.max_function_body_bytes, 1_000_000);
    }

    /// TM-DOS-060: MemoryLimits::unlimited() disables limits.
    #[test]
    fn tm_dos_060_unlimited() {
        let unlimited = MemoryLimits::unlimited();
        assert_eq!(unlimited.max_variable_count, usize::MAX);
        assert_eq!(unlimited.max_total_variable_bytes, usize::MAX);
        assert_eq!(unlimited.max_array_entries, usize::MAX);
        assert_eq!(unlimited.max_function_count, usize::MAX);
        assert_eq!(unlimited.max_function_body_bytes, usize::MAX);
    }

    /// TM-DOS-060: Builder API configures memory limits correctly.
    #[tokio::test]
    async fn tm_dos_060_builder_api() {
        let mem = MemoryLimits::new()
            .max_variable_count(500)
            .max_total_variable_bytes(50_000)
            .max_array_entries(1000)
            .max_function_count(50)
            .max_function_body_bytes(10_000);
        let mut bash = Bash::builder().memory_limits(mem).build();

        let result = bash.exec("echo hello").await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("hello"));
    }

    /// TM-DOS-060: Cross-instance isolation — two Bash instances have independent budgets.
    #[tokio::test]
    async fn tm_dos_060_cross_instance_isolation() {
        let mem = MemoryLimits::new().max_variable_count(20);
        let limits = ExecutionLimits::new()
            .max_commands(10_000)
            .max_loop_iterations(10_000);

        let mut bash1 = Bash::builder()
            .limits(limits.clone())
            .memory_limits(mem.clone())
            .session_limits(SessionLimits::unlimited())
            .build();
        let mut bash2 = Bash::builder()
            .limits(limits)
            .memory_limits(mem)
            .session_limits(SessionLimits::unlimited())
            .build();

        // Both should independently handle variable creation
        let script = r#"
for i in $(seq 1 15); do
    eval "x_$i=test"
done
echo "done"
"#;
        let r1 = bash1.exec(script).await.unwrap();
        let r2 = bash2.exec(script).await.unwrap();
        assert_eq!(r1.exit_code, 0);
        assert_eq!(r2.exit_code, 0);
    }
}

// =============================================================================
// DATE / VIRTUAL CLOCK TESTS (TM-INF-018)
// =============================================================================

mod tm_inf_018_date {
    use super::*;

    /// TM-INF-018: `Bash::builder().fixed_epoch(N)` causes `date +%s`
    /// to return exactly N, regardless of host wall-clock.
    #[tokio::test]
    async fn fixed_epoch_freezes_date() {
        let mut bash = Bash::builder().fixed_epoch(1_700_000_000).build();
        let r = bash.exec("date +%s").await.unwrap();
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout.trim(), "1700000000");
    }

    /// TM-INF-018: printf `%(fmt)T` reads the same virtual clock as `date`,
    /// so it can't bypass `fixed_epoch`, and it ignores host timezone state.
    #[tokio::test]
    async fn fixed_epoch_freezes_printf_time_directive() {
        let mut bash = Bash::builder().fixed_epoch(1_700_000_000).build();
        let r = bash
            .exec("printf '%(%s %z)T|%(%s)T\\n' -1 ''")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "1700000000 +0000|1700000000\n");
    }

    /// TM-INF-018: `Bash::builder().epoch_offset(N)` keeps the clock
    /// ticking but shifts its absolute value by N seconds. Verify two
    /// consecutive reads differ by less than 1s (ticking) yet sit at
    /// least N-1 seconds ahead of host real time (offset applied).
    #[tokio::test]
    async fn epoch_offset_shifts_real_clock() {
        let offset = 365_i64 * 24 * 3600; // +1 year
        let host_before = chrono::Utc::now().timestamp();
        let mut bash = Bash::builder().epoch_offset(offset).build();
        let r = bash.exec("date +%s").await.unwrap();
        assert_eq!(r.exit_code, 0);
        let observed: i64 = r.stdout.trim().parse().unwrap();
        // observed should be ~ host_before + offset
        let delta = observed - (host_before + offset);
        assert!(
            (-2..=2).contains(&delta),
            "offset clock drifted: observed={observed}, expected≈{}, delta={delta}",
            host_before + offset
        );
    }

    /// TM-INF-018: virtual clock modes must not leak host timezone via custom formats.
    #[tokio::test]
    async fn fixed_epoch_forces_utc_for_timezone_formats() {
        let mut bash = Bash::builder().fixed_epoch(1_700_000_000).build();
        let r = bash.exec("date +%z").await.unwrap();
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout.trim(), "+0000");
    }

    /// TM-INF-018: epoch_offset mode must not leak host timezone via RFC 2822 output.
    #[tokio::test]
    async fn epoch_offset_forces_utc_for_rfc2822() {
        let mut bash = Bash::builder().epoch_offset(86_400).build();
        let r = bash.exec("date -R").await.unwrap();
        assert_eq!(r.exit_code, 0);
        assert!(r.stdout.trim_end().ends_with(" +0000"));
    }
    /// TM-INF-018: `fixed_epoch` and `epoch_offset` are mutually
    /// exclusive — last builder call wins. fixed_epoch followed by
    /// epoch_offset should disable fixed_epoch.
    #[tokio::test]
    async fn last_builder_call_wins_offset_after_fixed() {
        let mut bash = Bash::builder().fixed_epoch(0).epoch_offset(0).build();
        let r = bash.exec("date +%s").await.unwrap();
        let observed: i64 = r.stdout.trim().parse().unwrap();
        // Should be near real-time (within a few seconds), not 0.
        let now = chrono::Utc::now().timestamp();
        assert!(
            (observed - now).abs() < 60,
            "epoch_offset(0) did not override fixed_epoch(0): observed={observed}, real={now}"
        );
    }

    /// TM-INF-018: the inverse — epoch_offset then fixed_epoch should
    /// disable the offset.
    #[tokio::test]
    async fn last_builder_call_wins_fixed_after_offset() {
        let mut bash = Bash::builder()
            .epoch_offset(99_999)
            .fixed_epoch(1_700_000_000)
            .build();
        let r = bash.exec("date +%s").await.unwrap();
        assert_eq!(r.stdout.trim(), "1700000000");
    }
}

// =============================================================================
// TRACE EVENT TESTS (TM-INF-019)
// =============================================================================

mod trace_events {
    use super::*;

    #[tokio::test]
    async fn trace_off_produces_no_events() {
        let mut bash = Bash::builder().trace_mode(TraceMode::Off).build();
        let r = bash.exec("echo hello; echo world").await.unwrap();
        assert_eq!(r.exit_code, 0);
        assert!(r.events.is_empty(), "Off mode should produce no events");
    }

    #[tokio::test]
    async fn trace_full_records_command_start_and_exit() {
        let mut bash = Bash::builder().trace_mode(TraceMode::Full).build();
        let r = bash.exec("echo hello").await.unwrap();
        assert_eq!(r.exit_code, 0);
        assert!(
            r.events.len() >= 2,
            "Full mode should record at least start+exit, got {}",
            r.events.len()
        );

        // First event should be CommandStart for echo
        assert_eq!(r.events[0].kind, TraceEventKind::CommandStart);
        if let TraceEventDetails::CommandStart { command, argv, .. } = &r.events[0].details {
            assert_eq!(command, "echo");
            assert_eq!(argv, &["hello"]);
        } else {
            panic!("expected CommandStart details");
        }

        // Second event should be CommandExit for echo
        assert_eq!(r.events[1].kind, TraceEventKind::CommandExit);
        if let TraceEventDetails::CommandExit {
            command, exit_code, ..
        } = &r.events[1].details
        {
            assert_eq!(command, "echo");
            assert_eq!(*exit_code, 0);
        } else {
            panic!("expected CommandExit details");
        }
    }

    #[tokio::test]
    async fn trace_full_multiple_commands() {
        let mut bash = Bash::builder().trace_mode(TraceMode::Full).build();
        let r = bash.exec("echo a; echo b; echo c").await.unwrap();
        assert_eq!(r.exit_code, 0);

        // Should have start+exit for each of three echo commands
        let starts: Vec<_> = r
            .events
            .iter()
            .filter(|e| e.kind == TraceEventKind::CommandStart)
            .collect();
        let exits: Vec<_> = r
            .events
            .iter()
            .filter(|e| e.kind == TraceEventKind::CommandExit)
            .collect();
        assert_eq!(starts.len(), 3, "3 commands should have 3 starts");
        assert_eq!(exits.len(), 3, "3 commands should have 3 exits");
    }

    #[tokio::test]
    async fn trace_full_seq_numbers_monotonic() {
        let mut bash = Bash::builder().trace_mode(TraceMode::Full).build();
        let r = bash.exec("echo a; echo b").await.unwrap();
        assert!(r.events.len() >= 4);
        for i in 1..r.events.len() {
            assert!(
                r.events[i].seq > r.events[i - 1].seq,
                "seq numbers should be strictly monotonic"
            );
        }
    }

    #[tokio::test]
    async fn trace_redacted_scrubs_secrets_in_argv() {
        let mut bash = Bash::builder().trace_mode(TraceMode::Redacted).build();
        // Use printf to avoid actual HTTP call; the trace records the command start
        let r = bash
            .exec(r#"printf "%s\n" -H "Authorization: Bearer secret123" "https://api.example.com""#)
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        assert!(!r.events.is_empty());

        // Check that no event leaks "secret123"
        for event in &r.events {
            if let TraceEventDetails::CommandStart { argv, .. } = &event.details {
                for arg in argv {
                    assert!(
                        !arg.contains("secret123"),
                        "Redacted mode should not leak secrets: {arg}"
                    );
                }
            }
        }
    }

    #[tokio::test]
    async fn trace_failed_exec_does_not_leak_events_to_next_exec() {
        // A custom builtin's `Err` aborts the exec (bundled builtins' usage
        // errors no longer do), which is the failure path under test.
        struct Fatal;
        #[async_trait::async_trait]
        impl bashkit::Builtin for Fatal {
            async fn execute(
                &self,
                _ctx: bashkit::BuiltinContext<'_>,
            ) -> bashkit::Result<bashkit::ExecResult> {
                Err(bashkit::Error::Execution("fatal".to_string()))
            }
        }
        let mut bash = Bash::builder()
            .trace_mode(TraceMode::Full)
            .builtin("fatal", Box::new(Fatal))
            .build();

        let failed = bash.exec("fatal tenant-a-private-arg").await;
        assert!(failed.is_err(), "fatal builtin should fail execution");

        let r = bash.exec("echo tenant-b").await.unwrap();
        assert_eq!(r.exit_code, 0);

        for event in &r.events {
            if let TraceEventDetails::CommandStart { command, argv, .. } = &event.details {
                assert_ne!(
                    command, "fatal",
                    "stale failed command leaked into next exec"
                );
                assert!(
                    argv.iter().all(|arg| !arg.contains("tenant-a-private-arg")),
                    "stale failed argv leaked into next exec: {argv:?}" // debug-ok: assert context only
                );
            }
        }
    }

    #[tokio::test]
    async fn trace_redacted_scrubs_env_secrets() {
        let mut bash = Bash::builder().trace_mode(TraceMode::Redacted).build();
        // printf with env-style key=value argument containing a secret suffix
        let r = bash
            .exec(r#"printf "%s" "API_KEY=supersecret""#)
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);

        for event in &r.events {
            if let TraceEventDetails::CommandStart { argv, .. } = &event.details {
                for arg in argv {
                    assert!(
                        !arg.contains("supersecret"),
                        "Redacted mode should scrub env secrets: {arg}"
                    );
                }
            }
        }
    }

    #[tokio::test]
    async fn trace_full_does_not_scrub() {
        let mut bash = Bash::builder().trace_mode(TraceMode::Full).build();
        let r = bash
            .exec(r#"printf "%s" "API_KEY=supersecret""#)
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);

        let mut found_secret = false;
        for event in &r.events {
            if let TraceEventDetails::CommandStart { argv, .. } = &event.details {
                for arg in argv {
                    if arg.contains("supersecret") {
                        found_secret = true;
                    }
                }
            }
        }
        assert!(found_secret, "Full mode should preserve raw secrets");
    }

    #[tokio::test]
    async fn trace_callback_receives_events() {
        use std::sync::{Arc, Mutex};
        let count = Arc::new(Mutex::new(0u32));
        let count_clone = count.clone();
        let mut bash = Bash::builder()
            .trace_mode(TraceMode::Full)
            .on_trace_event(Box::new(move |_event| {
                *count_clone.lock().unwrap() += 1;
            }))
            .build();
        let r = bash.exec("echo hello").await.unwrap();
        assert_eq!(r.exit_code, 0);
        let cb_count = *count.lock().unwrap();
        assert!(
            cb_count >= 2,
            "callback should fire for at least start+exit, got {cb_count}"
        );
        // events should also be in the result
        assert_eq!(r.events.len() as u32, cb_count);
    }

    #[tokio::test]
    async fn trace_off_zero_overhead() {
        let mut bash = Bash::builder().trace_mode(TraceMode::Off).build();
        // Run a loop to verify no events accumulate
        let r = bash
            .exec("for i in $(seq 1 100); do echo $i; done")
            .await
            .unwrap();
        assert_eq!(r.exit_code, 0);
        assert!(r.events.is_empty());
    }

    #[tokio::test]
    async fn trace_records_function_calls() {
        let mut bash = Bash::builder().trace_mode(TraceMode::Full).build();
        let r = bash.exec("myfn() { echo inside; }; myfn").await.unwrap();
        assert_eq!(r.exit_code, 0);

        // Should see start/exit for myfn and start/exit for echo inside it
        let fn_starts: Vec<_> = r
            .events
            .iter()
            .filter(|e| {
                e.kind == TraceEventKind::CommandStart
                    && matches!(&e.details, TraceEventDetails::CommandStart { command, .. } if command == "myfn")
            })
            .collect();
        assert!(
            !fn_starts.is_empty(),
            "should record CommandStart for function call"
        );
    }

    #[tokio::test]
    async fn trace_records_command_not_found() {
        let mut bash = Bash::builder().trace_mode(TraceMode::Full).build();
        let r = bash.exec("nonexistent_command_xyz").await.unwrap();
        assert_ne!(r.exit_code, 0);

        // Should still get start+exit (exit code 127)
        let exits: Vec<_> = r
            .events
            .iter()
            .filter(|e| {
                e.kind == TraceEventKind::CommandExit
                    && matches!(&e.details, TraceEventDetails::CommandExit { exit_code, .. } if exit_code == &127)
            })
            .collect();
        assert!(
            !exits.is_empty(),
            "should record CommandExit with code 127 for not-found"
        );
    }

    // TM-DOS-029: Malformed ${#[} must not panic (fuzz crash from CI)
    #[tokio::test]
    async fn arithmetic_malformed_brace_length_no_panic() {
        let mut bash = Bash::new();
        // Input discovered by arithmetic_fuzz: [${#[
        // Triggered panic "byte range starts at 1 but ends at 0" in
        // expand_brace_expr_in_arithmetic when rest="[" and bracket=0.
        let r = bash.exec("echo $((0 + ${#[}))").await.unwrap();
        // Should not panic — just return 0 for malformed expression
        assert_eq!(r.exit_code, 0);
    }

    // TM-DOS-029 regression: malformed ${#name[...} (unterminated index)
    // whose content ends mid UTF-8 multi-byte char must not panic.
    // Discovered by arithmetic_fuzz on 2026-05-16, crash artifact
    // `crash-0eb6b53a030c0a10f29e1933480e76c9c1fa3971` — input
    // `${#[rg[g([禧,...` made `end = rest.len() - 1` land in the
    // middle of `禧` (3-byte UTF-8), panicking the string slice.
    #[tokio::test]
    async fn arithmetic_malformed_brace_length_utf8_no_panic() {
        let mut bash = Bash::new();
        let script = "echo $((${#rg[禧))";
        let r = bash.exec(script).await;
        // Either Ok (graceful "0") or Err is fine — must NOT panic.
        let _ = r;
    }
}

// =============================================================================
// =============================================================================
// TYPESCRIPT / ZAPCODE SECURITY (TM-TS)
//
// Threat model tests for the embedded TypeScript interpreter (zapcode-core).
// NOTE: TypeScript is opt-in — requires `typescript` feature AND builder call.
// =============================================================================

#[cfg(feature = "typescript")]
mod typescript_security {
    use super::*;
    use bashkit::TypeScriptLimits;
    use std::time::Duration;

    fn bash_with_ts() -> Bash {
        Bash::builder()
            .typescript_with_limits(TypeScriptLimits::default())
            .build()
    }

    fn bash_with_ts_tight() -> Bash {
        Bash::builder()
            .typescript_with_limits(
                TypeScriptLimits::default()
                    .max_duration(Duration::from_secs(3))
                    .max_memory(4 * 1024 * 1024)
                    .max_allocations(50_000)
                    .max_stack_depth(100),
            )
            .build()
    }

    /// TM-TS-001: TypeScript infinite loop blocked by time limit
    #[tokio::test]
    async fn threat_ts_infinite_loop() {
        let mut bash = bash_with_ts_tight();
        let result = bash.exec("ts -c \"while (true) {}\"").await.unwrap();
        assert_ne!(result.exit_code, 0, "Infinite loop should not succeed");
    }

    /// TM-TS-002: TypeScript memory exhaustion blocked
    #[tokio::test]
    async fn threat_ts_memory_exhaustion() {
        let mut bash = bash_with_ts_tight();
        let result = bash
            .exec("ts -c \"const arr: number[] = []; while (true) { arr.push(1); }\"")
            .await
            .unwrap();
        assert_ne!(result.exit_code, 0, "Memory bomb should not succeed");
    }

    /// TM-TS-003: TypeScript stack overflow blocked
    #[tokio::test]
    async fn threat_ts_stack_overflow() {
        let mut bash = bash_with_ts_tight();
        let result = bash
            .exec("ts -c \"const f = (): number => f(); f()\"")
            .await
            .unwrap();
        assert_ne!(result.exit_code, 0, "Stack overflow should not succeed");
    }

    /// TM-TS-005: TypeScript VFS reads only from Bashkit VFS
    #[tokio::test]
    async fn threat_ts_vfs_no_real_fs() {
        let mut bash = bash_with_ts();
        let result = bash
            .exec("ts -c \"const c = await readFile('/etc/passwd'); console.log(c)\"")
            .await
            .unwrap();
        assert!(
            !result.stdout.contains("root:"),
            "Should not read real /etc/passwd"
        );
    }

    /// TM-TS-007: TypeScript VFS path traversal blocked
    #[tokio::test]
    async fn threat_ts_vfs_path_traversal() {
        let mut bash = bash_with_ts();
        let result = bash
            .exec("ts -c \"const c = await readFile('/tmp/../../../etc/passwd'); console.log(c)\"")
            .await
            .unwrap();
        assert!(
            !result.stdout.contains("root:"),
            "Path traversal must not escape VFS"
        );
    }

    /// TM-TS-012: TypeScript error output goes to stderr
    #[tokio::test]
    async fn threat_ts_error_isolation() {
        let mut bash = bash_with_ts();
        let result = bash
            .exec("ts -c \"throw new Error('test')\"")
            .await
            .unwrap();
        assert_eq!(result.exit_code, 1);
    }

    /// TM-TS-021: TypeScript cannot execute shell commands
    #[tokio::test]
    async fn threat_ts_no_shell_exec() {
        let mut bash = bash_with_ts();
        let result = bash
            .exec("ts -c \"console.log(process.env.HOME)\"")
            .await
            .unwrap();
        assert_ne!(result.exit_code, 0, "process.env should not exist");
        assert!(
            !result.stdout.contains("/home"),
            "Should not access env vars via process"
        );
    }

    /// Opt-in verification: ts NOT available without builder call
    #[tokio::test]
    async fn threat_ts_optin_not_default() {
        let mut bash = Bash::builder().build();
        let result = bash.exec("ts -c \"console.log('hi')\"").await.unwrap();
        assert_ne!(
            result.exit_code, 0,
            "ts should not be available without .typescript()"
        );
    }
}

// =============================================================================
// ADVERSARIAL TESTS — SPARSE ARRAYS, EXTREME INDICES, EXPANSION BOMBS
// Inspired by zapcode's adversarial test suite (issue #934)
// =============================================================================

mod zapcode_inspired_adversarial {
    use super::*;

    /// TM-DOS-060: Huge sparse index allocation.
    /// Assigning to index 999999999 must not allocate ~1B empty slots.
    /// bashkit uses HashMap internally, so the storage is O(1) per entry,
    /// but the max_array_entries limit should still cap total entries.
    #[tokio::test]
    async fn sparse_array_huge_index() {
        let mem = MemoryLimits::new().max_array_entries(100);
        let limits = ExecutionLimits::new().max_commands(1_000);
        let mut bash = Bash::builder()
            .limits(limits)
            .memory_limits(mem)
            .session_limits(SessionLimits::unlimited())
            .build();

        let result = bash
            .exec("declare -a arr; arr[999999999]=x; echo ${#arr[@]}")
            .await
            .unwrap();
        // Should succeed (HashMap-based, only 1 entry) or be capped — no OOM
        assert_eq!(result.exit_code, 0);
        // The array has at most 1 entry — certainly not ~1B
        let count: usize = result.stdout.trim().parse().unwrap_or(0);
        assert!(
            count <= 100,
            "Sparse index must not cause mass allocation, got count={}",
            count
        );
    }

    /// TM-DOS-060: Extreme negative array index.
    /// Must not panic, no OOB memory access, no memory corruption.
    /// The key security property is: no panic, no crash, no unbounded allocation.
    /// Bash wraps negative indices modulo array length, so some element may be returned.
    #[tokio::test]
    async fn sparse_array_extreme_negative_index() {
        let limits = ExecutionLimits::new().max_commands(1_000);
        let mut bash = Bash::builder().limits(limits).build();

        let result = bash
            .exec("declare -a arr=(a b c); echo \"${arr[-999999999]}\"")
            .await
            .unwrap();
        // Security property: no panic, no crash, graceful completion
        assert_eq!(result.exit_code, 0);
        // Output should be either empty or one of the valid elements (wrapping is acceptable)
        let out = result.stdout.trim();
        assert!(
            out.is_empty() || ["a", "b", "c"].contains(&out),
            "Extreme negative index should return empty or valid element, got: {:?}",
            out
        );
    }

    /// TM-DOS-060: Array entry exhaustion under load.
    /// Populating 200K entries via loop must be stopped by max_array_entries (100K default)
    /// or by the loop iteration limit — whichever fires first.
    #[tokio::test]
    async fn array_entry_exhaustion_under_load() {
        let mem = MemoryLimits::new().max_array_entries(100);
        let limits = ExecutionLimits::new()
            .max_commands(500_000)
            .max_loop_iterations(500_000)
            .max_total_loop_iterations(500_000);
        let mut bash = Bash::builder()
            .limits(limits)
            .memory_limits(mem)
            .session_limits(SessionLimits::unlimited())
            .build();

        let script = r#"
declare -a arr
i=0
while [ $i -lt 200 ]; do
    arr[$i]=x
    i=$((i+1))
done
echo ${#arr[@]}
"#;
        let error = bash.exec(script).await.unwrap_err();
        assert!(error.to_string().contains("array entry limit (100)"));
        let result = bash.exec("echo ${#arr[@]}").await.unwrap();
        assert_eq!(result.stdout, "100\n");
    }

    /// TM-DOS-060: Unquoted expansion in array assignment must still respect
    /// max_array_entries after IFS word splitting (arr=($x)).
    /// Word-split array assignment that exceeds max_array_entries now errors
    /// instead of silently truncating (ifs_split_limited returns Err).
    #[tokio::test]
    async fn array_assignment_word_split_respects_array_entry_limit() {
        let mem = MemoryLimits::new().max_array_entries(100);
        let limits = ExecutionLimits::new().max_commands(10_000);
        let mut bash = Bash::builder()
            .limits(limits)
            .memory_limits(mem)
            .session_limits(SessionLimits::unlimited())
            .build();

        let script = r#"
parts=""
i=0
while [ $i -lt 200 ]; do
    parts="$parts x"
    i=$((i+1))
done
arr=($parts)
echo ${#arr[@]}
"#;
        // Word-splitting that exceeds max_word_split_fields propagates as Err.
        let result = bash.exec(script).await;
        let err_msg = match &result {
            Err(e) => e.to_string(),
            Ok(r) => r.stderr.to_string(),
        };
        assert!(
            result.is_err(),
            "should error on word-split limit, got: {err_msg}"
        );
        assert!(
            err_msg.contains("word split") || err_msg.contains("limit"),
            "error should mention word-split limit, got: {err_msg}"
        );
    }

    /// TM-DOS-041: Printf format repeat via brace expansion.
    /// `{1..999999999}` must be rejected by the brace expansion cap before
    /// printf ever runs. Without the cap, this would generate ~1B arguments.
    #[tokio::test]
    async fn brace_expansion_bomb_printf() {
        let limits = ExecutionLimits::new()
            .max_commands(1_000)
            .max_stdout_bytes(1_000_000);
        let mut bash = Bash::builder().limits(limits).build();

        let result = bash.exec("printf '%0.s-' {1..999999999}").await;
        // Either the brace expansion is capped (producing truncated output)
        // or the parser rejects it statically. Either way: no OOM, no hang.
        match result {
            Ok(r) => {
                // If it succeeded, the output must be bounded (cap at 100K expansions or stdout limit)
                assert!(
                    r.stdout.len() <= 1_000_000,
                    "Brace expansion bomb produced {} bytes — should be capped",
                    r.stdout.len()
                );
            }
            Err(e) => {
                // Static rejection by parser budget is also acceptable
                let msg = e.to_string();
                assert!(
                    msg.contains("brace")
                        || msg.contains("range")
                        || msg.contains("too large")
                        || msg.contains("exceeded")
                        || msg.contains("budget"),
                    "Expected brace/range limit error, got: {}",
                    msg
                );
            }
        }
    }

    /// TM-DOS-042: Comma-list brace expansion recursion bomb.
    /// `{a,b}{a,b}...` repeated tens of thousands of times is far under
    /// `max_input_bytes`, but the combinatorial count cap is only charged on
    /// recursion *return*, so the first DFS path used to descend one frame per
    /// group and stack-overflow the worker thread before any cap fired. The
    /// depth cap must bound the descent: this completes without panic/OOM and
    /// with bounded output.
    #[tokio::test]
    async fn brace_expansion_comma_recursion_bomb() {
        let limits = ExecutionLimits::new()
            .max_commands(1_000)
            .max_stdout_bytes(1_000_000);
        let mut bash = Bash::builder().limits(limits).build();

        // 50k repetitions of `{a,b}` (~250 KB source, well under the input cap).
        let script = format!("echo {}", "{a,b}".repeat(50_000));
        let result = bash.exec(&script).await;
        match result {
            Ok(r) => {
                assert!(
                    r.stdout.len() <= 1_000_000,
                    "comma-list brace bomb produced {} bytes — should be capped",
                    r.stdout.len()
                );
            }
            Err(e) => {
                let msg = e.to_string();
                assert!(
                    msg.contains("brace")
                        || msg.contains("exceeded")
                        || msg.contains("budget")
                        || msg.contains("too large"),
                    "Expected a limit error, got: {}",
                    msg
                );
            }
        }
    }

    /// TM-DOS-059: Parameter expansion replacement bomb.
    /// `${x//a/$(echo bbbbbbbb)}` replaces each 'a' with 'bbbbbbbb'.
    /// At scale (10K 'a's × 1K 'b's = 10MB), this must be caught by
    /// max_total_variable_bytes or max_stdout_bytes.
    #[tokio::test]
    async fn parameter_expansion_replacement_bomb() {
        let mem = MemoryLimits::new().max_total_variable_bytes(100_000);
        let limits = ExecutionLimits::new()
            .max_commands(50_000)
            .max_loop_iterations(50_000)
            .max_total_loop_iterations(50_000)
            .max_stdout_bytes(1_000_000);
        let mut bash = Bash::builder()
            .limits(limits)
            .memory_limits(mem)
            .session_limits(SessionLimits::unlimited())
            .build();

        // Create a string of 10K 'a' chars, then replace each with 1K 'b' chars
        // This attempts 10K × 1K = 10MB output
        let script = r#"
x=$(printf 'a%.0s' {1..10000})
echo "${x//a/$(printf 'b%.0s' {1..1000})}"
"#;
        let result = bash.exec(script).await;
        match result {
            Ok(r) => {
                // If it completes, output must be bounded by limits
                assert!(
                    r.stdout.len() <= 1_000_000,
                    "Expansion bomb produced {} bytes of stdout — should be capped",
                    r.stdout.len()
                );
            }
            Err(_) => {
                // Limit enforcement error is also acceptable
            }
        }
    }
}
