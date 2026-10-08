//! Coprocesses: `coproc [NAME] command`.
//!
//! Important decisions:
//! - A coproc is a background job (see `jobs.rs`) whose stdin and stdout
//!   are two in-process [`pipe::Pipe`]s, the same bounded pipe concurrent
//!   pipeline stages use. The body runs on a forked shell (subshell
//!   isolation, nothing touches the host), polled together with the
//!   foreground on the same task, so it makes progress whenever the shell
//!   blocks: on a read of `${NAME[0]}`, on a full `${NAME[1]}`, in `wait`.
//!   A request/response loop (`echo q >&${C[1]}; read r <&${C[0]}`)
//!   therefore works line by line, and `tr`-style bodies that read to EOF
//!   finish once the write end is closed.
//! - Descriptors: `${NAME[1]}` is an `exec_fd_table` entry
//!   ([`FdTarget::Coproc`]), `${NAME[0]}` an [`InputFd::Pipe`] in
//!   `coproc_buffers`. Both hold the pipe end behind an `Arc`, so dups
//!   (`exec 5>&${C[1]}`) and subshell snapshots share it and the end closes
//!   (EOF for the coproc, SIGPIPE for its writes) when the last copy goes.
//!   The coproc's own fork never sees its write end, or another coproc's.
//! - fd numbers follow bash, which moves all four pipe ends to the highest
//!   free fds below 64 (parent read, child write, child read, parent
//!   write) and keeps the outer two: 63/60 for the first coproc, 62/58
//!   for a second one while the first is open.
//! - Cleanup is deterministic, not on SIGCHLD timing: once `wait` has
//!   reaped the coproc's job, the next command boundary unsets NAME and
//!   NAME_PID and closes its fds. Output it wrote before exiting stays
//!   readable until then (bash may lose it if it reaps first).
//! - Jobs are scoped to one `exec()` call, so are coprocs: at the end of
//!   `exec()` the shell's ends are closed (the coproc sees EOF) and NAME is
//!   unset before the remaining jobs are awaited.
//! - Sequential mode (`concurrent_jobs(false)`) runs the body to completion
//!   at `coproc` with stdin at end of input; its output stays readable.
//! - THREAT[TM-DOS-063]/[TM-DOS-122]: a coproc takes a background-job slot
//!   and two persistent descriptors; pipes buffer at most
//!   [`pipe::PIPE_CAPACITY`] plus one command's output before the writer
//!   waits.

use super::*;

/// A readable descriptor (`exec 3<file`, a here-doc, a coproc's output).
#[derive(Clone)]
pub(super) enum InputFd {
    /// Remaining lines, stored reversed so `pop()` yields the first.
    Lines(Vec<String>),
    /// Read end of a coproc's stdout pipe.
    Pipe(Arc<pipe::ReadEnd>),
}

impl InputFd {
    pub(super) fn pipe(&self) -> Option<&Arc<pipe::Pipe>> {
        match self {
            Self::Pipe(end) => Some(&end.0),
            Self::Lines(_) => None,
        }
    }
}

/// Write end of a coproc's stdin pipe, as an fd-table target.
#[derive(Clone)]
pub(super) struct CoprocWriter(Arc<pipe::WriteEnd>);

impl std::fmt::Debug for CoprocWriter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("CoprocWriter")
    }
}

impl CoprocWriter {
    pub(super) fn pipe(&self) -> &Arc<pipe::Pipe> {
        &self.0.0
    }

    /// Append `data`. A write after the coproc exited is dropped.
    // WTF: bash gets SIGPIPE (the shell dies) writing to a coproc that
    // exited but was not reaped yet; a sandboxed shell should not.
    pub(super) fn write(&self, data: &[u8]) {
        let _ = self.pipe().write(data);
    }
}

/// Wait while the coproc's stdin pipe holds a full buffer (it drains as
/// the coproc reads, or the coproc exits).
pub(super) async fn coproc_backpressure(w: &CoprocWriter) {
    Box::pin(w.pipe().writable()).await;
}

/// Read what a command with `demand` needs from a coproc's output pipe.
/// Line-wise demands leave the rest in the pipe.
pub(super) async fn read_coproc_input(
    pipe: Arc<pipe::Pipe>,
    demand: StdinDemand,
) -> crate::StreamData {
    let mut buf = Vec::new();
    while !demand.satisfied(&buf) {
        let chunk = if matches!(demand, StdinDemand::All | StdinDemand::Bytes(_)) {
            pipe.read_some().await
        } else {
            pipe.read_line_chunk().await
        };
        if chunk.is_empty() {
            break;
        }
        buf.extend_from_slice(&chunk);
    }
    buf.into()
}

/// The simple command a coproc body consists of (`cat`, `{ cat; }`).
fn lone_simple_command(body: &Command) -> Option<&SimpleCommand> {
    match body {
        Command::Simple(simple) => Some(simple),
        Command::Compound(CompoundCommand::BraceGroup(cmds), redirects) if redirects.is_empty() => {
            match cmds.as_slice() {
                [Command::Simple(simple)] => Some(simple),
                _ => None,
            }
        }
        _ => None,
    }
}

