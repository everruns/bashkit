//! bash command history: the `history` and `fc` builtins, `$HISTFILE`,
//! `$HISTSIZE`/`$HISTFILESIZE`, and line-by-line recording of the shell's
//! own input (`set -o history`, `bash -i`).
//!
//! Decisions:
//! - One history list per interpreter (`Interpreter::history`), the same one
//!   the embedder API records into after each `exec()`. bash semantics apply
//!   on top: with `set -o history` on (a `bash -i` child turns it on) each
//!   top-level input line is recorded as it is read, before it runs, and the
//!   per-exec recording then adds nothing for that exec.
//! - Growth stays bounded by `max_history_entries`/`max_history_bytes`
//!   (TM-DOS-094) whatever `$HISTSIZE` says; `$HISTFILE` reads are capped by
//!   `max_input_bytes` (TM-DOS-131). `$HISTFILE` is read and written through
//!   the VFS only.
//! - A `bash -i` child gets its own history, loaded from `$HISTFILE` and
//!   written back on exit (only when it added lines), and the parent's list
//!   comes back afterwards: a child shell is a separate process in bash.
//! - `fc` lists (`-l`) and re-runs (`-s`, `-e -`); an editor (`fc` alone,
//!   `fc -e vi`) is not available (L-HIST-001).

use std::path::PathBuf;
use std::sync::Arc;

use super::{ExecResult, HistoryEntry, Interpreter};
use crate::error::Result;
use crate::parser::{Redirect, Script};

/// bash history positions and numbering, beside the shared entry list.
#[derive(Debug, Clone, Default)]
pub(crate) struct BashHistory {
    /// Entries dropped from the front (stifling, caps): entry `i` is
    /// numbered `dropped + i + 1`. `history -c` resets it.
    pub(crate) dropped: usize,
    /// First entry not yet written by `history -a`/exit (bash's
    /// `history_lines_this_session` boundary).
    pub(crate) appended: usize,
    /// Lines of `$HISTFILE` already read or written (`history -n`).
    pub(crate) lines_in_file: usize,
    /// This exec recorded lines as it read them.
    pub(crate) recorded_live: bool,
    /// The line running now was recorded (so `fc -l`/`history -s` skip or
    /// replace it, as bash does with `hist_last_line_added`).
    pub(crate) current_live: bool,
    /// `HISTFILESIZE` changed: truncate `$HISTFILE` before the next line.
    pub(crate) pending_truncate: bool,
}

/// The shell's own input, read command line by command line.
pub(crate) struct LineReader {
    /// The script being read (identity only, never dereferenced).
    script_addr: usize,
    source: Arc<str>,
    /// Byte offset of each line start, built on first use.
    line_starts: Vec<usize>,
    /// Last source line handed to the shell.
    last_line: usize,
    /// `bash -i` reading stdin: prompts and PROMPT_COMMAND.
    pub(crate) interactive: bool,
}

impl LineReader {
    pub(crate) fn new(script: &Script, source: Arc<str>, interactive: bool) -> Self {
        Self {
            script_addr: script as *const Script as usize,
            source,
            line_starts: Vec::new(),
            last_line: 0,
            interactive,
        }
    }

    pub(super) fn reads(&self, script: &Script) -> bool {
        self.script_addr == script as *const Script as usize
    }

    /// Source lines `first..=last` (1-based), without the final newline.
    fn lines(&mut self, first: usize, last: usize) -> String {
        if self.line_starts.is_empty() {
            self.line_starts.push(0);
            for (i, b) in self.source.bytes().enumerate() {
                if b == b'\n' {
                    self.line_starts.push(i + 1);
                }
            }
        }
        let start = self
            .line_starts
            .get(first.saturating_sub(1))
            .copied()
            .unwrap_or(self.source.len());
        let end = self
            .line_starts
            .get(last)
            .copied()
            .unwrap_or(self.source.len());
        self.source
            .get(start..end.max(start))
            .unwrap_or("")
            .trim_end_matches('\n')
            .to_string()
    }
}

