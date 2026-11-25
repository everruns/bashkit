// Scaffold tests for the tomlq_fuzz target.
// Seeds the inputs that turned nightly `Fuzz Testing` red, so a regression
// shows up in `cargo test` rather than a day later in the fuzz matrix.
// Matches the TM-UNI-002 convention used by `awk_fuzz_scaffold_tests`.

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

/// THREAT[TM-UNI-001]: `strip_inline_comment` sliced the value string with a
/// *char* index, so any multi-byte character before the `#` landed inside that
/// character and panicked. The interpreter caught it, which surfaced as
/// `tomlq: builtin failed unexpectedly` — itself a banned shape, which is how
/// `tomlq_fuzz` reported it in both plan modes.
#[tokio::test]
async fn tomlq_multibyte_value_before_an_inline_comment() {
    let mut bash = fuzz_bash();
    // U+F8FF is three bytes; the fuzzer reduced to a run of them before a `#`.
    for doc in [
        "k='\u{f8ff}\u{f8ff}\u{f8ff}' #c",
        "k=\u{f8ff}#c",
        "k='\u{f8ff}' # \u{f8ff}",
        "[s]\nk=\"\u{1f600}\u{1f600}\" #tail",
    ] {
        let script = format!(
            "printf '%s\\n' '{}' | tomlq 'k'",
            doc.replace('\'', "'\\''")
        );
        fuzz_exec(&mut bash, &script, "tomlq_fuzz", &[]).await;
    }
}

/// A `#` inside a quoted value is part of the value, multi-byte or not.
#[tokio::test]
async fn tomlq_hash_inside_a_multibyte_quoted_value_is_not_a_comment() {
    let mut bash = fuzz_bash();
    let script = "printf '%s\\n' 'k = \"\u{f8ff}#kept\"' | tomlq 'k'";
    let result = bash.exec(script).await.expect("tomlq runs");
    assert_eq!(result.exit_code, 0, "stderr: {}", result.stderr);
    assert!(
        result.stdout.contains("#kept"),
        "a quoted `#` stays in the value; stdout: {:?}",
        result.stdout.to_string()
    );
}
