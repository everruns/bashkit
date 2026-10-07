//! `make`: a GNU make subset that runs recipes inside the sandbox.
//!
//! Decisions (see `knowledge/foundations/builtins.md#make`):
//! - The builtin is an [`ExecutionPlan::Driver`]: it parses the makefile,
//!   walks the dependency graph and hands each recipe line to the
//!   interpreter as `sh -c LINE`, deciding the next step from the previous
//!   result and fresh VFS mtimes. No host process is ever involved.
//! - Expansion is synchronous; `$(shell)`, `$(wildcard)` and `include` go
//!   through an answer cache and restart parsing on a miss (see `expand`).
//! - Builds are sequential: `-j` is accepted and ignored.
//! - No built-in implicit rules (as with `make -r`): there is no compiler
//!   in the sandbox for them to call. Gaps are recorded as `L-MAKE-*`.

mod expand;
mod parse;

use crate::time_compat::SystemTime;
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;

use super::{Builtin, Context, ExecutionPlan, PlanDriver, PlanStep, SubCommand};
use crate::StreamData;
use crate::error::Result;
use crate::fs::FileSystem;
use crate::interpreter::ExecResult;
use expand::{Expander, Message, Need, Oracle, Origin, Stop, Var, Vars};
use parse::{Makefile, ParseInput, RecipeLine};

/// `$(MAKE)` recursion cap. Each level nests make, `sh -c` and the
/// interpreter on the caller's stack (~200 KiB in debug builds); 4 levels
/// keep the deepest chain well inside a 2 MiB thread stack (TM-DOS-126).
const MAX_MAKELEVEL: u32 = 4;
/// Targets a single run may visit.
const MAX_TARGETS: usize = 100_000;
/// `$(shell)`/`$(wildcard)`/`include` answers a single run may gather.
const MAX_QUERIES: usize = 10_000;
/// Paths a single `$(wildcard)` may return.
const MAX_GLOB_RESULTS: usize = 100_000;

const VERSION: &str = "GNU Make 4.3\nBuilt for x86_64-pc-linux-gnu\n\
Copyright (C) 1988-2020 Free Software Foundation, Inc.\n\
This is the bashkit sandboxed subset; recipes run in the virtual shell.\n";

const USAGE: &str = "Usage: make [options] [target] ...\n\
Options:\n\
  -B, --always-make           Unconditionally make all targets.\n\
  -C DIRECTORY, --directory=DIRECTORY\n\
                              Change to DIRECTORY before doing anything.\n\
  -e, --environment-overrides\n\
                              Environment variables override makefiles.\n\
  -f FILE, --file=FILE, --makefile=FILE\n\
                              Read FILE as a makefile.\n\
  -i, --ignore-errors         Ignore errors from recipes.\n\
  -j [N], --jobs[=N]          Accepted; recipes run one at a time.\n\
  -k, --keep-going            Keep going when some targets can't be made.\n\
  -n, --just-print, --dry-run, --recon\n\
                              Don't actually run any recipe; just print them.\n\
  -q, --question              Run no recipe; exit status says if up to date.\n\
  -r, --no-builtin-rules      Accepted; there are no built-in rules.\n\
  -s, --silent, --quiet       Don't echo recipes.\n\
  -w, --print-directory       Print the current directory.\n\
  --no-print-directory        Turn off -w, even if it was turned on implicitly.\n\
  -v, --version               Print the version number of make and exit.\n";

/// `make` builtin.
pub struct Make;

#[derive(Default, Clone)]
struct Opts {
    files: Vec<String>,
    chdir: Vec<String>,
    dry_run: bool,
    silent: bool,
    keep_going: bool,
    ignore: bool,
    always: bool,
    question: bool,
    env_override: bool,
    print_dir: Option<bool>,
    goals: Vec<String>,
    cmdline_vars: Vec<(String, String)>,
}

enum Parsed {
    Run(Opts),
    Done(ExecResult),
}

fn parse_args(args: &[String], makeflags: Option<&str>) -> Parsed {
    let mut o = Opts::default();
    let mut all: Vec<String> = Vec::new();
    // MAKEFLAGS from a parent make: `ns -- VAR=val`.
    if let Some(mf) = makeflags {
        let mut it = mf.split_whitespace();
        let mut after_dd = false;
        if let Some(first) = mf.split_whitespace().next()
            && !first.starts_with('-')
            && !first.contains('=')
        {
            it.next();
            for c in first.chars() {
                all.push(format!("-{c}"));
            }
        }
        for w in it {
            if w == "--" {
                after_dd = true;
            } else if after_dd || w.contains('=') {
                if let Some((k, v)) = w.split_once('=') {
                    o.cmdline_vars.push((k.to_string(), v.to_string()));
                }
            } else if w.starts_with('-') {
                all.push(w.to_string());
            }
        }
        // Inherited flags never name goals.
        let inherited = std::mem::take(&mut all);
        if let Some(r) = apply_flags(&mut o, &inherited, true) {
            return Parsed::Done(r);
        }
    }
    all.extend(args.iter().cloned());
    match apply_flags(&mut o, &all, false) {
        None => Parsed::Run(o),
        Some(r) => Parsed::Done(r),
    }
}

/// Value of an option: inline (`-fFILE`, `--file=FILE`) or the next word.
fn take_value(args: &[String], i: &mut usize, inline: &str) -> Option<String> {
    if !inline.is_empty() {
        return Some(inline.to_string());
    }
    if *i < args.len() {
        *i += 1;
        return Some(args[*i - 1].clone());
    }
    None
}

fn missing_value(name: &str) -> ExecResult {
    ExecResult::err(
        format!("make: option requires an argument -- '{name}'\n{USAGE}"),
        2,
    )
}

