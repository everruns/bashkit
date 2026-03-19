//! Shell introspection builtins: type, which, hash
//!
//! These builtins query shell metadata (registered builtins, functions, keywords)
//! via [`ShellRef`](super::ShellRef) rather than direct interpreter access.

use async_trait::async_trait;

use super::{Builtin, Context};
use crate::error::Result;
use crate::interpreter::ExecResult;

/// `type` builtin — display information about command type.
///
/// Usage: type [-afptP] name [name ...]
pub struct Type;

#[async_trait]
impl Builtin for Type {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let Some(shell) = ctx.shell.as_ref() else {
            return Ok(ExecResult::err(
                "bash: type: not available in this context\n".to_string(),
                1,
            ));
        };

        if ctx.args.is_empty() {
            return Ok(ExecResult::err(
                "bash: type: usage: type [-afptP] name [name ...]\n".to_string(),
                1,
            ));
        }

        let mut type_only = false; // -t
        let mut path_only = false; // -p
        let mut show_all = false; // -a
        let mut names: Vec<&str> = Vec::new();

        for arg in ctx.args {
            if arg.starts_with('-') && arg.len() > 1 {
                for c in arg[1..].chars() {
                    match c {
                        't' => type_only = true,
                        'p' => path_only = true,
                        'a' => show_all = true,
                        'f' => {} // -f: suppress function lookup (ignored for now)
                        'P' => path_only = true,
                        _ => {
                            return Ok(ExecResult::err(
                                format!(
                                    "bash: type: -{}: invalid option\ntype: usage: type [-afptP] name [name ...]\n",
                                    c
                                ),
                                1,
                            ));
                        }
                    }
                }
            } else {
                names.push(arg);
            }
        }

        let mut output = String::new();
        let mut all_found = true;

        for name in &names {
            let is_func = shell.has_function(name);
            let is_builtin = shell.has_builtin(name);
            let is_kw = shell.is_keyword(name);

            if type_only {
                if is_func {
                    output.push_str("function\n");
                } else if is_kw {
                    output.push_str("keyword\n");
                } else if is_builtin {
                    output.push_str("builtin\n");
                } else {
                    all_found = false;
                }
            } else if path_only {
                if !is_func && !is_builtin && !is_kw {
                    all_found = false;
                }
            } else {
                let mut found_any = false;
                if is_func {
                    output.push_str(&format!("{} is a function\n", name));
                    found_any = true;
                    if !show_all {
                        continue;
                    }
                }
                if is_kw {
                    output.push_str(&format!("{} is a shell keyword\n", name));
                    found_any = true;
                    if !show_all {
                        continue;
                    }
                }
                if is_builtin {
                    output.push_str(&format!("{} is a shell builtin\n", name));
                    found_any = true;
                    if !show_all {
                        continue;
                    }
                }
                if !found_any {
                    output.push_str(&format!("bash: type: {}: not found\n", name));
                    all_found = false;
                }
            }
        }

        let exit_code = if all_found { 0 } else { 1 };
        Ok(ExecResult {
            stdout: output,
            exit_code,
            ..Default::default()
        })
    }
}

/// `which` builtin — locate a command.
///
/// In bashkit's sandboxed environment, builtins are the equivalent of
/// executables on PATH. Reports the name if found.
pub struct Which;

#[async_trait]
impl Builtin for Which {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let Some(shell) = ctx.shell.as_ref() else {
            return Ok(ExecResult::err(String::new(), 1));
        };

        if ctx.args.is_empty() {
            return Ok(ExecResult::ok(String::new()));
        }

        let mut output = String::new();
        let mut all_found = true;

        for name in ctx.args {
            if shell.has_builtin(name) || shell.has_function(name) || shell.is_keyword(name) {
                output.push_str(&format!("{}\n", name));
            } else {
                all_found = false;
            }
        }

        let exit_code = if all_found { 0 } else { 1 };
        Ok(ExecResult {
            stdout: output,
            exit_code,
            ..Default::default()
        })
    }
}

/// `hash` builtin — no-op in sandboxed environment.
///
/// In real bash, `hash` manages the command hash table for PATH lookups.
/// In bashkit's sandboxed environment there is no real PATH search cache,
/// so this is a no-op that always succeeds.
pub struct Hash;

#[async_trait]
impl Builtin for Hash {
    async fn execute(&self, _ctx: Context<'_>) -> Result<ExecResult> {
        Ok(ExecResult::ok(String::new()))
    }
}
