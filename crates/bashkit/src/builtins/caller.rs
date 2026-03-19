//! Caller builtin — display call stack information.
//!
//! Reads call stack via [`ShellRef`](super::ShellRef).

use async_trait::async_trait;

use super::{Builtin, Context};
use crate::error::Result;
use crate::interpreter::ExecResult;

/// `caller` builtin — display call stack information.
///
/// Usage: caller [FRAME_NUM]
///
/// Returns "line_number function_name source_file" for the given frame.
/// FRAME_NUM defaults to 0 (most recent caller).
pub struct Caller;

#[async_trait]
impl Builtin for Caller {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let Some(shell) = ctx.shell.as_ref() else {
            return Ok(ExecResult::err(String::new(), 1));
        };

        let frame_num: usize = ctx.args.first().and_then(|s| s.parse().ok()).unwrap_or(0);

        let depth = shell.call_stack_depth();
        if depth == 0 {
            return Ok(ExecResult::err(String::new(), 1));
        }

        let source = "main";
        let line = 1;
        let output = if frame_num == 0 && depth == 1 {
            format!("{} main {}\n", line, source)
        } else if frame_num + 1 < depth {
            // call_stack_frame_name(idx): idx 0 = most recent (vec.last())
            // Original interpreter: call_stack[len-2-frame_num].name
            // That maps to call_stack_frame_name(1 + frame_num)
            if let Some(name) = shell.call_stack_frame_name(1 + frame_num) {
                format!("{} {} {}\n", line, name, source)
            } else {
                return Ok(ExecResult::err(String::new(), 1));
            }
        } else if frame_num + 1 == depth {
            format!("{} main {}\n", line, source)
        } else {
            return Ok(ExecResult::err(String::new(), 1));
        };

        Ok(ExecResult::ok(output))
    }
}
