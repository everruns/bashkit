//! Tests for `set -e` (errexit) with AND-OR lists.
//!
//! Per POSIX, `set -e` should NOT cause an exit when a command fails
//! as part of an AND-OR chain (`cmd1 && cmd2`, `cmd1 || cmd2`).

use bashkit::Bash;

/// set -e: [[ false ]] && cmd in function should not exit
#[tokio::test]
async fn set_e_and_list_in_function() {
    let mut bash = Bash::new();
    let result = bash
        .exec(
            r#"
set -e
f() {
    [[ "a" == "b" ]] && return 0
    echo "should reach here"
}
f
echo "after f"
"#,
        )
        .await
        .unwrap();
    assert!(result.stdout.contains("should reach here"));
    assert!(result.stdout.contains("after f"));
}

/// set -e: [[ false ]] && cmd inside brace group with redirect
#[tokio::test]
async fn set_e_and_list_in_brace_group() {
    let mut bash = Bash::new();
    let result = bash
        .exec(
            r#"
set -e
x=""
{
    echo "line1"
    [[ -n "$x" ]] && echo "has value"
    echo "line2"
} > /tmp/out.txt
cat /tmp/out.txt
"#,
        )
        .await
        .unwrap();
    assert!(result.stdout.contains("line1"));
    assert!(result.stdout.contains("line2"));
}

/// set -e: [[ false ]] && cmd inside for loop
#[tokio::test]
async fn set_e_and_list_in_for_loop() {
    let mut bash = Bash::new();
    let result = bash
        .exec(
            r#"
set -e
f() {
    for x in a b c; do
        [[ "$x" == "b" ]] && return 0
    done
}
f
echo "after f"
"#,
        )
        .await
        .unwrap();
    assert!(result.stdout.contains("after f"));
}

/// set -e: top level [[ false ]] && cmd should still work
#[tokio::test]
async fn set_e_and_list_top_level() {
    let mut bash = Bash::new();
    let result = bash
        .exec(
            r#"
set -e
[[ "a" == "b" ]] && echo "match"
echo "reached"
"#,
        )
        .await
        .unwrap();
    assert!(result.stdout.contains("reached"));
}

/// set -e: [[ false ]] && cmd inside nested C-style for loops (issue #867)
#[tokio::test]
async fn set_e_and_list_in_nested_cfor() {
    let mut bash = Bash::new();
    let result = bash
        .exec(
            r#"
set -e
f() {
    for ((i=0; i<2; i++)); do
        for ((j=0; j<2; j++)); do
            [[ $i -ne $j ]] && echo "$i != $j"
        done
    done
    echo "done"
}
f
echo "after f"
"#,
        )
        .await
        .unwrap();
    assert!(result.stdout.contains("0 != 1"), "should print 0 != 1");
    assert!(result.stdout.contains("1 != 0"), "should print 1 != 0");
    assert!(result.stdout.contains("done"), "should reach done");
    assert!(result.stdout.contains("after f"), "should reach after f");
}

/// set -e: nested for-in loops with AND-OR list
#[tokio::test]
async fn set_e_and_list_in_nested_for_in() {
    let mut bash = Bash::new();
    let result = bash
        .exec(
            r#"
set -e
f() {
    for i in a b; do
        for j in a b; do
            [[ "$i" != "$j" ]] && echo "$i != $j"
        done
    done
    echo "done"
}
f
"#,
        )
        .await
        .unwrap();
    assert!(result.stdout.contains("a != b"));
    assert!(result.stdout.contains("b != a"));
    assert!(result.stdout.contains("done"));
}

/// set -e: nested while loop with AND-OR list
#[tokio::test]
async fn set_e_and_list_in_nested_while() {
    let mut bash = Bash::new();
    let result = bash
        .exec(
            r#"
set -e
i=0
while [[ $i -lt 2 ]]; do
    j=0
    while [[ $j -lt 2 ]]; do
        [[ $i -ne $j ]] && echo "$i != $j"
        ((j++)) || true
    done
    ((i++)) || true
done
echo "done"
"#,
        )
        .await
        .unwrap();
    assert!(result.stdout.contains("0 != 1"));
    assert!(result.stdout.contains("1 != 0"));
    assert!(result.stdout.contains("done"));
}

/// set -e should still exit on plain failure inside nested loops
#[tokio::test]
async fn set_e_exits_on_plain_failure_in_nested_loop() {
    let mut bash = Bash::new();
    let result = bash
        .exec(
            r#"
set -e
for ((i=0; i<2; i++)); do
    for ((j=0; j<2; j++)); do
        false
    done
done
echo "SHOULD NOT APPEAR"
"#,
        )
        .await
        .unwrap();
    assert!(!result.stdout.contains("SHOULD NOT APPEAR"));
}

/// set -e: && chain failure at end of for loop body should NOT exit (issue #873)
#[tokio::test]
async fn set_e_and_chain_at_end_of_for_body() {
    let mut bash = Bash::new();
    let result = bash
        .exec(
            r#"
set -euo pipefail
result=""
for src in yes no; do
  [[ "${src}" == "yes" ]] && result="${src}"
done
echo "result: ${result}"
"#,
        )
        .await
        .unwrap();
    assert!(
        result.stdout.contains("result: yes"),
        "should print 'result: yes' but got: {:?}",
        result.stdout
    );
}

/// set -e should still exit on non-AND-OR failures
#[tokio::test]
async fn set_e_exits_on_plain_failure() {
    let mut bash = Bash::new();
    let result = bash
        .exec(
            r#"
set -e
false
echo "SHOULD NOT APPEAR"
"#,
        )
        .await
        .unwrap();
    assert!(!result.stdout.contains("SHOULD NOT APPEAR"));
}
