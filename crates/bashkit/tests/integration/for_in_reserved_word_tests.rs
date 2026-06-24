//! Regression: reserved words (`do`, `done`, `in`, `then`, ...) are ordinary
//! words inside a `for`/`select` `in` list until a list terminator (`;` or
//! newline). They only become keywords in command position.
//!
//! Found by the nightly differential proptest:
//! `for a in do; do echo $a; done` must print `do`, not nothing.

use bashkit::Bash;

#[tokio::test]
async fn for_in_do_word_iterates() {
    let mut bash = Bash::new();
    let result = bash.exec("for a in do; do echo $a; done").await.unwrap();
    assert_eq!(result.stdout, "do\n");
}

#[tokio::test]
async fn for_in_multiple_reserved_words() {
    let mut bash = Bash::new();
    let result = bash
        .exec("for a in do done then in; do echo $a; done")
        .await
        .unwrap();
    assert_eq!(result.stdout, "do\ndone\nthen\nin\n");
}

#[tokio::test]
async fn for_in_reserved_word_newline_terminator() {
    let mut bash = Bash::new();
    let result = bash.exec("for a in done\ndo echo $a; done").await.unwrap();
    assert_eq!(result.stdout, "done\n");
}

#[tokio::test]
async fn for_in_normal_words_still_work() {
    let mut bash = Bash::new();
    let result = bash.exec("for a in 1 2 3; do echo $a; done").await.unwrap();
    assert_eq!(result.stdout, "1\n2\n3\n");
}
