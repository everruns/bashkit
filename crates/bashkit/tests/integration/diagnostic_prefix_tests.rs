//! Shell diagnostics carry bash's non-interactive `$0: line N: ` prefix.
//!
//! Every expected string below was checked against real bash 5.2 running the
//! same text as `bash SCRIPT` (so `$0` is `SCRIPT`) or `bash -c '...'` (so
//! `$0` is `bash`). Coreutils-style builtins are external programs in bash and
//! keep their own `cat: ...` form; usage lines stay unprefixed.

use bashkit::{Bash, ExecOptions};
use std::sync::{Arc, Mutex};

/// Run `script` the way `bash SCRIPT` would: `$0` is `SCRIPT`.
async fn run_script(script: &str) -> (String, i32) {
    let mut bash = Bash::new();
    let r = bash
        .exec_with_options(script, ExecOptions::new().arg0("SCRIPT"))
        .await
        .unwrap();
    (r.stderr.to_string(), r.exit_code)
}

/// Run `script` the way `bash -c` would: `$0` is the default `bash`.
async fn run_c(script: &str) -> (String, i32) {
    let mut bash = Bash::new();
    let r = bash.exec(script).await.unwrap();
    (r.stderr.to_string(), r.exit_code)
}

#[tokio::test]
async fn command_not_found_in_script_file() {
    let (err, code) = run_script("echo a >/dev/null\ncomando\n").await;
    assert_eq!(err, "SCRIPT: line 2: comando: command not found\n");
    assert_eq!(code, 127);
}

#[tokio::test]
async fn command_not_found_in_dash_c() {
    let (err, code) = run_c("comando").await;
    assert_eq!(err, "bash: line 1: comando: command not found\n");
    assert_eq!(code, 127);
}

#[tokio::test]
async fn missing_absolute_path_command() {
    let (err, code) = run_script("\n/abs/missing\n").await;
    assert_eq!(
        err,
        "SCRIPT: line 2: /abs/missing: No such file or directory\n"
    );
    assert_eq!(code, 127);
}

#[tokio::test]
async fn input_redirect_failure() {
    let (err, code) = run_script("cat < dados.txt\n").await;
    assert_eq!(
        err,
        "SCRIPT: line 1: dados.txt: No such file or directory\n"
    );
    assert_eq!(code, 1);
}

#[tokio::test]
async fn mapfile_redirect_failure() {
    let (err, _) = run_script("mapfile -t a < dados.txt\n").await;
    assert_eq!(
        err,
        "SCRIPT: line 1: dados.txt: No such file or directory\n"
    );
}

#[tokio::test]
async fn readonly_assignment() {
    let (err, code) = run_script("readonly ro=1\nro=2\n").await;
    assert_eq!(err, "SCRIPT: line 2: ro: readonly variable\n");
    assert_eq!(code, 1);
}

#[tokio::test]
async fn declare_p_missing() {
    let (err, code) = run_script("declare -p naoexiste\n").await;
    assert_eq!(err, "SCRIPT: line 1: declare: naoexiste: not found\n");
    assert_eq!(code, 1);
}

#[tokio::test]
async fn source_missing_file_has_prefix_and_newline() {
    let (err, code) = run_script("echo 1 >/dev/null\n. ./lib.sh\necho after >&2\n").await;
    assert_eq!(
        err,
        "SCRIPT: line 2: ./lib.sh: No such file or directory\nafter\n"
    );
    assert_eq!(code, 0);
    let (err, _) = run_script("source ./lib.sh\n").await;
    assert_eq!(err, "SCRIPT: line 1: ./lib.sh: No such file or directory\n");
}

#[tokio::test]
async fn source_without_argument() {
    let (err, code) = run_script("source\n").await;
    assert_eq!(
        err,
        "SCRIPT: line 1: source: filename argument required\n\
         source: usage: source filename [arguments]\n"
    );
    assert_eq!(code, 2);
}

