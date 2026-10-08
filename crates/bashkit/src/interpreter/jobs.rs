//! Job control: background jobs (`cmd &`) as concurrently polled futures.
//!
//! Decisions:
//! - A job runs on a forked interpreter (the same isolation as a subshell)
//!   as a boxed `'static` future. Jobs are not `tokio::spawn`ed: the
//!   interpreter's `execute()` polls them together with the foreground script
//!   on the same task ([`JobDriver`]), so no threads or reactor are needed,
//!   wasm keeps working, and a `Terminal`'s pull-driven session future drives
//!   its jobs too. Jobs make progress whenever the foreground awaits
//!   (`sleep`, `wait`, I/O), which is what `a & b & wait` needs.
//! - Spawning polls the job once right away. A job that never blocks (`echo
//!   hi &`) finishes inside that poll, so its output lands in order, as it
//!   did when jobs ran synchronously. Only jobs that block run concurrently.
//! - Output of a job that finishes later is delivered at the next reap
//!   point: `wait`, a top-level command boundary, or the end of `exec()`
//!   (which waits for every job, like a pipe reader waiting for EOF).
//! - `kill` drops the job's future (immediate, even mid-`sleep`); the job
//!   reports 128 + signal (143 for TERM).
//! - THREAT[TM-DOS-122]: live jobs are capped by
//!   `ExecutionLimits::max_background_jobs`; past the cap `&` fails. Jobs share
//!   the request's execution budget, cancellation token and timeout.
//! - Virtual PIDs start at [`FIRST_PID`] and never reuse; `$$` stays 1.

use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};
use std::sync::{Arc, Mutex as StdMutex, MutexGuard};
use std::task::{Context, Poll, Waker};

use futures_util::future::{AbortHandle, Abortable};
use futures_util::stream::{FuturesUnordered, StreamExt};

use crate::interpreter::ExecResult;

/// First virtual PID handed out (`$!` of the first job).
pub const FIRST_PID: u32 = 1001;

/// Boxed job future: the job's final result.
pub(crate) type JobFuture = Pin<Box<dyn Future<Output = ExecResult> + Send + 'static>>;

type Running = Pin<Box<dyn Future<Output = (usize, Option<ExecResult>)> + Send + 'static>>;

/// State of one job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JobState {
    /// Still running.
    Running,
    /// Finished with this exit code.
    Done(i32),
}

struct Job {
    /// Job number (`%N`): one more than the highest running job's, like
    /// bash, so numbers are reused once jobs finish.
    number: usize,
    pid: u32,
    command: String,
    abort: Option<AbortHandle>,
    /// Final result, once finished and until reaped.
    result: Option<ExecResult>,
    /// Signal number the job was killed with, if any.
    killed: Option<i32>,
    /// Whether finished output has been handed to the parent.
    reported: bool,
}

/// Snapshot of a job for `jobs`/`ps`.
#[derive(Debug, Clone)]
pub struct JobInfo {
    /// Job number (`%N`).
    pub number: usize,
    /// Virtual PID.
    pub pid: u32,
    /// Command text.
    pub command: String,
    /// Current state.
    pub state: JobState,
}

/// Job table plus the futures of running jobs. Jobs are keyed by PID.
pub struct JobTable {
    jobs: BTreeMap<usize, Job>,
    next_pid: u32,
    running: FuturesUnordered<Running>,
    driver: Option<Waker>,
}

impl Default for JobTable {
    fn default() -> Self {
        Self::new()
    }
}

impl JobTable {
    /// Create an empty table.
    pub fn new() -> Self {
        Self {
            jobs: BTreeMap::new(),
            next_pid: FIRST_PID,
            running: FuturesUnordered::new(),
            driver: None,
        }
    }

    /// Allocate a virtual PID (jobs and coprocesses share the sequence).
    pub fn alloc_pid(&mut self) -> u32 {
        let pid = self.next_pid;
        self.next_pid = self.next_pid.saturating_add(1);
        pid
    }

    fn insert(&mut self, command: String) -> (usize, u32) {
        let number = self
            .jobs
            .values()
            .filter(|j| j.result.is_none())
            .map(|j| j.number)
            .max()
            .unwrap_or(0)
            + 1;
        let pid = self.alloc_pid();
        let id = pid as usize;
        self.jobs.insert(
            id,
            Job {
                number,
                pid,
                command,
                abort: None,
                result: None,
                killed: None,
                reported: false,
            },
        );
        (id, pid)
    }

