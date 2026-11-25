//! Experiment: compile safe scalar expressions to stack instructions. Unsupported
//! grammar returns None; the ordinary evaluator remains the semantic fallback.
use super::*;

#[derive(Debug)]
enum Op {
    Number(i64),
    Load(String),
    Unary(&'static str),
    Binary(&'static str),
    Store(String),
    Update(String, i64, bool),
    Drop,
}

pub(in crate::interpreter) struct Expression<'s> {
    pub source: std::borrow::Cow<'s, str>,
    ops: Vec<Op>,
}

struct Compiler {
    tokens: Vec<(Tok, usize)>,
    at: usize,
    ops: Vec<Op>,
}

impl Compiler {
    fn peek(&self) -> &Tok {
        &self.tokens[self.at].0
    }
    fn next(&mut self) -> Tok {
        let token = std::mem::replace(&mut self.tokens[self.at].0, Tok::Eof);
        if self.at + 1 < self.tokens.len() {
            self.at += 1;
        }
        token
    }
    fn expression(&mut self, min: u8, depth: usize) -> Option<()> {
        if depth > 32 {
            return None;
        }
        match self.next() {
            Tok::Num(s) => self.ops.push(Op::Number(parse_arith_number(&s).ok()?)),
            Tok::Ident(name, None) => {
                if let Tok::Op(op @ ("++" | "--")) = self.peek() {
                    let delta = if *op == "++" { 1 } else { -1 };
                    self.next();
                    self.ops.push(Op::Update(name, delta, false));
                } else {
                    self.ops.push(Op::Load(name));
                }
            }
            Tok::Op(op @ ("!" | "~" | "-" | "+")) => {
                self.expression(12, depth + 1)?;
                self.ops.push(Op::Unary(op));
            }
            Tok::Op(op @ ("++" | "--")) => {
                let Tok::Ident(name, None) = self.next() else {
                    return None;
                };
                self.ops
                    .push(Op::Update(name, if op == "++" { 1 } else { -1 }, true));
            }
            Tok::LParen => {
                self.expression(0, depth + 1)?;
                if self.next() != Tok::RParen {
                    return None;
                }
            }
            _ => return None,
        }
        while let Tok::Op(op) = self.peek() {
            let op = *op;
            let precedence = match op {
                "|" => 1,
                "^" => 2,
                "&" => 3,
                "==" | "!=" => 4,
                "<" | ">" | "<=" | ">=" => 5,
                "<<" | ">>" => 6,
                "+" | "-" => 7,
                "*" => 8,
                _ => break,
            };
            if precedence < min {
                break;
            }
            self.next();
            self.expression(precedence + 1, depth + 1)?;
            self.ops.push(Op::Binary(op));
        }
        Some(())
    }
    fn assignment(&mut self) -> Option<()> {
        let op = if matches!(self.peek(), Tok::Ident(_, None)) {
            match self.tokens.get(self.at + 1) {
                Some((Tok::Op(op @ ("=" | "+=" | "-=" | "*=")), _)) => Some(*op),
                _ => None,
            }
        } else {
            None
        };
        if let Some(op) = op {
            let Tok::Ident(name, None) = self.next() else {
                unreachable!()
            };
            self.next();
            if op != "=" {
                self.ops.push(Op::Load(name.clone()));
            }
            self.expression(0, 0)?;
            if op != "=" {
                self.ops.push(Op::Binary(match op {
                    "+=" => "+",
                    "-=" => "-",
                    _ => "*",
                }));
            }
            self.ops.push(Op::Store(name));
        } else {
            self.expression(0, 0)?;
        }
        Some(())
    }
}

impl<'s> Expression<'s> {
    // Tokenization can grow before the token-count rejection. Bound geometric
    // capacity for one token per source byte, and at most two emitted ops per
    // token, plus moved/cloned source text. The caller also admits per-command
    // overhead, covering empty-input vectors and fixed container storage.
    pub const STORAGE_FACTOR: usize =
        2 * std::mem::size_of::<(Tok, usize)>() + 4 * std::mem::size_of::<Op>() + 4;

    // Mixed clauses and flat regions share one compiler body. Normalize the
    // source before this boundary instead of instantiating it for &str and Cow.
    #[inline(never)]
    pub fn compile(source: std::borrow::Cow<'s, str>) -> Option<Self> {
        if source.len() > 1024 || source.contains(['$', '"', '`']) {
            return None;
        }
        let tokens = tokenize(&source).ok()?;
        if tokens.len() > 256 {
            return None;
        }
        let mut c = Compiler {
            tokens,
            at: 0,
            ops: Vec::new(),
        };
        if source.trim().is_empty() {
            c.ops.push(Op::Number(0));
        } else {
            c.assignment()?;
            while c.peek() == &Tok::Op(",") {
                c.next();
                c.ops.push(Op::Drop);
                c.assignment()?;
            }
        }
        if c.peek() != &Tok::Eof {
            return None;
        }
        let mut stack = 0usize;
        for op in &c.ops {
            match op {
                Op::Number(_) | Op::Load(_) | Op::Update(..) => stack += 1,
                Op::Binary(_) | Op::Drop => stack = stack.checked_sub(1)?,
                _ => {}
            }
            if stack > 64 {
                return None;
            }
        }
        if stack != 1 {
            return None;
        }
        Some(Self { source, ops: c.ops })
    }
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.ops.iter().filter_map(|op| match op {
            Op::Load(n) | Op::Store(n) | Op::Update(n, ..) => Some(n.as_str()),
            _ => None,
        })
    }
}

