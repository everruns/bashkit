//! awk parser: recursive descent over the lazy lexer.
//!
//! Decisions:
//! - Precedence follows POSIX (lowest first): assignment, `?:`, `||`, `&&`,
//!   `in`, `~ !~`, relational, `cmd | getline`, concatenation, additive,
//!   multiplicative, unary `! + -`, `^` (right associative), `++ --`, `$`,
//!   grouping.
//! - Inside `print`/`printf` arguments an unparenthesized `>` is output
//!   redirection, not a comparison.
//! - THREAT[TM-DOS-027]: syntactic nesting (parentheses, unary operators,
//!   blocks) is capped at `AWK_MAX_PARSER_DEPTH`; operator chains
//!   (`a+b+c...`, long concatenations) also deepen the tree, so they count
//!   against a larger total-depth cap. The evaluator recurses over this
//!   tree, so the caps bound its stack use.

use std::collections::HashMap;

use super::ast::*;
use super::lexer::{LexError, Lexer, Tok};
use crate::builtins::limits::{
    AWK_MAX_MULTI_SUBSCRIPTS as MAX_SUBSCRIPTS, AWK_MAX_PARSER_DEPTH as MAX_DEPTH,
};

/// Total tree depth cap, operator chains included.
const MAX_TREE_DEPTH: usize = 1000;

#[derive(Debug)]
pub(super) struct ParseError {
    pub(super) msg: String,
    pub(super) pos: usize,
    pub(super) kind: ParseErrorKind,
}

#[derive(Debug, PartialEq)]
pub(super) enum ParseErrorKind {
    /// gawk "syntax error" style, printed with the source line and a caret.
    Syntax,
    /// A gawk `error:` without source echo (constant division by zero).
    Error,
    /// A gawk `fatal:` found while parsing (undefined function).
    Fatal,
    /// A bashkit resource limit.
    Limit,
}

type PResult<T> = Result<T, ParseError>;

impl From<LexError> for ParseError {
    fn from(e: LexError) -> Self {
        ParseError {
            msg: e.msg,
            pos: e.pos,
            kind: ParseErrorKind::Syntax,
        }
    }
}

pub(super) struct Parser<'a> {
    lex: Lexer<'a>,
    src: &'a str,
    tok: Tok,
    tok_pos: usize,
    prog: Program,
    global_index: HashMap<String, u32>,
    func_index: HashMap<String, u32>,
    /// Parameters of the function being parsed.
    locals: Option<HashMap<String, u32>>,
    rec_depth: usize,
    tree_depth: usize,
    /// Parsing print arguments: `>` is redirection.
    in_print: bool,
    /// Parenthesis nesting inside print arguments.
    paren_level: usize,
    /// Inside a loop / switch (for break/continue checks).
    loop_depth: usize,
    /// Source labels for statement locations.
    sources: &'a [Source],
    loc_index: HashMap<(usize, u32), u32>,
    /// Calls seen before their function's definition: (func id, offset).
    pending_calls: Vec<(u32, usize)>,
}

#[derive(Clone)]
struct Saved {
    lex_pos: usize,
    tok: Tok,
    tok_pos: usize,
}

impl<'a> Parser<'a> {
    pub(super) fn new(src: &'a str, sources: &'a [Source]) -> Self {
        let mut prog = Program::default();
        let mut global_index = HashMap::new();
        for (i, name) in special::NAMES.iter().enumerate() {
            prog.globals.push((*name).to_string());
            global_index.insert((*name).to_string(), i as u32);
        }
        Parser {
            lex: Lexer::new(src),
            src,
            tok: Tok::Eof,
            tok_pos: 0,
            prog,
            global_index,
            func_index: HashMap::new(),
            locals: None,
            rec_depth: 0,
            tree_depth: 0,
            in_print: false,
            paren_level: 0,
            loop_depth: 0,
            sources,
            loc_index: HashMap::new(),
            pending_calls: Vec::new(),
        }
    }

    // ----- token plumbing -----

    fn advance(&mut self) -> PResult<()> {
        let (t, p) = self.lex.next()?;
        self.tok = t;
        self.tok_pos = p;
        Ok(())
    }

    fn save(&self) -> Saved {
        Saved {
            lex_pos: self.lex.pos(),
            tok: self.tok.clone(),
            tok_pos: self.tok_pos,
        }
    }

    fn restore(&mut self, s: Saved) {
        self.lex.reset(s.lex_pos);
        self.tok = s.tok;
        self.tok_pos = s.tok_pos;
    }

    fn err<T>(&self, msg: &str) -> PResult<T> {
        Err(ParseError {
            msg: msg.to_string(),
            pos: self.tok_pos,
            kind: ParseErrorKind::Syntax,
        })
    }

    fn syntax<T>(&self) -> PResult<T> {
        if matches!(self.tok, Tok::Eof | Tok::Newline) {
            self.err("unexpected newline or end of string")
        } else {
            self.err("syntax error")
        }
    }

    fn expect(&mut self, t: Tok) -> PResult<()> {
        if self.tok == t {
            self.advance()
        } else {
            self.syntax()
        }
    }

    fn opt_nls(&mut self) -> PResult<()> {
        while self.tok == Tok::Newline {
            self.advance()?;
        }
        Ok(())
    }

    fn enter(&mut self) -> PResult<(usize, usize)> {
        let saved = (self.rec_depth, self.tree_depth);
        self.rec_depth += 1;
        self.tree_depth += 1;
        if self.rec_depth > MAX_DEPTH || self.tree_depth > MAX_TREE_DEPTH {
            return Err(ParseError {
                msg: format!(
                    "expression nesting too deep ({} levels, max {MAX_DEPTH})",
                    self.rec_depth.max(self.tree_depth)
                ),
                pos: self.tok_pos,
                kind: ParseErrorKind::Limit,
            });
        }
        Ok(saved)
    }

    fn leave(&mut self, saved: (usize, usize)) {
        self.rec_depth = saved.0;
        self.tree_depth = saved.1;
    }

    /// One more link in an operator chain.
    fn chain(&mut self) -> PResult<()> {
        self.tree_depth += 1;
        if self.tree_depth > MAX_TREE_DEPTH {
            return Err(ParseError {
                msg: format!("expression nesting too deep (max {MAX_TREE_DEPTH} operators)"),
                pos: self.tok_pos,
                kind: ParseErrorKind::Limit,
            });
        }
        Ok(())
    }

