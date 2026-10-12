//! sed execution engine.
//!
//! Important decisions (issue #2427):
//!
//! - Output goes through [`Sink`], which mirrors GNU's "missing newline"
//!   bookkeeping: a line that arrived without a trailing newline is written
//!   without one, and the newline is only emitted if more output follows. That
//!   is what stops `printf 'a' | sed 's/a/b/'` from inventing a terminator and,
//!   with `-i`, writing it back to the file (finding E).
//! - Pattern/hold ambush (TM-DOS-018 follow-up): input consumption resets the
//!   per-cycle step counter but `N` preserves pattern space, so
//!   `:a;N;100~100{h;G};ba` doubles pattern+hold every 100 lines without
//!   tripping the step limit (~13 MB from ~3 KB of input at 1500 lines,
//!   unbounded beyond). Pattern and hold are therefore `BudgetedString`s
//!   charged against the live-intermediate budget before growth, and every
//!   command charges aggregate work plus cancellation/deadline checks. Growth
//!   refusal aborts through the sink error slot (`sed: <error>`, exit 1),
//!   never warn-and-continue with a silently truncated pattern.
//! - Retention discipline: the old owner stays charged through its last use.
//!   Copies go straight from source into a fresh budgeted destination
//!   (`h`/`g` drop the target charge first, then copy field-to-field with no
//!   snapshot scratch); rebuilds (`s///`, `y`, `D` drain) keep the moved-out
//!   owner alive while the replacement charges incrementally, swapping only
//!   at the end (a non-matching `s///` restores the original instead of
//!   wiping it). The one unavoidable transient, the input-line clone in
//!   `read_next`, is admitted as scratch lease before allocation and held
//!   until the fresh charged copy lands. A dead budget fails every
//!   construction/replacement closed with the original refusal; nothing
//!   falls back to unbudgeted storage.
//! - Range addresses carry explicit [`RangeState`] per program counter, with a
//!   resolved end line for `,N`, `,+N` and `,~N`, so a closed range cannot
//!   silently re-open (finding C).
//! - `s///` walks the match list itself instead of delegating to
//!   `Regex::replace*`, which is what makes `s/x/y/Ng` mean "the Nth and later"
//!   (finding F) and keeps `$` literal in the replacement (finding A).
//! - Input is one continuous stream across operands unless `-s`/`-i` asks for
//!   per-file streams, so `$`, line numbers and `q` behave like GNU (finding D).

use std::collections::HashMap;

use super::pattern::SedRegex;
use super::script::{Addr, CaseOp, EndAddr, Kind, Program, RepPart, StartAddr, Subst};
use crate::builtins::limits::SED_MAX_CYCLE_STEPS;
use crate::limits::{BudgetedString, ExecutionBudget, LimitExceeded};

/// One input line plus the two facts the engine needs about it.
pub(super) struct InputLine {
    pub(super) text: String,
    /// Whether the line was terminated in the input.
    pub(super) had_newline: bool,
    /// Index into the operand list, for `F` and for `-s`/`-i` segmentation.
    pub(super) file: usize,
}

/// Output buffer that reproduces GNU sed's missing-newline handling.
pub(super) struct Sink {
    pub(super) buf: BudgetedString,
    pub(super) error: Option<LimitExceeded>,
    missing_newline: bool,
    /// Line terminator: `\n`, or NUL under `-z`.
    sep: char,
}

impl Sink {
    fn with_sep(sep: char, budget: Option<&ExecutionBudget>) -> Self {
        let (buf, error) = match BudgetedString::new(budget) {
            Ok(buf) => (buf, None),
            Err(error) => (
                BudgetedString::new(None).expect("an unbudgeted empty string is infallible"),
                Some(error),
            ),
        };
        Sink {
            buf,
            error,
            missing_newline: false,
            sep,
        }
    }
}

impl Sink {
    /// Write one line, adding a terminator only when the source had one.
    fn line(&mut self, text: &str, had_newline: bool) {
        self.flush_pending();
        self.push_str(text);
        if had_newline {
            self.push(self.sep);
        } else {
            self.missing_newline = true;
        }
    }

    /// Write text that already carries its own terminator.
    fn raw(&mut self, text: &str) {
        self.flush_pending();
        self.push_str(text);
    }

    /// Emit a newline that was withheld because the source line had none.
    fn flush_pending(&mut self) {
        if self.missing_newline {
            self.push(self.sep);
            self.missing_newline = false;
        }
    }

    fn push_str(&mut self, text: &str) {
        if self.error.is_some() {
            return;
        }
        if let Err(error) = self.buf.try_push_str(text) {
            self.error = Some(error);
        }
    }

    fn push(&mut self, ch: char) {
        let mut encoded = [0; 4];
        self.push_str(ch.encode_utf8(&mut encoded));
    }
}

#[derive(Default, Clone)]
struct RangeState {
    active: bool,
    /// Resolved last line of the range for numeric / `+N` / `~N` ends.
    end_line: Option<usize>,
    /// A line-number range start fires at most once. It opens on the first
    /// line at or past it — `n`, `N` and `b` can carry the stream past the
    /// line before the range is ever evaluated — and must not re-open
    /// afterwards, when `line_no >= start` is still trivially true.
    start_spent: bool,
}

enum Append {
    Text(String),
    File(String),
    FileLine(String),
}

enum Cycle {
    /// Fell off the end of the script: auto-print unless `-n`.
    Auto,
    /// `d`, `n` at end of input, `Q`: no auto-print.
    Silent,
    /// `D` with an embedded newline: rerun the script without reading input.
    Restart,
    /// Abort the segment now; the error is parked on the output sink and
    /// surfaced as `Err` by the trailing sink-error check.
    Break,
}

