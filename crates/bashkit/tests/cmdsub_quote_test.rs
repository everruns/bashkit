//! Test for issue #803: single-quoted strings inside $() lose double quotes

use bashkit::Bash;

#[tokio::test]
async fn cmdsub_preserves_double_quotes_in_single_quotes() {
    let mut bash = Bash::new();
    let result = bash
        .exec(r#"x="$(echo '{"a":1}')"; echo "${x}""#)
        .await
        .unwrap();
    assert_eq!(result.stdout.trim(), r#"{"a":1}"#);
}

#[tokio::test]
async fn cmdsub_preserves_double_quotes_simple() {
    let mut bash = Bash::new();
    let result = bash
        .exec(r#"y="$(echo 'say "hello" please')"; echo "${y}""#)
        .await
        .unwrap();
    assert_eq!(result.stdout.trim(), r#"say "hello" please"#);
}

/// Without outer double quotes, $() preserves double quotes correctly
#[tokio::test]
async fn cmdsub_without_outer_quotes_works() {
    let mut bash = Bash::new();
    let result = bash
        .exec(r#"x=$(echo '{"a":1}'); echo "$x""#)
        .await
        .unwrap();
    assert_eq!(result.stdout.trim(), r#"{"a":1}"#);
}
