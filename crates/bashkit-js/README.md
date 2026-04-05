# @everruns/bashkit

Sandboxed bash interpreter for JavaScript/TypeScript. Native NAPI-RS bindings to the [bashkit](https://github.com/everruns/bashkit) Rust core. Works with Node.js, Bun, and Deno.

## Install

```bash
npm install @everruns/bashkit   # Node.js
bun add @everruns/bashkit       # Bun
deno add npm:@everruns/bashkit  # Deno
```

## Features

- **Sandboxed execution** — all commands run in-process with a virtual filesystem, no containers needed
- **156 built-in commands** — echo, cat, grep, sed, awk, jq, curl, find, and more
- **Full bash syntax** — variables, pipelines, redirects, loops, functions, arrays
- **Resource limits** — protect against infinite loops and runaway scripts
- **Sync and async APIs** — `executeSync()` and `execute()` (Promise-based)
- **Virtual filesystem access** — read, write, mkdir, glob directly from JS
- **Cancellation** — `cancel()` and `AbortSignal` support
- **Scripted tool orchestration** — compose JS callbacks as bash builtins via `ScriptedTool`
- **LLM tool contract** — `BashTool` with discovery metadata, schemas, and system prompts

## Usage

```typescript
import { Bash, BashTool, ScriptedTool, getVersion } from '@everruns/bashkit';

// Basic usage
const bash = new Bash();
const result = bash.executeSync('echo "Hello, World!"');
console.log(result.stdout); // Hello, World!\n

// Async
const r = await bash.execute('echo "async!"');
console.log(r.stdout); // async!\n

// State persists between calls
bash.executeSync('X=42');
bash.executeSync('echo $X'); // stdout: 42\n

// With tool-contract metadata
const tool = new BashTool();
console.log(tool.name);           // "bashkit"
console.log(tool.inputSchema());  // JSON schema for LLM tool-use
console.log(tool.description());  // Token-efficient tool description
console.log(tool.help());         // Markdown help document
console.log(tool.systemPrompt()); // Compact system prompt

const tr = tool.executeSync('echo hello');
console.log(tr.stdout); // hello\n
```

### Virtual Filesystem

Read, write, and inspect files directly without executing bash commands:

```typescript
const bash = new Bash();
bash.writeFile('/data/config.json', '{"key": "value"}');
const content = bash.readFile('/data/config.json');
bash.mkdir('/data/subdir', true);   // recursive
bash.exists('/data/config.json');   // true
bash.remove('/data/subdir', true);  // recursive
bash.ls('/data');                   // string[]
bash.glob('**/*.json');             // string[]
```

### File Mounts

Mount files at construction time with strings, sync functions, or async functions:

```typescript
const bash = new Bash({
  files: {
    '/config.json': '{"key": "value"}',
    '/lazy.txt': () => 'computed on first read',
    '/async.txt': async () => fetchContent(),
  },
});

// For async file providers, use the static factory
const bash2 = await Bash.create({
  files: { '/data.txt': async () => loadData() },
});
```

### Cancellation

```typescript
const bash = new Bash();

// Cancel method
const promise = bash.execute('sleep 60');
bash.cancel();

// AbortSignal
const controller = new AbortController();
const promise2 = bash.execute('sleep 60', { signal: controller.signal });
controller.abort();
```

### Error Handling

```typescript
import { BashError } from '@everruns/bashkit';

// Throws BashError on non-zero exit
try {
  bash.executeSyncOrThrow('exit 1');
} catch (e) {
  if (e instanceof BashError) {
    console.log(e.exitCode); // 1
    console.log(e.stderr);
  }
}

// Async variant
await bash.executeOrThrow('false');
```

### ScriptedTool

Compose JS callbacks as bash builtins — an LLM writes a single bash script that pipes, loops, and branches across all registered tools:

```typescript
const tool = new ScriptedTool({ name: 'api' });
tool.addTool('get_user', 'Fetch user by ID', (params) => {
  return JSON.stringify({ id: params.id, name: 'Alice' });
});

const result = tool.executeSync("get_user --id 1 | jq -r '.name'");
console.log(result.stdout); // Alice
```

## API

### `Bash`

Core interpreter with virtual filesystem.

- `new Bash(options?)` — create instance
- `Bash.create(options?)` — async factory for async file providers
- `executeSync(commands)` — run bash commands, returns `ExecResult`
- `executeSyncOrThrow(commands)` — run, throws `BashError` on non-zero exit
- `execute(commands)` — async execution, returns `Promise<ExecResult>`
- `executeOrThrow(commands)` — async, throws `BashError` on non-zero exit
- `cancel()` — cancel running execution
- `reset()` — clear state, preserve config
- `readFile(path)` — read file as string
- `writeFile(path, content)` — write/overwrite file
- `mkdir(path, recursive?)` — create directory
- `exists(path)` — check path exists
- `remove(path, recursive?)` — delete file/directory
- `ls(path?)` — list directory contents
- `glob(pattern)` — find files by pattern

### `BashTool`

Interpreter + tool-contract metadata. All `Bash` methods, plus:

- `name` — tool name (`"bashkit"`)
- `version` — version string
- `shortDescription` — one-liner
- `description()` — token-efficient tool description
- `help()` — Markdown help document
- `systemPrompt()` — compact system prompt for LLM orchestration
- `inputSchema()` — JSON input schema
- `outputSchema()` — JSON output schema

### `ScriptedTool`

Multi-tool orchestration — register JS callbacks as bash builtins.

- `new ScriptedTool(options)` — create with name, shortDescription, limits
- `addTool(name, description, callback, schema?)` — register a tool
- `executeSync(script)` / `execute(script)` — run script
- `executeSyncOrThrow(script)` / `executeOrThrow(script)` — run, throw on error
- `env(key, value)` — set environment variable
- `toolCount()` — number of registered tools
- Tool metadata: `name`, `shortDescription`, `version`, `description()`, `help()`, `systemPrompt()`, `inputSchema()`, `outputSchema()`

### `BashOptions`

```typescript
interface BashOptions {
  username?: string;
  hostname?: string;
  maxCommands?: number;
  maxLoopIterations?: number;
  files?: Record<string, string | (() => string) | (() => Promise<string>)>;
  python?: boolean;
  externalFunctions?: string[];
}
```

### `ExecResult`

```typescript
interface ExecResult {
  stdout: string;
  stderr: string;
  exitCode: number;
  success: boolean;
  error?: string;
  stdoutTruncated?: boolean;
  stderrTruncated?: boolean;
}
```

### `BashError`

```typescript
class BashError extends Error {
  exitCode: number;
  stderr: string;
  display(): string;
}
```

### `getVersion()`

Returns the bashkit version string.

## Platform Support

| OS | Architecture |
|----|-------------|
| macOS | x86_64, aarch64 (Apple Silicon) |
| Linux | x86_64, aarch64 |
| Windows | x86_64 |
| WASM | wasm32-wasip1-threads |

## License

MIT
