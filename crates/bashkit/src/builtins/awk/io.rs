//! awk input and output: records, `getline`, redirections, pipes and
//! `system()`.
//!
//! Decisions:
//! - Main input follows `ARGV` at run time (so BEGIN can change it):
//!   `var=value` operands are assignments, `-` is stdin, a missing file is
//!   a gawk fatal error.
//! - Commands (`system()`, `print | cmd`, `cmd | getline`) run in the
//!   sandbox shell through the builtin's execution-plan driver: the
//!   evaluator posts a request on a shared slot and waits; the driver
//!   hands it to the interpreter and returns the result. Without a driver
//!   (direct `execute`), commands fail with a message.
//! - `print | cmd` buffers the command's stdin and runs it at `close()` or
//!   exit (the command sees all input at once, as with a real pipe).
//! - File redirections are buffered in small chunks and flushed through the
//!   VFS, so sandbox quotas apply while awk runs.
//! - THREAT[TM-DOS-028]: output (stdout, stderr, files, pipes) is capped at
//!   `AWK_MAX_OUTPUT_BYTES` and distinct redirection targets at
//!   `AWK_MAX_OUTPUT_TARGETS`.
//! - THREAT[TM-DOS-116]: `getline` inputs are capped per file, in number
//!   and in retained bytes; every load is charged to the request budget,
//!   which `close()` cannot refund.

use std::collections::HashMap;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context as TaskContext, Poll};

use super::ast::{Expr, GetlineSrc, Redirect, special};
use super::interp::{Cell, Elem, Interp, R, Unwind, next_char};
use super::regex::AwkRegex;
use super::value::Value;
use crate::builtins::Date;
use crate::builtins::limits::{
    AWK_MAX_GETLINE_CACHE_BYTES, AWK_MAX_GETLINE_CACHED_FILES, AWK_MAX_GETLINE_FILE_BYTES,
    AWK_MAX_OUTPUT_BYTES, AWK_MAX_OUTPUT_TARGETS,
};
use crate::fs::{FileSystem, vfs_join};
use crate::interpreter::ExecResult;

/// File redirections flush once this many bytes are buffered.
const FILE_FLUSH_BYTES: usize = 8 * 1024;

/// Record separator mode.
#[derive(Clone)]
pub(super) enum RsMode {
    Newline,
    Char(char),
    /// `RS = ""`: records separated by blank lines.
    Paragraph,
    Regex(Arc<AwkRegex>),
}

/// An input source held in memory.
pub(super) struct Reader {
    data: String,
    pos: usize,
    /// Exit status of the command that produced it (`cmd | getline`).
    status: i32,
}

impl Reader {
    pub(super) fn new(data: String) -> Self {
        Reader {
            data,
            pos: 0,
            status: 0,
        }
    }

    /// Next record and its terminator (`RT`).
    pub(super) fn read(&mut self, rs: &RsMode) -> Option<(String, String)> {
        let d = &self.data;
        let p = self.pos;
        let len = d.len();
        match rs {
            RsMode::Newline | RsMode::Char(_) => {
                if p >= len {
                    return None;
                }
                let sep = match rs {
                    RsMode::Char(c) => *c,
                    _ => '\n',
                };
                match d[p..].find(sep) {
                    Some(i) => {
                        let rec = d[p..p + i].to_string();
                        self.pos = p + i + sep.len_utf8();
                        Some((rec, sep.to_string()))
                    }
                    None => {
                        let rec = d[p..].to_string();
                        self.pos = len;
                        Some((rec, String::new()))
                    }
                }
            }
            RsMode::Paragraph => {
                let mut s = p;
                while s < len && d.as_bytes()[s] == b'\n' {
                    s += 1;
                }
                if s >= len {
                    self.pos = len;
                    return None;
                }
                match d[s..].find("\n\n") {
                    Some(i) => {
                        let end = s + i;
                        let mut j = end;
                        while j < len && d.as_bytes()[j] == b'\n' {
                            j += 1;
                        }
                        let r = (d[s..end].to_string(), d[end..j].to_string());
                        self.pos = j;
                        Some(r)
                    }
                    None => {
                        let rest = &d[s..];
                        let trimmed = rest.trim_end_matches('\n');
                        let r = (trimmed.to_string(), rest[trimmed.len()..].to_string());
                        self.pos = len;
                        Some(r)
                    }
                }
            }
            RsMode::Regex(re) => {
                if p >= len {
                    return None;
                }
                let mut from = p;
                while from <= len {
                    match re.find_at(d, from) {
                        Some((s, e)) if s == e => from = next_char(d, s),
                        Some((s, e)) => {
                            let r = (d[p..s].to_string(), d[s..e].to_string());
                            self.pos = e;
                            return Some(r);
                        }
                        None => break,
                    }
                }
                let rec = d[p..].to_string();
                self.pos = len;
                Some((rec, String::new()))
            }
        }
    }
}

