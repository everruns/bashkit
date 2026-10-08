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
