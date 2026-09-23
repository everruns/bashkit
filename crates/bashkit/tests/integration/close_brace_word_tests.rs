//! Regression: `}` is a reserved word, not a metacharacter, so it delimits a
//! word only when it stands alone. `echo a}b` prints `a}b`; the lexer used to
//! emit `RightBrace` for every `}` it met and split the word into `a } b`.
//!
//! bash's metacharacters are space, tab, newline, `|`, `&`, `;`, `(`, `)`,
//! `<` and `>`. Sibling of `for_in_reserved_word_tests`, which pins the same
//! distinction for `do`/`done`/`in`.
//!
//! Every expectation below was taken from GNU bash 5.2.21.

use bashkit::Bash;

async fn stdout_of(script: &str) -> String {
    let mut bash = Bash::new();
    bash.exec(script)
        .await
        .unwrap_or_else(|e| panic!("{script:?} failed to run: {e}"))
        .stdout
        .to_string()
}

#[tokio::test]
async fn close_brace_stays_inside_a_word() {
    assert_eq!(stdout_of("echo a}b").await, "a}b\n");
    assert_eq!(stdout_of("echo a}").await, "a}\n");
    assert_eq!(stdout_of("echo }b").await, "}b\n");
    assert_eq!(stdout_of("echo a}b}c").await, "a}b}c\n");
    assert_eq!(stdout_of("echo }}").await, "}}\n");
    assert_eq!(stdout_of("echo a}{b").await, "a}{b\n");
}

#[tokio::test]
async fn close_brace_does_not_split_fields() {
    assert_eq!(stdout_of("echo 1}2 3}4").await, "1}2 3}4\n");
}

/// A lone `}` is still a word of its own, and still prints as one.
#[tokio::test]
async fn lone_close_brace_is_still_a_word() {
    assert_eq!(stdout_of("echo }").await, "}\n");
}

/// The three shapes that did not just print the wrong thing but failed to
/// parse at all, because the split `}` landed where a terminator was expected.
#[tokio::test]
async fn close_brace_in_assignment_value() {
    assert_eq!(stdout_of("v=a}b; echo $v").await, "a}b\n");
}

#[tokio::test]
async fn close_brace_in_for_list() {
    assert_eq!(stdout_of("for i in a}b; do echo $i; done").await, "a}b\n");
}

#[tokio::test]
async fn close_brace_in_case_pattern() {
    assert_eq!(stdout_of("case x} in x}) echo m;; esac").await, "m\n");
    assert_eq!(stdout_of("case a}b in a}b) echo M;; esac").await, "M\n");
}

/// The reserved-word reading must survive: `}` closes a brace group when it
/// stands alone, which is how `{ cmd; }` and a function body are written.
#[tokio::test]
async fn brace_group_and_function_body_still_close() {
    assert_eq!(stdout_of("{ echo grp; }").await, "grp\n");
    assert_eq!(stdout_of("{ echo one; echo two; }").await, "one\ntwo\n");
    assert_eq!(stdout_of("f() { echo fn; }; f").await, "fn\n");
    assert_eq!(stdout_of("f(){ echo z; }; f").await, "z\n");
    assert_eq!(stdout_of("if true; then { echo n; }; fi").await, "n\n");
}

/// Parameter and brace expansion are untouched — their `}` is consumed by the
/// `${`/`{` readers before the word reader ever sees it.
#[tokio::test]
async fn expansions_are_unaffected() {
    assert_eq!(stdout_of("x=1; echo ${x}").await, "1\n");
    assert_eq!(stdout_of("echo ${x}y").await, "y\n");
    assert_eq!(stdout_of("a=(1 2); echo ${a[@]}").await, "1 2\n");
    assert_eq!(stdout_of("echo {1..3}").await, "1 2 3\n");
    assert_eq!(stdout_of("echo {a,b}").await, "a b\n");
    assert_eq!(stdout_of("echo x{a,b}y").await, "xay xby\n");
    assert_eq!(stdout_of("echo a{b}c").await, "a{b}c\n");
}

/// Quoting and escaping already produced the right answer; they must keep it.
#[tokio::test]
async fn quoted_and_escaped_close_brace_unchanged() {
    assert_eq!(stdout_of("echo \"a}b\"").await, "a}b\n");
    assert_eq!(stdout_of("echo 'a}b'").await, "a}b\n");
    assert_eq!(stdout_of("echo a\\}b").await, "a}b\n");
}

/// A metacharacter after `}` still ends the word, so redirection and
/// separators keep working on a word that contains one.
#[tokio::test]
async fn metacharacter_after_close_brace_still_terminates() {
    assert_eq!(stdout_of("echo a}b >/dev/null; echo ok").await, "ok\n");
}
