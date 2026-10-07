//! Pipeline control builtins - xargs, tee, watch

use super::clap_cache::cached_command;
use async_trait::async_trait;

use super::{Builtin, Context, ExecutionPlan, SubCommand, resolve_path};
use crate::error::Result;
use crate::interpreter::ExecResult;

/// The xargs builtin - build and execute command lines from stdin.
///
/// Usage: xargs [OPTION]... [COMMAND [ARGS...]]
///
/// Options (GNU findutils surface):
///   -0, --null              Items are NUL-terminated (quotes not special)
///   -a, --arg-file=FILE     Read items from FILE instead of stdin
///   -d, --delimiter=DELIM   Items are DELIM-terminated (char or `\` escape)
///   -E EOF, -e[EOF]         Stop reading at the logical end-of-file word
///   -I REPLACE, -i[REPLACE] Run once per line, replacing REPLACE
///   -L N, -l[N]             Use at most N input lines per command
///   -n N                    Use at most N arguments per command
///   -s N                    Limit command line length to N bytes
///   -r                      Do not run COMMAND when input is empty
///   -t, --verbose           Print each command to stderr before running it
///   -x                      Exit if the size is exceeded
///   -P N, --max-procs=N     Allocate N parallel slots (see decision below)
///   --process-slot-var=VAR  Set VAR to this invocation's slot index (0..N-1)
///
/// Input parsing follows GNU `read_line`/`read_string`: blanks separate
/// items, single/double quotes group, backslash escapes, a NUL truncates the
/// item, and `-L` lines ending in a blank continue onto the next line.
///
/// Important decision (parallelism): bashkit runs a single `Bash` interpreter
/// sequentially — even background `&` jobs execute synchronously for
/// deterministic output (see `knowledge/foundations/parallel-execution.md` and
/// `interpreter/jobs.rs`). So `-P N` does NOT spawn N OS processes for
/// wall-clock speedup; instead it allocates N round-robin *slots*, and the
/// commands still run in order. The slot index is exposed via
/// `--process-slot-var`, which is the behaviour real sharding logic depends
/// on (`worker $SLOT of $N`). GNU's own `--process-slot-var` ranges 0..N-1
/// and is 0 when N is 1, so this matches GNU exactly for the deterministic
/// case while staying faithful to bashkit's no-hidden-concurrency model.
pub struct Xargs;

/// Default `-s` limit (GNU caps the default at 128 KiB).
const XARGS_DEFAULT_MAX_CHARS: usize = 128 * 1024;

/// Parsed xargs options.
struct XargsOptions {
    replace_str: Option<String>,
    max_args: usize,
    max_lines: usize,
    max_chars: usize,
    delimiter: Option<char>,
    eof_str: Option<String>,
    arg_file: Option<String>,
    verbose: bool,
    /// `-P N` / `--max-procs=N`: number of parallel slots. `Some(0)` means
    /// "as many as possible" (one slot per command). `None` means 1 slot.
    max_procs: Option<usize>,
    /// `--process-slot-var=VAR`: env var to expose the per-command slot index.
    process_slot_var: Option<String>,
    /// `-r` / `--no-run-if-empty`: with no input items, run nothing. GNU
    /// otherwise runs the command once without extra arguments.
    no_run_if_empty: bool,
    command: Vec<String>,
}

fn xargs_err(msg: impl std::fmt::Display) -> ExecResult {
    ExecResult::err(format!("xargs: {msg}\n"), 1)
}

/// GNU `parse_num`: strtol-style (blanks, sign), then a lower bound.
#[allow(clippy::result_large_err)]
fn xargs_parse_num(s: &str, opt: char, min: i64) -> std::result::Result<usize, ExecResult> {
    let t = s.trim_start();
    let (neg, digits) = match t.as_bytes().first() {
        Some(b'-') => (true, &t[1..]),
        Some(b'+') => (false, &t[1..]),
        _ => (false, t),
    };
    let val: i64 = match digits.parse::<i64>() {
        Ok(v) if !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()) => {
            if neg {
                -v
            } else {
                v
            }
        }
        _ => {
            return Err(xargs_err(format!(
                "invalid number \"{s}\" for -{opt} option"
            )));
        }
    };
    if val < min {
        return Err(xargs_err(format!(
            "value {s} for -{opt} option should be >= {min}"
        )));
    }
    Ok(val as usize)
}

/// GNU `strtoul` prefix parse used for `-d '\xNN'` / `-d '\NNN'`.
fn strtoul_prefix(s: &str, base: u32) -> (u64, &str) {
    let t = s.trim_start();
    let (neg, mut rest) = match t.as_bytes().first() {
        Some(b'-') => (true, &t[1..]),
        Some(b'+') => (false, &t[1..]),
        _ => (false, t),
    };
    if base == 16
        && (rest.starts_with("0x") || rest.starts_with("0X"))
        && rest[2..].starts_with(|c: char| c.is_ascii_hexdigit())
    {
        rest = &rest[2..];
    }
    let n = rest.find(|c: char| !c.is_digit(base)).unwrap_or(rest.len());
    if n == 0 {
        // No conversion: strtoul leaves the end pointer at the start.
        return (0, s);
    }
    let v = u64::from_str_radix(&rest[..n], base).unwrap_or(u64::MAX);
    (if neg { v.wrapping_neg() } else { v }, &rest[n..])
}

/// GNU `get_input_delimiter`.
#[allow(clippy::result_large_err)]
fn xargs_delimiter(s: &str) -> std::result::Result<char, ExecResult> {
    let mut chars = s.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) => return Ok(c),
        (Some('\\'), Some(n)) => {
            let simple = match n {
                'a' => Some('\x07'),
                'b' => Some('\x08'),
                'f' => Some('\x0c'),
                'n' => Some('\n'),
                'r' => Some('\r'),
                't' => Some('\t'),
                'v' => Some('\x0b'),
                '\\' => Some('\\'),
                _ => None,
            };
            if let Some(c) = simple {
                return Ok(c);
            }
            let (digits, base) = if n == 'x' {
                (&s[2..], 16)
            } else if n.is_ascii_digit() {
                (&s[1..], 8)
            } else {
                return Err(xargs_err(format!(
                    "Invalid escape sequence {s} in input delimiter specification."
                )));
            };
            let (val, rest) = strtoul_prefix(digits, base);
            if val > 0xff {
                return Err(xargs_err(format!(
                    "Invalid escape sequence {s} in input delimiter specification; character values must not exceed {}.",
                    if base == 16 { "0xff" } else { "0377" }
                )));
            }
            if !rest.is_empty() {
                return Err(xargs_err(format!(
                    "Invalid escape sequence {s} in input delimiter specification; trailing characters {rest} not recognised."
                )));
            }
            Ok(char::from(val as u8))
        }
        _ => Err(xargs_err(format!(
            "Invalid input delimiter specification {s}: the delimiter must be either a single character or an escape sequence starting with \\."
        ))),
    }
}

