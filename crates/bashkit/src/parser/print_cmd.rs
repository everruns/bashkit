//! Function body printer for `type` and `declare -f`.
//!
//! Important decision: a port of bash's `print_cmd.c` layout rules
//! (4-space indent, `;` + newline between commands, `if`/`elif` printed as
//! nested `else if`, deferred here-document bodies, `was_heredoc` quirks)
//! over bashkit's AST. Words print as written (`Word::raw`, recorded by the
//! parser inside function definitions); a word without source text falls
//! back to a reconstruction from its parts. Output never uses `Debug`
//! (TM-INF-022).

use super::ast::*;
use super::raw::heredoc_eof_from_raw;

const INDENT_AMOUNT: usize = 4;

/// Bash's text for `type NAME` / `declare -f NAME`: `name () \n{ \n...\n}`.
pub fn function_string(name: &str, body: &Command) -> String {
    let mut p = Printer::default();
    if is_reserved_word(name) {
        p.out.push_str("function ");
    }
    p.out.push_str(name);
    p.out.push_str(" () \n");
    p.inside_function_def += 1;
    p.out.push_str("{ \n");
    p.indentation += INDENT_AMOUNT;
    let func_redirects = p.print_function_body(body);
    p.print_deferred_if_any("");
    p.indentation -= INDENT_AMOUNT;
    p.inside_function_def -= 1;
    p.close_function(func_redirects);
    p.out
}

fn is_reserved_word(name: &str) -> bool {
    matches!(
        name,
        "if" | "then"
            | "else"
            | "elif"
            | "fi"
            | "case"
            | "esac"
            | "for"
            | "select"
            | "while"
            | "until"
            | "do"
            | "done"
            | "in"
            | "function"
            | "time"
            | "{"
            | "}"
            | "!"
            | "[["
            | "]]"
            | "coproc"
    )
}

#[derive(Clone, Copy, PartialEq)]
enum Conn {
    Semi,
    Amp,
    And,
    Or,
    Pipe,
}

/// A here-document whose body prints after the current line.
struct HereDoc {
    body: String,
    eof: String,
}

#[derive(Default)]
struct Printer {
    out: String,
    indentation: usize,
    skip_this_indent: usize,
    was_heredoc: bool,
    printing_connection: usize,
    inside_function_def: usize,
    deferred: Vec<HereDoc>,
}

impl Printer {
    fn indent(&mut self, n: usize) {
        for _ in 0..n {
            self.out.push(' ');
        }
    }

    /// `newline (s)`: newline, indent, then `s`.
    fn newline(&mut self, s: &str) {
        self.out.push('\n');
        self.indent(self.indentation);
        self.out.push_str(s);
    }

    /// `semicolon ()`: a `;` unless the text already ends a line or a job.
    fn semicolon(&mut self) {
        if matches!(self.out.chars().last(), Some('&' | '\n')) {
            return;
        }
        self.out.push(';');
    }

