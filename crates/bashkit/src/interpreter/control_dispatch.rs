//! One definition of shell orchestration for ordinary and prepared execution.
#[cfg(feature = "experimental_execution_plan")]
use super::*;

macro_rules! control_body {
    (plain, $shell:ident, $commands:expr, $index:expr) => {
        $shell.execute_command_sequence($commands)
    };
    (prepared, $shell:ident, $commands:expr, $index:expr, $prepared:ident) => {
        $prepared.sequence($shell, $commands, $index, false)
    };
}
macro_rules! control_condition {
    (plain, $shell:ident, $commands:expr, $index:expr) => {
        $shell.execute_condition_sequence($commands)
    };
    (prepared, $shell:ident, $commands:expr, $index:expr, $prepared:ident) => {
        $prepared.sequence($shell, $commands, $index, true)
    };
}
macro_rules! control_clause {
    (plain, $shell:ident, $source:expr, $index:expr, $writes:expr) => {{
        let text = $shell.arith_for_expr($source).await?;
        if $writes {
            $shell.execute_arithmetic_with_side_effects(&text)
        } else {
            $shell.evaluate_arithmetic(&text)
        }
    }};
    (prepared, $shell:ident, $source:expr, $index:expr, $writes:expr, $prepared:ident) => {
        $shell
            .evaluate_prepared_for_clause($source, $prepared.clause($index), $writes)
            .await?
    };
}
macro_rules! control_compound {
    (plain, $shell:ident, $compound:ident) => {
        $shell.execute_compound($compound)
    };
    (prepared, $shell:ident, $compound:ident, $prepared:ident) => {
        $shell.execute_control_compound($compound, $prepared)
    };
}

macro_rules! sequence_yield {
    (plain, $shell:ident) => {};
    (prepared, $shell:ident) => {
        if $shell.counters.commands > 0
            && $shell.counters.commands.is_multiple_of(64)
            && $shell.execution_output().is_none()
        {
            crate::execution::yield_once().await;
        }
    };
}

macro_rules! sequence_body {
    ($kind:ident, $shell:ident, $commands:ident, $check_errexit:ident $(, $prepared:ident)?) => {{
        let mut stdout = crate::StreamData::new();
        let mut stderr = crate::StreamData::new();
        let mut exit_code = 0;
        let mut last_errexit_suppressed = false;

        for (_index, command) in $commands.iter().enumerate() {
            sequence_yield!($kind, $shell);
            let emit_before = $shell.output_emit_count;
            $shell.sequence_accum = (stdout.len(), stderr.len());
            let mut result = list_command!($kind, $shell, command, _index $(, $prepared)?).await?;
            if $shell.merge_stderr {
                $shell.merge_stderr_into_stdout(&mut result)?;
            }
            $shell.reap_coprocs();
            $shell.maybe_emit_output(&result.stdout, &result.stderr, emit_before)
                .await?;
            if $shell.retain_streamed_output() {
                $shell.append_output(&mut stdout, &mut stderr, &result.stdout, &result.stderr, false)?;
            }
            exit_code = result.exit_code;
            $shell.last_exit_code = exit_code;

            // Propagate control flow
            if result.control_flow != ControlFlow::None {
                return Ok(ExecResult {
                    stdout,
                    stderr,
                    exit_code,
                    control_flow: result.control_flow,
                    ..Default::default()
                });
            }

            // Check for errexit (set -e) if enabled.
            // Suppression is decided by the callee and surfaced through
            // result.errexit_suppressed (e.g. AND-OR lists).
            let suppress = result.errexit_suppressed;
            if $check_errexit && $shell.errexit_active() && exit_code != 0 && !suppress {
                $shell.errexit_fired = true;
                return Ok(ExecResult {
                    stdout,
                    stderr,
                    exit_code,
                    control_flow: ControlFlow::None,
                    ..Default::default()
                });
            }
            last_errexit_suppressed = suppress && exit_code != 0;
        }

        Ok(ExecResult {
            stdout,
            stderr,
            exit_code,
            control_flow: ControlFlow::None,
            errexit_suppressed: last_errexit_suppressed,
            ..Default::default()
        })
    }};
}

