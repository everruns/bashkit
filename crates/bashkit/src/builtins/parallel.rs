//! parallel builtin - GNU parallel-lite (virtual stub)
//!
//! Non-standard builtin. Cannot actually parallelize in VFS,
//! so parses options and reports what commands would be run.

use async_trait::async_trait;

use super::{Builtin, Context};
use crate::error::Result;
use crate::interpreter::ExecResult;

/// Parallel builtin - GNU parallel-lite stub.
///
/// Usage: parallel [OPTIONS] COMMAND ::: ARGS...
///
/// Options:
///   -j NUM         Number of parallel jobs (default: number of args)
///   --dry-run      Show commands that would be run
///   --keep-order   Keep output in input order (noted in plan)
///   --bar          Show progress bar (noted in plan)
///   -v             Verbose mode
///
/// The `:::` separator delimits the argument list.
/// `{}` in COMMAND is replaced by each argument.
/// Multiple `:::` groups produce the cartesian product.
///
/// Since this is a virtual environment, execution is always dry-run:
/// the builtin parses the invocation and reports the planned commands.
pub struct Parallel;

struct ParallelConfig {
    jobs: Option<u32>,
    dry_run: bool,
    keep_order: bool,
    bar: bool,
    verbose: bool,
    command_parts: Vec<String>,
    arg_groups: Vec<Vec<String>>,
}

fn parse_parallel_args(args: &[String]) -> std::result::Result<ParallelConfig, String> {
    let mut jobs = None;
    let mut dry_run = false;
    let mut keep_order = false;
    let mut bar = false;
    let mut verbose = false;
    let mut command_parts: Vec<String> = Vec::new();
    let mut arg_groups: Vec<Vec<String>> = Vec::new();

    // Split on ::: to find command template and argument groups
    let mut segments: Vec<Vec<String>> = vec![Vec::new()];
    for arg in args {
        if arg == ":::" {
            segments.push(Vec::new());
        } else if let Some(last) = segments.last_mut() {
            last.push(arg.clone());
        }
    }

    // First segment: options + command template
    let first = &segments[0];
    let mut p = super::arg_parser::ArgParser::new(first);
    while !p.is_done() {
        if let Some(val) = p.flag_value("-j", "parallel")? {
            let n: u32 = val
                .parse()
                .map_err(|_| format!("parallel: invalid job count '{}'", val))?;
            if n == 0 {
                return Err("parallel: -j must be at least 1".to_string());
            }
            jobs = Some(n);
        } else if p.flag("--dry-run") {
            dry_run = true;
        } else if p.flag_any(&["--keep-order", "-k"]) {
            keep_order = true;
        } else if p.flag("--bar") {
            bar = true;
        } else if p.flag("-v") {
            verbose = true;
        } else if p.is_flag() && command_parts.is_empty() {
            if let Some(arg) = p.current() {
                return Err(format!("parallel: unknown option '{}'", arg));
            }
            p.advance();
        } else if let Some(arg) = p.positional() {
            command_parts.push(arg.to_string());
        }
    }

    // Remaining segments are argument groups
    for seg in &segments[1..] {
        if seg.is_empty() {
            return Err("parallel: empty argument group after :::".to_string());
        }
        arg_groups.push(seg.clone());
    }

    Ok(ParallelConfig {
        jobs,
        dry_run,
        keep_order,
        bar,
        verbose,
        command_parts,
        arg_groups,
    })
}

/// Maximum number of cartesian product combinations allowed.
/// Prevents exponential memory blowup with many `:::` groups.
const MAX_CARTESIAN_PRODUCT: usize = 100_000;

