// Decision: these tests spawn the real `bashkit` binary (CARGO_BIN_EXE_bashkit)
// instead of calling `build_bash()` + `bash.exec()` in-process like the unit
// tests in `src/main.rs`. Only a subprocess can observe the parts of the CLI
// contract that are the process itself: exit status, stream separation, the
// anyhow error path out of `main`, and argv shapes clap has to resolve.
//
// Decision: every child runs with `RUST_BACKTRACE` cleared. anyhow prints a
// backtrace on the `Error:` path when that variable is set, so leaving the
// ambient value in place would make assertions depend on the developer's shell.
// `no_backtrace_without_rust_backtrace_env` pins the cleared-env behavior
// deliberately (TM-INF-021).
//
// Decision: tests that document a divergence from bash rather than a
// guarantee say so in a comment and reference the limitation ID in
// `knowledge/operations/limitations.md`, so lifting one has to update the row
// and the test together.

use std::io::Write;
use std::process::{Command, Output, Stdio};

const BIN: &str = env!("CARGO_BIN_EXE_bashkit");

fn cli() -> Command {
    let mut cmd = Command::new(BIN);
    cmd.env_remove("RUST_BACKTRACE");
    cmd
}

/// Run the CLI with the given argv tail and no stdin.
fn run(args: &[&str]) -> Output {
    cli().args(args).output().expect("spawn bashkit")
}

/// Run the CLI with `input` written to the child's stdin.
fn run_with_stdin(args: &[&str], input: &str) -> Output {
    let mut child = cli()
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn bashkit");
    child
        .stdin
        .take()
        .expect("stdin piped")
        .write_all(input.as_bytes())
        .expect("write stdin");
    child.wait_with_output().expect("wait for bashkit")
}

fn stdout(out: &Output) -> String {
    String::from_utf8(out.stdout.clone()).expect("stdout is utf-8")
}

fn stderr(out: &Output) -> String {
    String::from_utf8(out.stderr.clone()).expect("stderr is utf-8")
}

fn code(out: &Output) -> i32 {
    out.status.code().expect("child exited without a signal")
}

// ---------------------------------------------------------------------------
// Exit status
// ---------------------------------------------------------------------------

#[test]
fn success_exits_zero() {
    let out = run(&["-c", "echo hi"]);
    assert_eq!(code(&out), 0, "stderr: {}", stderr(&out));
    assert_eq!(stdout(&out), "hi\n");
}

#[test]
fn explicit_exit_code_propagates_to_process_status() {
    let out = run(&["-c", "exit 3"]);
    assert_eq!(code(&out), 3);
}

#[test]
fn failing_command_exits_one() {
    let out = run(&["-c", "false"]);
    assert_eq!(code(&out), 1);
}

#[test]
fn last_command_determines_exit_status() {
    let out = run(&["-c", "false; true"]);
    assert_eq!(code(&out), 0);

    let out = run(&["-c", "true; exit 42"]);
    assert_eq!(code(&out), 42);
}

#[test]
fn command_not_found_exits_127() {
    let out = run(&["-c", "definitely-not-a-command"]);
    assert_eq!(code(&out), 127);
    assert!(
        stderr(&out).contains("command not found"),
        "stderr: {}",
        stderr(&out)
    );
}

#[test]
fn script_mode_propagates_exit_code() {
    let dir = tempfile::tempdir().unwrap();
    let script = dir.path().join("s.sh");
    std::fs::write(&script, "echo scripted\nexit 7\n").unwrap();

    let out = run(&[script.to_str().unwrap()]);
    assert_eq!(code(&out), 7);
    assert_eq!(stdout(&out), "scripted\n");
}

// ---------------------------------------------------------------------------
// Stream routing
// ---------------------------------------------------------------------------

#[test]
fn stdout_and_stderr_are_separate_streams() {
    let out = run(&["-c", "echo to-out; echo to-err >&2"]);
    assert_eq!(code(&out), 0);
    assert_eq!(stdout(&out), "to-out\n");
    assert_eq!(stderr(&out), "to-err\n");
}