macro_rules! arithmetic_for_body {
    ($kind:ident, $shell:ident, $arith_for:ident $(, $prepared:ident)?) => {{
        let mut acc = state::LoopAccumulator::new($shell.retain_streamed_output());
        // DEBUG output of the clauses (bash runs it before each one), put
        // ahead of the next body's output or the loop's result.
        let mut debug: Option<Box<DebugTrapOutput>> = None;

        // Execute initialization
        if !$arith_for.init.is_empty() {
            if $shell.has_debug_trap() {
                $shell.current_line = $shell.line_at($arith_for.span.line());
                Self::absorb_debug(&mut debug, $shell.debug_trap_now().await);
                if let Some(r) = DebugTrapOutput::exit_result(&mut debug) {
                    return Ok(r);
                }
            }
            control_clause!($kind, $shell, &$arith_for.init, 0, true $(, $prepared)?);
        }

        $shell.counters.enter_loop();
        let result = async {
            loop {
                // Check loop iteration limit
                $shell.counters.tick_loop(&$shell.limits)?;

                // Check condition (if empty, always true)
                // The clauses see the `for` line, not the body's last one.
                let for_line = $shell.line_at($arith_for.span.line());
                $shell.current_line = for_line;
                if $shell.has_debug_trap() {
                    Self::absorb_debug(&mut debug, $shell.debug_trap_now().await);
                    if let Some(r) = DebugTrapOutput::exit_result(&mut debug) {
                        return Ok(r);
                    }
                }
                if !$arith_for.condition.is_empty() {
                    let cond_result = control_clause!($kind, $shell, &$arith_for.condition, 1, false $(, $prepared)?);
                    if cond_result == 0 {
                        break;
                    }
                }

                // Execute body
                let emit_before = $shell.output_emit_count;
                let result = control_body!($kind, $shell, &$arith_for.body, 0 $(, $prepared)?).await;
                let result = DebugTrapOutput::prepend_opt(debug.take(), result)?;
                $shell.maybe_emit_output(&result.stdout, &result.stderr, emit_before)
                    .await?;
                let should_errexit = $shell.errexit_active()
                    && result.exit_code != 0
                    && result.control_flow == ControlFlow::None
                    && !result.errexit_suppressed;
                match acc.accumulate(result, &$shell.execution_budget)? {
                    state::LoopAction::None | state::LoopAction::Continue => {
                        if should_errexit {
                            return Ok(acc.finish());
                        }
                    }
                    state::LoopAction::Break => break,
                    state::LoopAction::Exit(r) => return Ok(*r),
                }

                // Execute step
                if !$arith_for.step.is_empty() {
                    $shell.current_line = for_line;
                    if $shell.has_debug_trap() {
                        Self::absorb_debug(&mut debug, $shell.debug_trap_now().await);
                        if let Some(r) = DebugTrapOutput::exit_result(&mut debug) {
                            return Ok(r);
                        }
                    }
                    control_clause!($kind, $shell, &$arith_for.step, 2, true $(, $prepared)?);
                }
            }

            Ok(acc.finish())
        }
        .await;
        $shell.counters.exit_loop();
        DebugTrapOutput::prepend_opt(debug, result)
    }};
}

