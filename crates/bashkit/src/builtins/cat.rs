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
        let mut show_nonprinting = false;
        let mut number_lines = false;
        let mut files: Vec<&str> = Vec::new();

        // Parse flags
        for arg in ctx.args {
            if arg.starts_with('-') && arg.len() > 1 && !arg.starts_with("--") {
                for ch in arg[1..].chars() {
                    match ch {
                        'v' => show_nonprinting = true,
                        'n' => number_lines = true,
                        'e' => show_nonprinting = true, // -e implies -v + show $ at EOL (simplified)
                        't' => show_nonprinting = true, // -t implies -v + show ^I for tabs (simplified)
                        _ => {}
                    }
                }
            } else {
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
                            return Ok(ExecResult::err(format!("cat: {}: {}\n", file, e), 1));
                        }
                    }
                }
            }
        }

        if show_nonprinting {
            for ch in raw.chars() {
                match ch {
                    '\n' | '\t' => output.push(ch), // pass through newline and tab
                    c if (c as u32) < 32 => {
                        // Control characters: ^@, ^A, ..., ^Z, ^[, ^\, ^], ^^, ^_
                        output.push('^');
                        output.push((c as u8 + 64) as char);
                    }
                    '\x7f' => {
                        output.push('^');
                        output.push('?');
                    }
                    c => output.push(c),
                }
            }
        } else {
            output = raw;
        }

        if number_lines {
            let lines: Vec<&str> = output.split('\n').collect();
            let mut numbered = String::new();
            for (i, line) in lines.iter().enumerate() {
                if i < lines.len() - 1 || !line.is_empty() {
                    numbered.push_str(&format!("     {}\t{}", i + 1, line));
                    if i < lines.len() - 1 {
                        numbered.push('\n');
                    }
                }
            }
            output = numbered;
        }

        Ok(ExecResult::ok(output))
    }
}