    /// Prints a function body; returns the redirections of a `{ }` body,
    /// which print after its closing brace.
    fn print_function_body<'a>(&mut self, body: &'a Command) -> &'a [Redirect] {
        match body {
            Command::Compound(CompoundCommand::BraceGroup(cmds), redirects) => {
                self.print_list(cmds);
                redirects
            }
            other => {
                self.make(Some(other));
                &[]
            }
        }
    }

    fn close_function(&mut self, func_redirects: &[Redirect]) {
        if func_redirects.is_empty() {
            self.newline("}");
        } else {
            self.newline("} ");
            self.print_redirection_list(func_redirects);
        }
    }

    fn print_deferred_if_any(&mut self, cstring: &str) {
        if !self.deferred.is_empty() {
            self.print_deferred_heredocs(cstring);
        }
    }

    fn print_deferred_heredocs(&mut self, cstring: &str) {
        let prints_cstring = !cstring.is_empty() && cstring != ";";
        if prints_cstring {
            self.out.push_str(cstring);
        }
        if !self.deferred.is_empty() {
            let docs = std::mem::take(&mut self.deferred);
            self.print_heredoc_bodies(&docs);
            if prints_cstring {
                self.out.push(' ');
            }
            self.was_heredoc = true;
        }
    }

    fn print_heredoc_bodies(&mut self, docs: &[HereDoc]) {
        self.out.push('\n');
        for doc in docs {
            self.out.push_str(&doc.body);
            self.out.push_str(&doc.eof);
            self.out.push('\n');
        }
    }

    /// A command sequence (`;`/newline separated).
    fn print_list(&mut self, cmds: &[Command]) {
        match cmds {
            [] => self.make(None),
            [only] => self.make(Some(only)),
            [first, rest @ ..] => {
                let rest: Vec<(Conn, Option<&Command>)> =
                    rest.iter().map(|c| (Conn::Semi, Some(c))).collect();
                self.make_connection(first, &rest);
            }
        }
    }

    /// `make_command_string_internal`.
    fn make(&mut self, cmd: Option<&Command>) {
        let Some(cmd) = cmd else {
            return;
        };
        if self.skip_this_indent > 0 {
            self.skip_this_indent -= 1;
        } else {
            self.indent(self.indentation);
        }
        match cmd {
            Command::Simple(sc) => self.print_simple(sc),
            Command::Pipeline(p) => {
                if p.negated {
                    self.out.push_str("! ");
                }
                match p.commands.as_slice() {
                    [] => {}
                    [only] => {
                        self.skip_this_indent += 1;
                        self.make(Some(only));
                    }
                    [first, rest @ ..] => {
                        let rest: Vec<(Conn, Option<&Command>)> =
                            rest.iter().map(|c| (Conn::Pipe, Some(c))).collect();
                        self.connection_body(first, &rest);
                    }
                }
            }
            Command::List(list) => {
                let rest: Vec<(Conn, Option<&Command>)> = list
                    .rest
                    .iter()
                    .map(|(op, c)| {
                        let conn = match op {
                            ListOperator::And => Conn::And,
                            ListOperator::Or => Conn::Or,
                            ListOperator::Semicolon => Conn::Semi,
                            ListOperator::Background => Conn::Amp,
                        };
                        let second = if is_empty_placeholder(c) {
                            None
                        } else {
                            Some(c)
                        };
                        (conn, second)
                    })
                    .collect();
                self.connection_body(&list.first, &rest);
            }
            Command::Compound(cc, redirects) => {
                self.print_compound(cc);
                if !redirects.is_empty() {
                    self.out.push(' ');
                    self.print_redirection_list(redirects);
                }
            }
            Command::Function(f) => self.print_function_def(f),
        }
    }

    /// A connection chain, as an indented command of its own.
    fn make_connection(&mut self, first: &Command, rest: &[(Conn, Option<&Command>)]) {
        if self.skip_this_indent > 0 {
            self.skip_this_indent -= 1;
        } else {
            self.indent(self.indentation);
        }
        self.connection_body(first, rest);
    }

    /// The `cm_connection` case, flattened over a left-associative chain:
    /// each link ends like bash's nested connection (deferred heredocs).
    fn connection_body(&mut self, first: &Command, rest: &[(Conn, Option<&Command>)]) {
        self.skip_this_indent += 1;
        self.printing_connection += 1;
        self.make(Some(first));
        for (conn, second) in rest {
            match conn {
                Conn::Amp | Conn::Pipe => {
                    let s = if *conn == Conn::Amp { " &" } else { " |" };
                    self.print_deferred_heredocs(s);
                    if *conn != Conn::Amp || second.is_some() {
                        self.out.push(' ');
                        self.skip_this_indent += 1;
                    }
                }
                Conn::And | Conn::Or => {
                    let s = if *conn == Conn::And { " && " } else { " || " };
                    self.print_deferred_heredocs(s);
                    if second.is_some() {
                        self.skip_this_indent += 1;
                    }
                }
                Conn::Semi => {
                    if self.deferred.is_empty() {
                        if !self.was_heredoc {
                            self.out.push(';');
                        } else {
                            self.was_heredoc = false;
                        }
                    } else {
                        let s = if self.inside_function_def > 0 {
                            ""
                        } else {
                            ";"
                        };
                        self.print_deferred_heredocs(s);
                    }
                    if self.inside_function_def > 0 {
                        self.out.push('\n');
                    } else {
                        self.out.push(' ');
                        if second.is_some() {
                            self.skip_this_indent += 1;
                        }
                    }
                }
            }
            self.make(*second);
            self.print_deferred_if_any("");
        }
        self.printing_connection -= 1;
    }

    fn print_simple(&mut self, sc: &SimpleCommand) {
        let mut words: Vec<String> = Vec::new();
        for a in &sc.assignments {
            words.push(assignment_text(a));
        }
        let name = word_text(&sc.name);
        if !(name.is_empty() && sc.name.raw.is_none()) {
            words.push(name);
        }
        words.extend(sc.args.iter().map(word_text));
        self.out.push_str(&words.join(" "));
        if !sc.redirects.is_empty() {
            if !words.is_empty() {
                self.out.push(' ');
            }
            self.print_redirection_list(&sc.redirects);
        }
    }

    fn print_redirection_list(&mut self, redirects: &[Redirect]) {
        let mut heredocs = Vec::new();
        self.was_heredoc = false;
        for (i, r) in redirects.iter().enumerate() {
            if i > 0 {
                self.out.push(' ');
            }
            if matches!(r.kind, RedirectKind::HereDoc | RedirectKind::HereDocStrip) {
                let delim = r.heredoc_delim.clone().unwrap_or_default();
                self.out.push_str(&redirector(r, 0));
                self.out.push_str(if r.kind == RedirectKind::HereDocStrip {
                    "<<-"
                } else {
                    "<<"
                });
                self.out.push_str(&delim);
                let eof = if delim.starts_with('\'') {
                    heredoc_eof_from_raw(&delim).0
                } else {
                    delim
                };
                heredocs.push(HereDoc {
                    body: r
                        .target
                        .raw
                        .clone()
                        .unwrap_or_else(|| reconstruct_word(&r.target)),
                    eof,
                });
            } else {
                self.out.push_str(&redirection_text(r));
            }
        }
        if heredocs.is_empty() {
            return;
        }
        if self.printing_connection > 0 {
            self.deferred.extend(heredocs);
        } else {
            self.print_heredoc_bodies(&heredocs);
        }
    }

    fn print_compound(&mut self, cc: &CompoundCommand) {
        match cc {
            CompoundCommand::If(c) => self.print_if(
                &c.condition,
                &c.then_branch,
                &c.elif_branches,
                c.else_branch.as_deref(),
            ),
            CompoundCommand::For(f) => {
                self.out.push_str("for ");
                self.out.push_str(&f.variable);
                self.out.push_str(" in ");
                match &f.words {
                    Some(words) => {
                        let w: Vec<String> = words.iter().map(word_text).collect();
                        self.out.push_str(&w.join(" "));
                    }
                    None => self.out.push_str("\"$@\""),
                }
                self.loop_body_after_head(&f.body);
            }
            CompoundCommand::Select(f) => {
                self.out.push_str("select ");
                self.out.push_str(&f.variable);
                self.out.push_str(" in ");
                let w: Vec<String> = f.words.iter().map(word_text).collect();
                self.out.push_str(&w.join(" "));
                self.loop_body_after_head(&f.body);
            }
            CompoundCommand::ArithmeticFor(f) => {
                let parts: Vec<String> = match &f.raw {
                    Some(raw) => raw.iter().map(|p| arith_for_part(p)).collect(),
                    None => [&f.init, &f.condition, &f.step]
                        .iter()
                        .map(|p| arith_for_part(p))
                        .collect(),
                };
                self.out.push_str("for ((");
                self.out.push_str(&parts.join("; "));
                self.out.push_str("))");
                self.newline("do\n");
                self.indentation += INDENT_AMOUNT;
                self.print_list(&f.body);
                self.print_deferred_if_any("");
                self.semicolon();
                self.indentation -= INDENT_AMOUNT;
                self.newline("done");
            }
            CompoundCommand::While(w) => self.print_until_or_while("while", &w.condition, &w.body),
            CompoundCommand::Until(w) => self.print_until_or_while("until", &w.condition, &w.body),
            CompoundCommand::Case(c) => {
                self.out.push_str("case ");
                self.out.push_str(&word_text(&c.word));
                self.out.push_str(" in ");
                self.indentation += INDENT_AMOUNT;
                for item in &c.cases {
                    self.newline("");
                    let pats: Vec<String> = item.patterns.iter().map(word_text).collect();
                    self.out.push_str(&pats.join(" | "));
                    self.out.push_str(")\n");
                    self.indentation += INDENT_AMOUNT;
                    self.print_list(&item.commands);
                    self.indentation -= INDENT_AMOUNT;
                    self.print_deferred_if_any("");
                    self.newline(match item.terminator {
                        CaseTerminator::Break => ";;",
                        CaseTerminator::FallThrough => ";&",
                        CaseTerminator::Continue => ";;&",
                    });
                }
                self.indentation -= INDENT_AMOUNT;
                self.newline("esac");
            }
            CompoundCommand::Subshell(cmds) => {
                self.out.push_str("( ");
                self.skip_this_indent += 1;
                self.print_list(cmds);
                self.print_deferred_if_any("");
                self.out.push_str(" )");
            }
            CompoundCommand::BraceGroup(cmds) => {
                self.out.push_str("{ ");
                if self.inside_function_def == 0 {
                    self.skip_this_indent += 1;
                } else {
                    self.out.push('\n');
                    self.indentation += INDENT_AMOUNT;
                }
                self.print_list(cmds);
                self.print_deferred_if_any("");
                if self.inside_function_def > 0 {
                    self.out.push('\n');
                    self.indentation -= INDENT_AMOUNT;
                    self.indent(self.indentation);
                } else {
                    self.semicolon();
                    self.out.push(' ');
                }
                self.out.push('}');
            }
            CompoundCommand::Arithmetic(expr) => {
                self.out.push_str("((");
                self.out.push_str(expr);
                self.out.push_str("))");
            }
            CompoundCommand::Time(t) => {
                self.out.push_str("time");
                if t.posix_format {
                    self.out.push_str(" -p");
                }
                if let Some(cmd) = &t.command {
                    self.out.push(' ');
                    self.skip_this_indent += 1;
                    self.make(Some(cmd));
                }
            }
            CompoundCommand::Conditional(words) => {
                self.out.push_str("[[ ");
                let toks: Vec<String> = words.iter().map(word_text).collect();
                self.out.push_str(&cond_text(&toks));
                self.out.push_str(" ]]");
            }
            CompoundCommand::Coproc(c) => {
                self.out.push_str("coproc ");
                self.out.push_str(&c.name);
                self.out.push(' ');
                self.skip_this_indent += 1;
                self.make(Some(&c.body));
            }
        }
    }

    fn loop_body_after_head(&mut self, body: &[Command]) {
        self.out.push(';');
        self.newline("do\n");
        self.indentation += INDENT_AMOUNT;
        self.print_list(body);
        self.print_deferred_if_any("");
        self.semicolon();
        self.indentation -= INDENT_AMOUNT;
        self.newline("done");
    }

    fn print_until_or_while(&mut self, which: &str, test: &[Command], body: &[Command]) {
        self.out.push_str(which);
        self.out.push(' ');
        self.skip_this_indent += 1;
        self.print_list(test);
        self.print_deferred_if_any("");
        self.semicolon();
        self.out.push_str(" do\n");
        self.indentation += INDENT_AMOUNT;
        self.print_list(body);
        self.print_deferred_if_any("");
        self.indentation -= INDENT_AMOUNT;
        self.semicolon();
        self.newline("done");
    }

    /// `if`; bash stores `elif` as a nested `if` in the `else` branch.
    fn print_if(
        &mut self,
        test: &[Command],
        then: &[Command],
        elifs: &[(Vec<Command>, Vec<Command>)],
        else_branch: Option<&[Command]>,
    ) {
        self.out.push_str("if ");
        self.skip_this_indent += 1;
        self.print_list(test);
        self.semicolon();
        self.out.push_str(" then\n");
        self.indentation += INDENT_AMOUNT;
        self.print_list(then);
        self.print_deferred_if_any("");
        self.indentation -= INDENT_AMOUNT;
        if let Some(((etest, ethen), rest)) = elifs.split_first() {
            self.semicolon();
            self.newline("else\n");
            self.indentation += INDENT_AMOUNT;
            self.indent(self.indentation);
            self.print_if(etest, ethen, rest, else_branch);
            self.print_deferred_if_any("");
            self.indentation -= INDENT_AMOUNT;
        } else if let Some(else_cmds) = else_branch {
            self.semicolon();
            self.newline("else\n");
            self.indentation += INDENT_AMOUNT;
            self.print_list(else_cmds);
            self.print_deferred_if_any("");
            self.indentation -= INDENT_AMOUNT;
        }
        self.semicolon();
        self.newline("fi");
    }

    /// Nested `function name () { ... }` inside a body.
    fn print_function_def(&mut self, f: &FunctionDef) {
        self.out.push_str("function ");
        self.out.push_str(&f.name);
        self.out.push_str(" () \n");
        self.indent(self.indentation);
        self.out.push_str("{ \n");
        self.inside_function_def += 1;
        self.indentation += INDENT_AMOUNT;
        let func_redirects = self.print_function_body(&f.body);
        self.print_deferred_if_any("");
        self.indentation -= INDENT_AMOUNT;
        self.inside_function_def -= 1;
        self.close_function(func_redirects);
    }
}

