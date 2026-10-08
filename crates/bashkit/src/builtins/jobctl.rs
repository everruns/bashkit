//! Job control and process builtins over the virtual job table:
//! `kill`, `jobs`, `disown`, `bg`, `fg`, `ps`, `pgrep`, `pkill`.
//!
//! Decisions:
//! - The process table is the shell (`$$` = 1, command `bash`) plus its
//!   background jobs (virtual PIDs from `interpreter/jobs.rs`). Nothing is
//!   read from the host (TM-INF-014).
//! - `kill` delivers a signal by dropping the job's future; the job's status
//!   becomes 128 + signal. Signals to the shell itself (`kill $$`) and
//!   `-0` probes succeed without effect.
//! - Jobs never stop, so `bg` is a no-op on a running job and `fg` waits for
//!   it (printing its command first, as bash does).

use async_trait::async_trait;

use super::{Builtin, BuiltinSideEffect, Context};
use crate::error::Result;
use crate::interpreter::{ExecResult, JobInfo, JobState};

const SIGNALS: &[(&str, i32)] = &[
    ("HUP", 1),
    ("INT", 2),
    ("QUIT", 3),
    ("ILL", 4),
    ("TRAP", 5),
    ("ABRT", 6),
    ("BUS", 7),
    ("FPE", 8),
    ("KILL", 9),
    ("USR1", 10),
    ("SEGV", 11),
    ("USR2", 12),
    ("PIPE", 13),
    ("ALRM", 14),
    ("TERM", 15),
    ("CHLD", 17),
    ("CONT", 18),
    ("STOP", 19),
    ("TSTP", 20),
];

/// Signal number for `9`, `KILL`, `SIGKILL`, `kill` (case-insensitive).
fn parse_signal(spec: &str) -> Option<i32> {
    if let Ok(n) = spec.parse::<i32>() {
        return (0..=64).contains(&n).then_some(n);
    }
    let upper = spec.to_ascii_uppercase();
    let name = upper.strip_prefix("SIG").unwrap_or(&upper);
    SIGNALS.iter().find(|(n, _)| *n == name).map(|(_, v)| *v)
}

pub(crate) fn signal_name(n: i32) -> Option<&'static str> {
    SIGNALS.iter().find(|(_, v)| *v == n).map(|(name, _)| *name)
}

const SHELL_PID: u32 = 1;

/// The `kill` builtin.
pub struct Kill;

#[async_trait]
impl Builtin for Kill {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        if let Some(r) = super::check_help_version(
            ctx.args,
            "Usage: kill [-s SIGNAL | -SIGNAL] PID|%JOB...\n       kill -l [SIGNAL]\nSend a signal to a job.\n\n  -s SIGNAL\tspecify the signal to send\n  -n NUM\tspecify the signal number\n  -l, -L\tlist signal names\n      --help\tdisplay this help and exit\n      --version\toutput version information and exit\n",
            Some("kill (bashkit) 0.1"),
        ) {
            return Ok(r);
        }

