//! Variable manipulation builtins: set, unset, local, shift

use async_trait::async_trait;

use super::{Builtin, Context};
use crate::error::Result;
use crate::interpreter::ExecResult;

/// unset builtin - remove variables
pub struct Unset;

#[async_trait]
impl Builtin for Unset {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        for name in ctx.args {
            ctx.variables.remove(name);
            // Note: env is immutable in our model - environment variables
            // are inherited and can't be unset by the shell
        }
        Ok(ExecResult::ok(String::new()))
    }
}

/// set builtin - set/display shell options and positional parameters
///
/// Currently supports:
/// - `set -e` - exit on error (stored but not enforced yet)
/// - `set -x` - trace mode (stored but not enforced yet)
/// - `set --` - set positional parameters
pub struct Set;

#[async_trait]
impl Builtin for Set {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        if ctx.args.is_empty() {
            // Display all variables
            let mut output = String::new();
            for (name, value) in ctx.variables.iter() {
                output.push_str(&format!("{}={}\n", name, value));
            }
            return Ok(ExecResult::ok(output));
        }

        for arg in ctx.args.iter() {
            if arg == "--" {
                // Set positional parameters (would need call stack access)
                // For now, just consume remaining args
                break;
            } else if arg.starts_with('-') || arg.starts_with('+') {
                // Shell options - store in variables for now
                let enable = arg.starts_with('-');
                for opt in arg.chars().skip(1) {
                    let opt_name = format!("SHOPT_{}", opt);
                    ctx.variables
                        .insert(opt_name, if enable { "1" } else { "0" }.to_string());
                }
            }
        }

        Ok(ExecResult::ok(String::new()))
    }
}

/// shift builtin - shift positional parameters
pub struct Shift;

#[async_trait]
impl Builtin for Shift {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        // Number of positions to shift (default 1)
        let n: usize = ctx.args.first().and_then(|s| s.parse().ok()).unwrap_or(1);

        // In real bash, this shifts the positional parameters
        // For now, we store the shift count for the interpreter to handle
        ctx.variables
            .insert("_SHIFT_COUNT".to_string(), n.to_string());

        Ok(ExecResult::ok(String::new()))
    }
}

/// local builtin - declare local variables in functions
pub struct Local;

#[async_trait]
impl Builtin for Local {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        // Local sets variables in the current function scope
        // The actual scoping is handled by the interpreter's call stack
        for arg in ctx.args {
            if let Some(eq_pos) = arg.find('=') {
                let name = &arg[..eq_pos];
                let value = &arg[eq_pos + 1..];
                // Mark as local by setting it
                ctx.variables.insert(name.to_string(), value.to_string());
            } else {
                // Just declare without value
                ctx.variables.insert(arg.to_string(), String::new());
            }
        }
        Ok(ExecResult::ok(String::new()))
    }
}