fn bad_option(opt: &str) -> ExecResult {
    ExecResult::err(format!("make: invalid option -- '{opt}'\n{USAGE}"), 2)
}

/// Apply option words to `o`; `Some(result)` ends `make` right away.
fn apply_flags(o: &mut Opts, args: &[String], inherited: bool) -> Option<ExecResult> {
    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        i += 1;
        if let Some(long) = a.strip_prefix("--") {
            let (name, val) = match long.split_once('=') {
                Some((n, v)) => (n, Some(v)),
                None => (long, None),
            };
            match name {
                "" => continue,
                "help" => return Some(ExecResult::ok(USAGE.to_string())),
                "version" => return Some(ExecResult::ok(VERSION.to_string())),
                "always-make" => o.always = true,
                "directory" => match take_value(args, &mut i, val.unwrap_or("")) {
                    Some(v) => o.chdir.push(v),
                    None => return Some(missing_value("C")),
                },
                "environment-overrides" => o.env_override = true,
                "file" | "makefile" => match take_value(args, &mut i, val.unwrap_or("")) {
                    Some(v) => o.files.push(v),
                    None => return Some(missing_value("f")),
                },
                "ignore-errors" => o.ignore = true,
                "jobs" | "load-average" | "max-load" => {}
                "keep-going" => o.keep_going = true,
                "no-keep-going" | "stop" => o.keep_going = false,
                "just-print" | "dry-run" | "recon" => o.dry_run = true,
                "question" => o.question = true,
                "no-builtin-rules" | "no-builtin-variables" | "warn-undefined-variables" => {}
                "silent" | "quiet" => o.silent = true,
                "no-silent" => o.silent = false,
                "print-directory" => o.print_dir = Some(true),
                "no-print-directory" => o.print_dir = Some(false),
                _ => {
                    return Some(ExecResult::err(
                        format!("make: unrecognized option '{a}'\n{USAGE}"),
                        2,
                    ));
                }
            }
            continue;
        }
        if a.len() > 1 && a.starts_with('-') {
            let chars: Vec<char> = a[1..].chars().collect();
            let mut j = 0;
            while j < chars.len() {
                let c = chars[j];
                j += 1;
                let inline: String = chars[j..].iter().collect();
                match c {
                    'B' => o.always = true,
                    'e' => o.env_override = true,
                    'i' => o.ignore = true,
                    'k' => o.keep_going = true,
                    'S' => o.keep_going = false,
                    'n' => o.dry_run = true,
                    'q' => o.question = true,
                    'r' | 'R' => {}
                    's' => o.silent = true,
                    'w' => o.print_dir = Some(true),
                    'h' => return Some(ExecResult::ok(USAGE.to_string())),
                    'v' => return Some(ExecResult::ok(VERSION.to_string())),
                    'j' | 'l' => {
                        // Optional numeric argument.
                        if inline.chars().all(|d| d.is_ascii_digit() || d == '.') {
                            j = chars.len();
                        }
                        if inline.is_empty()
                            && i < args.len()
                            && args[i].chars().all(|d| d.is_ascii_digit() || d == '.')
                        {
                            i += 1;
                        }
                    }
                    'C' | 'f' | 'o' | 'W' | 'I' => {
                        let v = match take_value(args, &mut i, &inline) {
                            Some(v) => v,
                            None => return Some(missing_value(&c.to_string())),
                        };
                        j = chars.len();
                        match c {
                            'C' => o.chdir.push(v),
                            'f' => o.files.push(v),
                            // -o/-W/-I: accepted, no effect in this subset.
                            _ => {}
                        }
                    }
                    other => {
                        if inherited {
                            continue;
                        }
                        return Some(bad_option(&other.to_string()));
                    }
                }
            }
            continue;
        }
        if let Some((k, v)) = a.split_once('=')
            && !k.is_empty()
            && !k.contains([' ', '\t'])
        {
            o.cmdline_vars.push((k.to_string(), v.to_string()));
            continue;
        }
        if !inherited {
            o.goals.push(a.to_string());
        }
    }
    None
}

#[async_trait]
impl Builtin for Make {
    async fn execute(&self, _ctx: Context<'_>) -> Result<ExecResult> {
        // Unreachable in practice: execution_plan always returns a driver.
        Ok(ExecResult::err(
            "make: needs the interpreter to run recipes\n".to_string(),
            2,
        ))
    }

    async fn execution_plan(&self, ctx: &Context<'_>) -> Result<Option<ExecutionPlan>> {
        let env = |k: &str| ctx.env.get(k).or_else(|| ctx.variables.get(k)).cloned();
        let opts = match parse_args(ctx.args, env("MAKEFLAGS").as_deref()) {
            Parsed::Run(o) => o,
            Parsed::Done(r) => {
                return Ok(Some(ExecutionPlan::Driver(Box::new(DoneDriver(Some(r))))));
            }
        };
        let level: u32 = env("MAKELEVEL")
            .and_then(|v| v.trim().parse().ok())
            .unwrap_or(0);
        let mut dir = ctx.cwd.clone();
        for d in &opts.chdir {
            dir = super::resolve_path(&dir, d);
        }
        let mut env_vars: Vec<(String, String)> = ctx
            .env
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        env_vars.sort();
        let stdin = ctx.stdin.map(|s| s.text_lossy().into_owned());
        Ok(Some(ExecutionPlan::Driver(Box::new(MakeRun::new(
            ctx.fs.clone(),
            dir,
            opts,
            level,
            env_vars,
            stdin,
        )))))
    }

    fn llm_hint(&self) -> Option<&'static str> {
        Some(
            "make: GNU make subset (rules, pattern rules, variables, functions, conditionals, include, -C/-f/-n/-k/-B/-q); recipes run in the sandbox shell, sequentially; no built-in implicit rules or $(eval).",
        )
    }
}

