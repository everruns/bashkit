//! Test for issue #1777: `bash -c` subshell should NOT inherit the parent's
//! associative-array (or indexed-array, or non-exported variable) state.
//!
//! In real bash, `bash -c '...'` spawns a fresh child process. Associative
//! arrays cannot be exported, so the child starts with empty arrays — and
//! `declare -A name` inside the child sees an empty array, not the parent's.

use bashkit::Bash;

#[tokio::test]
async fn bash_c_does_not_inherit_assoc_array() {
    let mut bash = Bash::new();
    let result = bash
        .exec(
            r#"
declare -A outer
outer[x]=42
echo "parent: ${outer[x]}"
bash -c '
declare -A outer
echo "child: [${outer[x]:-EMPTY}]"
'
"#,
        )
        .await
        .unwrap();
    assert!(
        result.stdout.contains("parent: 42"),
        "parent value should be 42, got: {}",
        result.stdout
    );
    assert!(
        result.stdout.contains("child: [EMPTY]"),
        "child should not inherit assoc array (got: {})",
        result.stdout
    );
}

#[tokio::test]
async fn bash_c_does_not_inherit_indexed_array() {
    let mut bash = Bash::new();
    let result = bash
        .exec(
            r#"
declare -a outer
outer[0]=hello
echo "parent: ${outer[0]}"
bash -c '
declare -a outer
echo "child: [${outer[0]:-EMPTY}]"
'
"#,
        )
        .await
        .unwrap();
    assert!(
        result.stdout.contains("parent: hello"),
        "parent value should be hello, got: {}",
        result.stdout
    );
    assert!(
        result.stdout.contains("child: [EMPTY]"),
        "child should not inherit indexed array (got: {})",
        result.stdout
    );
}

#[tokio::test]
async fn bash_c_does_not_inherit_unexported_scalar() {
    let mut bash = Bash::new();
    let result = bash
        .exec(
            r#"
local_var=parent_only
bash -c 'echo "child: [${local_var:-EMPTY}]"'
"#,
        )
        .await
        .unwrap();
    assert!(
        result.stdout.contains("child: [EMPTY]"),
        "child should not see unexported scalar (got: {})",
        result.stdout
    );
}

#[tokio::test]
async fn bash_c_does_inherit_exported_scalar() {
    let mut bash = Bash::new();
    let result = bash
        .exec(
            r#"
export shared=hello_world
bash -c 'echo "child: [${shared:-EMPTY}]"'
"#,
        )
        .await
        .unwrap();
    assert!(
        result.stdout.contains("child: [hello_world]"),
        "child should inherit exported scalar (got: {})",
        result.stdout
    );
}

#[tokio::test]
async fn bash_c_mutations_do_not_leak_to_parent() {
    let mut bash = Bash::new();
    let result = bash
        .exec(
            r#"
export shared=parent
bash -c 'shared=child; echo "in child: $shared"'
echo "back in parent: $shared"
"#,
        )
        .await
        .unwrap();
    assert!(
        result.stdout.contains("in child: child"),
        "child should set its own value (got: {})",
        result.stdout
    );
    assert!(
        result.stdout.contains("back in parent: parent"),
        "parent value must not change after child runs (got: {})",
        result.stdout
    );
}
