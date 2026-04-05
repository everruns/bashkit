//! Pipeline control builtins - xargs, tee, watch

use async_trait::async_trait;

use super::{Builtin, Context, ExecutionPlan, SubCommand, resolve_path};
use crate::error::Result;
use crate::interpreter::ExecResult;

/// The xargs builtin - build and execute command lines from stdin.
///
/// Usage: xargs [-I REPLACE] [-n MAX-ARGS] [-d DELIM] [COMMAND [ARGS...]]
///
/// Options:
///   -I REPLACE   Replace REPLACE with input (implies -n 1)
///   -n MAX-ARGS  Use at most MAX-ARGS arguments per command
///   -d DELIM     Use DELIM as delimiter instead of whitespace
///   -0           Use NUL as delimiter (same as -d '\0')
pub struct Xargs;

/// Parsed xargs options.
struct XargsOptions {
    replace_str: Option<String>,
    max_args: Option<usize>,
    delimiter: Option<char>,
    command: Vec<String>,
}

/// Parse xargs arguments, returning options or an error ExecResult.
#[allow(clippy::result_large_err)]
fn parse_xargs_args(args: &[String]) -> std::result::Result<XargsOptions, ExecResult> {
    let mut replace_str: Option<String> = None;
    let mut max_args: Option<usize> = None;
    let mut delimiter: Option<char> = None;
    let mut command: Vec<String> = Vec::new();
    let mut p = super::arg_parser::ArgParser::new(args);

    while !p.is_done() {
        if let Some(val) = p
            .flag_value("-I", "xargs")
            .map_err(|e| ExecResult::err(format!("{e}\n"), 1))?
        {
            replace_str = Some(val.to_string());
            max_args = Some(1); // -I implies -n 1
        } else if let Some(val) = p
            .flag_value("-n", "xargs")
            .map_err(|e| ExecResult::err(format!("{e}\n"), 1))?
        {
            match val.parse::<usize>() {
                Ok(n) if n > 0 => max_args = Some(n),
                _ => {
                    return Err(ExecResult::err(
                        format!("xargs: invalid number: '{}'\n", val),
                        1,
                    ));
                }
            }
        } else if let Some(val) = p
            .flag_value("-d", "xargs")
            .map_err(|e| ExecResult::err(format!("{e}\n"), 1))?
        {
            delimiter = val.chars().next();
        } else if p.flag("-0") {
            delimiter = Some('\0');
        } else if p.is_flag() && p.current() != Some("-") {
            let Some(s) = p.current() else {
                p.advance();
                continue;
            };
            return Err(ExecResult::err(
                format!("xargs: invalid option -- '{}'\n", &s[1..]),
                1,
            ));
        } else {
            command.extend(p.rest().iter().cloned());
            break;
        }
    }

    if command.is_empty() {
        command.push("echo".to_string());
    }

    Ok(XargsOptions {
        replace_str,
        max_args,
        delimiter,
        command,
    })
}

/// Build the list of sub-commands from parsed options and stdin input.
fn build_xargs_commands(opts: &XargsOptions, input: &str) -> Vec<SubCommand> {
    if input.is_empty() {
        return Vec::new();
    }

    let items: Vec<&str> = if let Some(delim) = opts.delimiter {
        input.split(delim).filter(|s| !s.is_empty()).collect()
    } else {
        input.split_whitespace().collect()
    };

    if items.is_empty() {
        return Vec::new();
    }

    let chunk_size = opts.max_args.unwrap_or(items.len());
    let chunks: Vec<Vec<&str>> = items.chunks(chunk_size).map(|c| c.to_vec()).collect();

    chunks
        .into_iter()
        .map(|chunk| {
            let cmd_args: Vec<String> = if let Some(ref repl) = opts.replace_str {
                let item = chunk.first().unwrap_or(&"");
                opts.command
                    .iter()
                    .map(|arg| arg.replace(repl, item))
                    .collect()
            } else {
                let mut full = opts.command.clone();
                full.extend(chunk.iter().map(|s| s.to_string()));
                full
            };

            let name = cmd_args[0].clone();
            let args = cmd_args[1..].to_vec();
            SubCommand {
                name,
                args,
                stdin: None,
            }
        })
        .collect()
}

