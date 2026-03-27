//! Test for issue #872: Associative array keys with command substitutions
//! expand to empty string.

use bashkit::Bash;

#[tokio::test]
async fn assoc_key_command_substitution() {
    let mut bash = Bash::new();
    let result = bash
        .exec(
            r#"
declare -A m=()
m["$(echo hello)"]="world"
echo "count: ${#m[@]}"
for k in "${!m[@]}"; do echo "key=[$k] val=[${m[$k]}]"; done
"#,
        )
        .await
        .unwrap();
    assert!(
        result.stdout.contains("key=[hello] val=[world]"),
        "expected key=[hello], got: {}",
        result.stdout
    );
}

#[tokio::test]
async fn assoc_key_variable_expansion() {
    let mut bash = Bash::new();
    let result = bash
        .exec(
            r#"
declare -A m=()
key="mykey"
m[$key]="myval"
echo "${m[mykey]}"
"#,
        )
        .await
        .unwrap();
    assert!(
        result.stdout.contains("myval"),
        "expected myval, got: {}",
        result.stdout
    );
}

#[tokio::test]
async fn assoc_key_literal_unchanged() {
    let mut bash = Bash::new();
    let result = bash
        .exec(
            r#"
declare -A m=()
m[literal]="val"
echo "${m[literal]}"
"#,
        )
        .await
        .unwrap();
    assert!(
        result.stdout.contains("val"),
        "expected val, got: {}",
        result.stdout
    );
}
