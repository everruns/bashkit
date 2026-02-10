//! export builtin - mark variables for export

use async_trait::async_trait;

use super::{Builtin, Context};
use crate::error::Result;
use crate::interpreter::ExecResult;

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
                ctx.variables.insert(name.to_string(), value.to_string());
            } else {
                // Just marking for export - in our model this is a no-op
                // unless the variable exists, in which case we keep it
            }
        }

        Ok(ExecResult::ok(String::new()))
    }
}
