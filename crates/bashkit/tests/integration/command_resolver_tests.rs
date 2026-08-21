//! Integration tests for [`CommandResolver`] — the last-chance name resolver.
//!
//! The contract under test, in order of importance:
//! 1. it runs only when every other dispatch route missed (never shadows),
//! 2. resolved commands go through the normal builtin path, so `before_tool`
//!    fires with the resolved name and can veto them,
//! 3. returning `None` leaves the 127 behavior untouched.

use async_trait::async_trait;
use bashkit::hooks::HookAction;
use bashkit::{Bash, Builtin, BuiltinContext, CommandResolver, ExecResult};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Echoes which name it was resolved under, plus any args and stdin.
struct Resolved {
    name: String,
}

#[async_trait]
impl Builtin for Resolved {
    async fn execute(&self, ctx: BuiltinContext<'_>) -> bashkit::Result<ExecResult> {
        let mut out = format!("resolved:{}", self.name);
        if !ctx.args.is_empty() {
            out.push_str(&format!(" args:{}", ctx.args.join(",")));
        }
        if let Some(stdin) = ctx.stdin {
            out.push_str(&format!(" stdin:{}", stdin.trim_end()));
        }
        out.push('\n');
        Ok(ExecResult::ok(out))
    }
}

/// Resolves every name, counting calls so tests can assert it was not consulted.
struct ResolveAll {
    calls: Arc<AtomicUsize>,
}

impl CommandResolver for ResolveAll {
    fn resolve(&self, name: &str) -> Option<Arc<dyn Builtin>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Some(Arc::new(Resolved { name: name.into() }))
    }
}

/// Resolves nothing — the fall-through case.
struct ResolveNothing;

impl CommandResolver for ResolveNothing {
    fn resolve(&self, _name: &str) -> Option<Arc<dyn Builtin>> {
        None
    }
}

fn resolve_all() -> (Bash, Arc<AtomicUsize>) {
    let calls = Arc::new(AtomicUsize::new(0));
    let bash = Bash::builder()
        .command_resolver(Arc::new(ResolveAll {
            calls: Arc::clone(&calls),
        }))
        .build();
    (bash, calls)
}