/// The `cmd &` placeholder the parser appends after a trailing `&`.
fn is_empty_placeholder(cmd: &Command) -> bool {
    matches!(cmd, Command::Simple(sc)
        if sc.assignments.is_empty()
            && sc.args.is_empty()
            && sc.redirects.is_empty()
            && sc.name.raw.is_none()
            && sc.name.parts.iter().all(|p| matches!(p, WordPart::Literal(s) if s.is_empty())))
}

/// One clause of `for ((...))`: leading blanks dropped, empty means `1`.
fn arith_for_part(p: &str) -> String {
    let t = p.trim_start();
    if t.trim().is_empty() {
        "1".to_string()
    } else {
        t.to_string()
    }
}

fn redirector(r: &Redirect, default_fd: i32) -> String {
    if let Some(var) = &r.fd_var {
        return format!("{{{var}}}");
    }
    match r.fd {
        Some(fd) if fd != default_fd => fd.to_string(),
        _ => String::new(),
    }
}

fn redirection_text(r: &Redirect) -> String {
    let target = word_text(&r.target);
    match r.kind {
        RedirectKind::Output => format!("{}> {target}", redirector(r, 1)),
        RedirectKind::Clobber => format!("{}>| {target}", redirector(r, 1)),
        RedirectKind::Append => format!("{}>> {target}", redirector(r, 1)),
        RedirectKind::Input => format!("{}< {target}", redirector(r, 0)),
        RedirectKind::HereString => format!("{}<<< {target}", redirector(r, 0)),
        RedirectKind::ReadWrite => format!("{}<> {target}", redirector(r, 0)),
        RedirectKind::OutputBoth => format!("&> {target}"),
        RedirectKind::DupOutput => {
            let fd_is_default = r.fd_var.is_none() && r.fd.is_none_or(|fd| fd == 1);
            let word_target = !target.starts_with(|c: char| c == '-' || c.is_ascii_digit())
                && !target.starts_with('$');
            if fd_is_default && word_target {
                format!("&> {target}")
            } else {
                format!("{}>&{target}", redirector(r, 1))
            }
        }
        RedirectKind::DupInput => format!("{}<&{target}", redirector(r, 0)),
        RedirectKind::HereDoc | RedirectKind::HereDocStrip => String::new(),
    }
}

