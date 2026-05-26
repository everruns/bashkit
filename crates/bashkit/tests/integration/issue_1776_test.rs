//! Test for issue #1776: associative array element assignment doesn't persist
//! across `for` loop iterations when the right-hand-side is an arithmetic
//! expansion that reads the same element via `${arr[$key]:-default}`.

use bashkit::Bash;

#[tokio::test]
async fn assoc_array_increment_across_for_loop() {
    let mut bash = Bash::new();
    let result = bash
        .exec(
            r#"
declare -A counts
for item in a b a c a b; do
    counts[$item]=$(( ${counts[$item]:-0} + 1 ))
done
echo "a=${counts[a]} b=${counts[b]} c=${counts[c]}"
"#,
        )
        .await
        .unwrap();
    assert!(
        result.stdout.contains("a=3 b=2 c=1"),
        "expected a=3 b=2 c=1, got: {}",
        result.stdout
    );
}

#[tokio::test]
async fn assoc_array_default_in_arithmetic_with_variable_subscript() {
    // Narrow repro: ${arr[$key]:-N} inside $(( )) must resolve the subscript
    // through the array lookup path, not via `expand_variable("arr[$key]")`.
    let mut bash = Bash::new();
    let result = bash
        .exec(
            r#"
declare -A counts
counts[a]=5
item=a
echo "outside=$(( ${counts[$item]:-0} ))"
"#,
        )
        .await
        .unwrap();
    assert!(
        result.stdout.contains("outside=5"),
        "expected outside=5, got: {}",
        result.stdout
    );
}
