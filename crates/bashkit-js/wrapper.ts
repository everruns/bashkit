import { AsyncResource } from "node:async_hooks";
import { createRequire } from "node:module";
import type {
  Bash as NativeBashType,
  BashTool as NativeBashToolType,
  ScriptedTool as NativeScriptedToolType,
  ExecResult,
  BashOptions as NativeBashOptions,
  SnapshotOptions as NativeSnapshotOptions,
  NetworkOptions,
  NetworkCredential,
  NetworkCredentialPlaceholder,
  CredentialHeader,
  ShellState,
} from "./index.cjs";

const require = createRequire(import.meta.url);
const native = require("./index.cjs");
const NativeBash: typeof NativeBashType = native.Bash;
const NativeBashTool: typeof NativeBashToolType = native.BashTool;
const NativeScriptedTool: typeof NativeScriptedToolType = native.ScriptedTool;
const nativeGetVersion: () => string = native.getVersion;
const nativeCreateFileSystem: () => any = native.__createFileSystem;
const nativeRealFileSystem: (
  hostPath: string,
  writable?: boolean,
  allowedMountPaths?: string[],
) => any = native.__realFileSystem;
const nativeImportFileSystem: (external: unknown) => any =
  native.__importFileSystem;
const nativeFileSystemToExternal: (fs: any) => unknown =
  native.__fileSystemToExternal;
const nativeFileSystemReadFile: (fs: any, path: string) => string =
  native.__fileSystemReadFile;
const nativeFileSystemWriteFile: (
  fs: any,
  path: string,
  content: string,
) => void = native.__fileSystemWriteFile;
const nativeFileSystemAppendFile: (
  fs: any,
  path: string,
  content: string,
) => void = native.__fileSystemAppendFile;
const nativeFileSystemMkdir: (
  fs: any,
  path: string,
  recursive?: boolean,
) => void = native.__fileSystemMkdir;
const nativeFileSystemRemove: (
  fs: any,
  path: string,
  recursive?: boolean,
) => void = native.__fileSystemRemove;
const nativeFileSystemStat: (
  fs: any,
  path: string,
) => {
  fileType: string;
  size: number;
  mode: number;
  modified: number;
  created: number;
} = native.__fileSystemStat;
const nativeFileSystemExists: (fs: any, path: string) => boolean =
  native.__fileSystemExists;
const nativeFileSystemReadDir: (
  fs: any,
  path: string,
) => Array<{
  name: string;
  metadata: {
    fileType: string;
    size: number;
    mode: number;
    modified: number;
    created: number;
  };
}> = native.__fileSystemReadDir;
const nativeFileSystemSymlink: (fs: any, target: string, link: string) => void =
  native.__fileSystemSymlink;
const nativeFileSystemReadLink: (fs: any, path: string) => string =
  native.__fileSystemReadLink;
const nativeFileSystemChmod: (fs: any, path: string, mode: number) => void =
  native.__fileSystemChmod;
const nativeFileSystemRename: (
  fs: any,
  fromPath: string,
  toPath: string,
) => void = native.__fileSystemRename;
const nativeFileSystemCopy: (
  fs: any,
  fromPath: string,
  toPath: string,
) => void = native.__fileSystemCopy;

export type {
  ExecResult,
  NetworkOptions,
  NetworkCredential,
  NetworkCredentialPlaceholder,
  CredentialHeader,
  ShellState,
};

/**
 * A file value: either a string, a sync function returning a string,
 * or an async function returning a Promise<string>.
 *
 * Function values are resolved lazily on first read and cached.
 */
export type FileValue = string | (() => string) | (() => Promise<string>);

const MAX_JSON_NESTING_DEPTH = 64;

/**
 * Execution context passed to a custom builtin registered via
 * `customBuiltins` or `addBuiltin`.
 *
 * `name`, `argv`, `stdin`, `env`, and `cwd` are snapshots of the shell
 * state at invocation time; `fs` is a live handle to the interpreter's
 * virtual filesystem.
 */
export interface BuiltinContext {
  /** The command name as invoked. */
  readonly name: string;
  /** Arguments (not including the command name). */
  readonly argv: string[];
  /** Piped input, or `null` if there is no pipe. */
  readonly stdin: string | null;
  /** Environment variables visible at the call site. */
  readonly env: Record<string, string>;
  /** Current working directory. */
  readonly cwd: string;
  /**
   * Live handle to the interpreter's virtual filesystem — the same VFS the
   * executing script sees. Reads observe earlier script writes and writes
   * are visible to subsequent commands. Inherits mounts and read-only
   * configuration.
   */
  readonly fs: FileSystem;
}

/**
 * Callback signature for custom builtins. Return the stdout to emit.
 * Both sync (`string`) and async (`Promise<string>`) returns are supported.
 * Exceptions / rejections surface as stderr with exit code 1.
 */
export type BuiltinCallback = (ctx: BuiltinContext) => string | Promise<string>;

/**
 * Options for creating a Bash or BashTool instance.
 */