    fn loc(&mut self, pos: usize) -> u32 {
        let si = self
            .sources
            .iter()
            .rposition(|s| s.start <= pos)
            .unwrap_or(0);
        let start = self.sources.get(si).map_or(0, |s| s.start);
        let line = self.src[start..pos.min(self.src.len())]
            .bytes()
            .filter(|&b| b == b'\n')
            .count() as u32
            + 1;
        if let Some(&i) = self.loc_index.get(&(si, line)) {
            return i;
        }
        let name = self
            .sources
            .get(si)
            .map_or_else(|| "cmd. line".to_string(), |s| s.name.clone());
        let i = self.prog.locs.len() as u32;
        self.prog.locs.push((name, line));
        self.loc_index.insert((si, line), i);
        i
    }

    fn global(&mut self, name: &str) -> u32 {
        if let Some(&i) = self.global_index.get(name) {
            return i;
        }
        let i = self.prog.globals.len() as u32;
        self.prog.globals.push(name.to_string());
        self.global_index.insert(name.to_string(), i);
        i
    }

    fn var(&mut self, name: &str) -> VarRef {
        if let Some(l) = &self.locals
            && let Some(&i) = l.get(name)
        {
            return VarRef::Local(i);
        }
        VarRef::Global(self.global(name))
    }

    fn func_id(&mut self, name: &str) -> u32 {
        if let Some(&i) = self.func_index.get(name) {
            return i;
        }
        let i = self.prog.funcs.len() as u32;
        self.prog.funcs.push(Func {
            name: name.to_string(),
            ..Func::default()
        });
        self.func_index.insert(name.to_string(), i);
        i
    }

    fn regex_id(&mut self, re: String) -> u32 {
        let i = self.prog.regexes.len() as u32;
        self.prog.regexes.push(re);
        i
    }

    // ----- program -----

    pub(super) fn parse_program(mut self) -> PResult<Program> {
        self.advance()?;
        loop {
            while matches!(self.tok, Tok::Newline | Tok::Semi) {
                self.advance()?;
            }
            if self.tok == Tok::Eof {
                break;
            }
            self.parse_item()?;
        }
        for (f, pos) in std::mem::take(&mut self.pending_calls) {
            if !self.prog.funcs[f as usize].defined {
                return Err(ParseError {
                    msg: format!(
                        "function `{}' not defined",
                        self.prog.funcs[f as usize].name
                    ),
                    pos,
                    kind: ParseErrorKind::Fatal,
                });
            }
        }
        analyze_array_params(&mut self.prog);
        Ok(self.prog)
    }

    fn parse_item(&mut self) -> PResult<()> {
        match self.tok.clone() {
            Tok::Function => self.parse_function(),
            Tok::Begin | Tok::End | Tok::BeginFile | Tok::EndFile => {
                let which = self.tok.clone();
                self.advance()?;
                self.opt_nls()?;
                if self.tok != Tok::LBrace {
                    return self.syntax();
                }
                let block = self.parse_block()?;
                match which {
                    Tok::Begin => self.prog.begin.push(block),
                    Tok::End => self.prog.end.push(block),
                    Tok::BeginFile => self.prog.beginfile.push(block),
                    _ => self.prog.endfile.push(block),
                }
                Ok(())
            }
            Tok::LBrace => {
                let body = self.parse_block()?;
                self.prog.rules.push(Rule {
                    pattern: Pattern::All,
                    body: Some(body),
                });
                Ok(())
            }
            _ => {
                let first = self.parse_expr()?;
                let pattern = if self.tok == Tok::Comma {
                    self.advance()?;
                    self.opt_nls()?;
                    let second = self.parse_expr()?;
                    let id = self.prog.ranges;
                    self.prog.ranges += 1;
                    Pattern::Range(first, second, id)
                } else {
                    Pattern::Expr(first)
                };
                let body = if self.tok == Tok::LBrace {
                    Some(self.parse_block()?)
                } else {
                    match self.tok {
                        Tok::Newline | Tok::Semi | Tok::Eof => {}
                        _ => return self.syntax(),
                    }
                    None
                };
                self.prog.rules.push(Rule { pattern, body });
                Ok(())
            }
        }
    }

    fn parse_function(&mut self) -> PResult<()> {
        self.advance()?;
        let name = match self.tok.clone() {
            Tok::Name(n) | Tok::FuncName(n) => n,
            _ => return self.syntax(),
        };
        let fpos = self.tok_pos;
        self.advance()?;
        if self.global_index.contains_key(&name) && special::NAMES.contains(&name.as_str()) {
            return Err(ParseError {
                msg: format!("function name `{name}' previously defined"),
                pos: fpos,
                kind: ParseErrorKind::Syntax,
            });
        }
        self.expect(Tok::LParen)?;
        let mut params = Vec::new();
        let mut map = HashMap::new();
        self.opt_nls()?;
        while let Tok::Name(p) = self.tok.clone() {
            if map.contains_key(&p) {
                return self.err(&format!(
                    "function `{name}': parameter #{} `{p}' duplicates parameter #{}",
                    params.len() + 1,
                    map[&p] + 1
                ));
            }
            map.insert(p.clone(), params.len() as u32);
            params.push(p);
            self.advance()?;
            self.opt_nls()?;
            if self.tok == Tok::Comma {
                self.advance()?;
                self.opt_nls()?;
            } else {
                break;
            }
        }
        self.expect(Tok::RParen)?;
        self.opt_nls()?;
        let id = self.func_id(&name);
        if self.prog.funcs[id as usize].defined {
            return Err(ParseError {
                msg: format!("function `{name}' previously defined"),
                pos: fpos,
                kind: ParseErrorKind::Syntax,
            });
        }
        if self.tok != Tok::LBrace {
            return self.syntax();
        }
        self.locals = Some(map);
        let body = self.parse_block()?;
        self.locals = None;
        let n = params.len();
        let f = &mut self.prog.funcs[id as usize];
        f.params = params;
        f.body = body;
        f.array_params = vec![false; n];
        f.defined = true;
        Ok(())
    }

    // ----- statements -----

    fn parse_block(&mut self) -> PResult<Block> {
        let saved = self.enter()?;
        self.expect(Tok::LBrace)?;
        let mut stmts = Vec::new();
        loop {
            while matches!(self.tok, Tok::Newline | Tok::Semi) {
                self.advance()?;
            }
            if self.tok == Tok::RBrace {
                self.advance()?;
                break;
            }
            if self.tok == Tok::Eof {
                return self.syntax();
            }
            stmts.push(self.parse_stmt()?);
        }
        self.leave(saved);
        Ok(stmts)
    }

    /// Statement terminator: `;`, newline, or lookahead `}` / EOF.
    fn end_simple(&mut self) -> PResult<()> {
        match self.tok {
            Tok::Semi | Tok::Newline => self.advance(),
            Tok::RBrace | Tok::Eof => Ok(()),
            _ => self.syntax(),
        }
    }

