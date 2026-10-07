//! awk - pattern scanning and processing builtin (gawk-compatible)
//!
//! Usage:
//!   awk '{print $1}' file
//!   awk -F: -v OFS=- '{$1=$1; print}' /etc/passwd
//!   awk -f prog.awk data.txt
//!   awk 'BEGIN { while (("ls" | getline f) > 0) print f }'
//!
//! Decisions:
//! - The target is gawk 5 (Debian's `awk`): its option set, its error
//!   message shapes (`awk: cmd. line:1: ...` with a caret for syntax
//!   errors), its output for numbers, and extensions agents use (`gensub`,
//!   `asort`, `strftime`, `BEGINFILE`, `switch`, `--csv`, arrays of arrays).
//! - Pipeline: `lexer` -> `parser` (resolves names to slots, `ast`) ->
//!   `interp` (async evaluator) with `io` (records, getline, redirections,
//!   commands) and `funcs` (builtin functions). `regex` translates EREs and
//!   gives POSIX leftmost-longest matches.
//! - Commands run in the sandbox shell through an execution-plan driver.
//!   The driver is used only when the program can run commands (a `|` or
//!   `system` in the arguments, or a `-f` program); otherwise `execute`
//!   runs awk directly and a command attempt fails with a message.
//! - Resource limits (TM-DOS-027/028/033/109/110/116) are enforced in the
//!   parser and evaluator; see the THREAT notes there.

#![allow(clippy::unwrap_used)]

mod ast;
mod format;
mod funcs;
mod interp;
mod io;
mod lexer;
mod order;
mod parser;
mod regex;
mod value;

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::Poll;

use async_trait::async_trait;

use self::ast::{Source, VarRef, special};
use self::interp::{Interp, Limits};
use self::io::{Bridge, BridgeState, Host, Io};
use self::parser::{ParseError, ParseErrorKind, Parser};
use self::value::Value;
use super::{Builtin, Context, Date, ExecutionPlan, PlanDriver, PlanStep, SubCommand};
use crate::StreamData;
use crate::error::Result;
use crate::fs::vfs_join;
use crate::interpreter::ExecResult;
use crate::limits::ExecutionLimits;

/// awk command - pattern scanning and processing
#[derive(Clone, Copy, Default)]
pub struct Awk {
    clock: Date,
}

impl Awk {
    /// awk whose `systime()`, `strftime()` and `mktime()` use `clock`.
    pub fn with_clock(clock: Date) -> Self {
        Self { clock }
    }
}

const USAGE: &str = "\
Usage: awk [POSIX or GNU style options] -f progfile [--] file ...
Usage: awk [POSIX or GNU style options] [--] 'program' file ...
POSIX options:\t\tGNU long options: (standard)
\t-f progfile\t\t--file=progfile
\t-F fs\t\t\t--field-separator=fs
\t-v var=val\t\t--assign=var=val
Short options:\t\tGNU long options: (extensions)
\t-e 'program-text'\t--source='program-text'
\t-k\t\t\t--csv
";

const VERSION: &str = "GNU Awk 5.2.1 (bashkit)";

/// gawk options that change nothing here.
const IGNORED_FLAGS: &[&str] = &[
    "-b",
    "--characters-as-bytes",
    "-c",
    "--traditional",
    "-C",
    "--copyright",
    "-g",
    "--gen-pot",
    "-M",
    "--bignum",
    "-n",
    "--non-decimal-data",
    "-N",
    "--use-lc-numeric",
    "-O",
    "--optimize",
    "-P",
    "--posix",
    "-r",
    "--re-interval",
    "-s",
    "--no-optimize",
    "-S",
    "--sandbox",
    "-t",
    "--lint-old",
    "-L",
    "--lint",
];

/// Parse a CSV record per RFC 4180: quoted fields, embedded commas and
/// doubled quotes.
fn csv_split_fields(line: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut field = String::new();
    let mut in_quotes = false;
    let mut chars = line.chars().peekable();

    while let Some(c) = chars.next() {
        if in_quotes {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    field.push('"');
                    chars.next();
                } else {
                    in_quotes = false;
                }
            } else {
                field.push(c);
            }
        } else if c == '"' {
            in_quotes = true;
        } else if c == ',' {
            fields.push(std::mem::take(&mut field));
        } else {
            field.push(c);
        }
    }
    fields.push(field);
    fields
}

