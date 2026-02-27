//! Pipeline control builtins - xargs, tee, watch

use async_trait::async_trait;

use super::{resolve_path, Builtin, Context};
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
///
/// Note: xargs is intercepted at the interpreter level for actual command
/// execution. This builtin fallback only handles option parsing/validation.
pub struct Xargs;

#[async_trait]
impl Builtin for Xargs {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let mut replace_str: Option<String> = None;
        let mut max_args: Option<usize> = None;
        let mut delimiter: Option<char> = None;
        let mut command: Vec<String> = Vec::new();

        // Parse arguments
        let mut i = 0;
        while i < ctx.args.len() {
            let arg = &ctx.args[i];
            match arg.as_str() {
                "-I" => {
                    i += 1;
                    if i >= ctx.args.len() {
                        return Ok(ExecResult::err(
                            "xargs: option requires an argument -- 'I'\n".to_string(),
                            1,
                        ));
                    }
                    replace_str = Some(ctx.args[i].clone());
                    max_args = Some(1); // -I implies -n 1
                }
                "-n" => {
                    i += 1;
                    if i >= ctx.args.len() {
                        return Ok(ExecResult::err(
                            "xargs: option requires an argument -- 'n'\n".to_string(),
                            1,
                        ));
                    }
                    match ctx.args[i].parse::<usize>() {
                        Ok(n) if n > 0 => max_args = Some(n),
                        _ => {
                            return Ok(ExecResult::err(
                                format!("xargs: invalid number: '{}'\n", ctx.args[i]),
                                1,
                            ));
                        }
                    }
                }
                "-d" => {
                    i += 1;
                    if i >= ctx.args.len() {
                        return Ok(ExecResult::err(
                            "xargs: option requires an argument -- 'd'\n".to_string(),
                            1,
                        ));
                    }
                    delimiter = ctx.args[i].chars().next();
                }
                "-0" => {
                    delimiter = Some('\0');
                }
                s if s.starts_with('-') && s != "-" => {
                    return Ok(ExecResult::err(
                        format!("xargs: invalid option -- '{}'\n", &s[1..]),
                        1,
                    ));
                }
                _ => {
                    // Rest is the command
                    command.extend(ctx.args[i..].iter().cloned());
                    break;
                }
            }
            i += 1;
        }

        // Default command is echo
        if command.is_empty() {
            command.push("echo".to_string());
        }

        // Read input
        let input = ctx.stdin.unwrap_or("");
        if input.is_empty() {
            return Ok(ExecResult::ok(String::new()));
        }

        // Split input by delimiter
        let items: Vec<&str> = if let Some(delim) = delimiter {
            input.split(delim).filter(|s| !s.is_empty()).collect()
        } else {
            input.split_whitespace().collect()
        };

        if items.is_empty() {
            return Ok(ExecResult::ok(String::new()));
        }

        let mut output = String::new();

        // Group items based on max_args
        let chunk_size = max_args.unwrap_or(items.len());
        let chunks: Vec<Vec<&str>> = items.chunks(chunk_size).map(|c| c.to_vec()).collect();

        for chunk in chunks {
            if let Some(ref repl) = replace_str {
                // With -I, substitute REPLACE string
                let item = chunk.first().unwrap_or(&"");
                let cmd: Vec<String> = command.iter().map(|arg| arg.replace(repl, item)).collect();

                // Output the command that would be run
                output.push_str(&cmd.join(" "));
                output.push('\n');
            } else {
                // Append items as arguments
                let mut cmd = command.clone();
                cmd.extend(chunk.iter().map(|s| s.to_string()));

                // Output the command that would be run
                output.push_str(&cmd.join(" "));
                output.push('\n');
            }
        }

        Ok(ExecResult::ok(output))
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
                // Append to existing file or create new one
                ctx.fs.append_file(&path, input.as_bytes()).await?;
            } else {
                // Overwrite file
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

        // Parse arguments
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
                // -d, -t, -b, -e, -g are common watch options, ignore them
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

        // In virtual mode, we just display what the command would be
        // A real implementation would need interpreter support to execute commands
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
#[allow(clippy::unwrap_used)]
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
        };

        let result = Xargs.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("invalid option"));
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
        };

        let result = Tee.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "Hello, world!");

        // Verify file was written
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
        };

        let result = Tee.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "content");

        // Verify both files were written
        let content1 = fs.read_file(&cwd.join("file1.txt")).await.unwrap();
        let content2 = fs.read_file(&cwd.join("file2.txt")).await.unwrap();
        assert_eq!(content1, b"content");
        assert_eq!(content2, b"content");
    }

    #[tokio::test]
    async fn test_tee_append() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        // Create initial file
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
        };

        let result = Tee.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 0);

        // Verify file was appended
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
        };

        let result = Watch.execute(ctx).await.unwrap();
        assert_eq!(result.exit_code, 1);
        assert!(result.stderr.contains("invalid interval"));
    }
}
