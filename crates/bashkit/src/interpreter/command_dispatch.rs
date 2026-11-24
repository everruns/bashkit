//! Shared command boundaries expanded into separate ordinary/compiled futures.
//! Keeping the AST future free of plan state avoids taxing fallback commands.
use super::*;

pub(super) type CommandFuture<'a> =
    std::pin::Pin<Box<dyn std::future::Future<Output = Result<ExecResult>> + Send + 'a>>;

// One boundary definition: guards, accounting, pipes, diagnostics and ERR traps
// stay identical while each async frame contains only its own execution body.
macro_rules! command_future {
    ($shell:ident, $command:ident, $body:expr) => {
        // THREAT[TM-DOS-142]: every nesting level passes through here, so
        // each poll gets enough native stack (see grow_stack.rs).
        Box::pin(grow_stack::GrowStack::new(async move {
            // Whether ERR may fire after this command: bash decides before
            // running it, so a trap the command itself sets (`f` setting
            // ERR inside) does not fire for that same command.
            if $shell.counters.commands > 0
                && $shell.counters.commands.is_multiple_of(64)
                && $shell.execution_output().is_some()
            {
                crate::execution::yield_once().await;
            }
            let skip_stage = std::mem::take(&mut $shell.err_trap_skip_stage);
            let err_armed =
                !skip_stage && $shell.err_trap_armed() && Self::fires_err_trap($command);
            let emit_before = $shell.output_emit_count;
            $shell.check_cancelled()?;
            // Update current line for $LINENO
            $shell.set_command_lineno($command);
            if let Some(name) = Self::command_name($command) {
                $shell.last_command_name = name.to_string();
            }

            // `set -n`: read commands without running them for the rest of
            // this exec (bash does the same in scripts; nothing can undo it,
            // since `set +n` is not run either).
            if $shell.flags.contains(BashFlags::NOEXEC) {
                return Ok(ExecResult::ok(String::new()));
            }

            // Fail point: inject failures during command execution
            #[cfg(feature = "failpoints")]
            fail_point!("interp::execute_command", |action| {
                match action.as_deref() {
                    Some("panic") => {
                        // Test panic recovery
                        panic!("injected panic in execute_command");
                    }
                    Some("error") => {
                        return Err(Error::Execution("injected execution error".to_string()));
                    }
                    Some("exit_nonzero") => {
                        // Return non-zero exit code without error
                        return Ok(ExecResult {
                            stdout: crate::StreamData::new(),
                            stderr: "injected failure".into(),
                            exit_code: 127,
                            control_flow: ControlFlow::None,
                            ..Default::default()
                        });
                    }
                    _ => {}
                }
                Ok(ExecResult::ok(String::new()))
            });

            $shell.charge_command_execution()?;

            if $shell.pipe_out.is_some()
                && let Some(killed) = Box::pin($shell.wait_for_pipe_room()).await
            {
                return Ok(killed);
            }

            let result = $body;
            let mut result = result;
            $shell.settle_pending_subst_stderr(&mut result);
            let mut result = $shell.abort_line_on_error(result);
            if err_armed
                && let Ok(r) = &mut result
                && r.exit_code != 0
                && r.control_flow == ControlFlow::None
                && !r.errexit_suppressed
                && !$shell.is_in_condition_sequence()
            {
                // `$LINENO` in the handler is the failing command's line.
                $shell.set_command_lineno($command);
                Box::pin($shell.run_err_trap_after(r, emit_before)).await;
            }
            result
        }))
    };
}