pub(super) struct Machine<'a> {
    prog: &'a Program,
    names: &'a [String],
    read_files: &'a HashMap<String, String>,
    quiet: bool,
    default_line_len: usize,
    sep: char,
    execution_budget: Option<ExecutionBudget>,

    out: Sink,
    pub(super) stderr: String,
    pub(super) write_files: HashMap<String, Sink>,

    range: Vec<RangeState>,
    hold: BudgetedString,
    /// GNU tracks "was this line terminated" on the hold space too, and `g`,
    /// `G` and `x` carry it onto the pattern space. The hold space starts out
    /// terminated, which is why `printf 'ab' | sed G` emits a trailing newline
    /// even though the input had none.
    hold_had_newline: bool,
    appends: Vec<Append>,
    read_cursor: HashMap<String, usize>,
    last_regex: Option<std::sync::Arc<SedRegex>>,
    pub(super) exit_code: Option<i32>,
    warned: bool,
    /// `q` terminates the line it printed even when the input line had no
    /// terminator, unlike falling off the end of input. `Q`, which prints
    /// nothing, does not.
    flush_newline_on_quit: bool,

    // Per-segment state.
    lines: &'a [InputLine],
    cursor: usize,
    line_no: usize,
    ps: BudgetedString,
    ps_had_newline: bool,
    cur_file: usize,
    sub_made: bool,
}

/// Disjoint output handles for [`Machine::write_out`], so callers can pass
/// `&self.ps` slices straight in with no clone-scratch transient.
/// Behavior is identical to the former method form.
struct WriteHandles<'a> {
    sep: char,
    out: &'a mut Sink,
    err: &'a mut String,
    files: &'a mut HashMap<String, Sink>,
    budget: Option<&'a ExecutionBudget>,
}

impl<'a> Machine<'a> {
    pub(super) fn new(
        prog: &'a Program,
        names: &'a [String],
        read_files: &'a HashMap<String, String>,
        quiet: bool,
        line_len: usize,
        sep: char,
        execution_budget: Option<ExecutionBudget>,
    ) -> Result<Self, LimitExceeded> {
        let out = Sink::with_sep(sep, execution_budget.as_ref());
        // Fail closed before any growth: a dead budget refuses the machine
        // itself instead of running pattern/hold uncharged. Empty buffers
        // cannot trip a live budget, so only a dead one fails here.
        if let Some(budget) = &execution_budget {
            budget.check()?;
        }
        let hold = BudgetedString::new(execution_budget.as_ref())?;
        let ps = BudgetedString::new(execution_budget.as_ref())?;
        Ok(Machine {
            prog,
            names,
            read_files,
            quiet: quiet || prog.quiet,
            default_line_len: line_len,
            sep,
            out,
            stderr: String::new(),
            write_files: HashMap::new(),
            range: Vec::new(),
            hold,
            hold_had_newline: true,
            appends: Vec::new(),
            read_cursor: HashMap::new(),
            last_regex: None,
            exit_code: None,
            warned: false,
            flush_newline_on_quit: false,
            lines: &[],
            cursor: 0,
            line_no: 0,
            ps,
            execution_budget,
            ps_had_newline: true,
            cur_file: 0,
            sub_made: false,
        })
    }

    pub(super) fn finished(&self) -> bool {
        self.exit_code.is_some()
    }

    /// Refuse pattern/hold growth (or exhausted aggregate work / an expired
    /// deadline) by parking the error on the output sink. `run_segment`
    /// already aborts on a sink error, so this surfaces as `Err` ->
    /// `sed: <error>` with exit 1 instead of warn-and-continue with a
    /// silently truncated pattern space.
    fn space_fail(&mut self, error: LimitExceeded) -> Cycle {
        if self.out.error.is_none() {
            self.out.error = Some(error);
        }
        self.warned = true;
        Cycle::Break
    }

    /// A fresh budget-attached scratch buffer. Fails closed with the
    /// original refusal when the budget is dead: a poisoned/cancelled budget
    /// must terminate the segment, never silently continue uncharged. A
    /// genuinely unbudgeted context (`None`) stays exact.
    fn fresh_space(&self) -> Result<BudgetedString, LimitExceeded> {
        match &self.execution_budget {
            Some(budget) => {
                budget.check()?;
                BudgetedString::new(Some(budget))
            }
            None => BudgetedString::new(None),
        }
    }

    /// Take pattern space out, leaving a budget-attached empty behind.
    /// (`BudgetedString` is not `Default`, so `mem::take` cannot be used.)
    /// Fails closed like [`Self::fresh_space`] when the budget is dead.
    fn take_ps(&mut self) -> Result<BudgetedString, LimitExceeded> {
        let fresh = self.fresh_space()?;
        Ok(std::mem::replace(&mut self.ps, fresh))
    }

    /// Replace pattern space, dropping the old charge before taking the new
    /// one so a same-size rewrite near the limit is not double-counted.
    fn set_ps(&mut self, text: &str) -> Result<(), LimitExceeded> {
        self.ps = self.fresh_space()?;
        self.ps.try_push_str(text)
    }