/// What the interactive part of reading a line has to do.
pub(super) enum LineWork {
    None,
    /// Run PROMPT_COMMAND / print the prompt / truncate `$HISTFILE`.
    Async,
}

impl Interpreter {
    /// Install the reader of the top-level script's text (`Bash::exec`).
    pub(crate) fn set_top_level_source(&mut self, script: &Script, source: &str) {
        self.line_reader = Some(Box::new(LineReader::new(script, Arc::from(source), false)));
        self.bash_hist.recorded_live = false;
    }

    /// The per-exec recording is skipped when the exec recorded live.
    pub(crate) fn take_history_recorded_live(&mut self) -> bool {
        self.line_reader = None;
        std::mem::take(&mut self.bash_hist.recorded_live)
    }

    /// `\!`: the history number of the line being run.
    pub(super) fn history_number(&self) -> usize {
        if self.history.is_empty() {
            self.bash_hist.dropped + 1
        } else {
            self.bash_hist.dropped + self.history.len()
        }
    }

    fn history_enabled(&self) -> bool {
        self.scoped
            .variables
            .get("SHOPT_history")
            .is_some_and(|v| v == "1")
    }

    /// A top-level command of `script` is about to run: when it starts a new
    /// input line, count it (`\#`) and record the line in the history.
    #[inline(never)]
    pub(super) fn top_level_line(
        &mut self,
        script: &Script,
        index: usize,
        start_line: usize,
    ) -> LineWork {
        let Some(reader) = self.line_reader.as_mut() else {
            return LineWork::None;
        };
        if !reader.reads(script) {
            return LineWork::None;
        }
        let end_line = script
            .command_end_lines
            .get(index)
            .copied()
            .unwrap_or(start_line);
        if end_line <= reader.last_line && start_line <= reader.last_line {
            return LineWork::None;
        }
        // `(( ))`/`[[ ]]` carry no line of their own: their end line is it.
        let start_line = if start_line > reader.last_line && start_line <= end_line {
            start_line
        } else {
            end_line
        };
        let end_line = end_line.max(start_line);
        reader.last_line = end_line;
        let interactive = reader.interactive;
        self.command_number += 1;
        self.bash_hist.current_live = false;
        if self.history_enabled() {
            let text = self
                .line_reader
                .as_mut()
                .map(|r| r.lines(start_line, end_line))
                .unwrap_or_default();
            if !text.trim().is_empty() {
                self.push_bash_history(text);
                self.bash_hist.recorded_live = true;
                self.bash_hist.current_live = true;
            }
        }
        if interactive || self.bash_hist.pending_truncate {
            LineWork::Async
        } else {
            LineWork::None
        }
    }

    /// Add one line to the history (bash `add_history`), then stifle to
    /// `$HISTSIZE`.
    pub(super) fn push_bash_history(&mut self, command: String) {
        let cwd = self.cwd.to_string_lossy().to_string();
        let timestamp = crate::time_compat::now_utc().timestamp();
        self.record_history(command, timestamp, cwd, 0, 0);
        self.stifle_history();
    }

    /// Drop the `n` oldest entries, keeping every position in step.
    pub(super) fn drop_oldest_history(&mut self, n: usize) {
        let n = n.min(self.history.len());
        if n == 0 {
            return;
        }
        let freed: usize = self.history[..n]
            .iter()
            .map(HistoryEntry::retained_bytes)
            .sum();
        self.history.drain(..n);
        self.history_bytes = self.history_bytes.saturating_sub(freed);
        self.history_saved_entries = self.history_saved_entries.saturating_sub(n);
        self.history_needs_rewrite = true;
        self.bash_hist.dropped += n;
        self.bash_hist.appended = self.bash_hist.appended.saturating_sub(n);
    }

    /// `$HISTSIZE`: keep at most that many entries (a negative or
    /// non-numeric value means no limit beyond the sandbox caps).
    pub(super) fn stifle_history(&mut self) {
        let Some(max) = self
            .scoped
            .variables
            .get("HISTSIZE")
            .and_then(|v| v.trim().parse::<i64>().ok())
            .filter(|n| *n >= 0)
        else {
            return;
        };
        let max = usize::try_from(max).unwrap_or(usize::MAX);
        if self.history.len() > max {
            self.drop_oldest_history(self.history.len() - max);
        }
    }