macro_rules! for_body {
    ($kind:ident, $shell:ident, $for_cmd:ident $(, $prepared:ident)?) => {{
        // Validate for-loop variable name (bash rejects invalid names at runtime, exit 1)
        if !is_valid_var_name(&$for_cmd.variable) {
            return Ok(ExecResult::err(
                $shell.diag(format!("`{}': not a valid identifier\n", $for_cmd.variable)),
                1,
            ));
        }

        let mut acc = state::LoopAccumulator::new($shell.retain_streamed_output());

        // Get iteration values: expand fields, then apply brace/glob expansion
        let values: Vec<String> = if let Some(words) = &$for_cmd.words {
            let mut vals = Vec::new();
            for w0 in words {
                // Brace expansion runs first, on the unexpanded word.
                let braced = $shell.brace_expand_word(w0);
                for w in braced.as_deref().unwrap_or(std::slice::from_ref(w0)) {
                    let fields = $shell.expand_word_to_fields(w).await?;

                    // Quoted words skip brace/glob expansion — unless the
                    // word has unquoted glob chars (e.g. `"$var"*.ext`)
                    if w.quoted && !w.has_unquoted_glob {
                        vals.extend(fields);
                        continue;
                    }

                    for expanded in fields {
                        for item in [expanded] {
                            match $shell
                                .expand_glob_item(&item, w.quoted && w.has_unquoted_glob)
                                .await
                            {
                                Ok(items) => vals.extend(items),
                                Err(pat) => {
                                    $shell.last_exit_code = 1;
                                    return Ok(ExecResult::err(
                                        $shell.diag(format!("no match: {pat}\n")),
                                        1,
                                    ));
                                }
                            }
                        }
                    }
                }
            }
            vals
        } else {
            // No words specified - iterate over positional parameters ($@)
            $shell.call_stack
                .last()
                .map(|frame| frame.positional.clone())
                .unwrap_or_default()
        };

        $shell.counters.enter_loop();
        let result = async {
            for value in values {
                // Check loop iteration limit
                $shell.counters.tick_loop(&$shell.limits)?;

                // Set loop variable (respects nameref). `value` is moved
                // straight into `set_variable` — previously we cloned it
                // even though `values` already owned the String for us.
                // A nameref control variable is re-pointed at each word
                // (bash: `declare -n r; for r in a b` binds r to a, then b).
                if $shell.scoped.namerefs.contains_key(&$for_cmd.variable)
                    && declare::valid_nameref_target(&value)
                {
                    $shell.set_nameref(&$for_cmd.variable, value);
                } else {
                    $shell.set_variable($for_cmd.variable.clone(), value);
                }

                // DEBUG runs for each round, at the `for` line.
                let mut debug = if $shell.has_debug_trap() {
                    $shell.current_line = $shell.line_at($for_cmd.span.line());
                    $shell.debug_trap_now().await
                } else {
                    None
                };
                if let Some(r) = DebugTrapOutput::exit_result(&mut debug) {
                    return Ok(r);
                }

                // Execute body
                let emit_before = $shell.output_emit_count;
                let result = control_body!($kind, $shell, &$for_cmd.body, 0 $(, $prepared)?).await;
                let result = DebugTrapOutput::prepend_opt(debug, result)?;
                $shell.maybe_emit_output(&result.stdout, &result.stderr, emit_before)
                    .await?;
                let should_errexit = $shell.errexit_active()
                    && result.exit_code != 0
                    && result.control_flow == ControlFlow::None
                    && !result.errexit_suppressed;
                match acc.accumulate(result, &$shell.execution_budget)? {
                    state::LoopAction::None => {
                        if should_errexit {
                            return Ok(acc.finish());
                        }
                    }
                    state::LoopAction::Break => break,
                    state::LoopAction::Continue => continue,
                    state::LoopAction::Exit(r) => return Ok(*r),
                }
            }

            Ok(acc.finish())
        }
        .await;
        $shell.counters.exit_loop();
        result
    }};
}

macro_rules! condition_loop_body {
    ($kind:ident, $shell:ident, $condition:ident, $body:ident, $break_on_zero:ident $(, $prepared:ident)?) => {{
        let mut acc = state::LoopAccumulator::new($shell.retain_streamed_output());

        $shell.counters.enter_loop();
        let result = async {
            loop {
                // Check loop iteration limit
                $shell.counters.tick_loop(&$shell.limits)?;

                // Check $condition (no errexit - conditions are expected to fail)
                let emit_before_cond = $shell.output_emit_count;
                let condition_result = control_condition!($kind, $shell, $condition, 0 $(, $prepared)?).await?;
                // Condition commands produce visible output (e.g., `while cat <<EOF; do ... done`)
                $shell.maybe_emit_output(
                    &condition_result.stdout,
                    &condition_result.stderr,
                    emit_before_cond,
                )
                .await?;
                acc.append_output(&condition_result, &$shell.execution_budget)?;
                // `while break; do` leaves the loop from its $condition.
                if condition_result.control_flow != ControlFlow::None {
                    match acc.accumulate(ExecResult {
                        exit_code: condition_result.exit_code,
                        control_flow: condition_result.control_flow,
                        ..Default::default()
                    }, &$shell.execution_budget)? {
                        state::LoopAction::None => {}
                        state::LoopAction::Break => break,
                        state::LoopAction::Continue => continue,
                        state::LoopAction::Exit(r) => return Ok(*r),
                    }
                }
                let should_break = if $break_on_zero {
                    condition_result.exit_code == 0
                } else {
                    condition_result.exit_code != 0
                };
                if should_break {
                    break;
                }

                // Execute $body
                let emit_before = $shell.output_emit_count;
                let result = control_body!($kind, $shell, $body, 1 $(, $prepared)?).await?;
                $shell.maybe_emit_output(&result.stdout, &result.stderr, emit_before)
                    .await?;
                let should_errexit = $shell.errexit_active()
                    && result.exit_code != 0
                    && result.control_flow == ControlFlow::None
                    && !result.errexit_suppressed;
                match acc.accumulate(result, &$shell.execution_budget)? {
                    state::LoopAction::None => {
                        if should_errexit {
                            return Ok(acc.finish());
                        }
                    }
                    state::LoopAction::Break => break,
                    state::LoopAction::Continue => continue,
                    state::LoopAction::Exit(r) => return Ok(*r),
                }
            }

            Ok(acc.finish())
        }
        .await;
        $shell.counters.exit_loop();
        result
    }};
}