struct DoneDriver(Option<ExecResult>);

#[async_trait]
impl PlanDriver for DoneDriver {
    async fn next(&mut self, _last: Option<ExecResult>) -> Result<PlanStep> {
        Ok(PlanStep::Done(self.0.take().unwrap_or_default()))
    }
}

/// What a resolved target needs.
#[derive(Clone, Default)]
struct Plan {
    prereqs: Vec<String>,
    order_only: Vec<String>,
    recipe: Vec<RecipeLine>,
    stem: String,
}

#[derive(Clone, Copy)]
enum TState {
    Visiting,
    Done {
        rebuilt: bool,
        mtime: Option<SystemTime>,
        ok: bool,
    },
}

struct Frame {
    target: String,
    plan: Plan,
    idx: usize,
    my_mtime: Option<SystemTime>,
    exists: bool,
    any_rebuilt: bool,
    failed_prereq: bool,
    newer: Vec<String>,
    /// Out-of-date decision, once all prerequisites are done.
    decided: Option<bool>,
}

struct Prepared {
    cmd: String,
    echo: bool,
    ignore: bool,
    run_in_dry: bool,
    file: Arc<str>,
    line: usize,
}

struct Running {
    lines: VecDeque<Prepared>,
    current: Option<Prepared>,
}

enum Phase {
    Parse,
    Build,
    Finished,
}

enum Awaiting {
    Nothing,
    Shell(String),
    Recipe,
}

struct MakeRun {
    fs: Arc<dyn FileSystem>,
    dir: PathBuf,
    curdir: String,
    opts: Opts,
    level: u32,
    prog: String,
    env_vars: Vec<(String, String)>,
    stdin: Option<String>,
    oracle: Oracle,
    queries: usize,
    phase: Phase,
    awaiting: Awaiting,
    mk: Makefile,
    makefile_name: Option<String>,
    makefile_text: String,
    stdout: StreamData,
    stderr: StreamData,
    states: HashMap<String, TState>,
    visited: usize,
    stack: Vec<Frame>,
    goals: VecDeque<String>,
    goal: Option<String>,
    goal_commands_at: usize,
    commands: usize,
    running: Option<Running>,
    failed: bool,
    question_outdated: bool,
    exit: Option<i32>,
    entered: bool,
    /// Variables exported to recipes.
    exports: Vec<(String, String)>,
    /// Bytes of `stdout`/`stderr` already streamed (by an `Emit` or by the
    /// sub-command that produced them).
    emitted: (usize, usize),
    /// Step held back while an `Emit` goes first.
    queued: Option<PlanStep>,
}

impl MakeRun {
    fn new(
        fs: Arc<dyn FileSystem>,
        dir: PathBuf,
        opts: Opts,
        level: u32,
        env_vars: Vec<(String, String)>,
        stdin: Option<String>,
    ) -> Self {
        let prog = if level == 0 {
            "make".to_string()
        } else {
            format!("make[{level}]")
        };
        let curdir = dir.to_string_lossy().into_owned();
        Self {
            fs,
            dir,
            curdir,
            opts,
            level,
            prog,
            env_vars,
            stdin,
            oracle: Oracle::default(),
            queries: 0,
            phase: Phase::Parse,
            awaiting: Awaiting::Nothing,
            mk: Makefile::default(),
            makefile_name: None,
            makefile_text: String::new(),
            stdout: StreamData::new(),
            stderr: StreamData::new(),
            states: HashMap::new(),
            visited: 0,
            stack: Vec::new(),
            goals: VecDeque::new(),
            goal: None,
            goal_commands_at: 0,
            commands: 0,
            running: None,
            failed: false,
            question_outdated: false,
            exit: None,
            entered: false,
            exports: Vec::new(),
            emitted: (0, 0),
            queued: None,
        }
    }

    fn path(&self, p: &str) -> PathBuf {
        super::resolve_path(&self.dir, p)
    }

    fn err(&mut self, s: &str) {
        self.stderr.push_str(s);
    }

    fn out(&mut self, s: &str) {
        self.stdout.push_str(s);
    }

    fn print_dir(&self) -> bool {
        match self.opts.print_dir {
            Some(v) => v,
            None => (!self.opts.chdir.is_empty() || self.level > 0) && !self.opts.silent,
        }
    }

    fn fatal_stop(&mut self, stop: Stop) {
        match stop {
            Stop::Fatal {
                loc: Some((f, l)),
                msg,
            } => {
                let s = format!("{f}:{l}: *** {msg}.  Stop.\n");
                self.err(&s);
            }
            Stop::Fatal { loc: None, msg } => {
                let s = format!("{}: *** {msg}.  Stop.\n", self.prog);
                self.err(&s);
            }
            Stop::Need(_) => {}
        }
        self.exit = Some(2);
    }

    fn flush_messages(&mut self, msgs: Vec<Message>) {
        for m in msgs {
            match m {
                Message::Out(s) => self.out(&s),
                Message::Err(s) => self.err(&s),
            }
        }
    }

    fn finish(&mut self) -> ExecResult {
        if self.entered {
            let s = format!("{}: Leaving directory '{}'\n", self.prog, self.curdir);
            self.out(&s);
        }
        self.phase = Phase::Finished;
        let code = self.exit.unwrap_or(if self.opts.question {
            i32::from(self.question_outdated)
        } else if self.failed {
            2
        } else {
            0
        });
        ExecResult {
            stdout: std::mem::take(&mut self.stdout),
            stderr: std::mem::take(&mut self.stderr),
            exit_code: code,
            ..Default::default()
        }
    }