/// Parse xargs arguments, returning options or an error ExecResult.
#[allow(clippy::result_large_err)]
fn parse_xargs_args(args: &[String]) -> std::result::Result<XargsOptions, ExecResult> {
    use super::arg_parser::OptArg;
    let (opts, command) = super::arg_parser::gnu_getopt(
        "xargs",
        args,
        "0a:E:e::i::I:l::L:n:oprs:txP:d:",
        &[
            ("arg-file", OptArg::Required, 'a'),
            ("delimiter", OptArg::Required, 'd'),
            ("eof", OptArg::Optional, 'e'),
            ("exit", OptArg::No, 'x'),
            ("interactive", OptArg::No, 'p'),
            ("max-args", OptArg::Required, 'n'),
            ("max-chars", OptArg::Required, 's'),
            ("max-lines", OptArg::Optional, 'l'),
            ("max-procs", OptArg::Required, 'P'),
            ("no-run-if-empty", OptArg::No, 'r'),
            ("null", OptArg::No, '0'),
            ("open-tty", OptArg::No, 'o'),
            ("process-slot-var", OptArg::Required, 'V'),
            ("replace", OptArg::Optional, 'i'),
            ("show-limits", OptArg::No, 'S'),
            ("verbose", OptArg::No, 't'),
        ],
        false,
        1,
    )?;

    let mut o = XargsOptions {
        replace_str: None,
        max_args: 0,
        max_lines: 0,
        max_chars: XARGS_DEFAULT_MAX_CHARS,
        delimiter: None,
        eof_str: None,
        arg_file: None,
        verbose: false,
        max_procs: None,
        process_slot_var: None,
        no_run_if_empty: false,
        command,
    };
    // Order matters: -I/-L/-n override each other as in GNU xargs.c.
    for opt in opts {
        let val = opt.value;
        match opt.key {
            '0' => o.delimiter = Some('\0'),
            'a' => o.arg_file = val,
            'd' => o.delimiter = Some(xargs_delimiter(&val.unwrap_or_default())?),
            'E' | 'e' => o.eof_str = val.filter(|v| !v.is_empty()),
            'I' | 'i' => {
                o.replace_str = Some(val.unwrap_or_else(|| "{}".to_string()));
                o.max_args = 0;
                o.max_lines = 0;
            }
            'L' | 'l' => {
                o.max_lines = match val {
                    Some(v) => xargs_parse_num(&v, opt.key, 1)?,
                    None => 1,
                };
                o.max_args = 0;
                o.replace_str = None;
            }
            'n' => {
                o.max_lines = 0;
                o.max_args = xargs_parse_num(&val.unwrap_or_default(), 'n', 1)?;
                // GNU ignores -n1 after -i (savannah bug 57390).
                if o.max_args == 1 && o.replace_str.is_some() {
                    o.max_args = 0;
                } else {
                    o.replace_str = None;
                }
            }
            's' => o.max_chars = xargs_parse_num(&val.unwrap_or_default(), 's', 1)?,
            'P' => o.max_procs = Some(xargs_parse_num(&val.unwrap_or_default(), 'P', 0)?),
            'V' => {
                let v = val.unwrap_or_default();
                if v.is_empty() {
                    return Err(xargs_err("--process-slot-var requires a variable name"));
                }
                o.process_slot_var = Some(v);
            }
            'r' => o.no_run_if_empty = true,
            't' => o.verbose = true,
            'p' => return Err(xargs_err("failed to open /dev/tty for reading")),
            // -x, -o, --show-limits: nothing to enforce or show here.
            _ => {}
        }
    }

    if o.command.is_empty() {
        o.command.push("echo".to_string());
    }
    Ok(o)
}

/// Accumulates command lines the way GNU `buildcmd.c` does.
struct XargsBuilder<'a> {
    opts: &'a XargsOptions,
    initial_chars: usize,
    cur: Vec<String>,
    cur_chars: usize,
    commands: Vec<Vec<String>>,
}

impl<'a> XargsBuilder<'a> {
    fn new(opts: &'a XargsOptions) -> Self {
        let initial_chars = opts.command.iter().map(|a| a.len() + 1).sum();
        Self {
            opts,
            initial_chars,
            cur: Vec::new(),
            cur_chars: initial_chars,
            commands: Vec::new(),
        }
    }

    fn exec(&mut self) {
        let mut cmd = self.opts.command.clone();
        cmd.append(&mut self.cur);
        self.commands.push(cmd);
        self.cur_chars = self.initial_chars;
    }

    fn push(&mut self, arg: String) -> std::result::Result<(), String> {
        let len = arg.len() + 1;
        if self.cur_chars + len > self.opts.max_chars {
            if self.cur.is_empty() {
                return Err("argument line too long".to_string());
            }
            self.exec();
            if self.cur_chars + len > self.opts.max_chars {
                return Err("argument line too long".to_string());
            }
        }
        self.cur.push(arg);
        self.cur_chars += len;
        if self.opts.max_args > 0 && self.cur.len() >= self.opts.max_args {
            self.exec();
        }
        Ok(())
    }

    /// `-I`: one command per item with every REPLACE substituted.
    fn insert(&mut self, item: &str) -> std::result::Result<(), String> {
        let repl = self.opts.replace_str.as_deref().unwrap_or("{}");
        let cmd: Vec<String> = self
            .opts
            .command
            .iter()
            .map(|a| a.replace(repl, item))
            .collect();
        let size: usize = cmd.iter().map(|a| a.len() + 1).sum();
        if size > self.opts.max_chars {
            return Err("argument line too long".to_string());
        }
        self.commands.push(cmd);
        Ok(())
    }
}

