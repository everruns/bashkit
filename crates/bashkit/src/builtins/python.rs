//! python/python3 builtin via embedded Monty interpreter (pydantic/monty)
//!
//! Sandboxed Python execution with resource limits. No filesystem or network access.
//! Supports: `python -c "code"`, `python script.py`, stdin piping.

use async_trait::async_trait;
use monty::{
    CollectStringPrint, LimitedTracker, MontyException, MontyObject, MontyRun, ResourceLimits,
};
use std::time::Duration;

use super::{resolve_path, Builtin, Context};
use crate::error::Result;
use crate::interpreter::ExecResult;

/// Default resource limits for sandboxed Python execution.
const DEFAULT_MAX_ALLOCATIONS: usize = 1_000_000;
const DEFAULT_MAX_DURATION: Duration = Duration::from_secs(30);
const DEFAULT_MAX_MEMORY: usize = 64 * 1024 * 1024; // 64 MB
const DEFAULT_MAX_RECURSION: usize = 200;

/// The python/python3 builtin command.
///
/// Executes Python code using the embedded Monty interpreter (pydantic/monty).
/// Operates entirely in-memory with no real filesystem or network access.
///
/// # Usage
///
/// ```bash
/// python3 -c "print('hello')"
/// python3 script.py
/// echo "print('hello')" | python3
/// python3 -c "2 + 2"              # expression result printed
/// python3 --version
/// ```
pub struct Python;

#[async_trait]
impl Builtin for Python {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        let args = ctx.args;

        // python --version / python -V
        if args.first().map(|s| s.as_str()) == Some("--version")
            || args.first().map(|s| s.as_str()) == Some("-V")
        {
            return Ok(ExecResult::ok("Python 3.12.0 (monty)\n".to_string()));
        }

        // python --help / python -h
        if args.first().map(|s| s.as_str()) == Some("--help")
            || args.first().map(|s| s.as_str()) == Some("-h")
        {
            return Ok(ExecResult::ok(
                "usage: python3 [-c cmd | file | -] [arg ...]\n\
                 Options:\n  \
                 -c cmd : execute code from string\n  \
                 file   : execute code from file (VFS)\n  \
                 -      : read code from stdin\n  \
                 -V     : print version\n"
                    .to_string(),
            ));
        }

        let (code, filename) = if let Some(first) = args.first() {
            match first.as_str() {
                "-c" => {
                    // python -c "code"
                    let code = args.get(1).map(|s| s.as_str()).unwrap_or("");
                    if code.is_empty() {
                        return Ok(ExecResult::err(
                            "python3: option -c requires argument\n".to_string(),
                            2,
                        ));
                    }
                    (code.to_string(), "<string>".to_string())
                }
                "-" => {
                    // python - : read from stdin
                    match ctx.stdin {
                        Some(input) if !input.is_empty() => {
                            (input.to_string(), "<stdin>".to_string())
                        }
                        _ => {
                            return Ok(ExecResult::err(
                                "python3: no input from stdin\n".to_string(),
                                1,
                            ));
                        }
                    }
                }
                arg if arg.starts_with('-') => {
                    return Ok(ExecResult::err(
                        format!("python3: unknown option: {arg}\n"),
                        2,
                    ));
                }
                script_path => {
                    // python script.py
                    let path = resolve_path(ctx.cwd, script_path);
                    match ctx.fs.read_file(&path).await {
                        Ok(bytes) => match String::from_utf8(bytes) {
                            Ok(code) => (code, script_path.to_string()),
                            Err(_) => {
                                return Ok(ExecResult::err(
                                    format!(
                                        "python3: can't decode file '{script_path}': not UTF-8\n"
                                    ),
                                    1,
                                ));
                            }
                        },
                        Err(_) => {
                            return Ok(ExecResult::err(
                                format!(
                                    "python3: can't open file '{script_path}': No such file or directory\n"
                                ),
                                2,
                            ));
                        }
                    }
                }
            }
        } else if let Some(input) = ctx.stdin {
            // Piped input without arguments
            if input.is_empty() {
                return Ok(ExecResult::ok(String::new()));
            }
            (input.to_string(), "<stdin>".to_string())
        } else {
            // No args, no stdin — interactive mode not supported
            return Ok(ExecResult::err(
                "python3: interactive mode not supported in sandbox\n".to_string(),
                1,
            ));
        };