export interface BashOptions {
  username?: string;
  hostname?: string;
  maxCommands?: number;
  maxLoopIterations?: number;
  maxTotalLoopIterations?: number;
  /**
   * Maximum script input size in UTF-8 bytes.
   *
   * Async execute validates this before entering the per-instance queue so
   * oversized calls cannot wait while retaining large command strings.
   */
  maxInputBytes?: number;
  /**
   * Maximum interpreter memory in bytes (variables, arrays, functions).
   *
   * Caps the total byte budget for variable storage and function bodies.
   * Prevents OOM from untrusted input such as exponential string doubling.
   *
   * @example
   * ```typescript
   * const bash = new Bash({ maxMemory: 10 * 1024 * 1024 }); // 10 MB
   * ```
   */
  maxMemory?: number;
  /**
   * Execution timeout in milliseconds.
   *
   * When set, commands that exceed this duration are aborted with
   * exit code 124 (matching the bash `timeout` convention).
   *
   * @example
   * ```typescript
   * const bash = new Bash({ timeoutMs: 30000 }); // 30 seconds
   * ```
   */
  timeoutMs?: number;
  /**
   * Files to mount in the virtual filesystem.
   * Keys are absolute paths, values are content strings or lazy providers.
   *
   * String values are mounted immediately. Function values are called on
   * first read and the result is cached.
   *
   * @example
   * ```typescript
   * const bash = await Bash.create({
   *   files: {
   *     "/data/config.json": '{"key": "value"}',
   *     "/data/large.json": () => fetchData(),
   *     "/data/remote.txt": async () => await fetch(url).then(r => r.text()),
   *   }
   * });
   * ```
   */
  files?: Record<string, FileValue>;
  /**
   * Real filesystem mounts. Each mount maps a host directory into the VFS.
   *
   * @example
   * ```typescript
   * const bash = new Bash({
   *   mounts: [
   *     { path: "/docs", root: "/real/path/to/docs" },
   *   ],
   * });
   * ```
   */
  mounts?: Array<{ path: string; root: string; writable?: boolean }>;
  /**
   * Allowlist of host path prefixes permitted for real filesystem mounts.
   *
   * Required for `mounts` and runtime `mount()` APIs. Mount targets must
   * resolve under one of these prefixes; otherwise the call is rejected.
   */
  allowedMountPaths?: string[];
  /**
   * Enable embedded Python execution (`python`/`python3` builtins).
   *
   * When true, bash scripts can use `python -c '...'` or `python3 script.py`
   * to run Python code within the sandbox.
   */
  python?: boolean;
  /**
   * Enable the embedded SQLite builtin (`sqlite`/`sqlite3`).
   *
   * Backed by Turso. When `true`, the binding both registers the builtin
   * and injects `BASHKIT_ALLOW_INPROCESS_SQLITE=1` so the runtime gate is
   * satisfied. Defaults to `false`. Default `SqliteLimits` apply: 4 MiB
   * script cap, 256 MiB DB cap, 30 s wall-clock budget,
   * resource-affecting PRAGMAs and `ATTACH`/`DETACH` rejected.
   */
  sqlite?: boolean;
  /**
   * Outbound network configuration. When set, enables `curl`/`wget`
   * restricted to the configured allowlist, with optional transparent
   * credential injection. Omitted = network disabled (no `curl`/`wget`).
   *
   * Must specify either `allow` (list of URL patterns) or `allowAll: true`,
   * not both. `blockPrivateIps` defaults to `true`.
   *
   * @example
   * ```typescript
   * const bash = new Bash({
   *   network: {
   *     allow: ["https://api.example.com/**"],
   *     credentials: [
   *       { pattern: "https://api.example.com/**", kind: "bearer", token: secret },
   *     ],
   *   },
   * });
   * ```
   */
  network?: NetworkOptions;
  /**
   * Names of external functions callable from embedded Python code.
   *
   * These function names become available as Python builtins within
   * the embedded interpreter. When called, they invoke the external handler.
   */
  externalFunctions?: string[];
  /**
   * Custom JS callbacks registered as bash builtins.
   *
   * Each entry becomes a bash command that shares the instance's VFS — files
   * created by the callback persist across `execute()` calls. Callbacks can be
   * sync (`string` return) or async (`Promise<string>`); exceptions surface as
   * stderr with exit code 1.
   *
   * Equivalent to calling `addBuiltin(name, cb)` for each entry. Survives
   * `reset()`; not preserved through `snapshot()`/`restoreSnapshot()` — pass
   * the option again after restoring.
   *
   * Override precedence: shell function > POSIX special builtin > custom
   * builtin > baked-in builtin > PATH.
   *
   * @example
   * ```typescript
   * const bash = new Bash({
   *   customBuiltins: {
   *     "get-order": (ctx) =>
   *       JSON.stringify({ id: ctx.argv[0], status: "shipped" }) + "\n",
   *   },
   * });
   * await bash.execute("mkdir -p /scratch");
   * await bash.execute("get-order 42 > /scratch/order.json");
   * console.log((await bash.execute("cat /scratch/order.json")).stdout);
   * ```
   */
  customBuiltins?: Record<string, BuiltinCallback>;
}

export interface SnapshotOptions {
  excludeFilesystem?: boolean;
  excludeFunctions?: boolean;
  /**
   * Secret key used to authenticate snapshot bytes with HMAC-SHA256.
   * Required for BashTool snapshots because tool snapshots may cross tenant
   * or network trust boundaries. Recommended for any snapshot accepted from
   * users, shared storage, or remote callers.
   */
  hmacKey?: Uint8Array;
}

export interface OutputChunk {
  stdout: string;
  stderr: string;
}

export type OnOutput = (chunk: OutputChunk) => void;

export interface ExecuteOptions {
  signal?: AbortSignal;
  /**
   * Live chunk callback. Must be synchronous.
   *
   * Limitation: do not call back into the same `Bash` / `BashTool` instance
   * from this handler (`execute*`, `readFile`, `fs()`, etc.). The current
   * binding rejects same-instance re-entry to avoid deadlocks and runtime
   * panics.
   */
  onOutput?: OnOutput;
}

type NativeOnOutput = (chunkPair: [string, string]) => string | undefined;
const ASYNC_ON_OUTPUT_ERROR =
  "onOutput must be synchronous and must not return a Promise";
const DEFAULT_MAX_INPUT_BYTES = 10_000_000;
const MAX_PENDING_ASYNC_EXECUTIONS = 8;
const ASYNC_EXECUTE_QUEUE_FULL_ERROR =
  "too many pending async execute calls for this instance";
interface AsyncExecuteQueueState {
  tail: Promise<void>;
  pending: number;
}
const asyncExecuteQueues = new WeakMap<object, AsyncExecuteQueueState>();

function isAsyncFunction(fn: Function): boolean {
  return Object.prototype.toString.call(fn) === "[object AsyncFunction]";
}

function isPromiseLike(value: unknown): value is PromiseLike<unknown> {
  return (
    value !== null &&
    (typeof value === "object" || typeof value === "function") &&
    typeof (value as { then?: unknown }).then === "function"
  );
}

function errorExecResult(error: string): ExecResult {
  return {
    stdout: "",
    stderr: error,
    exitCode: 1,
    error,
    stdoutTruncated: false,
    stderrTruncated: false,
    finalEnv: undefined,
    success: false,
  };
}

function cancelledExecResult(): ExecResult {
  return errorExecResult("execution cancelled");
}

function inputTooLargeExecResult(
  commands: string,
  maxInputBytes: number,
): ExecResult | undefined {
  const inputBytes = Buffer.byteLength(commands, "utf8");
  if (inputBytes <= maxInputBytes) {
    return undefined;
  }
  return errorExecResult(
    `input too large: ${inputBytes} bytes exceeds maxInputBytes ${maxInputBytes}`,
  );
}

// Decision: serialize async execute() per instance in JS so queued AbortSignal
// listeners only attach once a call reaches the front of the line. Also bound
// the backlog before retaining large command strings in queued closures.
function queueAsyncExecute<T>(
  owner: object,
  run: () => Promise<T>,
): Promise<T> {
  let state = asyncExecuteQueues.get(owner);
  if (!state) {
    state = { tail: Promise.resolve(), pending: 0 };
    asyncExecuteQueues.set(owner, state);
  }
  if (state.pending >= MAX_PENDING_ASYNC_EXECUTIONS) {
    return Promise.reject(new Error(ASYNC_EXECUTE_QUEUE_FULL_ERROR));
  }
  state.pending += 1;
  const previous = state.tail;
  const completion = previous.then(
    () => run(),
    () => run(),
  );
  state.tail = completion.then(
    () => undefined,
    () => undefined,
  );
  state.tail.finally(() => {
    state.pending -= 1;
    if (state.pending === 0 && asyncExecuteQueues.get(owner) === state) {
      asyncExecuteQueues.delete(owner);
    }
  });
  return completion;
}

function bindOnOutputToCurrentAsyncContext(onOutput: OnOutput): OnOutput {
  return AsyncResource.bind(onOutput) as OnOutput;
}

