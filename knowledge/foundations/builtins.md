---
type: Subsystem Design
title: Builtin Commands
description: Builtin command trait, execution planning, registration, and implementation conventions.
tags:
  - bashkit
  - builtins
---

# Builtin Commands

## Status
Implemented

## Decision

Bashkit provides built-in commands for script execution in a virtual environment.
All builtins operate on the virtual filesystem. For the complete list, see
the generated [`builtins.json`](../status/builtins.json); for known gaps, [Known Limitations](../operations/limitations.md).

### Standard Flags

All external-style builtins support `--help` and `--version` flags via the
`check_help_version()` helper in `builtins/mod.rs` (long flags only, short
flags `-h`/`-V` have different meanings in many tools). Tools where `-h`/`-V`
genuinely mean help/version handle them directly in `execute()`.

### GNU Option Parsing

Hand-written coreutils-style builtins parse options with
`builtins::arg_parser::gnu_getopt` (getopt_long semantics: bundles such as
`-ud@0`, attached or separate values, `--long=VAL`, unambiguous long
prefixes, `--`, optional argument permutation). It returns the options in
command-line order plus operands, and GNU-worded errors (`invalid option --
'z'`, `unrecognized option`, `requires an argument`) with the caller's exit
code. New or reworked builtins should use it rather than ad-hoc loops, so
option spelling matches GNU; builtins on a ported clap surface (see
[Coreutils Argument Port](../runtimes/coreutils-args-port.md)) keep clap.
Obsolete forms GNU still accepts (`head -5`, `fold -5`) are rewritten to the
modern option before getopt runs, never when they are an option's value.

### Navigation resource accounting

`cd` borrows its target and `CDPATH` rather than cloning them. It searches
colon-separated entries in order, retains one candidate at a time, and keeps
the direct-path fallback. Empty entries use the current directory without
printing; non-empty hits print the resolved directory.

Before constructing each candidate, `lease_path_workspace` reserves a checked,
conservative bound for joined paths, logical resolution, and normalization's
component vector from the shared live-intermediate budget (TM-DOS-096).
The lease drops on each failed candidate. Candidate length charges aggregate
work; every 64 candidates yield cooperatively, and filesystem awaits run under
the request's cancellation/deadline gate. The final failure diagnostic uses
the same reservation. `execution_budget_tests::cdpath_*` cover script and
environment inputs, memory/work exhaustion, lease release, early success, and
cancellation; `cd-builtin.test.sh` covers Bash semantics.

### Command Dispatch Order

functions → special commands → builtins → path execution → $PATH search →
`CommandResolver` → "command not found"

Scripts containing `/` are resolved against VFS. Commands without `/` are
searched in `$PATH` directories. A shebang naming a registered non-shell
builtin (`#!/usr/bin/env python3`, `#!/usr/bin/python3`, `#!/usr/bin/awk -f`)
runs that builtin with the script path as its argument, as the kernel execs an
interpreter (only the basename matters; `#!/path/NAME ARG` passes ARG as one
argument, `env -S` splits). Otherwise (`bash`/`sh`, unknown interpreters,
shell-only builtins, no shebang) the shebang is stripped and the content runs
as bash. Exit 127: not found; Exit 126: not executable or is a directory.

### Command Hash Resource Ownership

`CommandHash` owns leases from the current `ExecutionBudget` for every retained
name, path and entry's metadata (TM-DOS-129). Admission happens before copying
borrowed `hash -p` arguments or automatic PATH lookups. Every mutation takes the
interpreter-owned budget explicitly, including direct interpreter calls and
child execution; it does not depend on a prior host execution to initialize
hidden hash state. Deletion, replacement,
512-entry eviction, and `hash -r`/PATH assignment release the removed entries.
Copy-on-write forks clone `Arc<str>` payloads and leases; hit counters remain
local. Each host execution re-admits persistent entries to its fresh budget
before parsing or dispatch, preventing cross-execution accumulation. Failed
admission preserves prior entries and fails the request through the existing
shared budget error; earlier successful insertions may remain, as with other
sequential shell mutations. `memory_growth_security_tests` covers bounded
amplification and recovery; `introspect` unit tests cover lifecycle and sharing.

### Builtin Trait

`Builtin` trait (`execute(ctx)` + optional `execution_plan(ctx)`, default
`Ok(None)`) and `Context` (args, env, variables, cwd, fs, stdin,
feature-gated borrowed http/git clients, `pub(crate) shell: Option<ShellRef>`, None
for custom builtins, and public lease-backed `execution_extension::<T>()`): see
`crates/bashkit/src/builtins/mod.rs` / rustdoc.

### Clap-Backed Custom Builtins

