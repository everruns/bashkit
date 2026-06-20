//! Integration tests for the host-owned `BuiltinRegistry`.
//!
//! Verifies that builtins registered through a `BuiltinRegistry` handle
//! survive interpreter mutation (VFS writes, variable assignments,
//! `reset_transient_state`) and that the host can add/remove entries
//! after the `Bash` instance has been built — without rebuilding it.

use async_trait::async_trait;
use bashkit::{Bash, Builtin, BuiltinContext, BuiltinRegistry, ExecResult, hooks};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

struct EchoArgs;

#[async_trait]
impl Builtin for EchoArgs {
    async fn execute(&self, ctx: BuiltinContext<'_>) -> bashkit::Result<ExecResult> {
        Ok(ExecResult::ok(format!("{}\n", ctx.args.join(","))))
    }
}

struct CountCalls {
    counter: Arc<AtomicU32>,
}

#[async_trait]
impl Builtin for CountCalls {
    async fn execute(&self, _ctx: BuiltinContext<'_>) -> bashkit::Result<ExecResult> {
        let n = self.counter.fetch_add(1, Ordering::SeqCst) + 1;
        Ok(ExecResult::ok(format!("call #{}\n", n)))
    }
}

#[tokio::test]
async fn registry_lookup_dispatches_builtin() {
    let registry = BuiltinRegistry::new();
    registry.insert("greet", Arc::new(EchoArgs));

    let mut bash = Bash::builder().builtin_registry(registry).build();
    let result = bash.exec("greet hello world").await.unwrap();

    assert_eq!(result.stdout, "hello,world\n");
    assert_eq!(result.exit_code, 0);
}

#[tokio::test]
async fn registry_entries_added_after_build_are_visible() {
    let registry = BuiltinRegistry::new();
    let mut bash = Bash::builder().builtin_registry(registry.clone()).build();

    // Run something first so the interpreter has state.
    bash.exec("mkdir -p /scratch && echo seed > /scratch/seed.txt")
        .await
        .unwrap();

    // Now register a new builtin — must be visible without rebuilding.
    registry.insert("post-build", Arc::new(EchoArgs));
    let result = bash.exec("post-build alpha beta").await.unwrap();
    assert_eq!(result.stdout, "alpha,beta\n");

    // Critical: pre-existing VFS contents are preserved.
    let result = bash.exec("cat /scratch/seed.txt").await.unwrap();
    assert_eq!(result.stdout, "seed\n");
}

#[tokio::test]
async fn registry_removal_takes_effect_immediately() {
    let registry = BuiltinRegistry::new();
    registry.insert("tmp", Arc::new(EchoArgs));

    let mut bash = Bash::builder().builtin_registry(registry.clone()).build();

    assert_eq!(bash.exec("tmp ok").await.unwrap().stdout, "ok\n");

    registry.remove("tmp");
    let result = bash.exec("tmp ok").await.unwrap();
    assert_eq!(result.exit_code, 127);
}

#[tokio::test]
async fn registry_overrides_baked_in_builtin() {
    let registry = BuiltinRegistry::new();
    // Override `echo`: host wins over baked-in.
    registry.insert("echo", Arc::new(EchoArgs));

    let mut bash = Bash::builder().builtin_registry(registry).build();
    let result = bash.exec("echo a b c").await.unwrap();

    // Baked-in echo emits "a b c\n"; our override emits "a,b,c\n".
    assert_eq!(result.stdout, "a,b,c\n");
}

#[tokio::test]
async fn shell_function_overrides_host_registry() {
    let registry = BuiltinRegistry::new();
    registry.insert("name", Arc::new(EchoArgs));

    let mut bash = Bash::builder().builtin_registry(registry).build();
    let result = bash
        .exec("name() { echo from-function; }\nname host")
        .await
        .unwrap();

    assert_eq!(result.stdout, "from-function\n");
}

#[tokio::test]
async fn registry_shared_across_clones() {
    let counter = Arc::new(AtomicU32::new(0));
    let registry = BuiltinRegistry::new();
    registry.insert(
        "count",
        Arc::new(CountCalls {
            counter: counter.clone(),
        }),
    );

    // Hand a clone to the builder, keep the original for mutation.
    let mut bash = Bash::builder().builtin_registry(registry.clone()).build();

    bash.exec("count").await.unwrap();
    bash.exec("count").await.unwrap();
    assert_eq!(counter.load(Ordering::SeqCst), 2);

    // Inserting via the original clone is visible to the interpreter.
    registry.insert("count2", Arc::new(EchoArgs));
    let result = bash.exec("count2 x").await.unwrap();
    assert_eq!(result.stdout, "x\n");
}

