# 005: Builtin Commands

## Status
Implemented

## Decision

Bashkit provides built-in commands for script execution in a virtual environment.
All builtins operate on the virtual filesystem. For the complete list of 160
builtins and per-command details, see `specs/009-implementation-status.md`.

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
```

### Shell State Access (ShellRef)

Internal builtins that need interpreter state receive it via `Context.shell`:

**Design rationale:**
- **Direct mutation** for aliases/traps — simple HashMaps with no invariants
- **Side effects** for arrays (budget checks), positional params (call stack),
  history (VFS persistence) — state with invariants the interpreter must enforce
- **Read-only methods** for introspection (functions, builtins, keywords,
  call stack, history, jobs) — builtins shouldn't mutate these
- `pub(crate)` keeps ShellRef out of the public API; custom builtins get `None`
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
5. Update `specs/009-implementation-status.md`

### Network Builtins

`curl`, `wget`, `http` require the `http_client` feature + URL allowlist.
When `bot-auth` feature is enabled, all outbound HTTP requests are transparently
signed with Ed25519 per RFC 9421 (see `specs/017-request-signing.md`).

## Alternatives Considered

Inline within design sections above.
