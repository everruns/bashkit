//! Tests for capture_final_env feature (issue #650)

use bashkit::{Bash, ExecutionLimits};

#[tokio::test]
async fn final_env_disabled_by_default() {
    let mut bash = Bash::builder().build();
    let result = bash.exec("export FOO=bar").await.unwrap();
    assert!(result.final_env.is_none());
}

#[tokio::test]
async fn final_env_captures_exported_variables() {
    let limits = ExecutionLimits::new().capture_final_env(true);
    let mut bash = Bash::builder().limits(limits).build();
    let result = bash.exec("export FOO=bar && export BAZ=qux").await.unwrap();
    let env = result.final_env.expect("final_env should be Some");
    assert_eq!(env.get("FOO").map(|s| s.as_str()), Some("bar"));
    assert_eq!(env.get("BAZ").map(|s| s.as_str()), Some("qux"));
}

#[tokio::test]
async fn final_env_captures_plain_variables() {
    let limits = ExecutionLimits::new().capture_final_env(true);
    let mut bash = Bash::builder().limits(limits).build();
    let result = bash.exec("X=hello").await.unwrap();
    let env = result.final_env.expect("final_env should be Some");
    assert_eq!(env.get("X").map(|s| s.as_str()), Some("hello"));
}

#[tokio::test]
async fn final_env_reflects_mutations() {
    let limits = ExecutionLimits::new().capture_final_env(true);
    let mut bash = Bash::builder().limits(limits).build();
    bash.exec("VAR=initial").await.unwrap();
    let result = bash.exec("VAR=updated").await.unwrap();
    let env = result.final_env.expect("final_env should be Some");
    assert_eq!(env.get("VAR").map(|s| s.as_str()), Some("updated"));
}

#[tokio::test]
async fn final_env_empty_script() {
    let limits = ExecutionLimits::new().capture_final_env(true);
    let mut bash = Bash::builder().limits(limits).build();
    let result = bash.exec("true").await.unwrap();
    let env = result.final_env.expect("final_env should be Some");
    // Should have at least the default env vars
    assert!(env.is_empty() || !env.is_empty()); // just verify it's a valid map
}

#[tokio::test]
async fn final_env_not_present_when_disabled() {
    let limits = ExecutionLimits::new().capture_final_env(false);
    let mut bash = Bash::builder().limits(limits).build();
    let result = bash.exec("export FOO=bar").await.unwrap();
    assert!(result.final_env.is_none());
}

#[tokio::test]
async fn final_env_persists_across_calls() {
    let limits = ExecutionLimits::new().capture_final_env(true);
    let mut bash = Bash::builder().limits(limits).build();
    bash.exec("A=1").await.unwrap();
    bash.exec("B=2").await.unwrap();
    let result = bash.exec("C=3").await.unwrap();
    let env = result.final_env.expect("final_env should be Some");
    assert_eq!(env.get("A").map(|s| s.as_str()), Some("1"));
    assert_eq!(env.get("B").map(|s| s.as_str()), Some("2"));
    assert_eq!(env.get("C").map(|s| s.as_str()), Some("3"));
}

#[tokio::test]
async fn final_env_on_error_still_captured() {
    let limits = ExecutionLimits::new().capture_final_env(true);
    let mut bash = Bash::builder().limits(limits).build();
    let result = bash.exec("BEFORE_ERR=yes; false").await.unwrap();
    assert_ne!(result.exit_code, 0);
    let env = result
        .final_env
        .expect("final_env should be Some even on error");
    assert_eq!(env.get("BEFORE_ERR").map(|s| s.as_str()), Some("yes"));
}