macro_rules! if_body {
    ($kind:ident, $shell:ident, $if_cmd:ident $(, $prepared:ident)?) => {{
        // Accumulate stdout/stderr from all condition evaluations
        let mut cond_stdout = crate::StreamData::new();
        let mut cond_stderr = crate::StreamData::new();

        // Execute condition (no errexit checking - conditions are expected to fail)
        let condition_result = control_condition!($kind, $shell, &$if_cmd.condition, 0 $(, $prepared)?).await?;
        $shell.append_output(&mut cond_stdout, &mut cond_stderr, &condition_result.stdout, &condition_result.stderr, false)?;
        // `exit`/`return`/`break` (or a fatal expansion error) inside the
        // condition ends the `if` with that control flow.
        if condition_result.control_flow != ControlFlow::None {
            return Ok(ExecResult {
                stdout: cond_stdout,
                stderr: cond_stderr,
                exit_code: condition_result.exit_code,
                control_flow: condition_result.control_flow,
                ..Default::default()
            });
        }

        if condition_result.exit_code == 0 {
            // Condition succeeded, execute then branch
            let mut result = control_body!($kind, $shell, &$if_cmd.then_branch, 1 $(, $prepared)?).await?;
            $shell.append_output(&mut cond_stdout, &mut cond_stderr, &result.stdout, &result.stderr, false)?;
            result.stdout = cond_stdout;
            result.stderr = cond_stderr;
            return Ok(result);
        }

        // Check elif branches
        for (_index, (elif_condition, elif_body)) in $if_cmd.elif_branches.iter().enumerate() {
            let elif_result = control_condition!($kind, $shell, elif_condition, 2 + _index * 2 $(, $prepared)?).await?;
            $shell.append_output(&mut cond_stdout, &mut cond_stderr, &elif_result.stdout, &elif_result.stderr, false)?;
            if elif_result.control_flow != ControlFlow::None {
                return Ok(ExecResult {
                    stdout: cond_stdout,
                    stderr: cond_stderr,
                    exit_code: elif_result.exit_code,
                    control_flow: elif_result.control_flow,
                    ..Default::default()
                });
            }

            if elif_result.exit_code == 0 {
                let mut result = control_body!($kind, $shell, elif_body, 3 + _index * 2 $(, $prepared)?).await?;
                $shell.append_output(&mut cond_stdout, &mut cond_stderr, &result.stdout, &result.stderr, false)?;
                result.stdout = cond_stdout;
                result.stderr = cond_stderr;
                return Ok(result);
            }
        }

        // Execute else branch if present
        if let Some(else_branch) = &$if_cmd.else_branch {
            let mut result = control_body!($kind, $shell, else_branch, 2 + $if_cmd.elif_branches.len() * 2 $(, $prepared)?).await?;
            $shell.append_output(&mut cond_stdout, &mut cond_stderr, &result.stdout, &result.stderr, false)?;
            result.stdout = cond_stdout;
            result.stderr = cond_stderr;
            return Ok(result);
        }

        // No branch executed, return condition output with success exit code
        Ok(ExecResult {
            stdout: cond_stdout,
            stderr: cond_stderr,
            exit_code: 0,
            ..Default::default()
        })
    }};
}