    /// Run one input stream and return everything it wrote to stdout.
    pub(super) fn run_segment(&mut self, lines: &'a [InputLine]) -> Result<String, LimitExceeded> {
        self.lines = lines;
        self.cursor = 0;
        self.line_no = 0;
        self.range = vec![RangeState::default(); self.prog.cmds.len()];
        self.out = Sink::with_sep(self.sep, self.execution_budget.as_ref());

        let mut restart = false;
        // THREAT[TM-DOS]: `D` restarts the script without reading input, so a
        // script like `G;D` never terminates and grows the pattern space
        // without bound. The per-cycle step budget cannot see across restarts,
        // so they get their own budget, reset whenever a line is read.
        let mut restarts = 0usize;
        let mut restart_cursor = self.cursor;
        loop {
            if let Some(budget) = &self.execution_budget
                && let Err(error) = budget.check()
            {
                return Err(error);
            }
            if self.exit_code.is_some() {
                break;
            }
            // `D` restarts the script on what is left of the pattern space
            // without reading input, and GNU only clears the `t` flag when a
            // line is actually read, so the reset lives in `read_next`.
            if restart {
                restarts += 1;
                if restarts > SED_MAX_CYCLE_STEPS {
                    self.warn_loop_limit();
                    break;
                }
                if self.cursor != restart_cursor {
                    // The restarted cycles consumed input (`$!N;P;D`), so
                    // they are bounded by the input; only count restarts
                    // that read nothing.
                    restarts = 0;
                    restart_cursor = self.cursor;
                }
            } else {
                if !self.read_next() {
                    break;
                }
                restarts = 0;
                restart_cursor = self.cursor;
            }
            restart = false;

            match self.cycle() {
                Cycle::Break => {
                    break;
                }
                Cycle::Auto => {
                    if !self.quiet {
                        self.out.line(&self.ps, self.ps_had_newline);
                    }
                    self.flush_appends();
                }
                Cycle::Silent => self.flush_appends(),
                Cycle::Restart => {
                    self.flush_appends();
                    restart = true;
                }
            }

            if self.flush_newline_on_quit {
                self.out.flush_pending();
            }
        }

        let replacement = Sink::with_sep(self.sep, self.execution_budget.as_ref());
        let out = std::mem::replace(&mut self.out, replacement);
        match out.error {
            Some(error) => Err(error),
            None => Ok(out.buf.into_inner()),
        }
    }