    /// Body of if/while/for: a statement, possibly after newlines; a bare
    /// `;` is an empty body.
    fn parse_body(&mut self) -> PResult<Block> {
        self.opt_nls()?;
        if self.tok == Tok::Semi {
            self.advance()?;
            return Ok(Vec::new());
        }
        let s = self.parse_stmt()?;
        Ok(match s.kind {
            StmtKind::Block(b) => b,
            _ => vec![s],
        })
    }

    fn parse_stmt(&mut self) -> PResult<Stmt> {
        let saved = self.enter()?;
        let pos = self.tok_pos;
        let loc = self.loc(pos);
        let kind = match self.tok.clone() {
            Tok::LBrace => StmtKind::Block(self.parse_block()?),
            Tok::If => {
                self.advance()?;
                self.expect(Tok::LParen)?;
                let cond = self.parse_expr()?;
                self.expect(Tok::RParen)?;
                let then = self.parse_body()?;
                // Look past terminators for `else`.
                let s = self.save();
                while matches!(self.tok, Tok::Newline | Tok::Semi) {
                    self.advance()?;
                }
                let els = if self.tok == Tok::Else {
                    self.advance()?;
                    Some(self.parse_body()?)
                } else {
                    self.restore(s);
                    None
                };
                StmtKind::If(cond, then, els)
            }
            Tok::While => {
                self.advance()?;
                self.expect(Tok::LParen)?;
                let cond = self.parse_expr()?;
                self.expect(Tok::RParen)?;
                if self.tok == Tok::Semi {
                    self.advance()?;
                    StmtKind::While(cond, Vec::new())
                } else {
                    self.loop_depth += 1;
                    let body = self.parse_body()?;
                    self.loop_depth -= 1;
                    StmtKind::While(cond, body)
                }
            }
            Tok::Do => {
                self.advance()?;
                self.loop_depth += 1;
                let body = self.parse_body()?;
                self.loop_depth -= 1;
                while matches!(self.tok, Tok::Newline | Tok::Semi) {
                    self.advance()?;
                }
                self.expect(Tok::While)?;
                self.expect(Tok::LParen)?;
                let cond = self.parse_expr()?;
                self.expect(Tok::RParen)?;
                self.end_simple()?;
                StmtKind::Do(body, cond)
            }
            Tok::For => self.parse_for()?,
            Tok::Semi => {
                self.advance()?;
                StmtKind::Block(Vec::new())
            }
            Tok::Switch => self.parse_switch()?,
            _ => {
                let k = self.parse_simple_or_jump()?;
                self.end_simple()?;
                k
            }
        };
        self.leave(saved);
        Ok(Stmt { kind, loc })
    }

    fn parse_for(&mut self) -> PResult<StmtKind> {
        self.advance()?;
        self.expect(Tok::LParen)?;
        // for (k in a)
        if let Tok::Name(k) = self.tok.clone() {
            let s = self.save();
            self.advance()?;
            if self.tok == Tok::In {
                self.advance()?;
                if let Tok::Name(a) = self.tok.clone() {
                    self.advance()?;
                    if self.tok == Tok::RParen {
                        self.advance()?;
                        let key = Expr::Var(self.var(&k));
                        let arr = Expr::Var(self.var(&a));
                        self.loop_depth += 1;
                        let body = self.parse_body()?;
                        self.loop_depth -= 1;
                        return Ok(StmtKind::ForIn(key, arr, body));
                    }
                }
            }
            self.restore(s);
        }
        let init = if self.tok == Tok::Semi {
            None
        } else {
            let loc = self.loc(self.tok_pos);
            Some(Box::new(Stmt {
                kind: self.parse_simple_or_jump()?,
                loc,
            }))
        };
        self.expect(Tok::Semi)?;
        self.opt_nls()?;
        let cond = if self.tok == Tok::Semi {
            None
        } else {
            Some(self.parse_expr()?)
        };
        self.expect(Tok::Semi)?;
        self.opt_nls()?;
        let incr = if self.tok == Tok::RParen {
            None
        } else {
            let loc = self.loc(self.tok_pos);
            Some(Box::new(Stmt {
                kind: self.parse_simple_or_jump()?,
                loc,
            }))
        };
        self.expect(Tok::RParen)?;
        if self.tok == Tok::Semi {
            self.advance()?;
            return Ok(StmtKind::For(init, cond, incr, Vec::new()));
        }
        self.loop_depth += 1;
        let body = self.parse_body()?;
        self.loop_depth -= 1;
        Ok(StmtKind::For(init, cond, incr, body))
    }

    fn parse_switch(&mut self) -> PResult<StmtKind> {
        self.advance()?;
        self.expect(Tok::LParen)?;
        let subject = self.parse_expr()?;
        self.expect(Tok::RParen)?;
        self.opt_nls()?;
        self.expect(Tok::LBrace)?;
        let mut cases = Vec::new();
        self.loop_depth += 1;
        loop {
            while matches!(self.tok, Tok::Newline | Tok::Semi) {
                self.advance()?;
            }
            let label = match self.tok.clone() {
                Tok::RBrace => {
                    self.advance()?;
                    break;
                }
                Tok::Case => {
                    self.advance()?;
                    if matches!(self.tok, Tok::Slash | Tok::DivAssign) {
                        let Tok::Regex(re) = self.lex.read_regex(self.tok_pos)? else {
                            return self.syntax();
                        };
                        self.advance()?;
                        CaseLabel::Regex(self.regex_id(re))
                    } else {
                        let neg = if self.tok == Tok::Minus {
                            self.advance()?;
                            true
                        } else {
                            false
                        };
                        let v = match self.tok.clone() {
                            Tok::Num(n) => Expr::Num(if neg { -n } else { n }),
                            Tok::Str(s) if !neg => Expr::Str(s),
                            _ => return self.syntax(),
                        };
                        self.advance()?;
                        CaseLabel::Value(v)
                    }
                }
                Tok::Default => {
                    self.advance()?;
                    CaseLabel::Default
                }
                _ => return self.syntax(),
            };
            self.expect(Tok::Colon)?;
            let mut body = Vec::new();
            loop {
                while matches!(self.tok, Tok::Newline | Tok::Semi) {
                    self.advance()?;
                }
                if matches!(self.tok, Tok::Case | Tok::Default | Tok::RBrace) {
                    break;
                }
                if self.tok == Tok::Eof {
                    return self.syntax();
                }
                body.push(self.parse_stmt()?);
            }
            cases.push((label, body));
        }
        self.loop_depth -= 1;
        Ok(StmtKind::Switch(subject, cases))
    }

