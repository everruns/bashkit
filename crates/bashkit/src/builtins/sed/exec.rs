//! sed execution engine.
//!
//! Important decisions (issue #2427):
//!
//! - Output goes through [`Sink`], which mirrors GNU's "missing newline"
//!   bookkeeping: a line that arrived without a trailing newline is written
//!   without one, and the newline is only emitted if more output follows. That
//!   is what stops `printf 'a' | sed 's/a/b/'` from inventing a terminator and,
//!   with `-i`, writing it back to the file (finding E).
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
    pub(super) buf: String,
    missing_newline: bool,
    /// Line terminator: `\n`, or NUL under `-z`.
    sep: char,
}

impl Sink {
    fn with_sep(sep: char) -> Self {
        Sink {
            buf: String::new(),
            missing_newline: false,
            sep,
        }
    }
}

impl Sink {
    /// Write one line, adding a terminator only when the source had one.
    fn line(&mut self, text: &str, had_newline: bool) {
        self.flush_pending();
        self.buf.push_str(text);
        if had_newline {
            self.buf.push(self.sep);
        } else {
            self.missing_newline = true;
        }
    }

    /// Write text that already carries its own terminator.
    fn raw(&mut self, text: &str) {
        self.flush_pending();
        self.buf.push_str(text);
    }

    /// Emit a newline that was withheld because the source line had none.
    fn flush_pending(&mut self) {
        if self.missing_newline {
            self.buf.push(self.sep);
            self.missing_newline = false;
        }
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
}

pub(super) struct Machine<'a> {
    prog: &'a Program,
    names: &'a [String],
    read_files: &'a HashMap<String, String>,
    quiet: bool,
    default_line_len: usize,
    sep: char,

    out: Sink,
    pub(super) stderr: String,
    pub(super) write_files: HashMap<String, Sink>,

    range: Vec<RangeState>,
    hold: String,
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
    ps: String,
    ps_had_newline: bool,
    cur_file: usize,
    sub_made: bool,
}

impl<'a> Machine<'a> {
    pub(super) fn new(
        prog: &'a Program,
        names: &'a [String],
        read_files: &'a HashMap<String, String>,
        quiet: bool,
        line_len: usize,
        sep: char,
    ) -> Self {
        Machine {
            prog,
            names,
            read_files,
            quiet: quiet || prog.quiet,
            default_line_len: line_len,
            sep,
            out: Sink::with_sep(sep),
            stderr: String::new(),
            write_files: HashMap::new(),
            range: Vec::new(),
            hold: String::new(),
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
            ps: String::new(),
            ps_had_newline: true,
            cur_file: 0,
            sub_made: false,
        }
    }

    pub(super) fn finished(&self) -> bool {
        self.exit_code.is_some()
    }

