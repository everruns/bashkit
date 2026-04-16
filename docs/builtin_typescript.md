# TypeScript in Bashkit

Bashkit includes an embedded TypeScript interpreter powered by
[ZapCode](https://github.com/TheUncharted/zapcode), a pure-Rust TypeScript
runtime with ~2µs cold start and zero V8 dependency. TypeScript runs entirely
in-memory alongside bash — files written by bash are readable from TypeScript
and vice versa.

## Getting started

Add the `typescript` feature to your `Cargo.toml`:

```toml
[dependencies]
bashkit = { version = "0.1", features = ["typescript"] }
```

Enable TypeScript in the builder:

```rust
use bashkit::Bash;

let mut bash = Bash::builder().typescript().build();

// Run TypeScript inline
let r = bash.exec("ts -c \"console.log('hello')\"").await?;
assert_eq!(r.stdout, "hello\n");
```

## Command aliases

Five commands are registered, all running the same ZapCode interpreter:

| Command | Inline flag | Example |
|---------|-------------|---------|
| `ts` | `-c` | `ts -c "console.log('hi')"` |
| `typescript` | `-c` | `typescript -c "1 + 2"` |
| `node` | `-e` | `node -e "console.log('hi')"` |
| `deno` | `-e` | `deno -e "console.log('hi')"` |
| `bun` | `-e` | `bun -e "console.log('hi')"` |

Both `-c` and `-e` are accepted by all aliases.

## Running TypeScript files

Write a `.ts` file to the VFS and execute it:

```bash
cat > /tmp/hello.ts << 'EOF'
const greet = (name: string): string => `Hello, ${name}!`;
console.log(greet("world"));
EOF
ts /tmp/hello.ts
```

Output: `Hello, world!`

Scripts can also be piped via stdin:

```bash
echo "console.log(2 ** 10)" | ts
```

## Working with the virtual filesystem

TypeScript code can read and write files through async VFS functions that are
automatically available as globals:

```bash
# Write from bash
echo "important data" > /tmp/config.txt

# Read from TypeScript
ts -c "await readFile('/tmp/config.txt')"
# Output: important data

# Write from TypeScript
ts -c "await writeFile('/tmp/output.txt', 'computed result\n')"

# Read from bash
cat /tmp/output.txt
# Output: computed result
```

### Available VFS functions

| Function | Description |
|----------|-------------|
| `readFile(path)` | Read file contents as string |
| `writeFile(path, content)` | Write string to file |
| `exists(path)` | Check if path exists (returns boolean) |
| `readDir(path)` | List directory entries (returns string[]) |
| `mkdir(path)` | Create directory (recursive) |
| `remove(path)` | Delete file or directory |
| `stat(path)` | Get file metadata (returns JSON string) |

## Example: data processing pipeline

Bash and TypeScript can work together in a single session, each using their
strengths:

```bash
# Step 1: Bash generates raw data
for i in $(seq 1 5); do
    echo "$i,$((i * 10)),$((RANDOM % 100))" >> /tmp/data.csv
done

# Step 2: TypeScript processes it
ts -c "
const csv = await readFile('/tmp/data.csv');
const rows = csv.trim().split('\n').map(r => r.split(',').map(Number));
const total = rows.reduce((sum, [_id, value, _score]) => sum + value, 0);
await writeFile('/tmp/summary.txt', 'Total: ' + total + '\n');
"

# Step 3: Bash uses the result
cat /tmp/summary.txt
# Output: Total: 150
```

## Example: JSON transformation

```bash
# Create JSON with bash
cat > /tmp/users.json << 'EOF'
[
  {"name": "Alice", "age": 30},
  {"name": "Bob", "age": 25},
  {"name": "Charlie", "age": 35}
]
EOF

# Transform with TypeScript
ts -c "
const data = JSON.parse(await readFile('/tmp/users.json'));
const names = data.map(u => u.name).join(', ');
console.log('Users: ' + names);
console.log('Average age: ' + (data.reduce((s, u) => s + u.age, 0) / data.length));
"
```

## Example: script file with VFS

```bash
# Write a TypeScript utility script
cat > /tmp/analyze.ts << 'TSEOF'
const content = await readFile('/tmp/numbers.txt');
const nums = content.trim().split('\n').map(Number);
const sum = nums.reduce((a, b) => a + b, 0);
const avg = sum / nums.length;
console.log('Count: ' + nums.length);
console.log('Sum:   ' + sum);
console.log('Avg:   ' + avg.toFixed(2));
console.log('Min:   ' + Math.min(...nums));
console.log('Max:   ' + Math.max(...nums));
TSEOF

# Generate data and run the script
seq 1 10 > /tmp/numbers.txt
ts /tmp/analyze.ts
```

## Configuration

### Resource limits

```rust
use bashkit::{Bash, TypeScriptLimits};
use std::time::Duration;

let bash = Bash::builder()
    .typescript_with_limits(
        TypeScriptLimits::default()
            .max_duration(Duration::from_secs(5))
            .max_memory(16 * 1024 * 1024)  // 16 MB
            .max_stack_depth(100)
            .max_allocations(100_000)
    )
    .build();
```

### Disabling compat aliases

If you only want `ts`/`typescript` and not `node`/`deno`/`bun`:

```rust
use bashkit::{Bash, TypeScriptConfig};

let bash = Bash::builder()
    .typescript_with_config(TypeScriptConfig::default().compat_aliases(false))
    .build();

// ts works:
bash.exec("ts -c \"console.log('ok')\"").await?;

// node does NOT work:
let r = bash.exec("node -e \"console.log('ok')\"").await?;
assert_ne!(r.exit_code, 0);
```

### Disabling unsupported-mode hints

By default, using Node/Deno/Bun-specific flags shows helpful guidance. Disable
this for cleaner error output:

```rust
use bashkit::{Bash, TypeScriptConfig};

let bash = Bash::builder()
    .typescript_with_config(TypeScriptConfig::default().unsupported_mode_hint(false))
    .build();
```

## Supported TypeScript features

| Feature | Example |
|---------|---------|
| Variables | `let x = 10; const y = 20;` |
| Arrow functions | `const add = (a: number, b: number) => a + b;` |
| Template literals | `` `hello ${name}` `` |
| Destructuring | `const { x, y } = obj; const [a, ...rest] = arr;` |
| Async/await | `const data = await readFile('/tmp/f.txt');` |
| Array methods | `.map()`, `.filter()`, `.reduce()`, `.forEach()`, `.find()` |
| For loops | `for`, `for...of`, `for...in`, `while`, `do...while` |
| Conditionals | `if/else`, ternary, `switch/case` |
| Type annotations | Parsed and accepted but not enforced at runtime |
| Math | `Math.floor()`, `Math.min()`, `Math.max()`, `Math.round()` etc. |
| JSON | `JSON.parse()`, `JSON.stringify()` |
| Closures | Full lexical scoping with closure capture |
| Generators | `function*` with `yield` |

## What's NOT supported

| Feature | Reason |
|---------|--------|
| `import`/`require` | No module system |
| `eval()`/`Function()` | Blocked for security |
| `fetch`/`XMLHttpRequest` | No network access |
| `process`/`Deno`/`Bun` globals | No runtime APIs |
| npm packages | No package manager |
| DOM APIs | No browser environment |

## Security

TypeScript execution is fully sandboxed:

- All file I/O goes through the virtual filesystem (no host access)
- No network, no process spawning, no dynamic code evaluation
- Independent resource limits (time, memory, stack, allocations)
- Opt-in only: requires both `typescript` Cargo feature AND `.typescript()` builder call
- Path traversal (`../../../etc/passwd`) is blocked by VFS normalization

See [TM-TS threat entries](../specs/threat-model.md) for the full security analysis.
