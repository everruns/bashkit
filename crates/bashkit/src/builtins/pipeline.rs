//! Pipeline control builtins - xargs, tee, watch

use async_trait::async_trait;

use super::{Builtin, Context, ExecutionPlan, SubCommand, resolve_path};
use crate::error::Result;
use crate::interpreter::ExecResult;

/// The xargs builtin - build and execute command lines from stdin.
///
/// Usage: xargs [-I REPLACE] [-n MAX-ARGS] [-d DELIM] [-P N]
///              [--process-slot-var=VAR] [COMMAND [ARGS...]]
///
/// Options:
///   -I REPLACE              Replace REPLACE with input (implies -n 1)
///   -n MAX-ARGS             Use at most MAX-ARGS arguments per command
///   -d DELIM                Use DELIM as delimiter instead of whitespace
///   -0                      Use NUL as delimiter (same as -d '\0')
///   -P N, --max-procs=N     Allocate N parallel slots (see decision below)
///   --process-slot-var=VAR  Set VAR to this invocation's slot index (0..N-1)
///
/// Important decision (parallelism): bashkit runs a single `Bash` interpreter
/// sequentially — even background `&` jobs execute synchronously for
/// deterministic output (see `specs/parallel-execution.md` and
/// `interpreter/jobs.rs`). So `-P N` does NOT spawn N OS processes for
/// wall-clock speedup; instead it allocates N round-robin *slots*, and the
/// commands still run in order. The slot index is exposed via
/// `--process-slot-var`, which is the behaviour real sharding logic depends
/// on (`worker $SLOT of $N`). GNU's own `--process-slot-var` ranges 0..N-1
/// and is 0 when N is 1, so this matches GNU exactly for the deterministic
/// case while staying faithful to bashkit's no-hidden-concurrency model.
pub struct Xargs;

/// Parsed xargs options.
struct XargsOptions {
    replace_str: Option<String>,
    max_args: Option<usize>,
    delimiter: Option<char>,
    /// `-P N` / `--max-procs=N`: number of parallel slots. `Some(0)` means
    /// "as many as possible" (one slot per command). `None` means 1 slot.
    max_procs: Option<usize>,
    /// `--process-slot-var=VAR`: env var to expose the per-command slot index.
    process_slot_var: Option<String>,
    command: Vec<String>,
}

/// Parse xargs arguments, returning options or an error ExecResult.
#[allow(clippy::result_large_err)]
fn parse_xargs_args(args: &[String]) -> std::result::Result<XargsOptions, ExecResult> {
    let mut replace_str: Option<String> = None;
    let mut max_args: Option<usize> = None;
    let mut delimiter: Option<char> = None;
    let mut max_procs: Option<usize> = None;
    let mut process_slot_var: Option<String> = None;
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
        } else if let Some(val) = p
            .flag_value("-P", "xargs")
            .map_err(|e| ExecResult::err(format!("{e}\n"), 1))?
            .or(p
                .long_value("--max-procs", "xargs")
                .map_err(|e| ExecResult::err(format!("{e}\n"), 1))?)
        {
            // -P 0 / --max-procs=0 means "as many as possible" (GNU).
            match val.parse::<usize>() {
                Ok(n) => max_procs = Some(n),
                _ => {
                    return Err(ExecResult::err(
                        format!("xargs: invalid number for -P option: '{}'\n", val),
                        1,
                    ));
                }
            }
        } else if let Some(val) = p
            .long_value("--process-slot-var", "xargs")
            .map_err(|e| ExecResult::err(format!("{e}\n"), 1))?
        {
            if val.is_empty() {
                return Err(ExecResult::err(
                    "xargs: --process-slot-var requires a variable name\n".to_string(),
                    1,
                ));
            }
            process_slot_var = Some(val.to_string());
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
        max_procs,
        process_slot_var,
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

    // Number of parallel slots for --process-slot-var assignment. `-P 0`
    // ("as many as possible") gives every command a distinct slot; absent
    // `-P`, GNU uses a single slot so the index is always 0.
    let slot_count = match opts.max_procs {
        Some(0) => chunks.len().max(1),
        Some(n) => n,
        None => 1,
    };

    chunks
        .into_iter()
        .enumerate()
        .map(|(idx, chunk)| {
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

            let assignments = match opts.process_slot_var {
                Some(ref var) => vec![(var.clone(), (idx % slot_count).to_string())],
                None => Vec::new(),
            };

            let name = cmd_args[0].clone();
            let args = cmd_args[1..].to_vec();
            SubCommand {
                name,
                args,
                stdin: None,
                assignments,
            }
        })
        .collect()
}

