//! Test for issue #875: Associative array key expansion (`${!assoc[@]}`)
//! returns empty when used inside a process substitution nested inside a
//! command substitution.

use bashkit::Bash;

#[tokio::test]
async fn assoc_keys_visible_in_nested_procsub_cmdsub() {
    let mut bash = Bash::new();
    let result = bash
        .exec(
            r#"
myfunc() {
  declare -A m=()
  m["a"]="1"
  m["b"]="2"

  # Process substitution inside command substitution
  result="$(while IFS= read -r k; do echo "$k"; done < <(printf '%s\n' "${!m[@]}"))"
  echo "nested result: [$result]"
}
myfunc
"#,
        )
        .await
        .unwrap();
    // Keys should be visible (order may vary since assoc arrays are unordered)
    assert!(
        result.stdout.contains("a") && result.stdout.contains("b"),
        "expected keys 'a' and 'b' in nested result, got: {}",
        result.stdout
    );
    assert!(
        !result.stdout.contains("nested result: []"),
        "nested result should not be empty, got: {}",
        result.stdout
    );
}

#[tokio::test]
async fn assoc_keys_direct_expansion() {
    let mut bash = Bash::new();
    let result = bash
        .exec(
            r#"
myfunc() {
  declare -A m=()
  m["x"]="10"
  m["y"]="20"
  echo "direct keys: ${!m[@]}"
}
myfunc
"#,
        )
        .await
        .unwrap();
    assert!(
        result.stdout.contains("x") && result.stdout.contains("y"),
        "expected keys 'x' and 'y', got: {}",
        result.stdout
    );
}

#[tokio::test]
async fn assoc_keys_in_procsub_alone() {
    let mut bash = Bash::new();
    let result = bash
        .exec(
            r#"
myfunc() {
  declare -A m=()
  m["p"]="1"
  m["q"]="2"
  while IFS= read -r k; do echo "procsub: $k"; done < <(printf '%s\n' "${!m[@]}")
}
myfunc
"#,
        )
        .await
        .unwrap();
    assert!(
        result.stdout.contains("procsub: p") || result.stdout.contains("procsub: q"),
        "expected keys in procsub output, got: {}",
        result.stdout
    );
}