#[async_trait]
impl Builtin for Xargs {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        // Validate arguments and return error for invalid input.
        // When no executor is available, output what commands would be run.
        let opts = match parse_xargs_args(ctx.args) {
            Ok(opts) => opts,
            Err(e) => return Ok(e),
        };

        let input = ctx.stdin.unwrap_or("");
        if input.is_empty() {
            return Ok(ExecResult::ok(String::new()));
        }

        let commands = build_xargs_commands(&opts, input);
        if commands.is_empty() {
            return Ok(ExecResult::ok(String::new()));
        }

        // Fallback: output what would be run (for standalone builtin context)
        let mut output = String::new();
        for cmd in &commands {
            output.push_str(&cmd.name);
            for arg in &cmd.args {
                output.push(' ');
                output.push_str(arg);
            }
            output.push('\n');
        }
        Ok(ExecResult::ok(output))
    }

    async fn execution_plan(&self, ctx: &Context<'_>) -> Result<Option<ExecutionPlan>> {
        let opts = match parse_xargs_args(ctx.args) {
            Ok(opts) => opts,
            Err(_) => return Ok(None), // Let execute() handle the error
        };

        let input = ctx.stdin.unwrap_or("");
        if input.is_empty() {
            return Ok(None); // Let execute() handle empty input
        }

        let commands = build_xargs_commands(&opts, input);
        if commands.is_empty() {
            return Ok(None);
        }

        Ok(Some(ExecutionPlan::Batch { commands }))
    }
}

/// The tee builtin - read from stdin and write to stdout and files.
///
/// Usage: tee [-a] [FILE...]
///
/// Options:
///   -a   Append to files instead of overwriting
pub struct Tee;

#[async_trait]
impl Builtin for Tee {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let mut append = false;
        let mut files: Vec<String> = Vec::new();

        // Parse arguments
        for arg in ctx.args {
            if arg == "-a" || arg == "--append" {
                append = true;
            } else if arg.starts_with('-') && arg != "-" {
                return Ok(ExecResult::err(
                    format!("tee: invalid option -- '{}'\n", &arg[1..]),
                    1,
                ));
            } else {
                files.push(arg.clone());
            }
        }

        // Read from stdin
        let input = ctx.stdin.unwrap_or("");

        // Write to each file
        for file in &files {
            let path = resolve_path(ctx.cwd, file);

            if append {
                ctx.fs.append_file(&path, input.as_bytes()).await?;
            } else {
                ctx.fs.write_file(&path, input.as_bytes()).await?;
            }
        }

        // Output to stdout as well
        Ok(ExecResult::ok(input.to_string()))
    }
}

/// The watch builtin - execute a program periodically.
///
/// Usage: watch [-n SECONDS] COMMAND
///
/// Options:
///   -n SECONDS   Specify update interval (default: 2)
///
/// Note: In Bashkit's virtual environment, watch runs the command once
/// and returns, since continuous execution isn't supported.
pub struct Watch;

#[async_trait]
impl Builtin for Watch {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let mut _interval: f64 = 2.0;
        let mut command_start: Option<usize> = None;

        let mut i = 0;
        while i < ctx.args.len() {
            let arg = &ctx.args[i];
            if arg == "-n" {
                i += 1;
                if i >= ctx.args.len() {
                    return Ok(ExecResult::err(
                        "watch: option requires an argument -- 'n'\n".to_string(),
                        1,
                    ));
                }
                match ctx.args[i].parse::<f64>() {
                    Ok(n) if n > 0.0 => _interval = n,
                    _ => {
                        return Ok(ExecResult::err(
                            format!("watch: invalid interval '{}'\n", ctx.args[i]),
                            1,
                        ));
                    }
                }
            } else if arg.starts_with('-') && arg != "-" {
                // Skip other options for compatibility
            } else {
                command_start = Some(i);
                break;
            }
            i += 1;
        }