function toNativeOnOutput(onOutput?: OnOutput): NativeOnOutput | undefined {
  if (!onOutput) return undefined;
  if (isAsyncFunction(onOutput)) {
    throw new TypeError(ASYNC_ON_OUTPUT_ERROR);
  }
  const onOutputWithContext = bindOnOutputToCurrentAsyncContext(onOutput);
  // The native binding passes one tuple payload `[stdoutChunk, stderrChunk]`.
  // Adapt that odd FFI shape here so the public wrapper API stays future-proof.
  return ([stdoutChunk, stderrChunk]) => {
    try {
      const result = onOutputWithContext({
        stdout: stdoutChunk,
        stderr: stderrChunk,
      });
      if (isPromiseLike(result)) {
        void Promise.resolve(result).catch(() => {});
        return ASYNC_ON_OUTPUT_ERROR;
      }
      return undefined;
    } catch (error) {
      // THREAT[TM-INF-028]: never propagate error.stack — it contains host file
      // paths and function names. Use error.message only, and strip path-like
      // patterns so attacker-controlled output cannot smuggle them.
      const raw =
        error instanceof Error
          ? (error.message ?? error.toString())
          : String(error);
      // Remove absolute-path and file:// URL segments from the message.
      // Use a negative lookbehind so paths after punctuation (e.g. "at(/home/…")
      // are also stripped, not just paths preceded by whitespace.
      const sanitized = raw
        .replace(/file:\/\/[^\s]*/g, "<path>")
        .replace(/(?<!\w)\/[^\s]*/g, "<path>");
      return sanitized.slice(0, 256) || "output callback failed";
    }
  };
}

/**
 * Resolve file values: sync functions are called immediately,
 * async functions are awaited. Returns a plain string map.
 */
async function resolveFiles(
  files?: Record<string, FileValue>,
): Promise<Record<string, string> | undefined> {
  if (!files) return undefined;
  const resolved: Record<string, string> = {};
  for (const [path, value] of Object.entries(files)) {
    if (typeof value === "string") {
      resolved[path] = value;
    } else if (typeof value === "function") {
      const result = value();
      resolved[path] =
        result instanceof Promise ? await result : (result as string);
    }
  }
  return resolved;
}

function validateJsonNestingDepth(value: unknown, depth = 0): void {
  if (depth > MAX_JSON_NESTING_DEPTH) {
    throw new RangeError(
      `JSON nesting depth exceeds maximum of ${MAX_JSON_NESTING_DEPTH}`,
    );
  }

  if (Array.isArray(value)) {
    for (const item of value) {
      validateJsonNestingDepth(item, depth + 1);
    }
    return;
  }

  if (value && typeof value === "object") {
    for (const item of Object.values(value as Record<string, unknown>)) {
      validateJsonNestingDepth(item, depth + 1);
    }
  }
}

/**
 * Resolve file values synchronously. Throws if any value is async.
 */
function resolveFilesSync(
  files?: Record<string, FileValue>,
): Record<string, string> | undefined {
  if (!files) return undefined;
  const resolved: Record<string, string> = {};
  for (const [path, value] of Object.entries(files)) {
    if (typeof value === "string") {
      resolved[path] = value;
    } else if (typeof value === "function") {
      const result = value();
      if (result instanceof Promise) {
        throw new Error(
          `File "${path}" has an async provider. Use Bash.create() instead of new Bash() for async file values.`,
        );
      }
      resolved[path] = result as string;
    }
  }
  return resolved;
}

function toNativeOptions(
  options?: BashOptions,
  resolvedFiles?: Record<string, string>,
): NativeBashOptions | undefined {
  if (!options && !resolvedFiles) return undefined;
  return {
    username: options?.username,
    hostname: options?.hostname,
    maxCommands: options?.maxCommands,
    maxLoopIterations: options?.maxLoopIterations,
    maxTotalLoopIterations: options?.maxTotalLoopIterations,
    maxMemory: options?.maxMemory,
    timeoutMs: options?.timeoutMs,
    files: resolvedFiles,
    mounts: options?.mounts?.map((m) => ({
      hostPath: m.root,
      vfsPath: m.path,
      writable: m.writable,
    })),
    allowedMountPaths: options?.allowedMountPaths,
    python: options?.python,
    externalFunctions: options?.externalFunctions,
    sqlite: options?.sqlite,
    network: options?.network,
  };
}

function toNativeSnapshotOptions(
  options?: SnapshotOptions,
): NativeSnapshotOptions | undefined {
  if (!options) return undefined;
  return {
    excludeFilesystem: options.excludeFilesystem,
    excludeFunctions: options.excludeFunctions,
    hmacKey: options.hmacKey ? Buffer.from(options.hmacKey) : undefined,
  };
}

function requireSnapshotHmacKey(options?: SnapshotOptions): void {
  if (!options?.hmacKey || options.hmacKey.byteLength === 0) {
    throw new Error(
      "BashTool snapshots require SnapshotOptions.hmacKey for HMAC authentication",
    );
  }
}

function isFileSystemLike(value: unknown): value is { toExternal(): unknown } {
  return (
    typeof (value as { toExternal?: unknown } | null)?.toExternal === "function"
  );
}

/**
 * Error thrown when a bash command execution fails.
 */
export class BashError extends Error {
  readonly exitCode: number;
  readonly stderr: string;

  constructor(result: ExecResult) {
    const message =
      result.error ?? result.stderr ?? `Exit code ${result.exitCode}`;
    super(message);
    this.name = "BashError";
    this.exitCode = result.exitCode;
    this.stderr = result.stderr;
  }

  display(): string {
    return `BashError(exit_code=${this.exitCode}): ${this.message}`;
  }
}

/**
 * Standalone filesystem handle for direct VFS operations and native addon interop.
 */
export interface FileSystemRealOptions {
  writable?: boolean;
  allowedMountPaths: string[];
}

export class FileSystem {
  private native: any;
  private external?: unknown;

  constructor() {
    this.native = nativeCreateFileSystem();
  }

  static fromNative(nativeFs: any): FileSystem {
    const fs = Object.create(FileSystem.prototype) as FileSystem;
    fs.native = nativeFs;
    fs.external = undefined;
    return fs;
  }

  static real(hostPath: string, options: FileSystemRealOptions): FileSystem {
    if (!options || !Array.isArray(options.allowedMountPaths)) {
      throw new TypeError("FileSystem.real requires options.allowedMountPaths");
    }
    return FileSystem.fromNative(
      nativeRealFileSystem(
        hostPath,
        options.writable,
        options.allowedMountPaths,
      ),
    );
  }

  static fromExternal(external: unknown): FileSystem {
    const fs = FileSystem.fromNative(nativeImportFileSystem(external));
    fs.external = external;
    return fs;
  }

  toExternal(): unknown {
    this.external ??= nativeFileSystemToExternal(this.native);
    return this.external;
  }

  readFile(path: string): string {
    return nativeFileSystemReadFile(this.native, path);
  }