    /// Run one input stream and return everything it wrote to stdout.
    pub(super) fn run_segment(&mut self, lines: &'a [InputLine]) -> String {
        self.lines = lines;
        self.cursor = 0;
        self.line_no = 0;
        self.range = vec![RangeState::default(); self.prog.cmds.len()];
        self.out = Sink::with_sep(self.sep);

        let mut restart = false;
        // THREAT[TM-DOS]: `D` restarts the script without reading input, so a
        // script like `G;D` never terminates and grows the pattern space
        // without bound. The per-cycle step budget cannot see across restarts,
        // so they get their own budget, reset whenever a line is read.
        let mut restarts = 0usize;
        loop {
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
            } else {
                if !self.read_next() {
                    break;
                }
                restarts = 0;
            }
            restart = false;

            match self.cycle() {
                Cycle::Auto => {
                    if !self.quiet {
                        let (ps, nl) = (std::mem::take(&mut self.ps), self.ps_had_newline);
                        self.out.line(&ps, nl);
                        self.ps = ps;
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

        std::mem::replace(&mut self.out, Sink::with_sep(self.sep)).buf
    }

    fn cycle(&mut self) -> Cycle {
        let cmds = &self.prog.cmds;
        let mut pc = 0usize;
        let mut steps = 0usize;

        while pc < cmds.len() {
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
                Kind::Substitute(subst) => self.substitute(subst),
                Kind::Transliterate(map) => {
                    self.ps = self
                        .ps
                        .chars()
                        .map(|c| {
                            map.iter()
                                .find(|(from, _)| *from == c)
                                .map_or(c, |(_, to)| *to)
                        })
                        .collect();
                }
                Kind::Delete => return Cycle::Silent,
                Kind::DeleteFirstLine => {
                    return match self.ps.find('\n') {
                        Some(idx) => {
                            self.ps.drain(..=idx);
                            Cycle::Restart
                        }
                        None => Cycle::Silent,
                    };
                }
                Kind::Print => {
                    let (ps, nl) = (std::mem::take(&mut self.ps), self.ps_had_newline);
                    self.out.line(&ps, nl);
                    self.ps = ps;
                }
                Kind::PrintFirstLine => {
                    let ps = std::mem::take(&mut self.ps);
                    match ps.find('\n') {
                        Some(idx) => self.out.line(&ps[..idx], true),
                        None => self.out.line(&ps, self.ps_had_newline),
                    }
                    self.ps = ps;
                }
                Kind::Next => {
                    if !self.quiet {
                        let (ps, nl) = (std::mem::take(&mut self.ps), self.ps_had_newline);
                        self.out.line(&ps, nl);
                        self.ps = ps;
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
                    self.ps.push('\n');
                    self.ps.push_str(&line.text);
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
                    self.hold.clone_from(&self.ps);
                    self.hold_had_newline = self.ps_had_newline;
                }
                Kind::HoldAppend => {
                    self.hold.push('\n');
                    self.hold.push_str(&self.ps);
                    self.hold_had_newline = self.ps_had_newline;
                }
                Kind::GetCopy => {
                    self.ps.clone_from(&self.hold);
                    self.ps_had_newline = self.hold_had_newline;
                }
                Kind::GetAppend => {
                    self.ps.push('\n');
                    self.ps.push_str(&self.hold);
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
                    let (ps, nl) = (std::mem::take(&mut self.ps), self.ps_had_newline);
                    self.write_out(name, &ps, nl);
                    self.ps = ps;
                }
                Kind::WriteFirstLine(name) => {
                    let ps = std::mem::take(&mut self.ps);
                    match ps.find('\n') {
                        Some(idx) => {
                            let head = ps[..idx].to_string();
                            self.write_out(name, &head, true);
                        }
                        None => self.write_out(name, &ps, self.ps_had_newline),
                    }
                    self.ps = ps;
                }
                Kind::Zap => self.ps.clear(),
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
        self.ps.clear();
        self.ps.push_str(&line.text);
        self.ps_had_newline = line.had_newline;
        self.cur_file = line.file;
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

    fn write_out(&mut self, name: &str, text: &str, had_newline: bool) {
        match name {
            "/dev/stdout" => self.out.line(text, had_newline),
            "/dev/stderr" => {
                self.stderr.push_str(text);
                if had_newline {
                    self.stderr.push('\n');
                }
            }
            _ => {
                let sep = self.sep;
                self.write_files
                    .entry(name.to_string())
                    .or_insert_with(|| Sink::with_sep(sep))
                    .line(text, had_newline)
            }
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

    fn substitute(&mut self, subst: &Subst) {
        let Some(re) = self.resolve(&subst.re) else {
            return;
        };
        let spans = re.matches(&self.ps);
        if spans.len() < subst.occurrence {
            return;
        }

        let mut out = String::with_capacity(self.ps.len());
        let mut cursor = 0usize;
        let mut replaced = false;

        for (index, span) in spans.iter().enumerate() {
            let nth = index + 1;
            if nth < subst.occurrence || (nth > subst.occurrence && !subst.global) {
                continue;
            }
            out.push_str(&self.ps[cursor..span.start]);
            expand(&subst.parts, &self.ps, span, &mut out);
            cursor = span.end;
            replaced = true;
        }

        if !replaced {
            return;
        }
        out.push_str(&self.ps[cursor..]);
        self.ps = out;
        self.sub_made = true;

        if subst.print {
            let (ps, nl) = (std::mem::take(&mut self.ps), self.ps_had_newline);
            self.out.line(&ps, nl);
            self.ps = ps;
        }
        if let Some(name) = &subst.wfile {
            let name = name.clone();
            let (ps, nl) = (std::mem::take(&mut self.ps), self.ps_had_newline);
            self.write_out(&name, &ps, nl);
            self.ps = ps;
        }
    }
}

#[derive(Default)]
struct CaseState {
    span: Option<bool>,
    one: Option<bool>,
}

fn push_cased(out: &mut String, text: &str, state: &mut CaseState) {
    for ch in text.chars() {
        if let Some(upper) = state.one.take() {
            if upper {
                out.extend(ch.to_uppercase());
            } else {
                out.extend(ch.to_lowercase());
            }
            continue;
        }
        match state.span {
            Some(true) => out.extend(ch.to_uppercase()),
            Some(false) => out.extend(ch.to_lowercase()),
            None => out.push(ch),
        }
    }
}

fn expand(parts: &[RepPart], subject: &str, span: &super::pattern::MatchSpan, out: &mut String) {
    let mut state = CaseState::default();
    for part in parts {
        match part {
            RepPart::Lit(text) => push_cased(out, text, &mut state),
            RepPart::Group(0) => push_cased(out, &subject[span.start..span.end], &mut state),
            RepPart::Group(n) => {
                if let Some(Some((s, e))) = span.groups.get(n - 1) {
                    let text = subject[*s..*e].to_string();
                    push_cased(out, &text, &mut state);
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