Custom Rust builtins can implement `ClapBuiltin` instead of `Builtin` when
their arguments are better represented as a `#[derive(clap::Parser)]` struct
(see `builtins/mod.rs` / rustdoc for the trait and an example). `clap` is an
unconditional dependency of `bashkit` (also used by ported coreutils argument
surfaces, see [Coreutils Argument Port](../runtimes/coreutils-args-port.md)), so this trait is always
available. Bashkit parses `Context::args` through clap, passes parsed args
plus a mutable `BashkitContext` to the handler, maps `--help`/`--version` to
successful stdout results, and maps clap parse failures to stderr with clap's
exit code. Parse diagnostics are capped to 1 KB to preserve TM-INF-022 stderr
constraints.

Clap output is kept free of ANSI escapes, and **not** by relying on the
workspace's `default-features = false` clap pin. Cargo unifies features across a
build, so a consumer that links clap with default features turns the `color`
feature on for bashkit's clap too; `Command::color` does not exist as a method
without that feature, so calling `.color(ColorChoice::Never)` would break the
colourless build instead of fixing the coloured one. Two layers cover it:
clap's `Display for StyledStr` runs `anstream::adapter::strip_str` whenever
`color` is on, and `clap_error_to_exec_result` strips escapes again on our side
so the guarantee does not rest on clap's internals. Builtin output is usually an
LLM tool result, never a terminal.

### Extension Trait

Extensions bundle a related set of builtins so embedders can add one capability
to `BashBuilder` or `BashToolBuilder` instead of registering each command
manually: `Extension::builtins() -> Vec<(String, Box<dyn Builtin>)>`
(`builtins/mod.rs`).

Rules:

- `BashBuilder::extension(ext)` / `BashToolBuilder::extension(ext)` expand each
  returned builtin into the builder's custom builtin map/list
- For `BashBuilder`, later registrations with the same command name override
  earlier registrations, matching `BashBuilder::builtin`
- Extensions must construct fresh builtin values or use shared ownership
  internally; builders may call `builtins()` when configuring reusable tools

Current extension: `TypeScriptExtension` registers `ts`/`typescript` and, when
enabled by `TypeScriptConfig`, `node`/`deno`/`bun`.

### BuiltinRegistry, Host-Owned Mutable Builtins

`BashBuilder::builtin(name, ...)` and `Extension::builtins()` are both
*build-time* registration: the set of builtins is frozen once the `Bash`
instance is built. For embedders that need to register or remove builtins
*after* construction (FFI bindings, REPLs, plugin systems),
`BuiltinRegistry` provides a host-owned mutable registry consulted at
command-dispatch time. API (`insert`/`insert_trusted`/`remove`/`lookup`/`names`/`is_empty`):
see `builtins/mod.rs` / rustdoc.

Wired in via `BashBuilder::builtin_registry(registry)`. The handle is
`Clone`; clones share the same underlying storage, so the embedder keeps a
clone for runtime mutation while the builder takes another.

Command-resolution order (see `Interpreter::dispatch_command`):

1. Shell functions (defined in scripts)
2. POSIX special builtins (`exec`, `set`, `:`, `eval`, …)
3. **Host registry** (`BuiltinRegistry::lookup`)
4. Baked-in + builder-registered builtins
5. Script execution by path / `$PATH` search
6. `CommandResolver::resolve` (last chance, see below)

So registry entries can override baked-in commands (e.g. wrap `cat` with
tracing) but shell functions still win, matching standard bash
precedence. `command -v` / `command -V` / `command name args…` consult
the registry too.

Implementation notes:

- Storage is an `Arc<RwLock<HashMap<...>>>` of builtin plus access mode (std
  only, no extra deps). Lookup clones the entry out of the lock before execution.
- `insert` is execution-scoped. `insert_trusted` is the explicit escape hatch
  for trusted host integrations that intentionally retain the raw VFS across calls.
- `Interpreter::builtins` was migrated from `HashMap<String, Box<dyn Builtin>>`
  to `HashMap<String, Arc<dyn Builtin>>` so registered and host-registry
  paths share one execution helper (`execute_builtin_arc`).
- The registry is host-owned: not part of interpreter state, so
  `reset_transient_state` leaves it untouched and snapshots do not
  serialize it. Restoring from a snapshot requires re-attaching the
  registry handle.

### CommandResolver, Last-Chance Name Resolution

`BuiltinRegistry` answers "which names are registered" from a map fixed before
the name is known. `CommandResolver` is asked *about a specific name*, after
every other route has missed and immediately before the 127 path, so an
embedder bridging an open-ended command space (host executables, a remote tool
catalog) does not have to enumerate it up front.