  writeFile(path: string, content: string): void {
    nativeFileSystemWriteFile(this.native, path, content);
  }

  appendFile(path: string, content: string): void {
    nativeFileSystemAppendFile(this.native, path, content);
  }

  mkdir(path: string, recursive?: boolean): void {
    nativeFileSystemMkdir(this.native, path, recursive);
  }

  remove(path: string, recursive?: boolean): void {
    nativeFileSystemRemove(this.native, path, recursive);
  }

  stat(path: string): {
    fileType: string;
    size: number;
    mode: number;
    modified: number;
    created: number;
  } {
    return nativeFileSystemStat(this.native, path);
  }

  exists(path: string): boolean {
    return nativeFileSystemExists(this.native, path);
  }

  readDir(path: string): Array<{
    name: string;
    metadata: {
      fileType: string;
      size: number;
      mode: number;
      modified: number;
      created: number;
    };
  }> {
    return nativeFileSystemReadDir(this.native, path);
  }

  symlink(target: string, link: string): void {
    nativeFileSystemSymlink(this.native, target, link);
  }

  readLink(path: string): string {
    return nativeFileSystemReadLink(this.native, path);
  }

  chmod(path: string, mode: number): void {
    nativeFileSystemChmod(this.native, path, mode);
  }

  rename(fromPath: string, toPath: string): void {
    nativeFileSystemRename(this.native, fromPath, toPath);
  }

  copy(fromPath: string, toPath: string): void {
    nativeFileSystemCopy(this.native, fromPath, toPath);
  }
}

/**
 * Wrap a {@link BuiltinCallback} for the native `addBuiltin` ABI.
 *
 * The native side passes the JSON-serialized context plus an opaque VFS
 * handle. Like the output callback, the napi TSFN delivers the
 * `(String, External)` args as one `[payload, fsHandle]` tuple; this
 * wrapper adapts that odd FFI shape into a single {@link BuiltinContext}
 * object with a live `fs` accessor.
 *
 * The Rust side expects a uniform `Promise<string>` return, so we always
 * route the callback result through `Promise.resolve` — sync `string`
 * returns get wrapped, native `Promise<string>` returns pass through, and
 * synchronous throws become rejected promises. Either way, the Rust adapter
 * sees one shape: a Promise it can `.await`.
 */
function makeBuiltinDispatcher(
  callback: BuiltinCallback,
): (requestPair: [string, unknown]) => Promise<string> {
  return (requestPair: [string, unknown]) => {
    try {
      const [payload, fsHandle] = requestPair;
      const parsed = JSON.parse(payload) as Omit<BuiltinContext, "fs">;
      const ctx: BuiltinContext = {
        ...parsed,
        fs: FileSystem.fromNative(fsHandle),
      };
      return Promise.resolve(callback(ctx));
    } catch (e) {
      return Promise.reject(e);
    }
  };
}

/**
 * Register each entry of `builtins` on `native` via `addBuiltin`.
 * Idempotent for empty/undefined input.
 */
function registerCustomBuiltins(
  native: {
    addBuiltin(
      name: string,
      dispatch: (requestPair: [string, unknown]) => Promise<string>,
    ): void;
  },
  builtins: Record<string, BuiltinCallback> | undefined,
): void {
  if (!builtins) return;
  for (const [name, cb] of Object.entries(builtins)) {
    native.addBuiltin(name, makeBuiltinDispatcher(cb));
  }
}

/**
 * Core bash interpreter with virtual filesystem.
 *
 * State persists between calls — files created in one `execute()` are
 * available in subsequent calls.
 *
 * @example
 * ```typescript
 * import { Bash } from '@everruns/bashkit';
 *
 * const bash = new Bash();
 * const result = bash.executeSync('echo "Hello, World!"');
 * console.log(result.stdout); // Hello, World!\n
 * ```
 */
export class Bash {
  private native: NativeBashType;
  private maxInputBytes: number;

  constructor(options?: BashOptions) {
    const resolved = resolveFilesSync(options?.files);
    this.native = new NativeBash(toNativeOptions(options, resolved));
    this.maxInputBytes = options?.maxInputBytes ?? DEFAULT_MAX_INPUT_BYTES;
    registerCustomBuiltins(this.native, options?.customBuiltins);
  }

  /**
   * Create a Bash instance with support for async file providers.
   *
   * Use this instead of `new Bash()` when file values are async functions.
   *
   * @example
   * ```typescript
   * const bash = await Bash.create({
   *   files: {
   *     "/data/remote.json": async () => await fetchData(),
   *   }
   * });
   * ```
   */
  static async create(options?: BashOptions): Promise<Bash> {
    const resolved = await resolveFiles(options?.files);
    const instance = Object.create(Bash.prototype) as Bash;
    instance.native = new NativeBash(toNativeOptions(options, resolved));
    instance.maxInputBytes = options?.maxInputBytes ?? DEFAULT_MAX_INPUT_BYTES;
    registerCustomBuiltins(instance.native, options?.customBuiltins);
    return instance;
  }

  /**
   * Register a JS callback as a custom bash builtin.
   *
   * The callback receives a {@link BuiltinContext} and returns the stdout to
   * emit. Sync (`string`) and async (`Promise<string>`) returns are both
   * supported. Exceptions become stderr + exit code 1.
   *
   * Safe to call at any time — the new builtin is visible to the next
   * `execute*()` invocation with no interpreter rebuild or VFS disturbance.
   * Survives `reset()`.
   */
  addBuiltin(name: string, callback: BuiltinCallback): void {
    this.native.addBuiltin(name, makeBuiltinDispatcher(callback));
  }

  /** Remove a previously registered custom builtin. */
  removeBuiltin(name: string): void {
    this.native.removeBuiltin(name);
  }

  /**
   * Execute bash commands synchronously and return the result.
   *
   * If `signal` is provided, the execution will be cancelled when the signal
   * is aborted. If `onOutput` is provided, it receives chunk objects with
   * `{ stdout, stderr }` during execution. Chunks are not line-aligned. The callback must be
   * synchronous; Promise-returning handlers are rejected. Do not re-enter the
   * same instance from `onOutput` via `execute*`, `readFile`, `fs()`, etc.
   */
  executeSync(commands: string, options?: ExecuteOptions): ExecResult {
    const nativeOnOutput = toNativeOnOutput(options?.onOutput);
    if (options?.signal) {
      const signal = options.signal;
      if (signal.aborted) {
        return cancelledExecResult();
      }
      const onAbort = () => this.native.cancel();
      signal.addEventListener("abort", onAbort, { once: true });
      try {
        return this.native.executeSync(commands, nativeOnOutput);
      } finally {
        signal.removeEventListener("abort", onAbort);
        if (signal.aborted) {
          this.native.clearCancel();
        }
      }
    }
    return this.native.executeSync(commands, nativeOnOutput);
  }

