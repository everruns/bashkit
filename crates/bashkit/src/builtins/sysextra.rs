//! Small system commands agents reach for: arch, groups, tty, logname,
//! uptime, free, getconf, sync, hostid, users, who, nohup, nice, flock.
//!
//! Decision: every value is synthetic and matches the rest of the virtual
//! identity (`uname -m`, `id`, `/proc/meminfo`, `nproc`), never the host
//! (TM-INF-008). Commands that only change how a program runs on a real
//! kernel (`nohup`, `nice`, `flock`) run their command through an
//! [`ExecutionPlan`]; there is no scheduling priority or second process to
//! lock against in one interpreter.

use async_trait::async_trait;

use super::{Builtin, Context, ExecutionPlan, SubCommand};
use crate::error::Result;
use crate::interpreter::ExecResult;

/// Machine name reported by `arch` and `uname -m`.
pub const VIRTUAL_ARCH: &str = "x86_64";

/// Memory size reported by `free` and `/proc/meminfo`, in KiB.
pub const VIRTUAL_MEM_TOTAL_KB: u64 = 4_194_304;
/// Free memory reported by `free` and `/proc/meminfo`, in KiB.
pub const VIRTUAL_MEM_FREE_KB: u64 = 3_145_728;

fn help(ctx: &Context<'_>, name: &str, usage: &str) -> Option<ExecResult> {
    super::check_help_version(ctx.args, usage, Some(&format!("{name} (bashkit) 0.1")))
}

/// Reject any option or operand for commands that take none.
fn no_args(ctx: &Context<'_>, name: &str) -> Option<ExecResult> {
    let arg = ctx.args.first()?;
    if arg.starts_with('-') && arg.len() > 1 {
        Some(super::invalid_option(name, arg, 1))
    } else {
        Some(ExecResult::err(
            format!("{name}: extra operand '{arg}'\n"),
            1,
        ))
    }
}

/// `arch` - print the machine hardware name.
pub struct Arch;

#[async_trait]
impl Builtin for Arch {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        if let Some(r) = help(&ctx, "arch", "Usage: arch\nPrint machine architecture.\n") {
            return Ok(r);
        }
        if let Some(r) = no_args(&ctx, "arch") {
            return Ok(r);
        }
        Ok(ExecResult::ok(format!("{VIRTUAL_ARCH}\n")))
    }
}

/// `groups` and `logname`: answers from the virtual user.
pub struct UserInfo {
    kind: UserInfoKind,
    username: String,
}

#[derive(Clone, Copy)]
enum UserInfoKind {
    Groups,
    Logname,
    Users,
    Who,
}

impl UserInfo {
    /// `groups [USER]`: the user's only group shares its name.
    pub fn groups(username: &str) -> Self {
        Self {
            kind: UserInfoKind::Groups,
            username: username.to_string(),
        }
    }

    /// `logname`: the login name.
    pub fn logname(username: &str) -> Self {
        Self {
            kind: UserInfoKind::Logname,
            username: username.to_string(),
        }
    }

    /// `users`: logged-in users. Nobody is logged in to a sandbox, like a
    /// container without utmp, so the list is empty.
    pub fn users() -> Self {
        Self {
            kind: UserInfoKind::Users,
            username: String::new(),
        }
    }

    /// `who`: login sessions; none in a sandbox.
    pub fn who() -> Self {
        Self {
            kind: UserInfoKind::Who,
            username: String::new(),
        }
    }
}