        let mut signal = 15;
        let mut targets: Vec<&str> = Vec::new();
        let mut args = ctx.args.iter().peekable();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "-l" | "-L" => {
                    // `kill -l 143` / `kill -l 15` → name; bare → list.
                    if let Some(v) = args.next() {
                        let n = v.parse::<i32>().unwrap_or(-1);
                        let n = if n > 128 { n - 128 } else { n };
                        return Ok(match signal_name(n) {
                            Some(name) => ExecResult::ok(format!("{name}\n")),
                            None => ExecResult::err(
                                format!("bash: kill: {v}: invalid signal specification\n"),
                                1,
                            ),
                        });
                    }
                    let list: Vec<String> = SIGNALS
                        .iter()
                        .map(|(n, v)| format!("{v:2}) SIG{n}"))
                        .collect();
                    return Ok(ExecResult::ok(format!("{}\n", list.join("\n"))));
                }
                "-s" | "-n" => {
                    let Some(spec) = args.next() else {
                        return Ok(ExecResult::err(
                            format!("bash: kill: {arg}: option requires an argument\n"),
                            2,
                        ));
                    };
                    match parse_signal(spec) {
                        Some(n) => signal = n,
                        None => {
                            return Ok(ExecResult::err(
                                format!("bash: kill: {spec}: invalid signal specification\n"),
                                1,
                            ));
                        }
                    }
                }
                "--" => targets.extend(args.by_ref().map(String::as_str)),
                a if a.starts_with('-') && targets.is_empty() && a.len() > 1 => {
                    match parse_signal(&a[1..]) {
                        Some(n) => signal = n,
                        None => {
                            return Ok(ExecResult::err(
                                format!("bash: kill: {}: invalid signal specification\n", &a[1..]),
                                1,
                            ));
                        }
                    }
                }
                a => targets.push(a),
            }
        }

        if targets.is_empty() {
            return Ok(ExecResult::err(
                "kill: usage: kill [-s sigspec | -n signum | -sigspec] pid | jobspec ... or kill -l [sigspec]\n",
                2,
            ));
        }

        let mut stderr = String::new();
        let mut exit_code = 0;
        let mut side_effects = Vec::new();
        for target in targets {
            if target.parse::<u32>() == Ok(SHELL_PID) {
                // `kill -SIG $$`: the interpreter runs the trap, or ends the
                // script with 128 + signal. `-0` only tests for the process.
                if signal != 0 {
                    side_effects.push(BuiltinSideEffect::SignalSelf(signal));
                }
                continue;
            }
            let resolved = ctx.shell.as_ref().and_then(|shell| {
                let jobs = shell.jobs();
                let mut table = jobs.lock();
                let id = table.resolve(target)?;
                let running = table
                    .list()
                    .iter()
                    .any(|j| j.pid as usize == id && j.state == JobState::Running);
                if signal != 0 && running {
                    table.kill(id, signal);
                }
                Some(running)
            });
            match resolved {
                Some(true) => {}
                // Finished but not reaped: bash reports no such process.
                Some(false) | None => {
                    exit_code = 1;
                    if target.starts_with('%') && resolved.is_none() {
                        stderr.push_str(&format!("bash: kill: {target}: no such job\n"));
                    } else if target.parse::<u32>().is_ok() {
                        stderr.push_str(&format!("bash: kill: ({target}) - No such process\n"));
                    } else if resolved.is_none() {
                        stderr.push_str(&format!(
                            "bash: kill: {target}: arguments must be process or job IDs\n"
                        ));
                    } else {
                        stderr.push_str(&format!("bash: kill: {target}: no such job\n"));
                    }
                }
            }
        }
        Ok(ExecResult {
            stderr: stderr.into(),
            exit_code,
            side_effects,
            ..Default::default()
        })
    }
}

fn job_marker(idx: usize, len: usize) -> char {
    if idx + 1 == len {
        '+'
    } else if idx + 2 == len {
        '-'
    } else {
        ' '
    }
}

fn state_text(state: &JobState) -> String {
    match state {
        JobState::Running => "Running".to_string(),
        JobState::Done(0) => "Done".to_string(),
        JobState::Done(c) if *c > 128 => match signal_name(c - 128) {
            Some("TERM") => "Terminated".to_string(),
            Some("KILL") => "Killed".to_string(),
            Some("INT") => "Interrupt".to_string(),
            Some(name) => name.to_string(),
            None => format!("Exit {c}"),
        },
        JobState::Done(c) => format!("Exit {c}"),
    }
}

/// The `jobs` builtin: `jobs [-l|-p|-r|-s] [JOBSPEC...]`.
pub struct Jobs;

#[async_trait]
impl Builtin for Jobs {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let Some(shell) = ctx.shell.as_ref() else {
            return Ok(ExecResult::ok(String::new()));
        };
        let (mut long, mut pids_only, mut running_only, mut stopped_only) =
            (false, false, false, false);
        let mut specs = Vec::new();
        for arg in ctx.args {
            match arg.as_str() {
                "-l" => long = true,
                "-p" => pids_only = true,
                "-r" => running_only = true,
                "-s" => stopped_only = true,
                "-n" => {}
                a if a.starts_with('-') => {
                    return Ok(ExecResult::err(
                        format!(
                            "bash: jobs: {a}: invalid option\njobs: usage: jobs [-lnprs] [jobspec ...]\n"
                        ),
                        2,
                    ));
                }
                a => specs.push(a.to_string()),
            }
        }
        let jobs = shell.jobs();
        let table = jobs.lock();
        // Non-interactive bash forgets finished jobs, so `jobs` lists only
        // running ones (by job number); `+` is the newest, `-` the one before.
        let mut running: Vec<JobInfo> = table
            .list()
            .into_iter()
            .filter(|j| j.state == JobState::Running)
            .collect();
        running.sort_by_key(|j| j.number);
        let selected: Vec<usize> = if specs.is_empty() {
            (0..running.len()).collect()
        } else {
            let mut out = Vec::new();
            for spec in &specs {
                let pos = table
                    .resolve(spec)
                    .and_then(|id| running.iter().position(|j| j.pid as usize == id));
                match pos {
                    Some(pos) => out.push(pos),
                    None => {
                        return Ok(ExecResult::err(
                            format!("bash: jobs: {spec}: no such job\n"),
                            1,
                        ));
                    }
                }
            }
            out
        };
        let mut output = String::new();
        for pos in selected {
            let job = &running[pos];
            if stopped_only {
                continue;
            }
            let _ = running_only;
            if pids_only {
                output.push_str(&format!("{}\n", job.pid));
                continue;
            }
            let marker = job_marker(pos, running.len());
            let state = format!("{:<24}", state_text(&job.state));
            if long {
                output.push_str(&format!(
                    "[{}]{} {} {}{} &\n",
                    job.number, marker, job.pid, state, job.command
                ));
            } else {
                output.push_str(&format!(
                    "[{}]{}  {}{} &\n",
                    job.number, marker, state, job.command
                ));
            }
        }
        Ok(ExecResult::ok(output))
    }
}