enum ProgSrc {
    File(String),
    Text(String),
}

struct Opts {
    fs: Option<String>,
    assigns: Vec<String>,
    progs: Vec<ProgSrc>,
    csv: bool,
    operands: Vec<String>,
}

fn usage_error(msg: Option<String>) -> Box<ExecResult> {
    let mut err = msg.map(|m| format!("{m}\n")).unwrap_or_default();
    err.push_str(USAGE);
    Box::new(exec_err(err, 1))
}

fn parse_opts(args: &[String]) -> std::result::Result<Opts, Box<ExecResult>> {
    let mut o = Opts {
        fs: None,
        assigns: Vec::new(),
        progs: Vec::new(),
        csv: false,
        operands: Vec::new(),
    };
    let mut i = 0;
    // Option value: attached (`-F:`, `--file=x`) or the next argument.
    let value = |i: &mut usize, attached: Option<&str>, opt: &str| {
        if let Some(v) = attached {
            return Ok(v.to_string());
        }
        *i += 1;
        args.get(*i).cloned().ok_or_else(|| {
            usage_error(Some(format!(
                "awk: option requires an argument -- '{}'",
                opt.trim_start_matches('-')
            )))
        })
    };
    while i < args.len() {
        let arg = args[i].as_str();
        if arg == "--" {
            i += 1;
            break;
        }
        if arg == "-" || !arg.starts_with('-') {
            break;
        }
        if IGNORED_FLAGS.contains(&arg) || arg.starts_with("--lint=") || arg.starts_with("-L") {
            i += 1;
            continue;
        }
        let (name, attached) = if let Some(long) = arg.strip_prefix("--") {
            match long.split_once('=') {
                Some((n, v)) => (format!("--{n}"), Some(v)),
                None => (arg.to_string(), None),
            }
        } else {
            let rest = &arg[2..];
            (arg[..2].to_string(), (!rest.is_empty()).then_some(rest))
        };
        match name.as_str() {
            "-F" | "--field-separator" => {
                let v = value(&mut i, attached, "F")?;
                o.fs = Some(if v == "t" { "\t".to_string() } else { v });
            }
            "-v" | "--assign" => o.assigns.push(value(&mut i, attached, "v")?),
            "-f" | "--file" => o.progs.push(ProgSrc::File(value(&mut i, attached, "f")?)),
            "-e" | "--source" => o.progs.push(ProgSrc::Text(value(&mut i, attached, "e")?)),
            "-k" | "--csv" if attached.is_none() => o.csv = true,
            _ if name.starts_with("--") => {
                return Err(usage_error(Some(format!(
                    "awk: unrecognized option '{arg}'"
                ))));
            }
            _ => {
                let c = arg.chars().nth(1).unwrap_or('-');
                return Err(usage_error(Some(format!("awk: invalid option -- '{c}'"))));
            }
        }
        i += 1;
    }
    let mut rest = args[i..].iter().cloned();
    if o.progs.is_empty() {
        match rest.next() {
            Some(p) => o.progs.push(ProgSrc::Text(p)),
            None => return Err(usage_error(None)),
        }
    }
    o.operands = rest.collect();
    Ok(o)
}