/// A command for the shell, posted by the evaluator to the plan driver.
pub(super) struct CmdRequest {
    /// Capture the output for awk (`cmd | getline`) instead of streaming it.
    pub(super) capture: bool,
    pub(super) command: String,
    pub(super) stdin: Option<String>,
    /// awk output to stream before the command runs (keeps order).
    pub(super) emit_stdout: String,
    pub(super) emit_stderr: String,
}

#[derive(Default)]
pub(super) struct BridgeState {
    pub(super) request: Option<CmdRequest>,
    pub(super) response: Option<ExecResult>,
}

pub(super) type Bridge = Arc<Mutex<BridgeState>>;

/// Resolves when the driver has stored a response. The driver polls the
/// awk future again only after storing one, so no waker is needed.
struct WaitResponse(Bridge);

impl Future for WaitResponse {
    type Output = ExecResult;
    fn poll(self: Pin<&mut Self>, _cx: &mut TaskContext<'_>) -> Poll<ExecResult> {
        match self.0.lock().ok().and_then(|mut s| s.response.take()) {
            Some(r) => Poll::Ready(r),
            None => Poll::Pending,
        }
    }
}

/// What awk needs from the shell around it.
pub(super) struct Host {
    pub(super) fs: Arc<dyn FileSystem>,
    pub(super) cwd: PathBuf,
    pub(super) env: Vec<(String, String)>,
    pub(super) stdin: Option<String>,
    pub(super) budget: Option<crate::limits::ExecutionBudget>,
    pub(super) clock: Date,
    pub(super) bridge: Option<Bridge>,
}

enum OutKind {
    File {
        path: PathBuf,
        append: bool,
        written: bool,
    },
    Pipe {
        command: String,
    },
}

struct Output {
    kind: OutKind,
    buf: String,
}

#[derive(Default)]
struct MainInput {
    /// Next `ARGV` index to look at.
    argi: usize,
    cur: Option<Reader>,
    /// Some file operand (or stdin) has been opened.
    used: bool,
}

pub(super) struct Io {
    pub(super) host: Host,
    pub(super) stdout: String,
    pub(super) stderr: String,
    /// Bytes of stdout/stderr already streamed by the driver.
    pub(super) emitted: (usize, usize),
    /// Bytes sent to files and pipes (output cap accounting).
    redirected: usize,
    outputs: HashMap<String, Output>,
    out_order: Vec<String>,
    readers: HashMap<String, Reader>,
    reader_bytes: usize,
    main: MainInput,
    stdin_taken: bool,
    warned_no_commands: bool,
}

impl Io {
    pub(super) fn new(host: Host) -> Self {
        Io {
            host,
            stdout: String::new(),
            stderr: String::new(),
            emitted: (0, 0),
            redirected: 0,
            outputs: HashMap::new(),
            out_order: Vec::new(),
            readers: HashMap::new(),
            reader_bytes: 0,
            main: MainInput::default(),
            stdin_taken: false,
            warned_no_commands: false,
        }
    }

    fn take_stdin(&mut self) -> String {
        if self.stdin_taken {
            return String::new();
        }
        self.stdin_taken = true;
        self.host.stdin.take().unwrap_or_default()
    }

    fn resolve(&self, name: &str) -> PathBuf {
        if name.starts_with('/') {
            PathBuf::from(name)
        } else {
            vfs_join(&self.host.cwd, name)
        }
    }

    fn total_out(&self) -> usize {
        self.stdout.len() + self.stderr.len() + self.redirected
    }
}

