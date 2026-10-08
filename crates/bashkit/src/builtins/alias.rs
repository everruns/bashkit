//! Alias management builtins: alias, unalias
//!
//! Directly mutate aliases via [`ShellRef`](super::ShellRef).

use async_trait::async_trait;

use super::helpers::single_quote;
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

        // Leading options: `-p` lists, `--` ends them.
        let mut args: &[String] = ctx.args;
        let mut print_all = false;
        while let Some(arg) = args.first() {
            if arg == "--" {
                args = &args[1..];
                break;
            }
            if arg.len() < 2 || !arg.starts_with('-') {
                break;
            }
            if arg.chars().skip(1).all(|c| c == 'p') {
                print_all = true;
                args = &args[1..];
                continue;
            }
            let bad = arg.chars().nth(1).unwrap_or('-');
            return Ok(ExecResult::err(
                format!(
                    "bash: alias: -{bad}: invalid option\n\
                     alias: usage: alias [-p] [name[=value] ... ]\n"
                ),
                2,
            ));
        }

        let mut print_listing = String::new();
        if args.is_empty() || print_all {
            // List all aliases
            let mut sorted: Vec<_> = shell.aliases.iter().collect();
            sorted.sort_by_key(|(k, _)| (*k).clone());
            let mut output = String::new();
            for (name, value) in sorted {
                output.push_str(&format!("alias {name}={}\n", single_quote(value)));
            }
            if args.is_empty() {
                return Ok(ExecResult::ok(output));
            }
            print_listing = output;
        }

        let mut output = print_listing;
        let mut exit_code = 0;
        let mut stderr = String::new();

        for arg in args {
            if let Some(eq_pos) = arg.find('=') {
                // alias name=value — set directly
                let name = &arg[..eq_pos];
                let value = &arg[eq_pos + 1..];
                shell.aliases.insert(name.to_string(), value.to_string());
            } else {
                // alias name — show the alias
                if let Some(value) = shell.aliases.get(arg.as_str()) {
                    output.push_str(&format!("alias {arg}={}\n", single_quote(value)));
                } else {
                    stderr.push_str(&format!("bash: alias: {}: not found\n", arg));
                    exit_code = 1;
                }
            }
        }

        Ok(ExecResult {
            stdout: output.into(),
            stderr: stderr.into(),
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

        let args = match ctx.args.first().map(String::as_str) {
            Some("--") => &ctx.args[1..],
            _ => ctx.args,
        };
        for arg in args {
            if arg == "-a" {
                shell.aliases.clear();
            } else if shell.aliases.remove(arg.as_str()).is_none() {
                stderr.push_str(&format!("bash: unalias: {}: not found\n", arg));
                exit_code = 1;
            }
        }

        Ok(ExecResult {
            stderr: stderr.into(),
            exit_code,
            ..Default::default()
        })
    }
}