/// Command lines built from the input, plus a fatal error that stopped
/// reading (GNU still runs what it collected before dying).
struct XargsPlan {
    commands: Vec<Vec<String>>,
    error: Option<String>,
}

/// Build command lines from parsed options and input bytes.
fn build_xargs_plan(opts: &XargsOptions, input: &str) -> XargsPlan {
    let mut b = XargsBuilder::new(opts);
    let mut error: Option<String> = None;
    if b.initial_chars > opts.max_chars {
        return XargsPlan {
            commands: Vec::new(),
            error: Some("argument list too long".to_string()),
        };
    }
    let replace = opts.replace_str.is_some();
    let mut lineno = 0usize;
    // An item is complete: insert (-I) or append, then honour -L.
    let take = |b: &mut XargsBuilder<'_>,
                item: String,
                push: bool,
                lineno: &mut usize|
     -> std::result::Result<(), String> {
        if replace {
            b.insert(&item)?;
        } else if push {
            b.push(item)?;
        }
        if opts.max_lines > 0 && *lineno >= opts.max_lines {
            b.exec();
            *lineno = 0;
        }
        Ok(())
    };

    let result: std::result::Result<(), String> = (|| {
        if let Some(delim) = opts.delimiter {
            // GNU `read_string`: every delimiter ends an item, even empty.
            let body = input;
            let mut parts: Vec<&str> = body.split(delim).collect();
            let last = parts.pop().unwrap_or("");
            for part in parts {
                lineno += 1;
                take(&mut b, part.to_string(), true, &mut lineno)?;
            }
            if !last.is_empty() {
                take(&mut b, last.to_string(), true, &mut lineno)?;
            }
            return Ok(());
        }

        // GNU `read_line` state machine.
        #[derive(PartialEq, Clone, Copy)]
        enum St {
            Space,
            Norm,
            Quote(char),
            Backslash,
        }
        let is_eof = |w: &str| opts.eof_str.as_deref() == Some(w);
        let is_blank = |c: char| c == ' ' || c == '\t';
        let is_space = |c: char| is_blank(c) || matches!(c, '\n' | '\r' | '\x0c' | '\x0b');
        // C strings end at the first NUL.
        let cstr = |w: &str| w.split('\0').next().unwrap_or("").to_string();

        let mut chars = input.chars().peekable();
        let mut prev_c;
        let mut c = '\0';
        'calls: loop {
            let mut state = St::Space;
            let mut word = String::new();
            let mut first = true;
            loop {
                prev_c = c;
                let Some(next) = chars.next() else {
                    // EOF
                    if word.is_empty() {
                        break 'calls;
                    }
                    if let St::Quote(q) = state {
                        let kind = if q == '"' { "double" } else { "single" };
                        return Err(format!(
                            "unmatched {kind} quote; by default quotes are special to xargs unless you use the -0 option"
                        ));
                    }
                    let w = cstr(&word);
                    if first && is_eof(&w) {
                        break 'calls;
                    }
                    take(&mut b, w, true, &mut lineno)?;
                    break 'calls;
                };
                c = next;
                if state == St::Space {
                    if is_space(c) {
                        continue;
                    }
                    state = St::Norm;
                }
                match state {
                    St::Norm => {
                        if c == '\n' {
                            if !is_blank(prev_c) {
                                lineno += 1;
                            }
                            if word.is_empty() {
                                state = St::Space;
                                continue;
                            }
                            let w = cstr(&word);
                            if is_eof(&w) {
                                break 'calls;
                            }
                            take(&mut b, w, true, &mut lineno)?;
                            continue 'calls;
                        }
                        if !replace && is_space(c) {
                            let w = cstr(&word);
                            if is_eof(&w) {
                                break 'calls;
                            }
                            // Pushed mid-line: no -L check until the line ends.
                            b.push(w)?;
                            word.clear();
                            state = St::Space;
                            first = false;
                            continue;
                        }
                        match c {
                            '\\' => {
                                state = St::Backslash;
                                continue;
                            }
                            '\'' | '"' => {
                                state = St::Quote(c);
                                continue;
                            }
                            _ => {}
                        }
                    }
                    St::Quote(q) => {
                        if c == '\n' {
                            let kind = if q == '"' { "double" } else { "single" };
                            return Err(format!(
                                "unmatched {kind} quote; by default quotes are special to xargs unless you use the -0 option"
                            ));
                        }
                        if c == q {
                            state = St::Norm;
                            continue;
                        }
                    }
                    St::Backslash => state = St::Norm,
                    St::Space => {}
                }
                word.push(c);
            }
        }
        Ok(())
    })();
    if let Err(e) = result {
        error = Some(e);
    }

    // GNU runs pending arguments, and runs once on empty input unless -r.
    if !replace && (!b.cur.is_empty() || (b.commands.is_empty() && !opts.no_run_if_empty)) {
        if error.is_none() || !b.cur.is_empty() {
            b.exec();
        }
    }
    XargsPlan {
        commands: b.commands,
        error,
    }
}

impl XargsOptions {
    fn subcommands(&self, commands: Vec<Vec<String>>) -> Vec<SubCommand> {
        // Number of parallel slots for --process-slot-var assignment. `-P 0`
        // ("as many as possible") gives every command a distinct slot; absent
        // `-P`, GNU uses a single slot so the index is always 0.
        let slot_count = match self.max_procs {
            Some(0) => commands.len().max(1),
            Some(n) => n,
            None => 1,
        };
        commands
            .into_iter()
            .enumerate()
            .map(|(idx, mut cmd)| {
                let name = cmd.remove(0);
                SubCommand {
                    name,
                    args: cmd,
                    stdin: None,
                    assignments: match self.process_slot_var {
                        Some(ref var) => vec![(var.clone(), (idx % slot_count).to_string())],
                        None => Vec::new(),
                    },
                }
            })
            .collect()
    }
}