macro_rules! compound_redirect_body {
    ($kind:ident, $shell:ident, $compound:ident, $redirects:ident $(, $prepared:ident)?) => {{
        Box::pin(async move {
            let end_line = (!$redirects.is_empty())
                .then(|| Self::compound_end_line($compound))
                .flatten();
            if let Some(line) = end_line {
                $shell.current_line = line;
            }
            // Process input redirections before executing $compound.
            // WTF: a coproc fd as a $compound's stdin (`while read l; do
            // ...; done <&${C[0]}`) is read to end of input up front, not
            // line by line as the loop runs.
            let stdin_setup = match $shell
                .process_input_redirections(None, $redirects, StdinDemand::All, HighFdStdin::Any)
                .await
            {
                Ok(s) => s,
                Err(crate::error::Error::CommandFailure(msg)) => {
                    return Ok(ExecResult::err(msg, 1));
                }
                Err(e) => return Err(e),
            };
            // Keep stdin's byte/text storage admitted through the body.
            let _stdin_storage = stdin_setup.stdin_lease;
            let stdin = stdin_setup.stdin;
            let prev_pipeline_stdin = if stdin.is_some() {
                let prev = $shell.pipeline_stdin.take();
                $shell.pipeline_stdin = stdin;
                Some(prev)
            } else {
                None
            };

            // Suspend output callback while output $redirects are active
            // so that maybe_emit_output inside the $compound body does not
            // leak output that will be redirected (e.g. `{ cmd; } 2>/dev/null`).
            let region = $shell.enter_output_region($redirects);

            let has_dup_output = $redirects.iter().any(|r| r.kind == RedirectKind::DupOutput);
            let has_file_redirect = $redirects.iter().any(|r| {
                matches!(
                    r.kind,
                    RedirectKind::Output
                        | RedirectKind::Clobber
                        | RedirectKind::Append
                        | RedirectKind::OutputBoth
                )
            });
            let capture_pending_fd = (has_dup_output && has_file_redirect)
                || redirection::has_high_fd_file_redirect($redirects);
            if capture_pending_fd {
                if $shell.pending_fd_capture_depth == 0 {
                    $shell.clear_pending_fd_redirect_state();
                }
                $shell.pending_fd_capture_depth += 1;
            }
            let result = control_compound!($kind, $shell, $compound $(, $prepared)?).await;
            if capture_pending_fd {
                $shell.pending_fd_capture_depth = $shell.pending_fd_capture_depth.saturating_sub(1);
                if result.is_err() {
                    $shell.clear_pending_fd_redirect_state();
                }
            }
            // Restore callback before applying redirections
            $shell.leave_output_region(region);
            let result = result?;

            if let Some(prev) = prev_pipeline_stdin {
                $shell.pipeline_stdin = prev;
            }
            if $redirects.is_empty() {
                Ok(result)
            } else {
                if let Some(line) = end_line {
                    $shell.current_line = line;
                }
                $shell.apply_redirections(result, $redirects).await
            }
        })
    }};
}

#[cfg(feature = "experimental_execution_plan")]
impl Interpreter {
    pub(super) async fn execute_prepared_sequence(
        &mut self,
        commands: &[Command],
        condition: bool,
        prepared: script_plan::PreparedCommands<'_, '_>,
    ) -> Result<ExecResult> {
        self.condition_sequence_depth += usize::from(condition);
        let check_errexit = !condition;
        let result =
            async { sequence_body!(prepared, self, commands, check_errexit, prepared) }.await;
        self.condition_sequence_depth -= usize::from(condition);
        result
    }