    /// Answer a non-shell query from the VFS. Returns a shell command to run
    /// when the query needs one.
    async fn answer(&mut self, need: Need) -> std::result::Result<Option<String>, Stop> {
        self.queries += 1;
        if self.queries > MAX_QUERIES {
            return expand::fatal(None, "too many $(shell)/$(wildcard)/include queries");
        }
        match need {
            Need::Shell(cmd) => Ok(Some(cmd)),
            Need::Glob(pat) => {
                let Some(found) = self.glob(&pat).await else {
                    return expand::fatal(
                        None,
                        format!("$(wildcard) matched more than {MAX_GLOB_RESULTS} paths"),
                    );
                };
                self.oracle.glob.insert(pat, found);
                Ok(None)
            }
            Need::Read(name) => {
                let text = match self.fs.read_file(&self.path(&name)).await {
                    Ok(b) => Some(String::from_utf8_lossy(&b).into_owned()),
                    Err(_) => None,
                };
                self.oracle.files.insert(name, text);
                Ok(None)
            }
        }
    }

    /// `None` when the pattern matches more than [`MAX_GLOB_RESULTS`] paths.
    async fn glob(&self, pat: &str) -> Option<Vec<String>> {
        if !pat.contains(['*', '?', '[']) {
            return Some(if self.fs.exists(&self.path(pat)).await.unwrap_or(false) {
                vec![pat.to_string()]
            } else {
                Vec::new()
            });
        }
        let absolute = pat.starts_with('/');
        let mut candidates: Vec<String> = vec![if absolute {
            "/".to_string()
        } else {
            String::new()
        }];
        let comps: Vec<&str> = pat.split('/').filter(|c| !c.is_empty()).collect();
        for (ci, comp) in comps.iter().enumerate() {
            let last = ci + 1 == comps.len();
            let mut next = Vec::new();
            for base in &candidates {
                let join = |name: &str| {
                    if base.is_empty() {
                        name.to_string()
                    } else if base.ends_with('/') {
                        format!("{base}{name}")
                    } else {
                        format!("{base}/{name}")
                    }
                };
                if !comp.contains(['*', '?', '[']) {
                    next.push(join(comp));
                    continue;
                }
                let dir = if base.is_empty() { "." } else { base.as_str() };
                let Ok(entries) = self.fs.read_dir(&self.path(dir)).await else {
                    continue;
                };
                for e in entries {
                    if e.name.starts_with('.') && !comp.starts_with('.') {
                        continue;
                    }
                    if !last && !e.metadata.file_type.is_dir() {
                        continue;
                    }
                    if super::ls::glob::glob_match(&e.name, comp) {
                        next.push(join(&e.name));
                    }
                }
            }
            if next.len() > MAX_GLOB_RESULTS {
                return None;
            }
            candidates = next;
        }
        let mut out = Vec::new();
        for c in candidates {
            if self.fs.exists(&self.path(&c)).await.unwrap_or(false) {
                out.push(c);
            }
        }
        out.sort();
        Some(out)
    }

    fn initial_vars(&self) -> Vars {
        let mut vars = Vars::new();
        let mut set = |k: &str, v: &str, origin: Origin| {
            vars.insert(
                k.to_string(),
                Var {
                    value: v.to_string(),
                    recursive: false,
                    origin,
                },
            );
        };
        for (k, v) in &self.env_vars {
            if k == "SHELL" || k == "MAKEFLAGS" || k == "MAKELEVEL" || k.starts_with("_TTY_") {
                continue;
            }
            set(k, v, Origin::Environment);
        }
        for (k, v) in [
            ("MAKE", "make"),
            ("MAKE_VERSION", "4.3"),
            ("SHELL", "/bin/sh"),
            ("CC", "cc"),
            ("CXX", "g++"),
            ("AR", "ar"),
            ("ARFLAGS", "rv"),
            ("RM", "rm -f"),
            ("CO", "co"),
        ] {
            set(k, v, Origin::Default);
        }
        set("CURDIR", &self.curdir, Origin::File);
        set("MAKELEVEL", &self.level.to_string(), Origin::Environment);
        for (k, v) in &self.opts.cmdline_vars {
            vars.insert(
                k.clone(),
                Var {
                    value: v.clone(),
                    recursive: true,
                    origin: Origin::CommandLine,
                },
            );
        }
        vars
    }

    /// Locate and read the makefile (once).
    async fn load_makefile(&mut self) -> std::result::Result<(), Stop> {
        if self.makefile_name.is_some() {
            return Ok(());
        }
        if let Some(f) = self.opts.files.first().cloned() {
            if f == "-" {
                self.makefile_text = self.stdin.clone().unwrap_or_default();
                self.makefile_name = Some("-".to_string());
                return Ok(());
            }
            match self.fs.read_file(&self.path(&f)).await {
                Ok(b) => {
                    self.makefile_text = String::from_utf8_lossy(&b).into_owned();
                    self.makefile_name = Some(f);
                    return Ok(());
                }
                Err(_) => {
                    let s = format!("{}: {f}: No such file or directory\n", self.prog);
                    self.err(&s);
                    return expand::fatal(None, format!("No rule to make target '{f}'"));
                }
            }
        }
        for name in ["GNUmakefile", "makefile", "Makefile"] {
            if let Ok(b) = self.fs.read_file(&self.path(name)).await {
                self.makefile_text = String::from_utf8_lossy(&b).into_owned();
                self.makefile_name = Some(name.to_string());
                return Ok(());
            }
        }
        self.makefile_name = Some(String::new());
        Ok(())
    }