#[tokio::test]
async fn resolver_handles_an_otherwise_unknown_command() {
    let (mut bash, calls) = resolve_all();
    let result = bash.exec("definitely-not-a-command-xyz").await.unwrap();
    assert_eq!(result.stdout, "resolved:definitely-not-a-command-xyz\n");
    assert_eq!(result.exit_code, 0);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn resolver_receives_args_and_pipeline_stdin() {
    let (mut bash, _) = resolve_all();
    let result = bash
        .exec("echo piped | some-host-tool --flag v")
        .await
        .unwrap();
    assert_eq!(
        result.stdout,
        "resolved:some-host-tool args:--flag,v stdin:piped\n"
    );
}

#[tokio::test]
async fn resolver_never_shadows_a_baked_in_builtin() {
    let (mut bash, calls) = resolve_all();
    let result = bash.exec("echo hello").await.unwrap();
    assert_eq!(result.stdout, "hello\n");
    assert_eq!(
        calls.load(Ordering::SeqCst),
        0,
        "resolver must not be consulted for a command that already resolves"
    );
}

#[tokio::test]
async fn resolver_never_shadows_a_shell_function() {
    let (mut bash, calls) = resolve_all();
    let result = bash
        .exec("greet() { echo from-function; }; greet")
        .await
        .unwrap();
    assert_eq!(result.stdout, "from-function\n");
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn resolver_never_shadows_a_builder_registered_builtin() {
    let calls = Arc::new(AtomicUsize::new(0));
    let mut bash = Bash::builder()
        .builtin(
            "deploy",
            Box::new(Resolved {
                name: "registered".into(),
            }),
        )
        .command_resolver(Arc::new(ResolveAll {
            calls: Arc::clone(&calls),
        }))
        .build();
    let result = bash.exec("deploy").await.unwrap();
    assert_eq!(result.stdout, "resolved:registered\n");
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn resolver_returning_none_keeps_command_not_found() {
    let mut bash = Bash::builder()
        .command_resolver(Arc::new(ResolveNothing))
        .build();
    let result = bash.exec("definitely-not-a-command-xyz").await.unwrap();
    assert_eq!(result.exit_code, 127);
    assert!(
        result.stderr.contains("command not found"),
        "unexpected: {}",
        result.stderr
    );
}

#[tokio::test]
async fn no_resolver_keeps_command_not_found() {
    let mut bash = Bash::new();
    let result = bash.exec("definitely-not-a-command-xyz").await.unwrap();
    assert_eq!(result.exit_code, 127);
}

#[tokio::test]
async fn resolver_panic_becomes_a_sanitized_shell_error() {
    struct PanickingResolver;

    impl CommandResolver for PanickingResolver {
        fn resolve(&self, _name: &str) -> Option<Arc<dyn Builtin>> {
            panic!("sensitive host resolver detail")
        }
    }

    let mut bash = Bash::builder()
        .command_resolver(Arc::new(PanickingResolver))
        .build();
    let result = bash.exec("attacker-controlled-name").await.unwrap();

    assert_eq!(result.exit_code, 1);
    assert_eq!(
        result.stderr,
        "bash: attacker-controlled-name: resolver failed unexpectedly\n"
    );
    assert!(!result.stderr.contains("sensitive host resolver detail"));
}

/// The security contract from `knowledge/integrations/script-analysis.md`:
/// `before_tool` is the enforcement backstop, and it must keep working for
/// names that only exist because a resolver produced them.
#[tokio::test]
async fn before_tool_veto_blocks_a_resolved_command() {
    let mut bash = Bash::builder()
        .command_resolver(Arc::new(ResolveAll {
            calls: Arc::new(AtomicUsize::new(0)),
        }))
        .before_tool(Box::new(|event| {
            if event.name == "forbidden-tool" {
                HookAction::Cancel("blocked by policy".into())
            } else {
                HookAction::Continue(event)
            }
        }))
        .build();

    let blocked = bash.exec("forbidden-tool").await.unwrap();
    assert_ne!(blocked.exit_code, 0, "veto must not succeed");
    assert!(
        !blocked.stdout.contains("resolved:"),
        "vetoed command still ran: {}",
        blocked.stdout
    );

    let allowed = bash.exec("permitted-tool").await.unwrap();
    assert_eq!(allowed.stdout, "resolved:permitted-tool\n");
}

/// `before_tool` sees the *resolved* name, so an allowlist keyed on names
/// works the same for resolver-provided commands as for registered ones.
#[tokio::test]
async fn before_tool_observes_the_resolved_name() {
    let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
    let recorder = Arc::clone(&seen);
    let mut bash = Bash::builder()
        .command_resolver(Arc::new(ResolveAll {
            calls: Arc::new(AtomicUsize::new(0)),
        }))
        .before_tool(Box::new(move |event| {
            recorder.lock().unwrap().push(event.name.clone());
            HookAction::Continue(event)
        }))
        .build();

    bash.exec("host-tool --version").await.unwrap();
    assert_eq!(seen.lock().unwrap().as_slice(), ["host-tool"]);
}

/// A resolved command is an ordinary command: its exit code drives `&&`/`||`
/// and `if`, and a non-zero code does not abort the script.
#[tokio::test]
async fn resolved_exit_codes_drive_control_flow() {
    struct Failing;
    #[async_trait]
    impl Builtin for Failing {
        async fn execute(&self, _ctx: BuiltinContext<'_>) -> bashkit::Result<ExecResult> {
            Ok(ExecResult::err("boom", 3))
        }
    }
    struct FailResolver;
    impl CommandResolver for FailResolver {
        fn resolve(&self, name: &str) -> Option<Arc<dyn Builtin>> {
            (name == "failing-tool").then(|| Arc::new(Failing) as Arc<dyn Builtin>)
        }
    }

    let mut bash = Bash::builder()
        .command_resolver(Arc::new(FailResolver))
        .build();

    let result = bash.exec("failing-tool; echo after").await.unwrap();
    assert!(
        result.stdout.contains("after"),
        "script aborted: {result:?}"
    );

    let result = bash.exec("failing-tool || echo recovered").await.unwrap();
    assert!(result.stdout.contains("recovered"));

    let result = bash.exec("failing-tool").await.unwrap();
    assert_eq!(result.exit_code, 3);
}

/// Resolver names are not enumerable, so they must not appear in
/// `builtin_names()` — an embedder gating on that list needs to know.
#[tokio::test]
async fn resolved_names_are_not_enumerable() {
    let (bash, _) = resolve_all();
    assert!(
        !bash
            .builtin_names()
            .contains(&"anything-at-all".to_string())
    );
}