    pub(super) fn execute_prepared_compound<'a>(
        &'a mut self,
        compound: &'a CompoundCommand,
        redirects: &'a [Redirect],
        prepared: script_plan::PreparedControl<'a, 'a>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<ExecResult>> + Send + 'a>> {
        Box::pin(async move {
            // Prepared compounds have no own redirects, but retain the same
            // lexical fd-scope boundary as the ordinary compound wrapper.
            let scope_len = self.fd_redirect_scope.len();
            let shell = &mut *self;
            let result =
                compound_redirect_body!(prepared, shell, compound, redirects, prepared).await;
            self.fd_redirect_scope.truncate(scope_len);
            result
        })
    }

    pub(super) async fn execute_control_compound(
        &mut self,
        compound: &CompoundCommand,
        prepared: script_plan::PreparedControl<'_, '_>,
    ) -> Result<ExecResult> {
        let is_loop = matches!(
            compound,
            CompoundCommand::For(_)
                | CompoundCommand::ArithmeticFor(_)
                | CompoundCommand::While(_)
                | CompoundCommand::Until(_)
        );
        self.loop_depth += usize::from(is_loop);
        let result = async {
            match compound {
                CompoundCommand::BraceGroup(commands) => {
                    prepared.sequence(self, commands, 0, false).await
                }
                CompoundCommand::For(for_cmd) => {
                    for_body!(prepared, self, for_cmd, prepared)
                }
                CompoundCommand::ArithmeticFor(arith_for) => {
                    arithmetic_for_body!(prepared, self, arith_for, prepared)
                }
                CompoundCommand::While(_) | CompoundCommand::Until(_) => {
                    let (condition, body, break_on_zero) = match compound {
                        CompoundCommand::While(w) => {
                            (w.condition.as_slice(), w.body.as_slice(), false)
                        }
                        CompoundCommand::Until(w) => {
                            (w.condition.as_slice(), w.body.as_slice(), true)
                        }
                        _ => unreachable!(),
                    };
                    condition_loop_body!(prepared, self, condition, body, break_on_zero, prepared)
                }
                CompoundCommand::If(if_cmd) => {
                    if_body!(prepared, self, if_cmd, prepared)
                }
                _ => unreachable!("prepared compound kind"),
            }
        }
        .await;
        self.loop_depth -= usize::from(is_loop);
        result
    }

    pub(super) async fn evaluate_prepared_for_clause(
        &mut self,
        source: &str,
        expression: Option<&arithmetic::program::Expression<'_>>,
        writes: bool,
    ) -> Result<i64> {
        if let Some(expression) = expression
            && script_plan::native_context(self)
            && expression
                .names()
                .all(|name| script_plan::plain_variable(self, name))
        {
            return Ok(match self.evaluate_program_expression(expression, writes) {
                Ok(value) => value,
                Err(message) => {
                    self.record_arith_error(message);
                    0
                }
            });
        }
        let text = self.arith_for_expr(source).await?;
        Ok(if writes {
            self.execute_arithmetic_with_side_effects(&text)
        } else {
            self.evaluate_arithmetic(&text)
        })
    }
}

#[cfg(feature = "experimental_execution_plan")]
impl Interpreter {
    #[inline]
    pub(super) fn execution_plan_enabled(&self) -> bool {
        #[cfg(feature = "experimental_plan_benchmark_control")]
        return self
            .current_execution_extensions()
            .get::<crate::ExperimentalPlanBenchmarkControl>()
            .and_then(|mode| {
                mode.try_with(|mode| matches!(mode, crate::ExperimentalPlanBenchmarkControl::Auto))
                    .ok()
            })
            .unwrap_or(true);
        #[cfg(not(feature = "experimental_plan_benchmark_control"))]
        true
    }

    pub(super) fn execute_planned_function_body<'a>(
        &'a mut self,
        body: &'a Command,
    ) -> command_dispatch::CommandFuture<'a> {
        // Tiny ordinary functions keep the original single boxed command
        // future. Only profitable bodies allocate a compiler-owned frame.
        let mut weight = 0;
        if !self.execution_plan_enabled() || !script_plan::worthwhile(body, &mut weight, 0) {
            return self.execute_command(body);
        }
        Box::pin(async move {
            // The definition stays borrowed through this invocation. Errors
            // return through the caller's normal function-frame cleanup.
            let plan = script_plan::ScriptPlan::compile_commands(
                std::slice::from_ref(body),
                &self.execution_budget,
            )?;
            self.execute_command_with_program(body, plan.as_ref().and_then(|plan| plan.command(0)))
                .await
        })
    }
}
