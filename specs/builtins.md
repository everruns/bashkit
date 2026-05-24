# Builtin Commands

## Status
Implemented

## Decision

Bashkit provides built-in commands for script execution in a virtual environment.
All builtins operate on the virtual filesystem. For the complete list of 160
builtins and per-command details, see `specs/implementation-status.md`.

### Standard Flags

All external-style builtins support `--help` and `--version` flags via the
`check_help_version()` helper in `builtins/mod.rs` (long flags only — short
flags `-h`/`-V` are not handled by the helper since they have different meanings
in many tools). Tools where `-h`/`-V` genuinely mean help/version handle them
directly in their `execute()` method.

### Command Dispatch Order

functions → special commands → builtins → path execution → $PATH search → "command not found"

Scripts containing `/` are resolved against VFS. Commands without `/` are
searched in `$PATH` directories. Shebang lines are stripped; content executed
as bash. Exit 127: not found; Exit 126: not executable or is a directory.

### Builtin Trait

```rust
#[async_trait]
pub trait Builtin: Send + Sync {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult>;

    /// Return an execution plan for sub-command execution.
    /// Default: Ok(None) — normal execute() is used.
    async fn execution_plan(&self, ctx: &Context<'_>) -> Result<Option<ExecutionPlan>> {
        Ok(None)
    }
}

pub struct Context<'a> {
    pub args: &'a [String],
    pub env: &'a HashMap<String, String>,
    pub variables: &'a mut HashMap<String, String>,
    pub cwd: &'a mut PathBuf,
    pub fs: Arc<dyn FileSystem>,
    pub stdin: Option<&'a str>,
    #[cfg(feature = "http_client")]
    pub http_client: Option<&'a HttpClient>,
    #[cfg(feature = "git")]
    pub git_client: Option<&'a GitClient>,
    /// Internal builtins only — None for custom builtins.
    pub(crate) shell: Option<ShellRef<'a>>,
}

impl Context<'_> {
    pub fn execution_extension<T>(&self) -> Option<&T>
    where
        T: Send + Sync + 'static;
}
```

### Clap-Backed Custom Builtins

Custom Rust builtins can implement `ClapBuiltin` instead of `Builtin` when
their arguments are better represented as a `#[derive(clap::Parser)]` struct.
`clap` is an unconditional dependency of `bashkit` (also used by ported
coreutils argument surfaces — see `specs/coreutils-args-port.md`), so this
trait is always available.
Bashkit parses `Context::args` through clap, passes parsed args plus a mutable
`BashkitContext` to the handler, maps `--help` and `--version` to successful
stdout results, and maps clap parse failures to stderr with clap's exit code.
Parse diagnostics are capped to 1 KB to preserve TM-INF-022 stderr constraints.

```rust
use bashkit::{BashkitContext, ClapBuiltin, async_trait};
use bashkit::clap::Parser;

#[derive(Parser)]
#[command(name = "greet")]
struct GreetArgs {
    #[arg(short, long, default_value = "World")]
    name: String,
}

struct Greet;

#[async_trait]
impl ClapBuiltin for Greet {
    type Args = GreetArgs;

    async fn execute_clap(
        &self,
        args: Self::Args,
        ctx: &mut BashkitContext<'_>,
    ) -> bashkit::Result<()> {
        ctx.write_stdout(format!("Hello, {}!\n", args.name));
        Ok(())
    }
}
```

### Extension Trait

Extensions bundle a related set of builtins so embedders can add one capability
to `BashBuilder` or `BashToolBuilder` instead of registering each command
manually.

```rust
pub trait Extension: Send + Sync {
    fn builtins(&self) -> Vec<(String, Box<dyn Builtin>)>;
}
```

Rules:

- `BashBuilder::extension(ext)` expands each returned builtin into the builder's
  custom builtin map
- `BashToolBuilder::extension(ext)` expands each returned builtin into the
  tool's custom builtin list
- For `BashBuilder`, later registrations with the same command name override
  earlier registrations, matching `BashBuilder::builtin`
- Extensions must construct fresh builtin values or use shared ownership
  internally; builders may call `builtins()` when configuring reusable tools

Current extension:

- `TypeScriptExtension` registers `ts`/`typescript` and, when enabled by
  `TypeScriptConfig`, `node`/`deno`/`bun`

### BuiltinRegistry — Host-Owned Mutable Builtins

`BashBuilder::builtin(name, ...)` and `Extension::builtins()` are both
*build-time* registration: the set of builtins is frozen once the `Bash`
instance is built. For embedders that need to register or remove builtins
*after* construction (FFI bindings, REPLs, plugin systems),
`BuiltinRegistry` provides a host-owned mutable registry consulted at
command-dispatch time.

```rust
#[derive(Clone, Default)]
pub struct BuiltinRegistry {
    inner: Arc<RwLock<HashMap<String, Arc<dyn Builtin>>>>,
}

impl BuiltinRegistry {
    pub fn new() -> Self;
    pub fn insert(&self, name: impl Into<String>, builtin: Arc<dyn Builtin>);
    pub fn remove(&self, name: &str) -> Option<Arc<dyn Builtin>>;
    pub fn lookup(&self, name: &str) -> Option<Arc<dyn Builtin>>;
    pub fn names(&self) -> Vec<String>;
    pub fn is_empty(&self) -> bool;
}
```