#[async_trait]
impl Builtin for UserInfo {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        match self.kind {
            UserInfoKind::Groups => {
                if let Some(r) = help(
                    &ctx,
                    "groups",
                    "Usage: groups [USER]...\nPrint group memberships for each USERNAME or the current user.\n",
                ) {
                    return Ok(r);
                }
                if ctx.args.is_empty() {
                    return Ok(ExecResult::ok(format!("{}\n", self.username)));
                }
                let mut out = String::new();
                let mut err = String::new();
                for user in ctx.args {
                    if user == &self.username {
                        out.push_str(&format!("{user} : {user}\n"));
                    } else {
                        err.push_str(&format!("groups: '{user}': no such user\n"));
                    }
                }
                let mut r = ExecResult::with_code(out, i32::from(!err.is_empty()));
                r.stderr = err.into();
                Ok(r)
            }
            UserInfoKind::Logname => {
                if let Some(r) = help(
                    &ctx,
                    "logname",
                    "Usage: logname\nPrint the user's login name.\n",
                ) {
                    return Ok(r);
                }
                if let Some(r) = no_args(&ctx, "logname") {
                    return Ok(r);
                }
                Ok(ExecResult::ok(format!("{}\n", self.username)))
            }
            UserInfoKind::Users => {
                if let Some(r) = help(
                    &ctx,
                    "users",
                    "Usage: users [FILE]\nOutput who is currently logged in.\n",
                ) {
                    return Ok(r);
                }
                Ok(ExecResult::ok(String::new()))
            }
            UserInfoKind::Who => {
                if let Some(r) = help(
                    &ctx,
                    "who",
                    "Usage: who [OPTION]... [ FILE | ARG1 ARG2 ]\nPrint information about users who are currently logged in.\n",
                ) {
                    return Ok(r);
                }
                Ok(ExecResult::ok(String::new()))
            }
        }
    }
}

/// `tty` - stdin is a terminal only when the embedder says so (`tty(0, true)`).
pub struct Tty;

#[async_trait]
impl Builtin for Tty {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        if let Some(r) = help(
            &ctx,
            "tty",
            "Usage: tty [OPTION]...\nPrint the file name of the terminal connected to standard input.\n\n  -s, --silent, --quiet\tprint nothing, only return an exit status\n",
        ) {
            return Ok(r);
        }
        let mut silent = false;
        for arg in ctx.args {
            match arg.as_str() {
                "-s" | "--silent" | "--quiet" => silent = true,
                a if a.starts_with('-') => return Ok(super::invalid_option("tty", a, 2)),
                a => return Ok(ExecResult::err(format!("tty: extra operand '{a}'\n"), 2)),
            }
        }
        // Same switch as `[ -t 0 ]` (`BashBuilder::tty(0, true)`).
        let stdin_is_tty = ctx
            .variables
            .get("_TTY_0")
            .or_else(|| ctx.env.get("_TTY_0"))
            .is_some_and(|v| v == "1");
        if stdin_is_tty {
            return Ok(ExecResult::ok(if silent { "" } else { "/dev/pts/0\n" }));
        }
        Ok(ExecResult::with_code(
            if silent { "" } else { "not a tty\n" },
            1,
        ))
    }
}

/// `uptime` - the virtual machine just booted.
pub struct Uptime {
    clock: super::Date,
}

impl Uptime {
    /// Report wall time from the shell's virtual clock.
    pub fn with_clock(clock: super::Date) -> Self {
        Self { clock }
    }
}

#[async_trait]
impl Builtin for Uptime {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        if let Some(r) = help(
            &ctx,
            "uptime",
            "Usage: uptime [options]\nTell how long the system has been running.\n\n  -p, --pretty\tshow uptime in pretty format\n  -s, --since\tsystem up since\n",
        ) {
            return Ok(r);
        }
        let (secs, _) = self.clock.now_epoch();
        let now = chrono::DateTime::from_timestamp(secs, 0).unwrap_or_default();
        match ctx.args.first().map(String::as_str) {
            None => Ok(ExecResult::ok(format!(
                " {} up 0 min,  0 users,  load average: 0.00, 0.00, 0.00\n",
                now.format("%H:%M:%S")
            ))),
            Some("-p" | "--pretty") => Ok(ExecResult::ok("up 0 minutes\n".to_string())),
            Some("-s" | "--since") => Ok(ExecResult::ok(format!(
                "{}\n",
                now.format("%Y-%m-%d %H:%M:%S")
            ))),
            Some(a) => Ok(super::invalid_option("uptime", a, 1)),
        }
    }
}

/// `free` - memory from the same virtual numbers as `/proc/meminfo`.
pub struct Free;