    fn parse_simple_or_jump(&mut self) -> PResult<StmtKind> {
        Ok(match self.tok.clone() {
            Tok::Print | Tok::Printf => self.parse_print()?,
            Tok::Next => {
                self.advance()?;
                StmtKind::Next
            }
            Tok::NextFile => {
                self.advance()?;
                StmtKind::NextFile
            }
            Tok::Break => {
                self.advance()?;
                StmtKind::Break
            }
            Tok::Continue => {
                self.advance()?;
                StmtKind::Continue
            }
            Tok::Exit => {
                self.advance()?;
                if self.at_simple_end() {
                    StmtKind::Exit(None)
                } else {
                    StmtKind::Exit(Some(self.parse_expr()?))
                }
            }
            Tok::Return => {
                if self.locals.is_none() {
                    return self.err("`return' used outside function context");
                }
                self.advance()?;
                if self.at_simple_end() {
                    StmtKind::Return(None)
                } else {
                    StmtKind::Return(Some(self.parse_expr()?))
                }
            }
            Tok::Delete => {
                self.advance()?;
                let paren = self.tok == Tok::LParen;
                if paren {
                    self.advance()?;
                }
                let Tok::Name(n) = self.tok.clone() else {
                    return self.syntax();
                };
                self.advance()?;
                let v = self.var(&n);
                let target = if self.tok == Tok::LBracket {
                    let subs = self.parse_subscripts()?;
                    Expr::Elem(v, subs)
                } else {
                    Expr::Var(v)
                };
                if paren {
                    self.expect(Tok::RParen)?;
                }
                StmtKind::Delete(target)
            }
            _ => StmtKind::Expr(self.parse_expr()?),
        })
    }

    fn at_simple_end(&self) -> bool {
        matches!(self.tok, Tok::Semi | Tok::Newline | Tok::RBrace | Tok::Eof)
    }

    fn parse_print(&mut self) -> PResult<StmtKind> {
        let is_printf = self.tok == Tok::Printf;
        self.advance()?;
        let mut args = Vec::new();
        let old = (self.in_print, self.paren_level);
        self.in_print = true;
        self.paren_level = 0;
        if !self.at_print_end() {
            // `print (a, b) > f`: a parenthesized list.
            let mut done = false;
            if self.tok == Tok::LParen {
                let s = self.save();
                self.advance()?;
                self.paren_level += 1;
                let mut list = vec![self.parse_expr()?];
                while self.tok == Tok::Comma {
                    self.advance()?;
                    self.opt_nls()?;
                    list.push(self.parse_expr()?);
                }
                self.paren_level -= 1;
                if self.tok == Tok::RParen {
                    self.advance()?;
                    if self.at_print_end() {
                        args = list;
                        done = true;
                    }
                }
                if !done {
                    self.restore(s);
                    self.paren_level = 0;
                }
            }
            if !done {
                args.push(self.parse_expr()?);
                while self.tok == Tok::Comma {
                    self.advance()?;
                    self.opt_nls()?;
                    args.push(self.parse_expr()?);
                }
            }
        }
        let redirect = match self.tok {
            Tok::Gt | Tok::Append | Tok::Pipe => {
                let kind = self.tok.clone();
                self.advance()?;
                // The target is a concatenation-level expression without
                // comparisons (`print > "a" "b"` writes to file "ab").
                let target = self.parse_concat()?;
                Some(match kind {
                    Tok::Gt => Redirect::File(target),
                    Tok::Append => Redirect::Append(target),
                    _ => Redirect::Pipe(target),
                })
            }
            Tok::PipeAmp => return self.err("coprocesses (|&) are not supported"),
            _ => None,
        };
        (self.in_print, self.paren_level) = old;
        if is_printf {
            if args.is_empty() {
                return self.syntax();
            }
            Ok(StmtKind::Printf(args, redirect))
        } else {
            Ok(StmtKind::Print(args, redirect))
        }
    }

    fn at_print_end(&self) -> bool {
        matches!(
            self.tok,
            Tok::Semi
                | Tok::Newline
                | Tok::RBrace
                | Tok::Eof
                | Tok::Gt
                | Tok::Append
                | Tok::Pipe
                | Tok::PipeAmp
        )
    }

    // ----- expressions -----

    pub(super) fn parse_expr(&mut self) -> PResult<Expr> {
        let saved = self.enter()?;
        let e = self.parse_ternary()?;
        let e = if e.is_lvalue() {
            let op = match self.tok {
                Tok::Assign => Some(None),
                Tok::AddAssign => Some(Some(BinOp::Add)),
                Tok::SubAssign => Some(Some(BinOp::Sub)),
                Tok::MulAssign => Some(Some(BinOp::Mul)),
                Tok::DivAssign => Some(Some(BinOp::Div)),
                Tok::ModAssign => Some(Some(BinOp::Mod)),
                Tok::PowAssign => Some(Some(BinOp::Pow)),
                _ => None,
            };
            match op {
                Some(op) => {
                    self.advance()?;
                    self.opt_nls()?;
                    let rhs = self.parse_expr()?;
                    Expr::Assign(Box::new(e), op, Box::new(rhs))
                }
                None => e,
            }
        } else {
            e
        };
        self.leave(saved);
        Ok(e)
    }

    fn parse_ternary(&mut self) -> PResult<Expr> {
        let cond = self.parse_or()?;
        if self.tok != Tok::Question {
            return Ok(cond);
        }
        self.advance()?;
        self.opt_nls()?;
        let a = self.parse_expr()?;
        self.opt_nls()?;
        self.expect(Tok::Colon)?;
        self.opt_nls()?;
        let b = self.parse_expr()?;
        Ok(Expr::Cond(Box::new(cond), Box::new(a), Box::new(b)))
    }

    fn parse_or(&mut self) -> PResult<Expr> {
        let mut l = self.parse_and()?;
        while self.tok == Tok::Or {
            self.chain()?;
            self.advance()?;
            self.opt_nls()?;
            let r = self.parse_and()?;
            l = Expr::Or(Box::new(l), Box::new(r));
        }
        Ok(l)
    }

    fn parse_and(&mut self) -> PResult<Expr> {
        let mut l = self.parse_in()?;
        while self.tok == Tok::And {
            self.chain()?;
            self.advance()?;
            self.opt_nls()?;
            let r = self.parse_in()?;
            l = Expr::And(Box::new(l), Box::new(r));
        }
        Ok(l)
    }

    fn parse_in(&mut self) -> PResult<Expr> {
        let mut l = self.parse_match()?;
        while self.tok == Tok::In {
            self.chain()?;
            self.advance()?;
            let arr = self.parse_array_name()?;
            let subs = match l {
                Expr::List(v) => v,
                Expr::Group(g) => vec![*g],
                other => vec![other],
            };
            l = Expr::In(subs, Box::new(arr));
        }
        Ok(l)
    }

