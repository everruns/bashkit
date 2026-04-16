# @everruns/bashkit

Sandboxed bash interpreter for JavaScript and TypeScript. Native NAPI-RS bindings to the `bashkit` Rust core for Node.js, Bun, and Deno.

## Features

- Sandboxed, in-process execution with a virtual filesystem
- Full bash syntax: variables, pipelines, redirects, loops, functions, and arrays
- 160 built-in commands including `grep`, `sed`, `awk`, `jq`, `curl`, and `find`
- Sync and async execution APIs
- Direct VFS helpers, constructor mounts, and live host mounts
- Cancellation support via `cancel()`
- Sticky cancellation recovery via `clearCancel()`
- Snapshot and restore support on `Bash`
- AI framework adapters for OpenAI, Anthropic, Vercel AI SDK, and LangChain

## Install

```bash
npm install @everruns/bashkit   # Node.js
bun add @everruns/bashkit       # Bun
deno add npm:@everruns/bashkit  # Deno
```

## Quick Start

### Sync Execution

```typescript
import { Bash } from "@everruns/bashkit";

const bash = new Bash();

const result = bash.executeSync('echo "Hello, World!"');
console.log(result.stdout); // Hello, World!\n

bash.executeSync("X=42");
console.log(bash.executeSync("echo $X").stdout); // 42\n
```

### Async Execution

```typescript
import { Bash } from "@everruns/bashkit";

const bash = new Bash();

const result = await bash.execute('echo -e "banana\\napple\\ncherry" | sort');
console.log(result.stdout); // apple\nbanana\ncherry\n

await bash.execute('printf "data\\n" > /tmp/file.txt');
console.log((await bash.execute("cat /tmp/file.txt")).stdout); // data\n
```

### Live Output

```typescript
const bash = new Bash();

const result = await bash.execute(
  'for i in 1 2 3; do echo out-$i; echo err-$i >&2; done',
  {
    onOutput({ stdout, stderr }) {
      if (stdout) process.stdout.write(stdout);
      if (stderr) process.stderr.write(stderr);
    },
  },
);
```

`onOutput` is optional and fires during execution with chunk objects shaped like
`{ stdout, stderr }`. Chunks are not line-aligned or exact terminal interleaving, but
concatenating all callback chunks matches the final `ExecResult.stdout` and
`ExecResult.stderr`. The handler must be synchronous; Promise-returning
handlers are rejected. Do not call back into the same `Bash` / `BashTool`
instance from `onOutput` via `execute*`, `readFile`, `fs()`, or similar
same-instance APIs.

## Configuration

### BashOptions

```typescript
import { Bash } from "@everruns/bashkit";

const bash = new Bash({
  username: "agent",
  hostname: "sandbox",
  maxCommands: 1000,
  maxLoopIterations: 10000,
  maxMemory: 10 * 1024 * 1024,
  timeoutMs: 30_000,
  mounts: [{ path: "/workspace", root: "./src", writable: true }],
  python: false,
});
```

## Virtual Filesystem

### Direct Methods on Bash and BashTool

```typescript
import { Bash } from "@everruns/bashkit";

const bash = new Bash();

bash.mkdir("/data", true);
bash.writeFile("/data/config.json", '{"debug":true}');
bash.appendFile("/data/config.json", "\n");

console.log(bash.readFile("/data/config.json"));
console.log(bash.exists("/data/config.json"));
console.log(bash.ls("/data"));
console.log(bash.glob("/data/*.json"));
```

`BashTool` exposes the same direct filesystem helpers.

### FileSystem Accessor

Call `bash.fs()` or `tool.fs()` when you need the underlying filesystem handle directly. For most applications, the convenience methods on `Bash` and `BashTool` are simpler and more explicit.

### Pre-Initialized Files

```typescript
import { Bash } from "@everruns/bashkit";

const bash = new Bash({
  files: {
    "/config.json": '{"key":"value"}',
    "/lazy.txt": () => "computed on first read",
  },
});

console.log(bash.readFile("/config.json"));

const asyncBash = await Bash.create({
  files: {
    "/async.txt": async () => "loaded asynchronously",
  },
});
```

### Real Filesystem Mounts

```typescript
import { Bash } from "@everruns/bashkit";

const bash = new Bash({
  mounts: [
    { path: "/docs", root: "./docs" },
    { path: "/workspace", root: "./src", writable: true },
  ],
});

console.log(bash.executeSync("ls /workspace").stdout);
```

### Live Mounts

```typescript
import { Bash } from "@everruns/bashkit";

const bash = new Bash();

bash.mount("./src", "/workspace", true);
console.log(bash.executeSync("ls /workspace").stdout);
bash.unmount("/workspace");
```

## Error Handling

```typescript
import { Bash, BashError } from "@everruns/bashkit";

const bash = new Bash();

try {
  bash.executeSyncOrThrow("exit 1");
} catch (err) {
  if (err instanceof BashError) {
    console.log(err.exitCode);
    console.log(err.stderr);
  }
}
```

## Cancellation

```typescript
import { Bash } from "@everruns/bashkit";

const bash = new Bash();

const running = bash.execute("sleep 60");
bash.cancel();
await running;

bash.clearCancel(); // preserve session/VFS state before reusing the instance
```

`cancel()` sets a sticky flag that causes future executions to fail with
`"execution cancelled"`. Call `clearCancel()` after the cancelled execution
has finished to reuse the same instance without losing shell or VFS state.
Use `reset()` only when you want to discard state entirely.

`BashTool` exposes the same `cancel()`, `clearCancel()`, and `reset()` methods.
For synchronous execution, `executeSync(...)` and `executeSyncOrThrow(...)`
also accept `{ signal }`.