#[async_trait]
impl Builtin for Free {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        if let Some(r) = help(
            &ctx,
            "free",
            "Usage: free [options]\nDisplay amount of free and used memory in the system.\n\n  -b, -k, -m, -g\tshow output in bytes, KiB, MiB, GiB\n  -h, --human\tshow human-readable output\n  -t, --total\tshow total for RAM + swap\n",
        ) {
            return Ok(r);
        }
        // Divisor from KiB, or None for human-readable.
        let mut unit: Option<(u64, bool)> = Some((1, false));
        let mut total = false;
        for arg in ctx.args {
            match arg.as_str() {
                "-b" | "--bytes" => unit = Some((1, true)),
                "-k" | "--kibi" => unit = Some((1, false)),
                "-m" | "--mebi" => unit = Some((1024, false)),
                "-g" | "--gibi" => unit = Some((1024 * 1024, false)),
                "-h" | "--human" => unit = None,
                "-t" | "--total" => total = true,
                "-w" | "--wide" | "-l" | "--lohi" => {}
                a => return Ok(super::invalid_option("free", a, 1)),
            }
        }
        let fmt = |kb: u64| -> String {
            match unit {
                Some((_, true)) => (kb * 1024).to_string(),
                Some((div, false)) => (kb / div).to_string(),
                None => human_kb(kb),
            }
        };
        let used = VIRTUAL_MEM_TOTAL_KB - VIRTUAL_MEM_FREE_KB;
        let mut out = format!(
            "{:>15} {:>11} {:>11} {:>11} {:>11} {:>11}\n",
            "total", "used", "free", "shared", "buff/cache", "available"
        );
        out.push_str(&format!(
            "Mem:  {:>11} {:>11} {:>11} {:>11} {:>11} {:>11}\n",
            fmt(VIRTUAL_MEM_TOTAL_KB),
            fmt(used),
            fmt(VIRTUAL_MEM_FREE_KB),
            fmt(0),
            fmt(0),
            fmt(VIRTUAL_MEM_FREE_KB)
        ));
        out.push_str(&format!(
            "Swap: {:>11} {:>11} {:>11}\n",
            fmt(0),
            fmt(0),
            fmt(0)
        ));
        if total {
            out.push_str(&format!(
                "Total:{:>11} {:>11} {:>11}\n",
                fmt(VIRTUAL_MEM_TOTAL_KB),
                fmt(used),
                fmt(VIRTUAL_MEM_FREE_KB)
            ));
        }
        Ok(ExecResult::ok(out))
    }
}

/// procps-style human size of a KiB count (`4.0Gi`, `1.0Gi`, `0B`).
fn human_kb(kb: u64) -> String {
    if kb == 0 {
        return "0B".to_string();
    }
    let units = ["Ki", "Mi", "Gi", "Ti"];
    let mut value = kb as f64;
    let mut i = 0;
    while value >= 1024.0 && i < units.len() - 1 {
        value /= 1024.0;
        i += 1;
    }
    if value < 10.0 {
        format!("{value:.1}{}", units[i])
    } else {
        format!("{}{}", value.round() as u64, units[i])
    }
}

/// `getconf NAME` - system configuration values.
pub struct Getconf;

/// Values `getconf` knows, consistent with `nproc` and `/proc/meminfo`.
fn getconf_value(name: &str) -> Option<String> {
    let page = 4096u64;
    Some(match name {
        "_NPROCESSORS_ONLN" | "_NPROCESSORS_CONF" | "NPROCESSORS_ONLN" | "NPROCESSORS_CONF" => {
            super::VIRTUAL_NPROC.to_string()
        }
        "PAGESIZE" | "PAGE_SIZE" | "_SC_PAGESIZE" => page.to_string(),
        "_PHYS_PAGES" | "PHYS_PAGES" => (VIRTUAL_MEM_TOTAL_KB * 1024 / page).to_string(),
        "_AVPHYS_PAGES" | "AVPHYS_PAGES" => (VIRTUAL_MEM_FREE_KB * 1024 / page).to_string(),
        "ARG_MAX" => "2097152".to_string(),
        "CHILD_MAX" => "63704".to_string(),
        "CLK_TCK" => "100".to_string(),
        "OPEN_MAX" => "1024".to_string(),
        "LONG_BIT" => "64".to_string(),
        "WORD_BIT" => "32".to_string(),
        "CHAR_BIT" => "8".to_string(),
        "INT_MAX" => i32::MAX.to_string(),
        "UINT_MAX" => u32::MAX.to_string(),
        "LINE_MAX" => "2048".to_string(),
        "HOST_NAME_MAX" => "64".to_string(),
        "LOGIN_NAME_MAX" => "256".to_string(),
        "PATH_MAX" => "4096".to_string(),
        "NAME_MAX" => "255".to_string(),
        "PIPE_BUF" => "4096".to_string(),
        "GNU_LIBC_VERSION" => "glibc 2.36".to_string(),
        "PATH" | "CS_PATH" => "/bin:/usr/bin".to_string(),
        _ => return None,
    })
}