/// The `disown` builtin: forget jobs (`-a` all, default current).
pub struct Disown;

#[async_trait]
impl Builtin for Disown {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let Some(shell) = ctx.shell.as_ref() else {
            return Ok(ExecResult::ok(String::new()));
        };
        let jobs = shell.jobs();
        let mut table = jobs.lock();
        let mut specs: Vec<&str> = Vec::new();
        let mut all = false;
        for arg in ctx.args {
            match arg.as_str() {
                "-a" => all = true,
                "-h" | "-r" => {}
                a => specs.push(a),
            }
        }
        if all {
            for id in table.ids() {
                table.disown(id);
            }
            return Ok(ExecResult::ok(String::new()));
        }
        let current = specs.is_empty();
        if current {
            specs.push("%+");
        }
        for spec in specs {
            match table.resolve(spec) {
                Some(id) => {
                    table.disown(id);
                }
                None => {
                    let name = if current { "current" } else { spec };
                    return Ok(ExecResult::err(
                        format!("bash: disown: {name}: no such job\n"),
                        1,
                    ));
                }
            }
        }
        Ok(ExecResult::ok(String::new()))
    }
}

/// The `bg` builtin. Jobs never stop, so a running job is left as is.
pub struct Bg;

#[async_trait]
impl Builtin for Bg {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let Some(shell) = ctx.shell.as_ref() else {
            return Ok(ExecResult::err("bash: bg: no job control\n", 1));
        };
        let jobs = shell.jobs();
        let table = jobs.lock();
        let spec = ctx.args.first().map(String::as_str).unwrap_or("%+");
        let Some(id) = table.resolve(spec) else {
            let name = if ctx.args.is_empty() { "current" } else { spec };
            return Ok(ExecResult::err(
                format!("bash: bg: {name}: no such job\n"),
                1,
            ));
        };
        let info = table.list().into_iter().find(|j| j.pid as usize == id);
        match info {
            Some(j) if j.state == JobState::Running => Ok(ExecResult::err(
                format!("bash: bg: job {} already in background\n", j.number),
                0,
            )),
            _ => Ok(ExecResult::err(
                format!("bash: bg: job has terminated\n[{id}]+  Done\n"),
                1,
            )),
        }
    }
}

/// The `fg` builtin: wait for a job, printing its command first.
pub struct Fg;

#[async_trait]
impl Builtin for Fg {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let Some(shell) = ctx.shell.as_ref() else {
            return Ok(ExecResult::err("bash: fg: no job control\n", 1));
        };
        let jobs = shell.jobs();
        let spec = ctx.args.first().map(String::as_str).unwrap_or("%+");
        let found = {
            let table = jobs.lock();
            table.resolve(spec).and_then(|id| {
                table
                    .list()
                    .into_iter()
                    .find(|j| j.pid as usize == id)
                    .map(|j| (id, j.command))
            })
        };
        let Some((id, command)) = found else {
            let name = if ctx.args.is_empty() { "current" } else { spec };
            return Ok(ExecResult::err(
                format!("bash: fg: {name}: no such job\n"),
                1,
            ));
        };
        let result = jobs
            .wait_until(|t| {
                t.reap(id)
                    .map(Some)
                    .or_else(|| (!t.ids().contains(&id)).then_some(None))
            })
            .await;
        let mut out = crate::StreamData::from(format!("{command}\n"));
        let (stderr, code) = match result {
            Some(r) => {
                out.append(&r.stdout);
                (r.stderr, r.exit_code)
            }
            None => (crate::StreamData::new(), 0),
        };
        Ok(ExecResult {
            stdout: out,
            stderr,
            exit_code: code,
            ..Default::default()
        })
    }
}