/// gawk's message for a parse error, and the exit status.
fn format_parse_error(src: &str, sources: &[Source], e: &ParseError) -> (String, i32) {
    let pos = e.pos.min(src.len());
    let si = sources.iter().rposition(|s| s.start <= pos).unwrap_or(0);
    let (name, start) = sources
        .get(si)
        .map_or(("cmd. line", 0), |s| (s.name.as_str(), s.start));
    let line_no = src[start..pos].bytes().filter(|&b| b == b'\n').count() + 1;
    let loc = format!("{}:{line_no}", interp::truncate(name, 200));
    match e.kind {
        ParseErrorKind::Syntax => {
            let line_start = src[..pos].rfind('\n').map_or(0, |p| p + 1).max(start);
            let line_end = src[pos..].find('\n').map_or(src.len(), |p| pos + p);
            let line = interp::truncate(&src[line_start..line_end], 400);
            let col = src[line_start..pos]
                .chars()
                .count()
                .min(line.chars().count());
            (
                format!(
                    "awk: {loc}: {line}\nawk: {loc}: {}^ {}\n",
                    " ".repeat(col),
                    interp::truncate(&e.msg, 300)
                ),
                1,
            )
        }
        ParseErrorKind::Error => (format!("awk: {loc}: error: {}\n", e.msg), 1),
        ParseErrorKind::Fatal => (format!("awk: {loc}: fatal: {}\n", e.msg), 2),
        ParseErrorKind::Limit => (format!("awk: fatal: {}\n", e.msg), 2),
    }
}

fn exec_err(stderr: String, code: i32) -> ExecResult {
    let mut r = ExecResult::with_code(String::new(), code);
    r.stderr = stderr.into();
    r
}

fn limits_from(ctx: &Context<'_>) -> Limits {
    let (max_loop, max_total_loop, max_live) = ctx
        .execution_extension::<ExecutionLimits>()
        .and_then(|limits| {
            limits
                .try_with(|l| {
                    (
                        l.max_loop_iterations,
                        l.max_total_loop_iterations,
                        l.max_live_intermediate_bytes,
                    )
                })
                .ok()
        })
        .unwrap_or_else(|| {
            let d = ExecutionLimits::default();
            (
                d.max_loop_iterations,
                d.max_total_loop_iterations,
                d.max_live_intermediate_bytes,
            )
        });
    Limits {
        max_loop,
        max_total_loop,
        max_mem: usize::try_from(max_live).unwrap_or(usize::MAX),
    }
}

enum Setup {
    Done(Box<ExecResult>),
    Run(Box<Interp>),
}

impl Awk {
    /// Parse arguments and the program and build a ready interpreter.
    async fn setup(&self, ctx: &Context<'_>, bridge: Option<Bridge>) -> Result<Setup> {
        if let Some(r) = super::check_help_version(ctx.args, USAGE, Some(VERSION)) {
            return Ok(Setup::Done(Box::new(r)));
        }
        let opts = match parse_opts(ctx.args) {
            Ok(o) => o,
            Err(r) => return Ok(Setup::Done(r)),
        };

        // Join all program sources; each keeps its name for messages.
        let mut program = String::new();
        let mut sources = Vec::new();
        for p in &opts.progs {
            if !program.is_empty() {
                program.push('\n');
            }
            let (name, text) = match p {
                ProgSrc::Text(t) => ("cmd. line".to_string(), t.clone()),
                ProgSrc::File(f) => {
                    let path = if f.starts_with('/') {
                        std::path::PathBuf::from(f)
                    } else {
                        vfs_join(ctx.cwd, f)
                    };
                    match ctx.fs.read_file(&path).await {
                        Ok(bytes) => {
                            ctx.consume_budget_input(bytes.len())?;
                            (f.clone(), String::from_utf8_lossy(&bytes).into_owned())
                        }
                        Err(e) => {
                            return Ok(Setup::Done(Box::new(exec_err(
                                format!(
                                    "awk: fatal: can't open source file `{}' for reading: {}\n",
                                    interp::truncate(f, 200),
                                    crate::error::io_error_reason(&e)
                                ),
                                2,
                            ))));
                        }
                    }
                }
            };
            sources.push(Source {
                name,
                start: program.len(),
            });
            program.push_str(&text);
        }

        let prog = match Parser::new(&program, &sources).parse_program() {
            Ok(p) => p,
            Err(e) => {
                let (msg, code) = format_parse_error(&program, &sources, &e);
                return Ok(Setup::Done(Box::new(exec_err(msg, code))));
            }
        };

        let mut env: Vec<(String, String)> = ctx
            .env
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        env.sort();
        let host = Host {
            fs: ctx.fs.clone(),
            cwd: ctx.cwd.clone(),
            env,
            stdin: ctx.stdin.map(|s| s.text_lossy().into_owned()),
            budget: ctx
                .execution_budget()
                .and_then(|b| b.try_with(Clone::clone).ok()),
            clock: self.clock,
            bridge,
        };
        let mut interp = match Interp::new(Arc::new(prog), Io::new(host), limits_from(ctx)) {
            Ok(i) => Box::new(i),
            Err(msg) => {
                return Ok(Setup::Done(Box::new(exec_err(
                    format!("awk: fatal: {msg}\n"),
                    2,
                ))));
            }
        };

        if let Err(code) = Self::init_globals(&mut interp, &opts) {
            return Ok(Setup::Done(Box::new(Self::result(&mut interp, code)?)));
        }
        Ok(Setup::Run(interp))
    }