    /// Array operand of `in`: a name, or `name[i]` (a subarray).
    fn parse_array_name(&mut self) -> PResult<Expr> {
        let Tok::Name(n) = self.tok.clone() else {
            return self.syntax();
        };
        self.advance()?;
        let v = self.var(&n);
        if self.tok == Tok::LBracket {
            let subs = self.parse_subscripts()?;
            return Ok(Expr::Elem(v, subs));
        }
        Ok(Expr::Var(v))
    }

    fn parse_match(&mut self) -> PResult<Expr> {
        let mut l = self.parse_comparison()?;
        while matches!(self.tok, Tok::Tilde | Tok::NoMatch) {
            self.chain()?;
            let neg = self.tok == Tok::NoMatch;
            self.advance()?;
            let r = self.parse_comparison()?;
            l = Expr::Match(neg, Box::new(l), Box::new(r));
        }
        Ok(l)
    }

    fn parse_comparison(&mut self) -> PResult<Expr> {
        let l = self.parse_pipe_getline()?;
        let op = match self.tok {
            Tok::Lt => CmpOp::Lt,
            Tok::Le => CmpOp::Le,
            Tok::Ne => CmpOp::Ne,
            Tok::Eq => CmpOp::Eq,
            Tok::Ge => CmpOp::Ge,
            Tok::Gt if !(self.in_print && self.paren_level == 0) => CmpOp::Gt,
            _ => return Ok(l),
        };
        self.advance()?;
        let r = self.parse_pipe_getline()?;
        Ok(Expr::Cmp(op, Box::new(l), Box::new(r)))
    }

    /// `cmd | getline [var]` (left associative: `"a" | getline | getline`
    /// is not meaningful, but `cmd | getline > 0` is common).
    fn parse_pipe_getline(&mut self) -> PResult<Expr> {
        let mut l = self.parse_concat()?;
        while self.tok == Tok::Pipe {
            let s = self.save();
            self.advance()?;
            if self.tok != Tok::Getline {
                // Output pipe of a print statement.
                self.restore(s);
                break;
            }
            self.chain()?;
            self.advance()?;
            let var = self.parse_getline_var()?;
            l = Expr::Getline(GetlineSrc::Cmd(Box::new(l)), var);
        }
        if self.tok == Tok::PipeAmp {
            return self.err("coprocesses (|&) are not supported");
        }
        Ok(l)
    }

    fn parse_getline_var(&mut self) -> PResult<Option<Box<Expr>>> {
        Ok(match self.tok.clone() {
            Tok::Name(_) | Tok::Dollar => Some(Box::new(self.parse_incdec_operand()?)),
            _ => None,
        })
    }

    /// Can the current token start a concatenated operand?
    fn starts_concat_operand(&self) -> bool {
        matches!(
            self.tok,
            Tok::Num(_)
                | Tok::Str(_)
                | Tok::Name(_)
                | Tok::FuncName(_)
                | Tok::Builtin(_)
                | Tok::Dollar
                | Tok::Not
                | Tok::LParen
                | Tok::Minus
                | Tok::Plus
                | Tok::Incr
                | Tok::Decr
                | Tok::At
        )
    }

    /// Concatenation (also the form of a redirection target).
    fn parse_concat(&mut self) -> PResult<Expr> {
        let mut l = self.parse_additive()?;
        loop {
            // `-`/`+` after an operand are binary (handled in additive), so
            // they never start a concatenated operand here.
            let starts = self.starts_concat_operand()
                && !matches!(self.tok, Tok::Minus | Tok::Plus | Tok::Not);
            if !starts {
                break;
            }
            self.chain()?;
            let r = self.parse_additive()?;
            l = Expr::Concat(Box::new(l), Box::new(r));
        }
        Ok(l)
    }

    fn parse_additive(&mut self) -> PResult<Expr> {
        let mut l = self.parse_mul()?;
        loop {
            let op = match self.tok {
                Tok::Plus => BinOp::Add,
                Tok::Minus => BinOp::Sub,
                _ => break,
            };
            self.chain()?;
            self.advance()?;
            let r = self.parse_mul()?;
            l = Expr::Binary(op, Box::new(l), Box::new(r));
        }
        Ok(l)
    }

    fn parse_mul(&mut self) -> PResult<Expr> {
        let mut l = self.parse_unary()?;
        loop {
            let op = match self.tok {
                Tok::Star => BinOp::Mul,
                Tok::Slash => BinOp::Div,
                Tok::Percent => BinOp::Mod,
                _ => break,
            };
            let pos = self.tok_pos;
            self.chain()?;
            self.advance()?;
            let r = self.parse_unary()?;
            if matches!(op, BinOp::Div | BinOp::Mod)
                && matches!(r, Expr::Num(z) if z == 0.0)
                && matches!(l, Expr::Num(_))
            {
                return Err(ParseError {
                    msg: "division by zero attempted".to_string(),
                    pos,
                    kind: ParseErrorKind::Error,
                });
            }
            l = Expr::Binary(op, Box::new(l), Box::new(r));
        }
        Ok(l)
    }

    fn parse_unary(&mut self) -> PResult<Expr> {
        match self.tok {
            Tok::Not => {
                let saved = self.enter()?;
                self.advance()?;
                let e = self.parse_unary()?;
                self.leave(saved);
                Ok(Expr::Not(Box::new(e)))
            }
            Tok::Minus => {
                let saved = self.enter()?;
                self.advance()?;
                let e = self.parse_unary()?;
                self.leave(saved);
                Ok(match e {
                    Expr::Num(n) => Expr::Num(-n),
                    e => Expr::Neg(Box::new(e)),
                })
            }
            Tok::Plus => {
                let saved = self.enter()?;
                self.advance()?;
                let e = self.parse_unary()?;
                self.leave(saved);
                Ok(Expr::Plus(Box::new(e)))
            }
            _ => self.parse_pow(),
        }
    }

    fn parse_pow(&mut self) -> PResult<Expr> {
        let base = self.parse_postfix()?;
        if self.tok == Tok::Caret {
            let saved = self.enter()?;
            self.advance()?;
            // Right associative; the exponent may carry a unary sign.
            let exp = match self.tok {
                Tok::Minus | Tok::Plus | Tok::Not => self.parse_unary()?,
                _ => self.parse_pow()?,
            };
            self.leave(saved);
            return Ok(Expr::Binary(BinOp::Pow, Box::new(base), Box::new(exp)));
        }
        Ok(base)
    }

    fn parse_postfix(&mut self) -> PResult<Expr> {
        match self.tok {
            Tok::Incr | Tok::Decr => {
                let delta = if self.tok == Tok::Incr { 1.0 } else { -1.0 };
                self.advance()?;
                let lv = self.parse_incdec_operand()?;
                return Ok(Expr::IncDec(Box::new(lv), delta, true));
            }
            _ => {}
        }
        let e = self.parse_primary()?;
        if e.is_lvalue() && matches!(self.tok, Tok::Incr | Tok::Decr) {
            let delta = if self.tok == Tok::Incr { 1.0 } else { -1.0 };
            self.advance()?;
            return Ok(Expr::IncDec(Box::new(e), delta, false));
        }
        Ok(e)
    }

