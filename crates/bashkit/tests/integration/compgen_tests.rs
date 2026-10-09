//! Behavioral tests for `compgen` builtin/function/alias listing.
//!
//! `compgen -b` reads the live builtin registry — the same source
//! `Bash::builtin_names()` reads — keeping bash's own builtins and
//! host-registered commands (the rest stand in for programs: `-c` only).

use std::collections::HashSet;
use std::sync::Arc;

use bashkit::{Bash, Builtin, BuiltinContext, BuiltinRegistry, ExecResult, async_trait};

struct NoopBuiltin;

#[async_trait]
impl Builtin for NoopBuiltin {
    async fn execute(&self, _ctx: BuiltinContext<'_>) -> bashkit::Result<ExecResult> {
        Ok(ExecResult::ok(String::new()))
    }
}

fn stdout_set(result: &ExecResult) -> HashSet<String> {
    result
        .stdout
        .lines()
        .map(|l| l.to_string())
        .collect::<HashSet<_>>()
}

#[tokio::test]
async fn compgen_b_lists_bash_builtins_in_the_registry() {
    let mut bash = Bash::new();
    let names: HashSet<String> = bash.builtin_names().into_iter().collect();

    let result = bash.exec("compgen -b").await.unwrap();
    assert_eq!(result.exit_code, 0, "stderr: {}", result.stderr);
    let listed = stdout_set(&result);

    // bash's own builtins that bashkit registers; commands standing in for
    // programs (`awk`, `grep`) are `-c` only, as in bash.
    assert!(listed.contains("getopts") && listed.contains("echo"));
    assert!(!listed.contains("grep") && !listed.contains("awk"));
    assert!(
        listed.is_subset(&names),
        "compgen -b lists only registered names"
    );
}

#[tokio::test]
async fn compgen_b_includes_host_registered_builtin() {
    let registry = BuiltinRegistry::new();
    registry.insert("my-host-cmd", Arc::new(NoopBuiltin));
    let mut bash = Bash::builder().builtin_registry(registry).build();

    let result = bash.exec("compgen -b my-host").await.unwrap();
    assert_eq!(result.exit_code, 0, "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "my-host-cmd\n");

    // And builtin_names() agrees.
    assert!(bash.builtin_names().iter().any(|n| n == "my-host-cmd"));
}

#[tokio::test]
async fn compgen_action_builtin_lists_builtins_only() {
    let mut bash = Bash::new();
    // `awk` is a bashkit builtin standing in for a program; like bash,
    // `-A builtin` lists only bash's builtins (`getopts`).
    let result = bash.exec("compgen -A builtin awk").await.unwrap();
    assert_eq!(result.exit_code, 1, "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "");
    let result = bash.exec("compgen -A builtin g").await.unwrap();
    assert_eq!(result.stdout, "getopts\n");

    // -A builtin must NOT include functions.
    let result = bash
        .exec("myfn() { :; }; compgen -A builtin myfn")
        .await
        .unwrap();
    assert_eq!(result.exit_code, 1);
    assert_eq!(result.stdout, "");
}

#[tokio::test]
async fn compgen_action_function_lists_functions() {
    let mut bash = Bash::new();
    let result = bash
        .exec("greet() { echo hi; }; other() { :; }; compgen -A function gre")
        .await
        .unwrap();
    assert_eq!(result.exit_code, 0, "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "greet\n");
}

#[tokio::test]
async fn compgen_action_alias_lists_aliases() {
    let mut bash = Bash::new();
    let result = bash
        .exec("alias ll='ls -l'; alias gg='git'; compgen -A alias l")
        .await
        .unwrap();
    assert_eq!(result.exit_code, 0, "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "ll\n");
}

#[tokio::test]
async fn compgen_c_includes_registry_builtins_and_functions() {
    let mut bash = Bash::new();
    let result = bash.exec("ech_fn() { :; }; compgen -c ech").await.unwrap();
    assert_eq!(result.exit_code, 0, "stderr: {}", result.stderr);
    let listed = stdout_set(&result);
    assert!(listed.contains("echo"), "got: {listed:?}");
    assert!(listed.contains("ech_fn"), "got: {listed:?}");
}