#[tokio::test]
async fn nounset_error() {
    let (err, code) = run_script("set -u\necho $naodefinida\n").await;
    assert_eq!(err, "SCRIPT: line 2: naodefinida: unbound variable\n");
    assert_eq!(code, 1);
}

#[tokio::test]
async fn param_error_if_unset_message() {
    let (err, code) = run_script("\necho ${obrigatorio:?precisa definir}\n").await;
    assert_eq!(err, "SCRIPT: line 2: obrigatorio: precisa definir\n");
    assert_eq!(code, 1);
}

#[tokio::test]
async fn error_inside_function_reports_line_of_failing_command() {
    let (err, code) = run_script("f() {\n  echo in >/dev/null\n  nocmd\n}\nf\n").await;
    assert_eq!(err, "SCRIPT: line 3: nocmd: command not found\n");
    assert_eq!(code, 127);
}

#[tokio::test]
async fn eval_syntax_error_uses_dollar_zero() {
    let (err, code) = run_script("\neval 'echo \"x'\n").await;
    assert!(err.starts_with("SCRIPT: eval: line 2: "), "{err}");
    for line in err.lines() {
        assert!(line.starts_with("SCRIPT: eval: line 2: "), "{err}");
    }
    assert_eq!(code, 2);
}

#[tokio::test]
async fn type_not_found_in_dash_c() {
    let (err, code) = run_c("type h").await;
    assert_eq!(err, "bash: line 1: type: h: not found\n");
    assert_eq!(code, 1);
}

#[tokio::test]
async fn builtin_usage_line_stays_unprefixed() {
    let (err, code) = run_c("umask -x").await;
    assert_eq!(
        err,
        "bash: line 1: umask: -x: invalid option\numask: usage: umask [-p] [-S] [mode]\n"
    );
    assert_eq!(code, 2);
}

#[tokio::test]
async fn shell_builtin_without_bash_tag_gets_prefix() {
    let (err, code) = run_script("\ncd /nonexist\n").await;
    assert_eq!(
        err,
        "SCRIPT: line 2: cd: /nonexist: No such file or directory\n"
    );
    assert_eq!(code, 1);
}

#[tokio::test]
async fn arithmetic_error_uses_dollar_zero() {
    let (err, code) = run_script("echo $((1/0))\n").await;
    assert_eq!(
        err,
        "SCRIPT: line 1: 1/0: division by 0 (error token is \"0\")\n"
    );
    assert_eq!(code, 1);
}

#[tokio::test]
async fn coreutils_builtin_stays_unprefixed() {
    let (err, code) = run_script("cat missing\n").await;
    assert_eq!(err, "cat: missing: No such file or directory\n");
    assert_eq!(code, 1);
}

#[tokio::test]
async fn child_shell_reports_its_own_name_and_line() {
    let (err, code) = run_script("\nbash -c 'nocmd'\n").await;
    assert_eq!(err, "bash: line 1: nocmd: command not found\n");
    assert_eq!(code, 127);
}

#[tokio::test]
async fn sourced_file_reports_its_own_path_and_line() {
    let mut bash = Bash::new();
    bash.exec("printf 'echo in >/dev/null\\nnocmd\\n' > /tmp/lib.sh")
        .await
        .unwrap();
    let r = bash
        .exec_with_options(
            "\n\nsource /tmp/lib.sh\n",
            ExecOptions::new().arg0("SCRIPT"),
        )
        .await
        .unwrap();
    assert_eq!(r.stderr, "/tmp/lib.sh: line 2: nocmd: command not found\n");
}

#[tokio::test]
async fn streaming_callback_sees_prefixed_text() {
    let seen: Arc<Mutex<String>> = Arc::new(Mutex::new(String::new()));
    let seen_cb = seen.clone();
    let mut bash = Bash::new();
    bash.exec_with_options(
        "echo a\nnocmd\n",
        ExecOptions::new()
            .arg0("SCRIPT")
            .streaming(Box::new(move |_stdout, stderr| {
                seen_cb.lock().unwrap().push_str(stderr);
            })),
    )
    .await
    .unwrap();
    assert_eq!(
        *seen.lock().unwrap(),
        "SCRIPT: line 2: nocmd: command not found\n"
    );
}

