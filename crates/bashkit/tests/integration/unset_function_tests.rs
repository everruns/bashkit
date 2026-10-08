//! Tests for unset -f (issue #673)

use bashkit::Bash;

async fn run(script: &str) -> bashkit::ExecResult {
    let mut bash = Bash::new();
    bash.exec(script).await.unwrap()
}

#[tokio::test]
async fn unset_f_removes_function() {
    let result = run(r#"
f() { echo hi; }
f
unset -f f
f 2>/dev/null; echo $?
"#)
    .await;
    assert_eq!(result.stdout, "hi\n127\n");
}

#[tokio::test]
async fn unset_f_nonexistent_function_is_noop() {
    let result = run("unset -f nonexistent; echo $?").await;
    assert_eq!(result.stdout, "0\n");
}

#[tokio::test]
async fn unset_f_does_not_affect_variables() {
    let result = run(r#"
x=hello
unset -f x
echo $x
"#)
    .await;
    assert_eq!(result.stdout, "hello\n");
}

#[tokio::test]
async fn unset_without_f_keeps_function_when_variable_exists() {
    let result = run(r#"
f=1
f() { echo hi; }
unset f
f
"#)
    .await;
    assert_eq!(result.stdout, "hi\n");
}

#[tokio::test]
async fn unset_without_f_falls_back_to_function() {
    // bash: with no variable of that name, `unset f` unsets the function.
    let result = run(r#"
f() { echo hi; }
unset f
f
echo status=$?
"#)
    .await;
    assert_eq!(result.stdout, "status=127\n");
}