/// `name=value` command-line assignment operand.
pub(super) fn cmdline_assignment(arg: &str) -> Option<(&str, &str)> {
    let (name, value) = arg.split_once('=')?;
    let mut chars = name.chars();
    let first = chars.next()?;
    if !(first.is_ascii_alphabetic() || first == '_')
        || !chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        return None;
    }
    Some((name, value))
}

impl Interp {
    // ----- main input -----

    pub(super) async fn next_main_record(&mut self) -> R<Option<String>> {
        loop {
            if let Some(r) = self.io.main.cur.as_mut() {
                let rs = self.rs.clone();
                if let Some((rec, rt)) = r.read(&rs) {
                    if let Some(b) = &self.io.host.budget
                        && let Err(e) = b.consume_work(1)
                    {
                        return Err(self.budget_exhausted(e));
                    }
                    self.nr += 1.0;
                    self.fnr += 1.0;
                    self.globals[special::NR as usize] = Cell::Val(Value::Num(self.nr));
                    self.globals[special::FNR as usize] = Cell::Val(Value::Num(self.fnr));
                    self.globals[special::RT as usize] = Cell::Val(Value::Str(rt));
                    return Ok(Some(rec));
                }
                self.end_main_file().await?;
            }
            if !self.open_next_main().await? {
                return Ok(None);
            }
        }
    }

    async fn end_main_file(&mut self) -> R<()> {
        if self.io.main.cur.take().is_some() {
            let prog = self.prog.clone();
            self.run_blocks(&prog.endfile).await?;
        }
        Ok(())
    }

    /// `nextfile`: stop reading the current file.
    pub(super) async fn skip_file(&mut self) -> R<()> {
        self.end_main_file().await
    }

    fn argv_elem(&self, i: usize) -> Option<String> {
        let Cell::Arr(id) = self.globals[special::ARGV as usize] else {
            return None;
        };
        match self.array(id)?.get(&i.to_string())? {
            Elem::Val(v) => Some(self.to_str(v)),
            Elem::Arr(_) => None,
        }
    }

    async fn open_next_main(&mut self) -> R<bool> {
        // ARGV[0] is the program name.
        self.io.main.argi = self.io.main.argi.max(1);
        loop {
            let argc = match &self.globals[special::ARGC as usize] {
                Cell::Val(v) => v.to_num(),
                Cell::Arr(_) => 0.0,
            };
            let argc = if argc.is_finite() && argc > 0.0 {
                argc as usize
            } else {
                0
            };
            if self.io.main.argi >= argc {
                break;
            }
            let i = self.io.main.argi;
            self.io.main.argi += 1;
            let Some(arg) = self.argv_elem(i) else {
                continue;
            };
            if arg.is_empty() {
                continue;
            }
            if let Some((name, value)) = cmdline_assignment(&arg) {
                let value = super::lexer::unescape(value);
                self.assign_by_name(name, Value::from_input(value))?;
                continue;
            }
            self.io.main.used = true;
            let data = if arg == "-" || arg == "/dev/stdin" {
                self.io.take_stdin()
            } else {
                let path = self.io.resolve(&arg);
                if let Ok(meta) = self.io.host.fs.stat(&path).await
                    && meta.file_type.is_dir()
                {
                    let msg = format!(
                        "awk: warning: command line argument `{arg}' is a directory: skipped"
                    );
                    self.err_line(&msg);
                    continue;
                }
                match self.io.host.fs.read_file(&path).await {
                    Ok(bytes) => {
                        if let Some(b) = &self.io.host.budget
                            && let Err(e) = b.consume_input(bytes.len())
                        {
                            return Err(self.budget_exhausted(e));
                        }
                        String::from_utf8_lossy(&bytes).into_owned()
                    }
                    Err(e) => {
                        let reason = crate::error::io_error_reason(&e);
                        return Err(self
                            .fatal_at(&format!("cannot open file `{arg}' for reading: {reason}")));
                    }
                }
            };
            self.start_main_file(arg, data).await?;
            return Ok(true);
        }
        if !self.io.main.used {
            self.io.main.used = true;
            let data = self.io.take_stdin();
            self.start_main_file(String::new(), data).await?;
            return Ok(true);
        }
        Ok(false)
    }