    fn try_parse(&self, partial: &mut Vec<Message>) -> std::result::Result<Makefile, Stop> {
        let name = self.makefile_name.clone().unwrap_or_default();
        let mut vars = self.initial_vars();
        if name.is_empty() {
            // No makefile: only goals that are existing files can succeed.
            return Ok(Makefile {
                vars,
                ..Default::default()
            });
        }
        // Later `-f` files are parsed as if included.
        let mut text = self.makefile_text.clone();
        for extra in self.opts.files.iter().skip(1) {
            text.push_str(&format!("\ninclude {extra}\n"));
        }
        vars.remove("MAKEFILE_LIST");
        parse::parse(
            ParseInput {
                name: &name,
                text: &text,
                vars,
                curdir: &self.curdir,
                env_override: self.opts.env_override,
            },
            &self.oracle,
            partial,
        )
    }

    fn start_build(&mut self) {
        self.phase = Phase::Build;
        let messages = std::mem::take(&mut self.mk.messages);
        self.flush_messages(messages);
        if self.print_dir() {
            let s = format!("{}: Entering directory '{}'\n", self.prog, self.curdir);
            // Directory messages precede everything this make prints.
            let mut out = StreamData::from(s);
            out.append(&self.stdout);
            self.stdout = out;
            self.entered = true;
        }
        let goals: Vec<String> = if self.opts.goals.is_empty() {
            match &self.mk.default_goal {
                Some(g) => vec![g.clone()],
                None => {
                    let msg = if self.makefile_name.as_deref() == Some("") {
                        "No targets specified and no makefile found"
                    } else {
                        "No targets"
                    };
                    self.fatal_stop(Stop::Fatal {
                        loc: None,
                        msg: msg.to_string(),
                    });
                    return;
                }
            }
        } else {
            self.opts.goals.clone()
        };
        self.goals = goals.into();
        // Variables exported to recipes.
        let mut exports: Vec<(String, String)> = Vec::new();
        let mut flags = String::new();
        for (on, c) in [
            (self.opts.always, 'B'),
            (self.opts.env_override, 'e'),
            (self.opts.ignore, 'i'),
            (self.opts.keep_going, 'k'),
            (self.opts.dry_run, 'n'),
            (self.opts.question, 'q'),
            (self.opts.silent, 's'),
        ] {
            if on {
                flags.push(c);
            }
        }
        let mut makeflags = flags;
        if !self.opts.cmdline_vars.is_empty() {
            makeflags.push_str(" --");
            for (k, v) in &self.opts.cmdline_vars {
                makeflags.push_str(&format!(" {k}={v}"));
            }
        }
        exports.push(("MAKEFLAGS".to_string(), makeflags));
        exports.push(("MAKELEVEL".to_string(), (self.level + 1).to_string()));
        let names: Vec<String> = {
            let mut n: Vec<String> = self
                .mk
                .vars
                .iter()
                .filter(|(k, v)| {
                    !self.mk.unexported.contains(*k)
                        && (self.mk.exported.contains(*k)
                            || v.origin == Origin::CommandLine
                            || (self.mk.export_all
                                && matches!(v.origin, Origin::File | Origin::Override)))
                        && is_env_name(k)
                })
                .map(|(k, _)| k.clone())
                .collect();
            // Exported names that were never assigned are exported empty
            // only when they come from the environment; skip the rest.
            n.sort();
            n
        };
        for n in names {
            let oracle = Oracle::default();
            let mut e = Expander::new(&self.mk.vars, &oracle, &self.curdir, None);
            if let Ok(v) = e.lookup(&n) {
                exports.push((n, v));
            }
        }
        self.exports = exports;
        self.phase = Phase::Build;
    }

    async fn resolve(&self, target: &str) -> Option<Plan> {
        let explicit = self.mk.rules.get(target);
        if let Some(r) = explicit
            && r.recipe.as_ref().is_some_and(|rc| !rc.is_empty())
        {
            return Some(Plan {
                prereqs: r.prereqs.clone(),
                order_only: r.order_only.clone(),
                recipe: r.recipe.clone().unwrap_or_default(),
                stem: r.stem.clone().unwrap_or_default(),
            });
        }
        // Pattern rules, in definition order.
        for p in &self.mk.patterns {
            if p.recipe.is_empty() {
                continue;
            }
            for tp in &p.targets {
                let Some(stem) = expand::pattern_match(tp, target) else {
                    continue;
                };
                // `%:` with no prerequisites would match everything.
                if tp == "%" && p.prereqs.is_empty() {
                    continue;
                }
                let prereqs: Vec<String> =
                    p.prereqs.iter().map(|x| x.replacen('%', stem, 1)).collect();
                let mut ok = true;
                for pr in &prereqs {
                    if !(self.mk.rules.contains_key(pr)
                        || self.mk.phony.contains(pr)
                        || self.fs.exists(&self.path(pr)).await.unwrap_or(false))
                    {
                        ok = false;
                        break;
                    }
                }
                if !ok {
                    continue;
                }
                let mut plan = Plan {
                    prereqs,
                    order_only: p
                        .order_only
                        .iter()
                        .map(|x| x.replacen('%', stem, 1))
                        .collect(),
                    recipe: p.recipe.clone(),
                    stem: stem.to_string(),
                };
                if let Some(r) = explicit {
                    plan.prereqs.extend(r.prereqs.iter().cloned());
                    plan.order_only.extend(r.order_only.iter().cloned());
                }
                return Some(plan);
            }
        }
        if let Some(r) = explicit {
            return Some(Plan {
                prereqs: r.prereqs.clone(),
                order_only: r.order_only.clone(),
                recipe: Vec::new(),
                stem: r.stem.clone().unwrap_or_default(),
            });
        }
        if self.mk.phony.contains(target) {
            return Some(Plan {
                ..Default::default()
            });
        }
        None
    }

    async fn mtime(&self, target: &str) -> Option<SystemTime> {
        self.fs
            .stat(&self.path(target))
            .await
            .ok()
            .map(|m| m.modified)
    }

