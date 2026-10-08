//! Builtin arguments bash refuses, and what it says about them.
//!
//! `return` outside a function or a sourced script, a name `unset -v` could
//! never have assigned, and a non-numeric `read -t`/`-n` are all usage errors
//! in bash. Each reports and fails; the script keeps running.

use bashkit::Bash;

async fn run(script: &str) -> bashkit::ExecResult {
    Bash::builder().build().exec(script).await.unwrap()
}

#[tokio::test]
async fn return_outside_a_function_reports_and_keeps_going() {
    let r = run("return\necho \"rc=$?\"\necho after").await;
    assert_eq!(
        r.stderr.to_string(),
        "bash: line 1: return: can only `return' from a function or sourced script\n"
    );
    assert_eq!(r.stdout.to_string(), "rc=2\nafter\n");
}

#[tokio::test]
async fn return_inside_a_function_still_works() {
    let r = run("f() { return 3; }\nf\necho \"rc=$?\"").await;
    assert_eq!(r.stderr.to_string(), "");
    assert_eq!(r.stdout.to_string(), "rc=3\n");
}

#[tokio::test]
async fn return_in_a_sourced_script_is_its_status() {
    let r = run("printf 'echo in\\nreturn 4\\necho no\\n' > s.sh\n. ./s.sh\necho \"rc=$?\"").await;
    assert_eq!(r.stderr.to_string(), "");
    assert_eq!(r.stdout.to_string(), "in\nrc=4\n");
}

#[tokio::test]
async fn unset_v_insists_on_an_identifier() {
    for name in ["arr[", "a-b", "1bad"] {
        let r = run(&format!("unset -v '{name}'\necho \"rc=$?\"")).await;
        assert_eq!(
            r.stderr.to_string(),
            format!("bash: line 1: unset: `{name}': not a valid identifier\n")
        );
        assert_eq!(r.stdout.to_string(), "rc=1\n");
    }
}

/// Without `-v`, and under `-f`, bash takes any word and skips what it
/// cannot find.
#[tokio::test]
async fn unset_without_v_accepts_any_word() {
    let r = run("unset 'arr['\nunset -f 'f['\necho \"rc=$?\"").await;
    assert_eq!(r.stderr.to_string(), "");
    assert_eq!(r.stdout.to_string(), "rc=0\n");
}

#[tokio::test]
async fn unset_v_still_takes_a_subscript() {
    let r = run("a=(x y)\nunset -v 'a[0]'\necho \"${a[@]}\"").await;
    assert_eq!(r.stderr.to_string(), "");
    assert_eq!(r.stdout.to_string(), "y\n");
}

#[tokio::test]
async fn read_rejects_a_non_numeric_timeout() {
    let r = run("read -t abc x <<< ''\necho \"rc=$?\"").await;
    assert_eq!(
        r.stderr.to_string(),
        "bash: line 1: read: abc: invalid timeout specification\n"
    );
    assert_eq!(r.stdout.to_string(), "rc=1\n");
}

#[tokio::test]
async fn read_rejects_a_non_numeric_count() {
    let r = run("read -n abc x <<< ''\necho \"rc=$?\"").await;
    assert_eq!(
        r.stderr.to_string(),
        "bash: line 1: read: abc: invalid number\n"
    );
    assert_eq!(r.stdout.to_string(), "rc=1\n");
}

#[tokio::test]
async fn read_still_takes_a_numeric_count() {
    let r = run("read -n 3 x <<< 'hello'\necho \"$x\"").await;
    assert_eq!(r.stderr.to_string(), "");
    assert_eq!(r.stdout.to_string(), "hel\n");
}