fn assignment_text(a: &Assignment) -> String {
    let mut s = a.name.clone();
    if let Some(idx) = &a.index {
        s.push('[');
        s.push_str(idx);
        s.push(']');
    }
    s.push_str(if a.append { "+=" } else { "=" });
    match &a.value {
        AssignmentValue::Scalar(w) => s.push_str(&word_text(w)),
        AssignmentValue::Array(elems) => {
            let e: Vec<String> = elems.iter().map(word_text).collect();
            s.push('(');
            s.push_str(&e.join(" "));
            s.push(')');
        }
    }
    s
}

/// A word as written: its source text without line continuations, or a
/// reconstruction when the parser did not record the source.
pub(crate) fn word_text(w: &Word) -> String {
    match &w.raw {
        Some(raw) => normalize_raw_word(raw),
        None => reconstruct_word(w),
    }
}

/// Source text the way bash prints it back: backslash-newline is dropped
/// outside single quotes, `$'...'` becomes the single-quoted text it decodes
/// to (`$'it\'s'` prints `'it'\''s'`) and `$"..."` prints as `"..."`.
/// Inside double quotes both stay as written.
fn normalize_raw_word(raw: &str) -> String {
    if !raw.contains("\\\n") && !raw.contains("$'") && !raw.contains("$\"") {
        return raw.to_string();
    }
    let mut out = String::with_capacity(raw.len());
    let mut in_single = false;
    let mut in_double = false;
    let mut i = 0;
    while let Some(c) = raw[i..].chars().next() {
        i += c.len_utf8();
        match c {
            '\'' if !in_double => {
                in_single = !in_single;
                out.push(c);
            }
            '"' if !in_single => {
                in_double = !in_double;
                out.push(c);
            }
            '$' if !in_single && !in_double && raw[i..].starts_with('\'') => {
                match super::lexer::Lexer::decode_ansi_c_body(&raw[i + 1..]) {
                    Some((text, used)) => {
                        out.push_str(&super::raw::single_quote(&text));
                        i += 1 + used;
                    }
                    None => out.push(c),
                }
            }
            '$' if !in_single && !in_double && raw[i..].starts_with('"') => {}
            '\\' if !in_single => {
                if raw[i..].starts_with('\n') {
                    i += 1;
                } else {
                    out.push(c);
                    if let Some(n) = raw[i..].chars().next() {
                        out.push(n);
                        i += n.len_utf8();
                    }
                }
            }
            _ => out.push(c),
        }
    }
    out
}