/// Generate the cartesian product of multiple argument groups.
///
/// Returns an error if the total number of combinations would exceed
/// `MAX_CARTESIAN_PRODUCT` to prevent exponential memory blowup.
fn cartesian_product(groups: &[Vec<String>]) -> std::result::Result<Vec<Vec<String>>, String> {
    if groups.is_empty() {
        return Ok(vec![vec![]]);
    }

    // Pre-calculate total combinations to reject before allocating.
    groups
        .iter()
        .try_fold(1usize, |acc, g| {
            acc.checked_mul(g.len())
                .filter(|&n| n <= MAX_CARTESIAN_PRODUCT)
        })
        .ok_or_else(|| {
            format!(
                "parallel: cartesian product too large (exceeds {MAX_CARTESIAN_PRODUCT} combinations)"
            )
        })?;

    let mut result = vec![vec![]];
    for group in groups {
        let mut new_result = Vec::with_capacity(result.len() * group.len());
        for existing in &result {
            for item in group {
                let mut combo = existing.clone();
                combo.push(item.clone());
                new_result.push(combo);
            }
        }
        result = new_result;
    }
    Ok(result)
}

/// Build a command string by substituting `{}` with the argument.
/// If no `{}` is present, append the argument to the command.
fn build_command(template: &[String], args: &[String]) -> String {
    let template_str = template.join(" ");
    if args.len() == 1 {
        if template_str.contains("{}") {
            template_str.replace("{}", &args[0])
        } else {
            format!("{} {}", template_str, args[0])
        }
    } else {
        // Multiple arg groups: replace {1}, {2}, etc. or append all
        let mut cmd = template_str.clone();
        let mut had_placeholder = false;
        for (idx, arg) in args.iter().enumerate() {
            let placeholder = format!("{{{}}}", idx + 1);
            if cmd.contains(&placeholder) {
                cmd = cmd.replace(&placeholder, arg);
                had_placeholder = true;
            }
        }
        if !had_placeholder {
            if cmd.contains("{}") {
                cmd = cmd.replace("{}", &args.join(" "));
            } else {
                cmd = format!("{} {}", cmd, args.join(" "));
            }
        }
        cmd
    }
}

#[async_trait]
impl Builtin for Parallel {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        if ctx.args.is_empty() {
            return Ok(ExecResult::err(
                "parallel: usage: parallel [OPTIONS] COMMAND ::: ARGS...\n".to_string(),
                1,
            ));
        }

        let config = match parse_parallel_args(ctx.args) {
            Ok(c) => c,
            Err(e) => return Ok(ExecResult::err(format!("{e}\n"), 1)),
        };

        if config.command_parts.is_empty() {
            return Ok(ExecResult::err(
                "parallel: no command specified\n".to_string(),
                1,
            ));
        }

        if config.arg_groups.is_empty() {
            return Ok(ExecResult::err(
                "parallel: no arguments provided (missing :::)\n".to_string(),
                1,
            ));
        }

        let combinations = match cartesian_product(&config.arg_groups) {
            Ok(c) => c,
            Err(e) => return Ok(ExecResult::err(format!("{e}\n"), 1)),
        };
        let num_commands = combinations.len();
        let effective_jobs = config.jobs.unwrap_or(num_commands as u32);

        let mut output = String::new();

        // Header
        if config.verbose || config.dry_run {
            output.push_str(&format!(
                "parallel: {} command(s), {} job(s)",
                num_commands, effective_jobs,
            ));
            if config.keep_order {
                output.push_str(", ordered output");
            }
            if config.bar {
                output.push_str(", progress bar");
            }
            output.push('\n');
        }

        // List commands
        for combo in &combinations {
            let cmd = build_command(&config.command_parts, combo);
            output.push_str(&cmd);
            output.push('\n');
        }

        if !config.dry_run {
            output.push_str("parallel: not supported in virtual environment\n");
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

    async fn run_parallel(args: &[&str]) -> ExecResult {
        let fs = Arc::new(InMemoryFs::new());
        let mut variables = HashMap::new();
        let env = HashMap::new();
        let mut cwd = PathBuf::from("/");
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs, None);
        Parallel.execute(ctx).await.unwrap()
    }