#[test]
fn silent_command_writes_nothing() {
    let out = run(&["-c", ":"]);
    assert_eq!(stdout(&out), "");
    assert_eq!(stderr(&out), "");
    assert_eq!(code(&out), 0);
}

#[test]
fn stdout_is_written_verbatim_without_added_newline() {
    // `print!` is used for stdout, so a script that emits no trailing newline
    // must not get one from the CLI.
    let out = run(&["-c", "printf no-newline"]);
    assert_eq!(stdout(&out), "no-newline");
}

#[test]
fn large_stdout_is_not_truncated() {
    let out = run(&["-c", "for i in $(seq 1 5000); do echo line$i; done"]);
    assert_eq!(code(&out), 0, "stderr: {}", stderr(&out));
    let text = stdout(&out);
    assert_eq!(text.lines().count(), 5000);
    assert!(text.starts_with("line1\n"));
    assert!(text.ends_with("line5000\n"));
}

#[test]
fn stdout_survives_a_nonzero_exit() {
    // Output is buffered until the end of the run and printed before
    // `process::exit`; a failing script must not lose what it already wrote.
    let out = run(&["-c", "echo before-failure; exit 9"]);
    assert_eq!(code(&out), 9);
    assert_eq!(stdout(&out), "before-failure\n");
}

// ---------------------------------------------------------------------------
// Error path out of `main`
// ---------------------------------------------------------------------------

#[test]
fn parse_error_reports_on_stderr_and_exits_nonzero() {
    let out = run(&["-c", "if ["]);
    assert_ne!(code(&out), 0);
    let err = stderr(&out);
    assert!(err.contains("parse error"), "stderr: {err}");
    assert_eq!(stdout(&out), "", "parse errors must not write to stdout");
}

#[test]
fn missing_script_file_reports_the_path() {
    let out = run(&["/definitely/missing/script.sh"]);
    assert_ne!(code(&out), 0);
    let err = stderr(&out);
    assert!(err.contains("Failed to read script"), "stderr: {err}");
    assert!(
        err.contains("/definitely/missing/script.sh"),
        "stderr: {err}"
    );
}

#[test]
fn no_backtrace_without_rust_backtrace_env() {
    // THREAT[TM-INF-021]: the anyhow error path must not print dependency
    // paths, crate versions, or rustc hashes when the operator has not asked
    // for a backtrace.
    let out = run(&["-c", "if ["]);
    let err = stderr(&out);
    assert!(!err.contains("Stack backtrace"), "stderr: {err}");
    assert!(!err.contains(".cargo/registry"), "stderr: {err}");
    assert!(!err.contains("/rustc/"), "stderr: {err}");
}

// ---------------------------------------------------------------------------
// Resource-limit flags
// ---------------------------------------------------------------------------

#[test]
fn default_limits_bound_an_infinite_loop() {
    // One-shot mode uses sandboxed defaults (not `ExecutionLimits::cli()`), so
    // this must terminate on its own rather than hang the test run.
    let out = run(&["-c", "while true; do :; done"]);
    assert_ne!(code(&out), 0);
    let err = stderr(&out);
    assert!(
        err.contains("maximum command count")
            || err.contains("maximum loop iterations")
            || err.contains("maximum total loop iterations"),
        "stderr: {err}"
    );
}

#[test]
fn max_commands_flag_is_enforced() {
    let out = run(&["--max-commands", "2", "-c", "echo a; echo b; echo c"]);
    assert_ne!(code(&out), 0);
    assert!(
        stderr(&out).contains("maximum command count exceeded (2)"),
        "stderr: {}",
        stderr(&out)
    );
}