/// Read the item source: `-a FILE` (or `-a -` for stdin) or stdin.
async fn xargs_input(ctx: &Context<'_>, opts: &XargsOptions) -> String {
    match opts.arg_file.as_deref() {
        None | Some("-") => ctx.stdin.map(|s| s.to_string()).unwrap_or_default(),
        Some(file) => {
            let path = resolve_path(ctx.cwd, file);
            // A directory or unreadable file yields no items, as GNU's
            // read error leaves the input empty.
            match ctx.fs.read_file(&path).await {
                Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
                Err(_) => String::new(),
            }
        }
    }
}

/// Runs the planned commands in order, GNU-style: `-t` echoes each command
/// to stderr first, exit status 123 when any command failed (1-125), and a
/// command exiting 255 stops xargs with status 124.
struct XargsRun {
    commands: std::collections::VecDeque<SubCommand>,
    verbose: bool,
    announced: bool,
    error: Option<String>,
    stdout: crate::StreamData,
    stderr: crate::StreamData,
    status: i32,
}

#[async_trait]
impl super::PlanDriver for XargsRun {
    async fn next(&mut self, last: Option<ExecResult>) -> Result<super::PlanStep> {
        if let Some(r) = last {
            self.stdout.append(&r.stdout);
            self.stderr.append(&r.stderr);
            match r.exit_code {
                0 => {}
                255 => {
                    let name = self
                        .commands
                        .pop_front()
                        .map(|c| c.name)
                        .unwrap_or_default();
                    self.commands.clear();
                    self.error = Some(format!("{name}: exited with status 255; aborting"));
                    self.status = 124;
                }
                126 | 127 => self.status = r.exit_code,
                _ => {
                    if self.status == 0 {
                        self.status = 123;
                    }
                }
            }
            if self.status != 124 {
                self.commands.pop_front();
            }
            self.announced = false;
        }
        if let Some(cmd) = self.commands.front() {
            if self.verbose && !self.announced {
                self.announced = true;
                let mut line = cmd.name.clone();
                for a in &cmd.args {
                    line.push(' ');
                    line.push_str(a);
                }
                line.push('\n');
                let err: crate::StreamData = line.into();
                self.stderr.append(&err);
                return Ok(super::PlanStep::Emit {
                    stdout: crate::StreamData::new(),
                    stderr: err,
                });
            }
            return Ok(super::PlanStep::Run {
                command: cmd.clone(),
                cwd: None,
            });
        }
        if let Some(e) = self.error.take() {
            let err: crate::StreamData = format!("xargs: {e}\n").into();
            self.stderr.append(&err);
            if self.status == 0 {
                self.status = 1;
            }
        }
        Ok(super::PlanStep::Done(ExecResult {
            stdout: std::mem::take(&mut self.stdout),
            stderr: std::mem::take(&mut self.stderr),
            exit_code: self.status,
            ..Default::default()
        }))
    }
}

#[async_trait]
impl Builtin for Xargs {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        if let Some(r) = super::check_help_version(
            ctx.args,
            "Usage: xargs [OPTION]... [COMMAND [ARGS]...]\nBuild and execute command lines from standard input.\n\n  -0, --null\titems are separated by a null, not whitespace\n  -a, --arg-file=FILE\tread arguments from FILE, not standard input\n  -d, --delimiter=CHAR\titems are separated by CHAR\n  -E END\tset logical EOF string\n  -I R\treplace R in initial-arguments with names read from input\n  -L, --max-lines=MAX-LINES\tuse at most MAX-LINES non-blank input lines per command line\n  -n, --max-args=MAX-ARGS\tuse at most MAX-ARGS arguments per command line\n  -r, --no-run-if-empty\tif there are no arguments, then do not run COMMAND\n  -s, --max-chars=MAX-CHARS\tlimit length of command line to MAX-CHARS\n  -t, --verbose\tprint commands before executing them\n  -x, --exit\texit if the size (see -s) is exceeded\n  -P, --max-procs=N\tallocate N parallel slots (runs sequentially)\n  --process-slot-var=VAR\tset VAR to the slot index (0..N-1)\n  --help\tdisplay this help and exit\n  --version\toutput version information and exit\n",
            Some("xargs (bashkit) 0.1"),
        ) {
            return Ok(r);
        }
        // Validate arguments and return error for invalid input.
        // When no executor is available, output what commands would be run.
        let opts = match parse_xargs_args(ctx.args) {
            Ok(opts) => opts,
            Err(e) => return Ok(e),
        };

        let input = xargs_input(&ctx, &opts).await;
        let plan = build_xargs_plan(&opts, &input);
        let commands = opts.subcommands(plan.commands);

        // Fallback: output what would be run (for standalone builtin context).
        // Command-scoped assignments (e.g. the --process-slot-var index) are
        // rendered as a `VAR=value` prefix so the slot is visible here too.
        let mut output = String::new();
        for cmd in &commands {
            for (var, val) in &cmd.assignments {
                output.push_str(var);
                output.push('=');
                output.push_str(val);
                output.push(' ');
            }
            output.push_str(&cmd.name);
            for arg in &cmd.args {
                output.push(' ');
                output.push_str(arg);
            }
            output.push('\n');
        }
        if let Some(e) = plan.error {
            let mut r = xargs_err(e);
            r.stdout = output.into();
            return Ok(r);
        }
        Ok(ExecResult::ok(output))
    }

    async fn execution_plan(&self, ctx: &Context<'_>) -> Result<Option<ExecutionPlan>> {
        let opts = match parse_xargs_args(ctx.args) {
            Ok(opts) => opts,
            Err(_) => return Ok(None), // Let execute() handle the error
        };

        let input = xargs_input(ctx, &opts).await;
        let plan = build_xargs_plan(&opts, &input);
        let commands = opts.subcommands(plan.commands);
        if !opts.verbose && plan.error.is_none() {
            if commands.is_empty() {
                return Ok(None);
            }
            return Ok(Some(ExecutionPlan::Batch { commands }));
        }
        Ok(Some(ExecutionPlan::Driver(Box::new(XargsRun {
            commands: commands.into(),
            verbose: opts.verbose,
            announced: false,
            error: plan.error,
            stdout: crate::StreamData::new(),
            stderr: crate::StreamData::new(),
            status: 0,
        }))))
    }
}

