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
        let mut array_mode = false; // -a: read into array
        let mut delimiter = None::<char>; // -d: custom delimiter
        let mut nchars = None::<usize>; // -n: read N chars
        let mut prompt = None::<String>; // -p prompt
        let mut var_args = Vec::new();
        let mut args_iter = ctx.args.iter();
        while let Some(arg) = args_iter.next() {
            if arg.starts_with('-') && arg.len() > 1 {
                let mut chars = arg[1..].chars();
                while let Some(flag) = chars.next() {
                    match flag {
                        'r' => raw_mode = true,
                        'a' => array_mode = true,
                        'd' => {
                            // -d delim: use first char of next arg as delimiter
                            let rest: String = chars.collect();
                            let delim_str = if rest.is_empty() {
                                args_iter.next().map(|s| s.as_str()).unwrap_or("")
                            } else {
                                &rest
                            };
                            delimiter = delim_str.chars().next();
                            break;
                        }
                        'n' => {
                            let rest: String = chars.collect();
                            let n_str = if rest.is_empty() {
                                args_iter.next().map(|s| s.as_str()).unwrap_or("0")
                            } else {
                                &rest
                            };
                            nchars = n_str.parse().ok();
                            break;
                        }
                        'p' => {
                            let rest: String = chars.collect();
                            prompt = Some(if rest.is_empty() {
                                args_iter.next().cloned().unwrap_or_default()
                            } else {
                                rest
                            });
                            break;
                        }
                        't' | 's' | 'u' | 'e' | 'i' => {
                            // -t timeout, -s silent, -u fd: accept and ignore
                            if matches!(flag, 't' | 'u') {
                                let rest: String = chars.collect();
                                if rest.is_empty() {
                                    args_iter.next();
                                }
                                break;
                            }
                        }
                        _ => {}
                    }
                }
            } else {
                var_args.push(arg.as_str());
            }
        }
        let _ = prompt; // prompt is for interactive use, ignored in non-interactive

        // Extract input based on delimiter or nchars
        let line = if let Some(n) = nchars {
            // -n N: read at most N chars
            input.chars().take(n).collect::<String>()
        } else if let Some(delim) = delimiter {
            // -d delim: read until delimiter
            input.split(delim).next().unwrap_or("").to_string()
        } else if raw_mode {
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

        if array_mode {
            // -a: read all words into array variable
            let arr_name = var_args.first().copied().unwrap_or("REPLY");
            // Store as _ARRAY_<name>_<idx> for the interpreter to pick up
            ctx.variables.insert(
                format!("_ARRAY_READ_{}", arr_name),
                words.join("\x1F"), // unit separator as delimiter
            );
            return Ok(ExecResult::ok(String::new()));
        }

        // If no variable names given, use REPLY
        let var_names: Vec<&str> = if var_args.is_empty() {
            vec!["REPLY"]
        } else {
            var_args
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
