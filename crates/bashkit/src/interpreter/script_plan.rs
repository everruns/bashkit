//! Isolated whole-script execution-plan experiment (off by default).
//!
//! THREAT[TM-DOS-147]: plans borrow one parsed Script, live only for that execution, and reserve
//! storage before lowering. Pure scalar command trees lower to a flat program.
//! Mixed command lists keep the existing shell orchestration with prepared
//! children. Unsupported syntax/state uses the ordinary interpreter. No global
//! cache, pointer-keyed lookup, or new shell syntax is involved.
use super::*;
use arithmetic::program::Expression;

pub(super) struct ScriptPlan<'s> {
    commands: Vec<Option<Program<'s>>>,
    ops: Vec<Instruction<'s>>,
    _storage: crate::limits::ExecutionBudgetLease,
}

pub(super) enum Program<'s> {
    Native(NativeProgram),
    List {
        list: &'s CommandList,
        children: Vec<Option<Program<'s>>>,
    },
}

pub(super) struct NativeProgram {
    range: std::ops::Range<usize>,
}

#[derive(Clone, Copy)]
pub(super) struct ProgramRef<'p, 's> {
    program: &'p Program<'s>,
    ops: &'p [Instruction<'s>],
}

struct Builder<'b, 's> {
    budget: &'b crate::limits::ExecutionBudget,
    used: usize,
    admitted: usize,
    ops: Vec<Instruction<'s>>,
    #[cfg(test)]
    _after_buffers: Option<AfterBuffers>,
    // Rust drops fields in declaration order: release admission only after
    // all owning buffers (including partially lowered regions) are destroyed.
    storage: crate::limits::ExecutionBudgetLease,
}

#[cfg(test)]
struct AfterBuffers(Option<Box<dyn FnOnce() + Send>>);
#[cfg(test)]
impl Drop for AfterBuffers {
    fn drop(&mut self) {
        if let Some(check) = self.0.take() {
            check();
        }
    }
}
impl Builder<'_, '_> {
    fn reserve(&mut self, bytes: usize) -> Result<bool> {
        let Some(used) = self.used.checked_add(bytes) else {
            return Ok(false);
        };
        if used > self.admitted {
            let capacity = used.checked_next_power_of_two().unwrap_or(used);
            if !self.storage.try_grow_plan_to(capacity)? {
                return Ok(false);
            }
            self.admitted = capacity;
        }
        self.used = used;
        Ok(true)
    }
}

