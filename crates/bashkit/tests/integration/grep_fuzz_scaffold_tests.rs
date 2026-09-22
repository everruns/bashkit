// Scaffold tests for the grep_fuzz target.
//
// Validates that the grep builtin survives arbitrary POSIX patterns without
// panicking, without leaking Debug shapes or host paths, and — since
// `fuzz_exec` also rejects a *caught* panic — without the interpreter error
// path that used to render a full Rust backtrace for a pattern as ordinary as
// `grep '+'` (issue #2437).

use bashkit::testing::{fuzz_exec, fuzz_init};
use bashkit::{Bash, ExecutionLimits};

fn fuzz_bash() -> Bash {
    fuzz_init();
    Bash::builder()
        .limits(
            ExecutionLimits::new()
                .max_commands(50)
                .max_subst_depth(3)
                .max_stdout_bytes(4096)
                .max_stderr_bytes(4096)
                .timeout(std::time::Duration::from_secs(2)),
        )
        .build()
}

async fn seed(name: &str, script: &str) {
    let mut bash = fuzz_bash();
    fuzz_exec(&mut bash, script, name, &[]).await;
}

/// Quantifiers with nothing to repeat: GNU degrades these, and every one of
/// them used to reach the interpreter error path in Bashkit.
#[tokio::test]
async fn grep_dangling_quantifiers() {
    for p in [
        "+", "?", "*a", "^*a", "a**", "{}", "a{b", "\\{", "\\}", "{", "}",
    ] {
        for flags in ["", "-E"] {
            seed(
                "grep_dangling_quantifiers",
                &format!("printf 'a\\n' | grep {flags} -- '{p}'"),
            )
            .await;
        }
    }
}

#[tokio::test]
async fn grep_malformed_brackets_and_groups() {
    for p in [
        "[",
        "[^",
        "[]",
        "[a-",
        "[[:bogus:]]",
        "\\(",
        "\\)",
        "(",
        ")",
        "\\",
        "a\\",
    ] {
        for flags in ["", "-E", "-P", "-F"] {
            seed(
                "grep_malformed_brackets_and_groups",
                &format!("printf 'a\\n' | grep {flags} -- '{p}'"),
            )
            .await;
        }
    }
}

#[tokio::test]
async fn grep_pathological_patterns() {
    for script in [
        r#"printf 'aaaa\n' | grep '\(a*\)*b'"#,
        r#"printf 'aaaa\n' | grep -E '(a+)+b'"#,
        r#"printf 'a\n' | grep 'a\{1,999999\}'"#,
        r#"printf 'a\n' | grep -E 'a{1,999999}'"#,
    ] {
        seed("grep_pathological_patterns", script).await;
    }
}

#[tokio::test]
async fn grep_multibyte_patterns() {
    for script in [
        r#"printf '≠\n' | grep '≠'"#,
        r#"printf 'é\n' | grep '[é]'"#,
        r#"printf '😀\n' | grep -E '😀+'"#,
        r#"printf 'a\n' | grep '[≠-😀]'"#,
    ] {
        seed("grep_multibyte_patterns", script).await;
    }
}
