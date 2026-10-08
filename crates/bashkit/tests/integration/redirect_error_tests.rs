//! Wording and status of a redirection that cannot be opened.
//!
//! Bash prefixes the diagnostic with the line the redirect is on and uses the
//! capitalized strerror text (`Is a directory`). A target written with a
//! trailing slash names a directory, so no file is created for it.

use bashkit::Bash;

async fn run(script: &str) -> bashkit::ExecResult {
    Bash::builder().build().exec(script).await.unwrap()
}

#[tokio::test]
async fn noclobber_names_the_line() {
    let r = run("echo a > f.txt\nset -o noclobber\necho b > f.txt").await;
    assert_eq!(
        r.stderr.to_string(),
        "bash: line 3: f.txt: cannot overwrite existing file\n"
    );
    assert_eq!(r.exit_code, 1);
}

#[tokio::test]
async fn redirecting_onto_a_directory_fails() {
    let r = run("mkdir d\necho x > d").await;
    assert_eq!(r.stderr.to_string(), "bash: line 2: d: Is a directory\n");
    assert_eq!(r.exit_code, 1);
}

#[tokio::test]
async fn a_trailing_slash_names_a_directory_and_creates_nothing() {
    let r = run("echo y > nodir/\necho \"rc=$?\"\nls").await;
    assert_eq!(
        r.stderr.to_string(),
        "bash: line 1: nodir/: Is a directory\n"
    );
    assert_eq!(r.stdout.to_string(), "rc=1\n");
}

#[tokio::test]
async fn append_rejects_a_trailing_slash_too() {
    let r = run("echo z >> nodir/").await;
    assert_eq!(
        r.stderr.to_string(),
        "bash: line 1: nodir/: Is a directory\n"
    );
    assert_eq!(r.exit_code, 1);
}

#[tokio::test]
async fn a_missing_parent_directory_names_the_line() {
    let r = run("echo w > /nope/deep/f").await;
    assert_eq!(
        r.stderr.to_string(),
        "bash: line 1: /nope/deep/f: No such file or directory\n"
    );
    assert_eq!(r.exit_code, 1);
}
