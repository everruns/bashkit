// Scaffold tests for the glob_fuzz target.
//
// Replays crash inputs found by `cargo +nightly fuzz run glob_fuzz` so the
// regression is covered by the normal `cargo test` run instead of only by
// the nightly fuzz workflow.
//
// Design note: `glob_fuzz` inlines its raw input into shell scripts and then
// asserts the cross-tool leak invariants from `bashkit::testing`. The target
// pre-filters inputs that literally contain a banned substring, but the shell
// drops NUL bytes during word expansion, so an input like `</r\0ustc/` becomes
// the path `/rustc/` *after* the filter has run. Any diagnostic that echoes
// such a path back must therefore use a real-shell error template that
// `bashkit::testing`'s echo filter recognizes.

use bashkit::testing::{fuzz_exec, fuzz_init};
use bashkit::{Bash, ExecutionLimits};

fn fuzz_bash() -> Bash {
    fuzz_init();
    Bash::builder()
        .limits(
            ExecutionLimits::new()
                .max_commands(50)
                .timeout(std::time::Duration::from_millis(500)),
        )
        .mount_text("/tmp/a.txt", "")
        .mount_text("/tmp/b.sh", "")
        .mount_text("/tmp/sub/d.txt", "")
        .build()
}

/// Regression: fuzz run 218 (`crash-fae53719665e9c77d2e1a1b7d4da7e43902525d0`,
/// bytes `[60, 47, 114, 0, 117, 115, 116, 99, 47]` = `</r\0ustc/`).
///
/// `ls /tmp/</r\0ustc/` is an input redirection from a path that does not
/// exist. The NUL is dropped during expansion, so the redirection target is
/// `/rustc/` — a `UNIVERSAL_BANNED` host-path shape. The diagnostic must use
/// bash's real template so the echo filter recognizes it as a shell echo of
/// user input rather than a TM-INF-016 host-path leak.
#[tokio::test]
async fn glob_fuzz_crash_nul_stripped_redirect_target() {
    // Replays all three scripts the glob_fuzz target builds from one input,
    // in the same order, so the scaffold covers the whole crashing iteration.
    let input = "</r\0ustc/";
    let scripts = [
        format!("ls /tmp/{}", input),
        format!(
            "case \"test.txt\" in {}) echo match;; *) echo no;; esac",
            input
        ),
        format!("if [[ \"hello.world\" == {} ]]; then echo y; fi", input),
    ];
    let mut bash = fuzz_bash();
    for script in &scripts {
        fuzz_exec(
            &mut bash,
            script,
            "glob_fuzz_crash_nul_stripped_redirect_target",
            &[],
        )
        .await;
    }
}

/// Regression: fuzz run 219 (`crash-2ac9af1dcb0bb66e7849b347828a8236e12fdda3`,
/// bytes `[10, 32, 0, 123, 32, 99, 111, 100, 101, 58, 60, 40, 93, 41, 10]`
/// = `\n \0{ code:<(])\n`).
///
/// Same NUL-stripping mechanism as run 218 — the raw bytes hide the banned
/// ` { code:` behind a NUL — but a second gap too: the resulting command name
/// ends in a newline, so bash's one-line `command not found` template renders
/// across two lines and the echo filter must match it as one span.
#[tokio::test]
async fn glob_fuzz_crash_nul_stripped_newline_command_name() {
    let input = "\n \0{ code:<(])\n";
    // The target now skips this input outright, since the shell would echo a
    // banned shape back. Assert that, then assert the harness would survive it
    // anyway — the pre-filter and the echo filter are independent defenses.
    assert!(bashkit::testing::input_echo_would_trip(input));

    let mut bash = fuzz_bash();
    for script in [
        format!("ls /tmp/{}", input),
        format!(
            "case \"test.txt\" in {}) echo match;; *) echo no;; esac",
            input
        ),
        format!("if [[ \"hello.world\" == {} ]]; then echo y; fi", input),
    ] {
        fuzz_exec(
            &mut bash,
            &script,
            "glob_fuzz_crash_nul_stripped_newline_command_name",
            &[],
        )
        .await;
    }
}

/// The missing-input-redirect diagnostic must match real bash byte for byte:
/// `bash: <path>: No such file or directory`.
#[tokio::test]
async fn missing_input_redirect_matches_bash_wording() {
    let mut bash = fuzz_bash();
    let result = bash.exec("ls /tmp/ < /nope/missing").await.unwrap();
    assert_eq!(
        result.stderr.to_string(),
        "bash: /nope/missing: No such file or directory\n"
    );
    assert_eq!(result.exit_code, 1);
}

/// Same template for an output redirect whose parent directory is missing.
/// The VFS's `parent directory not found` only restates `NotFound`, so it is
/// replaced by bash's wording rather than surfaced.
#[tokio::test]
async fn missing_output_redirect_dir_matches_bash_wording() {
    let mut bash = fuzz_bash();
    let result = bash.exec("echo hi > /nope/missing/out.txt").await.unwrap();
    assert_eq!(
        result.stderr.to_string(),
        "bash: /nope/missing/out.txt: No such file or directory\n"
    );
    assert_eq!(result.exit_code, 1);
}

/// A backend reason that says more than its errno survives: collapsing it to
/// the bare `Permission denied` would drop the only actionable detail. This
/// is what a custom [`bashkit::FileSystem`] impl's own wording relies on.
#[tokio::test]
async fn read_only_mount_keeps_its_specific_reason() {
    let mut bash = Bash::builder()
        .fs(std::sync::Arc::new(bashkit::ReadOnlyFs::new(
            std::sync::Arc::new(bashkit::InMemoryFs::new()),
        )))
        .build();
    let result = bash.exec("echo hi > /tmp/nope.txt").await.unwrap();
    assert_eq!(
        result.stderr.to_string(),
        "bash: /tmp/nope.txt: filesystem is read-only\n"
    );
    assert!(!result.stderr.to_string().contains("io error:"));
}

/// A NUL byte inside a word is dropped, not preserved, so the redirect target
/// collapses to the same path with or without it.
#[tokio::test]
async fn nul_byte_in_word_is_dropped() {
    let mut bash = fuzz_bash();
    let with_nul = bash.exec("ls /tmp/ < /no\0pe").await.unwrap();
    let without_nul = bash.exec("ls /tmp/ < /nope").await.unwrap();
    assert_eq!(with_nul.stderr.to_string(), without_nul.stderr.to_string());
}
