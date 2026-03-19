//! Alias management builtins: alias, unalias
//!
//! Directly mutate aliases via [`ShellRef`](super::ShellRef).

use async_trait::async_trait;

use super::{Builtin, Context};
use crate::error::Result;
use crate::interpreter::ExecResult;

/// `alias` builtin — define or display aliases.
///
/// Usage:
/// - `alias` — list all aliases
/// - `alias name` — show a specific alias
/// - `alias name=value` — define an alias
pub struct Alias;

#[async_trait]
impl Builtin for Alias {
    async fn execute(&self, mut ctx: Context<'_>) -> Result<ExecResult> {
        let Some(shell) = ctx.shell.as_mut() else {
            return Ok(ExecResult::ok(String::new()));
        };

        if ctx.args.is_empty() {
            // List all aliases
            let mut sorted: Vec<_> = shell.aliases.iter().collect();
            sorted.sort_by_key(|(k, _)| (*k).clone());
            let mut output = String::new();
            for (name, value) in sorted {
                output.push_str(&format!("alias {}='{}'\n", name, value));
            }
            return Ok(ExecResult::ok(output));
        }

        let mut output = String::new();
        let mut exit_code = 0;
        let mut stderr = String::new();

        for arg in ctx.args {
            if let Some(eq_pos) = arg.find('=') {
                // alias name=value — set directly
                let name = &arg[..eq_pos];
                let value = &arg[eq_pos + 1..];
                shell.aliases.insert(name.to_string(), value.to_string());
            } else {
                // alias name — show the alias
                if let Some(value) = shell.aliases.get(arg.as_str()) {
                    output.push_str(&format!("alias {}='{}'\n", arg, value));
                } else {
                    stderr.push_str(&format!("bash: alias: {}: not found\n", arg));
                    exit_code = 1;
                }
            }
        }

        Ok(ExecResult {
            stdout: output,
            stderr,
            exit_code,
            ..Default::default()
        })
    }
}

/// `unalias` builtin — remove alias definitions.
///
/// Usage:
/// - `unalias name` — remove alias
/// - `unalias -a` — remove all aliases
pub struct Unalias;

#[async_trait]
impl Builtin for Unalias {
    async fn execute(&self, mut ctx: Context<'_>) -> Result<ExecResult> {
        let Some(shell) = ctx.shell.as_mut() else {
            return Ok(ExecResult::err(
                "bash: unalias: usage: unalias [-a] name [name ...]\n".to_string(),
                2,
            ));
        };

        if ctx.args.is_empty() {
            return Ok(ExecResult::err(
                "bash: unalias: usage: unalias [-a] name [name ...]\n".to_string(),
                2,
            ));
        }

        let mut exit_code = 0;
        let mut stderr = String::new();

        for arg in ctx.args {
            if arg == "-a" {
                shell.aliases.clear();
            } else if shell.aliases.remove(arg.as_str()).is_none() {
                stderr.push_str(&format!("bash: unalias: {}: not found\n", arg));
                exit_code = 1;
            }
        }

        Ok(ExecResult {
            stderr,
            exit_code,
            ..Default::default()
        })
    }
}