Wired in via `BashBuilder::builtin_registry(registry)`. The handle is
`Clone`; clones share the same underlying storage, so the embedder keeps a
clone for runtime mutation while the builder takes another.

Command-resolution order (see `Interpreter::dispatch_command`):

1. Shell functions (defined in scripts)
2. POSIX special builtins (`exec`, `set`, `:`, `eval`, …)
3. **Host registry** (`BuiltinRegistry::lookup`)
4. Baked-in + builder-registered builtins
5. Script execution by path / `$PATH` search

So registry entries can override baked-in commands (e.g. wrap `cat` with
tracing) but shell functions still win — matching standard bash
precedence. `command -v` / `command -V` / `command name args…` consult
the registry too.

Implementation notes:

- Storage is `Arc<RwLock<HashMap<String, Arc<dyn Builtin>>>>` (std only,
  no extra deps). Lookup clones the `Arc` out of the lock, releasing it
  before execution.
- `Interpreter::builtins` was migrated from `HashMap<String, Box<dyn Builtin>>`
  to `HashMap<String, Arc<dyn Builtin>>` so registered and host-registry
  paths share one execution helper (`execute_builtin_arc`).
- The registry is host-owned: not part of interpreter state, so
  `reset_transient_state` leaves it untouched and snapshots do not
  serialize it. Restoring from a snapshot requires re-attaching the
  registry handle.

### Execution Extensions

`Bash::exec_with_extensions()` and `Bash::exec_streaming_with_extensions()`
accept a typed, per-call extension bag. Builtins read values from it via
`ctx.execution_extension::<T>()`.

Use this for request-scoped data that is not shell state:

- tracing/request IDs
- auth or tenant context
- host-language runtime sessions (Python/JS callback bridges)
- metrics/audit sinks for one execution

Rules:

- Extensions live for exactly one `exec*()` call
- Builtins may read them but must not retain references beyond execution
- Long-lived builtin registrations must not store request-scoped data themselves

### Shell State Access (ShellRef)

Internal builtins that need interpreter state receive it via `Context.shell`:

**Design rationale:**
- **Direct mutation** for aliases/traps — simple HashMaps with no invariants
- **Side effects** for arrays (budget checks), positional params (call stack),
  history (VFS persistence) — state with invariants the interpreter must enforce
- **Read-only methods** for introspection (functions, builtins, keywords,
  call stack, history, jobs) — builtins shouldn't mutate these
- `pub(crate)` keeps ShellRef out of the public API; custom builtins use
  public `execution_extension()` instead of direct shell access
- No dynamic dispatch — concrete struct, not trait

**Builtins using ShellRef:**
- `type`, `which` — read-only: check builtin/function/keyword names
- `alias`, `unalias` — direct mutation of `shell.aliases`
- `trap` — direct mutation of `shell.traps`
- `caller` — read call stack depth/frame names
- `history` — read history entries, clear via `ClearHistory` side effect
- `wait` — read job table, set exit code via `SetLastExitCode` side effect
- `mapfile`/`readarray` — set arrays via `SetIndexedArray` side effect

**Builtins still in interpreter dispatch chain** (fundamentally need interpreter):
- `exec` — redirect management, VFS I/O
- `local` — call frame locals mutation
- `source`/`.`, `eval` — parse and execute in current context
- `bash`/`sh` — script execution
- `command` — dispatch to builtins/functions
- `declare`/`typeset` — arrays, assoc arrays, variable attributes
- `unset` — functions, arrays, namerefs, call stack locals
- `let` — arithmetic evaluation with assignment
- `getopts` — complex variable + call stack interaction

### Execution Plans (Sub-Command Delegation)

Builtins cannot access the interpreter directly. When a builtin needs to run
other commands (e.g. `timeout`, `xargs`, `find -exec`), it returns a declarative
`ExecutionPlan` from `execution_plan()`. The interpreter checks this method
before `execute()` — when it returns `Some(plan)`, the interpreter fulfills the
plan instead of using the `execute()` result.

```rust
pub enum ExecutionPlan {
    Timeout { duration: Duration, preserve_status: bool, command: SubCommand },
    Batch { commands: Vec<SubCommand> },
}
```

**Current users:** `timeout` → Timeout, `xargs` → Batch, `find -exec` → Batch.

**Adding new execution plans:** Add a variant to `ExecutionPlan` and handle it
in the interpreter's plan fulfillment code (`interpreter/mod.rs`).

### Adding Internal Builtins

Simple builtins (zero-arg unit structs) are registered via the `register_builtins!`
macro in `interpreter/mod.rs`. To add a new one:

1. Create the builtin module in `crates/bashkit/src/builtins/` (implement `Builtin` trait)
2. Add `mod mycommand;` and `pub use mycommand::MyCommand;` in `builtins/mod.rs`
3. Add one line to the `register_builtins!` table in `interpreter/mod.rs`
4. Add spec tests in `tests/spec_cases/`
5. Update `specs/implementation-status.md`

### Network Builtins

`curl`, `wget`, `http` require the `http_client` feature + URL allowlist.
When `bot-auth` feature is enabled, all outbound HTTP requests are transparently
signed with Ed25519 per RFC 9421 (see `specs/request-signing.md`).

## Alternatives Considered

Inline within design sections above.
