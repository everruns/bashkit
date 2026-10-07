//! Makefile parser: logical lines, conditionals, `define`, `include`,
//! assignments and rules. Targets and prerequisites are expanded while
//! parsing (like GNU make); recipes are kept raw and expanded when run.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use super::expand::{
    Expander, Message, Need, Oracle, Origin, R, Stop, Var, Vars, fatal, top_level_find, words,
};

/// `include` nesting cap (an include cycle would otherwise never end).
const MAX_INCLUDE_DEPTH: usize = 16;

/// Suffixes GNU make knows for old-style suffix rules (`.c.o:`).
const SUFFIXES: &[&str] = &[
    ".out", ".a", ".ln", ".o", ".c", ".cc", ".C", ".cpp", ".p", ".f", ".F", ".m", ".r", ".y", ".l",
    ".ym", ".lm", ".s", ".S", ".mod", ".sym", ".def", ".h", ".info", ".dvi", ".tex", ".texinfo",
    ".texi", ".txinfo", ".w", ".ch", ".web", ".sh", ".elc", ".el",
];

#[derive(Debug, Clone)]
pub(super) struct RecipeLine {
    pub text: String,
    pub file: Arc<str>,
    pub line: usize,
}

#[derive(Debug, Clone, Default)]
pub(super) struct Rule {
    pub prereqs: Vec<String>,
    pub order_only: Vec<String>,
    pub recipe: Option<Vec<RecipeLine>>,
    /// Stem for static pattern rules.
    pub stem: Option<String>,
}

#[derive(Debug, Clone)]
pub(super) struct PatternRule {
    pub targets: Vec<String>,
    pub prereqs: Vec<String>,
    pub order_only: Vec<String>,
    pub recipe: Vec<RecipeLine>,
}

#[derive(Default)]
pub(super) struct Makefile {
    pub vars: Vars,
    pub exported: HashSet<String>,
    pub unexported: HashSet<String>,
    pub export_all: bool,
    pub rules: HashMap<String, Rule>,
    pub patterns: Vec<PatternRule>,
    pub target_vars: HashMap<String, Vars>,
    pub default_goal: Option<String>,
    pub phony: HashSet<String>,
    pub silent_all: bool,
    pub silent: HashSet<String>,
    pub ignore_all: bool,
    pub oneshell: bool,
    pub makefile_list: Vec<String>,
    pub messages: Vec<Message>,
}

/// Where recipe lines currently go.
enum Current {
    None,
    Explicit(Vec<String>),
    Pattern(usize),
}

struct Cond {
    /// This branch is being read.
    active: bool,
    /// Some branch of this conditional was taken already.
    taken: bool,
    /// The enclosing region is active.
    parent: bool,
}

struct Parser<'a> {
    mk: Makefile,
    oracle: &'a Oracle,
    curdir: &'a str,
    env_override: bool,
    current: Current,
    conds: Vec<Cond>,
    depth: usize,
    /// Targets whose next recipe line replaces (not extends) their recipe.
    fresh: HashSet<String>,
}

pub(super) struct ParseInput<'a> {
    pub name: &'a str,
    pub text: &'a str,
    pub vars: Vars,
    pub curdir: &'a str,
    pub env_override: bool,
}

/// `partial` receives `$(info)`/`$(warning)` output printed before a fatal
/// error.
pub(super) fn parse(
    input: ParseInput<'_>,
    oracle: &Oracle,
    partial: &mut Vec<Message>,
) -> R<Makefile> {
    let mut p = Parser {
        mk: Makefile {
            vars: input.vars,
            ..Default::default()
        },
        oracle,
        curdir: input.curdir,
        env_override: input.env_override,
        current: Current::None,
        conds: Vec::new(),
        depth: 0,
        fresh: HashSet::new(),
    };
    if let Err(e) = p.file(input.name, input.text) {
        // Output printed before a fatal error still shows.
        if matches!(e, Stop::Fatal { .. }) {
            *partial = std::mem::take(&mut p.mk.messages);
        }
        return Err(e);
    }
    if let Some(v) = p.mk.vars.get(".DEFAULT_GOAL") {
        let goal = v.value.trim().to_string();
        if !goal.is_empty() {
            p.mk.default_goal = Some(goal);
        }
    }
    Ok(p.mk)
}

/// Strip a `#` comment (not `\#`) and unescape `\#`.
fn strip_comment(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' if chars.peek() == Some(&'#') => {
                out.push('#');
                chars.next();
            }
            '#' => break,
            c => out.push(c),
        }
    }
    out
}