#[async_trait]
impl Builtin for Getconf {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        if let Some(r) = help(
            &ctx,
            "getconf",
            "Usage: getconf [-a] VARIABLE [PATH]\nQuery system configuration variables.\n",
        ) {
            return Ok(r);
        }
        match ctx.args.first().map(String::as_str) {
            None => Ok(ExecResult::err(
                "Usage: getconf [-v specification] variable_name [pathname]\n       getconf -a [pathname]\n"
                    .to_string(),
                1,
            )),
            Some("-a") => {
                let names = [
                    "ARG_MAX",
                    "CHILD_MAX",
                    "CLK_TCK",
                    "HOST_NAME_MAX",
                    "LINE_MAX",
                    "LOGIN_NAME_MAX",
                    "LONG_BIT",
                    "NAME_MAX",
                    "OPEN_MAX",
                    "PAGESIZE",
                    "PATH_MAX",
                    "PIPE_BUF",
                    "_AVPHYS_PAGES",
                    "_NPROCESSORS_CONF",
                    "_NPROCESSORS_ONLN",
                    "_PHYS_PAGES",
                ];
                let mut out = String::new();
                for n in names {
                    if let Some(v) = getconf_value(n) {
                        out.push_str(&format!("{n:<32}{v}\n"));
                    }
                }
                Ok(ExecResult::ok(out))
            }
            Some(name) => match getconf_value(name) {
                Some(v) => Ok(ExecResult::ok(format!("{v}\n"))),
                None => Ok(ExecResult::err(
                    format!("getconf: Unrecognized variable `{name}'\n"),
                    2,
                )),
            },
        }
    }
}

/// `sync` - the VFS has no write-back cache; succeeds.
pub struct SyncCmd;

#[async_trait]
impl Builtin for SyncCmd {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        if let Some(r) = help(
            &ctx,
            "sync",
            "Usage: sync [OPTION] [FILE]...\nSynchronize cached writes to persistent storage.\n",
        ) {
            return Ok(r);
        }
        for arg in ctx.args {
            match arg.as_str() {
                "-d" | "--data" | "-f" | "--file-system" => {}
                a if a.starts_with('-') => return Ok(super::invalid_option("sync", a, 1)),
                file => {
                    let path = super::resolve_path(ctx.cwd, file);
                    if !ctx.fs.exists(&path).await.unwrap_or(false) {
                        return Ok(ExecResult::err(
                            format!("sync: error opening '{file}': No such file or directory\n"),
                            1,
                        ));
                    }
                }
            }
        }
        Ok(ExecResult::ok(String::new()))
    }
}

/// `hostid` - fixed identifier (Linux derives it from 127.0.1.1 when
/// `/etc/hostid` is missing).
pub struct Hostid;

#[async_trait]
impl Builtin for Hostid {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        if let Some(r) = help(
            &ctx,
            "hostid",
            "Usage: hostid\nPrint the numeric identifier for the current host.\n",
        ) {
            return Ok(r);
        }
        if let Some(r) = no_args(&ctx, "hostid") {
            return Ok(r);
        }
        Ok(ExecResult::ok("007f0101\n".to_string()))
    }
}

/// `nohup`, `nice` and `flock`: run a command unchanged.
pub struct RunAs {
    kind: RunAsKind,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RunAsKind {
    Nohup,
    Nice,
    Flock,
}

impl RunAs {
    /// `nohup COMMAND [ARG]...`
    pub fn nohup() -> Self {
        Self {
            kind: RunAsKind::Nohup,
        }
    }
    /// `nice [-n N] [COMMAND [ARG]...]`
    pub fn nice() -> Self {
        Self {
            kind: RunAsKind::Nice,
        }
    }
    /// `flock [OPTIONS] FILE|FD [COMMAND [ARG]... | -c COMMAND]`
    pub fn flock() -> Self {
        Self {
            kind: RunAsKind::Flock,
        }
    }