        run_python(&code, &filename)
    }
}

/// Execute Python code via Monty with resource limits.
fn run_python(code: &str, filename: &str) -> Result<ExecResult> {
    // Strip shebang if present
    let code = if code.starts_with("#!") {
        match code.find('\n') {
            Some(pos) => &code[pos + 1..],
            None => "",
        }
    } else {
        code
    };

    let runner = match MontyRun::new(code.to_owned(), filename, vec![], vec![]) {
        Ok(r) => r,
        Err(e) => return Ok(format_exception(e)),
    };

    let limits = ResourceLimits::new()
        .max_allocations(DEFAULT_MAX_ALLOCATIONS)
        .max_duration(DEFAULT_MAX_DURATION)
        .max_memory(DEFAULT_MAX_MEMORY)
        .max_recursion_depth(Some(DEFAULT_MAX_RECURSION));

    let tracker = LimitedTracker::new(limits);
    let mut printer = CollectStringPrint::new();

    match runner.run(vec![], tracker, &mut printer) {
        Ok(result) => {
            let mut output = printer.into_output();

            // If the result is not None and there was no print output,
            // display the result (like Python REPL behavior for expressions)
            if !matches!(result, MontyObject::None) && output.is_empty() {
                output = format!("{}\n", result.py_repr());
            }

            Ok(ExecResult::ok(output))
        }
        Err(e) => {
            let printed = printer.into_output();
            Ok(format_exception_with_output(e, &printed))
        }
    }
}

/// Format a MontyException into an ExecResult with exit code 1.
fn format_exception(e: MontyException) -> ExecResult {
    ExecResult::err(format!("{e}\n"), 1)
}