    async fn start_main_file(&mut self, name: String, data: String) -> R<()> {
        self.set_var(
            super::ast::VarRef::Global(special::FILENAME),
            Value::Str(name),
        )?;
        self.fnr = 0.0;
        self.globals[special::FNR as usize] = Cell::Val(Value::Num(0.0));
        self.io.main.cur = Some(Reader::new(data));
        let prog = self.prog.clone();
        self.run_blocks(&prog.beginfile).await
    }

    pub(super) fn assign_by_name(&mut self, name: &str, v: Value) -> R<()> {
        if let Some(i) = self.prog.globals.iter().position(|g| g == name) {
            self.set_var(super::ast::VarRef::Global(i as u32), v)?;
        }
        Ok(())
    }

    // ----- getline -----

    pub(super) async fn getline(&mut self, src: &GetlineSrc, var: Option<&Expr>) -> R<Value> {
        match src {
            GetlineSrc::Main => match self.next_main_record().await? {
                Some(rec) => {
                    self.getline_store(var, rec).await?;
                    Ok(Value::Num(1.0))
                }
                None => Ok(Value::Num(0.0)),
            },
            GetlineSrc::File(f) => {
                let name = self.eval(f).await?;
                let name = self.to_str(&name);
                if !self.io.readers.contains_key(&name) && !self.open_file_reader(&name).await? {
                    return Ok(Value::Num(-1.0));
                }
                let rs = self.rs.clone();
                let rec = self.io.readers.get_mut(&name).and_then(|r| r.read(&rs));
                match rec {
                    Some((rec, rt)) => {
                        self.globals[special::RT as usize] = Cell::Val(Value::Str(rt));
                        self.getline_store(var, rec).await?;
                        Ok(Value::Num(1.0))
                    }
                    None => Ok(Value::Num(0.0)),
                }
            }
            GetlineSrc::Cmd(c) => {
                let cmd = self.eval(c).await?;
                let cmd = self.to_str(&cmd);
                if !self.io.readers.contains_key(&cmd) {
                    self.check_reader_slots()?;
                    let r = self.run_command(true, &cmd, None).await?;
                    let data = r.stdout.text_lossy().into_owned();
                    self.charge_reader(data.len())?;
                    let mut reader = Reader::new(data);
                    reader.status = r.exit_code;
                    self.io.readers.insert(cmd.clone(), reader);
                }
                let rs = self.rs.clone();
                let rec = self.io.readers.get_mut(&cmd).and_then(|r| r.read(&rs));
                match rec {
                    // Like gawk, redirected getline leaves NR and FNR alone.
                    Some((rec, rt)) => {
                        self.globals[special::RT as usize] = Cell::Val(Value::Str(rt));
                        self.getline_store(var, rec).await?;
                        Ok(Value::Num(1.0))
                    }
                    None => Ok(Value::Num(0.0)),
                }
            }
        }
    }

    async fn getline_store(&mut self, var: Option<&Expr>, rec: String) -> R<()> {
        match var {
            Some(v) => self.assign(v, Value::from_input(rec)).await,
            None => {
                self.set_record(rec);
                Ok(())
            }
        }
    }

    fn check_reader_slots(&mut self) -> R<()> {
        if self.io.readers.len() >= AWK_MAX_GETLINE_CACHED_FILES {
            return Err(self.fatal(&format!(
                "getline open file limit ({AWK_MAX_GETLINE_CACHED_FILES}) exceeded"
            )));
        }
        Ok(())
    }

    /// Charge a loaded input to the request budget and the retained cap.
    fn charge_reader(&mut self, len: usize) -> R<()> {
        if let Some(b) = &self.io.host.budget
            && let Err(e) = b
                .consume_input(len)
                .and_then(|()| b.consume_work(len as u64))
        {
            return Err(self.budget_exhausted(e));
        }
        let total = self.io.reader_bytes.saturating_add(len);
        if total > AWK_MAX_GETLINE_CACHE_BYTES {
            return Err(self.fatal(&format!(
                "getline total input limit ({AWK_MAX_GETLINE_CACHE_BYTES} bytes) exceeded"
            )));
        }
        self.io.reader_bytes = total;
        Ok(())
    }

    /// Open `name` for `getline <`; `false` when it cannot be read.
    /// gawk's `ERRNO`: why the last `getline` or `close` failed.
    fn set_errno(&mut self, reason: &str) {
        let reason = super::interp::truncate(reason, 200).to_string();
        self.globals[special::ERRNO as usize] = Cell::Val(Value::Str(reason));
    }

