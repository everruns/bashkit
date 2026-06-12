# @everruns/bashkit

Sandboxed bash interpreter for JavaScript and TypeScript. Native NAPI-RS bindings to the `bashkit` Rust core for Node.js, Bun, and Deno.

Homepage: [bashkit.sh](https://bashkit.sh)

## Features

- Sandboxed, in-process execution with a virtual filesystem
- Full bash syntax: variables, pipelines, redirects, loops, functions, and arrays
- 156 built-in commands including `grep`, `sed`, `awk`, `jq`, `curl`, and `find`
- Sync and async execution APIs
- Direct VFS helpers, standalone `FileSystem` handles, and live mounts
- Cancellation support via `cancel()`
- Sticky cancellation recovery via `clearCancel()`
- Snapshot and restore support on `Bash`
- Outbound HTTP (`curl`/`wget`) behind a URL allowlist with transparent credential injection
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
  "for i in 1 2 3; do echo out-$i; echo err-$i >&2; done",
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

```typescript
import { Bash, FileSystem } from "@everruns/bashkit";

const source = new FileSystem();
source.writeFile("/org/repo/README.md", "hello\n");

const bash = new Bash();
bash.mount("/workspace", source);

console.log(bash.executeSync("cat /workspace/org/repo/README.md").stdout);
```

Call `bash.fs()` or `tool.fs()` when you need a live handle to the current interpreter filesystem. Use `new FileSystem()` or `FileSystem.real(...)` when you need a standalone mountable filesystem.

### Native Addon Interop

```typescript
import { Bash, FileSystem } from "@everruns/bashkit";
import { createFilesystem } from "filesystem-addon";

const imported = FileSystem.fromExternal(createFilesystem());

const bash = new Bash();
bash.mount("/workspace", imported);
console.log(bash.executeSync("ls /workspace").stdout);
```

`toExternal()` / `fromExternal()` exchange an opaque native `External` token for
the versioned stable ABI handle, so JavaScript cannot inspect or mutate native
handle bytes. Native addons should depend on `bashkit` with the `interop`
feature and use `bashkit::interop::fs`.

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
import { Bash, FileSystem } from "@everruns/bashkit";

const bash = new Bash();
const workspace = FileSystem.real("./src", {
  writable: true,
  allowedMountPaths: ["./src"],
});

bash.mount("/workspace", workspace);
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

Async `execute(...)` calls are serialized per instance and keep a bounded
backlog. If too many executions are already pending on one `Bash`, `BashTool`,
or `ScriptedTool`, the next async call rejects instead of retaining another
queued command string. Commands larger than `maxInputBytes` are rejected before
entering the async queue.

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

## Custom Builtins

Register JS callbacks as bash builtins that share the `Bash` / `BashTool`
instance's VFS — files created in one call persist across `execute()` calls.

```typescript
import { Bash } from "@everruns/bashkit";

// Constructor-time registration
const bash = new Bash({
  customBuiltins: {
    "get-order": (ctx) =>
      JSON.stringify({ id: ctx.argv[0], status: "shipped" }) + "\n",
  },
});

// Or post-construction (safe to call at any time — no interpreter rebuild)
bash.addBuiltin("greet", (ctx) => `hello ${ctx.argv[0] ?? "world"}\n`);

// Tool output flows through the shared VFS
await bash.execute("mkdir -p /scratch");
await bash.execute("get-order 42 > /scratch/order.json");
console.log((await bash.execute("cat /scratch/order.json")).stdout);
// {"id":"42","status":"shipped"}

bash.removeBuiltin("greet"); // when you no longer need it
```

Callbacks can be sync (`string` return) or async (`Promise<string>`). Both
are awaited uniformly by the Rust side. Exceptions / rejections become
stderr with exit code 1.

```typescript
const bash = new Bash({
  customBuiltins: {
    fetch: async (ctx) => {
      const data = await fetchSomething(ctx.argv[0]);
      return JSON.stringify(data) + "\n";
    },
    fail: () => {
      throw new Error("nope"); // → stderr, exit 1
    },
  },
});
```

The callback receives a `BuiltinContext`:

- `name: string` — command name as invoked
- `argv: string[]` — arguments (not including the command name)
- `stdin: string | null` — piped input, or `null` if no pipe
- `env: Record<string, string>` — environment variables (only exported names)
- `cwd: string` — current working directory
- `fs: FileSystem` — live handle to the instance's virtual filesystem

`ctx.fs` is the same VFS the executing script sees: reads observe earlier
script writes, and writes are visible to subsequent commands. It inherits
mounts and read-only configuration.

```typescript
const bash = new Bash({
  customBuiltins: {
    head10: (ctx) =>
      ctx.fs.readFile(ctx.argv[0]).split("\n").slice(0, 10).join("\n") + "\n",
  },
});
await bash.execute("head10 /var/log/app.log");
```

Override precedence: shell function > POSIX special builtin > custom builtin

> baked-in builtin > `PATH`. Custom builtins can override baked-in commands
> (e.g. wrap `cat`), but shell functions defined in the script still win.

Custom builtins survive `reset()`. They are host-side configuration and are
**not** preserved by `snapshot()` / `restoreSnapshot()` — pass
`customBuiltins` again or call `addBuiltin` after restoring.

Use `execute()` (async). If the script invokes a custom builtin under
`executeSync()` the builtin fails fast with exit code 1 and stderr
`"<name>: custom builtins require execute() (async). ..."` instead of
deadlocking — the JS event loop is blocked while the synchronous call is
in flight, so the underlying `Promise<string>` callback could never run.

## Snapshot / Restore

State snapshots are available on both `Bash` and `BashTool` instances.

Security: unkeyed `Bash` snapshots use a public corruption-detection digest and
are forgeable. Use `hmacKey` whenever snapshot bytes cross a trust boundary
(network, user uploads, shared storage). `BashTool` snapshots require `hmacKey`
because they include tool session state, VFS contents, and counters that may be
restored in multi-tenant agent services.

```typescript
import { Bash, BashTool } from "@everruns/bashkit";

const bash = new Bash({ username: "agent", maxCommands: 100 });
await bash.execute(
  "export BUILD_ID=42; mkdir -p /workspace && cd /workspace && echo ready > state.txt",
);

const snapshot = bash.snapshot();
const shellOnly = bash.snapshot({ excludeFilesystem: true });
const promptOnly = bash.snapshot({
  excludeFilesystem: true,
  excludeFunctions: true,
});

const secretKey = Buffer.from(process.env.BASHKIT_SNAPSHOT_KEY!, "hex");
const keyedSnapshot = bash.snapshotKeyed(secretKey);
const keyedRestored = Bash.fromSnapshotKeyed(keyedSnapshot, secretKey);

const restored = Bash.fromSnapshot(snapshot);
console.log((await restored.execute("echo $BUILD_ID")).stdout); // 42\n

restored.reset();
restored.restoreSnapshot(snapshot);
restored.restoreSnapshot(shellOnly);
restored.restoreSnapshotKeyed(keyedSnapshot, secretKey);
console.log(restored.executeSync("pwd").stdout); // /workspace\n

const tool = new BashTool({ username: "agent", maxCommands: 5 });
tool.executeSync("export TOOL_STATE=ready");

const hmacKey = new TextEncoder().encode(process.env.SNAPSHOT_SECRET!);
const toolSnapshot = tool.snapshot({ hmacKey });
const toolShellOnly = tool.snapshot({ excludeFilesystem: true, hmacKey });
const restoredTool = BashTool.fromSnapshot(
  toolSnapshot,
  {
    username: "agent",
    maxCommands: 5,
  },
  { hmacKey },
);

console.log(restoredTool.executeSync("echo $TOOL_STATE").stdout); // ready\n
restoredTool.restoreSnapshot(toolShellOnly, { hmacKey });
```

## Network

Outbound HTTP (`curl` / `wget`) is disabled by default. Enable it with the
`network` option, which requires either an explicit URL allowlist or
`allowAll: true`:

```typescript
const bash = new Bash({
  network: {
    allow: ["https://api.example.com/**"],
    blockPrivateIps: true, // default
  },
});

await bash.execute("curl https://api.example.com/users"); // allowed
await bash.execute("curl https://evil.example.org"); // denied: not in allowlist
```

### Credential Injection

Attach credentials to matching requests without exposing them to scripts:

```typescript
const bash = new Bash({
  network: {
    allow: ["https://api.example.com/**"],
    // Direct injection — the script never sees the secret.
    credentials: [
      {
        pattern: "https://api.example.com/**",
        kind: "bearer",
        token: process.env.API_TOKEN!,
      },
    ],
    // Placeholder mode — the script sees an opaque `bk_placeholder_...`
    // value in $MY_TOKEN; it is swapped for the real secret on the wire.
    credentialPlaceholders: [
      {
        env: "MY_TOKEN",
        pattern: "https://api.example.com/**",
        kind: "header",
        name: "X-Api-Key",
        value: process.env.API_KEY!,
      },
    ],
  },
});
```

Credential kinds: `"bearer"` (requires `token`), `"header"` (requires
`name` + `value`), `"headers"` (requires `headers: Array<{ name, value }>`).

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
- `addBuiltin(name, callback)` / `removeBuiltin(name)` — register/unregister persistent JS builtins
- `snapshot(options?)` / `snapshotKeyed(key, options?)`
- `restoreSnapshot(data, options?)` / `restoreSnapshotKeyed(data, key)`
- `Bash.fromSnapshot(data, options?)` / `Bash.fromSnapshotKeyed(data, key)`
- Direct VFS helpers: `readFile`, `writeFile`, `appendFile`, `mkdir`, `remove`, `exists`, `stat`, `readDir`, `ls`, `glob`, `mount`, `unmount`, `fs`
- `shellState()` — lightweight inspection snapshot (variables, env, cwd, arrays, aliases, traps)

### BashTool

- All execution, cancellation (`cancel()`, `clearCancel()`), reset, custom builtins, snapshot, restore, and direct VFS helpers from `Bash`
- Tool metadata: `name`, `version`, `shortDescription`
- `snapshot({ hmacKey, ...options })`
- `restoreSnapshot(data, { hmacKey })`
- `BashTool.fromSnapshot(data, options?, { hmacKey })`
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
- `maxInputBytes?: number`
- `timeoutMs?: number`
- `files?: Record<string, string | (() => string) | (() => Promise<string>)>`
- `mounts?: Array<{ path: string; root: string; writable?: boolean }>`
- `python?: boolean`
- `externalFunctions?: string[]`
- `customBuiltins?: Record<string, (ctx: BuiltinContext) => string | Promise<string>>` — JS callbacks registered as bash builtins (see [Custom Builtins](#custom-builtins))
- `network?: NetworkOptions` — outbound HTTP configuration (see [Network](#network))

### BuiltinContext

- `name: string` — command name as invoked
- `argv: string[]` — arguments (not including the command name)
- `stdin: string | null` — piped input, `null` if no pipe
- `env: Record<string, string>` — exported environment variables
- `cwd: string` — current working directory
- `fs: FileSystem` — live handle to the instance's virtual filesystem

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