fn first_word(s: &str) -> (&str, &str) {
    let s = s.trim_start();
    match s.find([' ', '\t']) {
        Some(i) => (&s[..i], s[i..].trim_start()),
        None => (s, ""),
    }
}

impl Parser<'_> {
    fn active(&self) -> bool {
        self.conds.last().is_none_or(|c| c.active)
    }

    fn expander(&self, file: &str, line: usize) -> Expander<'_> {
        Expander::new(
            &self.mk.vars,
            self.oracle,
            self.curdir,
            Some((file.to_string(), line)),
        )
    }

    fn expand(&mut self, s: &str, file: &str, line: usize) -> R<String> {
        let mut e = self.expander(file, line);
        let r = e.expand(s);
        let msgs = std::mem::take(&mut e.messages);
        self.mk.messages.extend(msgs);
        r
    }

    fn file(&mut self, name: &str, text: &str) -> R<()> {
        self.depth += 1;
        if self.depth > MAX_INCLUDE_DEPTH {
            return fatal(None, format!("{name}: include nesting too deep"));
        }
        self.mk.makefile_list.push(name.to_string());
        self.set_simple(
            "MAKEFILE_LIST",
            self.mk.makefile_list.join(" "),
            Origin::File,
        );
        let file: Arc<str> = Arc::from(name);
        let lines: Vec<&str> = text.split('\n').collect();
        let conds_at_start = self.conds.len();
        let mut i = 0;
        while i < lines.len() {
            let lineno = i + 1;
            let raw = lines[i].strip_suffix('\r').unwrap_or(lines[i]);
            i += 1;

            // Recipe line: tab-led inside a rule.
            if raw.starts_with('\t') && !matches!(self.current, Current::None) {
                let mut text = raw[1..].to_string();
                while text.ends_with('\\') && i < lines.len() {
                    let next = lines[i].strip_suffix('\r').unwrap_or(lines[i]);
                    i += 1;
                    text.push('\n');
                    text.push_str(next.strip_prefix('\t').unwrap_or(next));
                }
                if self.active() {
                    self.add_recipe(RecipeLine {
                        text,
                        file: file.clone(),
                        line: lineno,
                    });
                }
                continue;
            }

            // Logical line with continuations joined by one space.
            let mut logical = raw.to_string();
            while logical.ends_with('\\') && !logical.ends_with("\\\\") && i < lines.len() {
                logical.pop();
                let next = lines[i].strip_suffix('\r').unwrap_or(lines[i]);
                i += 1;
                let trimmed = logical.trim_end().len();
                logical.truncate(trimmed);
                logical.push(' ');
                logical.push_str(next.trim_start());
            }

            let (word, rest) = first_word(&logical);
            // `define` keeps its body raw (no comment stripping).
            if word == "define"
                || ((word == "override" || word == "export") && first_word(rest).0 == "define")
            {
                let (is_override, is_export, spec) = match word {
                    "override" => (true, false, first_word(rest).1),
                    "export" => (false, true, first_word(rest).1),
                    _ => (false, false, rest),
                };
                let mut body = Vec::new();
                let mut nest = 0usize;
                let mut closed = false;
                while i < lines.len() {
                    let l = lines[i].strip_suffix('\r').unwrap_or(lines[i]);
                    i += 1;
                    let w = first_word(l).0;
                    if w == "define" {
                        nest += 1;
                    } else if w == "endef" {
                        if nest == 0 {
                            closed = true;
                            break;
                        }
                        nest -= 1;
                    }
                    body.push(l);
                }
                if !closed {
                    return fatal(
                        Some((name.to_string(), lineno)),
                        "missing 'endef', unterminated 'define'",
                    );
                }
                if self.active() {
                    let spec = strip_comment(spec);
                    let spec = spec.trim();
                    let (vname, op) = match spec.rsplit_once(char::is_whitespace) {
                        Some((n, op)) if ["=", ":=", "::=", "?=", "+="].contains(&op) => {
                            (n.trim(), op)
                        }
                        _ => match spec.strip_suffix('=') {
                            Some(n) => (n.trim_end_matches([':', '?', '+']).trim(), "="),
                            None => (spec, "="),
                        },
                    };
                    let vname = self.expand(vname, name, lineno)?;
                    self.assign(
                        vname.trim(),
                        op,
                        &body.join("\n"),
                        is_override,
                        name,
                        lineno,
                    )?;
                    if is_export {
                        self.mk.exported.insert(vname.trim().to_string());
                    }
                    self.current = Current::None;
                }
                continue;
            }

            let line = strip_comment(&logical);
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let (word, rest) = first_word(trimmed);

            // Conditionals are processed even in inactive regions.
            match word {
                "ifeq" | "ifneq" | "ifdef" | "ifndef" => {
                    let parent = self.active();
                    let value = parent && self.condition(word, rest, name, lineno)?;
                    self.conds.push(Cond {
                        active: parent && value,
                        taken: value,
                        parent,
                    });
                    continue;
                }
                "else" => {
                    let Some(c) = self.conds.last() else {
                        return fatal(Some((name.to_string(), lineno)), "extraneous 'else'");
                    };
                    let (parent, taken) = (c.parent, c.taken);
                    let (w2, rest2) = first_word(rest);
                    let value = if w2.is_empty() {
                        !taken
                    } else if ["ifeq", "ifneq", "ifdef", "ifndef"].contains(&w2) {
                        !taken && parent && self.condition(w2, rest2, name, lineno)?
                    } else {
                        return fatal(
                            Some((name.to_string(), lineno)),
                            "extraneous text after 'else' directive",
                        );
                    };
                    if let Some(c) = self.conds.last_mut() {
                        c.active = parent && value;
                        c.taken = taken || value;
                    }
                    continue;
                }
                "endif" => {
                    if self.conds.len() <= conds_at_start {
                        return fatal(Some((name.to_string(), lineno)), "extraneous 'endif'");
                    }
                    self.conds.pop();
                    continue;
                }
                _ => {}
            }
            if !self.active() {
                continue;
            }
            self.statement(trimmed, &file, lineno)?;
        }
        if self.conds.len() > conds_at_start {
            return fatal(Some((name.to_string(), lines.len())), "missing 'endif'");
        }
        self.depth -= 1;
        Ok(())
    }

    fn condition(&mut self, kind: &str, rest: &str, file: &str, line: usize) -> R<bool> {
        match kind {
            "ifdef" | "ifndef" => {
                let n = self.expand(rest, file, line)?;
                let defined = self
                    .mk
                    .vars
                    .get(n.trim())
                    .is_some_and(|v| !v.value.is_empty());
                Ok(defined == (kind == "ifdef"))
            }
            _ => {
                let (a, b) = match split_cond_args(rest) {
                    Some(ab) => ab,
                    None => {
                        return fatal(
                            Some((file.to_string(), line)),
                            "invalid syntax in conditional",
                        );
                    }
                };
                let a = self.expand(&a, file, line)?;
                let b = self.expand(&b, file, line)?;
                Ok((a == b) == (kind == "ifeq"))
            }
        }
    }

    fn statement(&mut self, line: &str, file: &Arc<str>, lineno: usize) -> R<()> {
        let (word, rest) = first_word(line);
        match word {
            "include" | "-include" | "sinclude" => {
                let names = self.expand(rest, file, lineno)?;
                for n in words(&names) {
                    let n = n.to_string();
                    match self.oracle.files.get(&n) {
                        None => return Err(Stop::Need(Need::Read(n))),
                        Some(Some(text)) => {
                            let text = text.clone();
                            let saved = std::mem::replace(&mut self.current, Current::None);
                            self.file(&n, &text)?;
                            self.current = saved;
                        }
                        Some(None) if word == "include" => {
                            self.mk.messages.push(Message::Err(format!(
                                "{file}:{lineno}: {n}: No such file or directory\n"
                            )));
                            return fatal(None, format!("No rule to make target '{n}'"));
                        }
                        Some(None) => {}
                    }
                }
                self.current = Current::None;
                return Ok(());
            }
            "export" | "unexport" if !line_is_rule(rest) => {
                if rest.is_empty() {
                    self.mk.export_all = word == "export";
                    return Ok(());
                }
                if word == "export" && assignment_op(rest).is_some() {
                    let name = self.assignment(rest, false, file, lineno)?;
                    self.mk.exported.insert(name.clone());
                    self.mk.unexported.remove(&name);
                    return Ok(());
                }
                let names = self.expand(rest, file, lineno)?;
                for n in words(&names) {
                    if word == "export" {
                        self.mk.exported.insert(n.to_string());
                        self.mk.unexported.remove(n);
                    } else {
                        self.mk.unexported.insert(n.to_string());
                        self.mk.exported.remove(n);
                    }
                }
                return Ok(());
            }
            "override" if assignment_op(rest).is_some() => {
                self.assignment(rest, true, file, lineno)?;
                return Ok(());
            }
            "private" if assignment_op(rest).is_some() => {
                self.assignment(rest, false, file, lineno)?;
                return Ok(());
            }
            "vpath" => return Ok(()),
            _ => {}
        }
        if assignment_op(line).is_some() {
            self.assignment(line, false, file, lineno)?;
            return Ok(());
        }
        if line_is_rule(line) {
            return self.rule(line, file, lineno);
        }
        // A line that expands to nothing (`$(info ...)`) is fine.
        if self.expand(line, file, lineno)?.trim().is_empty() {
            return Ok(());
        }
        fatal(Some((file.to_string(), lineno)), "missing separator")
    }

    /// `NAME op VALUE`; returns the (expanded) name.
    fn assignment(
        &mut self,
        line: &str,
        is_override: bool,
        file: &str,
        lineno: usize,
    ) -> R<String> {
        let (pos, op) = assignment_op(line).unwrap_or((line.len(), "="));
        let name = self.expand(line[..pos].trim(), file, lineno)?;
        let name = name.trim().to_string();
        let value = line[pos + op.len()..].trim_start();
        self.assign(&name, op, value, is_override, file, lineno)?;
        self.current = Current::None;
        Ok(name)
    }

    fn assign(
        &mut self,
        name: &str,
        op: &str,
        value: &str,
        is_override: bool,
        file: &str,
        lineno: usize,
    ) -> R<()> {
        if name.is_empty() {
            return fatal(Some((file.to_string(), lineno)), "empty variable name");
        }
        let existing = self.mk.vars.get(name).cloned();
        if let Some(e) = &existing
            && !is_override
            && (e.origin == Origin::CommandLine
                || e.origin == Origin::Override
                || (self.env_override && e.origin == Origin::Environment))
        {
            return Ok(());
        }
        let origin = if is_override {
            Origin::Override
        } else {
            Origin::File
        };
        let var = match op {
            "=" => Var {
                value: value.to_string(),
                recursive: true,
                origin,
            },
            ":=" | "::=" => Var {
                value: self.expand(value, file, lineno)?,
                recursive: false,
                origin,
            },
            "?=" => {
                if existing.is_some() {
                    return Ok(());
                }
                Var {
                    value: value.to_string(),
                    recursive: true,
                    origin,
                }
            }
            "+=" => match existing {
                Some(e) => {
                    let add = if e.recursive {
                        value.to_string()
                    } else {
                        self.expand(value, file, lineno)?
                    };
                    let joined = if e.value.is_empty() {
                        add
                    } else {
                        format!("{} {add}", e.value)
                    };
                    Var {
                        value: joined,
                        recursive: e.recursive,
                        origin: if e.origin == Origin::Override {
                            e.origin
                        } else {
                            origin
                        },
                    }
                }
                None => Var {
                    value: value.to_string(),
                    recursive: true,
                    origin,
                },
            },
            // `!=`: run the value as a shell command now.
            _ => {
                let cmd = self.expand(value, file, lineno)?;
                let out = match self.oracle.shell.get(&cmd) {
                    Some(o) => o.clone(),
                    None => return Err(Stop::Need(Need::Shell(cmd))),
                };
                Var {
                    value: out,
                    recursive: false,
                    origin,
                }
            }
        };
        self.mk.vars.insert(name.to_string(), var);
        Ok(())
    }

    fn set_simple(&mut self, name: &str, value: String, origin: Origin) {
        self.mk.vars.insert(
            name.to_string(),
            Var {
                value,
                recursive: false,
                origin,
            },
        );
    }

    fn rule(&mut self, line: &str, file: &Arc<str>, lineno: usize) -> R<()> {
        let colon = top_level_find(line, ':').unwrap_or(line.len());
        let targets_raw = &line[..colon];
        let mut rest = &line[colon + 1..];
        if let Some(r) = rest.strip_prefix(':') {
            // `::` rules are treated as ordinary rules.
            rest = r;
        }
        // Inline recipe after `;`.
        let (rest, inline) = match top_level_find(rest, ';') {
            Some(p) => (&rest[..p], Some(rest[p + 1..].trim_start().to_string())),
            None => (rest, None),
        };
        let targets = self.expand(targets_raw, file, lineno)?;
        let targets: Vec<String> = words(&targets).map(str::to_string).collect();
        if targets.is_empty() {
            if rest.trim().is_empty() && inline.is_none() {
                self.current = Current::None;
                return Ok(());
            }
            return fatal(Some((file.to_string(), lineno)), "missing target pattern");
        }

        // Target-specific variable: `target: VAR = value`.
        if let Some((pos, op)) = assignment_op(rest) {
            let (is_override, body) = match first_word(rest) {
                ("override", b) => (true, b),
                ("export", b) | ("private", b) => (false, b),
                _ => (false, rest.trim_start()),
            };
            let _ = pos;
            let (p2, op2) = assignment_op(body).unwrap_or((pos, op));
            let name = self.expand(body[..p2].trim(), file, lineno)?;
            let value = body[p2 + op2.len()..].trim_start();
            for t in &targets {
                let base = self.mk.target_vars.remove(t).unwrap_or_default();
                // Assign against a scratch view that sees target vars over globals.
                let saved = std::mem::take(&mut self.mk.vars);
                let mut view = saved.clone();
                view.extend(base.clone());
                self.mk.vars = view;
                let r = self.assign(name.trim(), op2, value, is_override, file, lineno);
                let assigned = self.mk.vars.remove(name.trim());
                self.mk.vars = saved;
                r?;
                let mut base = base;
                if let Some(v) = assigned {
                    base.insert(name.trim().to_string(), v);
                }
                self.mk.target_vars.insert(t.clone(), base);
            }
            self.current = Current::None;
            return Ok(());
        }

        let expanded = self.expand(rest, file, lineno)?;
        // Static pattern rule: `targets: target-pattern: prereq-patterns`.
        let (static_pat, prereq_text) = match expanded.split_once(':') {
            Some((tp, pp)) => (Some(tp.trim().to_string()), pp.to_string()),
            None => (None, expanded),
        };
        let (normal, order): (Vec<String>, Vec<String>) = {
            let mut normal = Vec::new();
            let mut order = Vec::new();
            let mut after_bar = false;
            for w in words(&prereq_text) {
                if w == "|" {
                    after_bar = true;
                } else if after_bar {
                    order.push(w.to_string());
                } else {
                    normal.push(w.to_string());
                }
            }
            (normal, order)
        };

        let first = targets[0].as_str();
        if first.starts_with('.') && self.special_target(first, &normal) {
            self.current = Current::None;
            return Ok(());
        }

        // Old-style suffix rule `.c.o:` becomes `%.o: %.c`.
        let suffix_rule = (targets.len() == 1 && normal.is_empty() && static_pat.is_none())
            .then(|| suffix_rule(first))
            .flatten();

        if let Some((tpat, ppat)) = suffix_rule {
            self.mk.patterns.push(PatternRule {
                targets: vec![tpat],
                prereqs: vec![ppat],
                order_only: Vec::new(),
                recipe: Vec::new(),
            });
            self.current = Current::Pattern(self.mk.patterns.len() - 1);
        } else if static_pat.is_none() && targets.iter().any(|t| t.contains('%')) {
            self.mk.patterns.push(PatternRule {
                targets,
                prereqs: normal,
                order_only: order,
                recipe: Vec::new(),
            });
            self.current = Current::Pattern(self.mk.patterns.len() - 1);
        } else {
            if self.mk.default_goal.is_none()
                && let Some(goal) = targets
                    .iter()
                    .find(|t| !t.starts_with('.') || t.contains('/'))
            {
                self.mk.default_goal = Some(goal.clone());
            }
            for t in &targets {
                let (prereqs, stem) = match &static_pat {
                    Some(tp) => {
                        let Some(stem) = super::expand::pattern_match(tp, t) else {
                            self.mk.messages.push(Message::Err(format!(
                                "{file}:{lineno}: target '{t}' doesn't match the target pattern\n"
                            )));
                            continue;
                        };
                        let stem = stem.to_string();
                        (
                            normal.iter().map(|p| p.replacen('%', &stem, 1)).collect(),
                            Some(stem),
                        )
                    }
                    None => (normal.clone(), None),
                };
                let rule = self.mk.rules.entry(t.clone()).or_default();
                rule.prereqs.extend(prereqs);
                rule.order_only.extend(order.iter().cloned());
                if stem.is_some() {
                    rule.stem = stem;
                }
            }
            self.fresh.extend(targets.iter().cloned());
            self.current = Current::Explicit(targets);
        }
        if let Some(text) = inline {
            self.add_recipe(RecipeLine {
                text,
                file: file.clone(),
                line: lineno,
            });
        }
        Ok(())
    }

    /// Handle `.PHONY` and friends. Returns true when `name` is special.
    fn special_target(&mut self, name: &str, prereqs: &[String]) -> bool {
        match name {
            ".PHONY" => self.mk.phony.extend(prereqs.iter().cloned()),
            ".SILENT" => {
                if prereqs.is_empty() {
                    self.mk.silent_all = true;
                } else {
                    self.mk.silent.extend(prereqs.iter().cloned());
                }
            }
            ".IGNORE" => self.mk.ignore_all |= prereqs.is_empty(),
            ".ONESHELL" => self.mk.oneshell = true,
            ".EXPORT_ALL_VARIABLES" => self.mk.export_all = true,
            ".SUFFIXES"
            | ".DELETE_ON_ERROR"
            | ".PRECIOUS"
            | ".INTERMEDIATE"
            | ".SECONDARY"
            | ".NOTPARALLEL"
            | ".POSIX"
            | ".LOW_RESOLUTION_TIME"
            | ".SECONDEXPANSION"
            | ".DEFAULT"
            | ".NOTINTERMEDIATE"
            | ".WAIT" => {}
            _ => return false,
        }
        true
    }

    fn add_recipe(&mut self, line: RecipeLine) {
        match &self.current {
            Current::None => {}
            Current::Pattern(i) => {
                if let Some(p) = self.mk.patterns.get_mut(*i) {
                    p.recipe.push(line);
                }
            }
            Current::Explicit(ts) => {
                for t in ts.clone() {
                    let fresh = self.fresh.remove(&t);
                    let rule = self.mk.rules.entry(t).or_default();
                    if fresh {
                        // A later rule with a recipe overrides the earlier one.
                        rule.recipe = Some(Vec::new());
                    }
                    rule.recipe.get_or_insert_with(Vec::new).push(line.clone());
                }
            }
        }
    }
}