```rust
pub trait CommandResolver: Send + Sync {
    fn resolve(&self, name: &str) -> Option<Arc<dyn Builtin>>;
}
```

Decision: resolvers return a `Builtin` rather than executing directly. The
resolved builtin runs through `execute_builtin_arc`, the same path as every
other builtin, so `before_tool` fires with the resolved name and can veto it,
`catch_unwind` contains panics from both resolution and execution, and
stdin/redirects behave identically. Resolver panic details are discarded and
the command returns a sanitized non-zero shell result. A bespoke execution path
would have to re-earn all of that.

Consequences to keep in mind:

- Resolver-provided names are **not enumerable**: they do not appear in
  `Bash::builtin_names()` or in `command not found` suggestions. Security
  implications are in `../integrations/script-analysis.md`.
- Being last, a resolver can never shadow a function, builtin, or `$PATH`
  script. Use `BashBuilder::builtin` to override one.
- `resolve` is called for every unresolved command, so it must be cheap;
  cache rather than probing the filesystem per call.
- Resolver-provided builtins receive the same execution-scoped VFS lease as
  builder and ordinary registry builtins.
- The `$PATH` search consumes the pipeline stdin, so the interpreter clones it
  for the resolver **only when a resolver is installed**: the common path does
  not pay for the clone.

### Execution Extensions

`Bash::exec_with_extensions()` and `Bash::exec_streaming_with_extensions()`
accept a typed, per-call extension bag. Builtins receive an
`ExecutionCapability<T>` via `ctx.execution_extension::<T>()` and access it
through `try_with`; retained handles fail with `ExecutionCapabilityError::Revoked`.

Use this for request-scoped data that is not shell state: tracing/request IDs,
auth or tenant context, host-language runtime sessions (Python/JS callback
bridges), metrics/audit sinks for one execution.

Rules:

- One lease covers extension values, scoped VFS handles, host-call brokers, and
  tool-callback context.
- Completion, timeout, or dropped execution futures revoke the lease before the
  interpreter restores its prior request state.
- Cleanup is idempotent and bounded; `ExecResult::capability_cleanup` and retained
  handles expose only a failure count, never poisoned-lock or host diagnostics.
- Borrowed HTTP/Git/SSH clients were already lifetime-scoped and remain borrowed;
  no parallel wrapper is added for facilities safe Rust cannot retain.
- Long-lived registrations may retain capability handles, but late access fails.

### Shell State Access (ShellRef)

Internal builtins that need interpreter state receive it via `Context.shell`:

**Design rationale:**
- **Direct mutation** for aliases/traps, simple HashMaps with no invariants
- **Side effects** for arrays (budget checks), positional params (call stack),
  history (VFS persistence), state with invariants the interpreter must enforce
- **Read-only methods** for introspection (functions, builtins, keywords,
  call stack, history, jobs), builtins shouldn't mutate these
- `pub(crate)` keeps ShellRef out of the public API; custom builtins use
  public `execution_extension()` instead of direct shell access
- No dynamic dispatch, concrete struct, not trait

**Builtins using ShellRef:**
- `type`, `which`, read-only: check builtin/function/keyword names
- `alias`, `unalias`, direct mutation of `shell.aliases`
- `trap`, direct mutation of `shell.traps`
- `caller`, read call stack depth/frame names
- `history`, read history entries, clear via `ClearHistory` side effect
- `wait`, read job table, set exit code via `SetLastExitCode` side effect
- `mapfile`/`readarray`, set arrays via `SetIndexedArray` side effect

**Builtins still in interpreter dispatch chain** (fundamentally need interpreter):
- `exec`, redirect management, VFS I/O
- `local`, call frame locals mutation
- `source`/`.`, `eval`, parse and execute in current context
- `bash`/`sh`, script execution
- `command`, dispatch to builtins/functions
- `declare`/`typeset`, arrays, assoc arrays, variable attributes
- `unset`, functions, arrays, namerefs, call stack locals
- `let`, arithmetic evaluation with assignment
- `getopts`, complex variable + call stack interaction
- bare `set` (no arguments), the same sorted, quoted listing as `declare`,
  which needs arrays; `set` with arguments stays a registered builtin

### Control flow and shell-state builtins

- `break`/`continue` act on the loops of the current function only: the
  interpreter keeps `loop_depth` (reset to 0 on a function call, `bash -c`
  and pipeline children; `source`/`eval` keep it), passed through `ShellRef`.
  Outside a loop they warn and return 0; the count is clamped to the depth.
