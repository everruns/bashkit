//! export builtin - mark variables for export

use async_trait::async_trait;

use super::{Builtin, Context};
use crate::error::Result;
use crate::interpreter::ExecResult;

/// Check if a variable name is valid: [a-zA-Z_][a-zA-Z0-9_]*
fn is_valid_var_name(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// export builtin - mark variables for export to child processes
///
/// In Bashkit's virtual environment, this primarily just sets variables.
/// The distinction between exported and non-exported isn't significant
/// since we don't spawn real child processes.
pub struct Export;

#[async_trait]
impl Builtin for Export {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        for arg in ctx.args {
            // Handle NAME=VALUE format
            if let Some(eq_pos) = arg.find('=') {
                let name = &arg[..eq_pos];
                let value = &arg[eq_pos + 1..];
                // Validate variable name
                if !is_valid_var_name(name) {
                    return Ok(ExecResult::err(
                        format!("export: `{}': not a valid identifier\n", arg),
                        1,
                    ));
                }
                ctx.variables.insert(name.to_string(), value.to_string());
            } else {
                // Just marking for export - in our model this is a no-op
                // unless the variable exists, in which case we keep it
            }
        }

        Ok(ExecResult::ok(String::new()))
    }
}