/// `.c.o` → (`%.o`, `%.c`); `.sh` → (`%`, `%.sh`).
fn suffix_rule(t: &str) -> Option<(String, String)> {
    let known = |s: &str| SUFFIXES.contains(&s);
    if known(t) {
        return Some(("%".to_string(), format!("%{t}")));
    }
    let rest = t.strip_prefix('.')?;
    let dot = rest.find('.')?;
    let (a, b) = (&t[..dot + 1], &t[dot + 1..]);
    (known(a) && known(b)).then(|| (format!("%{b}"), format!("%{a}")))
}

/// Find the assignment operator, if `line` is an assignment.
pub(super) fn assignment_op(line: &str) -> Option<(usize, &'static str)> {
    let mut depth = 0usize;
    let mut prev = '\0';
    let b = line.as_bytes();
    for (i, c) in line.char_indices() {
        match c {
            '(' | '{' if prev == '$' || depth > 0 => depth += 1,
            ')' | '}' if depth > 0 => depth -= 1,
            ';' if depth == 0 => return None,
            ':' if depth == 0 => {
                if line[i..].starts_with("::=") {
                    return Some((i, "::="));
                }
                if b.get(i + 1) == Some(&b'=') {
                    return Some((i, ":="));
                }
                return None;
            }
            '=' if depth == 0 => {
                return match prev {
                    '?' => Some((i - 1, "?=")),
                    '+' => Some((i - 1, "+=")),
                    '!' => Some((i - 1, "!=")),
                    _ => Some((i, "=")),
                };
            }
            _ => {}
        }
        prev = c;
    }
    None
}