impl Interpreter {
    pub(in crate::interpreter) fn evaluate_program_expression(
        &mut self,
        expr: &Expression<'_>,
        writes: bool,
    ) -> ArithResult<i64> {
        let mut budget = ArithmeticBudget::new();
        let mut ev = ArithEval::new(self, &mut budget);
        let result: ArithResult<i64> = (|| {
            ev.enter()?;
            ev.budget.charge(&expr.source)?;
            let mut stack = [0i64; 64];
            let mut used = 0;
            for op in &expr.ops {
                match op {
                    Op::Number(v) => {
                        stack[used] = *v;
                        used += 1;
                    }
                    Op::Load(n) => {
                        let target = ev.lvalue(n, None)?;
                        stack[used] = ev.read_lvalue_int(&target)?;
                        used += 1;
                    }
                    Op::Store(n) => {
                        let target = ev.lvalue(n, None)?;
                        ev.store(&target, stack[used - 1]);
                    }
                    Op::Update(n, delta, prefix) => {
                        let target = ev.lvalue(n, None)?;
                        let old = ev.read_lvalue_int(&target)?;
                        let new = old.wrapping_add(*delta);
                        ev.store(&target, new);
                        stack[used] = if *prefix { new } else { old };
                        used += 1;
                    }
                    Op::Drop => used -= 1,
                    Op::Unary(op) => {
                        let v = &mut stack[used - 1];
                        *v = match *op {
                            "!" => i64::from(*v == 0),
                            "~" => !*v,
                            "-" => v.wrapping_neg(),
                            _ => *v,
                        };
                    }
                    Op::Binary(op) => {
                        let r = stack[used - 1];
                        let l = stack[used - 2];
                        used -= 1;
                        stack[used - 1] = match *op {
                            "+" => l.wrapping_add(r),
                            "-" => l.wrapping_sub(r),
                            "*" => l.wrapping_mul(r),
                            "<<" => l.wrapping_shl((r & 63) as u32),
                            ">>" => l.wrapping_shr((r & 63) as u32),
                            "&" => l & r,
                            "|" => l | r,
                            "^" => l ^ r,
                            "<" => i64::from(l < r),
                            ">" => i64::from(l > r),
                            "<=" => i64::from(l <= r),
                            ">=" => i64::from(l >= r),
                            "==" => i64::from(l == r),
                            "!=" => i64::from(l != r),
                            _ => unreachable!(),
                        };
                    }
                }
            }
            ev.leave();
            Ok(stack[0])
        })();
        let pending = ev.writes;
        if writes {
            self.apply_arith_writes(pending);
        }
        result.map_err(|m| format!("{}: {m}", diag_echo(expr.source.trim())))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn compiler_reservation_covers_tokenizer_capacity_before_decline() {
        for source in ["()".repeat(512), format!("{}a", "a+=1,".repeat(63))] {
            let tokens = tokenize(&source).unwrap();
            let token_storage = tokens.capacity() * std::mem::size_of::<(Tok, usize)>();
            drop(tokens);
            let expression = Expression::compile(source.as_str().into());
            let op_storage = expression
                .as_ref()
                .map_or(0, |e| e.ops.capacity() * std::mem::size_of::<Op>());
            let reservation = (source.len() + 128) * Expression::STORAGE_FACTOR + 4096;
            assert!(
                reservation >= token_storage + op_storage,
                "compiler buffers need {}, admitted bound {reservation}",
                token_storage + op_storage
            );
        }
    }

    #[test]
    fn expressions_match_evaluator_values_and_ordered_writes() {
        for value in ["0", "7", "1+2", "9223372036854775807"] {
            for source in [
                "x+=3",
                "x=x*2+1",
                "x++ + x",
                "++x",
                "x<9",
                "x<<65",
                "x=3, x+=4, x*2",
                "!(x+3)",
                "~x & 0xff",
                "x=0, x--, x",
            ] {
                let mut a = Interpreter::new(Arc::new(crate::InMemoryFs::new()));
                let mut b = Interpreter::new(Arc::new(crate::InMemoryFs::new()));
                a.set_variable("x".into(), value.into());
                b.set_variable("x".into(), value.into());
                let expected = a.try_evaluate_arithmetic_with_assign(source);
                let expr = Expression::compile(source.into()).unwrap();
                let actual = b.evaluate_program_expression(&expr, true);
                assert_eq!(actual, expected, "{source}, x={value}");
                assert_eq!(
                    a.lookup_regular_variable("x"),
                    b.lookup_regular_variable("x")
                );
            }
        }
    }
    #[test]
    fn unsupported_grammar_uses_fallback() {
        for source in [
            "x/0",
            "a[0]++",
            "x && y++",
            "x?y:z",
            "$x+1",
            "x**2",
            "x=(y=3)",
            "08",
            "x<$(echo 4)",
        ] {
            assert!(Expression::compile(source.into()).is_none(), "{source}");
        }
    }
}