    /// Operand of prefix `++`/`--` and of `getline var`: an lvalue.
    fn parse_incdec_operand(&mut self) -> PResult<Expr> {
        match self.tok.clone() {
            Tok::Dollar => {
                self.advance()?;
                let saved = self.enter()?;
                let e = self.parse_field_operand()?;
                self.leave(saved);
                Ok(Expr::Field(Box::new(e)))
            }
            Tok::Name(n) => {
                self.advance()?;
                let v = self.var(&n);
                if self.tok == Tok::LBracket {
                    let subs = self.parse_subscripts()?;
                    Ok(Expr::Elem(v, subs))
                } else {
                    Ok(Expr::Var(v))
                }
            }
            _ => self.syntax(),
        }
    }

    /// Operand of `$`: a high-precedence expression (`$NF-1` is `($NF)-1`,
    /// `$i++` is `($i)++`, `$++i` increments `i`).
    fn parse_field_operand(&mut self) -> PResult<Expr> {
        match self.tok {
            Tok::Incr | Tok::Decr => {
                let delta = if self.tok == Tok::Incr { 1.0 } else { -1.0 };
                self.advance()?;
                let lv = self.parse_incdec_operand()?;
                Ok(Expr::IncDec(Box::new(lv), delta, true))
            }
            Tok::Minus => {
                self.advance()?;
                let e = self.parse_field_operand()?;
                Ok(Expr::Neg(Box::new(e)))
            }
            Tok::Plus => {
                self.advance()?;
                let e = self.parse_field_operand()?;
                Ok(Expr::Plus(Box::new(e)))
            }
            Tok::Not => {
                self.advance()?;
                let e = self.parse_field_operand()?;
                Ok(Expr::Not(Box::new(e)))
            }
            _ => self.parse_primary(),
        }
    }

    fn parse_subscripts(&mut self) -> PResult<Vec<Vec<Expr>>> {
        let mut groups = Vec::new();
        while self.tok == Tok::LBracket {
            self.advance()?;
            let old = (self.in_print, self.paren_level);
            self.in_print = false;
            let mut g = vec![self.parse_expr()?];
            while self.tok == Tok::Comma {
                if g.len() >= MAX_SUBSCRIPTS {
                    return Err(ParseError {
                        msg: format!("too many array subscripts (max {MAX_SUBSCRIPTS})"),
                        pos: self.tok_pos,
                        kind: ParseErrorKind::Limit,
                    });
                }
                self.advance()?;
                self.opt_nls()?;
                g.push(self.parse_expr()?);
            }
            (self.in_print, self.paren_level) = old;
            self.expect(Tok::RBracket)?;
            groups.push(g);
            if groups.len() > MAX_SUBSCRIPTS {
                return Err(ParseError {
                    msg: format!("too many array subscripts (max {MAX_SUBSCRIPTS})"),
                    pos: self.tok_pos,
                    kind: ParseErrorKind::Limit,
                });
            }
        }
        Ok(groups)
    }

    fn parse_args(&mut self) -> PResult<Vec<Expr>> {
        // Current token is `(`.
        self.advance()?;
        let old = (self.in_print, self.paren_level);
        self.in_print = false;
        self.opt_nls()?;
        let mut args = Vec::new();
        if self.tok != Tok::RParen {
            loop {
                args.push(self.parse_expr()?);
                self.opt_nls()?;
                if self.tok == Tok::Comma {
                    self.advance()?;
                    self.opt_nls()?;
                } else {
                    break;
                }
            }
        }
        (self.in_print, self.paren_level) = old;
        self.expect(Tok::RParen)?;
        Ok(args)
    }

    fn parse_primary(&mut self) -> PResult<Expr> {
        let pos = self.tok_pos;
        match self.tok.clone() {
            Tok::Num(n) => {
                self.advance()?;
                Ok(Expr::Num(n))
            }
            Tok::Str(s) => {
                self.advance()?;
                Ok(Expr::Str(s))
            }
            Tok::Slash | Tok::DivAssign => {
                let Tok::Regex(re) = self.lex.read_regex(self.tok_pos)? else {
                    return self.syntax();
                };
                self.advance()?;
                Ok(Expr::Regex(self.regex_id(re)))
            }
            Tok::Dollar => {
                self.advance()?;
                let saved = self.enter()?;
                let e = self.parse_field_operand()?;
                self.leave(saved);
                Ok(Expr::Field(Box::new(e)))
            }
            Tok::LParen => {
                let saved = self.enter()?;
                self.advance()?;
                self.paren_level += 1;
                let old_print = self.in_print;
                self.in_print = false;
                let first = self.parse_expr()?;
                let mut list = Vec::new();
                if self.tok == Tok::Comma {
                    list.push(first);
                    while self.tok == Tok::Comma {
                        if list.len() >= MAX_SUBSCRIPTS {
                            return Err(ParseError {
                                msg: format!("too many array subscripts (max {MAX_SUBSCRIPTS})"),
                                pos: self.tok_pos,
                                kind: ParseErrorKind::Limit,
                            });
                        }
                        self.advance()?;
                        self.opt_nls()?;
                        list.push(self.parse_expr()?);
                    }
                    self.in_print = old_print;
                    self.paren_level -= 1;
                    self.expect(Tok::RParen)?;
                    self.leave(saved);
                    if self.tok != Tok::In {
                        return self.syntax();
                    }
                    return Ok(Expr::List(list));
                }
                self.in_print = old_print;
                self.paren_level -= 1;
                self.expect(Tok::RParen)?;
                self.leave(saved);
                Ok(Expr::Group(Box::new(first)))
            }
            Tok::Getline => {
                self.advance()?;
                let var = self.parse_getline_var()?;
                if self.tok == Tok::Lt {
                    self.advance()?;
                    // The file operand: a primary (`getline < "a" "b"`
                    // reads file "a").
                    let saved = self.enter()?;
                    let f = self.parse_postfix_no_concat()?;
                    self.leave(saved);
                    return Ok(Expr::Getline(GetlineSrc::File(Box::new(f)), var));
                }
                Ok(Expr::Getline(GetlineSrc::Main, var))
            }
            Tok::Name(n) => {
                self.advance()?;
                let v = self.var(&n);
                if self.tok == Tok::LBracket {
                    let subs = self.parse_subscripts()?;
                    return Ok(Expr::Elem(v, subs));
                }
                Ok(Expr::Var(v))
            }
            Tok::FuncName(n) => {
                self.advance()?;
                let id = self.func_id(&n);
                let args = self.parse_args()?;
                if !self.prog.funcs[id as usize].defined {
                    self.pending_calls.push((id, pos));
                }
                Ok(Expr::Call(id, args))
            }
            Tok::At => {
                self.advance()?;
                let name = match self.tok.clone() {
                    Tok::FuncName(n) | Tok::Name(n) => n,
                    _ => return self.syntax(),
                };
                self.advance()?;
                if self.tok != Tok::LParen {
                    return self.syntax();
                }
                let v = Expr::Var(self.var(&name));
                let args = self.parse_args()?;
                Ok(Expr::Indirect(Box::new(v), args))
            }
            Tok::Builtin(name) => {
                self.advance()?;
                let Some(bi) = Bi::from_name(&name) else {
                    return self.syntax();
                };
                let args = if self.tok == Tok::LParen {
                    self.parse_args()?
                } else if bi == Bi::Length {
                    Vec::new()
                } else {
                    return self.syntax();
                };
                self.check_builtin_arity(bi, args.len(), pos)?;
                Ok(Expr::Builtin(bi, args))
            }
            Tok::Minus | Tok::Plus | Tok::Not => self.parse_unary(),
            Tok::Incr | Tok::Decr => self.parse_postfix(),
            _ => self.syntax(),
        }
    }