// Keep the ordinary list future's original structure. Only prepared lists carry
// a selector; the orchestration itself still has a single definition.
macro_rules! list_command {
    (plain,$shell:ident,$command:expr,$index:expr) => {
        $shell.execute_command($command)
    };
    (prepared,$shell:ident,$command:expr,$index:expr,$commands:ident) => {
        $commands.execute($shell, $command, $index)
    };
}
macro_rules! list_body {
    ($kind:ident,$shell:ident,$list:ident,$route:ident $(,$commands:ident)?)=> {{
        let mut stdout = crate::StreamData::new();
        let mut stderr = crate::StreamData::new();
        let mut exit_code;
        let mut control_flow;
        let mut exit_code_from_conditional_context = false;

        // Determine if the first command should run in the background.
        // The `&` terminator for first appears as rest[0].op == Background.
        let first_is_bg = matches!($list.rest.first(), Some((ListOperator::Background, _)));

        if first_is_bg {
            // Boxed: the job path holds a forked Interpreter across an await,
            // which would otherwise bloat every recursive execute_list frame.
            Box::pin($shell.spawn_in_background(&$list.first, &mut stdout, &mut stderr, $route)).await?;
            exit_code = 0;
            control_flow = ControlFlow::None;
            exit_code_from_conditional_context = false;
        } else {
            let emit_before = $shell.output_emit_count;
            let emitted_before = (
                $shell.output_stream_stdout_bytes,
                $shell.output_stream_stderr_bytes,
            );
            $shell.sequence_accum = (0, 0);
            // A non-final `&&`/`||` element runs with errexit ignored.
            let conditional = $list
                .rest
                .first()
                .is_some_and(|(op, _)| matches!(op, ListOperator::And | ListOperator::Or));
            $shell.condition_sequence_depth += usize::from(conditional);
            let result = list_command!($kind, $shell, &$list.first, 0 $(, $commands)?).await;
            $shell.condition_sequence_depth -= usize::from(conditional);
            let mut result = result?;
            if $route {
                Box::pin($shell.route_exec_output(
                    &mut result,
                    emitted_before,
                    (0, 0),
                    Self::command_name(&$list.first),
                ))
                .await?;
            }
            if $shell.merge_stderr {
                $shell.merge_stderr_into_stdout(&mut result)?;
            }
            $shell.maybe_emit_output(&result.stdout, &result.stderr, emit_before)
                .await?;
            if $route {
                Box::pin($shell.flush_exec_passthrough(&mut result)).await?;
            }
            if $shell.retain_streamed_output() {
                $shell.append_output(&mut stdout, &mut stderr, &result.stdout, &result.stderr, $route)?;
            }
            exit_code = result.exit_code;
            $shell.last_exit_code = exit_code;
            control_flow = result.control_flow;
            exit_code_from_conditional_context = result.errexit_suppressed
                || $list
                    .rest
                    .first()
                    .is_some_and(|(op, _)| matches!(op, ListOperator::And | ListOperator::Or));

            // If first command signaled control flow, return immediately
            if control_flow != ControlFlow::None {
                return Ok(ExecResult {
                    stdout,
                    stderr,
                    exit_code,
                    control_flow,
                    ..Default::default()
                });
            }
        }

        for (i, (op, cmd)) in $list.rest.iter().enumerate() {
            // Skip empty sentinel commands (produced by trailing `&`)
            if Self::is_empty_sentinel(cmd) {
                continue;
            }

            // Check if this command is followed by another && / || operator.
            // POSIX `errexit` suppression applies to non-final commands in an
            // AND-OR list; the final executed command can still abort on failure.
            let current_is_conditional = matches!(op, ListOperator::And | ListOperator::Or);

            // Determine if THIS command should be backgrounded.
            // A command is backgrounded when the NEXT separator is Background
            // (the `&` terminates the current command).
            let should_background =
                matches!($list.rest.get(i + 1), Some((ListOperator::Background, _)));

            // Check errexit before executing next semicolon-separated command:
            // if previous command failed outside conditional context, exit now.
            let should_check_errexit = matches!(op, ListOperator::Semicolon)
                && $shell.errexit_active()
                && exit_code != 0
                && !exit_code_from_conditional_context;

            if should_check_errexit {
                return Ok(ExecResult {
                    stdout,
                    stderr,
                    exit_code,
                    control_flow: ControlFlow::None,
                    ..Default::default()
                });
            }

            let should_execute = match op {
                ListOperator::And => exit_code == 0,
                ListOperator::Or => exit_code != 0,
                ListOperator::Semicolon | ListOperator::Background => true,
            };

            if !should_execute && current_is_conditional {
                // Short-circuited && / ||: the carried exit code came from
                // a conditional chain, so errexit must not fire on it.
                exit_code_from_conditional_context = true;
            }

            if should_execute {
                if should_background {
                    Box::pin($shell.spawn_in_background(cmd, &mut stdout, &mut stderr, $route)).await?;
                    exit_code = 0;
                    exit_code_from_conditional_context = false;
                } else {
                    let emit_before = $shell.output_emit_count;
                    let emitted_before = (
                        $shell.output_stream_stdout_bytes,
                        $shell.output_stream_stderr_bytes,
                    );
                    $shell.sequence_accum = (stdout.len(), stderr.len());
                    let followed_by_conditional_op =
                        $list.rest.get(i + 1).is_some_and(|(op, cmd)| {
                            !Self::is_empty_sentinel(cmd)
                                && matches!(op, ListOperator::And | ListOperator::Or)
                        });
                    $shell.condition_sequence_depth += usize::from(followed_by_conditional_op);
                    let result = list_command!($kind, $shell, cmd, i + 1 $(, $commands)?).await;
                    $shell.condition_sequence_depth -= usize::from(followed_by_conditional_op);
                    let mut result = result?;
                    if $route {
                        Box::pin($shell.route_exec_output(
                            &mut result,
                            emitted_before,
                            (0, 0),
                            Self::command_name(cmd),
                        ))
                        .await?;
                    }
                    if $shell.merge_stderr {
                        $shell.merge_stderr_into_stdout(&mut result)?;
                    }
                    $shell.maybe_emit_output(&result.stdout, &result.stderr, emit_before)
                        .await?;
                    if $route {
                        Box::pin($shell.flush_exec_passthrough(&mut result)).await?;
                    }
                    if $shell.retain_streamed_output() {
                        $shell.append_output(&mut stdout, &mut stderr, &result.stdout, &result.stderr, $route)?;
                    }
                    exit_code = result.exit_code;
                    $shell.last_exit_code = exit_code;
                    control_flow = result.control_flow;
                    // Bash suppresses errexit for AND-OR list elements except the
                    // command following the final &&/|| operator.
                    exit_code_from_conditional_context =
                        followed_by_conditional_op || result.errexit_suppressed;

                    // If command signaled control flow, return immediately
                    if control_flow != ControlFlow::None {
                        return Ok(ExecResult {
                            stdout,
                            stderr,
                            exit_code,
                            control_flow,
                            ..Default::default()
                        });
                    }
                }
            }
        }

        // Final errexit check for the last command. A non-zero status only
        // remains suppressed when it was carried from a short-circuited or
        // non-final AND-OR list element; a failing final &&/|| command exits.
        let should_final_errexit_check = $shell.errexit_active()
            && exit_code != 0
            && !exit_code_from_conditional_context
            && !$shell.is_in_condition_sequence();

        if should_final_errexit_check {
            return Ok(ExecResult {
                stdout,
                stderr,
                exit_code,
                control_flow: ControlFlow::None,
                ..Default::default()
            });
        }

        Ok(ExecResult {
            stdout,
            stderr,
            exit_code,
            control_flow: ControlFlow::None,
            errexit_suppressed: exit_code_from_conditional_context && exit_code != 0,
            ..Default::default()
        })
    }};
}