    async fn open_file_reader(&mut self, name: &str) -> R<bool> {
        self.check_reader_slots()?;
        let data = if name == "-" || name == "/dev/stdin" {
            self.io.take_stdin()
        } else {
            let path = self.io.resolve(name);
            let fs = self.io.host.fs.clone();
            match fs.stat(&path).await {
                Ok(meta) if meta.file_type.is_dir() => {
                    self.set_errno("Is a directory");
                    return Ok(false);
                }
                Ok(meta) if meta.size > AWK_MAX_GETLINE_FILE_BYTES as u64 => {
                    return Err(self.fatal(&format!(
                        "getline input file size limit ({AWK_MAX_GETLINE_FILE_BYTES} bytes) exceeded"
                    )));
                }
                Ok(_) => {}
                Err(e) => {
                    self.set_errno(&crate::error::io_error_reason(&e));
                    return Ok(false);
                }
            }
            match fs.read_file(&path).await {
                Ok(bytes) if bytes.len() > AWK_MAX_GETLINE_FILE_BYTES => {
                    return Err(self.fatal(&format!(
                        "getline input file size limit ({AWK_MAX_GETLINE_FILE_BYTES} bytes) exceeded"
                    )));
                }
                Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
                Err(e) => {
                    self.set_errno(&crate::error::io_error_reason(&e));
                    return Ok(false);
                }
            }
        };
        self.charge_reader(data.len())?;
        self.io.readers.insert(name.to_string(), Reader::new(data));
        Ok(true)
    }

    // ----- output -----

    pub(super) async fn write_out(&mut self, dest: Option<&Redirect>, text: String) -> R<()> {
        if text.len() > AWK_MAX_OUTPUT_BYTES.saturating_sub(self.io.total_out()) {
            return Err(self.fatal_plain("output limit exceeded (max 10MB)"));
        }
        let Some(dest) = dest else {
            self.io.stdout.push_str(&text);
            return Ok(());
        };
        let (target, append, pipe) = match dest {
            Redirect::File(e) => (e, false, false),
            Redirect::Append(e) => (e, true, false),
            Redirect::Pipe(e) => (e, false, true),
        };
        let name = self.eval(target).await?;
        let name = self.to_str(&name);
        if !pipe {
            match name.as_str() {
                "/dev/stdout" | "/dev/fd/1" => {
                    self.io.stdout.push_str(&text);
                    return Ok(());
                }
                "/dev/stderr" | "/dev/fd/2" => {
                    self.io.stderr.push_str(&text);
                    return Ok(());
                }
                "/dev/null" => return Ok(()),
                _ => {}
            }
        }
        if !self.io.outputs.contains_key(&name) {
            if self.io.outputs.len() >= AWK_MAX_OUTPUT_TARGETS {
                return Err(self.fatal_plain("too many output redirection targets"));
            }
            let kind = if pipe {
                OutKind::Pipe {
                    command: name.clone(),
                }
            } else {
                OutKind::File {
                    path: self.io.resolve(&name),
                    append,
                    written: false,
                }
            };
            self.io.outputs.insert(
                name.clone(),
                Output {
                    kind,
                    buf: String::new(),
                },
            );
            self.io.out_order.push(name.clone());
        }
        self.io.redirected += text.len();
        let flush = {
            let out = self.io.outputs.get_mut(&name).ok_or(Unwind::Fatal)?;
            out.buf.push_str(&text);
            matches!(out.kind, OutKind::File { .. }) && out.buf.len() >= FILE_FLUSH_BYTES
        };
        if flush {
            self.flush_output(&name).await?;
        }
        Ok(())
    }