- `return` is valid only inside a function or sourced file (`return_depth`);
  elsewhere it fails with status 2 and the script goes on. Without an
  argument it returns `$?`. `return` ends a sourced file with its status, and
  `break`/`continue`/`return`/`exit` in a `source`/`eval` body propagate to
  the caller's loop, function or shell.
- Trap handlers (`ERR`, `DEBUG`) run with the trapping command's `$?` and
  leave it unchanged. `trap` keys handlers canonically (`EXIT`, `INT`,
  `DEBUG`, `ERR`, `RETURN`), accepts `0`/`2`/`int`/`SIGINT`, rejects unknown
  specs (status 1) and lists in bash order (EXIT, signals by number, DEBUG,
  ERR, RETURN) with `sh_single_quote` quoting, as `alias` does.
- ERR fires in `execute_command`, after the failing command itself, for
  the commands bash fires it for: simple commands (function calls and
  `eval` included), multi-command pipelines, `( )`, `[[ ]]` and `(( ))`.
  Lists, groups, loops, `if`/`case` never fire it themselves; the failing
  command inside does. Whether it may fire is decided before the command
  runs (bash's `was_error_trap`), so a function that sets ERR does not fire
  it for its own call. Condition contexts (`if`/`while` tests, non-final
  `&&`/`||` elements, `!`) suppress it like errexit. `$LINENO` in any trap
  handler counts from the triggering line (`Interpreter::line_base`).
- ERR and RETURN scoping: without `set -E` a function body runs with ERR
  removed, and a subshell, `$( )` or pipeline stage keeps ERR listed for
  `trap -p` but dormant (`err_trap_dormant`, cleared when `trap` sets or
  resets ERR there). Without `set -T` a function body runs with RETURN
  removed. On return the caller's handler comes back only if the body left
  that trap unset (bash `trap_if_untrapped`), so a trap a function sets
  stays. RETURN runs when a function body or sourced file finishes, before
  locals are popped, keeping `$?` and the function's status.
- `hash` keeps no table (every lookup walks the virtual PATH): a bare `hash`
  reports `hash table empty`, `hash NAME` only checks that NAME resolves.
- `builtin NAME`, like `type`/`command -v`, treats a registered command that
  real bash runs from PATH (`cat`) as a file when the root filesystem
  provides it, so it is "not a shell builtin".
- `set -k` hoists unquoted `name=value` arguments into the command's
  temporary environment at execution time.
- `[[ a -eq b ]]` evaluates each operand as an arithmetic expression without
  a second `$` expansion; an invalid operand prints the arithmetic error and
  makes the test false. An invalid `=~` regex (including an unknown
  `[:class:]`) makes the deciding test return 2.

### Declaration builtins and variable scope

`declare`/`typeset`, `local`, `export` and `readonly` share one engine
(`interpreter/declare.rs`, `execute_declaration_builtin`); they differ only in
the option letters they accept (declare/local `aAfFgiIlnprtux`, export `fnp`,
readonly `aAfp`; others exit 2 with usage). Options are read only before the
first operand. `declare -p`/`set` listings use bash's quoting (`$'...'` for
control bytes) and print associative arrays in bash's hash-bucket order
(FNV-1 32-bit, `h & 1023`), so listings diff cleanly against real bash.