/// A live coproc of this shell.
pub(super) struct CoprocEntry {
    name: String,
    /// Job-table key.
    job: usize,
    pid: u32,
    read_fd: i32,
    write_fd: i32,
    /// The coproc's stdout pipe (read by `${NAME[0]}`).
    output: Arc<pipe::Pipe>,
    /// The coproc's stdin pipe (written by `${NAME[1]}`).
    input: Arc<pipe::Pipe>,
}

impl Interpreter {
    /// `coproc [NAME] command`. Boxed: this is one arm of the compound
    /// dispatch, so its state stays off that future.
    pub(super) fn execute_coproc<'a>(
        &'a mut self,
        coproc: &'a CoprocCommand,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<ExecResult>> + Send + 'a>> {
        Box::pin(async move {
            let mut stderr = String::new();
            if let Some(warning) = self.coproc_still_exists_warning() {
                stderr.push_str(&warning);
            }
            let (read_fd, write_fd) = self.allocate_coproc_fds()?;

            let input = pipe::Pipe::new();
            let output = pipe::Pipe::new();
            let Some((mut child, slot)) = self.fork_for_coproc(&mut stderr) else {
                self.last_exit_code = 1;
                return Ok(ExecResult::err(stderr, 1));
            };
            child
                .exec_fd_table
                .retain(|_, t| !matches!(t, FdTarget::Coproc(_)));
            if self.concurrent_jobs {
                child.pipe_in = Some(Arc::clone(&input));
                child.pipe_out = Some(Arc::clone(&output));
                let sink = Arc::clone(&output);
                child.output_callback = Some(Box::new(move |out, _err| {
                    sink.write(out.as_bytes());
                }));
            } else {
                // Nobody can write before the body has run: stdin is at EOF.
                input.close_write();
                child.pipeline_stdin = Some(crate::StreamData::new());
            }
            let body = coproc.body.clone();
            let streaming = self.concurrent_jobs;
            let job_diag = self.diag_prefix();
            let read_end = pipe::ReadEnd(Arc::clone(&input));
            let write_end = pipe::WriteEnd(Arc::clone(&output));
            let mut fut: jobs::JobFuture = Box::pin(async move {
                let _slot = slot;
                let _read_end = read_end;
                // A lone `cat`/`tr`/`grep` (`coproc cat`, `coproc { tr a-z
                // A-Z; }`) streams like a pipeline stage: each line written
                // in comes out before end of input.
                if streaming
                    && let Some(simple) = lone_simple_command(&body)
                    && Self::streams_stdout(simple)
                {
                    child.stream_stdout_command = Some(simple as *const SimpleCommand as usize);
                }
                let jobs = Arc::clone(&child.jobs);
                let result = jobs::with_jobs(&jobs, child.execute_command(&body)).await;
                jobs.finish_all().await;
                let (out, err) = jobs.lock().take_finished_output();
                let mut r = match result {
                    Ok(r) => r,
                    Err(e) => return ExecResult::err(format!("{job_diag}{e}\n"), 1),
                };
                r.stdout.append(&out);
                r.stderr.append(&err);
                // Output not yet sent at a command boundary goes now.
                let sent = child.output_stream_stdout_bytes.min(r.stdout.len());
                write_end.0.write(&r.stdout.as_bytes()[sent..]);
                r.stdout = crate::StreamData::new();
                if write_end.0.is_broken() {
                    r.exit_code = 141;
                } else if let ControlFlow::Exit(code) | ControlFlow::Return(code) = r.control_flow {
                    r.exit_code = code;
                }
                r.control_flow = ControlFlow::None;
                r
            });

            let text = format!("coproc {} {}", coproc.name, describe_command(&coproc.body));
            // Poll once (sequential mode: to completion). A body that never
            // blocks finishes here; its stderr is this command's.
            let finished = if self.concurrent_jobs {
                match std::future::poll_fn(|cx| std::task::Poll::Ready(fut.as_mut().poll(cx))).await
                {
                    std::task::Poll::Ready(r) => Some(r),
                    std::task::Poll::Pending => None,
                }
            } else {
                Some(fut.as_mut().await)
            };
            let (job, pid) = match finished {
                Some(result) => {
                    stderr.push_str(&result.stderr.text_lossy());
                    self.jobs.lock().spawn_finished(text, result)
                }
                None => self.jobs.lock().spawn_running(text, fut),
            };

            self.coproc_buffers.insert(
                read_fd,
                InputFd::Pipe(Arc::new(pipe::ReadEnd(Arc::clone(&output)))),
            );
            self.exec_fd_table.insert(
                write_fd,
                FdTarget::Coproc(CoprocWriter(Arc::new(pipe::WriteEnd(Arc::clone(&input))))),
            );
            let name = coproc.name.clone();
            let mut arr = HashMap::new();
            arr.insert(0, read_fd.to_string());
            arr.insert(1, write_fd.to_string());
            self.arrays_mut().insert(name.clone(), arr);
            self.vars_mut()
                .insert(format!("{name}_PID"), pid.to_string());
            self.last_bg_pid = Some(pid.to_string());
            self.coprocs.push(CoprocEntry {
                name,
                job,
                pid,
                read_fd,
                write_fd,
                output,
                input,
            });
            self.last_exit_code = 0;
            Ok(ExecResult {
                stderr: stderr.into(),
                ..Default::default()
            })
        })
    }