fn reconstruct_word(w: &Word) -> String {
    let mut s = String::new();
    for part in &w.parts {
        match part {
            WordPart::Literal(t) => s.push_str(t),
            WordPart::CommandSubstitution(cmds) => {
                s.push_str("$(");
                s.push_str(&inline_commands(cmds));
                s.push(')');
            }
            WordPart::ProcessSubstitution { commands, is_input } => {
                s.push_str(if *is_input { "<(" } else { ">(" });
                s.push_str(&inline_commands(commands));
                s.push(')');
            }
            other => {
                let single = Word {
                    parts: vec![other.clone()],
                    quoted: false,
                    has_unquoted_glob: false,
                    part_quoted: Vec::new(),
                    raw: None,
                };
                s.push_str(&single.to_string());
            }
        }
    }
    if w.quoted && w.parts.iter().all(|p| matches!(p, WordPart::Literal(_))) {
        return super::raw::single_quote(&s);
    }
    s
}

/// Commands printed on one line (inside `$(...)` reconstructions).
fn inline_commands(cmds: &[Command]) -> String {
    let mut p = Printer::default();
    p.print_list(cmds);
    p.print_deferred_if_any("");
    p.out
}

/// `[[ ... ]]` operands, as bash's `print_cond_node` prints its parse:
/// a lone operand becomes `-n WORD`.
fn cond_text(toks: &[String]) -> String {
    let mut pos = 0;
    let mut out = Vec::new();
    cond_or(toks, &mut pos, &mut out);
    // Anything the grammar did not consume prints as written.
    out.extend(toks[pos.min(toks.len())..].iter().cloned());
    out.join(" ")
}