    /// File operand of `getline <`: `$x`, a name, a literal or a group.
    fn parse_postfix_no_concat(&mut self) -> PResult<Expr> {
        self.parse_primary()
    }

    fn check_builtin_arity(&self, bi: Bi, n: usize, pos: usize) -> PResult<()> {
        let (min, max) = match bi {
            Bi::Length => (0, 1),
            Bi::Substr => (2, 3),
            Bi::Index => (2, 2),
            Bi::Split => (2, 4),
            Bi::Patsplit => (2, 4),
            Bi::Sub | Bi::Gsub => (2, 3),
            Bi::Gensub => (3, 4),
            Bi::Match => (2, 3),
            Bi::Sprintf => (1, usize::MAX),
            Bi::Sin | Bi::Cos | Bi::Exp | Bi::Log | Bi::Sqrt | Bi::Int => (1, 1),
            Bi::Atan2 => (2, 2),
            Bi::Rand | Bi::Systime => (0, 0),
            Bi::Srand => (0, 1),
            Bi::Tolower | Bi::Toupper | Bi::System | Bi::Strtonum | Bi::Typeof | Bi::Isarray => {
                (1, 1)
            }
            Bi::Close => (1, 2),
            Bi::Fflush => (0, 1),
            Bi::Strftime => (0, 3),
            Bi::Mktime => (1, 2),
            Bi::Asort | Bi::Asorti => (1, 3),
            Bi::And | Bi::Or | Bi::Xor => (2, usize::MAX),
            Bi::Lshift | Bi::Rshift => (2, 2),
            Bi::Compl => (1, 1),
        };
        if n < min || n > max {
            return Err(ParseError {
                msg: format!(
                    "{n} is invalid as number of arguments for {}",
                    super::lexer::BUILTIN_FUNCS
                        .iter()
                        .find(|name| Bi::from_name(name) == Some(bi))
                        .unwrap_or(&"function")
                ),
                pos,
                kind: ParseErrorKind::Syntax,
            });
        }
        Ok(())
    }
}

