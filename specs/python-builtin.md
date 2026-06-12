# Python Builtin (Monty)

> **Experimental.** Monty is an early-stage Python interpreter that may have
> undiscovered crash or security bugs. Resource limits are enforced by Monty's
> runtime. Do not rely on it for untrusted-input safety without additional
> hardening.

## Status
Implemented (experimental)

## Decision

Bashkit provides sandboxed Python execution via `python` and `python3` builtins,
powered by the [Monty](https://github.com/pydantic/monty) embedded Python
interpreter written in Rust.

### Feature Flag and Registration

The `python` cargo feature enables compilation; registration is opt-in via
the builder (matching the `git` pattern):

```rust
let bash = Bash::builder()
    .python()                                   // or .python_with_limits(PythonLimits::default()...)
    .env("BASHKIT_ALLOW_INPROCESS_PYTHON", "1") // runtime gate, required
    .build();
```

For security, execution is runtime-gated on `BASHKIT_ALLOW_INPROCESS_PYTHON=1`
(builder `.env(...)` or `export`).

### Why Monty

- Pure Rust, no CPython dependency
- Sub-microsecond startup
- Built-in resource limits (memory, allocations, time, recursion depth)
- No filesystem/network access by design (sandbox-safe)
- Snapshotable execution state

### Supported Usage

`python3 -c "code"` (REPL-like: last expression printed), `python3 script.py`
(from VFS), stdin (`echo "code" | python3`, `python3 - <<< ...`),
`--version`/`-V`. Shebang lines are stripped automatically.

### Resource Limits

Monty enforces its own limits independent of Bashkit's shell limits,
configurable via `PythonLimits`:

| Limit | Default | Builder Method |
|-------|---------|----------------|
| Max allocations | 1,000,000 | `.max_allocations(n)` |
| Max duration | 30 s | `.max_duration(d)` |
| Max memory | 64 MB | `.max_memory(bytes)` |
| Max recursion | 200 | `.max_recursion(depth)` |

Since Monty 0.0.4 the parser also enforces a nesting-depth limit (200
release / 35 debug) against stack overflow from deeply nested expressions.

### Python Feature Support

Monty implements a subset of Python 3.12. Supported: functions (incl.
defaults, `*args`/`**kwargs`, star unpacking, PEP 448), control flow,
exceptions, comprehensions/generator expressions, f-strings, core data
structures (list/dict/tuple/set/frozenset/namedtuple) with their operators
and views, `@property`, the common builtins (print, len, range, sorted,
isinstance, open, input, ...), and stdlib modules: sys, typing, math,
pathlib, os (getenv/environ), json, datetime (incl. `date.today()`,
`datetime.now(tz)`).

Not supported (Monty limitations): classes (planned upstream), match
statements, third-party imports, most stdlib modules, and HTTP/network I/O —
no `socket`/`urllib`/`requests`/`http.client`; Monty has no OsCall variants
for network operations, so there is no way to bridge these.

### VFS Bridging

Python `pathlib.Path` and `open()` operations are bridged to Bashkit's VFS
via Monty's OsCall pause/resume mechanism, so Python and bash share files:

```
Python code → Monty VM → OsCall(Open/ReadText, path) → Bashkit VFS → resume
```

Monty pauses at filesystem operations, yields an `OsCall` event with the
operation + arguments, Bashkit bridges it to the VFS, and resumes with the
result (or a Python exception).

Supported operations (one line): `open()`/`Path.open()` (read/write/append),
`Path.read_text/read_bytes/write_text/write_bytes/exists/is_file/is_dir/
is_symlink/mkdir/unlink/rmdir/iterdir/stat/rename/resolve/absolute`,
`os.getenv()`/`os.environ`, `datetime.date.today()`/`datetime.now(tz)`.

> **Note:** Monty 0.0.10+ has native filesystem mounting (`MountTable`,
> `MountDir`, `MountMode`) against host directories. Bashkit uses the OsCall
> bridge instead because our VFS is in-memory and may not be backed by host
> directories; the native mount system suits standalone real-filesystem use.

### External Functions

Host applications can register async external function handlers that Python
code calls by name — host capabilities (tool calls, lookups) without
serialization overhead; arguments arrive as raw `MontyObject` values.

```rust
let handler: PythonExternalFnHandler = Arc::new(|name, args, kwargs| {
    Box::pin(async move { ExtFunctionResult::Return(MontyObject::Int(42)) })
});
let bash = Bash::builder()
    .python_with_external_handler(PythonLimits::default(), vec!["get_answer".into()], handler)
    .build();
```

- Handler signature: `(function_name: String, positional_args: Vec<MontyObject>, keyword_args: Vec<(MontyObject, MontyObject)>) -> Pin<Box<dyn Future<Output = ExtFunctionResult> + Send>>`.
- Returns `ExtFunctionResult::Return(MontyObject)` (value to Python) or `ExtFunctionResult::Error(MontyException)` (raises).
- **Dispatch:** one handler receives all registered names; dispatch on `function_name` inside it.
- **Timeouts:** Each awaited handler call is wrapped in the remaining `PythonLimits::max_duration` wall-clock budget for the current Python invocation. If the budget expires while a handler is pending, Bashkit resumes Python with a `RuntimeError` instead of waiting for the handler indefinitely.
- **Trust model:** same as `BashBuilder::builtin()` and `ScriptedTool` callbacks — host registers trusted Rust code, untrusted scripts invoke by name. Handlers are trusted host code and should still enforce independent limits for outbound I/O, remote services, and other resources they consume.
- **Unstable re-exports:** `MontyObject`, `ExtFunctionResult`, `MontyException`, `ExcType` re-exported from the `monty` crate (git-pinned, not on crates.io); may break between bashkit releases.

### Security

See `specs/threat-model.md` § "Python / Monty Security (TM-PY)" for the full
analysis. Summary:

- **Code injection via bash expansion**: variables expand before reaching the builtin (by-design, consistent with all builtins); use single quotes to prevent.
- **Resource exhaustion**: Monty's allocation/time/memory caps apply even when shell limits are generous; print output is captured in memory and bounded by the memory cap.
- **Sandbox escape via filesystem**: all path ops go through the VFS; `/etc/passwd` reads VFS, not host. Relative paths resolve against the shell cwd; `../..` traversal constrained by VFS path normalization.
- **Sandbox escape via os/subprocess/socket**: not implemented in Monty; raise errors.

### Error Handling

Exit code 1: syntax/runtime errors (Python traceback on stderr; stdout
produced before a runtime error is preserved). Exit code 2: usage errors —
file not found, missing `-c` argument, unknown option.

### LLM Hints

Registration via `BashToolBuilder::python()` contributes a hint to `help()` /
`system_prompt()` documenting the limitations (stdlib subset, VFS-only file
I/O, no network/classes/third-party imports) through the general
`Builtin::llm_hint()` mechanism (hints deduplicated automatically).

The regex module `re` is intentionally disabled in Bashkit due to
catastrophic-backtracking DoS risk in untrusted code execution.

### Integration with Bashkit

`python`/`python3` map to the same builtin; works in pipelines (stdin
provides *code*, not data — matches real python's no-arg behavior), command
substitution, and conditionals.

## Verification

```bash
cargo test --features python --lib -- python
cargo test --features python --test spec_tests -- python
cargo test --features python --test threat_model_tests -- python
```