fn cond_or(toks: &[String], pos: &mut usize, out: &mut Vec<String>) {
    cond_and(toks, pos, out);
    while toks.get(*pos).map(String::as_str) == Some("||") {
        out.push("||".into());
        *pos += 1;
        cond_and(toks, pos, out);
    }
}

fn cond_and(toks: &[String], pos: &mut usize, out: &mut Vec<String>) {
    cond_term(toks, pos, out);
    while toks.get(*pos).map(String::as_str) == Some("&&") {
        out.push("&&".into());
        *pos += 1;
        cond_term(toks, pos, out);
    }
}

fn is_cond_unary(op: &str) -> bool {
    matches!(
        op,
        "-a" | "-b"
            | "-c"
            | "-d"
            | "-e"
            | "-f"
            | "-g"
            | "-h"
            | "-k"
            | "-p"
            | "-r"
            | "-s"
            | "-t"
            | "-u"
            | "-w"
            | "-x"
            | "-O"
            | "-G"
            | "-L"
            | "-S"
            | "-N"
            | "-n"
            | "-z"
            | "-o"
            | "-v"
            | "-R"
    )
}

fn is_cond_binary(op: &str) -> bool {
    matches!(
        op,
        "==" | "="
            | "!="
            | "<"
            | ">"
            | "=~"
            | "-eq"
            | "-ne"
            | "-lt"
            | "-le"
            | "-gt"
            | "-ge"
            | "-nt"
            | "-ot"
            | "-ef"
    )
}