    /// Register a job that already finished (sequential mode, or a job that
    /// completed during its first poll). Its output counts as reported.
    pub fn spawn_finished(&mut self, command: String, result: ExecResult) -> (usize, u32) {
        let (id, pid) = self.insert(command);
        let exit = result.exit_code;
        if let Some(job) = self.jobs.get_mut(&id) {
            job.result = Some(ExecResult::with_code(String::new(), exit));
            job.reported = true;
        }
        (id, pid)
    }

    /// Register a running job.
    pub(crate) fn spawn_running(&mut self, command: String, fut: JobFuture) -> (usize, u32) {
        let (id, pid) = self.insert(command);
        let (handle, reg) = AbortHandle::new_pair();
        if let Some(job) = self.jobs.get_mut(&id) {
            job.abort = Some(handle);
        }
        let fut = Abortable::new(fut, reg);
        self.running
            .push(Box::pin(async move { (id, fut.await.ok()) }));
        if let Some(w) = self.driver.take() {
            w.wake();
        }
        (id, pid)
    }

    /// Number of jobs still running.
    pub fn running_count(&self) -> usize {
        self.jobs.values().filter(|j| j.result.is_none()).count()
    }

    /// Keys of the jobs still running.
    pub fn running_ids(&self) -> Vec<usize> {
        self.jobs
            .iter()
            .filter(|(_, j)| j.result.is_none())
            .map(|(id, _)| *id)
            .collect()
    }

    /// Running jobs as `(number, key)`, oldest first.
    fn running_by_number(&self) -> Vec<(usize, usize)> {
        let mut v: Vec<(usize, usize)> = self
            .jobs
            .iter()
            .filter(|(_, j)| j.result.is_none())
            .map(|(id, j)| (j.number, *id))
            .collect();
        v.sort_unstable();
        v
    }

    /// Key for a `wait`/`kill` operand: `%N`, `%%`/`%+`, `%-` (running
    /// jobs), or a PID (any job not yet reaped).
    pub fn resolve(&self, spec: &str) -> Option<usize> {
        if let Some(rest) = spec.strip_prefix('%') {
            let running = self.running_by_number();
            return match rest {
                "" | "%" | "+" => running.last().map(|(_, id)| *id),
                "-" => running.iter().rev().nth(1).map(|(_, id)| *id),
                n => {
                    let n: usize = n.parse().ok()?;
                    running.iter().find(|(num, _)| *num == n).map(|(_, id)| *id)
                }
            };
        }
        let pid: u32 = spec.parse().ok()?;
        self.jobs
            .iter()
            .find(|(_, j)| j.pid == pid)
            .map(|(id, _)| *id)
    }

    /// Kill a running job with `signal`. Returns false if it already ended.
    pub fn kill(&mut self, id: usize, signal: i32) -> bool {
        let Some(job) = self.jobs.get_mut(&id) else {
            return false;
        };
        if job.result.is_some() {
            return false;
        }
        // Default-ignored signals (CHLD, URG, WINCH) and stop/continue
        // (STOP, TSTP, TTIN, TTOU, CONT) leave the job running: jobs can't
        // be suspended (L-PROC-002).
        if matches!(signal, 0 | 17..=23 | 28) {
            return true;
        }
        if let Some(handle) = job.abort.take() {
            handle.abort();
        }
        job.killed = Some(signal);
        true
    }

    /// Take a finished job's result, removing it from the table.
    pub fn reap(&mut self, id: usize) -> Option<ExecResult> {
        let job = self.jobs.get(&id)?;
        job.result.as_ref()?;
        let job = self.jobs.remove(&id)?;
        let mut result = job.result?;
        if job.reported {
            result.stdout = crate::StreamData::new();
            result.stderr = crate::StreamData::new();
        }
        Some(result)
    }

    /// Output of finished, not yet reported jobs (in job order). The jobs
    /// stay in the table so `wait $pid` still returns their status.
    pub fn take_finished_output(&mut self) -> (crate::StreamData, crate::StreamData) {
        let mut out = crate::StreamData::new();
        let mut err = crate::StreamData::new();
        for job in self.jobs.values_mut() {
            if job.reported {
                continue;
            }
            if let Some(result) = &mut job.result {
                out.append(&std::mem::take(&mut result.stdout));
                err.append(&std::mem::take(&mut result.stderr));
                job.reported = true;
            }
        }
        (out, err)
    }

