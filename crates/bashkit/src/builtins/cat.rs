//! cat builtin command

use async_trait::async_trait;
use std::path::Path;

use super::{Builtin, Context};
use crate::error::Result;
use crate::interpreter::ExecResult;

/// The cat builtin command.
pub struct Cat;

#[async_trait]
impl Builtin for Cat {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let mut output = String::new();

        // If no arguments and stdin is provided, output stdin
        if ctx.args.is_empty() {
            if let Some(stdin) = ctx.stdin {
                output.push_str(stdin);
            }
        } else {
            // Read files
            for arg in ctx.args {
                // Handle - as stdin
                if arg == "-" {
                    if let Some(stdin) = ctx.stdin {
                        output.push_str(stdin);
                    }
                } else {
                    let path = if Path::new(arg).is_absolute() {
                        arg.to_string()
                    } else {
                        ctx.cwd.join(arg).to_string_lossy().to_string()
                    };

                    match ctx.fs.read_file(Path::new(&path)).await {
                        Ok(content) => {
                            let text = String::from_utf8_lossy(&content);
                            output.push_str(&text);
                        }
                        Err(e) => {
                            return Ok(ExecResult::err(format!("cat: {}: {}\n", arg, e), 1));
                        }
                    }
                }
            }
        }

        Ok(ExecResult::ok(output))
    }
}