fn line_is_rule(line: &str) -> bool {
    top_level_find(line, ':').is_some() && assignment_op(line).is_none()
}

/// `(a,b)` or `"a" "b"` / `'a' 'b'`.
fn split_cond_args(s: &str) -> Option<(String, String)> {
    let s = s.trim();
    if let Some(inner) = s.strip_prefix('(').and_then(|r| r.strip_suffix(')')) {
        let comma = top_level_comma(inner)?;
        return Some((
            inner[..comma].trim().to_string(),
            inner[comma + 1..].trim().to_string(),
        ));
    }
    let mut parts = Vec::new();
    let mut rest = s;
    for _ in 0..2 {
        rest = rest.trim_start();
        let q = rest.chars().next()?;
        if q != '"' && q != '\'' {
            return None;
        }
        let end = rest[1..].find(q)? + 1;
        parts.push(rest[1..end].to_string());
        rest = &rest[end + 1..];
    }
    rest.trim()
        .is_empty()
        .then(|| (parts[0].clone(), parts[1].clone()))
}

fn top_level_comma(s: &str) -> Option<usize> {
    let mut depth = 0usize;
    for (i, c) in s.char_indices() {
        match c {
            '(' | '{' => depth += 1,
            ')' | '}' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => return Some(i),
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_str(text: &str) -> Makefile {
        let oracle = Oracle::default();
        match parse(
            ParseInput {
                name: "Makefile",
                text,
                vars: Vars::new(),
                curdir: "/w",
                env_override: false,
            },
            &oracle,
            &mut Vec::new(),
        ) {
            Ok(m) => m,
            Err(Stop::Fatal { msg, .. }) => panic!("fatal: {msg}"),
            Err(Stop::Need(_)) => panic!("need"),
        }
    }

    #[test]
    fn rules_vars_and_conditionals() {
        let mk = parse_str(
            "CC = cc\nOBJ := a.o \\\n  b.o\nifeq ($(CC),cc)\nMODE = yes\nelse\nMODE = no\nendif\n\
             .PHONY: all clean\nall: prog\nprog: $(OBJ) | outdir\n\t$(CC) -o $@ $^\n\
             %.o: %.c\n\t$(CC) -c $<\nclean: ; rm -f prog\n",
        );
        assert_eq!(mk.default_goal.as_deref(), Some("all"));
        assert_eq!(mk.vars["OBJ"].value, "a.o b.o");
        assert_eq!(mk.vars["MODE"].value, "yes");
        let prog = &mk.rules["prog"];
        assert_eq!(prog.prereqs, vec!["a.o", "b.o"]);
        assert_eq!(prog.order_only, vec!["outdir"]);
        assert_eq!(
            prog.recipe.as_ref().map(|r| r[0].text.as_str()),
            Some("$(CC) -o $@ $^")
        );
        assert_eq!(mk.patterns[0].targets, vec!["%.o"]);
        assert_eq!(
            mk.rules["clean"]
                .recipe
                .as_ref()
                .map(|r| r[0].text.as_str()),
            Some("rm -f prog")
        );
        assert!(mk.phony.contains("clean"));
    }

    #[test]
    fn define_static_pattern_and_suffix_rules() {
        let mk = parse_str(
            "define GREET\necho hi\necho there\nendef\nobjs = x.o y.o\n$(objs): %.o: %.c\n\tcc $<\n.c.o:\n\tcc -c $<\n",
        );
        assert_eq!(mk.vars["GREET"].value, "echo hi\necho there");
        assert_eq!(mk.rules["x.o"].prereqs, vec!["x.c"]);
        assert_eq!(mk.rules["y.o"].stem.as_deref(), Some("y"));
        assert_eq!(mk.patterns[0].targets, vec!["%.o"]);
        assert_eq!(mk.patterns[0].prereqs, vec!["%.c"]);
    }

    #[test]
    fn missing_separator_is_fatal() {
        let oracle = Oracle::default();
        let r = parse(
            ParseInput {
                name: "Makefile",
                text: "x:\n    echo hi\n",
                vars: Vars::new(),
                curdir: "/w",
                env_override: false,
            },
            &oracle,
            &mut Vec::new(),
        );
        assert!(
            matches!(r, Err(Stop::Fatal { ref msg, loc: Some((_, 2)) }) if msg == "missing separator")
        );
    }

    #[test]
    fn assignment_operators() {
        assert_eq!(assignment_op("A = b"), Some((2, "=")));
        assert_eq!(assignment_op("A := b"), Some((2, ":=")));
        assert_eq!(assignment_op("A ::= b"), Some((2, "::=")));
        assert_eq!(assignment_op("A ?= b"), Some((2, "?=")));
        assert_eq!(assignment_op("A += b"), Some((2, "+=")));
        assert_eq!(assignment_op("A != b"), Some((2, "!=")));
        assert_eq!(assignment_op("a: b"), None);
        assert_eq!(assignment_op("$(x:a=b): c"), None);
        assert_eq!(assignment_op("x: ; A=1 cmd"), None);
    }
}
