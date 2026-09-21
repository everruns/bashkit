// Scaffold tests for the sed_fuzz target.
//
// Validates that the sed builtin survives arbitrary scripts and input without
// panicking AND without leaking Debug shapes, host paths, or the host-env
// canary into stderr/stdout.
//
// `fuzz_exec` also rejects stderr that reports a *caught* panic (`internal
// error:` / `builtin failed unexpectedly`). Before issue #2427 those passed
// silently, so `sed 's≠a≠X≠'` — a byte-index slice through a multi-byte
// delimiter, THREAT[TM-UNI-002] — looked clean to the fuzzer.

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

/// Every seed must leave stderr free of Debug shapes and caught panics.
async fn seed(name: &str, script: &str) {
    let mut bash = fuzz_bash();
    fuzz_exec(&mut bash, script, name, &[]).await;
}

#[tokio::test]
async fn sed_multibyte_delimiters() {
    // THREAT[TM-UNI-002]: the delimiter is read as a char but the body used to
    // be sliced at byte offset 2, panicking inside `≠`.
    for script in [
        r#"printf 'a\n' | sed 's≠a≠X≠'"#,
        r#"printf 'a\n' | sed 's≠a'"#,
        r#"printf 'a\n' | sed 'sé'"#,
        r#"printf 'a\n' | sed 'y≠a≠b≠'"#,
        r#"printf 'a\n' | sed '\≠a≠p'"#,
        r#"printf 'a\n' | sed 's😀a😀X😀g'"#,
    ] {
        seed("sed_multibyte_delimiters", script).await;
    }
}

#[tokio::test]
async fn sed_truncated_scripts() {
    for script in [
        r#"printf 'a\n' | sed 's/a'"#,
        r#"printf 'a\n' | sed 's/'"#,
        r#"printf 'a\n' | sed 's'"#,
        r#"printf 'a\n' | sed '/a'"#,
        r#"printf 'a\n' | sed '\'"#,
        r#"printf 'a\n' | sed 'y/ab/x/'"#,
        r#"printf 'a\n' | sed '1,'"#,
        r#"printf 'a\n' | sed '{'"#,
        r#"printf 'a\n' | sed '}'"#,
        r#"printf 'a\n' | sed 's/a/b/zz'"#,
        r#"printf 'a\n' | sed 's/[//'"#,
        r#"printf 'a\n' | sed 's/a/b/0'"#,
    ] {
        seed("sed_truncated_scripts", script).await;
    }
}

#[tokio::test]
async fn sed_pathological_regexes() {
    for script in [
        r#"printf 'aaaa\n' | sed 's/\(a*\)*b/X/'"#,
        r#"printf 'aaaa\n' | sed -E 's/(a+)+\1b/X/'"#,
        r#"printf 'a\n' | sed 's/[[:bogus:]]/X/'"#,
        r#"printf 'a\n' | sed 's/a\{1,999999\}/X/'"#,
        r#"printf 'a\n' | sed -n '/\(/p'"#,
        r#"printf 'a\n' | sed 's/*a/X/'"#,
    ] {
        seed("sed_pathological_regexes", script).await;
    }
}

#[tokio::test]
async fn sed_flow_control_seeds() {
    for script in [
        r#"printf 'a\n' | sed ':a;ba'"#,
        r#"printf 'a\n' | sed ':a;s/a/aa/;ta'"#,
        r#"printf 'a\n' | sed 'b missing'"#,
        r#"printf 'a\nb\n' | sed 'N;N;N;P;D'"#,
        r#"printf 'a\n' | sed -n 'l 1'"#,
        r#"printf 'a\n' | sed -n 'l 0'"#,
    ] {
        seed("sed_flow_control_seeds", script).await;
    }
}

#[tokio::test]
async fn sed_option_seeds() {
    for script in [
        "printf 'a\\n' | sed -ne p",
        "printf 'a\\n' | sed ''",
        "printf 'a\\n' | sed -e",
        "printf 'a\\n' | sed -f",
        "printf 'a\\n' | sed -i",
        "printf 'a\\n' | sed -l x -n l",
        "printf 'a\\n' | sed -- -e p",
        "printf 'a\\n' | sed --in-place",
        "printf 'a\\n' | sed --bogus p",
    ] {
        seed("sed_option_seeds", script).await;
    }
}
