//! export builtin - mark variables for export

use async_trait::async_trait;

use super::{Builtin, Context};
use crate::error::Result;
use crate::interpreter::{ExecResult, is_internal_variable, is_valid_var_name};

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
                // THREAT[TM-INJ-015]: Block internal variable prefix injection via export
                if is_internal_variable(name) {
                    continue;
                }
                // THREAT[TM-INJ-021]: Refuse to overwrite readonly variables
                if ctx.variables.contains_key(&format!("_READONLY_{}", name)) {
                    continue;
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