    /// Write a file redirection's buffer through the VFS.
    async fn flush_output(&mut self, name: &str) -> R<()> {
        let fs = self.io.host.fs.clone();
        let Some(out) = self.io.outputs.get_mut(name) else {
            return Ok(());
        };
        let OutKind::File {
            path,
            append,
            written,
        } = &mut out.kind
        else {
            return Ok(());
        };
        if *written && out.buf.is_empty() {
            return Ok(());
        }
        let bytes = std::mem::take(&mut out.buf);
        let mut truncate = !(*append || *written);
        *written = true;
        let path = path.clone();
        let write = |truncate: bool, data: &str| {
            let fs = fs.clone();
            let path = path.clone();
            let data = data.as_bytes().to_vec();
            async move {
                if truncate {
                    fs.write_file(&path, &data).await
                } else {
                    fs.append_file(&path, &data).await
                }
            }
        };
        let mut res = write(truncate, &bytes).await;
        if res.is_err() && bytes.trim_end_matches('\n').contains('\n') {
            // A quota rejected the chunk: keep the lines that fit, as if
            // each print had been written on its own.
            for line in bytes.split_inclusive('\n') {
                res = write(truncate, line).await;
                if res.is_err() {
                    break;
                }
                truncate = false;
            }
        }
        if let Err(e) = res {
            let reason = crate::error::io_error_reason(&e);
            return Err(self.fatal_plain(&format!("cannot write {name}: {reason}")));
        }
        Ok(())
    }

    pub(super) async fn flush_all(&mut self) -> R<()> {
        let names: Vec<String> = self.io.out_order.clone();
        for n in names {
            self.flush_output(&n).await?;
        }
        Ok(())
    }

    /// `close(name)`: the command's exit status, 0 for files, -1 if not
    /// open.
    pub(super) async fn close_stream(&mut self, name: &str) -> R<f64> {
        if self.io.outputs.contains_key(name) {
            let r = self.flush_output(name).await;
            let out = self.io.outputs.remove(name);
            self.io.out_order.retain(|n| n != name);
            r?;
            if let Some(Output {
                kind: OutKind::Pipe { command },
                buf,
            }) = out
            {
                let res = self.run_command(false, &command, Some(buf)).await?;
                return Ok(res.exit_code as f64);
            }
            return Ok(0.0);
        }
        if let Some(r) = self.io.readers.remove(name) {
            // Retained bytes are freed; the request budget is not refunded.
            self.io.reader_bytes = self.io.reader_bytes.saturating_sub(r.data.len());
            return Ok(r.status as f64);
        }
        Ok(-1.0)
    }

    /// Flush files and run pending output pipes at exit.
    pub(super) async fn close_all(&mut self) {
        let names: Vec<String> = self.io.out_order.clone();
        for n in names {
            if let Err(Unwind::Fatal) = Box::pin(self.close_stream(&n)).await
                && self.fatal
            {
                // Keep closing the rest; the first error is reported.
                continue;
            }
        }
    }

    // ----- commands -----

    /// Run `command` with `sh -c` through the plan driver.
    pub(super) async fn run_command(
        &mut self,
        capture: bool,
        command: &str,
        stdin: Option<String>,
    ) -> R<ExecResult> {
        let Some(bridge) = self.io.host.bridge.clone() else {
            if !self.io.warned_no_commands {
                self.io.warned_no_commands = true;
                self.err_line("awk: running commands is not supported in this context");
            }
            return Ok(ExecResult::with_code(String::new(), 127));
        };
        // Commands see redirected output written so far (gawk flushes).
        self.flush_all().await?;
        let (emit_stdout, emit_stderr) = if capture {
            (String::new(), String::new())
        } else {
            let out = self.io.stdout[self.io.emitted.0..].to_string();
            let err = self.io.stderr[self.io.emitted.1..].to_string();
            self.io.emitted = (self.io.stdout.len(), self.io.stderr.len());
            (out, err)
        };
        if let Ok(mut s) = bridge.lock() {
            s.request = Some(CmdRequest {
                capture,
                command: command.to_string(),
                stdin,
                emit_stdout,
                emit_stderr,
            });
        }
        let res = WaitResponse(bridge).await;
        if !capture {
            // The shell streamed this output already; keep it in ours for
            // callers that capture the whole result.
            let out = res.stdout.text_lossy();
            let err = res.stderr.text_lossy();
            if out.len() + err.len() > AWK_MAX_OUTPUT_BYTES.saturating_sub(self.io.total_out()) {
                return Err(self.fatal_plain("output limit exceeded (max 10MB)"));
            }
            self.io.stdout.push_str(&out);
            self.io.stderr.push_str(&err);
            self.io.emitted = (self.io.stdout.len(), self.io.stderr.len());
        } else {
            let err = res.stderr.text_lossy().into_owned();
            self.io.stderr.push_str(&err);
        }
        Ok(res)
    }
}