/// The tee builtin - read from stdin and write to stdout and files.
///
/// Usage: tee [-a] [FILE...]
///
/// Options:
///   -a, --append              Append to files instead of overwriting
///   -i, --ignore-interrupts   No-op in bashkit's virtual mode (no signals)
///   -p                        Diagnose only non-pipe write errors
///   --output-error[=MODE]     Set write-error behavior (parsed but reduced
///                              to bashkit's all-or-nothing VFS write model)
///
/// Argument surface is generated from uutils/coreutils' `uu_app()` via
/// the `bashkit-coreutils-port` codegen tool — see
/// `generated/tee_args.rs`. Behaviour is implemented locally against
/// the bashkit VFS.
pub struct Tee;

// Cached `tee` arg surface: pre-built once, cloned per invocation.
// See `builtins::clap_cache` for why it is built, not just constructed.
cached_command!(tee_cmd, super::generated::tee_args::tee_command());

#[async_trait]
impl Builtin for Tee {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        use std::ffi::OsString;

        let argv: Vec<OsString> = std::iter::once(OsString::from("tee"))
            .chain(ctx.args.iter().map(OsString::from))
            .collect();

        let matches = match tee_cmd().try_get_matches_from(argv) {
            Ok(m) => m,
            Err(e) => {
                let kind = e.kind();
                let rendered = e.render().to_string();
                if matches!(
                    kind,
                    clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion
                ) {
                    return Ok(ExecResult::ok(rendered));
                }
                return Ok(ExecResult::err(rendered, 2));
            }
        };

        let append = matches.get_flag("append");
        // -i/--ignore-interrupts and -p are accepted but irrelevant in
        // bashkit: there are no signals and no pipe errors in the VFS
        // write model. Read them so clap counts them as consumed.
        let _ = matches.get_flag("ignore-interrupts");
        let _ = matches.get_flag("ignore-pipe-errors");
        let _ = matches.get_one::<String>("output-error");

        let files: Vec<String> = matches
            .get_many::<OsString>("file")
            .map(|vs| vs.map(|v| v.to_string_lossy().into_owned()).collect())
            .unwrap_or_default();

        let input = ctx.stdin.map(|stdin| &**stdin).unwrap_or("");

        for file in &files {
            // tee(1): "If a FILE is -, it refers to a file named - ."
            // The codegen output documents the same in `after_help`.
            let path = resolve_path(ctx.cwd, file);

            if append {
                ctx.fs.append_file(&path, input.as_bytes()).await?;
            } else {
                ctx.fs.write_file(&path, input.as_bytes()).await?;
            }
        }

        Ok(ExecResult::ok(input.to_string()))
    }
}

/// The watch builtin - execute a program periodically.
///
/// Usage: watch [-n SECONDS] [-t] [-g] [-x] COMMAND
///
/// Options:
///   -n SECONDS   Specify update interval (default: 2)
///   -t           No title line
///   -g           Exit when the output changes (`--chgexit`)
///   -x           Run COMMAND's words directly instead of `bash -c`
///
/// Inside a terminal session, watch reruns the command every interval on the
/// alternate screen until Ctrl-C (or a change with `-g`); each run and the
/// interval count against the execution timeout like `sleep`. Elsewhere it
/// prints a one-line notice, since there is no screen to refresh.
pub struct Watch;

const WATCH_USAGE: &str = "Usage: watch [OPTION]... COMMAND\nExecute a program periodically, showing output.\n\n  -n SECONDS\tupdate interval (default: 2)\n  -t\t\tno title line\n  -g\t\texit when the output changes\n  -x\t\tpass COMMAND's words to exec instead of bash -c\n  --help\tdisplay this help and exit\n  --version\toutput version information and exit\n";

struct WatchArgs {
    interval: f64,
    title: bool,
    chgexit: bool,
    exec: bool,
    command: Vec<String>,
}

fn parse_watch(args: &[String]) -> std::result::Result<WatchArgs, String> {
    let mut parsed = WatchArgs {
        interval: 2.0,
        title: true,
        chgexit: false,
        exec: false,
        command: Vec::new(),
    };
    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        if arg == "-n" || arg == "--interval" {
            i += 1;
            let Some(value) = args.get(i) else {
                return Err("watch: option requires an argument -- 'n'\n".to_string());
            };
            match value.parse::<f64>() {
                // THREAT[TM-DOS-119]: a floor keeps `-n 0` from spinning.
                Ok(n) if n > 0.0 && n.is_finite() => parsed.interval = n.max(0.1),
                _ => {
                    return Err(format!("watch: invalid interval '{value}'\n"));
                }
            }
        } else if arg == "-t" || arg == "--no-title" {
            parsed.title = false;
        } else if arg == "-g" || arg == "--chgexit" {
            parsed.chgexit = true;
        } else if arg == "-x" || arg == "--exec" {
            parsed.exec = true;
        } else if arg.starts_with('-') && arg != "-" {
            // Skip other options for compatibility
        } else {
            parsed.command = args[i..].to_vec();
            return Ok(parsed);
        }
        i += 1;
    }
    Err("watch: no command specified\n".to_string())
}

#[async_trait]
impl Builtin for Watch {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        if let Some(r) =
            super::check_help_version(ctx.args, WATCH_USAGE, Some("watch (bashkit) 0.1"))
        {
            return Ok(r);
        }
        let parsed = match parse_watch(ctx.args) {
            Ok(p) => p,
            Err(msg) => return Ok(ExecResult::err(msg, 1)),
        };
        let output = format!(
            "Every {:.1}s: {}\n\n(watch: continuous execution needs a terminal session)\n",
            parsed.interval,
            parsed.command.join(" ")
        );

        Ok(ExecResult::ok(output))
    }

    #[cfg(feature = "terminal")]
    async fn execution_plan(&self, ctx: &Context<'_>) -> Result<Option<ExecutionPlan>> {
        if ctx.args.iter().any(|a| a == "--help" || a == "--version") {
            return Ok(None);
        }
        let Some(tty) = super::pager::terminal(ctx)? else {
            return Ok(None);
        };
        let Ok(parsed) = parse_watch(ctx.args) else {
            return Ok(None);
        };
        Ok(Some(ExecutionPlan::Driver(Box::new(WatchRun {
            tty,
            args: parsed,
            first: None,
            on_screen: false,
        }))))
    }
}