#[test]
fn max_loop_iterations_flag_is_enforced() {
    let out = run(&[
        "--max-loop-iterations",
        "3",
        "-c",
        "for i in 1 2 3 4 5 6 7 8 9; do echo $i; done",
    ]);
    assert_ne!(code(&out), 0);
    assert!(
        stderr(&out).contains("maximum loop iterations exceeded (3)"),
        "stderr: {}",
        stderr(&out)
    );
}

#[test]
fn max_total_loop_iterations_flag_is_enforced() {
    let out = run(&[
        "--max-total-loop-iterations",
        "4",
        "-c",
        "for i in 1 2 3; do echo $i; done; for j in 1 2 3; do echo $j; done",
    ]);
    assert_ne!(code(&out), 0);
    assert!(
        stderr(&out).contains("maximum total loop iterations exceeded (4)"),
        "stderr: {}",
        stderr(&out)
    );
}

#[test]
fn timeout_flag_is_enforced() {
    let out = run(&["--timeout", "1", "-c", "sleep 30"]);
    assert_ne!(code(&out), 0);
    assert!(
        stderr(&out).contains("execution timeout"),
        "stderr: {}",
        stderr(&out)
    );
}

#[test]
fn generous_limits_do_not_trip() {
    let out = run(&[
        "--max-commands",
        "10000",
        "--max-loop-iterations",
        "10000",
        "--timeout",
        "60",
        "-c",
        "for i in $(seq 1 200); do echo $i; done",
    ]);
    assert_eq!(code(&out), 0, "stderr: {}", stderr(&out));
    assert_eq!(stdout(&out).lines().count(), 200);
}

// ---------------------------------------------------------------------------
// Capability flags, observed through the real binary
// ---------------------------------------------------------------------------

#[test]
fn http_is_denied_by_default() {
    let out = run(&["-c", "curl https://example.com"]);
    assert_ne!(code(&out), 0);
    assert!(
        stderr(&out).contains("network access not configured"),
        "stderr: {}",
        stderr(&out)
    );
}

#[test]
fn no_http_and_http_allow_all_conflict() {
    let out = run(&["--no-http", "--http-allow-all", "-c", "echo hi"]);
    assert_ne!(code(&out), 0);
    assert!(
        stderr(&out).contains("cannot be used with"),
        "stderr: {}",
        stderr(&out)
    );
}

#[test]
fn git_is_enabled_by_default() {
    let out = run(&["-c", "git init /repo && echo ok"]);
    assert_eq!(code(&out), 0, "stderr: {}", stderr(&out));
    assert!(stdout(&out).contains("ok"));
}

#[test]
fn no_git_disables_the_git_builtin() {
    let out = run(&["--no-git", "-c", "git init /repo"]);
    assert_ne!(code(&out), 0);
    assert!(
        stderr(&out).contains("not configured"),
        "stderr: {}",
        stderr(&out)
    );
}

#[cfg(feature = "python")]
#[test]
fn python_is_enabled_by_default() {
    let out = run(&["-c", "python -c 'print(2 + 2)'"]);
    assert_eq!(code(&out), 0, "stderr: {}", stderr(&out));
    assert_eq!(stdout(&out), "4\n");
}

#[cfg(feature = "python")]
#[test]
fn no_python_disables_the_python_builtin() {
    let out = run(&["--no-python", "-c", "python --version"]);
    assert_ne!(code(&out), 0);
    assert!(
        stderr(&out).contains("command not found"),
        "stderr: {}",
        stderr(&out)
    );
}

#[cfg(feature = "sqlite")]
#[test]
fn sqlite_is_enabled_by_default() {
    let out = run(&["-c", "sqlite :memory: 'SELECT 1'"]);
    assert_eq!(code(&out), 0, "stderr: {}", stderr(&out));
    assert_eq!(stdout(&out).trim(), "1");
}

#[cfg(feature = "sqlite")]
#[test]
fn no_sqlite_disables_the_sqlite_builtin() {
    let out = run(&["--no-sqlite", "-c", "sqlite :memory: 'SELECT 1'"]);
    assert_ne!(code(&out), 0);
    assert!(
        stderr(&out).contains("command not found"),
        "stderr: {}",
        stderr(&out)
    );
}

