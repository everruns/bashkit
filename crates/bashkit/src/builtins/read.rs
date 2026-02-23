//! read builtin - read a line of input

use async_trait::async_trait;

use super::{Builtin, Context};
use crate::error::Result;
use crate::interpreter::ExecResult;

/// read builtin - read a line of input into variables
pub struct Read;

#[async_trait]
impl Builtin for Read {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        // Get the input to read from stdin
        let input = match ctx.stdin {
            Some(s) => s.to_string(),
            None => return Ok(ExecResult::err("", 1)),
        };

        // Parse flags
        let mut raw_mode = false; // -r: don't interpret backslashes
        let mut prompt = None::<String>; // -p prompt
        let mut var_args = Vec::new();
        let mut args_iter = ctx.args.iter();
        while let Some(arg) = args_iter.next() {
            if arg.starts_with('-') && arg.len() > 1 {
                for flag in arg[1..].chars() {
                    match flag {
                        'r' => raw_mode = true,
                        'p' => {
                            // -p takes next arg as prompt
                            if let Some(p) = args_iter.next() {
                                prompt = Some(p.clone());
                            }
                        }
                        _ => {} // ignore unknown flags
                    }
                }
            } else {
                var_args.push(arg.as_str());
            }
        }
        let _ = prompt; // prompt is for interactive use, ignored in non-interactive

        // Get first line
        let line = if raw_mode {
            // -r: treat backslashes literally
            input.lines().next().unwrap_or("").to_string()
        } else {
            // Without -r: handle backslash line continuation
            let mut result = String::new();
            for l in input.lines() {
                if let Some(stripped) = l.strip_suffix('\\') {
                    result.push_str(stripped);
                } else {
                    result.push_str(l);
                    break;
                }
            }
            result
        };

        // If no variable names given, use REPLY
        let var_names: Vec<&str> = if var_args.is_empty() {
            vec!["REPLY"]
        } else {
            var_args
        };

        // Split line by IFS (default: space, tab, newline)
        let ifs = ctx.env.get("IFS").map(|s| s.as_str()).unwrap_or(" \t\n");
        let words: Vec<&str> = if ifs.is_empty() {
            // Empty IFS means no word splitting
            vec![&line]
        } else {
            line.split(|c: char| ifs.contains(c))
                .filter(|s| !s.is_empty())
                .collect()
        };

        // Assign words to variables
        for (i, var_name) in var_names.iter().enumerate() {
            if i == var_names.len() - 1 {
                // Last variable gets all remaining words
                let remaining: Vec<&str> = words.iter().skip(i).copied().collect();
                let value = remaining.join(" ");
                ctx.variables.insert(var_name.to_string(), value);
            } else if i < words.len() {
                ctx.variables
                    .insert(var_name.to_string(), words[i].to_string());
            } else {
                // Not enough words - set to empty
                ctx.variables.insert(var_name.to_string(), String::new());
            }
        }

        Ok(ExecResult::ok(String::new()))
    }
}