    fn cycle(&mut self) -> Cycle {
        let cmds = &self.prog.cmds;
        let mut pc = 0usize;
        let mut steps = 0usize;
        // The step budget is per line read: a loop that keeps reading input
        // with `N`/`n` (`:b;$!N;...;bb`) is bounded by the input instead.
        let mut step_cursor = self.cursor;

        while pc < cmds.len() {
            if self.cursor != step_cursor {
                step_cursor = self.cursor;
                steps = 0;
            }
            if let Some(budget) = &self.execution_budget
                && let Err(error) = budget.check().and_then(|()| budget.consume_work(1))
            {
                return self.space_fail(error);
            }
            steps += 1;
            if steps > SED_MAX_CYCLE_STEPS {
                self.warn_loop_limit();
                break;
            }

            let cmd = &cmds[pc];
            let (matched, range_ended) = match &cmd.addr {
                None => (true, false),
                Some(addr) => self.address_matches(pc, addr),
            };
            let apply = if cmd.negate { !matched } else { matched };

            match &cmd.kind {
                Kind::Block(end) => {
                    pc = if apply { pc + 1 } else { *end + 1 };
                    continue;
                }
                Kind::BlockEnd | Kind::Label(_) | Kind::Nop => {
                    pc += 1;
                    continue;
                }
                _ => {}
            }

            if !apply {
                pc += 1;
                continue;
            }

            match &cmd.kind {
                Kind::Branch(label) => match label {
                    None => return Cycle::Auto,
                    Some(name) => {
                        pc = self.label_pc(name);
                        continue;
                    }
                },
                Kind::BranchIfSub(label) => {
                    if self.sub_made {
                        self.sub_made = false;
                        match label {
                            None => return Cycle::Auto,
                            Some(name) => {
                                pc = self.label_pc(name);
                                continue;
                            }
                        }
                    }
                }
                Kind::BranchIfNoSub(label) => {
                    if self.sub_made {
                        self.sub_made = false;
                    } else {
                        match label {
                            None => return Cycle::Auto,
                            Some(name) => {
                                pc = self.label_pc(name);
                                continue;
                            }
                        }
                    }
                }
                Kind::Substitute(subst) => {
                    if let Err(error) = self.substitute(subst) {
                        return self.space_fail(error);
                    }
                }
                Kind::Transliterate(map) => {
                    // Keep the old owner charged through the last use: build
                    // the translation into a fresh budgeted buffer reading
                    // straight from pattern space (local destination, no
                    // scratch String), then swap. Peak is ~2x pattern, all
                    // charged; a dead budget fails closed via space_fail.
                    let mut translated = match BudgetedString::new(self.execution_budget.as_ref()) {
                        Ok(buf) => buf,
                        Err(error) => return self.space_fail(error),
                    };
                    let mut failed = None;
                    for c in self.ps.chars() {
                        let mapped = map
                            .iter()
                            .find(|(from, _)| *from == c)
                            .map_or(c, |(_, to)| *to);
                        if let Err(error) = translated.try_push(mapped) {
                            failed = Some(error);
                            break;
                        }
                    }
                    if let Some(error) = failed {
                        return self.space_fail(error);
                    }
                    self.ps = translated;
                }
                Kind::Delete => return Cycle::Silent,
                Kind::DeleteFirstLine => {
                    return match self.ps.find(self.sep) {
                        Some(idx) => {
                            // No snapshot scratch: build the remainder into a
                            // fresh budgeted buffer while pattern space stays
                            // charged, then swap (old released after last use).
                            let start = idx + self.sep.len_utf8();
                            let mut rest = match BudgetedString::new(self.execution_budget.as_ref())
                            {
                                Ok(buf) => buf,
                                Err(error) => return self.space_fail(error),
                            };
                            if let Err(error) = rest.try_push_str(&self.ps[start..]) {
                                return self.space_fail(error);
                            }
                            self.ps = rest;
                            Cycle::Restart
                        }
                        None => Cycle::Silent,
                    };
                }
                Kind::Print => {
                    self.out.line(&self.ps, self.ps_had_newline);
                }
                Kind::PrintFirstLine => match self.ps.find(self.sep) {
                    Some(idx) => {
                        let end = idx;
                        self.out.line(&self.ps[..end], true);
                    }
                    None => self.out.line(&self.ps, self.ps_had_newline),
                },
                Kind::Next => {
                    if !self.quiet {
                        self.out.line(&self.ps, self.ps_had_newline);
                    }
                    self.flush_appends();
                    if !self.read_next() {
                        self.exit_code = Some(0);
                        return Cycle::Silent;
                    }
                }
                Kind::NextAppend => {
                    self.flush_appends();
                    if self.cursor >= self.lines.len() {
                        // GNU extension: print what we have and stop.
                        self.exit_code = Some(0);
                        return Cycle::Auto;
                    }
                    let line = &self.lines[self.cursor];
                    // GNU joins with the buffer delimiter (NUL under -z).
                    if let Err(error) = self
                        .ps
                        .try_push(self.sep)
                        .and_then(|()| self.ps.try_push_str(&line.text))
                    {
                        return self.space_fail(error);
                    }
                    if let Some(budget) = &self.execution_budget
                        && let Err(error) = budget.consume_input(line.text.len())
                    {
                        return self.space_fail(error);
                    }
                    self.ps_had_newline = line.had_newline;
                    self.cur_file = line.file;
                    self.cursor += 1;
                    self.line_no += 1;
                    self.sub_made = false;
                }
                Kind::Quit(code) => {
                    self.exit_code = Some(*code);
                    self.flush_newline_on_quit = true;
                    return Cycle::Auto;
                }
                Kind::QuitSilent(code) => {
                    self.exit_code = Some(*code);
                    self.appends.clear();
                    return Cycle::Silent;
                }
                Kind::Append(text) => self.appends.push(Append::Text(text.clone())),
                Kind::Insert(text) => {
                    let text = format!("{text}\n");
                    self.out.raw(&text);
                }
                Kind::Change(text) => {
                    let ranged = cmd.addr.as_ref().is_some_and(|a| a.end.is_some());
                    if !ranged || cmd.negate || range_ended {
                        let text = format!("{text}\n");
                        self.out.raw(&text);
                    }
                    return Cycle::Silent;
                }
                Kind::HoldCopy => {
                    // No snapshot scratch: drop the old hold charge first,
                    // then copy straight from pattern space into the
                    // budgeted destination (disjoint fields, both charged).
                    // A dead budget fails closed here via space_fail.
                    self.hold = match BudgetedString::new(self.execution_budget.as_ref()) {
                        Ok(buf) => buf,
                        Err(error) => return self.space_fail(error),
                    };
                    if let Err(error) = self.hold.try_push_str(&self.ps) {
                        return self.space_fail(error);
                    }
                    self.hold_had_newline = self.ps_had_newline;
                }
                Kind::HoldAppend => {
                    if let Err(error) = self
                        .hold
                        .try_push(self.sep)
                        .and_then(|()| self.hold.try_push_str(&self.ps))
                    {
                        return self.space_fail(error);
                    }
                    self.hold_had_newline = self.ps_had_newline;
                }
                Kind::GetCopy => {
                    // No snapshot scratch: drop the old pattern charge first,
                    // then copy straight from hold space into the budgeted
                    // destination (disjoint fields, both charged).
                    self.ps = match BudgetedString::new(self.execution_budget.as_ref()) {
                        Ok(buf) => buf,
                        Err(error) => return self.space_fail(error),
                    };
                    if let Err(error) = self.ps.try_push_str(&self.hold) {
                        return self.space_fail(error);
                    }
                    self.ps_had_newline = self.hold_had_newline;
                }
                Kind::GetAppend => {
                    if let Err(error) = self
                        .ps
                        .try_push(self.sep)
                        .and_then(|()| self.ps.try_push_str(&self.hold))
                    {
                        return self.space_fail(error);
                    }
                    self.ps_had_newline = self.hold_had_newline;
                }
                Kind::Exchange => {
                    std::mem::swap(&mut self.ps, &mut self.hold);
                    std::mem::swap(&mut self.ps_had_newline, &mut self.hold_had_newline);
                }
                Kind::LineNumber => {
                    let text = format!("{}\n", self.line_no);
                    self.out.raw(&text);
                }
                Kind::List(width) => {
                    let width = width.unwrap_or(self.default_line_len);
                    let text = format!("{}\n", list_format(&self.ps, width));
                    self.out.raw(&text);
                }
                Kind::ReadFile(name) => self.appends.push(Append::File(name.clone())),
                Kind::ReadLine(name) => self.appends.push(Append::FileLine(name.clone())),
                Kind::WriteFile(name) => {
                    let nl = self.ps_had_newline;
                    let mut handles = WriteHandles {
                        sep: self.sep,
                        out: &mut self.out,
                        err: &mut self.stderr,
                        files: &mut self.write_files,
                        budget: self.execution_budget.as_ref(),
                    };
                    Self::write_out(&mut handles, name, &self.ps, nl);
                }
                Kind::WriteFirstLine(name) => match self.ps.find('\n') {
                    Some(idx) => {
                        let end = idx;
                        let mut handles = WriteHandles {
                            sep: self.sep,
                            out: &mut self.out,
                            err: &mut self.stderr,
                            files: &mut self.write_files,
                            budget: self.execution_budget.as_ref(),
                        };
                        Self::write_out(&mut handles, name, &self.ps[..end], true);
                    }
                    None => {
                        let nl = self.ps_had_newline;
                        let mut handles = WriteHandles {
                            sep: self.sep,
                            out: &mut self.out,
                            err: &mut self.stderr,
                            files: &mut self.write_files,
                            budget: self.execution_budget.as_ref(),
                        };
                        Self::write_out(&mut handles, name, &self.ps, nl);
                    }
                },
                Kind::Zap => match self.fresh_space() {
                    Ok(buf) => self.ps = buf,
                    Err(error) => return self.space_fail(error),
                },
                Kind::FileName => {
                    let name = self
                        .names
                        .get(self.cur_file)
                        .cloned()
                        .unwrap_or_else(|| "-".to_string());
                    let text = format!("{name}\n");
                    self.out.raw(&text);
                }
                // Handled above, before the address gate; listed for exhaustiveness.
                Kind::Block(_) | Kind::BlockEnd | Kind::Label(_) | Kind::Nop => {}
            }

            pc += 1;
        }

        Cycle::Auto
    }