    fn name(&self) -> &'static str {
        match self.kind {
            RunAsKind::Nohup => "nohup",
            RunAsKind::Nice => "nice",
            RunAsKind::Flock => "flock",
        }
    }
}

/// What a runner builtin resolved its arguments to.
enum Run {
    /// Run this command.
    Command(String, Vec<String>),
    /// Finish with this result (usage error, `nice` alone, `flock FD`).
    Done(ExecResult),
    /// `flock FILE ...`: create the lock file, then continue with the inner
    /// resolution.
    Lock(String, Box<Run>),
}

fn parse_runner(kind: RunAsKind, args: &[String]) -> Run {
    match kind {
        RunAsKind::Nohup => {
            let args = match args.first().map(String::as_str) {
                Some("--") => &args[1..],
                _ => args,
            };
            match args.split_first() {
                None => Run::Done(ExecResult::err(
                    "nohup: missing operand\nTry 'nohup --help' for more information.\n"
                        .to_string(),
                    125,
                )),
                Some((cmd, rest)) => Run::Command(cmd.clone(), rest.to_vec()),
            }
        }
        RunAsKind::Nice => {
            let mut i = 0;
            let mut adjustment = 10i64;
            while i < args.len() {
                let a = args[i].as_str();
                let value = if a == "-n" || a == "--adjustment" {
                    i += 1;
                    args.get(i).map(String::as_str)
                } else if let Some(v) = a.strip_prefix("--adjustment=") {
                    Some(v)
                } else if let Some(v) = a.strip_prefix("-n") {
                    Some(v)
                } else if a.len() > 1
                    && a.starts_with('-')
                    && a[1..]
                        .trim_start_matches('-')
                        .bytes()
                        .all(|b| b.is_ascii_digit())
                {
                    Some(&a[1..])
                } else if a == "--" {
                    i += 1;
                    break;
                } else {
                    break;
                };
                match value.and_then(|v| v.parse::<i64>().ok()) {
                    Some(n) => adjustment = n,
                    None => {
                        return Run::Done(ExecResult::err(
                            format!("nice: invalid adjustment '{}'\n", value.unwrap_or_default()),
                            125,
                        ));
                    }
                }
                i += 1;
            }
            let _ = adjustment; // no scheduler priority inside one interpreter
            match args[i..].split_first() {
                // `nice` alone prints the current niceness.
                None => Run::Done(ExecResult::ok("0\n".to_string())),
                Some((cmd, rest)) => Run::Command(cmd.clone(), rest.to_vec()),
            }
        }
        RunAsKind::Flock => {
            let mut i = 0;
            while i < args.len() {
                match args[i].as_str() {
                    "-s" | "--shared" | "-x" | "-e" | "--exclusive" | "-u" | "--unlock" | "-n"
                    | "--nb" | "--nonblock" | "-o" | "--close" | "-F" | "--no-fork"
                    | "--verbose" => i += 1,
                    "-w" | "--wait" | "--timeout" | "-E" | "--conflict-exit-code" => i += 2,
                    a if a.starts_with("--wait=")
                        || a.starts_with("--timeout=")
                        || a.starts_with("--conflict-exit-code=") =>
                    {
                        i += 1
                    }
                    a if a.starts_with('-') && a.len() > 1 && a != "-c" => {
                        return Run::Done(super::invalid_option("flock", a, 64));
                    }
                    _ => break,
                }
            }
            let Some(target) = args.get(i) else {
                return Run::Done(ExecResult::err(
                    "flock: not enough arguments\nTry 'flock --help' for more information.\n"
                        .to_string(),
                    64,
                ));
            };
            let rest = &args[i + 1..];
            let inner = match rest.split_first() {
                // `flock FD`: lock an already open descriptor; nothing else
                // can hold it here.
                None if target.bytes().all(|b| b.is_ascii_digit()) => {
                    return Run::Done(ExecResult::ok(String::new()));
                }
                None => {
                    return Run::Done(ExecResult::err(
                        format!("flock: {target}: not a number\n"),
                        64,
                    ));
                }
                Some((c, cmd)) if c == "-c" || c == "--command" => match cmd.first() {
                    Some(script) => {
                        Run::Command("bash".to_string(), vec!["-c".to_string(), script.clone()])
                    }
                    None => {
                        return Run::Done(ExecResult::err(
                            "flock: option requires an argument -- 'c'\n".to_string(),
                            64,
                        ));
                    }
                },
                Some((cmd, cmd_args)) => Run::Command(cmd.clone(), cmd_args.to_vec()),
            };
            Run::Lock(target.clone(), Box::new(inner))
        }
    }
}