Locals use **shallow binding**, like bash: the live maps (`variables`,
`var_attrs`, `namerefs`, arrays, `env`) always hold the visible binding;
`local x` saves the caller's binding in the frame (`CallFrame::saved_vars`)
and function return restores it. Lookups never walk frames. `unset` of a
local belonging to a calling frame pops that local and uncovers the outer
value (bash's dynamic-scope unset); `declare -g` writes the outermost saved
binding when a local hides the global. Declared-but-unassigned variables carry
`VarAttrs::NOVALUE` (`declare x; declare -p x` prints `declare -- x`).

Compound operands of declaration builtins (`declare -A m=([k]=v)`) are kept
as a `WordPart::CompoundAssignment` so their elements expand as array
elements (no word splitting of the whole operand, keyed `[k]=v` split after
expansion), and script analysis walks the substitutions inside them.

Bash quirks emulated on purpose: `local` outside a function fails with
status 1 but still assigns its compound operands as globals; prefix
assignments (`x=v cmd`) ignore `-i/-l/-u`; assigning a readonly variable
without a command abandons the rest of the line (`r=3 cmd` reports and still
runs `cmd`); assigning through a circular nameref warns and abandons the
line.

`time` is deliberately absent from the builtin registry. Bash grammar makes it
a reserved-word wrapper around a complete pipeline, so the interpreter measures
the AST directly. This preserves groups, functions, pipeline status, redirects,
errexit, cancellation, and the shared request budget.

### Builtin variable assignment

Builtins that assign shell variables (`read`, `printf -v`, `getopts`)
return a `BuiltinSideEffect::SetVariable` instead of writing
`ctx.variables`, so the interpreter applies the same rules as an
assignment: namerefs, locals (shallow binding), `name[subscript]` targets,
`declare -i` arithmetic (an arithmetic error aborts the line with status 1)
and `-l`/`-u`. A readonly target fails the builtin with
`bash: NAME: readonly variable` and status 1 (TM-INJ-019); the remaining
`SetVariable` effects of that builtin are dropped, matching `read a r b`
leaving `b` alone. `printf` reads options like bash's getopt (`v:`):
`-v NAME`, `-vNAME`, `--`; a missing `-v` argument, missing format, invalid
option or invalid `-v` name exits 2 with the usage line.
A numeric conversion of a non-number (`%d abc`, `%d 12abc`, `%f x`)
prints the parsed prefix, reports `bash: printf: ARG: invalid number` and
exits 1; the vendored uucore formatter's `show_error!` messages are
collected per call (`format_support::collect_diagnostics`, capped) and
mapped to bash's wording.

### Execution Plans (Sub-Command Delegation)

Builtins cannot access the interpreter directly. When a builtin needs to run
other commands (e.g. `timeout`, `xargs`, `find -exec`), it returns a declarative
`ExecutionPlan` from `execution_plan()`. The interpreter checks this method
before `execute()`, when it returns `Some(plan)`, the interpreter fulfills the
plan instead of using the `execute()` result.

Variants: `Timeout { duration, preserve_status, command }`,
`Batch { commands }`, `BatchWithStatus`, `Env { command, clear, unset, set,
chdir }`, `Driver(Box<dyn PlanDriver>)` (`builtins/mod.rs`).

`Driver` is for builtins whose next command depends on the previous result.
The interpreter calls `PlanDriver::next(None)`, runs each `PlanStep::Run {
command, cwd }` (temporarily switching cwd when set), feeds the `ExecResult`
back through `next(Some(..))`, and stops at `PlanStep::Done`. The driver owns
everything it needs (`Arc<dyn FileSystem>`, cwd, budget capability) so it is
`'static` and outlives the borrowed `Context`.

Each `SubCommand` carries optional command-scoped `assignments`
(`VAR=value cmd ...`), which the interpreter applies as the inner command's
environment. `xargs --process-slot-var=VAR` uses this to expose a
per-invocation parallel-slot index.

**Current users:** `timeout` → Timeout, `xargs` → Batch, `find -exec` /
`-execdir` → Driver, `env CMD` → Env.

`find` (`builtins/find/`) parses GNU's full expression grammar into a tree
and evaluates it in a resumable walker. `-exec ... \;` suspends evaluation,
the interpreter runs the command, and evaluation of that entry restarts with
the cached exit status, so `-exec test ... \; -print` filters like GNU and
output stays in order. Effects (prints, exec output, `-delete` results) are
committed per entry once its expression completes; `-delete` runs once and is
cached across restarts. Without `-exec`, `execute()` drives the same walker.
Gaps are listed as L-FIND-001/002.

`env [-i] [-u NAME] [-C DIR] [NAME=V]... CMD` runs CMD in a child-like scope:
the interpreter snapshots shell state + exported env, applies `-i` (keeps only
`_`-prefixed internal variables), unsets, assignments and `-C`, runs CMD, then
restores everything, so neither the env edits nor anything CMD does leak back
(matches a real `env` exec). Shell-only builtins (`cd`, `exit`, `export`, ...)
give 127 like bash, since `env` can only exec programs; an unknown command gives
`env: 'NAME': No such file or directory`. Option errors exit 125 (GNU).

#### `xargs -P` / `--process-slot-var` (parallelism)

bashkit runs a single `Bash` interpreter sequentially, even background `&`
jobs run synchronously for deterministic output (see
[Parallel Execution](parallel-execution.md)). So `xargs -P N` / `--max-procs=N` does **not**
spawn N OS processes for wall-clock speedup. Instead it allocates N
round-robin *slots* and the commands still run in order, with the slot index
(0..N-1, `idx % N`) surfaced via `--process-slot-var`. This is the behaviour
sharding logic depends on (`worker $SLOT of $N`) and matches GNU's
`--process-slot-var` for the deterministic case (single slot ⇒ index always
0). `-P 0` means "as many as possible" (one slot per command).

**Adding new execution plans:** Add a variant to `ExecutionPlan` and handle it
in the interpreter's plan fulfillment code (`interpreter/mod.rs`).

### Process-Local Host-Call Suspension

`BashBuilder::host_call_builtin(name)` registers a command fulfilled by the
host through `Bash::start_execution`. The first
`ExecutionHandle::next_event()` starts the ordinary interpreter future and
waits until it completes or the builtin sends a `HostCallRequest`;
`resume(id, ExecResult)` resolves the one-shot response and lets the same
future continue. The bounded request channel applies backpressure, request IDs
prevent mismatched responses, ordinary `exec()` fails the builtin promptly, and
the normal execution timeout remains armed while a request is pending.

`spawn_execution` (`host_call.rs`) decides who drives that future. Native
targets hand it to the ambient async runtime and JS-backed wasm to
`spawn_local`, so the deadline can fire while the host is parked and no further
handle poll is needed. A non-JS wasm embedder has no executor at all
([Non-JS WebAssembly Embedding](../runtimes/non-js-wasm.md)), so
`spawn_execution` hands the future
back and `next_event` polls it inline; the deadline is then enforced on the
host's next poll. The API contract does not vary by target: a timed-out
execution drops its session and cannot be recovered with `into_bash`. The
inline path is covered by native unit tests in `host_call.rs`, since CI only
compiles the non-JS wasm target.

This mechanism intentionally does not change interpreter control flow into a
serializable state machine. The driver owns both a pinned Rust future and the
`Bash` instance; normal completion makes the session recoverable through
`into_bash`, while a timeout or dropping a suspended handle drops the session
so partially unwound state cannot be reused. Pending calls cannot be included in snapshots or resumed in
another process. Portable mid-execution resume would require explicit
continuation frames for shell control flow, pipelines, substitutions,
redirects, accumulated output, and budgets; see
[Snapshot History](snapshot-history.md).

### Errors: Failed Command vs Aborted Execution

A builtin reports an ordinary failure as `Ok(ExecResult::err(msg, code))`.
For bashkit's own (bundled) builtins, an `Err(Error::Execution(_))` or
`Err(Error::Regex(_))` is treated the same way: the dispatcher turns it into
exit 2 with the message on stderr (capped at 1 KB, TM-INF-022), and the
script keeps running. That matches bash, where `grep -E '('` or an awk syntax
error fails one command, not the script. Before this, such errors aborted the
whole `exec()`.

`Err` from a custom builtin (`BashBuilder::builtin`, a `BuiltinRegistry`
entry or a `CommandResolver`) still aborts execution: that is the public
contract custom builtins rely on. Resource limits, cancellation and I/O
errors abort for every builtin.

### Adding Internal Builtins

Simple builtins (zero-arg unit structs) are registered via the `register_builtins!`
macro in `interpreter/mod.rs`. To add a new one:

1. Create the builtin module in `crates/bashkit/src/builtins/` (implement `Builtin` trait)
2. Add `mod mycommand;` and `pub use mycommand::MyCommand;` in `builtins/mod.rs`
3. Add one line to the `register_builtins!` table in `interpreter/mod.rs`
4. Add spec tests in `tests/spec_cases/`
5. Run `just regen-builtins`; record any gaps in [Known Limitations](../operations/limitations.md)

### Structured Query Builtins

`jq` and `yq` are registered together by the `jq` Cargo feature. `jq` owns the
jaq evaluator, compatibility definitions, execution-budget accounting, deadline,
depth, and output controls. `yq` is a format boundary around that implementation:
it parses YAML/JSON into a JSON stream, calls `Jq::execute`, then serializes the
results. It must not add YAML-specific expression parsing or duplicate evaluator
logic.

The old `yaml get/keys/length/type` helper was replaced rather than retained as
a nonstandard compatibility surface. `yq -i` evaluates and serializes fully,
writes a sibling temporary VFS file, and renames it over the source only after
all earlier stages succeed. See [Known Limitations](../operations/limitations.md)
for deliberate mikefarah/yq gaps and [Threat Model](../security/threat-model.md)
for input/output bounds.

### make

`make` is a GNU make 4.3 subset implemented as an `ExecutionPlan::Driver`
(`builtins/make/`). It parses the makefile (`parse.rs`: assignments of every
flavor, conditionals, `define`, `include`/`-include`, explicit, pattern,
static-pattern and old-style suffix rules, target-specific variables, special
targets `.PHONY`/`.SILENT`/`.IGNORE`/`.ONESHELL`/`.EXPORT_ALL_VARIABLES`/
`.DEFAULT_GOAL`), expands variables and functions (`expand.rs`, text, file-name,
conditional, `foreach`, `call`, `origin`, `flavor`, `error`/`warning`/`info`,
`wildcard`, `shell`), then walks the dependency graph comparing VFS mtimes.

Each recipe line goes back to the interpreter as `sh -c LINE` with exported
variables as assignments, so recipes see the sandbox shell, its budget and its
limits; no host process exists. `$(shell)` uses `PlanStep::Capture` (output
captured, never streamed) and make's own messages use `PlanStep::Emit`, so they
interleave with recipe output in order. Expansion is synchronous: a `$(shell)`,
`$(wildcard)` or `include` miss stops expansion, the driver answers the query
and parsing restarts with the answer cached. Messages and exit codes follow GNU
(`*** No rule to make target`, `Error N`, `-k`, `-q` exit 1, `-n`).

Deliberate gaps are L-MAKE-001..003 in
[Known Limitations](../operations/limitations.md); amplification caps are
TM-DOS-126 in the [Threat Model](../security/threat-model.md).

### awk

`awk` (also `gawk`, `mawk`, `nawk`) targets gawk 5.2, Debian's `awk`
(`builtins/awk/`): its options (`-F`, `-v`, `-f`, `-e`, `--csv`, ignored gawk
flags, usage on stderr with exit 1), its messages (`awk: cmd. line:1:` with a
caret for syntax errors, located runtime `fatal:` errors), its number output
and the extensions agents use (`gensub`, `asort`/`asorti`, `patsplit`,
`strftime`/`mktime`/`systime`, `BEGINFILE`/`ENDFILE`, `switch`, arrays of
arrays, `PROCINFO["sorted_in"]`, `FIELDWIDTHS`/`FPAT`, `IGNORECASE`, `RS` as a
regex, `RT`). `rand()` reproduces gawk's random() sequence. Debian-oracle
differential score: 94% of 260 awk cases (51% before the rewrite); every
gawk case passes, the misses expect `mawk`'s own dialect. Unsorted
`for (k in a)` follows gawk's storage (`order`): a non-negative integer
first key makes an integer array (other keys listed first, then integers
ascending), otherwise a chained hash (sdbm for strings) listed bucket by
bucket, newest first.

Pipeline: `lexer` -> `parser` (names resolved to slots, AST in `ast`) ->
`interp` (async evaluator) with `io` (records, `getline`, redirections,
commands) and `funcs` (builtin functions). `regex` translates EREs to the
`regex` crate and adds POSIX leftmost-longest matching with a hybrid DFA
when an alternation or quantifier could prefer a shorter match. Arrays live
in an arena referenced by id, which gives by-reference array arguments.
Each expression or statement kind is its own boxed future, so one awk call
level costs a few KiB of stack (the 64-level call cap fits a 2 MiB debug
thread with room to spare).

Commands (`system()`, `print | cmd`, `cmd | getline`) run as `sh -c` through
an `ExecutionPlan::Driver`, used only when the arguments contain `|` or
`system` or a `-f` program; awk output is streamed with `PlanStep::Emit`
before each command so order is kept. Without a driver (direct `execute`),
commands report 127. Gaps are L-AWK-001..004 in
[Known Limitations](../operations/limitations.md); caps are TM-DOS-027,
-028, -033, -109, -110, -116 and -128 in the
[Threat Model](../security/threat-model.md).

### grep

`grep` targets GNU grep 3.11 in a UTF-8 locale (Debian's grep). Debian-oracle
differential score: 96.8% of 185 grep cases (60.5% before) and every grep
probe of the 879 BRE/ERE regex cases except two whose pattern holds a raw
`\x01` byte the shell drops; the misses are readdir order (L-GREP-003).

- Patterns (`builtins/grep_pattern.rs`): `-G`/`-E` are translated from the
  GNU BRE/ERE dialects (POSIX brackets with literal backslash, `\{m,n\}`,
  `\+ \? \|`, `\< \> \b \B \w \W \s \S`, a leading BRE `*` as a literal, a
  leading ERE operator dropped with dfa.c's `* at start of expression`
  warning) and matched leftmost-longest through awk's `AwkRegex`. Syntax
  errors print glibc's messages (`Unmatched ( or \(`, `Invalid range end`,
  ...) with exit 2. Back-references (BRE and ERE) and `-P` use fancy-regex.
  `-F` is escaped literals on the POSIX path. `-e`/`-f` entries split on
  newlines; an empty `-f` file is zero patterns (matches nothing).
- Output (`FileScan` in `builtins/grep.rs`) follows GNU `prtext`/`prline`:
  prefixes FILE, LINE, BYTE, then `-T`'s tab; `:` for selected and `-` for
  context lines; group separators whenever a context option was given, also
  between files; `-o` prints non-empty matches (context lines only under
  `-v`), `-b -o` is the match offset; trailing context after `-m` is printed
  in full. One engine serves buffered and streaming (pipeline) runs.
- Input: lines end at `\n` (a `\r` is data). A NUL makes a file binary (no
  line output, first match ends it); a line that is not UTF-8 is binary when
  it would be printed. Both report `grep: FILE: binary file matches` on
  stderr unless `-a`.
- Options parse like getopt_long: mixed with operands, unique long-option
  prefixes, `-NUM`, `--label`, `-` for stdin, `-d read|skip|recurse`, `-R`
  following symlinks met while recursing (`-r` skips them).
- Recursive traversal compares canonical paths only against the current ancestor
  chain, using the VFS resolver shared with `find`. Cycles produce GNU's
  `warning: recursive directory loop` and are skipped; separate sibling aliases
  still search the same target under each display name. Diagnostics use the shared
  errno formatter and a UTF-8-safe 1 KiB cap. Traversal and ancestor
  comparisons consume shared work (checking deadline/cancellation), pending paths
  and listings carry live-byte leases, and files are admitted by metadata size
  before reading and scanned one at a time. Indexed search retains admitted paths
  instead of contents. `-q` stops on the first matching file. Regression coverage:
  `grep_recursive_security_tests` in the consolidated integration binary.

Gaps are L-GREP-001..003 in [Known Limitations](../operations/limitations.md);
regex caps are TM-DOS-023/025 in the [Threat Model](../security/threat-model.md).

### fmt

`fmt` vendors the uutils/coreutils `fmt` engine (MIT): `builtins/fmt/
linebreak.rs` (Knuth-Plass optimal fit) and `parasplit.rs` (paragraphs,
prefixes, crown/tagged indents), pinned to the same uutils revision as the
generated argument files. Bashkit owns only `fmt/mod.rs`: option parsing and
I/O. Options are GNU's (`-w`, `-g`, `-c`, `-t`, `-s`, `-u`, `-p`, obsolete
`-WIDTH`) plus the uutils extras (`-m`, `-P`, `-x`, `-X`, `-q`, `-T`). Width
rules and error messages follow GNU: goal is 93% of the width, `-g` alone sets
the width to goal + 10, width is capped at 2500, goal at the width. Vendored
`panic!`/`unwrap` paths were turned into early returns. GNU `fmt.c` is GPL and
must not be ported; line-break differences are L-FMT-001.