enum Instruction<'s> {
    Enter(&'s Command, bool),
    Leave(&'s Command),
    Assign(&'s SimpleCommand),
    Arithmetic(Expression<'s>, Mode),
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
    if value.is_empty() {
        return true;
    }
    // A chain of signs is an expression (or malformed increment), not a
    // literal. Keep recursive variable evaluation outside native admission.
    let magnitude = value
        .strip_prefix('-')
        .or_else(|| value.strip_prefix('+'))
        .unwrap_or(value);
    magnitude.as_bytes().first().is_some_and(u8::is_ascii_digit)
        && arithmetic::parse_arith_number(magnitude).is_ok()
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

// A small cold statement rarely recovers the cost of lowering it. The cheap
// probe stays inline; full lowering is outlined away from ordinary tool calls.
#[inline]
fn worthwhile(command: &Command, weight: &mut usize, depth: usize) -> bool {
    if depth > 64 {
        return false;
    }
    match command {
        Command::Simple(s) => {
            if assignments(s) {
                *weight = weight.saturating_add(1);
            }
        }
        Command::Compound(CompoundCommand::Arithmetic(_), r) if r.is_empty() => {
            *weight = weight.saturating_add(1);
        }
        Command::Compound(
            CompoundCommand::ArithmeticFor(_)
            | CompoundCommand::While(_)
            | CompoundCommand::Until(_),
            r,
        ) if r.is_empty() => {
            return size(command, 0).is_some();
        }
        Command::Compound(_, _) => {
            if let Some(bytes) = size(command, 0) {
                *weight = weight.saturating_add(bytes / 128);
            }
        }
        Command::List(list) => return worthwhile_list(list, weight, depth),
        _ => {}
    }
    *weight >= 8
}
fn worthwhile_list(list: &CommandList, weight: &mut usize, depth: usize) -> bool {
    std::iter::once(list.first.as_ref())
        .chain(list.rest.iter().map(|(_, c)| c))
        .any(|c| worthwhile(c, weight, depth + 1))
}

impl<'s> ScriptPlan<'s> {
    #[inline]
    pub fn compile(
        script: &'s Script,
        budget: &crate::limits::ExecutionBudget,
    ) -> Result<Option<Box<Self>>> {
        let mut weight = 0;
        if !script
            .commands
            .iter()
            .any(|c| worthwhile(c, &mut weight, 0))
        {
            return Ok(None);
        }
        Self::lower_script(script, budget)
    }

    #[cold]
    fn lower_script(
        script: &'s Script,
        budget: &crate::limits::ExecutionBudget,
    ) -> Result<Option<Box<Self>>> {
        let capacity = script.commands.len().saturating_mul(4).clamp(8, 256);
        let bytes = script
            .commands
            .len()
            .saturating_mul(std::mem::size_of::<Option<Program<'s>>>())
            .saturating_add(std::mem::size_of::<Self>())
            .saturating_add(capacity.saturating_mul(std::mem::size_of::<Instruction<'s>>()));
        let Some(storage) = budget.try_lease_plan(bytes)? else {
            return Ok(None);
        };
        let mut builder = Builder {
            budget,
            storage,
            used: bytes,
            admitted: bytes,
            ops: Vec::with_capacity(capacity),
            #[cfg(test)]
            _after_buffers: None,
        };
        // Keep capacity equal to the admitted command storage; collecting a
        // fallible iterator may otherwise grow geometrically past its length.
        let mut commands = Vec::with_capacity(script.commands.len());
        for command in &script.commands {
            commands.push(Program::compile(command, &mut builder, 0)?);
        }
        if commands.iter().all(Option::is_none) {
            return Ok(None);
        }
        Ok(Some(Box::new(Self {
            commands,
            ops: builder.ops,
            _storage: builder.storage,
        })))
    }
    pub fn command(&self, index: usize) -> Option<ProgramRef<'_, 's>> {
        self.commands[index].as_ref().map(|program| ProgramRef {
            program,
            ops: &self.ops,
        })
    }
}

impl<'s> Program<'s> {
    fn compile(
        command: &'s Command,
        builder: &mut Builder<'_, 's>,
        depth: usize,
    ) -> Result<Option<Self>> {
        if depth > 64 {
            return Ok(None);
        }
        if let Some(bytes) = size(command, 0) {
            builder.budget.consume_work(bytes as u64)?;
            let used = builder.used;
            let capacity = builder.ops.capacity();
            if !builder.reserve(bytes * Expression::STORAGE_FACTOR + 4096)? {
                return Ok(None);
            }
            let start = builder.ops.len();
            if lower(command, &mut builder.ops, true).is_some() {
                return Ok(Some(Self::Native(NativeProgram {
                    range: start..builder.ops.len(),
                })));
            }
            builder.ops.truncate(start);
            // Failed lowering drops expression buffers, but retains the shared
            // vector capacity. Keep those actual retained bytes admitted.
            builder.used = used.saturating_add(
                (builder.ops.capacity() - capacity)
                    .saturating_mul(std::mem::size_of::<Instruction<'s>>()),
            );
        }
        if let Command::List(list) = command {
            if !builder.reserve(
                (list.rest.len() + 1).saturating_mul(std::mem::size_of::<Option<Program<'s>>>()),
            )? {
                return Ok(None);
            }
            let mut children = Vec::with_capacity(list.rest.len() + 1);
            for command in
                std::iter::once(list.first.as_ref()).chain(list.rest.iter().map(|(_, c)| c))
            {
                children.push(Self::compile(command, builder, depth + 1)?);
            }
            if children.iter().all(Option::is_none) {
                return Ok(None);
            }
            return Ok(Some(Self::List { list, children }));
        }
        Ok(None)
    }
}

impl ProgramRef<'_, '_> {
    pub fn eligible(&self, shell: &Interpreter) -> bool {
        let Program::Native(program) = self.program else {
            return true;
        };
        // TODO(experiment): lower these contexts only after their boundary
        // semantics have dedicated VM coverage. Fallback is a normal execution.
        // Resource failpoints must exercise native fuel checks too. Only the
        // command-boundary injector needs AST execution: its early status/error
        // actions deliberately replace a whole shell command. This lookup is
        // compiled out of production builds.
        #[cfg(feature = "failpoints")]
        if fail::list()
            .iter()
            .any(|(name, action)| name == "interp::execute_command" && action != "off")
        {
            return false;
        }
        if shell.is_errexit_enabled()
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
        self.ops[program.range.clone()].iter().all(|op| match op {
            Instruction::Arithmetic(e, _) => e.names().all(plain),
            Instruction::Assign(s) => s.assignments.iter().all(|a| plain(&a.name)),
            _ => true,
        })
    }
}

fn expression<'s>(
    source: std::borrow::Cow<'s, str>,
    mode: Mode,
    ops: &mut Vec<Instruction<'s>>,
) -> Option<()> {
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
                let source = if e.contains('"') {
                    std::borrow::Cow::Owned(crate::parser::arith_exec_text(e))
                } else {
                    std::borrow::Cow::Borrowed(e.trim())
                };
                expression(source, Mode::Command, ops)?
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
                    expression(f.init.as_str().into(), Mode::Clause, ops)?;
                }
                ops.push(Instruction::LoopStart);
                let start = ops.len();
                ops.push(Instruction::Iteration(Some(f.span.line())));
                let mut branch = None;
                if !f.condition.is_empty() {
                    expression(f.condition.as_str().into(), Mode::Condition, ops)?;
                    branch = Some(ops.len());
                    ops.push(Instruction::BranchTest(0));
                }
                sequence(&f.body, ops)?;
                ops.push(Instruction::RememberBody);
                if !f.step.is_empty() {
                    ops.push(Instruction::Line(f.span.line()));
                    expression(f.step.as_str().into(), Mode::Clause, ops)?;
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
    pub(super) async fn execute_program(
        &mut self,
        program: ProgramRef<'_, '_>,
    ) -> Result<ExecResult> {
        let Program::Native(native) = program.program else {
            let Program::List { list, children } = program.program else {
                unreachable!()
            };
            let route = std::mem::take(&mut self.route_top_list);
            let result = self
                .execute_prepared_list(
                    list,
                    route,
                    PreparedCommands {
                        children,
                        ops: program.ops,
                    },
                )
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
            let mut pc = native.range.start;
            let mut status = 0;
            let mut test = false;
            while pc < native.range.end {
                let op = &program.ops[pc];
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

// A borrowed selector exists only in prepared lists. Ordinary list futures use
// the original direct command calls and carry no selector or plan reference.
pub(super) struct PreparedCommands<'p, 's> {
    children: &'p [Option<Program<'s>>],
    ops: &'p [Instruction<'s>],
}
impl PreparedCommands<'_, '_> {
    pub(super) fn execute<'a>(
        &'a self,
        shell: &'a mut Interpreter,
        command: &'a Command,
        index: usize,
    ) -> command_dispatch::CommandFuture<'a> {
        shell.execute_command_with_program(
            command,
            self.children[index].as_ref().map(|program| ProgramRef {
                program,
                ops: self.ops,
            }),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numeric_admission_requires_a_literal_instead_of_unary_expressions() {
        for value in ["++1", "--1", "-+2", "+-2", "+ 2"] {
            assert!(
                !numeric(value),
                "expression-valued variable must fall back: {value}"
            );
        }
        for value in ["", "-2", "+2", "  -2 ", "010", "16#ff"] {
            assert!(
                numeric(value),
                "numeric literal should remain eligible: {value}"
            );
        }
    }

    #[test]
    fn differential_fuzz_operator_templates_reach_native_execution() {
        let interp = Interpreter::new(Arc::new(crate::InMemoryFs::new()));
        for operator in ["+", "-", "*", "^", "|", "&", "<<", ">>"] {
            let source = format!(
                "for ((i=0;i<8;i++)); do if ((i&1)); then ((a=a{operator}16)); else ((a+=b)); fi; done"
            );
            let ast = Parser::new(&source).parse().unwrap();
            let plan = ScriptPlan::compile(&ast, &interp.execution_budget)
                .unwrap()
                .unwrap();
            let program = plan.command(0).unwrap();
            assert!(matches!(program.program, Program::Native(_)), "{operator}");
            assert!(program.eligible(&interp), "{operator}");
        }
    }

    #[tokio::test]
    async fn abandoned_native_execution_releases_plan_and_session_is_reusable() {
        let mut bash = crate::Bash::new();
        let mut future = Box::pin(bash.exec("for ((i=0;i<10000;i++)); do ((a+=i)); done"));
        let mut context = std::task::Context::from_waker(std::task::Waker::noop());
        assert!(std::future::Future::poll(future.as_mut(), &mut context).is_pending());
        drop(future);
        assert!(
            bash.interpreter
                .lookup_regular_variable("i")
                .unwrap()
                .parse::<i64>()
                .unwrap()
                > 0
        );
        assert_eq!(bash.interpreter.execution_budget.live_bytes_for_test(), 0);
        let next = bash
            .exec("a=0; for ((i=0;i<3;i++)); do ((a+=1)); done; printf '%s' \"$a\"")
            .await
            .unwrap();
        assert_eq!(next.stdout, "3");
        assert_eq!(next.exit_code, 0);
        assert_eq!(bash.interpreter.loop_depth, 0);
    }

    #[test]
    fn declined_and_failed_compilation_refund_storage() {
        let ast = Parser::new("for ((i=0;i<3;i++)); do ((a+=i)); done")
            .parse()
            .unwrap();
        for (limits, work_error) in [
            (ExecutionLimits::new().max_live_intermediate_bytes(1), false),
            (ExecutionLimits::new().max_work_units(1), true),
        ] {
            let budget =
                crate::limits::ExecutionBudget::new(&limits, Arc::new(AtomicBool::new(false)));
            let result = ScriptPlan::compile(&ast, &budget);
            if work_error {
                assert!(result.is_err());
            } else {
                assert!(result.unwrap().is_none());
            }
            assert_eq!(budget.live_bytes_for_test(), 0);
        }
    }

    #[cfg(feature = "failpoints")]
    #[tokio::test]
    async fn native_loop_fault_injection_keeps_command_budget_enforced() {
        let _scenario = fail::FailScenario::setup();
        fail::cfg("limits::tick_loop", "return(reset_counter)").unwrap();
        let ast = Parser::new("for ((i=0;1;i++)); do ((a+=i)); done")
            .parse()
            .unwrap();
        let mut interp = Interpreter::new(Arc::new(crate::InMemoryFs::new()));
        interp.limits = ExecutionLimits::new()
            .max_commands(12)
            .max_loop_iterations(2);
        let plan = ScriptPlan::compile(&ast, &interp.execution_budget)
            .unwrap()
            .unwrap();
        let program = plan.command(0).unwrap();
        assert!(
            program.eligible(&interp),
            "resource fault injection must reach the VM"
        );
        let result = interp
            .execute_command_with_program(&ast.commands[0], Some(program))
            .await;
        assert!(matches!(
            result,
            Err(crate::Error::ResourceLimit(
                crate::limits::LimitExceeded::MaxCommands(12)
            ))
        ));
        assert_eq!(interp.loop_depth, 0);
        fail::cfg("interp::execute_command", "return(error)").unwrap();
        assert!(
            !program.eligible(&interp),
            "command injection must retain AST boundaries"
        );
    }

    #[test]
    fn builder_reservation_outlives_buffer_destruction() {
        let budget = crate::limits::ExecutionBudget::new(
            &ExecutionLimits::new(),
            Arc::new(AtomicBool::new(false)),
        );
        let observed = Arc::new(std::sync::atomic::AtomicU64::new(u64::MAX));
        let observer = observed.clone();
        let shared = budget.clone();
        let builder = Builder {
            budget: &budget,
            storage: budget.try_lease_plan(1024).unwrap().unwrap(),
            used: 1024,
            admitted: 1024,
            ops: Vec::with_capacity(8),
            _after_buffers: Some(AfterBuffers(Some(Box::new(move || {
                observer.store(shared.live_bytes_for_test(), Ordering::Relaxed);
            })))),
        };
        drop(builder);
        assert!(
            observed.load(Ordering::Relaxed) > 0,
            "buffers must be released before their admission lease"
        );
        assert_eq!(budget.live_bytes_for_test(), 0);
    }

    #[test]
    fn automatic_policy_skips_short_scripts_and_keeps_general_regions() {
        let budget = crate::limits::ExecutionBudget::new(
            &ExecutionLimits::new(),
            Arc::new(AtomicBool::new(false)),
        );
        for source in ["printf hello", "a=2; ((a+=3)); printf '%s' \"$a\""] {
            let ast = Parser::new(source).parse().unwrap();
            assert!(ScriptPlan::compile(&ast, &budget).unwrap().is_none());
        }
        for source in [
            "a=0; while ((a<4)); do ((a++)); done".to_string(),
            format!("a=0\n{}", "((a+=1))\n".repeat(16)),
        ] {
            let ast = Parser::new(&source).parse().unwrap();
            assert!(ScriptPlan::compile(&ast, &budget).unwrap().is_some());
        }
    }

    #[tokio::test]
    async fn arena_offsets_and_failed_regions_preserve_later_branches() {
        let source = "a=1\nif ((a<3)); then ((a+=2)); else a=9; fi\n((a/0))\nfor ((i=0;i<3;i++)); do ((a+=i)); done\n((a+=4))";
        let ast = Parser::new(source).parse().unwrap();
        let mut before = Interpreter::new(Arc::new(crate::InMemoryFs::new()));
        let mut after = Interpreter::new(Arc::new(crate::InMemoryFs::new()));
        let plan = ScriptPlan::lower_script(&ast, &after.execution_budget)
            .unwrap()
            .unwrap();
        assert!(plan.command(2).is_none(), "division must use AST fallback");
        let mut end = 0;
        for (index, command) in ast.commands.iter().enumerate() {
            if let Some(program) = plan.command(index) {
                let Program::Native(native) = program.program else {
                    panic!("expected native region")
                };
                assert!(native.range.start >= end);
                end = native.range.end;
            }
            let expected = before.execute_command(command).await.unwrap();
            let actual = after
                .execute_command_with_program(command, plan.command(index))
                .await
                .unwrap();
            assert_eq!(
                (
                    expected.stdout,
                    expected.stderr,
                    expected.exit_code,
                    expected.control_flow
                ),
                (
                    actual.stdout,
                    actual.stderr,
                    actual.exit_code,
                    actual.control_flow
                )
            );
        }
        for name in ["a", "i"] {
            assert_eq!(
                before.lookup_regular_variable(name),
                after.lookup_regular_variable(name)
            );
        }
        assert_eq!(before.counters.commands, after.counters.commands);
    }

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
            let plan = ScriptPlan::lower_script(&ast, &actual.execution_budget)
                .unwrap()
                .unwrap();
            for (index, command) in ast.commands.iter().enumerate() {
                let program = plan.command(index).expect("test must compile");
                assert!(
                    matches!(program.program, Program::Native(_)),
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
            let plan = ScriptPlan::lower_script(&ast, &interp.execution_budget)
                .unwrap()
                .unwrap();
            let result = interp
                .execute_command_with_program(&ast.commands[0], plan.command(0))
                .await;
            assert!(result.is_err());
            assert_eq!(interp.loop_depth, 0);
        }
    }

    #[test]
    fn dynamic_and_instrumented_contexts_decline_native_execution() {
        let ast = Parser::new("((a+=1))").parse().unwrap();
        let mut interp = Interpreter::new(Arc::new(crate::InMemoryFs::new()));
        let plan = ScriptPlan::lower_script(&ast, &interp.execution_budget)
            .unwrap()
            .unwrap();
        let program = plan.command(0).unwrap();
        assert!(program.eligible(&interp));
        interp.set_variable("a".into(), "1+2".into());
        assert!(!program.eligible(&interp));
        interp.set_variable("a".into(), "2".into());
        interp.flags.insert(BashFlags::NOUNSET);
        assert!(!program.eligible(&interp));
    }
}