#[async_trait]
impl Builtin for Xargs {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        if let Some(r) = super::check_help_version(
            ctx.args,
            "Usage: xargs [OPTION]... [COMMAND [ARGS]...]\nBuild and execute command lines from standard input.\n\n  -I REPLACE\treplace REPLACE with input (implies -n 1)\n  -n MAX-ARGS\tuse at most MAX-ARGS arguments per command\n  -d DELIM\tuse DELIM as delimiter instead of whitespace\n  -0\tuse NUL as delimiter\n  -P, --max-procs=N\tallocate N parallel slots (runs sequentially)\n  --process-slot-var=VAR\tset VAR to the slot index (0..N-1)\n  --help\tdisplay this help and exit\n  --version\toutput version information and exit\n",
            Some("xargs (bashkit) 0.1"),
        ) {
            return Ok(r);
        }
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

        // Fallback: output what would be run (for standalone builtin context).
        // Command-scoped assignments (e.g. the --process-slot-var index) are
        // rendered as a `VAR=value` prefix so the slot is visible here too.
        let mut output = String::new();
        for cmd in &commands {
            for (var, val) in &cmd.assignments {
                output.push_str(var);
                output.push('=');
                output.push_str(val);
                output.push(' ');
            }
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
///   -a, --append              Append to files instead of overwriting
///   -i, --ignore-interrupts   No-op in bashkit's virtual mode (no signals)
///   -p                        Diagnose only non-pipe write errors
///   --output-error[=MODE]     Set write-error behavior (parsed but reduced
///                              to bashkit's all-or-nothing VFS write model)
///
/// Argument surface is generated from uutils/coreutils' `uu_app()` via
/// the `bashkit-coreutils-port` codegen tool — see
/// `generated/tee_args.rs`. Behaviour is implemented locally against
/// the bashkit VFS.
pub struct Tee;

#[async_trait]
impl Builtin for Tee {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        use super::generated::tee_args::tee_command;
        use std::ffi::OsString;

        let argv: Vec<OsString> = std::iter::once(OsString::from("tee"))
            .chain(ctx.args.iter().map(OsString::from))
            .collect();

        let cmd = tee_command().help_template("Usage: {usage}\n{about}\n\n{all-args}\n");
        let matches = match cmd.try_get_matches_from(argv) {
            Ok(m) => m,
            Err(e) => {
                let kind = e.kind();
                let rendered = e.render().to_string();
                if matches!(
                    kind,
                    clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion
                ) {
                    return Ok(ExecResult::ok(rendered));
                }
                return Ok(ExecResult::err(rendered, 2));
            }
        };

        let append = matches.get_flag("append");
        // -i/--ignore-interrupts and -p are accepted but irrelevant in
        // bashkit: there are no signals and no pipe errors in the VFS
        // write model. Read them so clap counts them as consumed.
        let _ = matches.get_flag("ignore-interrupts");
        let _ = matches.get_flag("ignore-pipe-errors");
        let _ = matches.get_one::<String>("output-error");

        let files: Vec<String> = matches
            .get_many::<OsString>("file")
            .map(|vs| vs.map(|v| v.to_string_lossy().into_owned()).collect())
            .unwrap_or_default();

        let input = ctx.stdin.unwrap_or("");

        for file in &files {
            // tee(1): "If a FILE is -, it refers to a file named - ."
            // The codegen output documents the same in `after_help`.
            let path = resolve_path(ctx.cwd, file);

            if append {
                ctx.fs.append_file(&path, input.as_bytes()).await?;
            } else {
                ctx.fs.write_file(&path, input.as_bytes()).await?;
            }
        }

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
        if let Some(r) = super::check_help_version(
            ctx.args,
            "Usage: watch [OPTION]... COMMAND\nExecute a program periodically, showing output.\n\n  -n SECONDS\tupdate interval (default: 2)\n  --help\tdisplay this help and exit\n  --version\toutput version information and exit\n",
            Some("watch (bashkit) 0.1"),
        ) {
            return Ok(r);
        }
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

    #[tokio::test]
    async fn test_xargs_p_option_accepted() {
        // -P must no longer be rejected as an invalid option.
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        let args = vec!["-P".to_string(), "4".to_string(), "echo".to_string()];
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
        assert!(result.stdout.contains("echo a b c"));
    }

    #[tokio::test]
    async fn test_xargs_p_invalid_number() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        let args = vec!["-P".to_string(), "abc".to_string(), "echo".to_string()];
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs: fs.clone(),
            stdin: Some("a"),
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
        assert!(result.stderr.contains("invalid number for -P"));
    }

    #[tokio::test]
    async fn test_xargs_process_slot_var_round_robin() {
        // -P N with --process-slot-var assigns slots 0..N-1 round-robin.
        // The fallback rendering shows them as a `VAR=value` prefix.
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        let args = vec![
            "-P".to_string(),
            "2".to_string(),
            "--process-slot-var=SLOT".to_string(),
            "-n".to_string(),
            "1".to_string(),
            "echo".to_string(),
        ];
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs: fs.clone(),
            stdin: Some("a b c d"),
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
        assert_eq!(
            lines,
            vec![
                "SLOT=0 echo a",
                "SLOT=1 echo b",
                "SLOT=0 echo c",
                "SLOT=1 echo d",
            ]
        );
    }

    #[tokio::test]
    async fn test_xargs_process_slot_var_single_slot_is_zero() {
        // Without -P there is one slot, so the index is always 0 (GNU parity).
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        let args = vec![
            "--process-slot-var".to_string(),
            "S".to_string(),
            "-n".to_string(),
            "1".to_string(),
            "echo".to_string(),
        ];
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs: fs.clone(),
            stdin: Some("a b"),
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
        assert_eq!(lines, vec!["S=0 echo a", "S=0 echo b"]);
    }

    #[tokio::test]
    async fn test_xargs_plan_carries_slot_assignment() {
        // The execution plan must carry the per-command slot assignment so the
        // interpreter runs each command with `VAR=slot cmd ...`.
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        let args = vec![
            "-P".to_string(),
            "2".to_string(),
            "--process-slot-var=SLOT".to_string(),
            "-n".to_string(),
            "1".to_string(),
            "echo".to_string(),
        ];
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
                assert_eq!(
                    commands[0].assignments,
                    vec![("SLOT".to_string(), "0".to_string())]
                );
                assert_eq!(
                    commands[1].assignments,
                    vec![("SLOT".to_string(), "1".to_string())]
                );
                assert_eq!(
                    commands[2].assignments,
                    vec![("SLOT".to_string(), "0".to_string())]
                );
            }
            _ => panic!("expected Batch plan"),
        }
    }

    #[tokio::test]
    async fn test_xargs_max_procs_long_form() {
        let (fs, mut cwd, mut variables) = create_test_ctx().await;
        let env = HashMap::new();

        let args = vec![
            "--max-procs=3".to_string(),
            "--process-slot-var=S".to_string(),
            "-n".to_string(),
            "1".to_string(),
            "echo".to_string(),
        ];
        let ctx = Context {
            args: &args,
            env: &env,
            variables: &mut variables,
            cwd: &mut cwd,
            fs: fs.clone(),
            stdin: Some("a b c d"),
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
        assert_eq!(
            lines,
            vec!["S=0 echo a", "S=1 echo b", "S=2 echo c", "S=0 echo d",]
        );
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
        // Unknown flag: clap returns exit code 2 with its own
        // "unexpected argument" diagnostic. GNU coreutils' tee exits
        // 1 with "invalid option". The clap-vs-GNU divergence is
        // documented in `tests/spec_cases/bash/tee.test.sh`.
        assert_eq!(result.exit_code, 2);
        assert!(
            result.stderr.contains("unexpected argument")
                || result.stderr.contains("invalid option")
        );
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