    /// Start visiting `target`. Pushes a frame, or settles it immediately.
    async fn visit(&mut self, target: &str, parent: Option<&str>) -> bool {
        self.visited += 1;
        if self.visited > MAX_TARGETS {
            self.fatal_stop(Stop::Fatal {
                loc: None,
                msg: "too many targets".to_string(),
            });
            return false;
        }
        let plan = self.resolve(target).await;
        let mtime = self.mtime(target).await;
        let Some(plan) = plan else {
            if mtime.is_some() {
                self.states.insert(
                    target.to_string(),
                    TState::Done {
                        rebuilt: false,
                        mtime,
                        ok: true,
                    },
                );
                return true;
            }
            let msg = match parent {
                Some(p) => format!("No rule to make target '{target}', needed by '{p}'"),
                None => format!("No rule to make target '{target}'"),
            };
            if self.opts.keep_going {
                let s = format!("{}: *** {msg}.\n", self.prog);
                self.err(&s);
                self.failed = true;
                self.states.insert(
                    target.to_string(),
                    TState::Done {
                        rebuilt: false,
                        mtime: None,
                        ok: false,
                    },
                );
                return true;
            }
            self.fatal_stop(Stop::Fatal { loc: None, msg });
            return false;
        };
        self.states.insert(target.to_string(), TState::Visiting);
        self.stack.push(Frame {
            target: target.to_string(),
            plan,
            idx: 0,
            exists: mtime.is_some(),
            my_mtime: mtime,
            any_rebuilt: false,
            failed_prereq: false,
            newer: Vec::new(),
            decided: None,
        });
        true
    }

    fn auto_vars(&self, f: &Frame) -> HashMap<String, String> {
        let mut dedup = Vec::new();
        for p in &f.plan.prereqs {
            if !dedup.contains(p) {
                dedup.push(p.clone());
            }
        }
        let newer = if f.exists {
            f.newer.join(" ")
        } else {
            dedup.join(" ")
        };
        HashMap::from([
            ("@".to_string(), f.target.clone()),
            (
                "<".to_string(),
                f.plan.prereqs.first().cloned().unwrap_or_default(),
            ),
            ("^".to_string(), dedup.join(" ")),
            ("+".to_string(), f.plan.prereqs.join(" ")),
            ("?".to_string(), newer),
            ("*".to_string(), f.plan.stem.clone()),
            ("|".to_string(), f.plan.order_only.join(" ")),
        ])
    }

    /// Expand the top frame's recipe. `Err(Some(cmd))` asks for a shell run.
    fn prepare_recipe(&mut self) -> std::result::Result<VecDeque<Prepared>, Stop> {
        let f = self.stack.last().expect("frame");
        let autos = self.auto_vars(f);
        let target = f.target.clone();
        let lines = f.plan.recipe.clone();
        let silent_target =
            self.opts.silent || self.mk.silent_all || self.mk.silent.contains(&target);
        let tv = self.mk.target_vars.get(&target);
        let mut out = VecDeque::new();
        let mut messages = Vec::new();
        let mut expanded = Vec::new();
        for l in &lines {
            let mut e = Expander::new(
                &self.mk.vars,
                &self.oracle,
                &self.curdir,
                Some((l.file.to_string(), l.line)),
            )
            .with_target_vars(tv)
            .with_locals(autos.clone());
            let text = e.expand(&l.text)?;
            messages.append(&mut e.messages);
            expanded.push((text, l));
        }
        self.flush_messages(messages);
        if self.mk.oneshell && !expanded.is_empty() {
            let (first, l) = &expanded[0];
            let (mut echo, mut ignore, mut force) = (true, false, false);
            let body = strip_prefixes(first, &mut echo, &mut ignore, &mut force);
            let mut cmd = body.to_string();
            for (t, _) in &expanded[1..] {
                cmd.push('\n');
                cmd.push_str(t.trim_start_matches(['@', '-', '+']));
            }
            out.push_back(Prepared {
                run_in_dry: force || mentions_make(&cmd),
                cmd,
                echo: echo && !silent_target,
                ignore: ignore || self.opts.ignore || self.mk.ignore_all,
                file: l.file.clone(),
                line: l.line,
            });
            return Ok(out);
        }
        for (text, l) in expanded {
            // One recipe line may expand to several (`define` bodies).
            for piece in text.split('\n') {
                let (mut echo, mut ignore, mut force) = (true, false, false);
                let cmd = strip_prefixes(piece, &mut echo, &mut ignore, &mut force).to_string();
                if cmd.trim().is_empty() {
                    continue;
                }
                out.push_back(Prepared {
                    run_in_dry: force || mentions_make(&l.text),
                    cmd,
                    echo: echo && !silent_target,
                    ignore: ignore || self.opts.ignore || self.mk.ignore_all,
                    file: l.file.clone(),
                    line: l.line,
                });
            }
        }
        Ok(out)
    }

    fn settle_top(&mut self, rebuilt: bool, ok: bool, mtime: Option<SystemTime>) {
        let Some(f) = self.stack.pop() else {
            return;
        };
        self.states
            .insert(f.target.clone(), TState::Done { rebuilt, mtime, ok });
        if let Some(parent) = self.stack.last_mut() {
            let normal = parent.idx <= parent.plan.prereqs.len();
            account(parent, &f.target, rebuilt, mtime, ok, normal);
        } else {
            self.goal_finished(&f, rebuilt, ok);
        }
    }

    fn goal_finished(&mut self, f: &Frame, rebuilt: bool, ok: bool) {
        if !ok {
            if self.opts.keep_going {
                let s = format!(
                    "{}: Target '{}' not remade because of errors.\n",
                    self.prog, f.target
                );
                self.err(&s);
            }
            return;
        }
        if self.opts.question {
            return;
        }
        let ran = self.commands > self.goal_commands_at;
        if !ran && (!rebuilt || f.plan.recipe.is_empty()) {
            let s = if !f.plan.recipe.is_empty() && !self.mk.phony.contains(&f.target) {
                format!("{}: '{}' is up to date.\n", self.prog, f.target)
            } else {
                format!("{}: Nothing to be done for '{}'.\n", self.prog, f.target)
            };
            self.out(&s);
        }
    }

