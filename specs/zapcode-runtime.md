# ZapCode TypeScript Runtime

> **Experimental.** ZapCode is an early-stage TypeScript interpreter. Resource
> limits are enforced by ZapCode's VM. Do not rely on it for untrusted-input
> safety without additional hardening.

## Status
Implemented (experimental)

## Decision

BashKit provides sandboxed TypeScript/JavaScript execution via `typescript`,
`ts`, `node`, `deno`, and `bun` builtins, powered by the
[ZapCode](https://github.com/TheUncharted/zapcode) embedded TypeScript
interpreter written in Rust.

### Feature Flag

Enable with:
```toml
[dependencies]
bashkit = { version = "0.1", features = ["typescript"] }
```

### Registration (Opt-in)

TypeScript builtins are **not** auto-registered. Enable via builder:

```rust
use bashkit::Bash;

// Default limits
let bash = Bash::builder().typescript().build();

// Custom limits
use bashkit::TypeScriptLimits;
use std::time::Duration;

let bash = Bash::builder()
    .typescript_with_limits(
        TypeScriptLimits::default()
            .max_duration(Duration::from_secs(5))
            .max_memory(16 * 1024 * 1024)
    )
    .build();
```

The `typescript` feature flag enables compilation; `.typescript()` on the builder
enables registration. This matches the `python` pattern
(`Bash::builder().python().build()`).

### Why ZapCode

- Pure Rust, no V8 or Node.js dependency
- Microsecond cold starts (~2 µs)
- Built-in resource limits (memory, time, stack depth)
- No filesystem/network/eval access by design (sandbox-safe)
- Snapshotable execution state (<2 KB)
- External function suspend/resume for VFS bridging
- Published on crates.io (`zapcode-core`)

### Supported Usage

```bash
# Inline code
ts -c "console.log('hello')"
node -e "console.log('hello')"

# Expression evaluation (last expression printed)
ts -c "1 + 2 * 3"

# Script file (from VFS)
ts script.ts
node script.js

# Stdin
echo "console.log('hello')" | ts
ts - <<< "console.log('hi')"

# Version
ts --version
node --version

# All aliases
typescript -c "console.log('hello')"
deno -e "console.log('hello')"
bun -e "console.log('hello')"
```

### Command Aliases

All aliases map to the same ZapCode interpreter:

| Command | Flag for inline code | Rationale |
|---------|---------------------|-----------|
| `ts` | `-c` | Short alias for TypeScript |
| `typescript` | `-c` | Full name |
| `node` | `-e` | Node.js compatibility |
| `deno` | `-e` | Deno compatibility (eval flag) |
| `bun` | `-e` | Bun compatibility (eval flag) |

The `-c` and `-e` flags are both accepted by all aliases for convenience.

### Resource Limits

ZapCode enforces its own resource limits independent of BashKit's shell limits.
All limits are configurable via `TypeScriptLimits`:

| Limit | Default | Builder Method | Purpose |
|-------|---------|----------------|---------|
| Max duration | 30 seconds | `.max_duration(d)` | Prevent infinite loops |
| Max memory | 64 MB | `.max_memory(bytes)` | Prevent memory exhaustion |
| Max stack depth | 512 | `.max_stack_depth(n)` | Prevent stack overflow |

```rust
use bashkit::TypeScriptLimits;
use std::time::Duration;

// Tighter limits for untrusted code
let limits = TypeScriptLimits::default()
    .max_duration(Duration::from_secs(5))
    .max_memory(16 * 1024 * 1024)  // 16 MB
    .max_stack_depth(100);
```

### TypeScript Feature Support

ZapCode implements a TypeScript/JavaScript subset:

**Supported:**
- Variables: let, const, var
- Arithmetic, comparison, logical operators
- String operations, template literals
- Arrays, objects, destructuring
- Functions, arrow functions, default parameters
- Async/await, Promises
- Array methods: map, reduce, filter, forEach, find, etc.
- Loops: for, for...of, for...in, while, do...while
- Conditionals: if/else, ternary, switch/case
- Type annotations (parsed but not enforced at runtime)
- Closures, generators

**Not supported (ZapCode limitations):**
- `import` / `require` (no module system)
- `eval()` / `Function()` constructor
- Filesystem access (use external functions)
- Network access (no fetch/XMLHttpRequest)
- `process`, `Deno`, `Bun` global objects
- DOM APIs
- Most Node.js/Deno/Bun standard library APIs

### VFS Bridging

TypeScript code can access BashKit's virtual filesystem through external
functions registered by the builtin. These functions are available as globals
in the TypeScript environment:

```bash
# Write from bash, read from TypeScript
echo "data" > /tmp/shared.txt
ts -c "const content = await readFile('/tmp/shared.txt'); console.log(content)"

# Write from TypeScript, read from bash
ts -c "await writeFile('/tmp/out.txt', 'hello\n')"
cat /tmp/out.txt

# Check file existence
ts -c "console.log(await exists('/tmp/shared.txt'))"

# List directory
ts -c "const entries = await readDir('/tmp'); console.log(entries)"
```

**Bridged operations:**
- `readFile(path: string): Promise<string>` — read text from VFS
- `writeFile(path: string, content: string): Promise<void>` — write to VFS
- `exists(path: string): Promise<boolean>` — check existence
- `readDir(path: string): Promise<string[]>` — list directory
- `mkdir(path: string): Promise<void>` — create directory
- `remove(path: string): Promise<void>` — delete file/directory
- `stat(path: string): Promise<{size: number, isFile: boolean, isDir: boolean}>` — metadata

**Architecture:**
```
TS code → ZapCode VM → ExternalFn("readFile", [path]) → BashKit VFS → resume
```

ZapCode suspends execution at external function calls, BashKit bridges the
call to the VFS, and resumes execution with the result.

**Limitation: stdout after VFS calls.** `ZapcodeSnapshot::resume()` returns
`VmState` but does not expose the VM's accumulated stdout. This means
`console.log()` output produced *after* a VFS call is not captured. Use the
return-value pattern instead — the last expression's value is printed:

```bash
# ✓ Works: return value pattern
ts -c "await readFile('/tmp/f.txt')"

# ✓ Works: console.log before VFS call
ts -c "console.log('loading...'); await readFile('/tmp/f.txt')"

# ✗ Lost output: console.log after VFS call
ts -c "const data = await readFile('/tmp/f.txt'); console.log(data)"
```

This is a `zapcode-core` API limitation. Upstream fix tracked.

### External Functions

Host applications can register custom external functions that TypeScript code
can call by name. This enables TypeScript scripts to invoke host-provided
capabilities (e.g., tool calls, API requests).

**Builder API:**

```rust
use bashkit::{Bash, TypeScriptLimits, TypeScriptExternalFnHandler};
use serde_json::Value;
use std::sync::Arc;

let handler: TypeScriptExternalFnHandler = Arc::new(|name, args| {
    Box::pin(async move {
        Ok(Value::Number(42.into()))
    })
});

let bash = Bash::builder()
    .typescript_with_external_handler(
        TypeScriptLimits::default(),
        vec!["getAnswer".into()],
        handler,
    )
    .build();
```

### Direct Integration

ZapCode runs directly in the host process via `zapcode-core`. No subprocess,
no IPC. Resource limits are enforced by ZapCode's own VM.

```rust
use bashkit::{Bash, TypeScriptLimits};

// Default limits
let bash = Bash::builder().typescript().build();

// Custom limits
let bash = Bash::builder()
    .typescript_with_limits(TypeScriptLimits::default().max_duration(Duration::from_secs(5)))
    .build();
```

### Security

See `specs/threat-model.md` section "TypeScript / ZapCode Security (TM-TS)"
for the full threat analysis.

#### Threat: Code injection via bash variable expansion
Bash variables are expanded before reaching the TypeScript builtin. This is
by-design consistent with all other builtins. Use single quotes to prevent
expansion: `ts -c 'console.log("hello")'`.

#### Threat: Resource exhaustion
ZapCode enforces independent resource limits. Even if BashKit's shell limits
are generous, TypeScript code cannot exceed ZapCode's time/memory/stack caps.

#### Threat: Sandbox escape via filesystem
TypeScript code has no direct filesystem access. VFS-bridged functions go
through BashKit's virtual filesystem. `/etc/passwd` reads from VFS, not host.

#### Threat: Sandbox escape via eval/import
ZapCode blocks `eval()`, `Function()`, `import`, and `require` at the language
level. These are not implemented in the interpreter.

#### Threat: Denial of service via large output
TypeScript console.log output is captured in memory. The memory limit on
ZapCode prevents unbounded output generation.

### Error Handling

- Syntax errors: Exit code 1, error message on stderr
- Runtime errors: Exit code 1, error on stderr, any stdout preserved
- File not found: Exit code 2, error on stderr
- Missing `-c`/`-e` argument: Exit code 2, error on stderr
- Unknown option: Exit code 2, error on stderr

### LLM Hints

When TypeScript is registered via `BashToolBuilder::typescript()`, the builtin
contributes a hint to `help()` and `system_prompt()`:

> ts/node/deno/bun: Embedded TypeScript (ZapCode). Supports ES2024 subset.
> File I/O via readFile()/writeFile() async functions. No npm/import/require.
> No HTTP/network. No eval().

### Integration with BashKit

- `ts`/`typescript`/`node`/`deno`/`bun` all map to the same builtin
- Works in pipelines: `echo "data" | ts -c "..."`
- Works in command substitution: `result=$(ts -c "console.log(42)")`
- Works in conditionals: `if ts -c "throw new Error()"; then ... else ... fi`
- Shebang lines (`#!/usr/bin/env ts`) are stripped automatically

## Verification

```bash
# Build with typescript feature
cargo build --features typescript

# Run unit tests
cargo test --features typescript --lib -- typescript

# Run spec tests
cargo test --features typescript --test spec_tests -- typescript
```
