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

use std::io::{Read, Write};
use std::process::{Command, Output, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

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

// THREAT[TM-DOS-020]: run in a child so a stack-overflow regression fails
// this test instead of aborting the entire Rust test process.
#[cfg(unix)]
fn run_with_two_mib_stack(script: &str) -> Output {
    Command::new("sh")
        .args([
            "-c",
            "set -e; ulimit -s 2048; exec \"$1\" --profile standard --no-stdin -c \"$2\"",
            "sh",
            BIN,
            script,
        ])
        .env_remove("RUST_BACKTRACE")
        .output()
        .expect("spawn bashkit with small stack")
}

#[cfg(unix)]
#[test]
fn function_recursion_is_bounded_on_a_two_mib_stack() {
    for script in ["f() { f; }; f", "f() { g; }; g() { f; }; f"] {
        let out = run_with_two_mib_stack(script);
        assert_eq!(out.status.code(), Some(1), "{script}: {}", stderr(&out));
        assert!(
            stderr(&out).contains("maximum function depth exceeded"),
            "{script}: {}",
            stderr(&out)
        );
    }
}

// Decision: hostile-recursion probes run through this fail-closed launcher,
// not plain `.output()`. It enforces stack (2 MiB), CPU (10 s), output
// (64 KiB per stream), and wall (20 s) bounds *during* execution: reader
// threads drain both pipes concurrently with byte caps, the poll loop kills
// on oversize/timeout, reap and reader completion are bounded, and timeout /
// oversize / incomplete-reader state is preserved on the result instead of
// hanging or OOMing the parent. A child abort surfaces as its real exit
// status, never as a passing test.
#[cfg(unix)]
struct ProbeRun {
    exit_code: Option<i32>,
    signaled: bool,
    timed_out: bool,
    oversized: bool,
    incomplete_readers: bool,
    limits_note: String,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

#[cfg(unix)]
fn run_bounded_probe(script: &str) -> ProbeRun {
    const STREAM_CAP: usize = 64 * 1024;
    let mut child = Command::new("sh")
        .args([
            "-c",
            "ulimit -s 2048 || exit 99; ulimit -t 10 || exit 99; echo \"harness-limits: stack=$(ulimit -s) cpu=$(ulimit -t)\" >&2; exec \"$1\" --no-stdin --max-commands 64 -c \"$2\"",
            "sh",
            BIN,
            script,
        ])
        .env_remove("RUST_BACKTRACE")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn signal trap probe");
    let stdout_pipe: Box<dyn Read + Send> = Box::new(child.stdout.take().expect("probe stdout"));
    let stderr_pipe: Box<dyn Read + Send> = Box::new(child.stderr.take().expect("probe stderr"));
    // Concurrent capped readers: a hostile child must neither fill a pipe
    // (parent/child deadlock) nor exhaust parent memory (unbounded output).
    // A reader stops at the cap; the wall watchdog then reaps the child and
    // the oversize flag fails the test.
    let (tx, rx) = mpsc::channel::<(bool, Vec<u8>, bool)>();
    for (pipe, is_stdout) in [(stdout_pipe, true), (stderr_pipe, false)] {
        let tx = tx.clone();
        thread::spawn(move || {
            let mut pipe = pipe;
            let mut buf = Vec::new();
            let mut chunk = [0u8; 8192];
            let mut oversized = false;
            loop {
                if buf.len() >= STREAM_CAP {
                    oversized = true;
                    break;
                }
                match pipe.read(&mut chunk) {
                    Ok(0) => break,
                    Ok(n) => buf.extend_from_slice(&chunk[..n]),
                    Err(_) => break,
                }
            }
            let _ = tx.send((is_stdout, buf, oversized));
        });
    }
    drop(tx);
    let start = Instant::now();
    let mut timed_out = false;
    loop {
        match child.try_wait().expect("poll signal trap probe") {
            Some(_) => break,
            None if start.elapsed() >= Duration::from_secs(20) => {
                timed_out = true;
                let _ = child.kill();
                break;
            }
            None => thread::sleep(Duration::from_millis(50)),
        }
    }
    // Bounded reap: a kill that does not land still ends the test instead
    // of hanging the suite. Capture the real exit status here.
    let status = child.wait().expect("reap signal trap probe");
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let mut oversized = false;
    let mut incomplete_readers = false;
    for _ in 0..2 {
        match rx.recv_timeout(Duration::from_secs(5)) {
            Ok((true, buf, over)) => {
                stdout = buf;
                oversized |= over;
            }
            Ok((false, buf, over)) => {
                stderr = buf;
                oversized |= over;
            }
            Err(_) => incomplete_readers = true,
        }
    }
    use std::os::unix::process::ExitStatusExt;
    let stderr_text = String::from_utf8_lossy(&stderr).into_owned();
    let limits_note = stderr_text
        .lines()
        .find(|line| line.starts_with("harness-limits:"))
        .unwrap_or("")
        .to_owned();
    ProbeRun {
        exit_code: status.code(),
        signaled: status.signal().is_some(),
        timed_out,
        oversized,
        incomplete_readers,
        limits_note,
        stdout,
        stderr,
    }
}

#[cfg(unix)]
fn dump_probe_logs(case: &str, run: &ProbeRun) -> (String, String) {
    let safe: String = case
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .take(32)
        .collect();
    let dir = std::env::temp_dir();
    let out_path = dir.join(format!("bashkit-signal-probe-{safe}.stdout"));
    let err_path = dir.join(format!("bashkit-signal-probe-{safe}.stderr"));
    let _ = std::fs::write(&out_path, &run.stdout);
    let _ = std::fs::write(&err_path, &run.stderr);
    (
        out_path.display().to_string(),
        err_path.display().to_string(),
    )
}

// Signal-trap self-delivery (`kill -USR1 $$` from inside a USR1 handler)
// recurses through the interpreter instead of returning to the host loop.
// The dedicated nesting cap plus the shared depth budget stop
// direct/mutual/function/source cycles with a resource-limit error instead
// of overflowing the child stack, and finite nesting keeps working. Each
// hostile shape runs in a fail-closed child (2 MiB stack, CPU/wall/output
// caps); an abort is never a passing test. See TM-DOS-020 and
// `signal_trap_depth_tests`.
#[cfg(unix)]
#[test]
fn signal_trap_recursion_is_bounded_on_a_two_mib_stack() {
    for (case, script) in [
        ("direct", "trap 'kill -USR1 $$' USR1; kill -USR1 $$"),
        (
            "mutual",
            "trap 'kill -USR2 $$' USR1; trap 'kill -USR1 $$' USR2; kill -USR1 $$",
        ),
        (
            "function",
            "f() { kill -USR1 $$; }; trap 'f' USR1; kill -USR1 $$",
        ),
        (
            "source",
            "echo 'kill -USR1 $$' > /handler; trap 'source /handler' USR1; kill -USR1 $$",
        ),
    ] {
        let run = run_bounded_probe(script);
        if run.timed_out
            || run.oversized
            || run.incomplete_readers
            || run.signaled
            || run.exit_code != Some(1)
            || !run.limits_note.contains("stack=2048")
            || !run.limits_note.contains("cpu=10")
            || !String::from_utf8_lossy(&run.stderr).contains("depth exceeded")
        {
            let (out_path, err_path) = dump_probe_logs(case, &run);
            panic!(
                "{case}: fail-closed probe violated: exit={:?} signaled={} timed_out={} oversized={} incomplete_readers={} limits={:?} stdout={:?} stderr={:?} logs={out_path} {err_path}",
                run.exit_code,
                run.signaled,
                run.timed_out,
                run.oversized,
                run.incomplete_readers,
                run.limits_note,
                String::from_utf8_lossy(&run.stdout),
                String::from_utf8_lossy(&run.stderr),
            );
        }
    }
}

#[cfg(unix)]
#[test]
fn function_nesting_below_limit_works_on_a_two_mib_stack() {
    let out = run_with_two_mib_stack(
        "f() { if [ \"$1\" -eq 0 ]; then echo ok; else f \"$(( $1 - 1 ))\"; fi; }; f 8",
    );
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    assert_eq!(stdout(&out), "ok\n");
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
    // bash exits 2 on a syntax error.
    assert_eq!(code(&out), 2);
    // Worded and numbered like bash: input ran out, so the line after the last.
    assert_eq!(
        stderr(&out),
        "bash: -c: line 2: syntax error: unexpected end of file\n"
    );
    assert_eq!(stdout(&out), "", "parse errors must not write to stdout");
}

#[test]
fn script_parse_error_exits_2_with_or_without_earlier_commands() {
    let dir = tempfile::tempdir().unwrap();
    for (body, want_stdout, line) in [
        ("[[ abc =~ ( ]]\n", "", 1),
        ("echo ok\n[[ abc =~ ( ]]\n", "ok\n", 2),
    ] {
        let script = dir.path().join("bad.sh");
        std::fs::write(&script, body).unwrap();
        let path = script.to_str().unwrap();
        let out = run(&[path]);
        assert_eq!(code(&out), 2, "{body:?}");
        assert_eq!(stdout(&out), want_stdout, "{body:?}");
        // bash's two diagnostics, named by the script path.
        assert_eq!(
            stderr(&out),
            format!(
                "{path}: line {line}: unexpected EOF while looking for matching `)'\n\
                 {path}: line {}: unexpected argument to conditional binary operator\n",
                line + 1
            ),
            "{body:?}"
        );
    }
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
    // a script that never reads still drains a writer that finishes.
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

#[test]
fn timeout_bounds_an_idle_open_stdin_pipe() {
    let mut child = cli()
        .args(["--timeout", "1", "-c", "echo unreachable"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn bashkit");
    let open_stdin = child.stdin.take().expect("stdin piped");
    let deadline = Instant::now() + Duration::from_secs(3);

    while child.try_wait().expect("poll bashkit").is_none() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(20));
    }
    if child.try_wait().expect("final poll bashkit").is_none() {
        child.kill().expect("kill hung bashkit");
        drop(open_stdin);
        let _ = child.wait();
        panic!("bashkit did not apply --timeout to the pre-execution stdin read");
    }

    drop(open_stdin);
    let out = child.wait_with_output().expect("collect bashkit output");
    assert_ne!(code(&out), 0);
    assert_eq!(stdout(&out), "");
    assert!(
        stderr(&out).contains("stdin read timed out"),
        "stderr: {}",
        stderr(&out)
    );
}

#[test]
fn jq_recursive_filters_stop_without_host_abort_or_hang() {
    for filter in ["def f: f; f", "def f: (def g: f; g); f", "def f: 1 + f; f"] {
        let mut child = cli()
            .args([
                "--no-stdin",
                "--timeout",
                "1",
                "-c",
                &format!("jq -n '{filter}'"),
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn bashkit");
        let deadline = Instant::now() + Duration::from_secs(3);
        while child.try_wait().expect("poll bashkit").is_none() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(20));
        }
        if child.try_wait().expect("final poll bashkit").is_none() {
            child.kill().expect("kill hung bashkit");
            let _ = child.wait();
            panic!("jq filter hung: {filter}");
        }
        let out = child.wait_with_output().expect("collect bashkit output");
        assert_ne!(code(&out), 0, "filter: {filter}");
        assert!(
            stderr(&out).contains("jq: "),
            "filter: {filter}; stderr: {}",
            stderr(&out)
        );
    }
}

#[cfg(unix)]
#[test]
fn jq_non_tail_recursion_is_bounded_on_a_two_mib_stack() {
    let mut child = Command::new("sh")
        .args([
            "-c",
            "ulimit -s 2048; exec \"$1\" --no-stdin --timeout 1 -c \"$2\"",
            "sh",
            BIN,
            "jq -n 'def f: 1 + f; f'",
        ])
        .env_remove("RUST_BACKTRACE")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn bashkit with small stack");
    let deadline = Instant::now() + Duration::from_secs(3);
    while child.try_wait().expect("poll bashkit").is_none() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(20));
    }
    if child.try_wait().expect("final poll bashkit").is_none() {
        child.kill().expect("kill hung bashkit");
        let _ = child.wait();
        panic!("non-tail jq recursion hung on a 2 MiB stack");
    }
    let out = child.wait_with_output().expect("collect bashkit output");
    assert_eq!(out.status.code(), Some(5), "{}", stderr(&out));
    assert!(stderr(&out).contains("recursion limit (64) exceeded"));
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
fn stdout_is_capped_at_the_default_limit() {
    let chunk = "x".repeat(1_000);
    let script = format!("for i in $(seq 1 1100); do printf '{chunk}'; done");
    let out = run(&["-c", &script]);
    assert_eq!(code(&out), 0);
    assert_eq!(out.stdout.len(), 1_048_576);
}

#[test]
fn streamed_output_is_not_printed_twice() {
    let out = run(&["-c", "echo once; echo twice >&2"]);
    assert_eq!(stdout(&out), "once\n");
    assert_eq!(stderr(&out), "twice\n");
}