async fn create_lock_file(ctx: &Context<'_>, file: &str) -> Result<()> {
    let path = super::resolve_path(ctx.cwd, file);
    if ctx.fs.exists(&path).await.unwrap_or(false) {
        return Ok(());
    }
    ctx.fs.write_file(&path, b"").await
}

#[async_trait]
impl Builtin for RunAs {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let usage = match self.kind {
            RunAsKind::Nohup => {
                "Usage: nohup COMMAND [ARG]...\nRun COMMAND, ignoring hangup signals.\n"
            }
            RunAsKind::Nice => {
                "Usage: nice [OPTION] [COMMAND [ARG]...]\nRun COMMAND with an adjusted niceness.\n\n  -n, --adjustment=N\tadd integer N to the niceness (default 10)\n"
            }
            RunAsKind::Flock => {
                "Usage: flock [options] <file>|<directory> <command> [<argument>...]\n       flock [options] <file>|<directory> -c <command>\n       flock [options] <file descriptor number>\nManage file locks from shell scripts.\n"
            }
        };
        if let Some(r) = help(&ctx, self.name(), usage) {
            return Ok(r);
        }
        // Reached only when there is nothing to run (see execution_plan),
        // or the lock file could not be created.
        match parse_runner(self.kind, ctx.args) {
            Run::Done(r) => Ok(r),
            Run::Lock(file, _) => match create_lock_file(&ctx, &file).await {
                Err(e) => Ok(ExecResult::err(
                    format!("flock: cannot open lock file {file}: {e}\n"),
                    1,
                )),
                Ok(()) => Ok(ExecResult::ok(String::new())),
            },
            Run::Command(..) => Ok(ExecResult::ok(String::new())),
        }
    }

    async fn execution_plan(&self, ctx: &Context<'_>) -> Result<Option<ExecutionPlan>> {
        if ctx.args.iter().any(|a| a == "--help" || a == "--version") {
            return Ok(None);
        }
        let mut run = parse_runner(self.kind, ctx.args);
        if let Run::Lock(file, inner) = run {
            // flock creates the lock file if needed, then runs the command;
            // on failure `execute` reports the error.
            if create_lock_file(ctx, &file).await.is_err() {
                return Ok(None);
            }
            run = *inner;
        }
        match run {
            Run::Command(name, args) => Ok(Some(ExecutionPlan::Batch {
                commands: vec![SubCommand {
                    name,
                    args,
                    stdin: ctx.stdin.cloned(),
                    assignments: Vec::new(),
                }],
            })),
            _ => Ok(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn human_sizes_match_procps() {
        assert_eq!(human_kb(VIRTUAL_MEM_TOTAL_KB), "4.0Gi");
        assert_eq!(human_kb(VIRTUAL_MEM_FREE_KB), "3.0Gi");
        assert_eq!(human_kb(0), "0B");
        assert_eq!(human_kb(512), "512Ki");
    }

    #[test]
    fn getconf_knows_common_names() {
        assert_eq!(getconf_value("_NPROCESSORS_ONLN").as_deref(), Some("4"));
        assert_eq!(getconf_value("PAGESIZE").as_deref(), Some("4096"));
        assert_eq!(getconf_value("LONG_BIT").as_deref(), Some("64"));
        assert_eq!(getconf_value("WORD_BIT").as_deref(), Some("32"));
        assert!(getconf_value("NOPE").is_none());
    }

    #[test]
    fn nice_parses_adjustments() {
        let args = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert!(matches!(
            parse_runner(RunAsKind::Nice, &args(&["-n", "5", "echo", "x"])),
            Run::Command(c, _) if c == "echo"
        ));
        assert!(matches!(
            parse_runner(RunAsKind::Nice, &args(&["-5", "echo"])),
            Run::Command(c, _) if c == "echo"
        ));
        assert!(matches!(
            parse_runner(RunAsKind::Nice, &args(&["-n", "x", "echo"])),
            Run::Done(r) if r.exit_code == 125
        ));
    }
}