    /// The async part of reading a line: `$HISTFILESIZE` truncation, and for
    /// `bash -i` PROMPT_COMMAND then the prompt (on stderr).
    /// Boxed so the hot script loop holds only a pointer, never this
    /// future's state (stack budget, see `stack_overflow_regression_tests`).
    #[inline(never)]
    pub(super) fn top_level_line_async(
        &mut self,
        eof: bool,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = (crate::StreamData, crate::StreamData)> + Send + '_>,
    > {
        Box::pin(self.top_level_line_work(eof))
    }

    async fn top_level_line_work(&mut self, eof: bool) -> (crate::StreamData, crate::StreamData) {
        let mut out = crate::StreamData::new();
        let mut err = crate::StreamData::new();
        if std::mem::take(&mut self.bash_hist.pending_truncate) {
            self.truncate_histfile().await;
        }
        let interactive = self.line_reader.as_ref().is_some_and(|r| r.interactive);
        if !interactive {
            return (out, err);
        }
        if let Some(r) = self.run_prompt_command().await {
            out.append(&r.stdout);
            err.append(&r.stderr);
        }
        let ps1 = self.expand_variable("PS1");
        let prompt = self.expand_prompt_string(ps1).await.unwrap_or_default();
        let emit_before = self.output_emit_count;
        let mut text = prompt;
        if eof {
            text.push_str("exit\n");
        }
        let prompt = crate::StreamData::from(text);
        self.maybe_emit_output(&crate::StreamData::new(), &prompt, emit_before);
        err.append(&prompt);
        (out, err)
    }

    /// PROMPT_COMMAND, run before each prompt. `$?` is left as it was; a
    /// parse or runtime error in it is reported and otherwise ignored.
    async fn run_prompt_command(&mut self) -> Option<ExecResult> {
        let cmd = self.expand_variable("PROMPT_COMMAND");
        if cmd.is_empty() {
            return None;
        }
        let saved_status = self.last_exit_code;
        let emit_before = self.output_emit_count;
        let result = match self.parse_embedded_script(&cmd).await {
            Ok(script) => self
                .execute_command_sequence(&script.commands)
                .await
                .unwrap_or_else(|e| ExecResult::err(self.diag(format!("{e}\n")), 1)),
            Err(e) => ExecResult::err(self.diag(format!("{e}\n")), 2),
        };
        self.last_exit_code = saved_status;
        self.maybe_emit_output(&result.stdout, &result.stderr, emit_before);
        Some(result)
    }

    /// `$HISTFILE` as a VFS path (`None` when unset or empty).
    fn histfile_path(&self, explicit: Option<&str>) -> Option<PathBuf> {
        let name = match explicit {
            Some(name) => name.to_string(),
            None => self.expand_variable("HISTFILE"),
        };
        (!name.is_empty()).then(|| self.resolve_path(&name))
    }

    /// Read a history file's command lines (timestamp comments dropped).
    /// THREAT[TM-DOS-131]: bounded by `max_input_bytes`.
    async fn read_histfile(&self, path: &std::path::Path) -> Option<Vec<String>> {
        // Size check before reading so a huge file is never loaded.
        let size = self.fs.stat(path).await.ok()?.size;
        if size > self.limits.max_input_bytes as u64 {
            return None;
        }
        let bytes = self.fs.read_file(path).await.ok()?;
        if bytes.len() > self.limits.max_input_bytes {
            return None;
        }
        let text = String::from_utf8_lossy(&bytes);
        Some(
            text.lines()
                .filter(|l| {
                    !(l.starts_with('#')
                        && l.len() > 1
                        && l[1..].bytes().all(|b| b.is_ascii_digit()))
                })
                .map(str::to_string)
                .collect(),
        )
    }

    fn history_text(&self, from: usize) -> String {
        let mut text = String::new();
        for entry in self.history.iter().skip(from) {
            text.push_str(&entry.command);
            text.push('\n');
        }
        text
    }

    /// `$HISTFILESIZE`: keep only that many lines of `$HISTFILE`.
    async fn truncate_histfile(&mut self) {
        let Some(max) = self
            .scoped
            .variables
            .get("HISTFILESIZE")
            .and_then(|v| v.trim().parse::<i64>().ok())
            .filter(|n| *n >= 0)
        else {
            return;
        };
        let Some(path) = self.histfile_path(None) else {
            return;
        };
        let Some(lines) = self.read_histfile(&path).await else {
            return;
        };
        let max = usize::try_from(max).unwrap_or(usize::MAX);
        if lines.len() <= max {
            return;
        }
        let mut text = lines[lines.len() - max..].join("\n");
        if !text.is_empty() {
            text.push('\n');
        }
        let _ = self.fs.write_file(&path, text.as_bytes()).await;
    }

    /// Note an assignment to a history variable.
    #[inline(never)]
    pub(super) fn history_variable_assigned(&mut self, name: &str) {
        match name {
            "HISTSIZE" => self.stifle_history(),
            "HISTFILESIZE" => self.bash_hist.pending_truncate = true,
            _ => {}
        }
    }

    /// Load `$HISTFILE` into a fresh history (start of `bash -i`).
    pub(super) async fn load_histfile(&mut self) {
        let Some(path) = self.histfile_path(None) else {
            return;
        };
        if let Some(lines) = self.read_histfile(&path).await {
            self.bash_hist.lines_in_file = lines.len();
            for line in lines {
                self.push_bash_history(line);
            }
        }
        self.bash_hist.appended = self.history.len();
    }

    /// End of `bash -i`: write the history back when this session added
    /// lines (append with `shopt -s histappend`), then apply
    /// `$HISTFILESIZE`.
    pub(super) async fn save_histfile(&mut self) {
        let new_lines = self.history.len().saturating_sub(self.bash_hist.appended);
        if new_lines == 0 {
            return;
        }
        let Some(path) = self.histfile_path(None) else {
            return;
        };
        if crate::builtins::shopt_on(&self.scoped.variables, "histappend") {
            let text = self.history_text(self.bash_hist.appended);
            let _ = self.fs.append_file(&path, text.as_bytes()).await;
        } else {
            let text = self.history_text(0);
            let _ = self.fs.write_file(&path, text.as_bytes()).await;
        }
        self.bash_hist.appended = self.history.len();
        self.truncate_histfile().await;
    }

    /// The `history` builtin, bash's options.
    pub(super) async fn execute_history_builtin(
        &mut self,
        args: &[String],
        redirects: &[Redirect],
    ) -> Result<ExecResult> {
        let result = self.history_builtin(args).await;
        self.redirect_result(result, redirects).await
    }

    async fn history_builtin(&mut self, args: &[String]) -> ExecResult {
        let usage = "history: usage: history [-c] [-d offset] [n] or history -anrw [filename] or history -ps arg [arg...]\n";
        let mut clear = false;
        let mut delete: Option<String> = None;
        let mut file_op: Option<char> = None;
        let mut print_args = false;
        let mut store_args = false;
        let mut idx = 0;
        while idx < args.len() {
            let arg = &args[idx];
            if arg == "--" {
                idx += 1;
                break;
            }
            if !arg.starts_with('-')
                || arg.len() < 2
                || arg[1..].starts_with(|c: char| c.is_ascii_digit())
            {
                break;
            }
            for c in arg[1..].chars() {
                match c {
                    'c' => clear = true,
                    'a' | 'n' | 'r' | 'w' => {
                        if file_op.is_some_and(|op| op != c) {
                            return ExecResult::err(
                                self.diag(format!(
                                    "history: cannot use more than one of -anrw\n{usage}"
                                )),
                                1,
                            );
                        }
                        file_op = Some(c);
                    }
                    'p' => print_args = true,
                    's' => store_args = true,
                    'd' => {
                        idx += 1;
                        let Some(value) = args.get(idx) else {
                            return ExecResult::err(
                                self.diag(format!(
                                    "history: -d: option requires an argument\n{usage}"
                                )),
                                2,
                            );
                        };
                        delete = Some(value.clone());
                    }
                    other => {
                        return ExecResult::err(
                            self.diag(format!("history: -{other}: invalid option\n{usage}")),
                            2,
                        );
                    }
                }
            }
            idx += 1;
        }
        let rest = &args[idx..];

        if clear {
            self.clear_history();
            self.bash_hist.dropped = 0;
            self.bash_hist.appended = 0;
            self.bash_hist.current_live = false;
            // Persist now: `history -c` is a same-exec sanitization boundary
            // for the builder's history file.
            Box::pin(self.save_history()).await;
            if delete.is_none() && file_op.is_none() && !print_args && !store_args {
                return ExecResult::ok(String::new());
            }
        }

        if let Some(spec) = delete {
            return self.history_delete(&spec);
        }

        if print_args {
            // No history expansion in bashkit: the words print as given.
            // Like bash, the `history -p` line itself leaves the list.
            self.drop_current_history_line();
            let mut out = String::new();
            for word in rest {
                out.push_str(word);
                out.push('\n');
            }
            return ExecResult::ok(out);
        }

        if store_args {
            self.drop_current_history_line();
            if !rest.is_empty() {
                self.push_bash_history(rest.join(" "));
            }
            return ExecResult::ok(String::new());
        }

        if let Some(op) = file_op {
            return self
                .history_file_op(op, rest.first().map(String::as_str))
                .await;
        }

        let count = match rest {
            [] => None,
            [n] => match n.parse::<i64>() {
                Ok(n) => Some(usize::try_from(n.max(0)).unwrap_or(0)),
                Err(_) => {
                    return ExecResult::err(
                        self.diag(format!("history: {n}: numeric argument required\n")),
                        1,
                    );
                }
            },
            _ => {
                return ExecResult::err(self.diag("history: too many arguments\n"), 1);
            }
        };
        self.history_listing(count)
    }

    /// The line running now leaves the list (`history -s`, `history -p`).
    fn drop_current_history_line(&mut self) {
        if std::mem::take(&mut self.bash_hist.current_live) && !self.history.is_empty() {
            let last = self.history.len() - 1;
            self.remove_history_range(last, last);
        }
    }

    fn remove_history_range(&mut self, first: usize, last: usize) {
        let freed: usize = self.history[first..=last]
            .iter()
            .map(HistoryEntry::retained_bytes)
            .sum();
        let n = last - first + 1;
        self.history.drain(first..=last);
        self.history_bytes = self.history_bytes.saturating_sub(freed);
        self.history_needs_rewrite = true;
        self.history_saved_entries = self.history_saved_entries.min(self.history.len());
        if self.bash_hist.appended > first {
            self.bash_hist.appended = self.bash_hist.appended.saturating_sub(n).max(first);
        }
    }

    /// `history -d N`, `-d -N` (from the end), `-d A-B`.
    fn history_delete(&mut self, spec: &str) -> ExecResult {
        let len = self.history.len();
        let base = self.bash_hist.dropped;
        let position = |text: &str| -> Option<usize> {
            let n: i64 = text.parse().ok()?;
            let index = if n < 0 {
                i64::try_from(len).ok()? + n
            } else {
                n - 1 - i64::try_from(base).ok()?
            };
            usize::try_from(index).ok().filter(|i| *i < len)
        };
        let range = match spec[1..].find('-').map(|i| i + 1) {
            Some(dash) if !spec.is_empty() => {
                position(&spec[..dash]).zip(position(&spec[dash + 1..]))
            }
            _ => position(spec).map(|i| (i, i)),
        };
        match range {
            Some((first, last)) if first <= last => {
                self.remove_history_range(first, last);
                ExecResult::ok(String::new())
            }
            _ => ExecResult::err(
                self.diag(format!("history: {spec}: history position out of range\n")),
                1,
            ),
        }
    }

    /// `history -a/-n/-r/-w [file]`.
    async fn history_file_op(&mut self, op: char, file: Option<&str>) -> ExecResult {
        let Some(path) = self.histfile_path(file) else {
            return ExecResult::with_code("", 1);
        };
        match op {
            'r' | 'n' => {
                let Some(lines) = self.read_histfile(&path).await else {
                    return ExecResult::with_code("", 1);
                };
                let skip = if op == 'n' {
                    self.bash_hist.lines_in_file.min(lines.len())
                } else {
                    0
                };
                let total = lines.len();
                for line in lines.into_iter().skip(skip) {
                    self.push_bash_history(line);
                }
                self.bash_hist.lines_in_file = total;
                self.bash_hist.appended = self.history.len();
                ExecResult::ok(String::new())
            }
            'a' => {
                let text = self.history_text(self.bash_hist.appended);
                let lines = self.history.len().saturating_sub(self.bash_hist.appended);
                if self.fs.append_file(&path, text.as_bytes()).await.is_err() {
                    return ExecResult::with_code("", 1);
                }
                self.bash_hist.appended = self.history.len();
                self.bash_hist.lines_in_file += lines;
                ExecResult::ok(String::new())
            }
            _ => {
                let text = self.history_text(0);
                if self.fs.write_file(&path, text.as_bytes()).await.is_err() {
                    return ExecResult::with_code("", 1);
                }
                self.bash_hist.appended = self.history.len();
                self.bash_hist.lines_in_file = self.history.len();
                ExecResult::ok(String::new())
            }
        }
    }

    /// `history [n]`: `%5d  command` lines.
    /// THREAT[TM-DOS-109]: cut at `max_history_output_bytes`.
    fn history_listing(&self, count: Option<usize>) -> ExecResult {
        let len = self.history.len();
        let start = count.map_or(0, |n| len.saturating_sub(n));
        let max = self.limits.max_history_output_bytes;
        let mut out = String::new();
        for (i, entry) in self.history.iter().enumerate().skip(start) {
            let line = format!("{:>5}  {}\n", self.bash_hist.dropped + i + 1, entry.command);
            if out.len().saturating_add(line.len()) > max {
                return crate::builtins::limits::cap_exceeded(
                    "history",
                    out,
                    "output",
                    format!("{max} bytes"),
                );
            }
            out.push_str(&line);
        }
        ExecResult::ok(out)
    }

    /// The `fc` builtin: `-l` lists, `-s`/`-e -` re-run a command.
    pub(super) async fn execute_fc_builtin(
        &mut self,
        args: &[String],
        redirects: &[Redirect],
    ) -> Result<ExecResult> {
        let usage = "fc: usage: fc [-e ename] [-lnr] [first] [last] or fc -s [pat=rep] [command]\n";
        let mut list = false;
        let mut numbers = true;
        let mut reverse = false;
        let mut rerun = false;
        let mut editor: Option<String> = None;
        let mut idx = 0;
        while idx < args.len() {
            let arg = &args[idx];
            if arg == "--" {
                idx += 1;
                break;
            }
            if !arg.starts_with('-')
                || arg.len() < 2
                || arg[1..].starts_with(|c: char| c.is_ascii_digit())
            {
                break;
            }
            for c in arg[1..].chars() {
                match c {
                    'l' => list = true,
                    'n' => numbers = false,
                    'r' => reverse = true,
                    's' => rerun = true,
                    'e' => {
                        idx += 1;
                        let Some(name) = args.get(idx) else {
                            let r = ExecResult::err(
                                self.diag(format!("fc: -e: option requires an argument\n{usage}")),
                                2,
                            );
                            return self.redirect_result(r, redirects).await;
                        };
                        editor = Some(name.clone());
                    }
                    other => {
                        let r = ExecResult::err(
                            self.diag(format!("fc: -{other}: invalid option\n{usage}")),
                            2,
                        );
                        return self.redirect_result(r, redirects).await;
                    }
                }
            }
            idx += 1;
        }
        let rest = &args[idx..];
        if editor.as_deref() == Some("-") {
            rerun = true;
        }
        if list {
            let r = self.fc_list(rest, numbers, reverse);
            return self.redirect_result(r, redirects).await;
        }
        if !rerun {
            let r = ExecResult::err(
                self.diag("fc: editing history is not supported in bashkit\n"),
                1,
            );
            return self.redirect_result(r, redirects).await;
        }
        Box::pin(self.fc_rerun(rest, redirects)).await
    }

    /// Last entry `fc` may address: the running `fc` line itself is not one.
    fn fc_last_index(&self) -> Option<usize> {
        let len = self.history.len();
        let len = if self.bash_hist.current_live {
            len.saturating_sub(1)
        } else {
            len
        };
        len.checked_sub(1)
    }

    /// An `fc` history reference: a number, `-N` back from the last, or
    /// the most recent command starting with the text.
    fn fc_resolve(&self, spec: &str, last: usize) -> Option<usize> {
        if let Ok(n) = spec.parse::<i64>() {
            let index = if n < 0 {
                i64::try_from(last).ok()? + 1 + n
            } else {
                n - 1 - i64::try_from(self.bash_hist.dropped).ok()?
            };
            let index = index.clamp(0, i64::try_from(last).ok()?);
            return usize::try_from(index).ok();
        }
        (0..=last)
            .rev()
            .find(|&i| self.history[i].command.starts_with(spec))
    }

    fn fc_list(&self, rest: &[String], numbers: bool, reverse: bool) -> ExecResult {
        let Some(last) = self.fc_last_index() else {
            return ExecResult::ok(String::new());
        };
        let first_spec = rest.first().map_or("-16", String::as_str);
        let last_spec = rest.get(1).map_or("-1", String::as_str);
        let (Some(first), Some(end)) = (
            self.fc_resolve(first_spec, last),
            self.fc_resolve(last_spec, last),
        ) else {
            return ExecResult::err(self.diag("fc: history specification out of range\n"), 1);
        };
        let descending = reverse || first > end;
        let (lo, hi) = (first.min(end), first.max(end));
        let mut indices: Vec<usize> = (lo..=hi).collect();
        if descending {
            indices.reverse();
        }
        let max = self.limits.max_history_output_bytes;
        let mut out = String::new();
        for i in indices {
            let line = if numbers {
                format!(
                    "{}\t {}\n",
                    self.bash_hist.dropped + i + 1,
                    self.history[i].command
                )
            } else {
                format!("\t {}\n", self.history[i].command)
            };
            if out.len().saturating_add(line.len()) > max {
                return crate::builtins::limits::cap_exceeded(
                    "fc",
                    out,
                    "output",
                    format!("{max} bytes"),
                );
            }
            out.push_str(&line);
        }
        ExecResult::ok(out)
    }

    /// `fc -s [old=new] [command]`: re-run a command, echoed on stderr; it
    /// replaces the `fc` line in the history.
    async fn fc_rerun(&mut self, rest: &[String], redirects: &[Redirect]) -> Result<ExecResult> {
        let (subst, rest) = match rest.first() {
            Some(first) if first.contains('=') => {
                let (old, new) = first.split_once('=').unwrap_or_default();
                (Some((old.to_string(), new.to_string())), &rest[1..])
            }
            _ => (None, rest),
        };
        let found = self
            .fc_last_index()
            .and_then(|last| self.fc_resolve(rest.first().map_or("-1", String::as_str), last));
        let Some(index) = found else {
            let r = ExecResult::err(self.diag("fc: no command found\n"), 1);
            return self.redirect_result(r, redirects).await;
        };
        let mut command = self.history[index].command.clone();
        if let Some((old, new)) = subst
            && !old.is_empty()
        {
            command = command.replacen(&old, &new, 1);
        }
        self.drop_current_history_line();
        if self.history_enabled() {
            self.push_bash_history(command.clone());
        }
        let echo = crate::StreamData::from(format!("{command}\n"));
        let emit_before = self.output_emit_count;
        self.maybe_emit_output(&crate::StreamData::new(), &echo, emit_before);
        let mut result = match self.parse_embedded_script(&command).await {
            Ok(script) => self.execute_command_sequence(&script.commands).await?,
            Err(e) => ExecResult::err(self.diag(format!("{e}\n")), 2),
        };
        let mut stderr = echo;
        stderr.append(&result.stderr);
        result.stderr = stderr;
        self.redirect_result(result, redirects).await
    }
}

/// The parent's history while a `bash -i` child runs with its own.
pub(super) struct ParentHistory {
    entries: Vec<HistoryEntry>,
    bytes: usize,
    saved_entries: usize,
    needs_rewrite: bool,
    bash_hist: BashHistory,
}

/// Future of `enter_interactive_shell`: the parent's history and the rc
/// file's result.
type EnterInteractive<'a> = std::pin::Pin<
    Box<dyn std::future::Future<Output = (Box<ParentHistory>, Option<ExecResult>)> + Send + 'a>,
>;

impl Interpreter {
    /// Start of a `bash -i` child: interactive variables, the rc file
    /// (`--rcfile FILE`, else `~/.bashrc`; none with `--norc`), then a fresh
    /// history loaded from `$HISTFILE`. Returns the parent's history and the
    /// rc file's result.
    #[inline(never)]
    pub(super) fn enter_interactive_shell(
        &mut self,
        rcfile: Option<String>,
        norc: bool,
    ) -> EnterInteractive<'_> {
        Box::pin(self.enter_interactive_work(rcfile, norc))
    }

