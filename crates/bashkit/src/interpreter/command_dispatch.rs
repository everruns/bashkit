//! Shared command boundaries expanded into separate ordinary/compiled futures.
//! Keeping the AST future free of plan state avoids taxing fallback commands.
use super::*;

pub(super) type CommandFuture<'a> =
    std::pin::Pin<Box<dyn std::future::Future<Output = Result<ExecResult>> + Send + 'a>>;

pub(super) trait ListCommands: Sync {
    fn execute<'a>(
        &'a self,
        shell: &'a mut Interpreter,
        command: &'a Command,
        index: usize,
    ) -> CommandFuture<'a>;
}

pub(super) struct PlainCommands;
impl ListCommands for PlainCommands {
    fn execute<'a>(
        &'a self,
        shell: &'a mut Interpreter,
        command: &'a Command,
        _index: usize,
    ) -> CommandFuture<'a> {
        shell.execute_command(command)
    }
}

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