/// Reruns the command on the alternate screen. Dropped on Ctrl-C (the
/// interpreter future is dropped), which restores the primary screen.
#[cfg(feature = "terminal")]
struct WatchRun {
    tty: crate::terminal::Tty,
    args: WatchArgs,
    /// Output of the first run, for `-g`.
    first: Option<String>,
    on_screen: bool,
}

#[cfg(feature = "terminal")]
impl WatchRun {
    fn command(&self) -> super::PlanStep {
        let (name, args) = if self.args.exec {
            (
                self.args.command[0].clone(),
                self.args.command[1..].to_vec(),
            )
        } else {
            (
                "bash".to_string(),
                vec!["-c".to_string(), self.args.command.join(" ")],
            )
        };
        super::PlanStep::Run {
            command: SubCommand {
                name,
                args,
                stdin: None,
                assignments: Vec::new(),
            },
            cwd: None,
        }
    }

    fn render(&self, output: &str) {
        let size = self.tty.size();
        let cols = usize::from(size.cols);
        let mut rows = usize::from(size.rows);
        let mut screen = String::from("\x1b[H\x1b[2J");
        if self.args.title {
            let title = format!(
                "Every {:.1}s: {}",
                self.args.interval,
                self.args.command.join(" ")
            );
            screen.extend(title.chars().take(cols));
            screen.push_str("\r\n\r\n");
            rows = rows.saturating_sub(2);
        }
        let lines: Vec<String> = output
            .lines()
            .take(rows)
            .map(|l| caret_notation(l).chars().take(cols).collect())
            .collect();
        screen.push_str(&lines.join("\r\n"));
        self.tty.write(screen.as_bytes());
    }
}

/// Control characters as `^X` (tabs kept, `\r` dropped), so command output
/// cannot move the cursor or inject escape sequences into the host terminal.
#[cfg(feature = "terminal")]
fn caret_notation(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    for c in line.chars() {
        match c {
            '\r' => {}
            '\t' => out.push(' '),
            c if (c as u32) < 0x20 || c == '\x7f' => {
                out.push('^');
                out.push(((c as u8) ^ 0x40) as char);
            }
            c => out.push(c),
        }
    }
    out
}

#[cfg(feature = "terminal")]
impl Drop for WatchRun {
    fn drop(&mut self) {
        if self.on_screen {
            self.tty.write(b"\x1b[?1049l");
        }
    }
}

#[cfg(feature = "terminal")]
#[async_trait]
impl super::PlanDriver for WatchRun {
    async fn next(&mut self, last: Option<ExecResult>) -> Result<super::PlanStep> {
        let Some(result) = last else {
            self.tty.write(b"\x1b[?1049h");
            self.on_screen = true;
            return Ok(self.command());
        };
        let mut output = result.stdout.text_lossy().into_owned();
        output.push_str(&result.stderr.text_lossy());
        self.render(&output);
        if self.args.chgexit {
            match &self.first {
                None => self.first = Some(output),
                Some(first) if *first != output => {
                    self.tty.write(b"\x1b[?1049l");
                    self.on_screen = false;
                    return Ok(super::PlanStep::Done(ExecResult::ok("")));
                }
                Some(_) => {}
            }
        }
        crate::time_compat::sleep(std::time::Duration::from_secs_f64(self.args.interval)).await;
        Ok(self.command())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Arc;

    use crate::fs::{FileSystem, InMemoryFs};

    async fn create_test_ctx() -> (Arc<InMemoryFs>, PathBuf, HashMap<String, String>) {
        let fs = Arc::new(InMemoryFs::new());
        let cwd = PathBuf::from("/home/user");
        let variables = HashMap::new();

        fs.mkdir(&cwd, true).await.unwrap();

        (fs, cwd, variables)
    }

    // ==================== xargs tests ====================

    fn xargs_plan(args: &[&str], input: &str) -> (Vec<Vec<String>>, Option<String>) {
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let opts = parse_xargs_args(&args).unwrap_or_else(|e| panic!("{}", e.stderr));
        let plan = build_xargs_plan(&opts, input);
        (plan.commands, plan.error)
    }

    fn cmds(list: &[&[&str]]) -> Vec<Vec<String>> {
        list.iter()
            .map(|c| c.iter().map(|s| s.to_string()).collect())
            .collect()
    }

    #[test]
    fn test_xargs_quotes_backslashes_and_nul() {
        let (c, _) = xargs_plan(&["-n1"], "\"a b\" 'c' d\\ e x\0y\n");
        assert_eq!(
            c,
            cmds(&[
                &["echo", "a b"],
                &["echo", "c"],
                &["echo", "d e"],
                &["echo", "x"]
            ])
        );
        // An unmatched quote stops reading after running what was read.
        let (c, e) = xargs_plan(&[], "a 'b\nc\n");
        assert_eq!(c, cmds(&[&["echo", "a"]]));
        assert!(e.unwrap().contains("unmatched single quote"));
    }

    #[test]
    fn test_xargs_lines_eof_and_replace() {
        let (c, _) = xargs_plan(&["-L1"], "a b \nc\nd\n");
        assert_eq!(c, cmds(&[&["echo", "a", "b", "c"], &["echo", "d"]]));
        let (c, _) = xargs_plan(&["-E", "STOP"], "a STOP b\n");
        assert_eq!(c, cmds(&[&["echo", "a"]]));
        let (c, _) = xargs_plan(&["-I{}", "-n1", "x", "[{}]"], "  a b  \n\nc\n");
        assert_eq!(c, cmds(&[&["x", "[a b  ]"], &["x", "[c]"]]));
        // -n after -I (other than 1) turns replacement off.
        let (c, _) = xargs_plan(&["-I{}", "-n2", "x", "{}"], "a\nb\n");
        assert_eq!(c, cmds(&[&["x", "{}", "a", "b"]]));
    }

    #[test]
    fn test_xargs_delimiters_and_size_limit() {
        let (c, _) = xargs_plan(&["-d", "\\x2c"], "a,,b");
        assert_eq!(c, cmds(&[&["echo", "a", "", "b"]]));
        let (c, _) = xargs_plan(&["-0", "-n1"], "a b\0\0c\0");
        assert_eq!(c, cmds(&[&["echo", "a b"], &["echo", ""], &["echo", "c"]]));
        let (c, _) = xargs_plan(&["-s", "9"], "a b c\n");
        assert_eq!(c, cmds(&[&["echo", "a", "b"], &["echo", "c"]]));
        let (c, e) = xargs_plan(&["-I{}", "-s", "6", "echo", "x{}"], "abcde\n");
        assert!(c.is_empty() && e.is_some());
        assert!(xargs_delimiter("\\xg").is_err());
        assert!(xargs_delimiter("ab").is_err());
    }

    #[tokio::test]
    async fn test_xargs_basic() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        let args: Vec<String> = vec![];
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs: fs.clone(),
            stdin: Some(crate::builtins::test_stream("foo bar baz")),
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
        };

