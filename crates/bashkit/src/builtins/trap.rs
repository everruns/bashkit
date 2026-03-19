//! Trap builtin — register signal/event handlers.
//!
//! Directly mutates traps via [`ShellRef`](super::ShellRef).

use async_trait::async_trait;

use super::{Builtin, Context};
use crate::error::Result;
use crate::interpreter::ExecResult;

/// `trap` builtin — register signal/event handlers.
///
/// Usage:
/// - `trap` — list all traps
/// - `trap -p [SIGNAL...]` — print trap commands
/// - `trap COMMAND SIGNAL...` — set trap handler
/// - `trap - SIGNAL...` — remove trap handler
/// - `trap SIGNAL` (single arg) — remove trap for SIGNAL
pub struct Trap;

#[async_trait]
impl Builtin for Trap {
    async fn execute(&self, mut ctx: Context<'_>) -> Result<ExecResult> {
        let Some(shell) = ctx.shell.as_mut() else {
            return Ok(ExecResult::ok(String::new()));
        };

        if ctx.args.is_empty() {
            // List all traps
            let mut sorted: Vec<_> = shell.traps.iter().collect();
            sorted.sort_by_key(|(sig, _)| (*sig).clone());
            let mut output = String::new();
            for (sig, cmd) in sorted {
                output.push_str(&format!("trap -- '{}' {}\n", cmd, sig));
            }
            return Ok(ExecResult::ok(output));
        }

        // Handle -p flag (print traps)
        if ctx.args[0] == "-p" {
            let mut output = String::new();
            if ctx.args.len() == 1 {
                let mut sorted: Vec<_> = shell.traps.iter().collect();
                sorted.sort_by_key(|(sig, _)| (*sig).clone());
                for (sig, cmd) in sorted {
                    output.push_str(&format!("trap -- '{}' {}\n", cmd, sig));
                }
            } else {
                for sig in &ctx.args[1..] {
                    let sig_upper = sig.to_uppercase();
                    if let Some(cmd) = shell.traps.get(&sig_upper) {
                        output.push_str(&format!("trap -- '{}' {}\n", cmd, sig_upper));
                    }
                }
            }
            return Ok(ExecResult::ok(output));
        }

        if ctx.args.len() == 1 {
            let sig = ctx.args[0].to_uppercase();
            shell.traps.remove(&sig);
        } else {
            let cmd = ctx.args[0].clone();
            for sig in &ctx.args[1..] {
                let sig_upper = sig.to_uppercase();
                if cmd == "-" {
                    shell.traps.remove(&sig_upper);
                } else {
                    shell.traps.insert(sig_upper, cmd.clone());
                }
            }
        }

        Ok(ExecResult::ok(String::new()))
    }
}