### Network Builtins

`curl`, `wget`, `http` require the `http_client` feature + URL allowlist.
When `bot-auth` feature is enabled, all outbound HTTP requests are transparently
signed with Ed25519 per RFC 9421 (see [Request Signing](../security/request-signing.md)).

### pr

`pr` (`builtins/pr.rs`) is written from GNU `pr`'s observable behavior, not
its source (GPL). Output matches GNU byte for byte on an 88-case differential
corpus: page layout (5-line header with date, centered title and page number,
body, 5-line trailer, 66 lines), `-COLUMN` down/across with last-page
balancing, `-m`, `-n`, `-d`, `-o`, `-s`/`-S`/`-J`, `-l`/`-w`/`-W`, `+FIRST:LAST`
and the error messages. Multi-column padding uses GNU's tab rule: padding and
blank runs of two or more become tabs where they reach a tab stop. Header
dates come from the sandbox clock and `TZ` (the `date` builtin's), file dates
from VFS mtimes. Output is capped at 16 MiB because padding can multiply
input size. Gaps: L-PR-001.

### Archive Compression

`tar` supports plain, gzip, and bzip2 archives. It accepts GNU short/old-style
bundles (`-cjf`, `cjf`) and `--bzip2`, and detects gzip/bzip2 magic while listing
or extracting so `.tar.bz2`/`.tbz2` inputs work without an explicit codec flag.
`bzip2`, `bunzip2`, and `bzcat` share the same byte-native codec path.

The implementation uses `bzip2` 0.6 with its default pure-Rust
`libbz2-rs-sys` backend. The crate is maintained by the Trifecta Tech
Foundation, dual MIT/Apache-2.0; its backend retains the permissive
`bzip2-1.0.6` license. Both are admitted by `deny.toml`. The release supports
the repository's Rust floor and WASM targets and is newer than the 0.4.4 fix
for RUSTSEC-2023-0004. Cargo-vet exemptions are limited to these versions after
reviewing the wrapper's FFI slice bounds and stream lifecycle plus the enabled
backend's no-stdio Rust-allocator path, pointer bounds, allocation lifecycle,
and CRC failure path. Decoder output is checked and request-memory charged
before buffer growth; bzip2 stream/CRC errors fail closed. Filesystem quotas,
expansion ratio, aggregate input/work, live-memory leases, and extraction path
validation remain layered controls.

See [Threat Model](../security/threat-model.md) for TM-DOS-007/008/096/102 and
TM-INJ-010.