  /**
   * Execute bash commands asynchronously, returning a Promise.
   *
   * Non-blocking for the Node.js event loop.
   * If `onOutput` is provided, it receives chunk objects with `{ stdout, stderr }`
   * during execution. Chunks are not line-aligned. The callback must be
   * synchronous; Promise-returning handlers are rejected. Do not re-enter the
   * same instance from `onOutput` via `execute*`, `readFile`, `fs()`, etc.
   *
   * @example
   * ```typescript
   * const result = await bash.execute('echo hello');
   * console.log(result.stdout); // hello\n
   * ```
   */
  async execute(
    commands: string,
    options?: ExecuteOptions,
  ): Promise<ExecResult> {
    const nativeOnOutput = toNativeOnOutput(options?.onOutput);
    const signal = options?.signal;
    if (signal?.aborted) {
      return cancelledExecResult();
    }
    const inputLimitResult = inputTooLargeExecResult(
      commands,
      this.maxInputBytes,
    );
    if (inputLimitResult) {
      return inputLimitResult;
    }
    return queueAsyncExecute(this, async () => {
      if (signal?.aborted) {
        return cancelledExecResult();
      }
      if (signal) {
        let signalTriggered = false;
        const onAbort = () => {
          signalTriggered = true;
          this.native.cancel();
        };
        signal.addEventListener("abort", onAbort, { once: true });
        try {
          if (nativeOnOutput) {
            return await this.native.executeWithOutput(
              commands,
              nativeOnOutput,
            );
          }
          return await this.native.execute(commands);
        } finally {
          signal.removeEventListener("abort", onAbort);
          if (signalTriggered) {
            this.native.clearCancel();
          }
        }
      }
      if (nativeOnOutput) {
        return this.native.executeWithOutput(commands, nativeOnOutput);
      }
      return this.native.execute(commands);
    });
  }

  /**
   * Execute bash commands synchronously. Throws `BashError` on non-zero exit.
   */
  executeSyncOrThrow(commands: string, options?: ExecuteOptions): ExecResult {
    const result = this.executeSync(commands, options);
    if (result.exitCode !== 0) {
      throw new BashError(result);
    }
    return result;
  }

  /**
   * Execute bash commands asynchronously. Throws `BashError` on non-zero exit.
   */
  async executeOrThrow(
    commands: string,
    options?: ExecuteOptions,
  ): Promise<ExecResult> {
    const result = await this.execute(commands, options);
    if (result.exitCode !== 0) {
      throw new BashError(result);
    }
    return result;
  }

  /**
   * Cancel the currently running execution.
   */
  cancel(): void {
    this.native.cancel();
  }

  /**
   * Clear the cancellation flag so subsequent executions proceed normally.
   *
   * Call this after `cancel()` once the in-flight execution has finished and
   * you want to reuse the same instance without discarding shell or VFS state.
   */
  clearCancel(): void {
    this.native.clearCancel();
  }

  /**
   * Reset interpreter to fresh state, preserving configuration.
   */
  reset(): void {
    this.native.reset();
  }

  // Snapshot / Resume

  /**
   * Serialize interpreter state (variables, VFS, counters) to a Uint8Array.
   *
   * Use `hmacKey` when snapshots are stored outside the current trust boundary
   * (network, user uploads, shared storage). Without `hmacKey`, the snapshot
   * digest only detects accidental corruption and is forgeable.
   *
   * @example
   * ```typescript
   * const bash = new Bash();
   * await bash.execute("x=42");
   * const snapshot = bash.snapshot();
   * // persist snapshot...
   * const bash2 = Bash.fromSnapshot(snapshot);
   * const r = await bash2.execute("echo $x"); // "42\n"
   * ```
   */
  snapshot(options?: SnapshotOptions): Uint8Array {
    return this.native.snapshot(toNativeSnapshotOptions(options));
  }

  /**
   * Restore interpreter state from a previously captured snapshot.
   * Preserves current configuration (limits, builtins) but replaces
   * shell state and VFS contents.
   */
  restoreSnapshot(data: Uint8Array, options?: SnapshotOptions): void {
    this.native.restoreSnapshot(
      Buffer.from(data),
      toNativeSnapshotOptions(options),
    );
  }

  /**
   * Serialize interpreter state with HMAC-SHA256 using a caller-provided key.
   * Use for snapshots crossing trust boundaries.
   */
  snapshotKeyed(key: Uint8Array, options?: SnapshotOptions): Uint8Array {
    return this.native.snapshotKeyed(
      Buffer.from(key),
      toNativeSnapshotOptions(options),
    );
  }

  /**
   * Restore interpreter state from a HMAC-protected snapshot.
   */
  restoreSnapshotKeyed(data: Uint8Array, key: Uint8Array): void {
    this.native.restoreSnapshotKeyed(Buffer.from(data), Buffer.from(key));
  }

  /**
   * Create a new Bash instance from a snapshot.
   *
   * @example
   * ```typescript
   * const snapshot = existingBash.snapshot();
   * const restored = Bash.fromSnapshot(snapshot);
   * ```
   */
  static fromSnapshot(data: Uint8Array, options?: SnapshotOptions): Bash {
    const instance = new Bash();
    instance.native = NativeBash.fromSnapshot(
      Buffer.from(data),
      undefined,
      toNativeSnapshotOptions(options),
    );
    return instance;
  }

  /**
   * Create a new Bash instance from a HMAC-protected snapshot.
   */
  static fromSnapshotKeyed(data: Uint8Array, key: Uint8Array): Bash {
    const instance = new Bash();
    instance.native = NativeBash.fromSnapshotKeyed(
      Buffer.from(data),
      Buffer.from(key),
    );
    return instance;
  }

  // VFS — direct filesystem access

  /** Read a file from the virtual filesystem as a UTF-8 string. */
  readFile(path: string): string {
    return this.native.readFile(path);
  }

  /** Write a string to a file in the virtual filesystem. */
  writeFile(path: string, content: string): void {
    this.native.writeFile(path, content);
  }

  /** Create a directory. If recursive is true, creates parents as needed. */
  mkdir(path: string, recursive?: boolean): void {
    this.native.mkdir(path, recursive);
  }

  /** Check if a path exists in the virtual filesystem. */
  exists(path: string): boolean {
    return this.native.exists(path);
  }

  /** Remove a file or directory. If recursive is true, removes contents. */
  remove(path: string, recursive?: boolean): void {
    this.native.remove(path, recursive);
  }

  /** Get metadata for a path (fileType, size, mode, timestamps). */
  stat(path: string): {
    fileType: string;
    size: number;
    mode: number;
    modified: number;
    created: number;
  } {
    return this.native.stat(path);
  }

  /** Append content to a file. */
  appendFile(path: string, content: string): void {
    this.native.appendFile(path, content);
  }

  /** Change file permissions (octal mode, e.g. 0o755). */
  chmod(path: string, mode: number): void {
    this.native.chmod(path, mode);
  }

  /** Create a symbolic link pointing to target. */
  symlink(target: string, link: string): void {
    this.native.symlink(target, link);
  }

  /** Read the target of a symbolic link. */
  readLink(path: string): string {
    return this.native.readLink(path);
  }

