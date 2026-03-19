//! fc builtin - display and re-execute history entries
//!
//! Non-standard simplified version. In Bashkit's virtual environment,
//! history is session-limited, so fc provides listing and substitution.

use async_trait::async_trait;

use super::{Builtin, Context};
use crate::error::Result;
use crate::interpreter::ExecResult;

/// fc builtin - list and manipulate command history.
///
/// Usage: fc [-l] [-n] [-r] [-s [old=new]] [first [last]]
///
/// Options:
///   -l        List history entries (default behavior in virtual env)
///   -n        Suppress line numbers in listing
///   -r        Reverse order
///   -s old=new  Substitute and display (no re-execution in virtual env)
///
/// In Bashkit's virtual environment, fc only lists and formats history.
/// Re-execution and editor support are not available.
pub struct Fc;

#[async_trait]
impl Builtin for Fc {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let mut list_mode = false;
        let mut no_numbers = false;
        let mut reverse = false;
        let mut substitute: Option<(String, String)> = None;
        let mut positional: Vec<String> = Vec::new();

        let mut i = 0;
        while i < ctx.args.len() {
            match ctx.args[i].as_str() {
                "-l" => list_mode = true,
                "-n" => no_numbers = true,
                "-r" => reverse = true,
                "-s" => {
                    i += 1;
                    if let Some(arg) = ctx.args.get(i) {
                        if let Some(eq_pos) = arg.find('=') {
                            substitute =
                                Some((arg[..eq_pos].to_string(), arg[eq_pos + 1..].to_string()));
                        } else {
                            return Ok(ExecResult::err(
                                "fc: -s requires old=new argument\n".to_string(),
                                1,
                            ));
                        }
                    } else {
                        return Ok(ExecResult::err(
                            "fc: -s requires an argument\n".to_string(),
                            1,
                        ));
                    }
                }
                "-ln" | "-nl" => {
                    list_mode = true;
                    no_numbers = true;
                }
                "-lr" | "-rl" => {
                    list_mode = true;
                    reverse = true;
                }
                arg if arg.starts_with('-') && arg.len() > 1 => {
                    // Check for combined flags
                    let flags = &arg[1..];
                    for ch in flags.chars() {
                        match ch {
                            'l' => list_mode = true,
                            'n' => no_numbers = true,
                            'r' => reverse = true,
                            _ => {
                                return Ok(ExecResult::err(
                                    format!("fc: invalid option -- '{ch}'\n"),
                                    1,
                                ));
                            }
                        }
                    }
                }
                _ => positional.push(ctx.args[i].clone()),
            }
            i += 1;
        }

        // Handle substitution mode
        if let Some((old, new)) = substitute {
            return Ok(ExecResult::ok(format!(
                "fc: would substitute '{old}' with '{new}' in last command (not supported in virtual environment)\n"
            )));
        }

        // Default to list mode in virtual environment
        let _ = list_mode;

        // Virtual history entries - in a real shell these would come from
        // the interpreter's command history
        let history: Vec<String> = Vec::new();

        if history.is_empty() {
            return Ok(ExecResult::ok(
                "fc: no history available in virtual environment\n".to_string(),
            ));
        }

        // Parse range from positional args
        let first = positional
            .first()
            .and_then(|s| s.parse::<i64>().ok())
            .unwrap_or(-(history.len() as i64));
        let last = positional
            .get(1)
            .and_then(|s| s.parse::<i64>().ok())
            .unwrap_or(-1);

        let len = history.len() as i64;
        let start = if first < 0 {
            (len + first).max(0) as usize
        } else {
            (first - 1).max(0) as usize
        };
        let end = if last < 0 {
            (len + last + 1).max(0) as usize
        } else {
            last.min(len) as usize
        };

        let mut entries: Vec<(usize, &str)> = history[start..end.min(history.len())]
            .iter()
            .enumerate()
            .map(|(i, s)| (start + i + 1, s.as_str()))
            .collect();

        if reverse {
            entries.reverse();
        }

        let mut output = String::new();
        for (num, cmd) in &entries {
            if no_numbers {
                output.push_str(&format!("{cmd}\n"));
            } else {
                output.push_str(&format!("{num}\t{cmd}\n"));
            }
        }

        Ok(ExecResult::ok(output))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Arc;

    use crate::fs::InMemoryFs;

    async fn run_fc(args: &[&str]) -> ExecResult {
        let fs = Arc::new(InMemoryFs::new());
        let mut variables = HashMap::new();
        let env = HashMap::new();
        let mut cwd = PathBuf::from("/");
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs, None);
        Fc.execute(ctx).await.unwrap()
    }

    #[tokio::test]
    async fn test_list_empty_history() {
        let result = run_fc(&["-l"]).await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("no history"));
    }

    #[tokio::test]
    async fn test_default_empty_history() {
        let result = run_fc(&[]).await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("no history"));
    }

    #[tokio::test]
    async fn test_substitute_mode() {
        let result = run_fc(&["-s", "foo=bar"]).await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("substitute"));
        assert!(result.stdout.contains("foo"));
        assert!(result.stdout.contains("bar"));
    }

    #[tokio::test]
    async fn test_substitute_missing_arg() {
        let result = run_fc(&["-s"]).await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("requires an argument"));
    }

    #[tokio::test]
    async fn test_substitute_invalid_format() {
        let result = run_fc(&["-s", "noequalssign"]).await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("old=new"));
    }

    #[tokio::test]
    async fn test_invalid_option() {
        let result = run_fc(&["-z"]).await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("invalid option"));
    }

    #[tokio::test]
    async fn test_combined_flags() {
        let result = run_fc(&["-ln"]).await;
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn test_reverse_flag() {
        let result = run_fc(&["-r"]).await;
        assert_eq!(result.exit_code, 0);
    }
}