    fn warn_loop_limit(&mut self) {
        if self.warned {
            return;
        }
        self.warned = true;
        self.stderr.push_str(&format!(
            "sed: warning: branch/label loop limit ({SED_MAX_CYCLE_STEPS}) reached on line {}; \
             output may be truncated\n",
            self.line_no
        ));
    }

    fn read_next(&mut self) -> bool {
        if self.cursor >= self.lines.len() {
            return false;
        }
        let line = &self.lines[self.cursor];
        // Admit the transient line clone as scratch BEFORE allocating it:
        // the clone lives until set_ps stores a fresh charged copy, so its
        // bytes stay charged the whole time. Admission itself fails closed.
        let _scratch = match &self.execution_budget {
            Some(budget) => match budget.lease_bytes(line.text.len()) {
                Ok(lease) => Some(lease),
                Err(error) => {
                    if self.out.error.is_none() {
                        self.out.error = Some(error);
                    }
                    self.warned = true;
                    return false;
                }
            },
            None => None,
        };
        let text = line.text.clone();
        let had_newline = line.had_newline;
        let file = line.file;
        if let Some(budget) = &self.execution_budget
            && let Err(error) = budget.consume_input(text.len())
        {
            if self.out.error.is_none() {
                self.out.error = Some(error);
            }
            self.warned = true;
            return false;
        }
        if let Err(error) = self.set_ps(&text) {
            if self.out.error.is_none() {
                self.out.error = Some(error);
            }
            self.warned = true;
            return false;
        }
        self.ps_had_newline = had_newline;
        self.cur_file = file;
        self.cursor += 1;
        self.line_no += 1;
        // Reading a line clears the `t`/`T` flag, as GNU's read_pattern_space does.
        self.sub_made = false;
        true
    }

    fn label_pc(&self, target: &str) -> usize {
        self.prog
            .cmds
            .iter()
            .position(|c| matches!(&c.kind, Kind::Label(name) if name == target))
            .unwrap_or(self.prog.cmds.len())
    }

    /// File/output routing without borrowing all of `self`: taking the
    /// touched pieces as disjoint parameters lets callers pass `&self.ps`
    /// slices straight in, with no clone-scratch transient. Behavior is
    /// identical to the former method form.
    fn write_out(h: &mut WriteHandles<'_>, name: &str, text: &str, had_newline: bool) {
        match name {
            "/dev/stdout" => h.out.line(text, had_newline),
            "/dev/stderr" => {
                h.err.push_str(text);
                if had_newline {
                    h.err.push('\n');
                }
            }
            _ => h
                .files
                .entry(name.to_string())
                .or_insert_with(|| Sink::with_sep(h.sep, h.budget))
                .line(text, had_newline),
        }
    }

    fn flush_appends(&mut self) {
        if self.appends.is_empty() {
            return;
        }
        for item in std::mem::take(&mut self.appends) {
            match item {
                Append::Text(text) => {
                    let text = format!("{text}\n");
                    self.out.raw(&text);
                }
                Append::File(name) => {
                    let Some(content) = self.read_files.get(&name) else {
                        continue;
                    };
                    if content.is_empty() {
                        continue;
                    }
                    match content.strip_suffix('\n') {
                        Some(body) => self.out.line(body, true),
                        None => self.out.line(content, false),
                    }
                }
                Append::FileLine(name) => {
                    let Some(content) = self.read_files.get(&name) else {
                        continue;
                    };
                    let offset = self.read_cursor.entry(name.clone()).or_insert(0);
                    if *offset >= content.len() {
                        continue;
                    }
                    let rest = &content[*offset..];
                    match rest.find('\n') {
                        Some(idx) => {
                            let line = rest[..idx].to_string();
                            *offset += idx + 1;
                            self.out.line(&line, true);
                        }
                        None => {
                            let line = rest.to_string();
                            *offset = content.len();
                            self.out.line(&line, false);
                        }
                    }
                }
            }
        }
    }

    // ----- addresses -------------------------------------------------------

    fn resolve(
        &mut self,
        re: &Option<std::sync::Arc<SedRegex>>,
    ) -> Option<std::sync::Arc<SedRegex>> {
        match re {
            Some(re) => {
                self.last_regex = Some(re.clone());
                Some(re.clone())
            }
            None => self.last_regex.clone(),
        }
    }

    fn is_last_line(&self) -> bool {
        self.cursor >= self.lines.len()
    }

