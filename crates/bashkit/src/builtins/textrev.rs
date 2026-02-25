//! Text reversal builtins: tac (reverse line order) and rev (reverse characters per line)

use async_trait::async_trait;
use std::path::Path;

use super::{Builtin, Context};
use crate::error::Result;
use crate::interpreter::ExecResult;

/// Read input from files or stdin, returning the raw text.
async fn read_input(ctx: &Context<'_>) -> std::result::Result<String, ExecResult> {
    let mut files: Vec<&str> = Vec::new();
    for arg in ctx.args {
        if !arg.starts_with('-') {
            files.push(arg);
        }
    }

    let mut raw = String::new();
    if files.is_empty() {
        if let Some(stdin) = ctx.stdin {
            raw.push_str(stdin);
        }
    } else {
        for file in &files {
            if *file == "-" {
                if let Some(stdin) = ctx.stdin {
                    raw.push_str(stdin);
                }
            } else {
                let path = if Path::new(file).is_absolute() {
                    file.to_string()
                } else {
                    ctx.cwd.join(file).to_string_lossy().to_string()
                };
                match ctx.fs.read_file(Path::new(&path)).await {
                    Ok(content) => {
                        let text = String::from_utf8_lossy(&content);
                        raw.push_str(&text);
                    }
                    Err(e) => {
                        return Err(ExecResult::err(format!("tac: {}: {}\n", file, e), 1));
                    }
                }
            }
        }
    }
    Ok(raw)
}

/// The tac builtin - concatenate and print files in reverse (line order).
///
/// Usage: tac [FILE...]
///
/// Prints lines in reverse order. Reads from stdin if no files given.
pub struct Tac;

#[async_trait]
impl Builtin for Tac {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let raw = match read_input(&ctx).await {
            Ok(r) => r,
            Err(e) => return Ok(e),
        };

        if raw.is_empty() {
            return Ok(ExecResult::ok(String::new()));
        }

        let has_trailing_newline = raw.ends_with('\n');
        let trimmed = if has_trailing_newline {
            &raw[..raw.len() - 1]
        } else {
            &raw
        };

        let mut lines: Vec<&str> = trimmed.split('\n').collect();
        lines.reverse();

        let mut output = lines.join("\n");
        output.push('\n');

        Ok(ExecResult::ok(output))
    }
}

/// The rev builtin - reverse characters of each line.
///
/// Usage: rev [FILE...]
///
/// Reverses characters on each line. Reads from stdin if no files given.
pub struct Rev;

#[async_trait]
impl Builtin for Rev {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let raw = match read_input(&ctx).await {
            Ok(r) => r,
            Err(e) => return Ok(e),
        };

        if raw.is_empty() {
            return Ok(ExecResult::ok(String::new()));
        }

        let has_trailing_newline = raw.ends_with('\n');
        let trimmed = if has_trailing_newline {
            &raw[..raw.len() - 1]
        } else {
            &raw
        };

        let mut output = String::new();
        for (i, line) in trimmed.split('\n').enumerate() {
            if i > 0 {
                output.push('\n');
            }
            let reversed: String = line.chars().rev().collect();
            output.push_str(&reversed);
        }
        output.push('\n');

        Ok(ExecResult::ok(output))
    }
}