// ---------------------------------------------------------------------------
// argv shapes
// ---------------------------------------------------------------------------

#[test]
fn empty_command_string_is_a_no_op() {
    let out = run(&["-c", ""]);
    assert_eq!(code(&out), 0);
    assert_eq!(stdout(&out), "");
}

#[test]
fn command_string_may_start_with_a_dash() {
    // clap must treat the `-c` value as data, not as more flags.
    let out = run(&["-c", "echo -n dashed"]);
    assert_eq!(code(&out), 0, "stderr: {}", stderr(&out));
    assert_eq!(stdout(&out), "dashed");
}

#[test]
fn multiline_command_string_runs_every_line() {
    let out = run(&["-c", "echo one\necho two\n"]);
    assert_eq!(code(&out), 0, "stderr: {}", stderr(&out));
    assert_eq!(stdout(&out), "one\ntwo\n");
}

#[test]
fn version_flag_prints_version_and_exits_zero() {
    let out = run(&["--version"]);
    assert_eq!(code(&out), 0);
    assert!(stdout(&out).starts_with("bashkit "), "{}", stdout(&out));
}

#[test]
fn help_flag_exits_zero_and_documents_c() {
    let out = run(&["--help"]);
    assert_eq!(code(&out), 0);
    let text = stdout(&out);
    assert!(text.contains("-c"), "{text}");
    assert!(text.contains("--timeout"), "{text}");
}

#[test]
fn unknown_flag_is_rejected() {
    let out = run(&["--not-a-flag", "-c", "echo hi"]);
    assert_ne!(code(&out), 0);
    assert!(
        stderr(&out).contains("unexpected argument"),
        "stderr: {}",
        stderr(&out)
    );
}

// ---------------------------------------------------------------------------
// Positional parameters
// ---------------------------------------------------------------------------

#[test]
fn trailing_args_become_positional_params() {
    // Bash semantics: after `-c 'script'` the first trailing argument is `$0`
    // and the rest are `$1`...
    let out = run(&[
        "-c",
        "echo \"0=$0 1=$1 2=$2 count=$#\"",
        "myname",
        "foo",
        "bar",
    ]);
    assert_eq!(code(&out), 0, "stderr: {}", stderr(&out));
    assert_eq!(stdout(&out), "0=myname 1=foo 2=bar count=2\n");
}

#[test]
fn dollar_zero_defaults_when_no_trailing_args() {
    let out = run(&["-c", "echo \"0=$0 count=$#\""]);
    assert_eq!(code(&out), 0, "stderr: {}", stderr(&out));
    assert_eq!(stdout(&out), "0=bash count=0\n");
}

#[test]
fn positional_params_preserve_arguments_with_spaces() {
    let out = run(&[
        "-c",
        "for a in \"$@\"; do echo \"[$a]\"; done",
        "n0",
        "sp ace",
        "b",
    ]);
    assert_eq!(code(&out), 0, "stderr: {}", stderr(&out));
    assert_eq!(stdout(&out), "[sp ace]\n[b]\n");
}

#[test]
fn script_mode_exposes_args_and_uses_the_path_as_dollar_zero() {
    let dir = tempfile::tempdir().unwrap();
    let script = dir.path().join("args.sh");
    std::fs::write(
        &script,
        "echo \"count=$# first=$1 all=$@\"\nbasename \"$0\"\n",
    )
    .unwrap();

    let out = run(&[script.to_str().unwrap(), "alpha", "beta"]);
    assert_eq!(code(&out), 0, "stderr: {}", stderr(&out));
    assert_eq!(
        stdout(&out),
        "count=2 first=alpha all=alpha beta\nargs.sh\n"
    );
}