/// Mark function parameters used as arrays (fixed point over calls).
fn analyze_array_params(prog: &mut Program) {
    loop {
        let mut changed = false;
        for fi in 0..prog.funcs.len() {
            let mut marks = prog.funcs[fi].array_params.clone();
            let body = std::mem::take(&mut prog.funcs[fi].body);
            for s in &body {
                walk_stmt(s, &mut |e| mark_expr(e, &mut marks, &prog.funcs));
                mark_stmt(s, &mut marks);
            }
            prog.funcs[fi].body = body;
            if marks != prog.funcs[fi].array_params {
                prog.funcs[fi].array_params = marks;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
}

fn local_of(e: &Expr) -> Option<usize> {
    match e {
        Expr::Var(VarRef::Local(i)) => Some(*i as usize),
        _ => None,
    }
}

fn set_mark(marks: &mut [bool], e: &Expr) {
    if let Some(i) = local_of(e)
        && let Some(m) = marks.get_mut(i)
    {
        *m = true;
    }
}

fn mark_expr(e: &Expr, marks: &mut [bool], funcs: &[Func]) {
    match e {
        Expr::Elem(VarRef::Local(i), _) => {
            if let Some(m) = marks.get_mut(*i as usize) {
                *m = true;
            }
        }
        Expr::In(_, arr) => set_mark(marks, arr),
        Expr::Builtin(bi, args) => {
            for &p in bi.array_args() {
                if let Some(a) = args.get(p) {
                    set_mark(marks, a);
                }
            }
        }
        Expr::Call(f, args) => {
            if let Some(func) = funcs.get(*f as usize) {
                for (j, a) in args.iter().enumerate() {
                    if func.array_params.get(j).copied().unwrap_or(false) {
                        set_mark(marks, a);
                    }
                }
            }
        }
        _ => {}
    }
}

fn mark_stmt(s: &Stmt, marks: &mut Vec<bool>) {
    match &s.kind {
        StmtKind::ForIn(_, arr, _) => set_mark(marks, arr),
        StmtKind::Delete(target) => set_mark(marks, target),
        _ => {}
    }
    for b in child_blocks(s) {
        for s in b {
            mark_stmt(s, marks);
        }
    }
}

fn child_blocks(s: &Stmt) -> Vec<&Block> {
    match &s.kind {
        StmtKind::If(_, a, b) => {
            let mut v = vec![a];
            if let Some(b) = b {
                v.push(b);
            }
            v
        }
        StmtKind::While(_, b) | StmtKind::Do(b, _) | StmtKind::ForIn(_, _, b) => vec![b],
        StmtKind::Block(b) => vec![b],
        StmtKind::For(_, _, _, b) => vec![b],
        StmtKind::Switch(_, cases) => cases.iter().map(|(_, b)| b).collect(),
        _ => Vec::new(),
    }
}

/// Visit every expression in a statement tree.
pub(super) fn walk_stmt(s: &Stmt, f: &mut dyn FnMut(&Expr)) {
    match &s.kind {
        StmtKind::Expr(e) => walk_expr(e, f),
        StmtKind::Print(args, r) | StmtKind::Printf(args, r) => {
            for a in args {
                walk_expr(a, f);
            }
            if let Some(Redirect::File(e) | Redirect::Append(e) | Redirect::Pipe(e)) = r {
                walk_expr(e, f);
            }
        }
        StmtKind::If(c, a, b) => {
            walk_expr(c, f);
            for s in a {
                walk_stmt(s, f);
            }
            if let Some(b) = b {
                for s in b {
                    walk_stmt(s, f);
                }
            }
        }
        StmtKind::While(c, b) | StmtKind::Do(b, c) => {
            walk_expr(c, f);
            for s in b {
                walk_stmt(s, f);
            }
        }
        StmtKind::For(i, c, n, b) => {
            if let Some(i) = i {
                walk_stmt(i, f);
            }
            if let Some(c) = c {
                walk_expr(c, f);
            }
            if let Some(n) = n {
                walk_stmt(n, f);
            }
            for s in b {
                walk_stmt(s, f);
            }
        }
        StmtKind::ForIn(k, a, b) => {
            walk_expr(k, f);
            walk_expr(a, f);
            for s in b {
                walk_stmt(s, f);
            }
        }
        StmtKind::Block(b) => {
            for s in b {
                walk_stmt(s, f);
            }
        }
        StmtKind::Exit(Some(e)) | StmtKind::Return(Some(e)) | StmtKind::Delete(e) => {
            walk_expr(e, f)
        }
        StmtKind::Switch(e, cases) => {
            walk_expr(e, f);
            for (_, b) in cases {
                for s in b {
                    walk_stmt(s, f);
                }
            }
        }
        _ => {}
    }
}

pub(super) fn walk_expr(e: &Expr, f: &mut dyn FnMut(&Expr)) {
    f(e);
    match e {
        Expr::Elem(_, groups) => {
            for g in groups {
                for x in g {
                    walk_expr(x, f);
                }
            }
        }
        Expr::Field(x) | Expr::Neg(x) | Expr::Plus(x) | Expr::Not(x) | Expr::Group(x) => {
            walk_expr(x, f)
        }
        Expr::IncDec(x, _, _) => walk_expr(x, f),
        Expr::Assign(a, _, b)
        | Expr::Binary(_, a, b)
        | Expr::Cmp(_, a, b)
        | Expr::Match(_, a, b)
        | Expr::And(a, b)
        | Expr::Or(a, b)
        | Expr::Concat(a, b) => {
            walk_expr(a, f);
            walk_expr(b, f);
        }
        Expr::Cond(a, b, c) => {
            walk_expr(a, f);
            walk_expr(b, f);
            walk_expr(c, f);
        }
        Expr::In(subs, arr) => {
            for s in subs {
                walk_expr(s, f);
            }
            walk_expr(arr, f);
        }
        Expr::Call(_, args) | Expr::Builtin(_, args) | Expr::List(args) => {
            for a in args {
                walk_expr(a, f);
            }
        }
        Expr::Indirect(v, args) => {
            walk_expr(v, f);
            for a in args {
                walk_expr(a, f);
            }
        }
        Expr::Getline(src, var) => {
            if let GetlineSrc::File(x) | GetlineSrc::Cmd(x) = src {
                walk_expr(x, f);
            }
            if let Some(v) = var {
                walk_expr(v, f);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(src: &str) -> PResult<Program> {
        let sources = vec![Source {
            name: "cmd. line".to_string(),
            start: 0,
        }];
        Parser::new(src, &sources).parse_program()
    }

    #[test]
    fn parses_common_programs() {
        for src in [
            "{print $1}",
            "BEGIN { x = 1; y = x ^= 2 } END { print x, y }",
            "!seen[$0]++",
            "NR==1, /end/ { print }",
            "function f(a, b,   c) { c = a + b; return c } { print f($1, $2) }",
            "{ if ($1 > 2) print \"big\"; else print \"small\" }",
            "{ for (k in a) print k, a[k] }",
            "{ for (i = 1; i <= NF; i++) s += $i } END { print s }",
            "BEGIN { while ((\"echo hi\" | getline line) > 0) print line }",
            "BEGIN { print \"x\" > \"/dev/stderr\"; print 1, 2 > \"f\" }",
            "BEGIN { printf(\"%d\\n\", 3) > \"f\" }",
            "{ print (1,2) in a }",
            "BEGIN { if ((1,2) in a) print \"y\" }",
            "BEGIN { a[1][2] = 3; print length(a[1]) }",
            "BEGIN { switch (x) { case 1: print 1; break; case /a/: print 2; default: print 3 } }",
            "BEGIN { getline line < \"file\"; print line }",
            "{ $3 = \"\"; print }",
            "BEGIN { x = -2^2; print x }",
            "BEGIN { print 1 \" \" 2 }",
            "/a/ && /b/",
            "BEGIN { f = \"g\"; print @f(1) } function g(x) { return x }",
            "BEGIN { n = split(\"a b\", arr); print n }",
            "BEGIN { print length }",
            "{ print $NF-1, $(NF-1) }",
            "BEGIN {\n  if (x)\n    print 1\n  else\n    print 2\n}",
            "BEGIN { do x++; while (x < 3); print x }",
        ] {
            if let Err(e) = parse(src) {
                panic!("{src}: {} at {}", e.msg, e.pos);
            }
        }
    }

    #[test]
    fn syntax_errors() {
        let e = parse("{print $1").unwrap_err();
        assert_eq!(e.msg, "unexpected newline or end of string");
        let e = parse("BEGIN { x = = 1 }").unwrap_err();
        assert_eq!(e.msg, "syntax error");
        assert_eq!(e.pos, 12);
        let e = parse("BEGIN { nosuch(1) }").unwrap_err();
        assert_eq!(e.kind, ParseErrorKind::Fatal);
        let e = parse("BEGIN { print 1/0 }").unwrap_err();
        assert_eq!(e.kind, ParseErrorKind::Error);
    }

    #[test]
    fn depth_limits() {
        let deep = format!("{{print {}1{}}}", "(".repeat(200), ")".repeat(200));
        assert_eq!(parse(&deep).unwrap_err().kind, ParseErrorKind::Limit);
        let unary = format!("{{print {}1}}", "- ".repeat(200));
        assert_eq!(parse(&unary).unwrap_err().kind, ParseErrorKind::Limit);
        let chain = format!("{{print 1{}}}", "+1".repeat(5000));
        assert_eq!(parse(&chain).unwrap_err().kind, ParseErrorKind::Limit);
        let ok = format!("{{print 1{}}}", "+1".repeat(200));
        assert!(parse(&ok).is_ok());
    }

    #[test]
    fn array_params_are_detected() {
        let p = parse(
            "function f(a, n) { a[1] = n } function g(x, y) { f(x, 1); return y } BEGIN { g(z) }",
        )
        .unwrap();
        assert_eq!(p.funcs[0].array_params, vec![true, false]);
        assert_eq!(p.funcs[1].array_params, vec![true, false]);
    }
}
