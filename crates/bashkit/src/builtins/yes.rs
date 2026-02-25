//! yes builtin - repeatedly output a line

use async_trait::async_trait;

use super::{Builtin, Context};
use crate::error::Result;
use crate::interpreter::ExecResult;

/// The yes builtin - output a string repeatedly.
///
/// Usage: yes [STRING]
///
/// Repeatedly outputs STRING (default: "y") followed by newline.
/// In bashkit, output is limited to avoid infinite loops.
pub struct Yes;

/// Maximum number of lines to output (safety limit)
const MAX_LINES: usize = 10_000;

#[async_trait]
impl Builtin for Yes {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let text = if ctx.args.is_empty() {
            "y".to_string()
        } else {
            ctx.args.join(" ")
        };

        let mut output = String::new();
        for _ in 0..MAX_LINES {
            output.push_str(&text);
            output.push('\n');
        }

        Ok(ExecResult::ok(output))
    }
}