  /** List directory entries with metadata. */
  readDir(path: string): Array<{
    name: string;
    metadata: {
      fileType: string;
      size: number;
      mode: number;
      modified: number;
      created: number;
    };
  }> {
    return this.native.readDir(path);
  }

  /** Get a FileSystem handle for direct VFS operations. */
  fs(): FileSystem {
    return FileSystem.fromNative(this.native.fs());
  }

  mount(vfsPath: string, fs: FileSystem): void;
  mount(hostPath: string, vfsPath: string, writable?: boolean): void;
  /** Mount either a host directory or a FileSystem into the VFS. */
  mount(
    hostPathOrVfsPath: string,
    vfsPathOrFs: string | FileSystem,
    writable?: boolean,
  ): void {
    if (isFileSystemLike(vfsPathOrFs)) {
      this.native.mountFileSystem(hostPathOrVfsPath, vfsPathOrFs.toExternal());
      return;
    }
    this.native.mount(hostPathOrVfsPath, vfsPathOrFs, writable);
  }

  /** Unmount a previously mounted filesystem. */
  unmount(vfsPath: string): void {
    this.native.unmount(vfsPath);
  }

  /**
   * List entry names in a directory. Returns empty array if directory does not exist.
   */
  ls(path?: string): string[] {
    const target = path ?? ".";
    try {
      return this.native.readDir(target).map((e: { name: string }) => e.name);
    } catch {
      return [];
    }
  }

  /**
   * Find files matching a name pattern. Returns absolute paths.
   */
  glob(pattern: string): string[] {
    // Reject patterns containing shell metacharacters to prevent injection.
    // Allow only safe glob characters: alphanumeric, *, ?, [], ., -, _, /
    if (/[^a-zA-Z0-9*?\[\]._ /-]/.test(pattern)) {
      return [];
    }
    const result = this.executeSync(
      `find / -name '${pattern}' -type f 2>/dev/null`,
    );
    if (result.exitCode !== 0) return [];
    return result.stdout
      .split("\n")
      .map((s) => s.trim())
      .filter((s) => s.length > 0);
  }

  /**
   * Capture a lightweight snapshot of shell state (variables, env, cwd,
   * arrays, aliases, traps) for inspection — e.g. prompt rendering or
   * debugging. Function definitions are omitted; use `snapshot()` for full
   * state capture/restore.
   */
  shellState(): ShellState {
    return this.native.shellState();
  }
}

/**
 * Bash interpreter with tool-contract metadata.
 *
 * Use this when integrating with AI frameworks that need tool definitions.
 *
 * @example
 * ```typescript
 * import { BashTool } from '@everruns/bashkit';
 *
 * const tool = new BashTool();
 * console.log(tool.name);           // "bashkit"
 * console.log(tool.inputSchema());  // JSON schema string
 * console.log(tool.help());         // Markdown help document
 *
 * const result = tool.executeSync('echo hello');
 * console.log(result.stdout);       // hello\n
 * ```
 */
export class BashTool {
  private native: NativeBashToolType;
  private maxInputBytes: number;

  constructor(options?: BashOptions) {
    const resolved = resolveFilesSync(options?.files);
    this.native = new NativeBashTool(toNativeOptions(options, resolved));
    this.maxInputBytes = options?.maxInputBytes ?? DEFAULT_MAX_INPUT_BYTES;
    registerCustomBuiltins(this.native, options?.customBuiltins);
  }

  /**
   * Create a BashTool instance with support for async file providers.
   */
  static async create(options?: BashOptions): Promise<BashTool> {
    const resolved = await resolveFiles(options?.files);
    const instance = Object.create(BashTool.prototype) as BashTool;
    instance.native = new NativeBashTool(toNativeOptions(options, resolved));
    instance.maxInputBytes = options?.maxInputBytes ?? DEFAULT_MAX_INPUT_BYTES;
    registerCustomBuiltins(instance.native, options?.customBuiltins);
    return instance;
  }

  /** Register a JS callback as a custom builtin. See {@link Bash.addBuiltin}. */
  addBuiltin(name: string, callback: BuiltinCallback): void {
    this.native.addBuiltin(name, makeBuiltinDispatcher(callback));
  }

  /** Remove a previously registered custom builtin. */
  removeBuiltin(name: string): void {
    this.native.removeBuiltin(name);
  }

  /**
   * Execute bash commands synchronously and return the result.
   *
   * If `onOutput` is provided, it must be synchronous; Promise-returning
   * handlers are rejected. Do not re-enter the same instance from `onOutput`
   * via `execute*`, `readFile`, `fs()`, etc.
   */
  executeSync(commands: string, options?: ExecuteOptions): ExecResult {
    const nativeOnOutput = toNativeOnOutput(options?.onOutput);
    if (options?.signal) {
      const signal = options.signal;
      if (signal.aborted) {
        return cancelledExecResult();
      }
      const onAbort = () => this.native.cancel();
      signal.addEventListener("abort", onAbort, { once: true });
      try {
        return this.native.executeSync(commands, nativeOnOutput);
      } finally {
        signal.removeEventListener("abort", onAbort);
        if (signal.aborted) {
          this.native.clearCancel();
        }
      }
    }
    return this.native.executeSync(commands, nativeOnOutput);
  }

  /**
   * Execute bash commands asynchronously, returning a Promise.
   *
   * If `onOutput` is provided, it must be synchronous; Promise-returning
   * handlers are rejected. Do not re-enter the same instance from `onOutput`
   * via `execute*`, `readFile`, `fs()`, etc.
   */
  async execute(
    commands: string,
    options?: ExecuteOptions,
  ): Promise<ExecResult> {
    const nativeOnOutput = toNativeOnOutput(options?.onOutput);
    const signal = options?.signal;
    if (signal?.aborted) {
      return cancelledExecResult();
    }
    const inputLimitResult = inputTooLargeExecResult(
      commands,
      this.maxInputBytes,
    );
    if (inputLimitResult) {
      return inputLimitResult;
    }
    return queueAsyncExecute(this, async () => {
      if (signal?.aborted) {
        return cancelledExecResult();
      }
      if (signal) {
        let signalTriggered = false;
        const onAbort = () => {
          signalTriggered = true;
          this.native.cancel();
        };
        signal.addEventListener("abort", onAbort, { once: true });
        try {
          if (nativeOnOutput) {
            return await this.native.executeWithOutput(
              commands,
              nativeOnOutput,
            );
          }
          return await this.native.execute(commands);
        } finally {
          signal.removeEventListener("abort", onAbort);
          if (signalTriggered) {
            this.native.clearCancel();
          }
        }
      }
      if (nativeOnOutput) {
        return this.native.executeWithOutput(commands, nativeOnOutput);
      }
      return this.native.execute(commands);
    });
  }

  /**
   * Execute bash commands synchronously. Throws `BashError` on non-zero exit.
   */
  executeSyncOrThrow(commands: string, options?: ExecuteOptions): ExecResult {
    const result = this.executeSync(commands, options);
    if (result.exitCode !== 0) {
      throw new BashError(result);
    }
    return result;
  }