    /// Drive the build until a command must run or everything is done.
    async fn step_build(&mut self) -> Result<Option<PlanStep>> {
        loop {
            if self.exit.is_some() {
                return Ok(Some(PlanStep::Done(self.finish())));
            }
            // Recipe in progress.
            if let Some(run) = self.running.as_mut() {
                if let Some(line) = run.lines.pop_front() {
                    let cmd = line.cmd.clone();
                    let echo = line.echo || (self.opts.dry_run && !line.run_in_dry);
                    let skip = self.opts.dry_run && !line.run_in_dry;
                    run.current = Some(line);
                    if echo {
                        self.out(&format!("{cmd}\n"));
                    }
                    self.commands += 1;
                    if skip {
                        continue;
                    }
                    self.awaiting = Awaiting::Recipe;
                    return Ok(Some(PlanStep::Run {
                        command: SubCommand {
                            name: "sh".to_string(),
                            args: vec!["-c".to_string(), cmd],
                            stdin: None,
                            assignments: self.exports.clone(),
                        },
                        cwd: Some(self.dir.clone()),
                    }));
                }
                self.running = None;
                let target = self
                    .stack
                    .last()
                    .map(|f| f.target.clone())
                    .unwrap_or_default();
                let mtime = self.mtime(&target).await;
                self.settle_top(true, true, mtime);
                continue;
            }
            // Next goal.
            if self.stack.is_empty() {
                let Some(g) = self.goals.pop_front() else {
                    return Ok(Some(PlanStep::Done(self.finish())));
                };
                if let Some(TState::Done { .. }) = self.states.get(&g) {
                    continue;
                }
                self.goal = Some(g.clone());
                self.goal_commands_at = self.commands;
                if self.visit(&g, None).await
                    && self.stack.is_empty()
                    && !self.opts.question
                    && let Some(TState::Done { ok: true, .. }) = self.states.get(&g)
                {
                    let s = format!("{}: Nothing to be done for '{g}'.\n", self.prog);
                    self.out(&s);
                }
                continue;
            }
            let f = self.stack.last_mut().expect("frame");
            let total = f.plan.prereqs.len() + f.plan.order_only.len();
            if f.idx < total {
                let p = if f.idx < f.plan.prereqs.len() {
                    f.plan.prereqs[f.idx].clone()
                } else {
                    f.plan.order_only[f.idx - f.plan.prereqs.len()].clone()
                };
                let normal = f.idx < f.plan.prereqs.len();
                f.idx += 1;
                let parent = f.target.clone();
                match self.states.get(&p).copied() {
                    Some(TState::Done { rebuilt, mtime, ok }) => {
                        let f = self.stack.last_mut().expect("frame");
                        account(f, &p, rebuilt, mtime, ok, normal);
                    }
                    Some(TState::Visiting) => {
                        let s = format!(
                            "{}: Circular {parent} <- {p} dependency dropped.\n",
                            self.prog
                        );
                        self.err(&s);
                    }
                    None => {
                        let depth = self.stack.len();
                        self.visit(&p, Some(&parent)).await;
                        // Settled without a frame (a plain file, or -k error).
                        if self.stack.len() == depth
                            && let Some(TState::Done { rebuilt, mtime, ok }) =
                                self.states.get(&p).copied()
                            && let Some(f) = self.stack.last_mut()
                        {
                            account(f, &p, rebuilt, mtime, ok, normal);
                        }
                    }
                }
                continue;
            }
            // All prerequisites done.
            if f.failed_prereq {
                self.settle_top(false, false, None);
                continue;
            }
            let outdated = match f.decided {
                Some(d) => d,
                None => {
                    let phony = self.mk.phony.contains(&f.target);
                    let d = phony
                        || self.opts.always
                        || !f.exists
                        || f.any_rebuilt
                        || !f.newer.is_empty();
                    f.decided = Some(d);
                    d
                }
            };
            if !outdated {
                let mtime = f.my_mtime;
                self.settle_top(false, true, mtime);
                continue;
            }
            if f.plan.recipe.is_empty() {
                let mtime = f.my_mtime;
                self.settle_top(true, true, mtime);
                continue;
            }
            if self.opts.question {
                self.question_outdated = true;
                self.settle_top(true, true, None);
                continue;
            }
            match self.prepare_recipe() {
                Ok(lines) => {
                    self.running = Some(Running {
                        lines,
                        current: None,
                    });
                }
                Err(Stop::Need(need)) => match self.answer(need).await {
                    Ok(Some(cmd)) => return Ok(Some(self.shell_step(cmd))),
                    Ok(None) => {}
                    Err(stop) => self.fatal_stop(stop),
                },
                Err(stop) => {
                    self.fatal_stop(stop);
                }
            }
        }
    }

    fn shell_step(&mut self, cmd: String) -> PlanStep {
        self.awaiting = Awaiting::Shell(cmd.clone());
        PlanStep::Capture {
            command: SubCommand {
                name: "sh".to_string(),
                args: vec!["-c".to_string(), cmd],
                stdin: None,
                assignments: Vec::new(),
            },
            cwd: Some(self.dir.clone()),
        }
    }