    /// bash keeps one coproc at a time and warns when a new one starts
    /// while it runs (the new one still starts).
    fn coproc_still_exists_warning(&self) -> Option<String> {
        let jobs = self.jobs.lock();
        let running: Vec<usize> = jobs.running_ids();
        let live = self
            .coprocs
            .iter()
            .rev()
            .find(|c| running.contains(&c.job))?;
        Some(self.diag(format!(
            "warning: execute_coproc: coproc [{}:{}] still exists\n",
            live.pid, live.name
        )))
    }

    /// The fd pair for a new coproc (see the module docs).
    fn allocate_coproc_fds(&self) -> Result<(i32, i32)> {
        let mut free = Vec::with_capacity(4);
        let mut fd = redirection::COPROC_FIRST_FD;
        while free.len() < 4 && fd >= 3 {
            if !self.fd_is_open(fd) {
                free.push(fd);
            }
            fd -= 1;
        }
        let max = self.limits.max_file_descriptors;
        if free.len() < 4 || self.persistent_fd_count() + 2 > max {
            return Err(crate::limits::LimitExceeded::MaxFileDescriptors(max).into());
        }
        Ok((free[0], free[3]))
    }

    /// Fork the shell for a coproc body and claim its job slot. On failure
    /// the fork error is appended to `stderr`.
    fn fork_for_coproc(&mut self, stderr: &mut String) -> Option<(Interpreter, jobs::JobSlot)> {
        // THREAT[TM-DOS-122]: a coproc is a background job.
        let slot = self.jobs.try_claim(self.limits.max_background_jobs);
        let mut child = self.fork_for_job();
        let depth_ok = child.counters.push_subshell(&child.limits).is_ok();
        match slot.filter(|_| depth_ok) {
            Some(slot) => Some((child, slot)),
            None => {
                stderr.push_str(&format!(
                    "{}: fork: retry: Resource temporarily unavailable (max {} background jobs, {} nested)\n",
                    self.diag_name(),
                    self.limits.max_background_jobs,
                    self.limits.max_subshell_depth
                ));
                None
            }
        }
    }

    /// At a command boundary: forget coprocs whose job `wait` reaped,
    /// unsetting NAME / NAME_PID and closing their fds like bash does.
    #[inline(never)]
    pub(super) fn reap_coprocs(&mut self) {
        if self.coprocs.is_empty() {
            return;
        }
        let ids = self.jobs.lock().ids();
        let (gone, live): (Vec<_>, Vec<_>) = std::mem::take(&mut self.coprocs)
            .into_iter()
            .partition(|c| !ids.contains(&c.job));
        self.coprocs = live;
        for c in gone {
            self.drop_coproc(c);
        }
    }

    /// End of `exec()`: close every coproc's fds (it sees EOF) and unset
    /// its variables before the remaining jobs are awaited.
    pub(super) fn close_coprocs(&mut self) {
        for c in std::mem::take(&mut self.coprocs) {
            self.drop_coproc(c);
        }
    }

    fn drop_coproc(&mut self, c: CoprocEntry) {
        if self
            .coproc_buffers
            .get(&c.read_fd)
            .and_then(InputFd::pipe)
            .is_some_and(|p| Arc::ptr_eq(p, &c.output))
        {
            self.coproc_buffers.remove(&c.read_fd);
        }
        if matches!(self.exec_fd_table.get(&c.write_fd),
            Some(FdTarget::Coproc(w)) if Arc::ptr_eq(w.pipe(), &c.input))
        {
            self.exec_fd_table.remove(&c.write_fd);
        }
        self.arrays_mut().remove(&c.name);
        self.vars_mut().remove(&format!("{}_PID", c.name));
    }

    /// `read -u FD` on a readable fd: the line it reads, or the coproc pipe
    /// to read it from.
    pub(super) fn read_u_source(&mut self, args: &[String]) -> Option<ReadSource> {
        let mut iter = args.iter();
        let fd = loop {
            let arg = iter.next()?;
            if arg == "-u" {
                break iter.next()?.parse::<i32>().ok()?;
            }
            if let Some(rest) = arg.strip_prefix("-u")
                && let Ok(fd) = rest.parse::<i32>()
            {
                break fd;
            }
        };
        match self.coproc_buffers.get_mut(&fd)? {
            InputFd::Lines(buf) => Some(ReadSource::Ready(
                buf.pop().map(|l| format!("{l}\n")).unwrap_or_default(),
            )),
            InputFd::Pipe(end) => Some(ReadSource::Pipe(Arc::clone(&end.0))),
        }
    }
}

/// Where `read -u FD` takes its input from.
pub(super) enum ReadSource {
    /// Already available (empty at end of input).
    Ready(String),
    /// A coproc's output pipe.
    Pipe(Arc<pipe::Pipe>),
}