    /// Returns `(selected, range_just_closed)`.
    fn address_matches(&mut self, pc: usize, addr: &Addr) -> (bool, bool) {
        let Some(end) = &addr.end else {
            return (self.start_matches(&addr.start), false);
        };

        let state = self.range[pc].clone();
        if state.active {
            // A numeric end both closes the range and *excludes* the line once
            // the stream is past it: `n`/`N`/`b` can carry the stream beyond
            // the end line without the range ever being evaluated there.
            let (closed, selected) = match end {
                EndAddr::Line(n) => (self.line_no >= *n, self.line_no <= *n),
                EndAddr::Plus(_) | EndAddr::Multiple(_) => match state.end_line {
                    Some(e) => (self.line_no >= e, self.line_no <= e),
                    None => (false, true),
                },
                EndAddr::Last => (self.is_last_line(), true),
                EndAddr::Regex(re) => {
                    let re = self.resolve(re);
                    (re.is_some_and(|re| re.is_match(&self.ps)), true)
                }
            };
            if closed {
                // A relative end (`+N`, `~N`) is recomputed every time the
                // range opens, so skipping past it (via `n`, `N`, `D` or a
                // branch) re-arms the start address; an absolute `,N` end is
                // spent for good once the stream is past it.
                let relative = matches!(end, EndAddr::Plus(_) | EndAddr::Multiple(_));
                self.range[pc] = RangeState {
                    start_spent: state.start_spent && (selected || !relative),
                    ..RangeState::default()
                };
            }
            if selected || !matches!(end, EndAddr::Plus(_) | EndAddr::Multiple(_)) {
                return (selected, selected && closed);
            }
            // Fall through and retest the start address on this line.
        }

        let state = self.range[pc].clone();
        let one_shot = matches!(addr.start, StartAddr::Line(_) | StartAddr::Zero);
        let starts = match &addr.start {
            // A numeric start opens on the first line at or past it, once.
            StartAddr::Line(n) => !state.start_spent && self.line_no >= *n,
            StartAddr::Zero => !state.start_spent,
            other => self.start_matches(other),
        };
        if !starts {
            return (false, false);
        }
        if one_shot {
            self.range[pc].start_spent = true;
        }

        let end_line = match end {
            EndAddr::Line(n) => Some(*n),
            EndAddr::Plus(n) => Some(self.line_no + n),
            EndAddr::Multiple(n) => Some(if *n <= 1 {
                self.line_no
            } else {
                (self.line_no / n + 1) * n
            }),
            EndAddr::Last | EndAddr::Regex(_) => None,
        };

        // A numeric end at or before the start line makes this a one-line range.
        if let Some(e) = end_line
            && e <= self.line_no
        {
            return (true, true);
        }
        if matches!(end, EndAddr::Last) && self.is_last_line() {
            return (true, true);
        }
        // Running off the end of input does NOT close a range: GNU only fires
        // `c` when the end address actually matched, so `sed '1,5c\Z'` over two
        // lines prints nothing.
        // `0,/re/` is the one form whose end regex is tested on the start line.
        if let EndAddr::Regex(re) = end
            && matches!(addr.start, StartAddr::Zero)
        {
            let re = self.resolve(re);
            if re.is_some_and(|re| re.is_match(&self.ps)) {
                return (true, true);
            }
        }

        self.range[pc] = RangeState {
            active: true,
            end_line,
            start_spent: one_shot,
        };
        (true, false)
    }

    fn start_matches(&mut self, start: &StartAddr) -> bool {
        match start {
            StartAddr::Line(n) => self.line_no == *n,
            // `0` is only legal as the start of `0,/re/`, which
            // `address_matches` handles before reaching here; the parser
            // rejects a bare `0` address.
            StartAddr::Zero => false,
            StartAddr::Last => self.is_last_line(),
            StartAddr::Step(first, step) => {
                if *step == 0 {
                    self.line_no == *first
                } else if *first == 0 {
                    self.line_no.is_multiple_of(*step)
                } else {
                    self.line_no >= *first && (self.line_no - *first).is_multiple_of(*step)
                }
            }
            StartAddr::Regex(re) => {
                let re = self.resolve(re);
                re.is_some_and(|re| re.is_match(&self.ps))
            }
        }
    }

    // ----- substitution ----------------------------------------------------

    fn substitute(&mut self, subst: &Subst) -> Result<(), LimitExceeded> {
        let Some(re) = self.resolve(&subst.re) else {
            return Ok(());
        };
        let spans = re.matches(&self.ps);
        if spans.len() < subst.occurrence {
            return Ok(());
        }

        // Keep the old owner charged through the last use: move pattern
        // space aside (its lease stays live) and build the replacement into
        // a fresh budgeted buffer, then swap. Peak is ~2x pattern, all
        // charged; a dead budget fails here before growth.
        let old = self.take_ps()?;
        let mut out = BudgetedString::new(self.execution_budget.as_ref())?;
        let mut cursor = 0usize;
        let mut replaced = false;

        for (index, span) in spans.iter().enumerate() {
            let nth = index + 1;
            if nth < subst.occurrence || (nth > subst.occurrence && !subst.global) {
                continue;
            }
            out.try_push_str(&old[cursor..span.start])?;
            expand(&subst.parts, &old, span, &mut out)?;
            cursor = span.end;
            replaced = true;
        }

        if !replaced {
            // No match at this occurrence: restore the charged original
            // instead of dropping it (an empty pattern here would silently
            // wipe pattern space for every non-matching `s///`).
            self.ps = old;
            return Ok(());
        }
        out.try_push_str(&old[cursor..])?;
        self.ps = out;
        self.sub_made = true;

        if subst.print {
            self.out.line(&self.ps, self.ps_had_newline);
        }
        if let Some(name) = &subst.wfile {
            let nl = self.ps_had_newline;
            let mut handles = WriteHandles {
                sep: self.sep,
                out: &mut self.out,
                err: &mut self.stderr,
                files: &mut self.write_files,
                budget: self.execution_budget.as_ref(),
            };
            Self::write_out(&mut handles, name, &self.ps, nl);
        }
        Ok(())
    }
}

#[derive(Default)]
struct CaseState {
    span: Option<bool>,
    one: Option<bool>,
}

fn push_cased(
    out: &mut BudgetedString,
    text: &str,
    state: &mut CaseState,
) -> Result<(), LimitExceeded> {
    for ch in text.chars() {
        if let Some(upper) = state.one.take() {
            let mapped: Vec<char> = if upper {
                ch.to_uppercase().collect()
            } else {
                ch.to_lowercase().collect()
            };
            for c in mapped {
                out.try_push(c)?;
            }
            continue;
        }
        match state.span {
            Some(true) => {
                for c in ch.to_uppercase() {
                    out.try_push(c)?;
                }
            }
            Some(false) => {
                for c in ch.to_lowercase() {
                    out.try_push(c)?;
                }
            }
            None => out.try_push(ch)?,
        }
    }
    Ok(())
}