fn cond_term(toks: &[String], pos: &mut usize, out: &mut Vec<String>) {
    let Some(tok) = toks.get(*pos) else {
        return;
    };
    match tok.as_str() {
        "!" => {
            out.push("!".into());
            *pos += 1;
            cond_term(toks, pos, out);
        }
        "(" => {
            out.push("(".into());
            *pos += 1;
            cond_or(toks, pos, out);
            if toks.get(*pos).map(String::as_str) == Some(")") {
                *pos += 1;
            }
            out.push(")".into());
        }
        _ => {
            let next = toks.get(*pos + 1).map(String::as_str);
            if next.is_some_and(is_cond_binary) {
                out.push(tok.clone());
                out.push(toks[*pos + 1].clone());
                if let Some(rhs) = toks.get(*pos + 2) {
                    out.push(rhs.clone());
                }
                *pos += 3;
            } else if is_cond_unary(tok) && next.is_some_and(|n| n != "&&" && n != "||" && n != ")")
            {
                out.push(tok.clone());
                out.push(toks[*pos + 1].clone());
                *pos += 2;
            } else {
                out.push("-n".into());
                out.push(tok.clone());
                *pos += 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::Parser;

    fn print_fn(src: &str) -> String {
        let script = Parser::new(src).parse().expect("parses");
        let Some(Command::Function(f)) = script.commands.first() else {
            panic!("expected a function definition");
        };
        function_string(&f.name, &f.body)
    }

    #[test]
    fn simple_body_keeps_words_as_written() {
        assert_eq!(
            print_fn("f() { echo 'a b' \"$x\" \\$y >out; }"),
            "f () \n{ \n    echo 'a b' \"$x\" \\$y > out\n}"
        );
    }

    #[test]
    fn elif_prints_as_nested_else_if() {
        assert_eq!(
            print_fn("f() { if a; then b; elif c; then d; fi; }"),
            "f () \n{ \n    if a; then\n        b;\n    else\n        if c; then\n            d;\n        fi;\n    fi\n}"
        );
    }

    #[test]
    fn heredoc_body_follows_its_line() {
        assert_eq!(
            print_fn("f() { cat <<'E'\n$x\nE\necho z; }"),
            "f () \n{ \n    cat <<'E'\n$x\nE\n\n    echo z\n}"
        );
    }

    #[test]
    fn lone_cond_operand_gets_dash_n() {
        assert_eq!(
            print_fn("f() { [[ x ]]; [[ ! ( a == b ) ]]; }"),
            "f () \n{ \n    [[ -n x ]];\n    [[ ! ( a == b ) ]]\n}"
        );
    }

    #[test]
    fn pipe_both_prints_as_dup_redirect() {
        assert_eq!(print_fn("f() { a |& b; }"), "f () \n{ \n    a 2>&1 | b\n}");
    }

    #[test]
    fn reserved_word_name_gets_function_keyword() {
        let body = Parser::new("{ :; }").parse().expect("parses").commands[0].clone();
        assert!(function_string("if", &body).starts_with("function if () \n"));
    }

    #[test]
    fn word_without_source_text_is_reconstructed() {
        assert_eq!(word_text(&Word::literal("plain")), "plain");
        assert_eq!(word_text(&Word::quoted_literal("a b")), "'a b'");
    }

    #[test]
    fn line_continuation_dropped_outside_single_quotes() {
        assert_eq!(normalize_raw_word("a\\\nb"), "ab");
        assert_eq!(normalize_raw_word("'a\\\nb'"), "'a\\\nb'");
    }

    #[test]
    fn ansi_c_and_locale_quotes_print_as_plain_quotes() {
        assert_eq!(normalize_raw_word("$'a\\tb'"), "'a\tb'");
        assert_eq!(normalize_raw_word("$'it\\'s'"), "'it'\\''s'");
        assert_eq!(normalize_raw_word("$\"y\""), "\"y\"");
        assert_eq!(normalize_raw_word("\"$'x'\""), "\"$'x'\"");
        assert_eq!(normalize_raw_word("'$\"z\"'"), "'$\"z\"'");
    }

    #[test]
    fn empty_arith_for_clause_reads_one() {
        assert_eq!(arith_for_part("   "), "1");
        assert_eq!(arith_for_part(" i < 3 "), "i < 3 ");
    }

    #[test]
    fn top_level_words_do_not_keep_source_text() {
        // Only function bodies pay for recorded source text.
        let script = Parser::new("echo 'a'").parse().expect("parses");
        let Some(Command::Simple(sc)) = script.commands.first() else {
            panic!("expected a simple command");
        };
        assert!(sc.args[0].raw.is_none());
    }
}