    /// Finished jobs, in job order.
    pub fn finished_ids(&self) -> Vec<usize> {
        self.jobs
            .iter()
            .filter(|(_, j)| j.result.is_some())
            .map(|(id, _)| *id)
            .collect()
    }

    /// All job numbers, in order.
    pub fn ids(&self) -> Vec<usize> {
        self.jobs.keys().copied().collect()
    }

    /// Snapshot of every job.
    pub fn list(&self) -> Vec<JobInfo> {
        self.jobs
            .values()
            .map(|j| JobInfo {
                number: j.number,
                pid: j.pid,
                command: j.command.clone(),
                state: match &j.result {
                    Some(r) => JobState::Done(r.exit_code),
                    None => JobState::Running,
                },
            })
            .collect()
    }

    /// Forget a job (`disown`).
    pub fn disown(&mut self, id: usize) -> bool {
        self.jobs.remove(&id).is_some()
    }

    /// Drop every job (running ones are aborted).
    pub fn clear(&mut self) {
        for job in self.jobs.values_mut() {
            if let Some(handle) = job.abort.take() {
                handle.abort();
            }
        }
        self.jobs.clear();
        self.running = FuturesUnordered::new();
    }

    fn finish(&mut self, id: usize, result: Option<ExecResult>) {
        if let Some(job) = self.jobs.get_mut(&id) {
            job.abort = None;
            job.result = Some(match (result, job.killed) {
                (Some(r), None) => r,
                (_, Some(sig)) => ExecResult::with_code(String::new(), 128 + sig),
                (None, None) => ExecResult::with_code(String::new(), 143),
            });
        }
    }

    /// Poll running jobs; returns true when at least one finished.
    fn poll_running(&mut self, cx: &mut Context<'_>) -> bool {
        let mut progressed = false;
        while let Poll::Ready(Some((id, result))) = self.running.poll_next_unpin(cx) {
            self.finish(id, result);
            progressed = true;
        }
        self.driver = Some(cx.waker().clone());
        progressed
    }
}

/// Shared job table plus a wakeup for waiters.
pub struct JobControl {
    table: StdMutex<JobTable>,
    changed: tokio::sync::Notify,
    /// Live jobs across the whole session: this shell and every job forked
    /// from it (nested `&` inside a job counts too).
    live: Arc<AtomicUsize>,
}

/// One live job's claim on the session-wide job cap; released on drop
/// (finish or abort).
pub struct JobSlot(Arc<AtomicUsize>);

impl Drop for JobSlot {
    fn drop(&mut self) {
        self.0.fetch_sub(1, AtomicOrdering::AcqRel);
    }
}

impl JobControl {
    fn new() -> Self {
        Self::with_live(Arc::new(AtomicUsize::new(0)))
    }

    fn with_live(live: Arc<AtomicUsize>) -> Self {
        Self {
            table: StdMutex::new(JobTable::new()),
            changed: tokio::sync::Notify::new(),
            live,
        }
    }

    /// Empty table for a forked job that shares this session's job cap.
    pub fn fork(&self) -> SharedJobTable {
        Arc::new(Self::with_live(Arc::clone(&self.live)))
    }

    /// Claim a slot under `max` live jobs session-wide (TM-DOS-122).
    pub fn try_claim(&self, max: usize) -> Option<JobSlot> {
        self.live
            .fetch_update(AtomicOrdering::AcqRel, AtomicOrdering::Acquire, |n| {
                (n < max).then_some(n + 1)
            })
            .ok()
            .map(|_| JobSlot(Arc::clone(&self.live)))
    }

    /// Lock the table. Never held across an `.await`.
    pub fn lock(&self) -> MutexGuard<'_, JobTable> {
        self.table.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Wait until `ready` returns `Some`, re-checking after every job
    /// completion. Jobs are driven by the [`JobDriver`] polled alongside.
    pub async fn wait_until<T>(&self, mut ready: impl FnMut(&mut JobTable) -> Option<T>) -> T {
        loop {
            let notified = self.changed.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if let Some(v) = ready(&mut self.lock()) {
                return v;
            }
            notified.await;
        }
    }