    /// ENVIRON, ARGV, ARGC, PROCINFO, `-F`, `--csv` and `-v` assignments.
    fn init_globals(interp: &mut Interp, opts: &Opts) -> std::result::Result<(), i32> {
        let fail = |interp: &mut Interp| if interp.fatal { 2 } else { 1 };
        let env = interp.io.host.env.clone();
        let Ok(arr) = interp.array_of_var(VarRef::Global(special::ENVIRON)) else {
            return Err(fail(interp));
        };
        for (k, v) in env {
            interp.elem_store(arr, k, interp::Elem::Val(Value::from_input(v)));
        }
        let Ok(arr) = interp.array_of_var(VarRef::Global(special::ARGV)) else {
            return Err(fail(interp));
        };
        interp.elem_store(arr, "0".into(), interp::Elem::Val(Value::Str("awk".into())));
        for (i, a) in opts.operands.iter().enumerate() {
            interp.elem_store(
                arr,
                (i + 1).to_string(),
                interp::Elem::Val(Value::from_input(a.clone())),
            );
        }
        let argc = (opts.operands.len() + 1) as f64;
        let Ok(arr) = interp.array_of_var(VarRef::Global(special::PROCINFO)) else {
            return Err(fail(interp));
        };
        for (k, v) in [
            ("version", "5.2.1"),
            ("strftime", "%a %b %e %H:%M:%S %Z %Y"),
            ("FS", "FS"),
            ("platform", "posix"),
        ] {
            interp.elem_store(arr, k.into(), interp::Elem::Val(Value::Str(v.into())));
        }
        let set = |interp: &mut Interp, slot: u32, v: Value| {
            interp
                .set_var(VarRef::Global(slot), v)
                .map_err(|_| fail(interp))
        };
        set(interp, special::ARGC, Value::Num(argc))?;
        if opts.csv {
            interp.csv = true;
            set(interp, special::FS, Value::Str(",".into()))?;
        }
        if let Some(fs) = &opts.fs {
            set(interp, special::FS, Value::Str(lexer::unescape(fs)))?;
        }
        for a in &opts.assigns {
            let Some((name, val)) = io::cmdline_assignment(a) else {
                let msg = format!(
                    "`{}' argument to `-v' not in `var=value' form",
                    interp::truncate(a, 200)
                );
                interp.fatal(&msg);
                return Err(2);
            };
            let v = Value::from_input(lexer::unescape(val));
            interp.assign_by_name(name, v).map_err(|_| fail(interp))?;
        }
        Ok(())
    }

    fn result(interp: &mut Interp, code: i32) -> Result<ExecResult> {
        if let Some(e) = interp.budget_error.take() {
            return Err(e.into());
        }
        let mut r = ExecResult::with_code(std::mem::take(&mut interp.io.stdout), code);
        r.stderr = std::mem::take(&mut interp.io.stderr).into();
        Ok(r)
    }

    /// Whether the program may run shell commands.
    fn may_run_commands(args: &[String]) -> bool {
        args.iter().any(|a| {
            a == "-f"
                || a.starts_with("--file")
                || (a.starts_with("-f") && !a.starts_with("-F"))
                || a.contains("system")
                || a.replace("||", "").contains('|')
        })
    }
}