#[test]
fn script_can_shift_and_reset_positional_params() {
    let out = run(&[
        "-c",
        "shift; echo \"$# $1\"; set -- z; echo \"$# $1 $0\"",
        "n0",
        "a",
        "b",
    ]);
    assert_eq!(code(&out), 0, "stderr: {}", stderr(&out));
    assert_eq!(stdout(&out), "1 b\n1 z n0\n");
}

// ---------------------------------------------------------------------------
// Stdin forwarding
// ---------------------------------------------------------------------------

#[test]
fn piped_stdin_reaches_the_script() {
    let out = run_with_stdin(&["-c", "cat"], "piped-in\nsecond\n");
    assert_eq!(code(&out), 0, "stderr: {}", stderr(&out));
    assert_eq!(stdout(&out), "piped-in\nsecond\n");
}

#[test]
fn read_consumes_piped_stdin() {
    let out = run_with_stdin(
        &["-c", "read -r line; echo \"got=[$line]\""],
        "hello\nrest\n",
    );
    assert_eq!(stdout(&out), "got=[hello]\n", "stderr: {}", stderr(&out));
}

#[test]
fn piped_stdin_feeds_a_filter() {
    let out = run_with_stdin(&["-c", "grep -c line"], "one line\ntwo line\nnope\n");
    assert_eq!(code(&out), 0, "stderr: {}", stderr(&out));
    assert_eq!(stdout(&out).trim(), "2");
}

#[test]
fn no_stdin_flag_suppresses_forwarding() {
    let out = run_with_stdin(&["--no-stdin", "-c", "cat"], "ignored\n");
    assert_eq!(code(&out), 0, "stderr: {}", stderr(&out));
    assert_eq!(stdout(&out), "");
}

#[test]
fn an_inline_pipe_still_wins_over_host_stdin() {
    let out = run_with_stdin(&["-c", "echo inline | cat"], "outer\n");
    assert_eq!(stdout(&out), "inline\n", "stderr: {}", stderr(&out));
}

#[test]
fn stdin_is_consumed_even_when_the_script_ignores_it() {
    // L-CLI-002: stdin is read to EOF before execution rather than lazily, so
    // a script that never reads still drains the pipe. Harmless for a writer
    // that finishes; a writer that never closes would block the run.
    let out = run_with_stdin(&["-c", "echo hi"], "unread\n");
    assert_eq!(code(&out), 0, "stderr: {}", stderr(&out));
    assert_eq!(stdout(&out), "hi\n");
}

#[test]
fn empty_stdin_is_harmless() {
    let out = run_with_stdin(&["-c", "cat; echo done"], "");
    assert_eq!(code(&out), 0, "stderr: {}", stderr(&out));
    assert_eq!(stdout(&out), "done\n");
}

// ---------------------------------------------------------------------------
// Streaming
// ---------------------------------------------------------------------------

#[test]
fn output_produced_before_an_abort_is_kept() {
    // Streaming writes each chunk through as it is produced, so a run killed
    // by a resource limit keeps what it already printed.
    let out = run(&["--timeout", "1", "-c", "echo early-output; sleep 30"]);
    assert_ne!(code(&out), 0);
    assert_eq!(stdout(&out), "early-output\n");
    assert!(
        stderr(&out).contains("execution timeout"),
        "stderr: {}",
        stderr(&out)
    );
}

#[test]
fn output_before_a_limit_abort_is_kept() {
    let out = run(&[
        "--max-commands",
        "2",
        "-c",
        "echo one; echo two; echo three; echo four",
    ]);
    assert_ne!(code(&out), 0);
    assert!(
        stdout(&out).starts_with("one\n"),
        "stdout: {:?}",
        stdout(&out)
    );
}

#[test]
fn streamed_output_is_not_printed_twice() {
    let out = run(&["-c", "echo once; echo twice >&2"]);
    assert_eq!(stdout(&out), "once\n");
    assert_eq!(stderr(&out), "twice\n");
}
