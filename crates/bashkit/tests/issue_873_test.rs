//! Test for issue #873: set -e incorrectly triggers on compound commands
//! whose body ends with && chain failure.

use bashkit::Bash;

#[tokio::test]
async fn set_e_for_loop_and_chain_no_exit() {
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
        "expected 'result: yes', got: {}",
        result.stdout
    );
}

#[tokio::test]
async fn set_e_while_loop_and_chain_no_exit() {
    let mut bash = Bash::new();
    let result = bash
        .exec(
            r#"
set -e
i=0
while [[ $i -lt 3 ]]; do
    [[ $i -eq 1 ]] && echo "found one"
    ((i++)) || true
done
echo "done"
"#,
        )
        .await
        .unwrap();
    assert!(
        result.stdout.contains("found one"),
        "stdout: {}",
        result.stdout
    );
    assert!(result.stdout.contains("done"), "stdout: {}", result.stdout);
}

#[tokio::test]
async fn set_e_plain_failure_in_loop_still_exits() {
    let mut bash = Bash::new();
    let result = bash
        .exec(
            r#"
set -e
for x in a b; do
    false
done
echo "SHOULD NOT APPEAR"
"#,
        )
        .await
        .unwrap();
    assert!(!result.stdout.contains("SHOULD NOT APPEAR"));
}