/// Format exception, preserving any output produced before the error.
fn format_exception_with_output(e: MontyException, printed: &str) -> ExecResult {
    let mut stderr = String::new();
    if !printed.is_empty() {
        // Any stdout produced before the error goes to stdout via with_code
        // but error traceback goes to stderr
    }
    stderr.push_str(&format!("{e}\n"));
    let mut result = ExecResult::err(stderr, 1);
    if !printed.is_empty() {
        result.stdout = printed.to_string();
    }
    result
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::builtins::Context;
    use crate::fs::{FileSystem, InMemoryFs};
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Arc;

    async fn run(args: &[&str], stdin: Option<&str>) -> ExecResult {
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let env = HashMap::new();
        let mut variables = HashMap::new();
        let mut cwd = PathBuf::from("/home/user");
        let fs = Arc::new(InMemoryFs::new());
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs, stdin);
        Python.execute(ctx).await.unwrap()
    }

    async fn run_with_file(args: &[&str], file_path: &str, content: &str) -> ExecResult {
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let env = HashMap::new();
        let mut variables = HashMap::new();
        let mut cwd = PathBuf::from("/home/user");
        let fs = Arc::new(InMemoryFs::new());
        fs.write_file(std::path::Path::new(file_path), content.as_bytes())
            .await
            .unwrap();
        let ctx = Context::new_for_test(&args, &env, &mut variables, &mut cwd, fs, None);
        Python.execute(ctx).await.unwrap()
    }

    #[tokio::test]
    async fn test_version() {
        let r = run(&["--version"], None).await;
        assert_eq!(r.exit_code, 0);
        assert!(r.stdout.contains("Python 3.12.0"));
    }

    #[tokio::test]
    async fn test_inline_print() {
        let r = run(&["-c", "print('hello world')"], None).await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "hello world\n");
    }

    #[tokio::test]
    async fn test_inline_expression() {
        let r = run(&["-c", "2 + 3"], None).await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "5\n");
    }

    #[tokio::test]
    async fn test_inline_multiline() {
        let r = run(&["-c", "x = 10\ny = 20\nprint(x + y)"], None).await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "30\n");
    }

    #[tokio::test]
    async fn test_syntax_error() {
        let r = run(&["-c", "def"], None).await;
        assert_eq!(r.exit_code, 1);
        assert!(r.stderr.contains("SyntaxError") || r.stderr.contains("Error"));
    }

    #[tokio::test]
    async fn test_runtime_error() {
        let r = run(&["-c", "1/0"], None).await;
        assert_eq!(r.exit_code, 1);
        assert!(r.stderr.contains("ZeroDivisionError"));
    }

    #[tokio::test]
    async fn test_stdin_code() {
        let r = run(&["-"], Some("print('from stdin')")).await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "from stdin\n");
    }

    #[tokio::test]
    async fn test_piped_stdin() {
        let r = run(&[], Some("print('piped')")).await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "piped\n");
    }

    #[tokio::test]
    async fn test_file_execution() {
        let r = run_with_file(&["script.py"], "/home/user/script.py", "print('from file')").await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "from file\n");
    }

    #[tokio::test]
    async fn test_file_not_found() {
        let r = run(&["missing.py"], None).await;
        assert_eq!(r.exit_code, 2);
        assert!(r.stderr.contains("can't open file"));
    }

    #[tokio::test]
    async fn test_no_args_no_stdin() {
        let r = run(&[], None).await;
        assert_eq!(r.exit_code, 1);
        assert!(r.stderr.contains("interactive mode not supported"));
    }

    #[tokio::test]
    async fn test_c_flag_missing_arg() {
        let r = run(&["-c"], None).await;
        assert_eq!(r.exit_code, 2);
        assert!(r.stderr.contains("requires argument"));
    }

    #[tokio::test]
    async fn test_unknown_option() {
        let r = run(&["-x"], None).await;
        assert_eq!(r.exit_code, 2);
        assert!(r.stderr.contains("unknown option"));
    }

    #[tokio::test]
    async fn test_help() {
        let r = run(&["--help"], None).await;
        assert_eq!(r.exit_code, 0);
        assert!(r.stdout.contains("usage:"));
    }

    // --- Positive tests for features that work ---

    #[tokio::test]
    async fn test_dict_access() {
        let r = run(&["-c", "d = dict()\nd['a'] = 1\nprint(d['a'])"], None).await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "1\n");
    }

    #[tokio::test]
    async fn test_list_comprehension() {
        let r = run(&["-c", "[x*2 for x in range(3)]"], None).await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "[0, 2, 4]\n");
    }

    #[tokio::test]
    async fn test_fstring() {
        let r = run(&["-c", "x = 42\nprint(f'value={x}')"], None).await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "value=42\n");
    }

    #[tokio::test]
    async fn test_recursion_limit() {
        // Should hit recursion limit, not stack overflow
        let r = run(&["-c", "def r(): r()\nr()"], None).await;
        assert_eq!(r.exit_code, 1);
        assert!(r.stderr.contains("RecursionError") || r.stderr.contains("recursion"));
    }

    #[tokio::test]
    async fn test_shebang_stripped() {
        let r = run_with_file(
            &["script.py"],
            "/home/user/script.py",
            "#!/usr/bin/env python3\nprint('shebang ok')",
        )
        .await;
        assert_eq!(r.exit_code, 0);
        assert_eq!(r.stdout, "shebang ok\n");
    }

    #[tokio::test]
    async fn test_name_error() {
        let r = run(&["-c", "print(undefined_var)"], None).await;
        assert_eq!(r.exit_code, 1);
        assert!(r.stderr.contains("NameError"));
    }

    #[tokio::test]
    async fn test_type_error() {
        let r = run(&["-c", "1 + 'a'"], None).await;
        assert_eq!(r.exit_code, 1);
        assert!(r.stderr.contains("TypeError"));
    }

    #[tokio::test]
    async fn test_index_error() {
        let r = run(&["-c", "lst = [1, 2]\nprint(lst[10])"], None).await;
        assert_eq!(r.exit_code, 1);
        assert!(r.stderr.contains("IndexError"));
    }

    #[tokio::test]
    async fn test_empty_stdin() {
        let r = run(&["-"], Some("")).await;
        assert_eq!(r.exit_code, 1);
    }

    #[tokio::test]
    async fn test_output_before_error() {
        let r = run(&["-c", "print('before')\n1/0"], None).await;
        assert_eq!(r.exit_code, 1);
        assert_eq!(r.stdout, "before\n");
        assert!(r.stderr.contains("ZeroDivisionError"));
    }
}