/// Process table rows: the shell itself, then each job.
fn process_rows(ctx: &Context<'_>) -> Vec<(u32, String, String)> {
    let mut rows = vec![(SHELL_PID, "bash".to_string(), "bash".to_string())];
    if let Some(shell) = ctx.shell.as_ref() {
        for job in shell.jobs().lock().list() {
            if job.state == JobState::Running {
                let name = job
                    .command
                    .split_whitespace()
                    .next()
                    .unwrap_or("bash")
                    .to_string();
                rows.push((job.pid, name, job.command));
            }
        }
    }
    rows
}

fn user_name(ctx: &Context<'_>) -> String {
    ctx.variables
        .get("USER")
        .or_else(|| ctx.env.get("USER"))
        .cloned()
        .unwrap_or_else(|| super::DEFAULT_USERNAME.to_string())
}

/// The `ps` builtin over the virtual process table.
///
/// Formats: default (`PID TTY TIME CMD`), `-f`/`-ef` (full), `aux` (BSD
/// user format), `-o pid=` / `-o pid,comm` (subset of keywords).
pub struct Ps;

#[async_trait]
impl Builtin for Ps {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let mut full = false;
        let mut bsd_user = false;
        let mut columns: Option<Vec<String>> = None;
        let mut pid_filter: Option<Vec<u32>> = None;
        let mut args = ctx.args.iter();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "-o" | "-O" => {
                    if let Some(spec) = args.next() {
                        columns = Some(spec.split(',').map(str::to_string).collect());
                    }
                }
                "-p" | "--pid" => {
                    if let Some(list) = args.next() {
                        pid_filter = Some(
                            list.split(',')
                                .filter_map(|p| p.trim().parse().ok())
                                .collect(),
                        );
                    }
                }
                a if !a.starts_with('-') && a.contains('u') => bsd_user = true,
                a if a.starts_with('-') && a.contains('f') => full = true,
                _ => {}
            }
        }
        let rows: Vec<_> = process_rows(&ctx)
            .into_iter()
            .filter(|(pid, ..)| pid_filter.as_ref().is_none_or(|f| f.contains(pid)))
            .collect();
        if pid_filter.is_some() && rows.is_empty() {
            return Ok(ExecResult::with_code(
                if columns.is_some() {
                    String::new()
                } else {
                    "    PID TTY          TIME CMD\n".to_string()
                },
                1,
            ));
        }
        let user = user_name(&ctx);
        let mut out = String::new();
        if let Some(cols) = columns {
            let headers: Vec<(String, Option<String>)> = cols
                .iter()
                .map(|c| match c.split_once('=') {
                    Some((k, h)) => (k.to_string(), Some(h.to_string())),
                    None => (c.clone(), None),
                })
                .collect();
            let header: Vec<String> = headers
                .iter()
                .map(|(k, h)| h.clone().unwrap_or_else(|| k.to_ascii_uppercase()))
                .collect();
            if header.iter().any(|h| !h.is_empty()) {
                out.push_str(&format!("{}\n", header.join(" ")));
            }
            for (pid, name, cmd) in &rows {
                let fields: Vec<String> = headers
                    .iter()
                    .map(|(k, _)| match k.as_str() {
                        "pid" => pid.to_string(),
                        "ppid" => if *pid == SHELL_PID { "0" } else { "1" }.to_string(),
                        "comm" | "ucomm" => name.clone(),
                        "args" | "cmd" | "command" => cmd.clone(),
                        "user" | "uname" => user.clone(),
                        "uid" => "1000".to_string(),
                        "stat" | "s" => "S".to_string(),
                        "tty" | "tt" => "pts/0".to_string(),
                        "time" => "00:00:00".to_string(),
                        _ => "-".to_string(),
                    })
                    .collect();
                out.push_str(&format!("{}\n", fields.join(" ")));
            }
        } else if bsd_user {
            out.push_str(
                "USER         PID %CPU %MEM    VSZ   RSS TTY      STAT START   TIME COMMAND\n",
            );
            for (pid, _, cmd) in &rows {
                out.push_str(&format!(
                    "{:<8} {:>7}  0.0  0.0      0     0 pts/0    S    00:00   0:00 {}\n",
                    user, pid, cmd
                ));
            }
        } else if full {
            out.push_str("UID          PID    PPID  C STIME TTY          TIME CMD\n");
            for (pid, _, cmd) in &rows {
                let ppid = if *pid == SHELL_PID { 0 } else { SHELL_PID };
                out.push_str(&format!(
                    "{:<8} {:>7} {:>7}  0 00:00 pts/0    00:00:00 {}\n",
                    user, pid, ppid, cmd
                ));
            }
        } else {
            out.push_str("    PID TTY          TIME CMD\n");
            for (pid, name, _) in &rows {
                out.push_str(&format!("{:>7} pts/0    00:00:00 {}\n", pid, name));
            }
        }
        Ok(ExecResult::ok(out))
    }
}