#[tokio::test]
async fn interactive_mode_keeps_bare_prefix() {
    let mut bash = Bash::new();
    bash.set_interactive(true);
    let r = bash.exec("nocmd").await.unwrap();
    assert_eq!(r.stderr, "bash: nocmd: command not found\n");
}

/// Small-budget reproduction: the original stderr fits; repeating $0 does not.
#[tokio::test]
async fn diagnostic_prefix_amplification_hits_shared_memory_budget() {
    for (redirect, streaming) in [
        ("", false),
        ("", true),
        (" 2>/tmp/diagnostics", true),
        (" 2>&1", true),
        (" 2>/dev/null", true),
    ] {
        let mut bash = Bash::builder()
            .limits(
                bashkit::ExecutionLimits::new()
                    .timeout(std::time::Duration::from_secs(600))
                    .max_live_intermediate_bytes(8192)
                    .max_stderr_bytes(64),
            )
            .build();
        let seen = Arc::new(Mutex::new(String::new()));
        let seen_cb = seen.clone();
        let script = format!(
            "bash -c 'unalias {{1..100}}' '{}'{}",
            "x".repeat(128),
            redirect
        );
        let options = if streaming {
            ExecOptions::new().streaming(Box::new(move |stdout, stderr| {
                let mut seen = seen_cb.lock().unwrap();
                seen.push_str(stdout);
                seen.push_str(stderr);
            }))
        } else {
            ExecOptions::new()
        };
        let error = bash
            .exec_with_options(&script, options)
            .await
            .expect_err("prefix amplification must fail before routing or capture");
        assert!(
            error.to_string().contains("live intermediate"),
            "{redirect}: {error}"
        );
        assert!(seen.lock().unwrap().is_empty());
        let recovered = bash.exec("echo recovered").await.unwrap();
        assert_eq!(recovered.stdout, "recovered\n");
    }
}

#[tokio::test]
async fn diagnostic_name_is_bounded_utf8_without_changing_arg0() {
    let name = "€".repeat(400);
    let mut bash = Bash::new();
    let script = format!("bash -c 'echo ${{#0}}; unalias absent' '{name}'");
    let result = bash.exec(&script).await.unwrap();
    assert_eq!(result.stdout, "400\n");
    assert_eq!(
        result.stderr,
        format!("{}: line 1: unalias: absent: not found\n", "€".repeat(341))
    );
}

#[tokio::test]
async fn diagnostic_prefix_matches_bash_and_redirect_keeps_full_output() {
    let name = "child".repeat(24);
    let body = "unalias first second third";
    let oracle = std::process::Command::new("bash")
        .env_clear()
        .args(["-c", body, &name])
        .output()
        .unwrap();
    let script = format!("bash -c '{body}' '{name}'");
    let mut bash = Bash::builder()
        .limits(bashkit::ExecutionLimits::new().max_live_intermediate_bytes(8192))
        .build();
    let result = bash.exec(&script).await.unwrap();
    assert_eq!(result.stderr.as_bytes(), oracle.stderr);
    assert_eq!(result.stdout.as_bytes(), oracle.stdout);
    assert_eq!(Some(result.exit_code), oracle.status.code());

    let mut bash = Bash::builder()
        .limits(
            bashkit::ExecutionLimits::new()
                .max_live_intermediate_bytes(8192)
                .max_stderr_bytes(64),
        )
        .build();
    let result = bash
        .exec(&format!(
            "{script} 2>/tmp/diagnostics; cat /tmp/diagnostics"
        ))
        .await
        .unwrap();
    assert!(result.stderr.is_empty());
    assert_eq!(result.stdout.as_bytes(), oracle.stderr);
}