    async fn enter_interactive_work(
        &mut self,
        rcfile: Option<String>,
        norc: bool,
    ) -> (Box<ParentHistory>, Option<ExecResult>) {
        let parent = Box::new(ParentHistory {
            entries: std::mem::take(&mut self.history),
            bytes: std::mem::take(&mut self.history_bytes),
            saved_entries: std::mem::take(&mut self.history_saved_entries),
            needs_rewrite: std::mem::take(&mut self.history_needs_rewrite),
            bash_hist: std::mem::take(&mut self.bash_hist),
        });
        let home = self.expand_variable("HOME");
        for (name, value) in [
            ("SHOPT_history", "1".to_string()),
            ("PS1", "\\s-\\v\\$ ".to_string()),
            ("PS2", "> ".to_string()),
            ("PS4", "+ ".to_string()),
            (
                "HISTFILE",
                format!("{}/.bash_history", home.trim_end_matches('/')),
            ),
            ("HISTSIZE", "500".to_string()),
            ("HISTFILESIZE", "500".to_string()),
        ] {
            if name == "SHOPT_history" || !self.is_variable_set(name) {
                self.insert_variable_checked(name.to_string(), value);
            }
        }

        let mut rc_result = None;
        if !norc {
            let path = rcfile.unwrap_or_else(|| "~/.bashrc".to_string());
            let path = match path.strip_prefix("~/") {
                Some(rest) => format!("{}/{rest}", home.trim_end_matches('/')),
                None => path,
            };
            let resolved = self.resolve_path(&path);
            if self
                .fs
                .stat(&resolved)
                .await
                .is_ok_and(|m| m.file_type.is_file())
            {
                rc_result = Some(
                    Box::pin(self.execute_source("source", &[path], &[]))
                        .await
                        .unwrap_or_else(|e| ExecResult::err(format!("{e}\n"), 1)),
                );
            }
        }
        self.load_histfile().await;
        (parent, rc_result)
    }

    /// End of a `bash -i` child: write its history to `$HISTFILE`, then
    /// bring back the parent's.
    #[inline(never)]
    pub(super) fn leave_interactive_shell(
        &mut self,
        parent: Box<ParentHistory>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + '_>> {
        Box::pin(self.leave_interactive_work(parent))
    }

    async fn leave_interactive_work(&mut self, parent: Box<ParentHistory>) {
        if self.history_enabled() {
            self.save_histfile().await;
        }
        let parent = *parent;
        self.history = parent.entries;
        self.history_bytes = parent.bytes;
        self.history_saved_entries = parent.saved_entries;
        self.history_needs_rewrite = parent.needs_rewrite;
        self.bash_hist = parent.bash_hist;
    }
}