/// `pgrep` and `pkill` (selected by `kill_signal`): match jobs by command
/// name (or full command line with `-f`) against a regex.
#[derive(Default)]
pub struct Pgrep {
    kill: bool,
}

impl Pgrep {
    /// `pgrep`.
    pub fn new() -> Self {
        Self { kill: false }
    }

    /// `pkill`.
    pub fn pkill() -> Self {
        Self { kill: true }
    }
}

#[async_trait]
impl Builtin for Pgrep {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let tool = if self.kill { "pkill" } else { "pgrep" };
        let mut full = false;
        let mut list_name = false;
        let mut list_full = false;
        let mut count = false;
        let mut exact = false;
        let mut signal = 15;
        let mut pattern: Option<&str> = None;
        for arg in ctx.args {
            match arg.as_str() {
                "-f" | "--full" => full = true,
                "-l" | "--list-name" => list_name = true,
                "-a" | "--list-full" => list_full = true,
                "-c" | "--count" => count = true,
                "-x" | "--exact" => exact = true,
                a if self.kill && a.starts_with('-') && parse_signal(&a[1..]).is_some() => {
                    signal = parse_signal(&a[1..]).unwrap_or(15);
                }
                a if a.starts_with('-') => {
                    return Ok(ExecResult::err(
                        format!(
                            "{tool}: invalid option -- '{}'\n",
                            a.trim_start_matches('-')
                        ),
                        2,
                    ));
                }
                a => pattern = Some(a),
            }
        }
        let Some(pattern) = pattern else {
            return Ok(ExecResult::err(
                format!("{tool}: no matching criteria specified\n"),
                2,
            ));
        };
        let re = match regex::Regex::new(&if exact {
            format!("^(?:{pattern})$")
        } else {
            pattern.to_string()
        }) {
            Ok(re) => re,
            Err(_) => {
                return Ok(ExecResult::err(
                    format!("{tool}: invalid regular expression\n"),
                    2,
                ));
            }
        };
        let matches: Vec<(u32, String, String)> = process_rows(&ctx)
            .into_iter()
            .filter(|(_, name, cmd)| re.is_match(if full { cmd } else { name }))
            .collect();
        if self.kill
            && let Some(shell) = ctx.shell.as_ref()
        {
            let jobs = shell.jobs();
            let mut table = jobs.lock();
            for (pid, ..) in &matches {
                if *pid != SHELL_PID
                    && let Some(id) = table.resolve(&pid.to_string())
                {
                    table.kill(id, signal);
                }
            }
        }
        let code = if matches.is_empty() { 1 } else { 0 };
        if count {
            return Ok(ExecResult::with_code(format!("{}\n", matches.len()), code));
        }
        if self.kill {
            return Ok(ExecResult::with_code(String::new(), code));
        }
        let mut out = String::new();
        for (pid, name, cmd) in &matches {
            if list_full {
                out.push_str(&format!("{pid} {cmd}\n"));
            } else if list_name {
                out.push_str(&format!("{pid} {name}\n"));
            } else {
                out.push_str(&format!("{pid}\n"));
            }
        }
        Ok(ExecResult::with_code(out, code))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signals_parse() {
        assert_eq!(parse_signal("9"), Some(9));
        assert_eq!(parse_signal("KILL"), Some(9));
        assert_eq!(parse_signal("sigterm"), Some(15));
        assert_eq!(parse_signal("BOGUS"), None);
        assert_eq!(signal_name(15), Some("TERM"));
    }

    #[test]
    fn job_states_render_like_bash() {
        assert_eq!(state_text(&JobState::Done(0)), "Done");
        assert_eq!(state_text(&JobState::Done(3)), "Exit 3");
        assert_eq!(state_text(&JobState::Done(143)), "Terminated");
        assert_eq!(state_text(&JobState::Running), "Running");
    }
}
