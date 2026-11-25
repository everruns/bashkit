// THREAT[TM-INJ-027]
//
// Decision: the `failglob` "no match" error travels out of band. It used to
// ride the expanded argument vector as a `\0ERR\0`-prefixed first word, which
// made it forgeable: any script whose first expanded word began with those
// bytes was treated as a failed glob, so the data decided the exit code, the
// stream its own tail landed on, and whether the rest of the line ran.
//
// Found by `archive_fuzz` (nightly fuzz run 304) while reproducing a separate
// leak-detector false positive: the crash unit began `\0ERR\0` and the
// `gzip | gunzip` roundtrip handed it back on *stderr* with stdout empty.

use bashkit::Bash;

/// The sentinel bytes a script must not be able to forge.
const FORGED: &str = "\0ERR\0";

#[tokio::test]
async fn forged_sentinel_in_a_quoted_word_is_data_not_a_glob_error() {
    let mut bash = Bash::new();

    let result = bash.exec(&format!("echo '{FORGED}X'")).await.unwrap();

    assert_eq!(
        result.stdout, "\0ERR\0X\n",
        "the word is data and belongs on stdout"
    );
    assert_eq!(
        result.stderr, "",
        "nothing was wrong, so stderr stays empty"
    );
    assert_eq!(result.exit_code, 0);
}

#[tokio::test]
async fn forged_sentinel_does_not_abandon_the_rest_of_the_line() {
    // `failglob` aborts the remainder of the command line (bash DISCARD), so a
    // forged sentinel used to swallow every later command.
    let mut bash = Bash::new();

    let result = bash
        .exec(&format!("echo '{FORGED}X'; echo DONE"))
        .await
        .unwrap();

    assert_eq!(result.stdout, "\0ERR\0X\nDONE\n");
    assert_eq!(result.exit_code, 0);
}

#[tokio::test]
async fn forged_sentinel_after_the_command_name_is_data() {
    // The old check only looked at the first expanded word, so the position of
    // the forged bytes decided whether it fired.
    let mut bash = Bash::new();

    let result = bash.exec(&format!("echo a '{FORGED}X' b")).await.unwrap();

    assert_eq!(result.stdout, "a \0ERR\0X b\n");
    assert_eq!(result.exit_code, 0);
}

#[tokio::test]
async fn forged_sentinel_in_a_vanished_name_word_is_a_command_name() {
    // `$empty <word>` promotes the first argument field to the command name.
    // That path never checked the sentinel at all, so the forged bytes became
    // a command name; they must still be treated as one, not as a glob error.
    let mut bash = Bash::new();

    let result = bash
        .exec(&format!("EMPTY=; $EMPTY '{FORGED}X'"))
        .await
        .unwrap();

    assert_ne!(result.exit_code, 0, "no such command");
    assert!(
        result.stderr.contains("command not found"),
        "expected a command-not-found diagnostic, got: {:?}",
        result.stderr.to_string()
    );
}

#[tokio::test]
async fn real_failglob_still_reports_no_match_and_abandons_the_line() {
    let mut bash = Bash::new();

    let result = bash
        .exec("shopt -s failglob; echo *.nonexistent; echo NOTREACHED")
        .await
        .unwrap();

    assert_eq!(result.exit_code, 1);
    assert!(
        result.stderr.contains("no match: *.nonexistent"),
        "stderr: {:?}",
        result.stderr.to_string()
    );
    assert!(
        !result.stdout.contains("NOTREACHED"),
        "failglob abandons the rest of the line; stdout: {:?}",
        result.stdout.to_string()
    );
}

#[tokio::test]
async fn real_failglob_error_is_not_mistaken_for_data() {
    // The complement of the forgery test: the genuine error must not end up on
    // stdout as a word.
    let mut bash = Bash::new();

    let result = bash
        .exec("shopt -s failglob; echo *.nonexistent")
        .await
        .unwrap();

    assert_eq!(result.stdout, "", "the command never ran");
    assert!(
        !result.stderr.contains('\0'),
        "no sentinel bytes reach stderr"
    );
}
