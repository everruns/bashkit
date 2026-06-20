# TypeScript API reference

Auto-generated reference for the [`@everruns/bashkit`](https://www.npmjs.com/package/@everruns/bashkit) npm package. Reflects the latest published release.

> Install with `npm install @everruns/bashkit`. See the [Embedding guide](/docs/embedding/) and [LLM tools guide](/docs/llm-tools/) for task-oriented walkthroughs.

## Bash

Core bash interpreter with virtual filesystem.

State persists between calls — files created in one `execute()` are
available in subsequent calls.

```typescript
import { Bash } from '@everruns/bashkit';

const bash = new Bash();
const result = bash.executeSync('echo "Hello, World!"');
console.log(result.stdout); // Hello, World!\n
```

### Constructor

```typescript
new Bash(options?: BashOptions): Bash
```

### `addBuiltin`

```typescript
Bash.addBuiltin(name: string, callback: BuiltinCallback): void
```

Register a JS callback as a custom bash builtin.

The callback receives a `BuiltinContext` and returns the stdout to
emit. Sync (`string`) and async (`Promise<string>`) returns are both
supported. Exceptions become stderr + exit code 1.

Safe to call at any time — the new builtin is visible to the next
`execute*()` invocation with no interpreter rebuild or VFS disturbance.
Survives `reset()`.

### `appendFile`

```typescript
Bash.appendFile(path: string, content: string): void
```

Append content to a file.

### `cancel`

```typescript
Bash.cancel(): void
```

Cancel the currently running execution.

### `chmod`

```typescript
Bash.chmod(path: string, mode: number): void
```

Change file permissions (octal mode, e.g. 0o755).

### `clearCancel`

```typescript
Bash.clearCancel(): void
```

Clear the cancellation flag so subsequent executions proceed normally.

Call this after `cancel()` once the in-flight execution has finished and
you want to reuse the same instance without discarding shell or VFS state.

### `execute`

```typescript
Bash.execute(commands: string, options?: ExecuteOptions): Promise<ExecResult>
```

Execute bash commands asynchronously, returning a Promise.

Non-blocking for the Node.js event loop.
If `onOutput` is provided, it receives chunk objects with `{ stdout, stderr }`
during execution. Chunks are not line-aligned. The callback must be
synchronous; Promise-returning handlers are rejected. Do not re-enter the
same instance from `onOutput` via `execute*`, `readFile`, `fs()`, etc.

```typescript
const result = await bash.execute('echo hello');
console.log(result.stdout); // hello\n
```

### `executeOrThrow`

```typescript
Bash.executeOrThrow(commands: string, options?: ExecuteOptions): Promise<ExecResult>
```

Execute bash commands asynchronously. Throws `BashError` on non-zero exit.

### `executeSync`

```typescript
Bash.executeSync(commands: string, options?: ExecuteOptions): ExecResult
```

Execute bash commands synchronously and return the result.

If `signal` is provided, the execution will be cancelled when the signal
is aborted. If `onOutput` is provided, it receives chunk objects with
`{ stdout, stderr }` during execution. Chunks are not line-aligned. The callback must be
synchronous; Promise-returning handlers are rejected. Do not re-enter the
same instance from `onOutput` via `execute*`, `readFile`, `fs()`, etc.

### `executeSyncOrThrow`

```typescript
Bash.executeSyncOrThrow(commands: string, options?: ExecuteOptions): ExecResult
```

Execute bash commands synchronously. Throws `BashError` on non-zero exit.

### `exists`

```typescript
Bash.exists(path: string): boolean
```

Check if a path exists in the virtual filesystem.

### `fs`

```typescript
Bash.fs(): FileSystem
```

Get a FileSystem handle for direct VFS operations.

### `glob`

```typescript
Bash.glob(pattern: string): string[]
```

Find files matching a name pattern. Returns absolute paths.

### `ls`

```typescript
Bash.ls(path?: string): string[]
```

List entry names in a directory. Returns empty array if directory does not exist.

### `mkdir`

```typescript
Bash.mkdir(path: string, recursive?: boolean): void
```

Create a directory. If recursive is true, creates parents as needed.

### `mount`

```typescript
Bash.mount(vfsPath: string, fs: FileSystem): void
```

Mount either a host directory or a FileSystem into the VFS.

### `readDir`

```typescript
Bash.readDir(path: string): ({ metadata: { created: number; fileType: string; mode: number; modified: number; size: number }; name: string })[]
```

List directory entries with metadata.

### `readFile`

```typescript
Bash.readFile(path: string): string
```

Read a file from the virtual filesystem as a UTF-8 string.

### `readLink`

```typescript
Bash.readLink(path: string): string
```

Read the target of a symbolic link.

### `remove`

```typescript
Bash.remove(path: string, recursive?: boolean): void
```

Remove a file or directory. If recursive is true, removes contents.

### `removeBuiltin`

```typescript
Bash.removeBuiltin(name: string): void
```

Remove a previously registered custom builtin.

### `reset`

```typescript
Bash.reset(): void
```

Reset interpreter to fresh state, preserving configuration.

### `restoreSnapshot`

```typescript
Bash.restoreSnapshot(data: Uint8Array, options?: SnapshotOptions): void
```

Restore interpreter state from a previously captured snapshot.
Preserves current configuration (limits, builtins) but replaces
shell state and VFS contents.

### `restoreSnapshotKeyed`

```typescript
Bash.restoreSnapshotKeyed(data: Uint8Array, key: Uint8Array): void
```

Restore interpreter state from a HMAC-protected snapshot.

### `shellState`

```typescript
Bash.shellState(): ShellState
```

Capture a lightweight snapshot of shell state (variables, env, cwd,
arrays, aliases, traps) for inspection — e.g. prompt rendering or
debugging. Function definitions are omitted; use `snapshot()` for full
state capture/restore.

### `snapshot`

```typescript
Bash.snapshot(options?: SnapshotOptions): Uint8Array
```

Serialize interpreter state (variables, VFS, counters) to a Uint8Array.

Use `hmacKey` when snapshots are stored outside the current trust boundary
(network, user uploads, shared storage). Without `hmacKey`, the snapshot
digest only detects accidental corruption and is forgeable.

```typescript
const bash = new Bash();
await bash.execute("x=42");
const snapshot = bash.snapshot();
// persist snapshot...
const bash2 = Bash.fromSnapshot(snapshot);
const r = await bash2.execute("echo $x"); // "42\n"
```

### `snapshotKeyed`

```typescript
Bash.snapshotKeyed(key: Uint8Array, options?: SnapshotOptions): Uint8Array
```

Serialize interpreter state with HMAC-SHA256 using a caller-provided key.
Use for snapshots crossing trust boundaries.

### `stat`

```typescript
Bash.stat(path: string): { created: number; fileType: string; mode: number; modified: number; size: number }
```

Get metadata for a path (fileType, size, mode, timestamps).

### `symlink`

```typescript
Bash.symlink(target: string, link: string): void
```

Create a symbolic link pointing to target.

### `unmount`

```typescript
Bash.unmount(vfsPath: string): void
```

Unmount a previously mounted filesystem.

### `writeFile`

```typescript
Bash.writeFile(path: string, content: string): void
```

Write a string to a file in the virtual filesystem.

### `create`

```typescript
Bash.create(options?: BashOptions): Promise<Bash>
```

Create a Bash instance with support for async file providers.

Use this instead of `new Bash()` when file values are async functions.

```typescript
const bash = await Bash.create({
  files: {
    "/data/remote.json": async () => await fetchData(),
  }
});
```

### `fromSnapshot`

```typescript
Bash.fromSnapshot(data: Uint8Array, options?: SnapshotOptions): Bash
```

Create a new Bash instance from a snapshot.

```typescript
const snapshot = existingBash.snapshot();
const restored = Bash.fromSnapshot(snapshot);
```

### `fromSnapshotKeyed`

```typescript
Bash.fromSnapshotKeyed(data: Uint8Array, key: Uint8Array): Bash
```

Create a new Bash instance from a HMAC-protected snapshot.

## BashTool

Bash interpreter with tool-contract metadata.

Use this when integrating with AI frameworks that need tool definitions.

```typescript
import { BashTool } from '@everruns/bashkit';

const tool = new BashTool();
console.log(tool.name);           // "bashkit"
console.log(tool.inputSchema());  // JSON schema string
console.log(tool.help());         // Markdown help document

const result = tool.executeSync('echo hello');
console.log(result.stdout);       // hello\n
```

### Constructor

```typescript
new BashTool(options?: BashOptions): BashTool
```

### `name`

```typescript
name: string
```

Tool name.

### `shortDescription`

```typescript
shortDescription: string
```

Short description.

### `version`

```typescript
version: string
```

Tool version.

### `addBuiltin`

```typescript
BashTool.addBuiltin(name: string, callback: BuiltinCallback): void
```

Register a JS callback as a custom builtin. See `Bash.addBuiltin`.

### `appendFile`

```typescript
BashTool.appendFile(path: string, content: string): void
```

Append content to a file.

### `cancel`

```typescript
BashTool.cancel(): void
```

Cancel the currently running execution.

### `chmod`

```typescript
BashTool.chmod(path: string, mode: number): void
```

Change file permissions (octal mode, e.g. 0o755).

### `clearCancel`

```typescript
BashTool.clearCancel(): void
```

Clear the cancellation flag so subsequent executions proceed normally.

Call this after `cancel()` once the in-flight execution has finished and
you want to reuse the same instance without discarding shell or VFS state.

### `description`

```typescript
BashTool.description(): string
```

Token-efficient tool description.

### `execute`

```typescript
BashTool.execute(commands: string, options?: ExecuteOptions): Promise<ExecResult>
```

Execute bash commands asynchronously, returning a Promise.

If `onOutput` is provided, it must be synchronous; Promise-returning
handlers are rejected. Do not re-enter the same instance from `onOutput`
via `execute*`, `readFile`, `fs()`, etc.

### `executeOrThrow`

```typescript
BashTool.executeOrThrow(commands: string, options?: ExecuteOptions): Promise<ExecResult>
```

Execute bash commands asynchronously. Throws `BashError` on non-zero exit.

### `executeSync`

```typescript
BashTool.executeSync(commands: string, options?: ExecuteOptions): ExecResult
```

Execute bash commands synchronously and return the result.

If `onOutput` is provided, it must be synchronous; Promise-returning
handlers are rejected. Do not re-enter the same instance from `onOutput`
via `execute*`, `readFile`, `fs()`, etc.

### `executeSyncOrThrow`

```typescript
BashTool.executeSyncOrThrow(commands: string, options?: ExecuteOptions): ExecResult
```

Execute bash commands synchronously. Throws `BashError` on non-zero exit.

### `exists`

```typescript
BashTool.exists(path: string): boolean
```

Check whether a path exists in the virtual filesystem.

### `fs`

```typescript
BashTool.fs(): FileSystem
```

Get a FileSystem handle for direct VFS operations.

### `glob`

```typescript
BashTool.glob(pattern: string): string[]
```

Find files matching a name pattern. Returns absolute paths.

### `help`

```typescript
BashTool.help(): string
```

Markdown help document.

### `inputSchema`

```typescript
BashTool.inputSchema(): string
```

JSON input schema as string.

### `ls`

```typescript
BashTool.ls(path?: string): string[]
```

List entry names in a directory. Returns empty array if directory does not exist.

### `mkdir`

```typescript
BashTool.mkdir(path: string, recursive?: boolean): void
```

Create a directory. If recursive is true, creates parents as needed.

### `mount`

```typescript
BashTool.mount(vfsPath: string, fs: FileSystem): void
```

Mount either a host directory or a FileSystem into the VFS.

### `outputSchema`

```typescript
BashTool.outputSchema(): string
```

JSON output schema as string.

### `readDir`

```typescript
BashTool.readDir(path: string): ({ metadata: { created: number; fileType: string; mode: number; modified: number; size: number }; name: string })[]
```

List directory entries with metadata.

### `readFile`

```typescript
BashTool.readFile(path: string): string
```

Read file contents from the virtual filesystem.
Throws `BashError` if the file does not exist.

### `readLink`

```typescript
BashTool.readLink(path: string): string
```

Read the target of a symbolic link.

### `remove`

```typescript
BashTool.remove(path: string, recursive?: boolean): void
```

Remove a file or directory. If recursive is true, removes contents.

### `removeBuiltin`

```typescript
BashTool.removeBuiltin(name: string): void
```

Remove a previously registered custom builtin.

### `reset`

```typescript
BashTool.reset(): void
```

Reset interpreter to fresh state, preserving configuration.

### `restoreSnapshot`

```typescript
BashTool.restoreSnapshot(data: Uint8Array, options: SnapshotOptions & { hmacKey: Uint8Array }): void
```

Restore interpreter state from an HMAC-authenticated snapshot.
Preserves current configuration (limits, identity) but replaces
shell state and VFS contents.

### `restoreSnapshotKeyed`

```typescript
BashTool.restoreSnapshotKeyed(data: Uint8Array, key: Uint8Array): void
```

Restore interpreter state from a HMAC-protected snapshot.

### `shellState`

```typescript
BashTool.shellState(): ShellState
```

Capture a lightweight snapshot of shell state (variables, env, cwd,
arrays, aliases, traps) for inspection — e.g. prompt rendering or
debugging. Function definitions are omitted; use `snapshot()` for full
state capture/restore.

### `snapshot`

```typescript
BashTool.snapshot(options: SnapshotOptions & { hmacKey: Uint8Array }): Uint8Array
```

Serialize interpreter state (variables, VFS, counters) to an
HMAC-authenticated Uint8Array. BashTool snapshots require `hmacKey` because
they include tenant-controlled shell state, VFS contents, and counters.

### `snapshotKeyed`

```typescript
BashTool.snapshotKeyed(key: Uint8Array, options?: SnapshotOptions): Uint8Array
```

Serialize interpreter state with HMAC-SHA256 using a caller-provided key.

### `stat`

```typescript
BashTool.stat(path: string): { created: number; fileType: string; mode: number; modified: number; size: number }
```

Get metadata for a path (fileType, size, mode, timestamps).

### `symlink`

```typescript
BashTool.symlink(target: string, link: string): void
```

Create a symbolic link pointing to target.

### `systemPrompt`

```typescript
BashTool.systemPrompt(): string
```

Compact system prompt for orchestration.

### `unmount`

```typescript
BashTool.unmount(vfsPath: string): void
```

Unmount a previously mounted filesystem.

### `writeFile`

```typescript
BashTool.writeFile(path: string, content: string): void
```

Write content to a file in the virtual filesystem.
Creates parent directories as needed.

### `create`

```typescript
BashTool.create(options?: BashOptions): Promise<BashTool>
```

Create a BashTool instance with support for async file providers.

### `fromSnapshot`

```typescript
BashTool.fromSnapshot(data: Uint8Array, options: undefined | BashOptions, snapshotOptions: SnapshotOptions & { hmacKey: Uint8Array }): BashTool
```

Create a new BashTool instance from an HMAC-authenticated snapshot.

Any provided Bash options are applied before restoring the snapshot so
limits and identity settings survive round-trips.

### `fromSnapshotKeyed`

```typescript
BashTool.fromSnapshotKeyed(data: Uint8Array, key: Uint8Array, options?: BashOptions): BashTool
```

Create a new BashTool instance from a HMAC-protected snapshot.

## ScriptedTool

Compose JS callbacks as bash builtins for multi-tool orchestration.

Each registered tool becomes a bash builtin command. An LLM (or user) writes
a single bash script that pipes, loops, and branches across all tools.

```typescript
import { ScriptedTool } from '@everruns/bashkit';

const tool = new ScriptedTool({ name: "api" });
tool.addTool("greet", "Greet user",
  (params) => `hello ${params.name ?? "world"}\n`
);
const result = tool.executeSync("greet --name Alice");
console.log(result.stdout); // hello Alice\n
```

### Constructor

```typescript
new ScriptedTool(options: ScriptedToolOptions): ScriptedTool
```

### `name`

```typescript
name: string
```

Tool name.

### `shortDescription`

```typescript
shortDescription: string
```

Short description.

### `version`

```typescript
version: string
```

Tool version.

### `addTool`

```typescript
ScriptedTool.addTool(name: string, description: string, callback: ToolCallback, schema?: Record<string, unknown>): void
```

Register a tool command.

### `description`

```typescript
ScriptedTool.description(): string
```

Token-efficient tool description.

### `env`

```typescript
ScriptedTool.env(key: string, value: string): void
```

Add an environment variable visible inside scripts.

### `execute`

```typescript
ScriptedTool.execute(commands: string): Promise<ExecResult>
```

Execute a bash script asynchronously, returning a Promise.

This is the recommended execution method for ScriptedTool since
tool callbacks require the Node.js event loop to be running.

### `executeOrThrow`

```typescript
ScriptedTool.executeOrThrow(commands: string): Promise<ExecResult>
```

Execute asynchronously. Throws `BashError` on non-zero exit.

### `executeSync`

```typescript
ScriptedTool.executeSync(commands: string): ExecResult
```

Execute a bash script synchronously.

Note: ScriptedTool callbacks run asynchronously via Node's event loop.
If a registered tool is invoked, this method returns a non-zero result
instead of queueing a callback that would deadlock. Use `execute()`
(async) for scripts that call registered tools. Only use this for scripts
that don't invoke any registered tools (e.g., pure bash).

### `executeSyncOrThrow`

```typescript
ScriptedTool.executeSyncOrThrow(commands: string): ExecResult
```

Execute synchronously. Throws `BashError` on non-zero exit.

Same caveats as `executeSync()` — throws when a registered tool would
require the blocked Node event loop. Use `executeOrThrow()` instead.

### `help`

```typescript
ScriptedTool.help(): string
```

Markdown help document.

### `inputSchema`

```typescript
ScriptedTool.inputSchema(): string
```

JSON input schema as string.

### `outputSchema`

```typescript
ScriptedTool.outputSchema(): string
```

JSON output schema as string.

### `systemPrompt`

```typescript
ScriptedTool.systemPrompt(): string
```

Compact system prompt for orchestration.

### `toolCount`

```typescript
ScriptedTool.toolCount(): number
```

Number of registered tools.

## FileSystem

### Constructor

```typescript
new FileSystem(): FileSystem
```

### `appendFile`

```typescript
FileSystem.appendFile(path: string, content: string): void
```

### `chmod`

```typescript
FileSystem.chmod(path: string, mode: number): void
```

### `copy`

```typescript
FileSystem.copy(fromPath: string, toPath: string): void
```

### `exists`

```typescript
FileSystem.exists(path: string): boolean
```

### `mkdir`

```typescript
FileSystem.mkdir(path: string, recursive?: boolean): void
```

### `readDir`

```typescript
FileSystem.readDir(path: string): ({ metadata: { created: number; fileType: string; mode: number; modified: number; size: number }; name: string })[]
```

### `readFile`

```typescript
FileSystem.readFile(path: string): string
```

### `readLink`

```typescript
FileSystem.readLink(path: string): string
```

### `remove`

```typescript
FileSystem.remove(path: string, recursive?: boolean): void
```

### `rename`

```typescript
FileSystem.rename(fromPath: string, toPath: string): void
```

### `stat`

```typescript
FileSystem.stat(path: string): { created: number; fileType: string; mode: number; modified: number; size: number }
```

### `symlink`

```typescript
FileSystem.symlink(target: string, link: string): void
```

### `toExternal`

```typescript
FileSystem.toExternal(): unknown
```

### `writeFile`

```typescript
FileSystem.writeFile(path: string, content: string): void
```

### `fromExternal`

```typescript
FileSystem.fromExternal(external: unknown): FileSystem
```

### `fromNative`

```typescript
FileSystem.fromNative(nativeFs: any): FileSystem
```

### `real`

```typescript
FileSystem.real(hostPath: string, options: FileSystemRealOptions): FileSystem
```

## BashError

Error thrown when a bash command execution fails.

### Properties

- **`exitCode`** — `number`
- **`stderr`** — `string`

### Constructor

```typescript
new BashError(result: ExecResult): BashError
```

### `display`

```typescript
BashError.display(): string
```

## getVersion()

```typescript
getVersion(): string
```

Get the bashkit version string.

## BashOptions

Options for creating a Bash or BashTool instance.

### Fields

- **`allowedMountPaths?`** — `string[]`

  Allowlist of host path prefixes permitted for real filesystem mounts.

  Required for `mounts` and runtime `mount()` APIs. Mount targets must
  resolve under one of these prefixes; otherwise the call is rejected.
- **`customBuiltins?`** — `Record<string, BuiltinCallback>`

  Custom JS callbacks registered as bash builtins.

  Each entry becomes a bash command that shares the instance's VFS — files
  created by the callback persist across `execute()` calls. Callbacks can be
  sync (`string` return) or async (`Promise<string>`); exceptions surface as
  stderr with exit code 1.

  Equivalent to calling `addBuiltin(name, cb)` for each entry. Survives
  `reset()`; not preserved through `snapshot()`/`restoreSnapshot()` — pass
  the option again after restoring.

  Override precedence: shell function > POSIX special builtin > custom
  builtin > baked-in builtin > PATH.
- **`cwd?`** — `string`

  Initial working directory for the shell.

  Sets the starting `cwd` directly instead of running a leading
  `cd "${cwd}"` command, avoiding the parse/exec overhead.
- **`env?`** — `Record<string, string>`

  Initial environment variables, applied before execution.

  Scripts see these immediately without an `export` prelude. Keys are
  variable names, values are their string values.
- **`externalFunctions?`** — `string[]`

  Names of external functions callable from embedded Python code.

  These function names become available as Python builtins within
  the embedded interpreter. When called, they invoke the external handler.
- **`files?`** — `Record<string, FileValue>`

  Files to mount in the virtual filesystem.
  Keys are absolute paths, values are content strings or lazy providers.

  String values are mounted immediately. Function values are called on
  first read and the result is cached.
- **`hostname?`** — `string`
- **`maxCommands?`** — `number`
- **`maxInputBytes?`** — `number`

  Maximum script input size in UTF-8 bytes.

  Async execute validates this before entering the per-instance queue so
  oversized calls cannot wait while retaining large command strings.
- **`maxLoopIterations?`** — `number`
- **`maxMemory?`** — `number`

  Maximum interpreter memory in bytes (variables, arrays, functions).

  Caps the total byte budget for variable storage and function bodies.
  Prevents OOM from untrusted input such as exponential string doubling.
- **`maxTotalLoopIterations?`** — `number`
- **`mounts?`** — `({ path: string; root: string; writable?: boolean })[]`

  Real filesystem mounts. Each mount maps a host directory into the VFS.
- **`network?`** — `NetworkOptions`

  Outbound network configuration. When set, enables `curl`/`wget`
  restricted to the configured allowlist, with optional transparent
  credential injection. Omitted = network disabled (no `curl`/`wget`).

  Must specify either `allow` (list of URL patterns) or `allowAll: true`,
  not both. `blockPrivateIps` defaults to `true`.
- **`python?`** — `boolean`

  Enable embedded Python execution (`python`/`python3` builtins).

  When true, bash scripts can use `python -c '...'` or `python3 script.py`
  to run Python code within the sandbox.
- **`sqlite?`** — `boolean`

  Enable the embedded SQLite builtin (`sqlite`/`sqlite3`).

  Backed by Turso. When `true`, the binding both registers the builtin
  and injects `BASHKIT_ALLOW_INPROCESS_SQLITE=1` so the runtime gate is
  satisfied. Defaults to `false`. Default `SqliteLimits` apply: 4 MiB
  script cap, 256 MiB DB cap, 30 s wall-clock budget,
  resource-affecting PRAGMAs and `ATTACH`/`DETACH` rejected.
- **`timeoutMs?`** — `number`

  Execution timeout in milliseconds.

  When set, commands that exceed this duration are aborted with
  exit code 124 (matching the bash `timeout` convention).
- **`username?`** — `string`

## BuiltinContext

Execution context passed to a custom builtin registered via
`customBuiltins` or `addBuiltin`.

`name`, `argv`, `stdin`, `env`, and `cwd` are snapshots of the shell
state at invocation time; `fs` is a live handle to the interpreter's
virtual filesystem.

### Fields

- **`argv`** — `string[]`

  Arguments (not including the command name).
- **`cwd`** — `string`

  Current working directory.
- **`env`** — `Record<string, string>`

  Environment variables visible at the call site.
- **`fs`** — `FileSystem`

  Live handle to the interpreter's virtual filesystem — the same VFS the
  executing script sees. Reads observe earlier script writes and writes
  are visible to subsequent commands. Inherits mounts and read-only
  configuration.
- **`name`** — `string`

  The command name as invoked.
- **`stdin`** — `null | string`

  Piped input, or `null` if there is no pipe.

## CredentialHeader

A single HTTP header (name/value pair) for credential injection.

### Fields

- **`name`** — `string`
- **`value`** — `string`

## ExecResult

Result from executing bash commands.

### Fields

- **`error?`** — `string`
- **`exitCode`** — `number`
- **`finalEnv?`** — `Record<string, string>`
- **`stderr`** — `string`
- **`stderrTruncated`** — `boolean`
- **`stdout`** — `string`
- **`stdoutTruncated`** — `boolean`
- **`success`** — `boolean`

  True if exit_code is 0.

## ExecuteOptions

### Fields

- **`onOutput?`** — `OnOutput`

  Live chunk callback. Must be synchronous.

  Limitation: do not call back into the same `Bash` / `BashTool` instance
  from this handler (`execute*`, `readFile`, `fs()`, etc.). The current
  binding rejects same-instance re-entry to avoid deadlocks and runtime
  panics.
- **`signal?`** — `AbortSignal`

## FileSystemRealOptions

Standalone filesystem handle for direct VFS operations and native addon interop.

### Fields

- **`allowedMountPaths`** — `string[]`
- **`writable?`** — `boolean`

## NetworkCredential

Credential injected into outbound HTTP requests matching `pattern`.

`kind` selects the shape: `"bearer"` (requires `token`), `"header"`
(requires `name` + `value`), or `"headers"` (requires `headers`).
Scripts never see the secret — it is attached on the wire only.

### Fields

- **`headers?`** — `CredentialHeader[]`

  Header list (for `kind: "headers"`).
- **`kind`** — `string`

  One of `"bearer"`, `"header"`, `"headers"`.
- **`name?`** — `string`

  Header name (for `kind: "header"`).
- **`pattern`** — `string`

  URL pattern the credential applies to (e.g. `https://api.example.com/**`).
- **`token?`** — `string`

  Bearer token (for `kind: "bearer"`).
- **`value?`** — `string`

  Header value (for `kind: "header"`).

## NetworkCredentialPlaceholder

Placeholder-mode credential injection: `env` is set to an opaque
placeholder value visible to scripts; outbound requests matching
`pattern` have the placeholder replaced with the real credential.

### Fields

- **`env`** — `string`

  Environment variable that receives the placeholder value.
- **`headers?`** — `CredentialHeader[]`

  Header list (for `kind: "headers"`).
- **`kind`** — `string`

  One of `"bearer"`, `"header"`, `"headers"`.
- **`name?`** — `string`

  Header name (for `kind: "header"`).
- **`pattern`** — `string`

  URL pattern the credential applies to.
- **`token?`** — `string`

  Bearer token (for `kind: "bearer"`).
- **`value?`** — `string`

  Header value (for `kind: "header"`).

## NetworkOptions

Outbound network configuration (enables `curl`/`wget`).

Must specify either `allow` (list of URL patterns) or `allowAll: true`,
not both. `blockPrivateIps` defaults to `true`.

### Fields

- **`allow?`** — `string[]`

  URL patterns permitted for outbound requests.
- **`allowAll?`** — `boolean`

  Allow all outbound requests (mutually exclusive with `allow`).
- **`blockPrivateIps?`** — `boolean`

  Block requests resolving to private/loopback IPs (default: true).
- **`credentialPlaceholders?`** — `NetworkCredentialPlaceholder[]`

  Placeholder-mode credential injection (see type docs).
- **`credentials?`** — `NetworkCredential[]`

  Credentials injected transparently into matching requests.

## OutputChunk

### Fields

- **`stderr`** — `string`
- **`stdout`** — `string`

## ScriptedToolOptions

Options for creating a ScriptedTool instance.

### Fields

- **`maxCommands?`** — `number`
- **`maxLoopIterations?`** — `number`
- **`name`** — `string`
- **`shortDescription?`** — `string`

## ShellState

Lightweight snapshot of shell state for inspection (prompt rendering,
debugging). Mirrors the Python binding's `ShellState`; omits function
definitions (use `snapshot()` for full state capture/restore).

### Fields

- **`aliases`** — `Record<string, string>`

  Shell aliases.
- **`arrays`** — `Record<string, Record<string, string>>`

  Indexed arrays: name → { index (as string) → value }. Sparse indices
  are preserved, hence a map rather than a dense JS array.
- **`assocArrays`** — `Record<string, Record<string, string>>`

  Associative arrays: name → { key → value }.
- **`cwd`** — `string`

  Current working directory.
- **`env`** — `Record<string, string>`

  Environment variables.
- **`lastExitCode`** — `number`

  Exit code of the last executed command.
- **`traps`** — `Record<string, string>`

  Trap handlers keyed by signal/condition name.
- **`variables`** — `Record<string, string>`

  Shell variables (non-exported).

## SnapshotOptions

### Fields

- **`excludeFilesystem?`** — `boolean`
- **`excludeFunctions?`** — `boolean`
- **`hmacKey?`** — `Uint8Array<ArrayBufferLike>`

  Secret key used to authenticate snapshot bytes with HMAC-SHA256.
  Required for BashTool snapshots because tool snapshots may cross tenant
  or network trust boundaries. Recommended for any snapshot accepted from
  users, shared storage, or remote callers.

## BuiltinCallback

Callback signature for custom builtins. Return the stdout to emit.
Both sync (`string`) and async (`Promise<string>`) returns are supported.
Exceptions / rejections surface as stderr with exit code 1.

```typescript
type BuiltinCallback = (ctx: BuiltinContext) => string | Promise<string>
```

## FileValue

A file value: either a string, a sync function returning a string,
or an async function returning a Promise<string>.

Function values are resolved lazily on first read and cached.

```typescript
type FileValue = string | () => string | () => Promise<string>
```

## OnOutput

```typescript
type OnOutput = (chunk: OutputChunk) => void
```

## ToolCallback

Callback type for ScriptedTool tool commands.

Receives parsed `--key value` flags as `params` and optional piped input as `stdin`.
Must return a string.

```typescript
type ToolCallback = (params: Record<string, unknown>, stdin: string | null) => string
```

---

# Framework integrations

## `@everruns/bashkit/langchain`

LangChain.js integration for Bashkit.

Provides LangChain-compatible tools wrapping BashTool and ScriptedTool
for use with LangChain agents and chains.

```typescript
import { createBashTool, createScriptedTool } from '@everruns/bashkit/langchain';

// Basic bash tool
const tool = createBashTool();
const result = await tool.invoke({ commands: "echo hello" });

// Scripted tool
import { ScriptedTool } from '@everruns/bashkit';
const st = new ScriptedTool({ name: "api" });
st.addTool("greet", "Greet user", (p) => `hello ${p.name}\n`);
const langchainTool = createScriptedTool(st);
```

### createBashTool()

```typescript
createBashTool(options?: Omit<BashOptions, "files">): DynamicStructuredTool
```

Create a LangChain-compatible Bashkit tool.

Returns a `DynamicStructuredTool` that can be passed directly to
LangChain agents like `createReactAgent`.

```typescript
import { createBashTool } from '@everruns/bashkit/langchain';
import { createReactAgent } from '@langchain/langgraph/prebuilt';

const tool = createBashTool({ username: "agent" });
const agent = createReactAgent({ llm: model, tools: [tool] });
```

### createScriptedTool()

```typescript
createScriptedTool(scriptedTool: ScriptedTool): DynamicStructuredTool
```

Create a LangChain-compatible tool from a configured ScriptedTool.

The ScriptedTool should already have tools registered via `addTool()`.

```typescript
import { ScriptedTool } from '@everruns/bashkit';
import { createScriptedTool } from '@everruns/bashkit/langchain';

const st = new ScriptedTool({ name: "api" });
st.addTool("get_data", "Fetch data", (p) => JSON.stringify({ id: p.id }));
const tool = createScriptedTool(st);
```

## `@everruns/bashkit/anthropic`

Anthropic SDK adapter for Bashkit.

Returns a ready-to-use `{ system, tools, handler }` object for Claude's
`messages.create()` API, eliminating boilerplate for tool integration.

```typescript
import Anthropic from "@anthropic-ai/sdk";
import { bashTool } from "@everruns/bashkit/anthropic";

const client = new Anthropic();
const bash = bashTool();

const response = await client.messages.create({
  model: "claude-haiku-4-5-20251001",
  max_tokens: 1024,
  system: bash.system,
  tools: bash.tools,
  messages: [{ role: "user", content: "List files in /home" }],
});

for (const block of response.content) {
  if (block.type === "tool_use") {
    const result = await bash.handler(block);
    // send result back as tool_result
  }
}
```

### bashTool()

```typescript
bashTool(options?: BashToolOptions): BashToolAdapter
```

Create a bash tool adapter for the Anthropic SDK.

Returns `{ system, tools, handler }` that plugs directly into
`client.messages.create()`.

```typescript
import Anthropic from "@anthropic-ai/sdk";
import { bashTool } from "@everruns/bashkit/anthropic";

const client = new Anthropic();
const bash = bashTool({ files: { "/data.txt": "hello" } });

const response = await client.messages.create({
  model: "claude-haiku-4-5-20251001",
  max_tokens: 256,
  system: bash.system,
  tools: bash.tools,
  messages: [{ role: "user", content: "Read /data.txt" }],
});
```

### BashToolAdapter

Return value of `bashTool()`.

### Fields

- **`bash`** — `BashTool`

  The underlying BashTool instance for direct access.
- **`handler`** — `(toolUse: ToolUseBlock, options: HandlerOptions) => Promise<ToolResult>`

  Handler that executes a tool_use block and returns a tool_result.

  Pass an AbortSignal via the options parameter to cancel execution
  when the framework aborts the tool call:

  ```typescript
  const controller = new AbortController();
  const result = await bash.handler(block, { signal: controller.signal });
  ```
- **`system`** — `string`

  System prompt describing bash capabilities and constraints.
- **`tools`** — `AnthropicTool[]`

  Tool definitions for Anthropic's messages.create() API.

### BashToolOptions

Options for configuring the bash tool adapter.

### Fields

- **`allowedMountPaths?`** — `string[]`

  Allowlist of host path prefixes permitted for real filesystem mounts.

  Required for `mounts` and runtime `mount()` APIs. Mount targets must
  resolve under one of these prefixes; otherwise the call is rejected.
- **`customBuiltins?`** — `Record<string, BuiltinCallback>`

  Custom JS callbacks registered as bash builtins.

  Each entry becomes a bash command that shares the instance's VFS — files
  created by the callback persist across `execute()` calls. Callbacks can be
  sync (`string` return) or async (`Promise<string>`); exceptions surface as
  stderr with exit code 1.

  Equivalent to calling `addBuiltin(name, cb)` for each entry. Survives
  `reset()`; not preserved through `snapshot()`/`restoreSnapshot()` — pass
  the option again after restoring.

  Override precedence: shell function > POSIX special builtin > custom
  builtin > baked-in builtin > PATH.
- **`cwd?`** — `string`

  Initial working directory for the shell.

  Sets the starting `cwd` directly instead of running a leading
  `cd "${cwd}"` command, avoiding the parse/exec overhead.
- **`env?`** — `Record<string, string>`

  Initial environment variables, applied before execution.

  Scripts see these immediately without an `export` prelude. Keys are
  variable names, values are their string values.
- **`externalFunctions?`** — `string[]`

  Names of external functions callable from embedded Python code.

  These function names become available as Python builtins within
  the embedded interpreter. When called, they invoke the external handler.
- **`files?`** — `Record<string, string>`

  Pre-populate VFS files. Keys are absolute paths, values are file contents.
- **`hostname?`** — `string`
- **`maxCommands?`** — `number`
- **`maxInputBytes?`** — `number`

  Maximum script input size in UTF-8 bytes.

  Async execute validates this before entering the per-instance queue so
  oversized calls cannot wait while retaining large command strings.
- **`maxLoopIterations?`** — `number`
- **`maxMemory?`** — `number`

  Maximum interpreter memory in bytes (variables, arrays, functions).

  Caps the total byte budget for variable storage and function bodies.
  Prevents OOM from untrusted input such as exponential string doubling.
- **`maxOutputLength?`** — `number`

  Maximum output length in characters (default: 100000).

  Output exceeding this limit is truncated with a `[truncated]` marker.
  Prevents context window flooding when scripts produce large output.
- **`maxTotalLoopIterations?`** — `number`
- **`mounts?`** — `({ path: string; root: string; writable?: boolean })[]`

  Real filesystem mounts. Each mount maps a host directory into the VFS.
- **`network?`** — `NetworkOptions`

  Outbound network configuration. When set, enables `curl`/`wget`
  restricted to the configured allowlist, with optional transparent
  credential injection. Omitted = network disabled (no `curl`/`wget`).

  Must specify either `allow` (list of URL patterns) or `allowAll: true`,
  not both. `blockPrivateIps` defaults to `true`.
- **`python?`** — `boolean`

  Enable embedded Python execution (`python`/`python3` builtins).

  When true, bash scripts can use `python -c '...'` or `python3 script.py`
  to run Python code within the sandbox.
- **`sanitizeOutput?`** — `boolean`

  Wrap tool output in XML boundary markers (default: false).

  When enabled, output is wrapped in `<tool_output>...</tool_output>` tags
  to help LLMs distinguish tool output data from instructions, reducing
  prompt injection risk via tool output.

  **Security note:** This is a defense-in-depth measure. Tool output from
  untrusted sources (files, network) may contain text that attempts to
  manipulate LLM behavior. Boundary markers help but do not eliminate this risk.
- **`sqlite?`** — `boolean`

  Enable the embedded SQLite builtin (`sqlite`/`sqlite3`).

  Backed by Turso. When `true`, the binding both registers the builtin
  and injects `BASHKIT_ALLOW_INPROCESS_SQLITE=1` so the runtime gate is
  satisfied. Defaults to `false`. Default `SqliteLimits` apply: 4 MiB
  script cap, 256 MiB DB cap, 30 s wall-clock budget,
  resource-affecting PRAGMAs and `ATTACH`/`DETACH` rejected.
- **`timeoutMs?`** — `number`

  Execution timeout in milliseconds.

  When set, this is passed to the underlying BashTool as `timeoutMs`.
  Commands exceeding this duration are aborted with exit code 124.
  Framework-level timeouts can be propagated here to ensure bashkit
  stops execution when the framework cancels a tool call.
- **`username?`** — `string`

### HandlerOptions

Options for handler invocation.

### Fields

- **`signal?`** — `AbortSignal`

  AbortSignal to cancel execution when the framework aborts the tool call.

### ToolResult

Result from handling a tool call, ready to send back as tool_result.

### Fields

- **`content`** — `string`
- **`is_error?`** — `boolean`
- **`tool_use_id`** — `string`
- **`type`** — `"tool_result"`

## `@everruns/bashkit/openai`

OpenAI SDK adapter for Bashkit.

Returns a ready-to-use `{ system, tools, handler }` object for OpenAI's
`chat.completions.create()` API.

```typescript
import OpenAI from "openai";
import { bashTool } from "@everruns/bashkit/openai";

const client = new OpenAI();
const bash = bashTool();

const response = await client.chat.completions.create({
  model: "gpt-4.1-mini",
  tools: bash.tools,
  messages: [
    { role: "system", content: bash.system },
    { role: "user", content: "Create a file with today's date" },
  ],
});

for (const call of response.choices[0].message.tool_calls ?? []) {
  const result = await bash.handler(call);
  // send result back as tool message
}
```

### bashTool()

```typescript
bashTool(options?: BashToolOptions): BashToolAdapter
```

Create a bash tool adapter for the OpenAI SDK.

Returns `{ system, tools, handler }` that plugs directly into
`client.chat.completions.create()`.

```typescript
import OpenAI from "openai";
import { bashTool } from "@everruns/bashkit/openai";

const client = new OpenAI();
const bash = bashTool({ files: { "/data.txt": "42" } });

const response = await client.chat.completions.create({
  model: "gpt-4.1-nano",
  tools: bash.tools,
  messages: [
    { role: "system", content: bash.system },
    { role: "user", content: "What's in /data.txt?" },
  ],
});
```

### BashToolAdapter

Return value of `bashTool()`.

### Fields

- **`bash`** — `BashTool`

  The underlying BashTool instance for direct access.
- **`handler`** — `(toolCall: OpenAIToolCall, options: HandlerOptions) => Promise<ToolResult>`

  Handler that executes a tool_call and returns a tool message.

  Pass an AbortSignal via the options parameter to cancel execution
  when the framework aborts the tool call:

  ```typescript
  const controller = new AbortController();
  const result = await bash.handler(call, { signal: controller.signal });
  ```
- **`system`** — `string`

  System prompt describing bash capabilities and constraints.
- **`tools`** — `OpenAITool[]`

  Tool definitions for OpenAI's chat.completions.create() API.

### BashToolOptions

Options for configuring the bash tool adapter.

### Fields

- **`allowedMountPaths?`** — `string[]`

  Allowlist of host path prefixes permitted for real filesystem mounts.

  Required for `mounts` and runtime `mount()` APIs. Mount targets must
  resolve under one of these prefixes; otherwise the call is rejected.
- **`customBuiltins?`** — `Record<string, BuiltinCallback>`

  Custom JS callbacks registered as bash builtins.

  Each entry becomes a bash command that shares the instance's VFS — files
  created by the callback persist across `execute()` calls. Callbacks can be
  sync (`string` return) or async (`Promise<string>`); exceptions surface as
  stderr with exit code 1.

  Equivalent to calling `addBuiltin(name, cb)` for each entry. Survives
  `reset()`; not preserved through `snapshot()`/`restoreSnapshot()` — pass
  the option again after restoring.

  Override precedence: shell function > POSIX special builtin > custom
  builtin > baked-in builtin > PATH.
- **`cwd?`** — `string`

  Initial working directory for the shell.

  Sets the starting `cwd` directly instead of running a leading
  `cd "${cwd}"` command, avoiding the parse/exec overhead.
- **`env?`** — `Record<string, string>`

  Initial environment variables, applied before execution.

  Scripts see these immediately without an `export` prelude. Keys are
  variable names, values are their string values.
- **`externalFunctions?`** — `string[]`

  Names of external functions callable from embedded Python code.

  These function names become available as Python builtins within
  the embedded interpreter. When called, they invoke the external handler.
- **`files?`** — `Record<string, string>`

  Pre-populate VFS files. Keys are absolute paths, values are file contents.
- **`hostname?`** — `string`
- **`maxCommands?`** — `number`
- **`maxInputBytes?`** — `number`

  Maximum script input size in UTF-8 bytes.

  Async execute validates this before entering the per-instance queue so
  oversized calls cannot wait while retaining large command strings.
- **`maxLoopIterations?`** — `number`
- **`maxMemory?`** — `number`

  Maximum interpreter memory in bytes (variables, arrays, functions).

  Caps the total byte budget for variable storage and function bodies.
  Prevents OOM from untrusted input such as exponential string doubling.
- **`maxOutputLength?`** — `number`

  Maximum output length in characters (default: 100000).

  Output exceeding this limit is truncated with a `[truncated]` marker.
  Prevents context window flooding when scripts produce large output.
- **`maxTotalLoopIterations?`** — `number`
- **`mounts?`** — `({ path: string; root: string; writable?: boolean })[]`

  Real filesystem mounts. Each mount maps a host directory into the VFS.
- **`network?`** — `NetworkOptions`

  Outbound network configuration. When set, enables `curl`/`wget`
  restricted to the configured allowlist, with optional transparent
  credential injection. Omitted = network disabled (no `curl`/`wget`).

  Must specify either `allow` (list of URL patterns) or `allowAll: true`,
  not both. `blockPrivateIps` defaults to `true`.
- **`python?`** — `boolean`

  Enable embedded Python execution (`python`/`python3` builtins).

  When true, bash scripts can use `python -c '...'` or `python3 script.py`
  to run Python code within the sandbox.
- **`sanitizeOutput?`** — `boolean`

  Wrap tool output in XML boundary markers (default: false).

  When enabled, output is wrapped in `<tool_output>...</tool_output>` tags
  to help LLMs distinguish tool output data from instructions, reducing
  prompt injection risk via tool output.
- **`sqlite?`** — `boolean`

  Enable the embedded SQLite builtin (`sqlite`/`sqlite3`).

  Backed by Turso. When `true`, the binding both registers the builtin
  and injects `BASHKIT_ALLOW_INPROCESS_SQLITE=1` so the runtime gate is
  satisfied. Defaults to `false`. Default `SqliteLimits` apply: 4 MiB
  script cap, 256 MiB DB cap, 30 s wall-clock budget,
  resource-affecting PRAGMAs and `ATTACH`/`DETACH` rejected.
- **`timeoutMs?`** — `number`

  Execution timeout in milliseconds.

  When set, this is passed to the underlying BashTool as `timeoutMs`.
  Commands exceeding this duration are aborted with exit code 124.
  Framework-level timeouts can be propagated here to ensure bashkit
  stops execution when the framework cancels a tool call.
- **`username?`** — `string`

### HandlerOptions

Options for handler invocation.

### Fields

- **`signal?`** — `AbortSignal`

  AbortSignal to cancel execution when the framework aborts the tool call.

### ToolResult

Result from handling a tool call, ready to send as a tool message.

### Fields

- **`content`** — `string`
- **`role`** — `"tool"`
- **`tool_call_id`** — `string`

## `@everruns/bashkit/ai`

Vercel AI SDK adapter for Bashkit.

Returns `{ system, tools }` that plugs directly into `generateText()` /
`streamText()` with zero boilerplate. Tools include built-in `execute`
functions, so the AI SDK auto-executes tool calls in its `maxSteps` loop.

```typescript
import { generateText } from "ai";
import { anthropic } from "@ai-sdk/anthropic";
import { bashTool } from "@everruns/bashkit/ai";

const bash = bashTool({
  files: { "/home/user/data.csv": "name,age\nAlice,30\nBob,25" },
});

const { text } = await generateText({
  model: anthropic("claude-haiku-4-5-20251001"),
  system: bash.system,
  tools: bash.tools,
  maxSteps: 5,
  prompt: "Analyze the CSV file and tell me the average age",
});
```

### bashTool()

```typescript
bashTool(options?: BashToolOptions): BashToolAdapter
```

Create a bash tool adapter for the Vercel AI SDK.

Returns `{ system, tools }` that plugs directly into `generateText()` or
`streamText()`. The tool includes a built-in `execute` function, so tool
calls are auto-executed when using `maxSteps`.

```typescript
import { generateText } from "ai";
import { anthropic } from "@ai-sdk/anthropic";
import { bashTool } from "@everruns/bashkit/ai";

const bash = bashTool({ files: { "/test.txt": "hello world" } });

const { text } = await generateText({
  model: anthropic("claude-haiku-4-5-20251001"),
  system: bash.system,
  tools: bash.tools,
  maxSteps: 3,
  prompt: "Read /test.txt and tell me what it says",
});
```

### BashToolAdapter

Return value of `bashTool()`.

### Fields

- **`bash`** — `BashTool`

  The underlying BashTool instance for direct access.
- **`system`** — `string`

  System prompt describing bash capabilities and constraints.
- **`tools`** — `Record<string, AiTool>`

  Tool definitions for Vercel AI SDK's generateText/streamText.

### BashToolOptions

Options for configuring the bash tool adapter.

### Fields

- **`allowedMountPaths?`** — `string[]`

  Allowlist of host path prefixes permitted for real filesystem mounts.

  Required for `mounts` and runtime `mount()` APIs. Mount targets must
  resolve under one of these prefixes; otherwise the call is rejected.
- **`customBuiltins?`** — `Record<string, BuiltinCallback>`

  Custom JS callbacks registered as bash builtins.

  Each entry becomes a bash command that shares the instance's VFS — files
  created by the callback persist across `execute()` calls. Callbacks can be
  sync (`string` return) or async (`Promise<string>`); exceptions surface as
  stderr with exit code 1.

  Equivalent to calling `addBuiltin(name, cb)` for each entry. Survives
  `reset()`; not preserved through `snapshot()`/`restoreSnapshot()` — pass
  the option again after restoring.

  Override precedence: shell function > POSIX special builtin > custom
  builtin > baked-in builtin > PATH.
- **`cwd?`** — `string`

  Initial working directory for the shell.

  Sets the starting `cwd` directly instead of running a leading
  `cd "${cwd}"` command, avoiding the parse/exec overhead.
- **`env?`** — `Record<string, string>`

  Initial environment variables, applied before execution.

  Scripts see these immediately without an `export` prelude. Keys are
  variable names, values are their string values.
- **`externalFunctions?`** — `string[]`

  Names of external functions callable from embedded Python code.

  These function names become available as Python builtins within
  the embedded interpreter. When called, they invoke the external handler.
- **`files?`** — `Record<string, string>`

  Pre-populate VFS files. Keys are absolute paths, values are file contents.
- **`hostname?`** — `string`
- **`maxCommands?`** — `number`
- **`maxInputBytes?`** — `number`

  Maximum script input size in UTF-8 bytes.

  Async execute validates this before entering the per-instance queue so
  oversized calls cannot wait while retaining large command strings.
- **`maxLoopIterations?`** — `number`
- **`maxMemory?`** — `number`

  Maximum interpreter memory in bytes (variables, arrays, functions).

  Caps the total byte budget for variable storage and function bodies.
  Prevents OOM from untrusted input such as exponential string doubling.
- **`maxTotalLoopIterations?`** — `number`
- **`mounts?`** — `({ path: string; root: string; writable?: boolean })[]`

  Real filesystem mounts. Each mount maps a host directory into the VFS.
- **`network?`** — `NetworkOptions`

  Outbound network configuration. When set, enables `curl`/`wget`
  restricted to the configured allowlist, with optional transparent
  credential injection. Omitted = network disabled (no `curl`/`wget`).

  Must specify either `allow` (list of URL patterns) or `allowAll: true`,
  not both. `blockPrivateIps` defaults to `true`.
- **`python?`** — `boolean`

  Enable embedded Python execution (`python`/`python3` builtins).

  When true, bash scripts can use `python -c '...'` or `python3 script.py`
  to run Python code within the sandbox.
- **`sqlite?`** — `boolean`

  Enable the embedded SQLite builtin (`sqlite`/`sqlite3`).

  Backed by Turso. When `true`, the binding both registers the builtin
  and injects `BASHKIT_ALLOW_INPROCESS_SQLITE=1` so the runtime gate is
  satisfied. Defaults to `false`. Default `SqliteLimits` apply: 4 MiB
  script cap, 256 MiB DB cap, 30 s wall-clock budget,
  resource-affecting PRAGMAs and `ATTACH`/`DETACH` rejected.
- **`timeoutMs?`** — `number`

  Execution timeout in milliseconds.

  When set, commands that exceed this duration are aborted with
  exit code 124 (matching the bash `timeout` convention).
- **`username?`** — `string`