#[tokio::test]
async fn registry_survives_multiple_exec_calls_with_state() {
    let registry = BuiltinRegistry::new();
    registry.insert("emit", Arc::new(EchoArgs));

    let mut bash = Bash::builder().builtin_registry(registry).build();

    bash.exec("mkdir -p /data").await.unwrap();
    bash.exec("emit 1 > /data/a.txt").await.unwrap();
    bash.exec("emit 2 > /data/b.txt").await.unwrap();

    let result = bash.exec("cat /data/a.txt /data/b.txt").await.unwrap();
    assert_eq!(result.stdout, "1\n2\n");
}

#[tokio::test]
async fn registry_pipe_chain() {
    let registry = BuiltinRegistry::new();
    registry.insert("emit", Arc::new(EchoArgs));

    struct Upper;
    #[async_trait]
    impl Builtin for Upper {
        async fn execute(&self, ctx: BuiltinContext<'_>) -> bashkit::Result<ExecResult> {
            Ok(ExecResult::ok(ctx.stdin.unwrap_or("").to_uppercase()))
        }
    }
    registry.insert("upper", Arc::new(Upper));

    let mut bash = Bash::builder().builtin_registry(registry).build();
    let result = bash.exec("emit hello world | upper").await.unwrap();
    assert_eq!(result.stdout, "HELLO,WORLD\n");
}

#[tokio::test]
async fn command_v_finds_host_builtin() {
    let registry = BuiltinRegistry::new();
    registry.insert("custom-cmd", Arc::new(EchoArgs));

    let mut bash = Bash::builder().builtin_registry(registry).build();

    // `command -v custom-cmd` should print its name (found).
    let result = bash.exec("command -v custom-cmd").await.unwrap();
    assert_eq!(result.stdout, "custom-cmd\n");

    // `command -V` should describe it as a builtin.
    let result = bash.exec("command -V custom-cmd").await.unwrap();
    assert!(
        result.stdout.contains("is a shell builtin"),
        "got: {}",
        result.stdout
    );
}

#[tokio::test]
async fn command_respects_before_tool_for_host_builtin() {
    let registry = BuiltinRegistry::new();
    registry.insert("sensitive", Arc::new(EchoArgs));

    let mut bash = Bash::builder()
        .builtin_registry(registry)
        .before_tool(Box::new(|event: hooks::ToolEvent| {
            if event.name == "sensitive" {
                hooks::HookAction::Cancel("sensitive blocked".to_string())
            } else {
                hooks::HookAction::Continue(event)
            }
        }))
        .build();

    let direct = bash.exec("sensitive direct").await.unwrap();
    assert_eq!(direct.exit_code, 1);
    assert!(direct.stderr.contains("cancelled by before_tool hook"));

    let via_command = bash.exec("command sensitive via-command").await.unwrap();
    assert_eq!(via_command.exit_code, 1);
    assert!(via_command.stderr.contains("cancelled by before_tool hook"));
}

#[tokio::test]
async fn builtin_names_lists_baked_in_and_host_builtins() {
    // Baked-in builtins, sorted, no duplicates.
    let bash = Bash::new();
    let names = bash.builtin_names();
    for expected in ["echo", "cd", "grep", "awk", "sed"] {
        assert!(names.iter().any(|n| n == expected), "missing {expected}");
    }
    for special in [
        ".", "bash", "command", "declare", "eval", "exec", "getopts", "let", "local", "sh",
        "source", "typeset", "unset",
    ] {
        assert!(
            names.iter().any(|n| n == special),
            "missing special builtin {special}"
        );
    }
    let mut sorted = names.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(names, sorted, "names must be sorted and deduped");

    // Host-registered builtins are included; unknown names are not.
    let registry = BuiltinRegistry::new();
    registry.insert("custom-cmd", Arc::new(EchoArgs));
    let bash = Bash::builder().builtin_registry(registry).build();
    let names = bash.builtin_names();
    assert!(names.iter().any(|n| n == "custom-cmd"));
    assert!(!names.iter().any(|n| n == "no-such-builtin"));
}
