//! Tests for `Bash::set_env()` on a running instance (issue #2291).
//!
//! Mirrors `live_mount_tests.rs`: hosts can contribute environment after
//! build, without rebuilding the interpreter or losing shell state.

use bashkit::Bash;

#[tokio::test]
async fn set_env_is_visible_to_scripts() {
    let mut bash = Bash::new();

    bash.set_env("SKILL_PATH", "/skills/my-skill");

    let result = bash.exec("echo $SKILL_PATH").await.unwrap();
    assert_eq!(result.stdout, "/skills/my-skill\n");
}

#[tokio::test]
async fn set_env_is_exported_to_child_context() {
    let mut bash = Bash::new();

    bash.set_env("SKILL_PATH", "/skills/my-skill");

    let result = bash.exec("env | grep '^SKILL_PATH='").await.unwrap();
    assert_eq!(result.stdout, "SKILL_PATH=/skills/my-skill\n");
}

#[tokio::test]
async fn set_env_preserves_existing_shell_state() {
    let mut bash = Bash::new();

    bash.exec("export EXISTING=kept").await.unwrap();
    bash.exec("cd /tmp").await.unwrap();

    bash.set_env("ADDED", "new");

    let result = bash.exec("echo $EXISTING $ADDED $PWD").await.unwrap();
    assert_eq!(result.stdout, "kept new /tmp\n");
}

#[tokio::test]
async fn script_assignment_overrides_set_env() {
    let mut bash = Bash::new();

    bash.set_env("SKILL_PATH", "/skills/my-skill");
    bash.exec("export SKILL_PATH=/skills/other").await.unwrap();

    let result = bash.exec("echo $SKILL_PATH").await.unwrap();
    assert_eq!(result.stdout, "/skills/other\n");
}

#[tokio::test]
async fn set_env_overrides_builder_env() {
    // `BashBuilder::env` sets both the exported env entry and a shell variable,
    // and the variable wins during expansion. A host value applied later must
    // beat it, or `set_env` would silently no-op for any pre-configured key.
    let mut bash = Bash::builder().env("SKILL_PATH", "/from-builder").build();

    bash.set_env("SKILL_PATH", "/from-set-env");

    let result = bash.exec("echo $SKILL_PATH").await.unwrap();
    assert_eq!(result.stdout, "/from-set-env\n");
}

#[tokio::test]
async fn set_env_overrides_builder_env_for_child_context() {
    let mut bash = Bash::builder().env("SKILL_PATH", "/from-builder").build();

    bash.set_env("SKILL_PATH", "/from-set-env");

    let result = bash.exec("env | grep '^SKILL_PATH='").await.unwrap();
    assert_eq!(result.stdout, "SKILL_PATH=/from-set-env\n");
}

#[tokio::test]
async fn set_env_overwrites_previous_value() {
    let mut bash = Bash::new();

    bash.set_env("SKILL_PATH", "/first");
    bash.set_env("SKILL_PATH", "/second");

    let result = bash.exec("echo $SKILL_PATH").await.unwrap();
    assert_eq!(result.stdout, "/second\n");
}
