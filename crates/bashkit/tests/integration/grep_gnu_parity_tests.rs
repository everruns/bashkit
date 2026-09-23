//! Regression: six ways `grep` diverged from GNU grep 3.11, all found by
//! pointing the differential gate at the grep spec suite for the first time.
//!
//! The spec suite's expectations had been written from Bashkit's own output,
//! so they recorded the bugs rather than catching them. `spec_tests::
//! grep_comparison_tests` now runs the same cases against the host tool, and
//! CI runs every gate rather than only the bash one.

use bashkit::Bash;

async fn run(script: &str) -> bashkit::ExecResult {
    let mut bash = Bash::builder()
        .mount_text("/m.txt", "foo\n")
        .mount_text("/n.txt", "bar\n")
        .build();
    bash.exec(script)
        .await
        .unwrap_or_else(|e| panic!("{script:?} failed: {e}"))
}

/// GNU names stdin `(standard input)` wherever it names it — `-H`, `-l`, `-L`
/// alike. Bashkit used `(stdin)` for the list flags only, so the same stream
/// had two names depending on the flag.
#[tokio::test]
async fn stdin_is_named_standard_input_under_every_flag() {
    assert_eq!(
        run("printf 'foo\\n' | grep -l foo")
            .await
            .stdout
            .to_string(),
        "(standard input)\n"
    );
    assert_eq!(
        run("printf 'bar\\n' | grep -L foo")
            .await
            .stdout
            .to_string(),
        "(standard input)\n"
    );
    assert_eq!(
        run("printf 'foo\\n' | grep -H foo")
            .await
            .stdout
            .to_string(),
        "(standard input):foo\n"
    );
}

/// `-z` means NUL-terminated *records*, on the way out as well as in.
#[tokio::test]
async fn null_data_terminates_output_records_with_nul() {
    let r = run("printf 'foo\\0bar\\0' | grep -z foo").await;
    assert_eq!(r.stdout.to_string(), "foo\0");
}

/// `-a` means "treat as text", not "delete the NUL bytes". Stripping them
/// silently altered the bytes the caller got back.
#[tokio::test]
async fn binary_as_text_preserves_nul_bytes() {
    let r = run("printf 'foo\\0bar\\n' | grep -a foo").await;
    assert_eq!(r.stdout.to_string(), "foo\0bar\n");
}

/// The binary-match notice is a diagnostic, so it belongs on stderr in GNU's
/// `grep: FILE: binary file matches` form. On stdout it became part of the
/// data a pipeline reads.
#[tokio::test]
async fn binary_match_notice_goes_to_stderr() {
    let r = run("printf 'foo\\0bar\\n' | grep foo").await;
    assert_eq!(r.stdout.to_string(), "");
    assert_eq!(
        r.stderr.to_string(),
        "grep: (standard input): binary file matches\n"
    );
    assert_eq!(r.exit_code, 0);
}

#[tokio::test]
async fn binary_match_notice_stays_out_of_a_pipeline() {
    let r = run("printf 'foo\\0bar\\n' | grep foo 2>/dev/null | wc -l").await;
    assert_eq!(r.stdout.to_string().trim(), "0");
}

/// The exit status reports whether a *match* was found, independent of what
/// `-L` chose to print. Bashkit had inverted it for `-L`.
#[tokio::test]
async fn exit_status_tracks_the_match_not_the_listing() {
    // -L: matching file prints nothing and exits 0.
    let matched = run("grep -L foo /m.txt").await;
    assert_eq!(matched.stdout.to_string(), "");
    assert_eq!(matched.exit_code, 0);

    // -L: non-matching file prints the name and exits 1.
    let unmatched = run("grep -L foo /n.txt").await;
    assert_eq!(unmatched.stdout.to_string(), "/n.txt\n");
    assert_eq!(unmatched.exit_code, 1);

    // Mixed: one file matched, so 0, while -L still lists the other.
    let mixed = run("grep -L foo /m.txt /n.txt").await;
    assert_eq!(mixed.stdout.to_string(), "/n.txt\n");
    assert_eq!(mixed.exit_code, 0);

    // Nothing matched anywhere: both listed, status 1.
    let none = run("grep -L zzz /m.txt /n.txt").await;
    assert_eq!(none.stdout.to_string(), "/m.txt\n/n.txt\n");
    assert_eq!(none.exit_code, 1);

    // -l is unchanged by all this.
    let listed = run("grep -l foo /m.txt /n.txt").await;
    assert_eq!(listed.stdout.to_string(), "/m.txt\n");
    assert_eq!(listed.exit_code, 0);
}