    /// Future that drives running jobs; never completes on its own.
    pub fn driver(self: &Arc<Self>) -> JobDriver {
        JobDriver {
            control: Arc::clone(self),
        }
    }

    /// Drive every running job to completion (end of `exec()`).
    pub async fn finish_all(self: &Arc<Self>) {
        let driver = self.driver();
        let all_done = self.wait_until(|t| (t.running_count() == 0).then_some(()));
        tokio::pin!(all_done);
        tokio::pin!(driver);
        std::future::poll_fn(|cx| {
            let _ = driver.as_mut().poll(cx);
            all_done.as_mut().poll(cx)
        })
        .await;
    }
}

/// Polls the running jobs of one [`JobControl`].
pub struct JobDriver {
    control: Arc<JobControl>,
}

impl Future for JobDriver {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        let progressed = self.control.lock().poll_running(cx);
        if progressed {
            self.control.changed.notify_waiters();
        }
        Poll::Pending
    }
}

/// Run `fut` while driving the jobs of `control` on the same task.
pub(crate) async fn with_jobs<F: Future>(control: &Arc<JobControl>, fut: F) -> F::Output {
    let driver = control.driver();
    tokio::pin!(fut);
    tokio::pin!(driver);
    std::future::poll_fn(|cx| {
        if let Poll::Ready(v) = fut.as_mut().poll(cx) {
            return Poll::Ready(v);
        }
        let _ = driver.as_mut().poll(cx);
        // A job may have finished and woken a waiter inside `fut`.
        fut.as_mut().poll(cx)
    })
    .await
}

/// Thread-safe job control handle.
pub type SharedJobTable = Arc<JobControl>;

/// Create a new shared job table.
pub fn new_shared_job_table() -> SharedJobTable {
    Arc::new(JobControl::new())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn sleeper(ms: u64, out: &'static str) -> JobFuture {
        Box::pin(async move {
            tokio::time::sleep(Duration::from_millis(ms)).await;
            ExecResult::ok(out.to_string())
        })
    }

    #[tokio::test]
    async fn jobs_run_concurrently_with_the_foreground() {
        let jobs = new_shared_job_table();
        let start = tokio::time::Instant::now();
        let (a, pa) = jobs.lock().spawn_running("a".into(), sleeper(100, "a\n"));
        let (b, _) = jobs.lock().spawn_running("b".into(), sleeper(100, "b\n"));
        assert_eq!(pa, FIRST_PID);
        with_jobs(&jobs, async {
            jobs.wait_until(|t| (t.running_count() == 0).then_some(()))
                .await;
        })
        .await;
        assert!(start.elapsed() < Duration::from_millis(190));
        assert_eq!(jobs.lock().reap(a).unwrap().stdout.to_string(), "a\n");
        assert_eq!(jobs.lock().reap(b).unwrap().exit_code, 0);
    }

    #[tokio::test]
    async fn kill_aborts_and_reports_signal() {
        let jobs = new_shared_job_table();
        let (id, _) = jobs.lock().spawn_running("s".into(), sleeper(10_000, ""));
        assert!(jobs.lock().kill(id, 15));
        jobs.finish_all().await;
        assert_eq!(jobs.lock().reap(id).unwrap().exit_code, 143);
    }

    #[tokio::test]
    async fn resolve_specs() {
        let jobs = new_shared_job_table();
        let mut t = jobs.lock();
        let (a, pa) = t.spawn_finished("a".into(), ExecResult::ok(String::new()));
        let (b, _) = t.spawn_finished("b".into(), ExecResult::ok(String::new()));
        // Finished jobs keep their PID for `wait` but leave the job list.
        assert_eq!(t.resolve("%1"), None);
        assert_eq!(t.resolve(&pa.to_string()), Some(a));
        assert_eq!(t.resolve("999"), None);
        drop(t);
        let (c, _) = jobs.lock().spawn_running("c".into(), sleeper(10_000, ""));
        let (d, _) = jobs.lock().spawn_running("d".into(), sleeper(10_000, ""));
        let t = jobs.lock();
        assert_eq!(t.resolve("%1"), Some(c));
        assert_eq!(t.resolve("%%"), Some(d));
        assert_eq!(t.resolve("%-"), Some(c));
        assert_ne!(b, c);
    }
}