    fn recipe_result(&mut self, r: ExecResult) {
        // The recipe streamed its own output; keep the offsets in step.
        let streamed = (self.stdout.len(), self.stderr.len()) == self.emitted;
        self.stdout.append(&r.stdout);
        self.stderr.append(&r.stderr);
        if streamed {
            self.emitted = (self.stdout.len(), self.stderr.len());
        }
        if r.exit_code == 0 {
            return;
        }
        let target = self
            .stack
            .last()
            .map(|f| f.target.clone())
            .unwrap_or_default();
        let Some(line) = self.running.as_mut().and_then(|run| run.current.take()) else {
            return;
        };
        if line.ignore {
            let s = format!(
                "{}: [{}:{}: {target}] Error {} (ignored)\n",
                self.prog, line.file, line.line, r.exit_code
            );
            self.err(&s);
            return;
        }
        let s = format!(
            "{}: *** [{}:{}: {target}] Error {}\n",
            self.prog, line.file, line.line, r.exit_code
        );
        self.err(&s);
        self.failed = true;
        self.running = None;
        if self.opts.keep_going {
            self.settle_top(false, false, None);
        } else {
            self.exit = Some(2);
        }
    }
}

fn account(
    f: &mut Frame,
    p: &str,
    rebuilt: bool,
    mtime: Option<SystemTime>,
    ok: bool,
    normal: bool,
) {
    if !ok {
        f.failed_prereq = true;
        return;
    }
    if !normal {
        return;
    }
    let newer = match (mtime, f.my_mtime) {
        (Some(pm), Some(tm)) => pm > tm,
        _ => false,
    };
    if rebuilt {
        f.any_rebuilt = true;
    }
    if (rebuilt || newer) && !f.newer.iter().any(|n| n == p) {
        f.newer.push(p.to_string());
    }
}

fn strip_prefixes<'a>(s: &'a str, echo: &mut bool, ignore: &mut bool, force: &mut bool) -> &'a str {
    let mut s = s.trim_start();
    loop {
        match s.chars().next() {
            Some('@') => *echo = false,
            Some('-') => *ignore = true,
            Some('+') => *force = true,
            _ => return s,
        }
        s = s[1..].trim_start();
    }
}

fn mentions_make(s: &str) -> bool {
    s.contains("$(MAKE)") || s.contains("${MAKE}")
}

fn is_env_name(k: &str) -> bool {
    let mut c = k.chars();
    c.next()
        .is_some_and(|f| f.is_ascii_alphabetic() || f == '_')
        && c.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

#[async_trait]
impl PlanDriver for MakeRun {
    async fn next(&mut self, last: Option<ExecResult>) -> Result<PlanStep> {
        if last.is_none()
            && let Some(step) = self.queued.take()
        {
            return Ok(step);
        }
        let step = self.advance(last).await?;
        // Stream make's own lines before the next command (or the end).
        let (out, err) = match &step {
            PlanStep::Done(r) => (r.stdout.as_bytes(), r.stderr.as_bytes()),
            _ => (self.stdout.as_bytes(), self.stderr.as_bytes()),
        };
        let out = out.get(self.emitted.0..).unwrap_or_default().to_vec();
        let err = err.get(self.emitted.1..).unwrap_or_default().to_vec();
        if out.is_empty() && err.is_empty() {
            return Ok(step);
        }
        self.emitted.0 += out.len();
        self.emitted.1 += err.len();
        self.queued = Some(step);
        Ok(PlanStep::Emit {
            stdout: StreamData::from(out),
            stderr: StreamData::from(err),
        })
    }
}

impl MakeRun {
    async fn advance(&mut self, last: Option<ExecResult>) -> Result<PlanStep> {
        if let Some(r) = last {
            match std::mem::replace(&mut self.awaiting, Awaiting::Nothing) {
                Awaiting::Shell(cmd) => {
                    self.stderr.append(&r.stderr);
                    let text = r.stdout.text_lossy();
                    let value = text.trim_end_matches('\n').replace('\n', " ");
                    self.oracle.shell.insert(cmd, value);
                }
                Awaiting::Recipe => self.recipe_result(r),
                Awaiting::Nothing => {}
            }
        }
        loop {
            match self.phase {
                Phase::Finished => return Ok(PlanStep::Done(ExecResult::default())),
                Phase::Parse => {
                    if self.level >= MAX_MAKELEVEL {
                        self.fatal_stop(Stop::Fatal {
                            loc: None,
                            msg: format!("recursive make depth exceeds {MAX_MAKELEVEL}"),
                        });
                        return Ok(PlanStep::Done(self.finish()));
                    }
                    if !self.fs.exists(&self.dir).await.unwrap_or(false) {
                        let s = format!(
                            "{}: *** {}: No such file or directory.  Stop.\n",
                            self.prog,
                            self.opts.chdir.last().cloned().unwrap_or_default()
                        );
                        self.err(&s);
                        self.exit = Some(2);
                        return Ok(PlanStep::Done(self.finish()));
                    }
                    if let Err(stop) = self.load_makefile().await {
                        self.fatal_stop(stop);
                        return Ok(PlanStep::Done(self.finish()));
                    }
                    let mut partial = Vec::new();
                    let parsed = self.try_parse(&mut partial);
                    match parsed {
                        Ok(mk) => {
                            self.mk = mk;
                            self.start_build();
                        }
                        Err(Stop::Need(need)) => match self.answer(need).await {
                            Ok(Some(cmd)) => return Ok(self.shell_step(cmd)),
                            Ok(None) => {}
                            Err(stop) => {
                                self.fatal_stop(stop);
                                return Ok(PlanStep::Done(self.finish()));
                            }
                        },
                        Err(stop) => {
                            // Parse-time output before the error still shows.
                            self.flush_messages(partial);
                            self.fatal_stop(stop);
                            return Ok(PlanStep::Done(self.finish()));
                        }
                    }
                }
                Phase::Build => {
                    if let Some(step) = self.step_build().await? {
                        return Ok(step);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