  /**
   * Execute bash commands asynchronously. Throws `BashError` on non-zero exit.
   */
  async executeOrThrow(
    commands: string,
    options?: ExecuteOptions,
  ): Promise<ExecResult> {
    const result = await this.execute(commands, options);
    if (result.exitCode !== 0) {
      throw new BashError(result);
    }
    return result;
  }

  /**
   * Cancel the currently running execution.
   */
  cancel(): void {
    this.native.cancel();
  }

  /**
   * Clear the cancellation flag so subsequent executions proceed normally.
   *
   * Call this after `cancel()` once the in-flight execution has finished and
   * you want to reuse the same instance without discarding shell or VFS state.
   */
  clearCancel(): void {
    this.native.clearCancel();
  }

  /**
   * Reset interpreter to fresh state, preserving configuration.
   */
  reset(): void {
    this.native.reset();
  }

  /**
   * Serialize interpreter state (variables, VFS, counters) to an
   * HMAC-authenticated Uint8Array. BashTool snapshots require `hmacKey` because
   * they include tenant-controlled shell state, VFS contents, and counters.
   */
  snapshot(options: SnapshotOptions & { hmacKey: Uint8Array }): Uint8Array {
    requireSnapshotHmacKey(options);
    return this.native.snapshot(toNativeSnapshotOptions(options));
  }

  /**
   * Restore interpreter state from an HMAC-authenticated snapshot.
   * Preserves current configuration (limits, identity) but replaces
   * shell state and VFS contents.
   */
  restoreSnapshot(
    data: Uint8Array,
    options: SnapshotOptions & { hmacKey: Uint8Array },
  ): void {
    requireSnapshotHmacKey(options);
    this.native.restoreSnapshot(
      Buffer.from(data),
      toNativeSnapshotOptions(options),
    );
  }

  /**
   * Serialize interpreter state with HMAC-SHA256 using a caller-provided key.
   */
  snapshotKeyed(key: Uint8Array, options?: SnapshotOptions): Uint8Array {
    return this.native.snapshotKeyed(
      Buffer.from(key),
      toNativeSnapshotOptions(options),
    );
  }

  /**
   * Restore interpreter state from a HMAC-protected snapshot.
   */
  restoreSnapshotKeyed(data: Uint8Array, key: Uint8Array): void {
    this.native.restoreSnapshotKeyed(Buffer.from(data), Buffer.from(key));
  }

  /**
   * Create a new BashTool instance from an HMAC-authenticated snapshot.
   *
   * Any provided Bash options are applied before restoring the snapshot so
   * limits and identity settings survive round-trips.
   */
  static fromSnapshot(
    data: Uint8Array,
    options: BashOptions | undefined,
    snapshotOptions: SnapshotOptions & { hmacKey: Uint8Array },
  ): BashTool {
    requireSnapshotHmacKey(snapshotOptions);
    const resolved = resolveFilesSync(options?.files);
    const instance = Object.create(BashTool.prototype) as BashTool;
    instance.native = NativeBashTool.fromSnapshot(
      Buffer.from(data),
      toNativeOptions(options, resolved),
      toNativeSnapshotOptions(snapshotOptions),
    );
    return instance;
  }

  /**
   * Create a new BashTool instance from a HMAC-protected snapshot.
   */
  static fromSnapshotKeyed(
    data: Uint8Array,
    key: Uint8Array,
    options?: BashOptions,
  ): BashTool {
    const resolved = resolveFilesSync(options?.files);
    const instance = Object.create(BashTool.prototype) as BashTool;
    instance.native = NativeBashTool.fromSnapshotKeyed(
      Buffer.from(data),
      Buffer.from(key),
      toNativeOptions(options, resolved),
    );
    return instance;
  }

  // ==========================================================================
  // VFS file helpers
  // ==========================================================================

  /**
   * Check whether a path exists in the virtual filesystem.
   */
  exists(path: string): boolean {
    try {
      return this.native.exists(path);
    } catch {
      return false;
    }
  }

  /**
   * Read file contents from the virtual filesystem.
   * Throws `BashError` if the file does not exist.
   */
  readFile(path: string): string {
    return this.native.readFile(path);
  }

  /**
   * Write content to a file in the virtual filesystem.
   * Creates parent directories as needed.
   */
  writeFile(path: string, content: string): void {
    // Ensure parent directory exists (matches prior shell-based behavior)
    const lastSlash = path.lastIndexOf("/");
    if (lastSlash > 0) {
      const parent = path.slice(0, lastSlash);
      try {
        this.native.mkdir(parent, true);
      } catch {
        // parent may already exist — ignore
      }
    }
    this.native.writeFile(path, content);
  }

  /** Create a directory. If recursive is true, creates parents as needed. */
  mkdir(path: string, recursive?: boolean): void {
    this.native.mkdir(path, recursive);
  }

  /** Remove a file or directory. If recursive is true, removes contents. */
  remove(path: string, recursive?: boolean): void {
    this.native.remove(path, recursive);
  }

  /** Get metadata for a path (fileType, size, mode, timestamps). */
  stat(path: string): {
    fileType: string;
    size: number;
    mode: number;
    modified: number;
    created: number;
  } {
    return this.native.stat(path);
  }

  /** Append content to a file. */
  appendFile(path: string, content: string): void {
    this.native.appendFile(path, content);
  }

  /** Change file permissions (octal mode, e.g. 0o755). */
  chmod(path: string, mode: number): void {
    this.native.chmod(path, mode);
  }

  /** Create a symbolic link pointing to target. */
  symlink(target: string, link: string): void {
    this.native.symlink(target, link);
  }

  /** Read the target of a symbolic link. */
  readLink(path: string): string {
    return this.native.readLink(path);
  }

  /** List directory entries with metadata. */
  readDir(path: string): Array<{
    name: string;
    metadata: {
      fileType: string;
      size: number;
      mode: number;
      modified: number;
      created: number;
    };
  }> {
    return this.native.readDir(path);
  }

  /** Get a FileSystem handle for direct VFS operations. */
  fs(): FileSystem {
    return FileSystem.fromNative(this.native.fs());
  }

  mount(vfsPath: string, fs: FileSystem): void;
  mount(hostPath: string, vfsPath: string, writable?: boolean): void;
  /** Mount either a host directory or a FileSystem into the VFS. */
  mount(
    hostPathOrVfsPath: string,
    vfsPathOrFs: string | FileSystem,
    writable?: boolean,
  ): void {
    if (isFileSystemLike(vfsPathOrFs)) {
      this.native.mountFileSystem(hostPathOrVfsPath, vfsPathOrFs.toExternal());
      return;
    }
    this.native.mount(hostPathOrVfsPath, vfsPathOrFs, writable);
  }

  /** Unmount a previously mounted filesystem. */
  unmount(vfsPath: string): void {
    this.native.unmount(vfsPath);
  }

  /**
   * List entry names in a directory. Returns empty array if directory does not exist.
   */
  ls(path?: string): string[] {
    const target = path ?? ".";
    try {
      return this.native.readDir(target).map((e: { name: string }) => e.name);
    } catch {
      return [];
    }
  }