        let result = Xargs.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("echo foo bar baz"));
    }

    #[tokio::test]
    async fn test_xargs_with_command() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        let args = vec!["rm".to_string()];
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs: fs.clone(),
            stdin: Some(crate::builtins::test_stream("file1 file2")),
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
        };

        let result = Xargs.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("rm file1 file2"));
    }

    #[tokio::test]
    async fn test_xargs_n_option() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        let args = vec!["-n".to_string(), "1".to_string(), "echo".to_string()];
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs: fs.clone(),
            stdin: Some(crate::builtins::test_stream("a b c")),
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
        };

        let result = Xargs.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        let lines: Vec<_> = result.stdout.lines().collect();
        assert_eq!(lines.len(), 3);
        assert!(lines[0].contains("echo a"));
        assert!(lines[1].contains("echo b"));
        assert!(lines[2].contains("echo c"));
    }

    #[tokio::test]
    async fn test_xargs_i_option() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        let args = vec![
            "-I".to_string(),
            "{}".to_string(),
            "cp".to_string(),
            "{}".to_string(),
            "{}.bak".to_string(),
        ];
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs: fs.clone(),
            stdin: Some(crate::builtins::test_stream("file1\nfile2")),
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
        };

        let result = Xargs.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("cp file1 file1.bak"));
        assert!(result.stdout.contains("cp file2 file2.bak"));
    }

    #[tokio::test]
    async fn test_xargs_d_option() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        let args = vec!["-d".to_string(), ":".to_string()];
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs: fs.clone(),
            stdin: Some(crate::builtins::test_stream("a:b:c")),
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
        };

        let result = Xargs.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("echo a b c"));
    }

    #[tokio::test]
    async fn test_xargs_empty_input() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        let args: Vec<String> = vec![];
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs: fs.clone(),
            stdin: Some(crate::builtins::test_stream("")),
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
        };

        // GNU: empty input still runs the command once with no args.
        let result = Xargs.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "echo\n");
    }

    #[tokio::test]
    async fn test_xargs_no_run_if_empty() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        let args = vec!["-r".to_string()];
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs: fs.clone(),
            stdin: Some(crate::builtins::test_stream("  \n")),
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
        };

        let result = Xargs.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.is_empty());
    }

    #[tokio::test]
    async fn test_xargs_invalid_option() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        let args = vec!["-z".to_string()];
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs: fs.clone(),
            stdin: Some(crate::builtins::test_stream("test")),
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
        };

        let result = Xargs.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("invalid option"));
    }

    #[tokio::test]
    async fn test_xargs_plan_basic() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        let args = vec!["rm".to_string()];
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs: fs.clone(),
            stdin: Some(crate::builtins::test_stream("file1 file2")),
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
        };

        let plan = Xargs.execution_plan(&ctx).await.unwrap();
        match plan {
            Some(ExecutionPlan::Batch { commands }) => {
                assert_eq!(commands.len(), 1);
                assert_eq!(commands[0].name, "rm");
                assert_eq!(commands[0].args, vec!["file1", "file2"]);
            }
            _ => panic!("expected Batch plan"),
        }
    }

    #[tokio::test]
    async fn test_xargs_plan_n_option() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        let args = vec!["-n".to_string(), "1".to_string(), "echo".to_string()];
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs: fs.clone(),
            stdin: Some(crate::builtins::test_stream("a b c")),
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
        };

        let plan = Xargs.execution_plan(&ctx).await.unwrap();
        match plan {
            Some(ExecutionPlan::Batch { commands }) => {
                assert_eq!(commands.len(), 3);
                assert_eq!(commands[0].name, "echo");
                assert_eq!(commands[0].args, vec!["a"]);
                assert_eq!(commands[1].args, vec!["b"]);
                assert_eq!(commands[2].args, vec!["c"]);
            }
            _ => panic!("expected Batch plan"),
        }
    }

    #[tokio::test]
    async fn test_xargs_p_option_accepted() {
        // -P must no longer be rejected as an invalid option.
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        let args = vec!["-P".to_string(), "4".to_string(), "echo".to_string()];
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs: fs.clone(),
            stdin: Some(crate::builtins::test_stream("a b c")),
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
        };

        let result = Xargs.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("echo a b c"));
    }

    #[tokio::test]
    async fn test_xargs_p_invalid_number() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        let args = vec!["-P".to_string(), "abc".to_string(), "echo".to_string()];
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs: fs.clone(),
            stdin: Some(crate::builtins::test_stream("a")),
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
        };

        let result = Xargs.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 1);
        assert!(
            result
                .stderr
                .contains("invalid number \"abc\" for -P option")
        );
    }

    #[tokio::test]
    async fn test_xargs_process_slot_var_round_robin() {
        // -P N with --process-slot-var assigns slots 0..N-1 round-robin.
        // The fallback rendering shows them as a `VAR=value` prefix.
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        let args = vec![
            "-P".to_string(),
            "2".to_string(),
            "--process-slot-var=SLOT".to_string(),
            "-n".to_string(),
            "1".to_string(),
            "echo".to_string(),
        ];
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs: fs.clone(),
            stdin: Some(crate::builtins::test_stream("a b c d")),
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
        };

        let result = Xargs.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        let lines: Vec<_> = result.stdout.lines().collect();
        assert_eq!(
            lines,
            vec![
                "SLOT=0 echo a",
                "SLOT=1 echo b",
                "SLOT=0 echo c",
                "SLOT=1 echo d",
            ]
        );
    }

    #[tokio::test]
    async fn test_xargs_process_slot_var_single_slot_is_zero() {
        // Without -P there is one slot, so the index is always 0 (GNU parity).
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        let args = vec![
            "--process-slot-var".to_string(),
            "S".to_string(),
            "-n".to_string(),
            "1".to_string(),
            "echo".to_string(),
        ];
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs: fs.clone(),
            stdin: Some(crate::builtins::test_stream("a b")),
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
        };

        let result = Xargs.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        let lines: Vec<_> = result.stdout.lines().collect();
        assert_eq!(lines, vec!["S=0 echo a", "S=0 echo b"]);
    }

    #[tokio::test]
    async fn test_xargs_plan_carries_slot_assignment() {
        // The execution plan must carry the per-command slot assignment so the
        // interpreter runs each command with `VAR=slot cmd ...`.
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        let args = vec![
            "-P".to_string(),
            "2".to_string(),
            "--process-slot-var=SLOT".to_string(),
            "-n".to_string(),
            "1".to_string(),
            "echo".to_string(),
        ];
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs: fs.clone(),
            stdin: Some(crate::builtins::test_stream("a b c")),
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
        };

        let plan = Xargs.execution_plan(&ctx).await.unwrap();
        match plan {
            Some(ExecutionPlan::Batch { commands }) => {
                assert_eq!(commands.len(), 3);
                assert_eq!(
                    commands[0].assignments,
                    vec![("SLOT".to_string(), "0".to_string())]
                );
                assert_eq!(
                    commands[1].assignments,
                    vec![("SLOT".to_string(), "1".to_string())]
                );
                assert_eq!(
                    commands[2].assignments,
                    vec![("SLOT".to_string(), "0".to_string())]
                );
            }
            _ => panic!("expected Batch plan"),
        }
    }

    #[tokio::test]
    async fn test_xargs_max_procs_long_form() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        let args = vec![
            "--max-procs=3".to_string(),
            "--process-slot-var=S".to_string(),
            "-n".to_string(),
            "1".to_string(),
            "echo".to_string(),
        ];
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs: fs.clone(),
            stdin: Some(crate::builtins::test_stream("a b c d")),
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
        };

        let result = Xargs.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        let lines: Vec<_> = result.stdout.lines().collect();
        assert_eq!(
            lines,
            vec!["S=0 echo a", "S=1 echo b", "S=2 echo c", "S=0 echo d",]
        );
    }

    // ==================== tee tests ====================

    #[tokio::test]
    async fn test_tee_basic() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        let args = vec!["output.txt".to_string()];
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs: fs.clone(),
            stdin: Some(crate::builtins::test_stream("Hello, world!")),
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
        };

        let result = Tee.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "Hello, world!");

        let content = fs.read_file(&cwd.join("output.txt")).await.unwrap();
        assert_eq!(content, b"Hello, world!");
    }

    #[tokio::test]
    async fn test_tee_multiple_files() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        let args = vec!["file1.txt".to_string(), "file2.txt".to_string()];
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs: fs.clone(),
            stdin: Some(crate::builtins::test_stream("content")),
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
        };

        let result = Tee.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "content");

        let content1 = fs.read_file(&cwd.join("file1.txt")).await.unwrap();
        let content2 = fs.read_file(&cwd.join("file2.txt")).await.unwrap();
        assert_eq!(content1, b"content");
        assert_eq!(content2, b"content");
    }

    #[tokio::test]
    async fn test_tee_append() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        fs.write_file(&cwd.join("output.txt"), b"initial\n")
            .await
            .unwrap();

        let args = vec!["-a".to_string(), "output.txt".to_string()];
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs: fs.clone(),
            stdin: Some(crate::builtins::test_stream("appended")),
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
        };

        let result = Tee.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);

        let content = fs.read_file(&cwd.join("output.txt")).await.unwrap();
        assert_eq!(content, b"initial\nappended");
    }

    #[tokio::test]
    async fn test_tee_no_files() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        let args: Vec<String> = vec![];
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs: fs.clone(),
            stdin: Some(crate::builtins::test_stream("pass through")),
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
        };

        let result = Tee.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "pass through");
    }

    #[tokio::test]
    async fn test_tee_invalid_option() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        let args = vec!["-z".to_string()];
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs: fs.clone(),
            stdin: Some(crate::builtins::test_stream("test")),
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
        };

        let result = Tee.execute(ctx).await.unwrap();
        // Unknown flag: clap returns exit code 2 with its own
        // "unexpected argument" diagnostic. GNU coreutils' tee exits
        // 1 with "invalid option". The clap-vs-GNU divergence is
        // documented in `tests/spec_cases/bash/tee.test.sh`.
        assert_eq!(result.exit_code, 2);
        assert!(
            result.stderr.contains("unexpected argument")
                || result.stderr.contains("invalid option")
        );
    }

    // ==================== watch tests ====================

    #[tokio::test]
    async fn test_watch_basic() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        let args = vec!["ls".to_string(), "-l".to_string()];
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs: fs.clone(),
            stdin: None,
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
        };

        let result = Watch.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("ls -l"));
        assert!(result.stdout.contains("Every 2.0s"));
    }

    #[tokio::test]
    async fn test_watch_n_option() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        let args = vec!["-n".to_string(), "5".to_string(), "date".to_string()];
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs: fs.clone(),
            stdin: None,
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
        };

        let result = Watch.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("Every 5.0s"));
        assert!(result.stdout.contains("date"));
    }

    #[tokio::test]
    async fn test_watch_no_command() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        let args: Vec<String> = vec![];
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs: fs.clone(),
            stdin: None,
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
        };

        let result = Watch.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("no command specified"));
    }

    #[tokio::test]
    async fn test_watch_invalid_interval() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        let args = vec!["-n".to_string(), "abc".to_string(), "ls".to_string()];
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs: fs.clone(),
            stdin: None,
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
        };

        let result = Watch.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("invalid interval"));
    }
}