    #[tokio::test]
    async fn test_no_args() {
        let result = run_parallel(&[]).await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("usage"));
    }

    #[tokio::test]
    async fn test_basic_command_generation() {
        let result = run_parallel(&["echo", ":::", "a", "b", "c"]).await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("echo a"));
        assert!(result.stdout.contains("echo b"));
        assert!(result.stdout.contains("echo c"));
    }

    #[tokio::test]
    async fn test_placeholder_substitution() {
        let result = run_parallel(&["echo", "hello", "{}", ":::", "world", "test"]).await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("echo hello world"));
        assert!(result.stdout.contains("echo hello test"));
    }

    #[tokio::test]
    async fn test_dry_run_header() {
        let result = run_parallel(&["--dry-run", "echo", ":::", "x", "y"]).await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("2 command(s)"));
        // dry-run should NOT print the "not supported" message
        assert!(!result.stdout.contains("not supported"));
    }

    #[tokio::test]
    async fn test_jobs_option() {
        let result = run_parallel(&["-j", "4", "--dry-run", "echo", ":::", "a", "b"]).await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("4 job(s)"));
    }

    #[tokio::test]
    async fn test_keep_order_flag() {
        let result = run_parallel(&["--keep-order", "--dry-run", "echo", ":::", "x"]).await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("ordered output"));
    }

    #[tokio::test]
    async fn test_bar_flag() {
        let result = run_parallel(&["--bar", "--dry-run", "echo", ":::", "x"]).await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("progress bar"));
    }

    #[tokio::test]
    async fn test_no_command() {
        let result = run_parallel(&[":::", "a", "b"]).await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("no command"));
    }

    #[tokio::test]
    async fn test_no_separator() {
        let result = run_parallel(&["echo", "hello"]).await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("no arguments"));
    }

    #[tokio::test]
    async fn test_invalid_jobs() {
        let result = run_parallel(&["-j", "abc", "echo", ":::", "x"]).await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("invalid job count"));
    }

    #[tokio::test]
    async fn test_zero_jobs() {
        let result = run_parallel(&["-j", "0", "echo", ":::", "x"]).await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("must be at least 1"));
    }

    #[tokio::test]
    async fn test_missing_j_arg() {
        let result = run_parallel(&["-j"]).await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("-j requires an argument"));
    }

    #[tokio::test]
    async fn test_cartesian_product_two_groups() {
        let result = run_parallel(&["echo", "{1}", "{2}", ":::", "a", "b", ":::", "1", "2"]).await;
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("echo a 1"));
        assert!(result.stdout.contains("echo a 2"));
        assert!(result.stdout.contains("echo b 1"));
        assert!(result.stdout.contains("echo b 2"));
    }

    #[tokio::test]
    async fn test_virtual_env_message() {
        let result = run_parallel(&["echo", ":::", "x"]).await;
        assert_eq!(result.exit_code, 0);
        assert!(
            result
                .stdout
                .contains("not supported in virtual environment")
        );
    }

    #[test]
    fn test_cartesian_product_small() {
        let groups = vec![
            vec!["a".to_string(), "b".to_string()],
            vec!["1".to_string(), "2".to_string()],
        ];
        let result = cartesian_product(&groups).unwrap();
        assert_eq!(result.len(), 4);
        assert!(result.contains(&vec!["a".to_string(), "1".to_string()]));
        assert!(result.contains(&vec!["b".to_string(), "2".to_string()]));
    }

    #[test]
    fn test_cartesian_product_exceeds_limit() {
        // 20 groups of 4 elements each = 4^20 = ~1 trillion combinations
        let groups: Vec<Vec<String>> = (0..20)
            .map(|_| vec!["a".into(), "b".into(), "c".into(), "d".into()])
            .collect();
        let result = cartesian_product(&groups);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("cartesian product too large"));
    }

    #[tokio::test]
    async fn test_cartesian_product_limit_via_builtin() {
        // Build args: echo ::: a b c d ::: a b c d ... (20 groups)
        let mut args: Vec<&str> = vec!["echo"];
        for _ in 0..20 {
            args.push(":::");
            args.extend(["a", "b", "c", "d"]);
        }
        let result = run_parallel(&args).await;
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("cartesian product too large"));
    }
}