## BashTool

`BashTool` wraps the interpreter with tool-contract metadata for agent frameworks:

- `name`
- `version`
- `shortDescription`
- `description()`
- `help()`
- `systemPrompt()`
- `inputSchema()`
- `outputSchema()`

```typescript
import { BashTool } from "@everruns/bashkit";

const tool = new BashTool();

console.log(tool.name);
console.log(tool.inputSchema());

const result = tool.executeSync("echo hello");
console.log(result.stdout);
```

## ScriptedTool

Use `ScriptedTool` to register JavaScript callbacks as bash-callable tools:

```typescript
import { ScriptedTool } from "@everruns/bashkit";

const tool = new ScriptedTool({ name: "api" });
tool.addTool("get_user", "Fetch user by ID", (params) => {
  return JSON.stringify({ id: params.id, name: "Alice" });
});

const result = tool.executeSync("get_user --id 1 | jq -r '.name'");
console.log(result.stdout); // Alice
```

## Snapshot / Restore

State snapshots are available on both `Bash` and `BashTool` instances:

```typescript
import { Bash, BashTool } from "@everruns/bashkit";

const bash = new Bash({ username: "agent", maxCommands: 100 });
await bash.execute(
  "export BUILD_ID=42; mkdir -p /workspace && cd /workspace && echo ready > state.txt",
);

const snapshot = bash.snapshot();

const restored = Bash.fromSnapshot(snapshot);
console.log((await restored.execute("echo $BUILD_ID")).stdout); // 42\n

restored.reset();
restored.restoreSnapshot(snapshot);
console.log(restored.executeSync("pwd").stdout); // /workspace\n

const tool = new BashTool({ username: "agent", maxCommands: 5 });
tool.executeSync("export TOOL_STATE=ready");

const toolSnapshot = tool.snapshot();
const restoredTool = BashTool.fromSnapshot(toolSnapshot, {
  username: "agent",
  maxCommands: 5,
});

console.log(restoredTool.executeSync("echo $TOOL_STATE").stdout); // ready\n
```

## Framework Integrations

### OpenAI

```typescript
import { bashTool } from "@everruns/bashkit/openai";

const bash = bashTool();
```

### Anthropic

```typescript
import { bashTool } from "@everruns/bashkit/anthropic";

const bash = bashTool();
```

### Vercel AI SDK

```typescript
import { bashTool } from "@everruns/bashkit/ai";

const bash = bashTool();
```

### LangChain

```typescript
import {
  createBashTool,
  createScriptedTool,
} from "@everruns/bashkit/langchain";
```

## API Reference

### Bash

- `new Bash(options?)`
- `Bash.create(options?)`
- `executeSync(commands, options?)`
- `execute(commands, options?)`
- `executeSyncOrThrow(commands, options?)`
- `executeOrThrow(commands, options?)`
- `cancel()`
- `clearCancel()`
- `reset()`
- `snapshot()`
- `restoreSnapshot(data)`
- `Bash.fromSnapshot(data)`
- Direct VFS helpers: `readFile`, `writeFile`, `appendFile`, `mkdir`, `remove`, `exists`, `stat`, `readDir`, `ls`, `glob`, `mount`, `unmount`, `fs`

### BashTool

- All execution, cancellation (`cancel()`, `clearCancel()`), reset, snapshot, restore, and direct VFS helpers from `Bash`
- Tool metadata: `name`, `version`, `shortDescription`
- `snapshot()`
- `restoreSnapshot(data)`
- `BashTool.fromSnapshot(data, options?)`
- `description()`
- `help()`
- `systemPrompt()`
- `inputSchema()`
- `outputSchema()`

### ScriptedTool

- `new ScriptedTool(options)`
- `addTool(name, description, callback, schema?)`
- `executeSync(script)`
- `execute(script)`
- `executeSyncOrThrow(script)`
- `executeOrThrow(script)`
- `env(key, value)`
- `toolCount()`

### BashOptions

- `username?: string`
- `hostname?: string`
- `maxCommands?: number`
- `maxLoopIterations?: number`
- `maxMemory?: number`
- `timeoutMs?: number`
- `files?: Record<string, string | (() => string) | (() => Promise<string>)>`
- `mounts?: Array<{ path: string; root: string; writable?: boolean }>`
- `python?: boolean`
- `externalFunctions?: string[]`

### ExecuteOptions

- `signal?: AbortSignal`
- `onOutput?: (chunk: { stdout: string; stderr: string }) => void`

### ExecResult and BashError

- `ExecResult.stdout`
- `ExecResult.stderr`
- `ExecResult.exitCode`
- `ExecResult.error`
- `ExecResult.success`
- `ExecResult.stdoutTruncated`
- `ExecResult.stderrTruncated`
- `BashError.exitCode`
- `BashError.stderr`

## Platform Support

| OS      | Architecture            |
| ------- | ----------------------- |
| macOS   | `x86_64`, `aarch64`     |
| Linux   | `x86_64`, `aarch64`     |
| Windows | `x86_64`                |
| WASM    | `wasm32-wasip1-threads` |

## How It Works

The JavaScript package wraps the Rust `bashkit` interpreter through NAPI-RS bindings. Commands execute in-process against a virtual filesystem, with the Rust core enforcing parsing, execution, and resource limits while the JS wrapper exposes a TypeScript-friendly API and framework adapters.

## Part of Everruns

Bashkit is part of the [Everruns](https://github.com/everruns) ecosystem. See the [bashkit monorepo](https://github.com/everruns/bashkit) for the Rust core, the Python package (`bashkit`), and related tooling.

## License

MIT