        let start = match command_start {
            Some(s) => s,
            None => {
                return Ok(ExecResult::err(
                    "watch: no command specified\n".to_string(),
                    1,
                ));
            }
        };

        let command: Vec<_> = ctx.args[start..].iter().collect();
        let output = format!(
            "Every {:.1}s: {}\n\n(watch: continuous execution not supported in virtual mode)\n",
            _interval,
            command
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join(" ")
        );

        Ok(ExecResult::ok(output))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Arc;

    use crate::fs::{FileSystem, InMemoryFs};

    async fn create_test_ctx() -> (Arc<InMemoryFs>, PathBuf, HashMap<String, String>) {
        let fs = Arc::new(InMemoryFs::new());
        let cwd = PathBuf::from("/home/user");
        let variables = HashMap::new();

        fs.mkdir(&cwd, true).await.unwrap();

        (fs, cwd, variables)
    }

    // ==================== xargs tests ====================

    #[tokio::test]
    async fn test_xargs_basic() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        let args: Vec<String> = vec![];
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs: fs.clone(),
            stdin: Some("foo bar baz"),
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
        };

        let result = Xargs.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("echo foo bar baz"));
    }

    #[tokio::test]
    async fn test_xargs_with_command() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        let args = vec!["rm".to_string()];
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs: fs.clone(),
            stdin: Some("file1 file2"),
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
        };

        let result = Xargs.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("rm file1 file2"));
    }

    #[tokio::test]
    async fn test_xargs_n_option() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        let args = vec!["-n".to_string(), "1".to_string(), "echo".to_string()];
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs: fs.clone(),
            stdin: Some("a b c"),
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
        };

        let result = Xargs.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        let lines: Vec<_> = result.stdout.lines().collect();
        assert_eq!(lines.len(), 3);
        assert!(lines[0].contains("echo a"));
        assert!(lines[1].contains("echo b"));
        assert!(lines[2].contains("echo c"));
    }

    #[tokio::test]
    async fn test_xargs_i_option() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        let args = vec![
            "-I".to_string(),
            "{}".to_string(),
            "cp".to_string(),
            "{}".to_string(),
            "{}.bak".to_string(),
        ];
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs: fs.clone(),
            stdin: Some("file1\nfile2"),
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
        };

        let result = Xargs.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("cp file1 file1.bak"));
        assert!(result.stdout.contains("cp file2 file2.bak"));
    }

    #[tokio::test]
    async fn test_xargs_d_option() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        let args = vec!["-d".to_string(), ":".to_string()];
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs: fs.clone(),
            stdin: Some("a:b:c"),
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
        };

        let result = Xargs.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("echo a b c"));
    }

    #[tokio::test]
    async fn test_xargs_empty_input() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        let args: Vec<String> = vec![];
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs: fs.clone(),
            stdin: Some(""),
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
        };

        let result = Xargs.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.is_empty());
    }

    #[tokio::test]
    async fn test_xargs_invalid_option() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        let args = vec!["-z".to_string()];
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs: fs.clone(),
            stdin: Some("test"),
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
        };

        let result = Xargs.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("invalid option"));
    }

    #[tokio::test]
    async fn test_xargs_plan_basic() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        let args = vec!["rm".to_string()];
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs: fs.clone(),
            stdin: Some("file1 file2"),
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
        };

        let plan = Xargs.execution_plan(&ctx).await.unwrap();
        match plan {
            Some(ExecutionPlan::Batch { commands }) => {
                assert_eq!(commands.len(), 1);
                assert_eq!(commands[0].name, "rm");
                assert_eq!(commands[0].args, vec!["file1", "file2"]);
            }
            _ => panic!("expected Batch plan"),
        }
    }

    #[tokio::test]
    async fn test_xargs_plan_n_option() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        let args = vec!["-n".to_string(), "1".to_string(), "echo".to_string()];
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs: fs.clone(),
            stdin: Some("a b c"),
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
        };

        let plan = Xargs.execution_plan(&ctx).await.unwrap();
        match plan {
            Some(ExecutionPlan::Batch { commands }) => {
                assert_eq!(commands.len(), 3);
                assert_eq!(commands[0].name, "echo");
                assert_eq!(commands[0].args, vec!["a"]);
                assert_eq!(commands[1].args, vec!["b"]);
                assert_eq!(commands[2].args, vec!["c"]);
            }
            _ => panic!("expected Batch plan"),
        }
    }

    // ==================== tee tests ====================

    #[tokio::test]
    async fn test_tee_basic() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        let args = vec!["output.txt".to_string()];
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs: fs.clone(),
            stdin: Some("Hello, world!"),
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
        };

        let result = Tee.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "Hello, world!");

        let content = fs.read_file(&cwd.join("output.txt")).await.unwrap();
        assert_eq!(content, b"Hello, world!");
    }

    #[tokio::test]
    async fn test_tee_multiple_files() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        let args = vec!["file1.txt".to_string(), "file2.txt".to_string()];
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs: fs.clone(),
            stdin: Some("content"),
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
        };

        let result = Tee.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "content");

        let content1 = fs.read_file(&cwd.join("file1.txt")).await.unwrap();
        let content2 = fs.read_file(&cwd.join("file2.txt")).await.unwrap();
        assert_eq!(content1, b"content");
        assert_eq!(content2, b"content");
    }

    #[tokio::test]
    async fn test_tee_append() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        fs.write_file(&cwd.join("output.txt"), b"initial\n")
            .await
            .unwrap();

        let args = vec!["-a".to_string(), "output.txt".to_string()];
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs: fs.clone(),
            stdin: Some("appended"),
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
        };

        let result = Tee.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);

        let content = fs.read_file(&cwd.join("output.txt")).await.unwrap();
        assert_eq!(content, b"initial\nappended");
    }

    #[tokio::test]
    async fn test_tee_no_files() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        let args: Vec<String> = vec![];
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs: fs.clone(),
            stdin: Some("pass through"),
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
        };

        let result = Tee.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "pass through");
    }

    #[tokio::test]
    async fn test_tee_invalid_option() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        let args = vec!["-z".to_string()];
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs: fs.clone(),
            stdin: Some("test"),
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
        };

        let result = Tee.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("invalid option"));
    }

    // ==================== watch tests ====================

    #[tokio::test]
    async fn test_watch_basic() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        let args = vec!["ls".to_string(), "-l".to_string()];
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs: fs.clone(),
            stdin: None,
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
        };

        let result = Watch.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("ls -l"));
        assert!(result.stdout.contains("Every 2.0s"));
    }

    #[tokio::test]
    async fn test_watch_n_option() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        let args = vec!["-n".to_string(), "5".to_string(), "date".to_string()];
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs: fs.clone(),
            stdin: None,
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
        };

        let result = Watch.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("Every 5.0s"));
        assert!(result.stdout.contains("date"));
    }

    #[tokio::test]
    async fn test_watch_no_command() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        let args: Vec<String> = vec![];
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs: fs.clone(),
            stdin: None,
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
        };

        let result = Watch.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("no command specified"));
    }

    #[tokio::test]
    async fn test_watch_invalid_interval() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        let args = vec!["-n".to_string(), "abc".to_string(), "ls".to_string()];
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs: fs.clone(),
            stdin: None,
            #[cfg(feature = "http_client")]
            http_client: None,
            #[cfg(feature = "git")]
            git_client: None,
            #[cfg(feature = "ssh")]
            ssh_client: None,
            shell: None,
        };

        let result = Watch.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("invalid interval"));
    }
}
