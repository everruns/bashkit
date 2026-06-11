# ZapCode TypeScript Runtime

> **Experimental.** ZapCode is an early-stage TypeScript interpreter. Resource
> limits are enforced by ZapCode's VM. Do not rely on it for untrusted-input
> safety without additional hardening.

## Decision

Sandboxed TypeScript/JavaScript execution via `typescript`, `ts`, `node`,
`deno`, and `bun` builtins, powered by the
[ZapCode](https://github.com/TheUncharted/zapcode) embedded TypeScript
interpreter written in Rust (`zapcode-core` on crates.io). Runs directly in
the host process — no subprocess, no IPC.

### Registration (Opt-in)

The `typescript` cargo feature enables compilation; registration is explicit,
matching the `python` pattern: `Bash::builder().typescript()`,
`.typescript_with_limits(limits)`, or `.extension(TypeScriptExtension::default())`
(works on both `BashBuilder` and `BashToolBuilder`). TypeScript builtins are
**not** auto-registered.

### Why ZapCode

- Pure Rust, no V8 or Node.js dependency
- Microsecond cold starts (~2 µs)
- Built-in resource limits (memory, time, stack depth)
- No filesystem/network/eval access by design (sandbox-safe)
- Snapshotable execution state (<2 KB)
- External function suspend/resume for VFS bridging
- Published on crates.io (`zapcode-core`)

### Usage

Inline code (`ts -c "..."` / `node -e "..."`; `-c` and `-e` both accepted by
all aliases), expression evaluation (last expression printed), script file
from VFS, stdin (`ts -`), `--version`. All aliases (`ts`, `typescript`,
`node`, `deno`, `bun`) map to the same ZapCode builtin. Shebang lines
(`#!/usr/bin/env ts`) stripped automatically. Works in pipelines, command
substitution, and conditionals like any builtin.

### Resource Limits

ZapCode enforces its own resource limits independent of Bashkit's shell limits.
Configurable via `TypeScriptLimits`:

| Limit | Default | Builder Method | Purpose |
|-------|---------|----------------|---------|
| Max duration | 30 seconds | `.max_duration(d)` | Prevent infinite loops |
| Max memory | 64 MB | `.max_memory(bytes)` | Prevent memory exhaustion |
| Max stack depth | 512 | `.max_stack_depth(n)` | Prevent stack overflow |

### Language Support

ZapCode implements a TypeScript/JavaScript (ES2024) subset — see ZapCode docs
for details. Type annotations parsed but not enforced. Not supported by
design: `import`/`require` (no module system), `eval()`/`Function()`,
filesystem access (use bridged functions), network access,
`process`/`Deno`/`Bun` globals, DOM APIs, most Node/Deno/Bun stdlib.

### VFS Bridging

TypeScript code accesses Bashkit's VFS through external functions registered
by the builtin, available as globals:

- `readFile(path: string): Promise<string>`
- `writeFile(path: string, content: string): Promise<void>`
- `exists(path: string): Promise<boolean>`
- `readDir(path: string): Promise<string[]>`
- `mkdir(path: string): Promise<void>`
- `remove(path: string): Promise<void>`
- `stat(path: string): Promise<{size, isFile, isDir}>`

Architecture: ZapCode suspends execution at external function calls
(`TS code → ZapCode VM → ExternalFn → Bashkit VFS → resume`), Bashkit bridges
the call to the VFS, and resumes with the result.

**Limitation: stdout after VFS calls.** `ZapcodeSnapshot::resume()` returns
`VmState` but does not expose the VM's accumulated stdout, so `console.log()`
output produced *after* a VFS call is lost. Use the return-value pattern
instead (last expression's value is printed); `console.log` *before* the VFS
call works. `zapcode-core` API limitation; upstream fix tracked.

### External Functions

Hosts can register custom external functions callable by name from TypeScript
(tool calls, API requests, etc.) via
`Bash::builder().typescript_with_external_handler(limits, names, handler)`
where `handler: TypeScriptExternalFnHandler` is an
`Arc` async closure `(name, args) -> Result<serde_json::Value>`. See rustdoc.

### Security

See `specs/threat-model.md` section "TypeScript / ZapCode Security (TM-TS)"
for the full threat analysis.

- **Code injection via bash variable expansion**: variables expand before
  reaching the builtin — by-design consistent with all builtins. Use single
  quotes to prevent expansion.
- **Resource exhaustion**: ZapCode enforces independent time/memory/stack caps
  even if shell limits are generous.
- **Sandbox escape via filesystem**: no direct FS access; bridged functions go
  through the VFS (`/etc/passwd` reads VFS, not host).
- **Sandbox escape via eval/import**: `eval()`, `Function()`, `import`,
  `require` blocked at the language level (not implemented).
- **DoS via large output**: console.log output captured in memory; ZapCode
  memory limit bounds it.

### Error Handling

- Syntax errors: Exit code 1, error message on stderr
- Runtime errors: Exit code 1, error on stderr, any stdout preserved
- File not found: Exit code 2, error on stderr
- Missing `-c`/`-e` argument: Exit code 2, error on stderr
- Unknown option: Exit code 2, error on stderr

### LLM Hints

When registered via `BashToolBuilder::typescript()`, the builtin contributes
a hint to `help()` and `system_prompt()`:

> ts/node/deno/bun: Embedded TypeScript (ZapCode). Supports ES2024 subset.
> File I/O via readFile()/writeFile() async functions. No npm/import/require.
> No HTTP/network. No eval().