  /**
   * Find files matching a name pattern. Returns absolute paths.
   */
  glob(pattern: string): string[] {
    // Reject patterns containing shell metacharacters to prevent injection.
    // Allow only safe glob characters: alphanumeric, *, ?, [], ., -, _, /
    if (/[^a-zA-Z0-9*?\[\]._ /-]/.test(pattern)) {
      return [];
    }
    const result = this.executeSync(
      `find / -name '${pattern}' -type f 2>/dev/null`,
    );
    if (result.exitCode !== 0) return [];
    return result.stdout
      .split("\n")
      .map((s) => s.trim())
      .filter((s) => s.length > 0);
  }

  /**
   * Capture a lightweight snapshot of shell state (variables, env, cwd,
   * arrays, aliases, traps) for inspection — e.g. prompt rendering or
   * debugging. Function definitions are omitted; use `snapshot()` for full
   * state capture/restore.
   */
  shellState(): ShellState {
    return this.native.shellState();
  }

  /** Tool name. */
  get name(): string {
    return this.native.name;
  }

  /** Short description. */
  get shortDescription(): string {
    return this.native.shortDescription;
  }

  /** Token-efficient tool description. */
  description(): string {
    return this.native.description();
  }

  /** Markdown help document. */
  help(): string {
    return this.native.help();
  }

  /** Compact system prompt for orchestration. */
  systemPrompt(): string {
    return this.native.systemPrompt();
  }

  /** JSON input schema as string. */
  inputSchema(): string {
    return this.native.inputSchema();
  }

  /** JSON output schema as string. */
  outputSchema(): string {
    return this.native.outputSchema();
  }

  /** Tool version. */
  get version(): string {
    return this.native.version;
  }
}

/**
 * Options for creating a ScriptedTool instance.
 */
export interface ScriptedToolOptions {
  name: string;
  shortDescription?: string;
  maxCommands?: number;
  maxLoopIterations?: number;
}

/**
 * Callback type for ScriptedTool tool commands.
 *
 * Receives parsed `--key value` flags as `params` and optional piped input as `stdin`.
 * Must return a string.
 */
export type ToolCallback = (
  params: Record<string, unknown>,
  stdin: string | null,
) => string;

/**
 * Compose JS callbacks as bash builtins for multi-tool orchestration.
 *
 * Each registered tool becomes a bash builtin command. An LLM (or user) writes
 * a single bash script that pipes, loops, and branches across all tools.
 *
 * @example
 * ```typescript
 * import { ScriptedTool } from '@everruns/bashkit';
 *
 * const tool = new ScriptedTool({ name: "api" });
 * tool.addTool("greet", "Greet user",
 *   (params) => `hello ${params.name ?? "world"}\n`
 * );
 * const result = tool.executeSync("greet --name Alice");
 * console.log(result.stdout); // hello Alice\n
 * ```
 */
export class ScriptedTool {
  private native: NativeScriptedToolType;
  // Keep strong JS refs while native TSFN callbacks are weak.
  private callbackRefs: Array<(requestJson: string) => string> = [];

  constructor(options: ScriptedToolOptions) {
    this.native = new NativeScriptedTool({
      name: options.name,
      shortDescription: options.shortDescription,
      maxCommands: options.maxCommands,
      maxLoopIterations: options.maxLoopIterations,
    });
  }

  /**
   * Register a tool command.
   *
   * @param name - Command name (becomes a bash builtin)
   * @param description - Human-readable description
   * @param callback - JS function `(params, stdin) => string`
   * @param schema - Optional JSON Schema for input parameters
   */
  addTool(
    name: string,
    description: string,
    callback: ToolCallback,
    schema?: Record<string, unknown>,
  ): void {
    if (schema) {
      validateJsonNestingDepth(schema);
    }
    // Wrap the user callback to handle JSON serialization protocol
    const wrappedCallback = (requestJson: string): string => {
      const request = JSON.parse(requestJson) as {
        params: Record<string, unknown>;
        stdin: string | null;
      };
      return callback(request.params, request.stdin);
    };
    this.callbackRefs.push(wrappedCallback);
    this.native.addTool(
      name,
      description,
      wrappedCallback,
      schema ? JSON.stringify(schema) : undefined,
    );
  }

  /**
   * Add an environment variable visible inside scripts.
   */
  env(key: string, value: string): void {
    this.native.env(key, value);
  }

  /**
   * Execute a bash script synchronously.
   *
   * Note: ScriptedTool callbacks run asynchronously via Node's event loop.
   * If a registered tool is invoked, this method returns a non-zero result
   * instead of queueing a callback that would deadlock. Use `execute()`
   * (async) for scripts that call registered tools. Only use this for scripts
   * that don't invoke any registered tools (e.g., pure bash).
   */
  executeSync(commands: string): ExecResult {
    return this.native.executeSync(commands);
  }

  /**
   * Execute a bash script asynchronously, returning a Promise.
   *
   * This is the recommended execution method for ScriptedTool since
   * tool callbacks require the Node.js event loop to be running.
   */
  async execute(commands: string): Promise<ExecResult> {
    const inputLimitResult = inputTooLargeExecResult(
      commands,
      DEFAULT_MAX_INPUT_BYTES,
    );
    if (inputLimitResult) {
      return inputLimitResult;
    }
    return queueAsyncExecute(this, () => this.native.execute(commands));
  }

  /**
   * Execute synchronously. Throws `BashError` on non-zero exit.
   *
   * Same caveats as `executeSync()` — throws when a registered tool would
   * require the blocked Node event loop. Use `executeOrThrow()` instead.
   */
  executeSyncOrThrow(commands: string): ExecResult {
    const result = this.native.executeSync(commands);
    if (result.exitCode !== 0) {
      throw new BashError(result);
    }
    return result;
  }

  /**
   * Execute asynchronously. Throws `BashError` on non-zero exit.
   */
  async executeOrThrow(commands: string): Promise<ExecResult> {
    const result = await this.execute(commands);
    if (result.exitCode !== 0) {
      throw new BashError(result);
    }
    return result;
  }

  /** Tool name. */
  get name(): string {
    return this.native.name;
  }

  /** Short description. */
  get shortDescription(): string {
    return this.native.shortDescription;
  }

  /** Number of registered tools. */
  toolCount(): number {
    return this.native.toolCount();
  }

  /** Token-efficient tool description. */
  description(): string {
    return this.native.description();
  }

  /** Markdown help document. */
  help(): string {
    return this.native.help();
  }

  /** Compact system prompt for orchestration. */
  systemPrompt(): string {
    return this.native.systemPrompt();
  }

  /** JSON input schema as string. */
  inputSchema(): string {
    return this.native.inputSchema();
  }

  /** JSON output schema as string. */
  outputSchema(): string {
    return this.native.outputSchema();
  }

  /** Tool version. */
  get version(): string {
    return this.native.version;
  }
}

/**
 * Get the bashkit version string.
 */
export function getVersion(): string {
  return nativeGetVersion();
}
