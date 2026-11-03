//! Finite, ordinary indirect-reference compatibility fixtures.

use bashkit::Bash;
use std::process::Command;

#[tokio::test]
async fn indirect_reference_nested_index_matches_bash() {
    let script = r#"a=(zero one); b=(1); r='a[b[0]]'; printf '%s/%s\n' "${!r}" "${a[b[0]]}""#;
    let result = Bash::new().exec(script).await.unwrap();
    let oracle = Command::new("bash")
        .env_clear()
        .args(["--noprofile", "--norc", "-c", script])
        .output()
        .unwrap();
    assert_eq!(result.stdout, "one/one\n");
    assert_eq!(result.stderr, "");
    assert_eq!(result.exit_code, 0);
    assert_eq!(result.stdout.as_bytes(), oracle.stdout);
    assert_eq!(result.stderr.as_bytes(), oracle.stderr);
    assert_eq!(Some(result.exit_code), oracle.status.code());
}

#[tokio::test]
async fn indirect_reference_quoted_and_escaped_bracket_keys() {
    let script = r#"
declare -A items
items['left]right']=ok
quoted='items["left]right"]'
escaped='items[left\]right]'
printf '%s/%s/%s\n' "${!quoted}" "${!escaped}" "${items['left]right']}"
"#;
    let result = Bash::new().exec(script).await.unwrap();
    assert_eq!(result.stdout, "ok/ok/ok\n");
    assert_eq!(result.stderr, "");
    assert_eq!(result.exit_code, 0);
}

#[tokio::test]
async fn indirect_reference_explicit_default_and_slice_remain_valid() {
    let script = r#"a=(zero one); r='a[1]'; printf '%s/%s\n' "${!r:-fallback}" "${!r:1}""#;
    let result = Bash::new().exec(script).await.unwrap();
    let oracle = Command::new("bash")
        .env_clear()
        .args(["--noprofile", "--norc", "-c", script])
        .output()
        .unwrap();
    assert_eq!(result.stdout, "one/ne\n");
    assert_eq!(result.stderr, "");
    assert_eq!(result.exit_code, 0);
    assert_eq!(result.stdout.as_bytes(), oracle.stdout);
    assert_eq!(result.stderr.as_bytes(), oracle.stderr);
    assert_eq!(Some(result.exit_code), oracle.status.code());
}

#[tokio::test]
async fn indirect_reference_trailing_text_is_rejected_and_session_recovers() {
    let mut bash = Bash::new();
    let result = bash
        .exec(r#"r='items[0][1]'; printf '%s' "${!r}""#)
        .await
        .unwrap();
    assert_eq!(result.stdout, "");
    assert!(result.stderr.contains("invalid variable name"));
    assert_eq!(result.exit_code, 1);
    let recovered = bash.exec("printf ok").await.unwrap();
    assert_eq!(recovered.stdout, "ok");
    assert_eq!(recovered.stderr, "");
    assert_eq!(recovered.exit_code, 0);
}