fn expand(
    parts: &[RepPart],
    subject: &str,
    span: &super::pattern::MatchSpan,
    out: &mut BudgetedString,
) -> Result<(), LimitExceeded> {
    let mut state = CaseState::default();
    for part in parts {
        match part {
            RepPart::Lit(text) => push_cased(out, text, &mut state)?,
            RepPart::Group(0) => push_cased(out, &subject[span.start..span.end], &mut state)?,
            RepPart::Group(n) => {
                if let Some(Some((s, e))) = span.groups.get(n - 1) {
                    // Borrow the match slice directly: no transient String.
                    push_cased(out, &subject[*s..*e], &mut state)?;
                }
            }
            RepPart::Case(op) => match op {
                CaseOp::Upper => {
                    state.span = Some(true);
                    state.one = None;
                }
                CaseOp::Lower => {
                    state.span = Some(false);
                    state.one = None;
                }
                CaseOp::UpperOne => state.one = Some(true),
                CaseOp::LowerOne => state.one = Some(false),
                CaseOp::End => {
                    state.span = None;
                    state.one = None;
                }
            },
        }
    }
    Ok(())
}

/// Render the pattern space the way `l` does: unambiguous escapes, `$`
/// terminator, and wrapping with a trailing backslash at `width` columns.
pub(super) fn list_format(text: &str, width: usize) -> String {
    let mut out = String::new();
    let mut column = 0usize;

    for ch in text.chars() {
        let token = match ch {
            '\\' => "\\\\".to_string(),
            '\x07' => "\\a".to_string(),
            '\x08' => "\\b".to_string(),
            '\x0c' => "\\f".to_string(),
            '\n' => "\\n".to_string(),
            '\r' => "\\r".to_string(),
            '\t' => "\\t".to_string(),
            '\x0b' => "\\v".to_string(),
            c if c.is_ascii_graphic() || c == ' ' => c.to_string(),
            c => {
                let mut buf = [0u8; 4];
                c.encode_utf8(&mut buf)
                    .as_bytes()
                    .iter()
                    .map(|b| format!("\\{b:03o}"))
                    .collect()
            }
        };
        if width > 1 && column + token.len() > width - 1 {
            out.push_str("\\\n");
            column = 0;
        }
        column += token.len();
        out.push_str(&token);
    }

    out.push('$');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::limits::ExecutionLimits;
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;

    fn parse_script(script: &str) -> Program {
        match super::super::script::parse_with(script, false, false) {
            Ok(prog) => prog,
            Err(_) => panic!("test script parses"),
        }
    }

    fn test_program() -> Program {
        parse_script("p")
    }

    fn test_empty_program() -> Program {
        parse_script("")
    }

    fn test_lines() -> Vec<InputLine> {
        vec![InputLine {
            text: "x".to_string(),
            had_newline: true,
            file: 0,
        }]
    }

    fn test_machine(
        prog: &Program,
        budget: Option<ExecutionBudget>,
    ) -> Result<Machine<'_>, LimitExceeded> {
        // The empty file map must outlive the machine; a `&'static` empty
        // map is the simplest owner. Lazily leaked once per test process.
        use std::sync::OnceLock;
        static EMPTY: OnceLock<HashMap<String, String>> = OnceLock::new();
        let files = EMPTY.get_or_init(HashMap::new);
        Machine::new(prog, &[], files, true, 0, '\n', budget)
    }

    /// TM-DOS-018 follow-up: the synchronous segment loop cannot be
    /// interrupted by the async timeout on its own, so the per-command
    /// budget checkpoints must refuse a cancelled/expired budget before
    /// running anything. Pre-cancelled here for determinism (no timing).
    #[test]
    fn cancelled_budget_refuses_segment() {
        let prog = test_program();
        let cancelled = Arc::new(AtomicBool::new(true));
        let budget = ExecutionBudget::new(&ExecutionLimits::new(), cancelled);
        // A dead budget fails the machine itself (fail closed before any
        // growth); assert on construction directly.
        assert!(
            test_machine(&prog, Some(budget)).is_err(),
            "dead budget must refuse construction"
        );
    }

    /// The loop-top checkpoint is the only one that can fire when nothing
    /// allocates and no command ever runs (empty program, empty input), so
    /// a cancelled budget must refuse even then.
    #[test]
    fn cancelled_budget_refuses_empty_program() {
        let prog = test_empty_program();
        let cancelled = Arc::new(AtomicBool::new(true));
        let budget = ExecutionBudget::new(&ExecutionLimits::new(), cancelled);
        assert!(
            test_machine(&prog, Some(budget)).is_err(),
            "dead budget must refuse construction"
        );
    }

    /// Same-shape control: an uncancelled budget runs the script fine, so the
    /// refusals above come from the checkpoints, not the harness.
    #[test]
    fn live_budget_runs_segment() {
        let prog = test_program();
        let budget =
            ExecutionBudget::new(&ExecutionLimits::new(), Arc::new(AtomicBool::new(false)));
        let mut machine = match test_machine(&prog, Some(budget)) {
            Ok(machine) => machine,
            Err(error) => panic!("live budget must construct: {error:?}"), // debug-ok: test assertion
        };
        let result = machine.run_segment(&test_lines());
        assert!(result.is_ok(), "live budget must run, got {result:?}"); // debug-ok: test assertion
    }

    fn many_x_lines(count: usize) -> Vec<InputLine> {
        (0..count)
            .map(|_| InputLine {
                text: "x".to_string(),
                had_newline: true,
                file: 0,
            })
            .collect()
    }

    /// TM-DOS-018 follow-up, unit level: the `N`-preserves-pattern / `h;G`
    /// doubling loop cannot outgrow a small live budget. Growth model:
    /// 1500 x-lines (~3 KB in) need ~13 MB of pattern against 256 KB cap.
    #[test]
    fn doubling_loop_refuses_live_budget() {
        let prog = parse_script(":a;N;100~100{h;G};ba");
        let limits = ExecutionLimits::new().max_live_intermediate_bytes(1 << 18);
        let budget = ExecutionBudget::new(&limits, Arc::new(AtomicBool::new(false)));
        let mut machine = match test_machine(&prog, Some(budget)) {
            Ok(machine) => machine,
            Err(error) => panic!("live budget must construct: {error:?}"), // debug-ok: test assertion
        };
        let result = machine.run_segment(&many_x_lines(1500));
        assert!(result.is_err(), "doubling loop must refuse, got {result:?}"); // debug-ok: test assertion
    }

    /// TM-DOS-018 follow-up, unit level: per-command aggregate-work charges
    /// refuse a long command list under a tiny work budget.
    #[test]
    fn commands_refuse_work_budget() {
        let prog = parse_script(&"p;".repeat(20));
        let limits = ExecutionLimits::new().max_work_units(10);
        let budget = ExecutionBudget::new(&limits, Arc::new(AtomicBool::new(false)));
        let mut machine = match test_machine(&prog, Some(budget)) {
            Ok(machine) => machine,
            Err(error) => panic!("live budget must construct: {error:?}"), // debug-ok: test assertion
        };
        let result = machine.run_segment(&test_lines());
        assert!(result.is_err(), "work budget must refuse, got {result:?}"); // debug-ok: test assertion
    }

    /// TM-DOS-018 follow-up, unit level: swap moves ownership without
    /// growth, so `x` fits even when pattern+hold nearly fill a tiny cap.
    /// Same-shape control proving the cap, not the harness, refuses the
    /// growth tests above.
    #[test]
    fn swap_fits_under_cap() {
        let prog = parse_script("h;x");
        let limits = ExecutionLimits::new().max_live_intermediate_bytes(64);
        let budget = ExecutionBudget::new(&limits, Arc::new(AtomicBool::new(false)));
        let mut machine = match test_machine(&prog, Some(budget)) {
            Ok(machine) => machine,
            Err(error) => panic!("live budget must construct: {error:?}"), // debug-ok: test assertion
        };
        let result = machine.run_segment(&many_x_lines(2));
        assert!(result.is_ok(), "swap must fit, got {result:?}"); // debug-ok: test assertion
    }

    /// TM-DOS-018 follow-up, unit level: `D` only shrinks pattern space, so
    /// it fits under a cap the doubling loop trips.
    #[test]
    fn delete_first_line_fits_under_cap() {
        let prog = parse_script("D");
        let limits = ExecutionLimits::new().max_live_intermediate_bytes(64);
        let budget = ExecutionBudget::new(&limits, Arc::new(AtomicBool::new(false)));
        let mut machine = match test_machine(&prog, Some(budget)) {
            Ok(machine) => machine,
            Err(error) => panic!("live budget must construct: {error:?}"), // debug-ok: test assertion
        };
        let result = machine.run_segment(&many_x_lines(2));
        assert!(result.is_ok(), "drain must fit, got {result:?}"); // debug-ok: test assertion
    }

    /// TM-DOS-018 follow-up, unit level: same-size transliteration fits when
    /// charged twice over (old + new) under the cap.
    #[test]
    fn transliterate_same_size_fits() {
        let prog = parse_script("y/abc/xyz/");
        let limits = ExecutionLimits::new().max_live_intermediate_bytes(64);
        let budget = ExecutionBudget::new(&limits, Arc::new(AtomicBool::new(false)));
        let mut machine = match test_machine(&prog, Some(budget)) {
            Ok(machine) => machine,
            Err(error) => panic!("live budget must construct: {error:?}"), // debug-ok: test assertion
        };
        let result = machine.run_segment(&many_x_lines(2));
        assert!(result.is_ok(), "same-size rewrite must fit, got {result:?}"); // debug-ok: test assertion
    }

    /// TM-DOS-018 follow-up, unit level: widening substitution charges the
    /// replacement incrementally and refuses past the cap.
    #[test]
    fn widening_substitution_refuses() {
        let prog = parse_script("s/x/xxxxxxxxxx/g");
        let limits = ExecutionLimits::new().max_live_intermediate_bytes(64);
        let budget = ExecutionBudget::new(&limits, Arc::new(AtomicBool::new(false)));
        let mut machine = match test_machine(&prog, Some(budget)) {
            Ok(machine) => machine,
            Err(error) => panic!("live budget must construct: {error:?}"), // debug-ok: test assertion
        };
        let lines = vec![InputLine {
            text: "xxxxxxxxxxxxxxxx".to_string(),
            had_newline: true,
            file: 0,
        }];
        let result = machine.run_segment(&lines);
        assert!(
            result.is_err(),
            "widening rewrite must refuse, got {result:?}" // debug-ok: test assertion
        );
    }

    /// TM-DOS-018 follow-up, unit level: repeated hold-appends charge every
    /// copy and refuse past the cap.
    #[test]
    fn hold_append_refuses() {
        let prog = parse_script("h;H;H;H;H;H;H;H;H;H;H");
        let limits = ExecutionLimits::new().max_live_intermediate_bytes(64);
        let budget = ExecutionBudget::new(&limits, Arc::new(AtomicBool::new(false)));
        let mut machine = match test_machine(&prog, Some(budget)) {
            Ok(machine) => machine,
            Err(error) => panic!("live budget must construct: {error:?}"), // debug-ok: test assertion
        };
        let lines = vec![InputLine {
            text: "xxxxxxxxxxxxxxxx".to_string(),
            had_newline: true,
            file: 0,
        }];
        let result = machine.run_segment(&lines);
        assert!(result.is_err(), "hold appends must refuse, got {result:?}"); // debug-ok: test assertion
    }

    /// TM-DOS-018 follow-up, unit level: per-line aggregate-input charges
    /// refuse reading past a tiny input budget.
    #[test]
    fn reads_refuse_input_budget() {
        let prog = parse_script("p");
        let limits = ExecutionLimits::new().max_aggregate_input_bytes(5);
        let budget = ExecutionBudget::new(&limits, Arc::new(AtomicBool::new(false)));
        let mut machine = match test_machine(&prog, Some(budget)) {
            Ok(machine) => machine,
            Err(error) => panic!("live budget must construct: {error:?}"), // debug-ok: test assertion
        };
        let result = machine.run_segment(&many_x_lines(10));
        assert!(result.is_err(), "input budget must refuse, got {result:?}"); // debug-ok: test assertion
    }
}
