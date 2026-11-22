//! Isolated whole-script execution-plan experiment (off by default).
//!
//! Plans borrow one parsed Script, live only for that execution, and reserve
//! storage before lowering. Pure scalar command trees lower to a flat program.
//! Mixed command lists keep the existing shell orchestration with prepared
//! children. Unsupported syntax/state uses the ordinary interpreter. No global
//! cache, pointer-keyed lookup, or new shell syntax is involved.
use super::*;
use arithmetic::program::Expression;

pub(super) struct ScriptPlan<'s> {
    pub commands: Vec<Option<Program<'s>>>,
    _storage: crate::limits::ExecutionBudgetLease,
}

pub(super) enum Program<'s> {
    Native(NativeProgram<'s>),
    List {
        list: &'s CommandList,
        children: Vec<Option<Program<'s>>>,
        _storage: crate::limits::ExecutionBudgetLease,
    },
}

pub(super) struct NativeProgram<'s> {
    ops: Vec<Instruction<'s>>,
    _storage: crate::limits::ExecutionBudgetLease,
}

enum Instruction<'s> {
    Enter(&'s Command, bool),
    Leave(&'s Command),
    Assign(&'s SimpleCommand),
    Arithmetic(Expression, Mode),
    Status(i32),
    Jump(usize),
    BranchStatus(usize, bool),
    BranchTest(usize),
    LoopStart,
    Iteration(Option<usize>),
    Line(usize),
    RememberBody,
    LoopEnd,
}

#[derive(Clone, Copy)]
enum Mode {
    Command,
    Condition,
    Clause,
}

fn numeric(value: &str) -> bool {
    if value.len() > 128 {
        return false;
    }
    let value = value.trim();
    value.is_empty() || arithmetic::parse_arith_number(value.trim_start_matches(['-', '+'])).is_ok()
}

fn assignments(simple: &SimpleCommand) -> bool {
    simple.name.parts.iter().all(|p| matches!(p,WordPart::Literal(s) if s.is_empty()))
        && simple.args.is_empty() && simple.redirects.is_empty() && !simple.assignments.is_empty()
        && simple.assignments.iter().all(|a| {
            a.index.is_none() && !a.append && matches!(&a.value,
                AssignmentValue::Scalar(w) if matches!(w.parts.as_slice(), [WordPart::Literal(s)] if numeric(s)))
        })
}

// Bound traversal before reserving any lowering buffers. Parser depth is bounded
// separately; this optional backend declines deeper/larger trees.
fn size(command: &Command, depth: usize) -> Option<usize> {
    if depth > 64 {
        return None;
    }
    let sequence = |body: &[Command]| {
        body.iter()
            .try_fold(0usize, |n, c| n.checked_add(size(c, depth + 1)?))
    };
    let extra = match command {
        Command::Simple(s) if assignments(s) => {
            s.assignments.iter().map(|a| a.name.len() + 64).sum()
        }
        Command::List(l) => {
            size(&l.first, depth + 1)?
                + l.rest
                    .iter()
                    .try_fold(0usize, |n, (_, c)| Some(n + size(c, depth + 1)?))?
        }
        Command::Compound(c, r) if r.is_empty() => match c {
            CompoundCommand::Arithmetic(e) => e.len(),
            CompoundCommand::ArithmeticFor(f) => {
                f.init.len() + f.condition.len() + f.step.len() + sequence(&f.body)?
            }
            CompoundCommand::While(w) => sequence(&w.condition)? + sequence(&w.body)?,
            CompoundCommand::Until(w) => sequence(&w.condition)? + sequence(&w.body)?,
            CompoundCommand::BraceGroup(b) => sequence(b)?,
            CompoundCommand::If(i) => {
                sequence(&i.condition)?
                    + sequence(&i.then_branch)?
                    + i.elif_branches
                        .iter()
                        .try_fold(0usize, |n, (c, b)| Some(n + sequence(c)? + sequence(b)?))?
                    + i.else_branch.as_ref().map_or(Some(0), |b| sequence(b))?
            }
            _ => return None,
        },
        _ => return None,
    };
    let total = extra.checked_add(128)?;
    (total <= 32768).then_some(total)
}

impl<'s> ScriptPlan<'s> {
    pub fn compile(
        script: &'s Script,
        budget: &crate::limits::ExecutionBudget,
    ) -> Result<Option<Box<Self>>> {
        fn candidate(command: &Command, depth: usize) -> bool {
            if depth > 64 {
                return false;
            }
            if let Command::List(list) = command {
                candidate(&list.first, depth + 1)
                    || list.rest.iter().any(|(_, c)| candidate(c, depth + 1))
            } else {
                size(command, 0).is_some()
            }
        }
        if !script.commands.iter().any(|c| candidate(c, 0)) {
            return Ok(None);
        }
        let Some(storage) = budget.try_lease_plan(
            script
                .commands
                .len()
                .saturating_mul(std::mem::size_of::<Option<Program<'s>>>())
                .saturating_add(std::mem::size_of::<Self>()),
        )?
        else {
            return Ok(None);
        };
        let commands = script
            .commands
            .iter()
            .map(|c| Program::compile(c, budget, 0))
            .collect::<Result<Vec<_>>>()?;
        if commands.iter().all(Option::is_none) {
            return Ok(None);
        }
        Ok(Some(Box::new(Self {
            commands,
            _storage: storage,
        })))
    }
}

impl<'s> Program<'s> {
    fn compile(
        command: &'s Command,
        budget: &crate::limits::ExecutionBudget,
        depth: usize,
    ) -> Result<Option<Self>> {
        if depth > 64 {
            return Ok(None);
        }
        if let Some(bytes) = size(command, 0) {
            budget.consume_work(bytes as u64)?;
            if let Some(storage) = budget.try_lease_plan(bytes * 64 + 4096)? {
                let mut ops = Vec::new();
                if lower(command, &mut ops, true).is_some() {
                    return Ok(Some(Self::Native(NativeProgram {
                        ops,
                        _storage: storage,
                    })));
                }
            }
        }
        if let Command::List(list) = command {
            let Some(storage) = budget.try_lease_plan(
                (list.rest.len() + 1).saturating_mul(std::mem::size_of::<Option<Program<'s>>>()),
            )?
            else {
                return Ok(None);
            };
            let children = std::iter::once(list.first.as_ref())
                .chain(list.rest.iter().map(|(_, c)| c))
                .map(|c| Self::compile(c, budget, depth + 1))
                .collect::<Result<Vec<_>>>()?;
            if children.iter().all(Option::is_none) {
                return Ok(None);
            }
            return Ok(Some(Self::List {
                list,
                children,
                _storage: storage,
            }));
        }
        Ok(None)
    }
    pub fn eligible(&self, shell: &Interpreter) -> bool {
        let Self::Native(program) = self else {
            return true;
        };
        // TODO(experiment): lower these contexts only after their boundary
        // semantics have dedicated VM coverage. Fallback is a normal execution.
        if cfg!(feature = "failpoints")
            || shell.is_errexit_enabled()
            || shell.is_nounset()
            || shell.is_xtrace_enabled()
            || shell.flags.contains(BashFlags::NOEXEC)
            || !shell.scoped.traps.is_empty()
            || shell.pipe_out.is_some()
            || !shell.subst_stderr.is_empty()
            || shell.has_arith_unbound()
        {
            return false;
        }
        let plain = |name: &str| {
            shell
                .var_attrs_get(name)
                .difference(VarAttrs::EXPORT)
                .is_empty()
                && !shell.scoped.arrays.contains_key(name)
                && !shell.scoped.assoc_arrays.contains_key(name)
                && shell.resolve_nameref(name) == name
                && shell.lookup_regular_variable_ref(name).is_none_or(numeric)
        };
        program.ops.iter().all(|op| match op {
            Instruction::Arithmetic(e, _) => e.names().all(plain),
            Instruction::Assign(s) => s.assignments.iter().all(|a| plain(&a.name)),
            _ => true,
        })
    }
}

fn expression<'s>(source: &str, mode: Mode, ops: &mut Vec<Instruction<'s>>) -> Option<()> {
    ops.push(Instruction::Arithmetic(Expression::compile(source)?, mode));
    Some(())
}
fn sequence<'s>(commands: &'s [Command], ops: &mut Vec<Instruction<'s>>) -> Option<()> {
    ops.push(Instruction::Status(0));
    for command in commands {
        lower(command, ops, false)?;
    }
    Some(())
}
fn patch(ops: &mut [Instruction<'_>], index: usize, target: usize) {
    match &mut ops[index] {
        Instruction::Jump(n) | Instruction::BranchStatus(n, _) | Instruction::BranchTest(n) => {
            *n = target
        }
        _ => unreachable!(),
    }
}
fn lower<'s>(command: &'s Command, ops: &mut Vec<Instruction<'s>>, root: bool) -> Option<()> {
    ops.push(Instruction::Enter(command, root));
    match command {
        Command::Simple(s) if assignments(s) => ops.push(Instruction::Assign(s)),
        Command::List(l) if l.rest.iter().all(|(op, _)| *op == ListOperator::Semicolon) => {
            lower(&l.first, ops, false)?;
            for (_, c) in &l.rest {
                lower(c, ops, false)?;
            }
        }
        Command::Compound(c, r) if r.is_empty() => match c {
            CompoundCommand::Arithmetic(e) => {
                expression(&crate::parser::arith_exec_text(e), Mode::Command, ops)?
            }
            CompoundCommand::BraceGroup(b) => sequence(b, ops)?,
            CompoundCommand::If(i) => {
                let mut ends = Vec::new();
                for (condition, body) in std::iter::once((&i.condition, &i.then_branch))
                    .chain(i.elif_branches.iter().map(|(c, b)| (c, b)))
                {
                    sequence(condition, ops)?;
                    let branch = ops.len();
                    ops.push(Instruction::BranchStatus(0, false));
                    sequence(body, ops)?;
                    ends.push(ops.len());
                    ops.push(Instruction::Jump(0));
                    let next = ops.len();
                    patch(ops, branch, next);
                }
                sequence(i.else_branch.as_deref().unwrap_or(&[]), ops)?;
                let end = ops.len();
                for jump in ends {
                    patch(ops, jump, end);
                }
            }
            CompoundCommand::ArithmeticFor(f) => {
                if !f.init.is_empty() {
                    expression(&f.init, Mode::Clause, ops)?;
                }
                ops.push(Instruction::LoopStart);
                let start = ops.len();
                ops.push(Instruction::Iteration(Some(f.span.line())));
                let mut branch = None;
                if !f.condition.is_empty() {
                    expression(&f.condition, Mode::Condition, ops)?;
                    branch = Some(ops.len());
                    ops.push(Instruction::BranchTest(0));
                }
                sequence(&f.body, ops)?;
                ops.push(Instruction::RememberBody);
                if !f.step.is_empty() {
                    ops.push(Instruction::Line(f.span.line()));
                    expression(&f.step, Mode::Clause, ops)?;
                }
                ops.push(Instruction::Jump(start));
                let end = ops.len();
                if let Some(branch) = branch {
                    patch(ops, branch, end);
                }
                ops.push(Instruction::LoopEnd);
            }
            CompoundCommand::While(_) | CompoundCommand::Until(_) => {
                let (condition, body, until) = match c {
                    CompoundCommand::While(w) => (w.condition.as_slice(), w.body.as_slice(), false),
                    CompoundCommand::Until(w) => (w.condition.as_slice(), w.body.as_slice(), true),
                    _ => unreachable!(),
                };
                ops.push(Instruction::LoopStart);
                let start = ops.len();
                ops.push(Instruction::Iteration(None));
                sequence(condition, ops)?;
                let branch = ops.len();
                ops.push(Instruction::BranchStatus(0, until));
                sequence(body, ops)?;
                ops.push(Instruction::RememberBody);
                ops.push(Instruction::Jump(start));
                let end = ops.len();
                patch(ops, branch, end);
                ops.push(Instruction::LoopEnd);
            }
            _ => return None,
        },
        _ => return None,
    }
    ops.push(Instruction::Leave(command));
    Some(())
}

impl Interpreter {
    pub(super) async fn execute_program(&mut self, program: &Program<'_>) -> Result<ExecResult> {
        let Program::Native(program) = program else {
            let Program::List { list, children, .. } = program else {
                unreachable!()
            };
            let route = std::mem::take(&mut self.route_top_list);
            let result = self
                .execute_list_with_commands(list, route, PreparedCommands(children))
                .await;
            if route && let Ok(r) = &result {
                self.top_list_routed = Some((r.stdout.len(), r.stderr.len()));
            }
            return result;
        };
        // Loop stack is bounded by the admitted compiler depth, and restored on
        // every error. No interpreter scope survives this borrowed plan.
        let mut loops = [0i32; 128];
        let mut loop_count = 0;
        let result = async {
            let mut pc = 0;
            let mut status = 0;
            let mut test = false;
            while let Some(op) = program.ops.get(pc) {
                self.check_cancelled()?;
                pc += 1;
                match op {
                    Instruction::Enter(command, root) => {
                        if !root {
                            if self.counters.commands > 0
                                && self.counters.commands.is_multiple_of(64)
                            {
                                crate::execution::yield_once().await;
                            }
                            self.set_command_lineno(command);
                            if let Some(name) = Self::command_name(command) {
                                self.last_command_name = name.to_string();
                            }
                            self.charge_command_execution()?;
                        }
                    }
                    Instruction::Leave(command) => {
                        if matches!(
                            command,
                            Command::Simple(_)
                                | Command::Compound(CompoundCommand::Arithmetic(_), _)
                        ) {
                            self.set_simple_pipestatus(status);
                        }
                        let result = self.abort_line_on_error(Ok(ExecResult {
                            exit_code: status,
                            ..Default::default()
                        }))?;
                        if result.control_flow != ControlFlow::None {
                            return Ok(result);
                        }
                        self.last_exit_code = status;
                    }
                    Instruction::Assign(simple) => {
                        let result = self.execute_simple_command(simple, None).await?;
                        if !result.stdout.is_empty()
                            || !result.stderr.is_empty()
                            || result.control_flow != ControlFlow::None
                        {
                            return Ok(result);
                        }
                        status = result.exit_code;
                    }
                    Instruction::Arithmetic(e, mode) => {
                        match self.evaluate_program_expression(e, !matches!(mode, Mode::Condition))
                        {
                            Ok(value) => match mode {
                                Mode::Command => status = i32::from(value == 0),
                                Mode::Condition => test = value != 0,
                                Mode::Clause => {}
                            },
                            Err(message) => {
                                if matches!(mode, Mode::Command) {
                                    return Ok(ExecResult::err(
                                        self.arith_diag("((: ", &message),
                                        1,
                                    ));
                                }
                                self.record_arith_error(message);
                                test = false;
                            }
                        }
                    }
                    Instruction::Status(value) => status = *value,
                    Instruction::Jump(target) => pc = *target,
                    Instruction::BranchStatus(target, success) => {
                        if (status == 0) == *success {
                            pc = *target;
                        }
                    }
                    Instruction::BranchTest(target) => {
                        if !test {
                            pc = *target;
                        }
                    }
                    Instruction::LoopStart => {
                        self.counters.enter_loop();
                        self.loop_depth += 1;
                        loops[loop_count] = 0;
                        loop_count += 1;
                    }
                    Instruction::Iteration(line) => {
                        self.counters.tick_loop(&self.limits)?;
                        if let Some(line) = line {
                            self.current_line = self.line_at(*line);
                        }
                    }
                    Instruction::Line(line) => self.current_line = self.line_at(*line),
                    Instruction::RememberBody => loops[loop_count - 1] = status,
                    Instruction::LoopEnd => {
                        loop_count -= 1;
                        status = loops[loop_count];
                        self.counters.exit_loop();
                        self.loop_depth -= 1;
                    }
                }
            }
            Ok(ExecResult {
                exit_code: status,
                ..Default::default()
            })
        }
        .await;
        for _ in 0..loop_count {
            self.counters.exit_loop();
            self.loop_depth -= 1;
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn native_regions_match_ast_execution_and_command_charges() {
        for source in [
            "a=2; ((a+=3)); if ((a<10)); then ((a*=2)); else a=0; fi",
            "a=0; while ((a<8)); do ((a++)); done",
            "a=0; until ((a==8)); do ((a++)); done",
            "total=0; for ((i=0;i<20;i++)); do ((total+=i)); done",
            "total=0; for ((i=0;i<5;i++)); do for ((j=0;j<3;j++)); do ((total+=i+j)); done; done",
            "a=0; if ((a)); then a=2; elif ((1)); then a=3; else a=4; fi",
            "if ((0)); then a=1; fi",
            "for ((i=0;i<0;i++)); do ((i++)); done",
            "a=0; { ((a++)); ((a+=7)); }",
        ] {
            let ast = Parser::new(source).parse().unwrap();
            let mut expected = Interpreter::new(Arc::new(crate::InMemoryFs::new()));
            let mut actual = Interpreter::new(Arc::new(crate::InMemoryFs::new()));
            let plan = ScriptPlan::compile(&ast, &actual.execution_budget)
                .unwrap()
                .unwrap();
            for (index, command) in ast.commands.iter().enumerate() {
                let program = plan.commands[index].as_ref().expect("test must compile");
                assert!(
                    matches!(program, Program::Native(_)),
                    "must exercise flat instructions: {source}"
                );
                assert!(program.eligible(&actual), "must exercise VM: {source}");
                let before = expected.execute_command(command).await.unwrap();
                let after = actual
                    .execute_command_with_program(command, Some(program))
                    .await
                    .unwrap();
                assert_eq!(
                    (
                        before.stdout,
                        before.stderr,
                        before.exit_code,
                        before.control_flow
                    ),
                    (
                        after.stdout,
                        after.stderr,
                        after.exit_code,
                        after.control_flow
                    ),
                    "{source}"
                );
            }
            for name in ["a", "i", "j", "total"] {
                assert_eq!(
                    expected.lookup_regular_variable(name),
                    actual.lookup_regular_variable(name),
                    "{source}: {name}"
                );
            }
            assert_eq!(
                expected.counters.commands, actual.counters.commands,
                "{source}"
            );
            assert_eq!(
                expected.scoped.arrays.get("PIPESTATUS"),
                actual.scoped.arrays.get("PIPESTATUS"),
                "{source}"
            );
            assert_eq!(actual.loop_depth, 0);
        }
    }

    #[tokio::test]
    async fn budget_errors_restore_native_loop_scopes() {
        let ast = Parser::new("for ((i=0;i<100;i++)); do ((a+=i)); done")
            .parse()
            .unwrap();
        for limits in [
            ExecutionLimits::new().max_commands(5),
            ExecutionLimits::new().max_loop_iterations(5),
        ] {
            let mut interp = Interpreter::new(Arc::new(crate::InMemoryFs::new()));
            interp.limits = limits;
            let plan = ScriptPlan::compile(&ast, &interp.execution_budget)
                .unwrap()
                .unwrap();
            let result = interp
                .execute_command_with_program(&ast.commands[0], plan.commands[0].as_ref())
                .await;
            assert!(result.is_err());
            assert_eq!(interp.loop_depth, 0);
        }
    }

    #[test]
    fn dynamic_and_instrumented_contexts_decline_native_execution() {
        let ast = Parser::new("((a+=1))").parse().unwrap();
        let mut interp = Interpreter::new(Arc::new(crate::InMemoryFs::new()));
        let plan = ScriptPlan::compile(&ast, &interp.execution_budget)
            .unwrap()
            .unwrap();
        let program = plan.commands[0].as_ref().unwrap();
        assert!(program.eligible(&interp));
        interp.set_variable("a".into(), "1+2".into());
        assert!(!program.eligible(&interp));
        interp.set_variable("a".into(), "2".into());
        interp.flags.insert(BashFlags::NOUNSET);
        assert!(!program.eligible(&interp));
    }
}

// A borrowed selector exists only in prepared lists. Ordinary lists use a
// zero-sized PlainCommands selector and carry no plan reference in their future.
struct PreparedCommands<'p, 's>(&'p [Option<Program<'s>>]);
impl ListCommands for PreparedCommands<'_, '_> {
    fn execute<'a>(
        &'a self,
        shell: &'a mut Interpreter,
        command: &'a Command,
        index: usize,
    ) -> command_dispatch::CommandFuture<'a> {
        shell.execute_command_with_program(command, self.0[index].as_ref())
    }
}