#[async_trait]
impl Builtin for Awk {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        match self.setup(&ctx, None).await? {
            Setup::Done(r) => Ok(*r),
            Setup::Run(mut interp) => {
                let code = interp.run().await;
                Self::result(&mut interp, code)
            }
        }
    }

    async fn execution_plan(&self, ctx: &Context<'_>) -> Result<Option<ExecutionPlan>> {
        if !Self::may_run_commands(ctx.args) {
            return Ok(None);
        }
        let bridge: Bridge = Arc::new(Mutex::new(BridgeState::default()));
        let mut interp = match self.setup(ctx, Some(bridge.clone())).await? {
            Setup::Done(r) => {
                return Ok(Some(ExecutionPlan::Driver(Box::new(AwkRun::done(
                    bridge, *r,
                )))));
            }
            Setup::Run(i) => i,
        };
        let fut = async move {
            let code = interp.run().await;
            let emitted = interp.io.emitted;
            Self::result(&mut interp, code).map(|r| (r, emitted))
        };
        Ok(Some(ExecutionPlan::Driver(Box::new(AwkRun {
            fut: Some(Box::pin(fut)),
            bridge,
            queued: None,
        }))))
    }
}

type RunFuture = Pin<Box<dyn Future<Output = Result<(ExecResult, (usize, usize))>> + Send>>;

/// Plan driver: runs the awk program and fulfils its command requests.
struct AwkRun {
    fut: Option<RunFuture>,
    bridge: Bridge,
    queued: Option<PlanStep>,
}

impl AwkRun {
    fn done(bridge: Bridge, r: ExecResult) -> Self {
        AwkRun {
            fut: None,
            bridge,
            queued: Some(PlanStep::Done(r)),
        }
    }
}

#[async_trait]
impl PlanDriver for AwkRun {
    async fn next(&mut self, last: Option<ExecResult>) -> Result<PlanStep> {
        if let Some(r) = last {
            if let Ok(mut s) = self.bridge.lock() {
                s.response = Some(r);
            }
        } else if let Some(step) = self.queued.take() {
            return Ok(step);
        }
        let Some(fut) = self.fut.as_mut() else {
            return Ok(PlanStep::Done(ExecResult::default()));
        };
        let bridge = self.bridge.clone();
        // Ready(Some) when awk finished, Ready(None) when it waits on a
        // command; real I/O pending inside awk keeps us pending.
        let outcome = std::future::poll_fn(|cx| match fut.as_mut().poll(cx) {
            Poll::Ready(r) => Poll::Ready(Some(r)),
            Poll::Pending => {
                if bridge.lock().is_ok_and(|s| s.request.is_some()) {
                    Poll::Ready(None)
                } else {
                    Poll::Pending
                }
            }
        })
        .await;
        match outcome {
            Some(res) => {
                self.fut = None;
                let (r, emitted) = res?;
                // Stream what awk printed since the last command.
                let out = r.stdout.as_bytes().get(emitted.0..).unwrap_or_default();
                let err = r.stderr.as_bytes().get(emitted.1..).unwrap_or_default();
                if emitted == (0, 0) || (out.is_empty() && err.is_empty()) {
                    return Ok(PlanStep::Done(r));
                }
                let step = PlanStep::Emit {
                    stdout: StreamData::from(out.to_vec()),
                    stderr: StreamData::from(err.to_vec()),
                };
                self.queued = Some(PlanStep::Done(r));
                Ok(step)
            }
            None => {
                let Some(req) = self.bridge.lock().ok().and_then(|mut s| s.request.take()) else {
                    return Ok(PlanStep::Done(ExecResult::default()));
                };
                let command = SubCommand {
                    name: "sh".to_string(),
                    args: vec!["-c".to_string(), req.command],
                    stdin: req.stdin.map(StreamData::from),
                    assignments: Vec::new(),
                };
                let run = if req.capture {
                    PlanStep::Capture { command, cwd: None }
                } else {
                    PlanStep::Run { command, cwd: None }
                };
                if req.emit_stdout.is_empty() && req.emit_stderr.is_empty() {
                    return Ok(run);
                }
                self.queued = Some(run);
                Ok(PlanStep::Emit {
                    stdout: StreamData::from(req.emit_stdout),
                    stderr: StreamData::from(req.emit_stderr),
                })
            }
        }
    }
}

#[cfg(test)]
mod tests;
