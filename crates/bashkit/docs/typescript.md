# Embedded TypeScript (ZapCode)

> **Experimental.** ZapCode is an early-stage TypeScript interpreter that may
> have undiscovered crash or security bugs. Resource limits are enforced by
> ZapCode's VM. The integration should be treated as experimental.

Bashkit embeds the [ZapCode](https://github.com/TheUncharted/zapcode) TypeScript
interpreter, a pure-Rust implementation with ~2µs cold start, no V8 dependency,
and built-in sandboxing. TypeScript runs entirely in-memory with configurable
resource limits and no host access.

**See also:**
- [Threat Model](./threat-model.md) - Security considerations (TM-TS-*)
- [Custom Builtins](./custom_builtins.md) - Writing your own builtins
- [Compatibility Reference](./compatibility.md) - Bash feature support
- [`specs/016-zapcode-runtime.md`][spec] - Full specification

## Quick Start

Enable the `typescript` feature and register via builder:

```rust
use bashkit::Bash;

# #[tokio::main]
# async fn main() -> bashkit::Result<()> {
let mut bash = Bash::builder().typescript().build();

let result = bash.exec("ts -c \"console.log('hello from ZapCode')\"").await?;
assert_eq!(result.stdout, "hello from ZapCode\n");
# Ok(())
# }
```

## Usage Patterns

### Inline Code

```bash
ts -c "console.log(2 ** 10)"
# Output: 1024

# Node.js, Deno, and Bun aliases also work
node -e "console.log('hello')"
deno -e "console.log('hello')"
bun -e "console.log('hello')"
```

### Expression Evaluation

When no `console.log()` is called, the last expression is displayed (REPL behavior):

```bash
ts -c "1 + 2 * 3"
# Output: 7
```

### Script Files (from VFS)

```bash
cat > /tmp/script.ts << 'EOF'
const data = [1, 2, 3, 4, 5];
const sum = data.reduce((a, b) => a + b, 0);
console.log(`sum=${sum}, avg=${sum / data.length}`);
EOF
ts /tmp/script.ts
```

### Pipelines and Command Substitution

```bash
result=$(ts -c "console.log(42 * 3)")
echo "Result: $result"

echo "console.log('piped')" | ts
```

## Virtual Filesystem (VFS) Bridging

VFS operations are available as async global functions in the TypeScript
environment. Files created by bash are readable from TypeScript and vice versa.

### Bash → TypeScript

```bash
echo "important data" > /tmp/shared.txt
ts -c "await readFile('/tmp/shared.txt')"
# Output: important data
```

### TypeScript → Bash

```bash
ts -c "await writeFile('/tmp/result.txt', 'computed by ts\n')"
cat /tmp/result.txt
# Output: computed by ts
```

### Supported VFS Operations

| Operation | Function | Return |
|-----------|----------|--------|
| Read file | `readFile(path)` | `string` |
| Write file | `writeFile(path, content)` | `void` |
| Check exists | `exists(path)` | `boolean` |
| List directory | `readDir(path)` | `string[]` |
| Create directory | `mkdir(path)` | `void` |
| Delete | `remove(path)` | `void` |
| File metadata | `stat(path)` | JSON string |

### Architecture

```text
TS code → ZapCode VM → ExternalFn("readFile", [path]) → Bashkit VFS → resume
```

ZapCode suspends at external function calls, Bashkit bridges them to the VFS,
then resumes execution with the return value.

**Note:** `console.log()` output produced *after* a VFS call is not captured
due to a `zapcode-core` API limitation. Use the return-value pattern instead —
the last expression's value is printed automatically.

## Resource Limits

Default limits prevent runaway TypeScript code. Customize via `TypeScriptLimits`:

```rust,no_run
use bashkit::{Bash, TypeScriptLimits};
use std::time::Duration;

# fn main() {
let bash = Bash::builder()
    .typescript_with_limits(
        TypeScriptLimits::default()
            .max_duration(Duration::from_secs(5))
            .max_memory(16 * 1024 * 1024)   // 16 MB
            .max_allocations(100_000)
            .max_stack_depth(100)
    )
    .build();
# }
```

| Limit | Default | Purpose |
|-------|---------|---------|
| Duration | 30 seconds | Execution timeout |
| Memory | 64 MB | Heap memory cap |
| Stack depth | 512 | Call stack depth |
| Allocations | 1,000,000 | Heap allocation cap |

## Configuration

Use `TypeScriptConfig` for full control over aliases and hint behavior:

```rust,no_run
use bashkit::{Bash, TypeScriptConfig, TypeScriptLimits};
use std::time::Duration;

# fn main() {
// Default: ts, typescript, node, deno, bun + unsupported-mode hints
let bash = Bash::builder().typescript().build();

// Only ts/typescript commands, no node/deno/bun aliases
let bash = Bash::builder()
    .typescript_with_config(TypeScriptConfig::default().compat_aliases(false))
    .build();

// Disable unsupported-mode hints (plain errors only)
let bash = Bash::builder()
    .typescript_with_config(TypeScriptConfig::default().unsupported_mode_hint(false))
    .build();

// Custom limits + selective config
let bash = Bash::builder()
    .typescript_with_config(
        TypeScriptConfig::default()
            .limits(TypeScriptLimits::default().max_duration(Duration::from_secs(5)))
            .compat_aliases(false)
    )
    .build();
# }
```

### Unsupported Mode Hints

When enabled (default), using unsupported Node/Deno/Bun flags or subcommands
produces helpful guidance:

```text
$ node --inspect app.js
node: unsupported option or subcommand: --inspect
hint: This is an embedded TypeScript interpreter (ZapCode), not Node.js.
hint: Only inline execution is supported:
hint:   node -e "console.log('hello')"   # run inline code
hint:   node script.js                   # run file from VFS
hint:   echo "code" | node               # pipe code via stdin
```

## LLM Tool Integration

When using `BashTool` for AI agents, call `.typescript()` on the tool builder:

```rust,ignore
use bashkit::{BashTool, Tool};

let tool = BashTool::builder()
    .typescript()
    .build();

// help() and system_prompt() automatically document TypeScript limitations
let help = tool.help();
```

The builtin's `llm_hint()` is automatically included in the tool's documentation,
so LLMs know not to generate code using `import`, `eval()`, or HTTP.

## Limitations

**No `import`/`require`.** ZapCode has no module system. All code runs in a
single scope.

**No `eval()`/`Function()`.** Dynamic code generation is blocked at the
language level.

**No HTTP/network.** No `fetch`, `XMLHttpRequest`, or network APIs. ZapCode
has no network primitives.

**No `process`/`Deno`/`Bun` globals.** Runtime-specific APIs are not available.
Only standard TypeScript/JavaScript language features work.

**No npm packages.** Only built-in language features and registered external
functions are available.

**stdout after VFS calls.** `console.log()` output after an `await readFile()`
or similar VFS call is not captured. Use the return-value pattern: make the
last expression the value you want printed.

## Security

All TypeScript execution runs in a virtual environment:

- **No host filesystem access** — all paths resolve through the VFS
- **No network access** — no sockets, HTTP, or DNS
- **No dynamic code execution** — `eval()`, `Function()`, `import` blocked
- **Resource limited** — time, memory, stack depth, and allocation caps
- **Path traversal safe** — `../..` is resolved by VFS path normalization
- **Opt-in only** — requires both `typescript` feature AND `.typescript()` builder call

See threat IDs TM-TS-001 through TM-TS-023 in the [threat model](./threat-model.md).

[spec]: https://github.com/everruns/bashkit/blob/main/specs/016-zapcode-runtime.md
