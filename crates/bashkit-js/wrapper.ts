import { createRequire } from "node:module";
import type {
  Bash as NativeBashType,
  BashTool as NativeBashToolType,
  ScriptedTool as NativeScriptedToolType,
  ExecResult,
  BashOptions as NativeBashOptions,
} from "./index.cjs";

const require = createRequire(import.meta.url);
const native = require("./index.cjs");
const NativeBash: typeof NativeBashType = native.Bash;
const NativeBashTool: typeof NativeBashToolType = native.BashTool;
const NativeScriptedTool: typeof NativeScriptedToolType = native.ScriptedTool;
const nativeGetVersion: () => string = native.getVersion;

export type { ExecResult };

/**
 * A file value: either a string, a sync function returning a string,
 * or an async function returning a Promise<string>.
 *
 * Function values are resolved lazily on first read and cached.
 */
export type FileValue = string | (() => string) | (() => Promise<string>);

const MAX_JSON_NESTING_DEPTH = 64;

/**
 * Options for creating a Bash or BashTool instance.
 */
export interface BashOptions {
  username?: string;
  hostname?: string;
  maxCommands?: number;
  maxLoopIterations?: number;
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
   * Enable embedded Python execution (`python`/`python3` builtins).
   *
   * When true, bash scripts can use `python -c '...'` or `python3 script.py`
   * to run Python code within the sandbox.
   */
  python?: boolean;
  /**
   * Names of external functions callable from embedded Python code.
   *
   * These function names become available as Python builtins within
   * the embedded interpreter. When called, they invoke the external handler.
   */
  externalFunctions?: string[];
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
    maxMemory: options?.maxMemory,
    timeoutMs: options?.timeoutMs,
    files: resolvedFiles,
    mounts: options?.mounts?.map((m) => ({
      hostPath: m.root,
      vfsPath: m.path,
      writable: m.writable,
    })),
    python: options?.python,
    externalFunctions: options?.externalFunctions,
  };
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

  constructor(options?: BashOptions) {
    const resolved = resolveFilesSync(options?.files);
    this.native = new NativeBash(toNativeOptions(options, resolved));
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
    return instance;
  }

  /**
   * Execute bash commands synchronously and return the result.
   *
   * If `signal` is provided, the execution will be cancelled when the signal
   * is aborted. The result will have `error: "execution cancelled"`.
   */
  executeSync(
    commands: string,
    options?: { signal?: AbortSignal },
  ): ExecResult {
    if (options?.signal) {
      const signal = options.signal;
      if (signal.aborted) {
        return {
          stdout: "",
          stderr: "",
          exitCode: 1,
          error: "execution cancelled",
          stdoutTruncated: false,
          stderrTruncated: false,
          finalEnv: undefined,
          success: false,
        };
      }
      const onAbort = () => this.native.cancel();
      signal.addEventListener("abort", onAbort, { once: true });
      try {
        return this.native.executeSync(commands);
      } finally {
        signal.removeEventListener("abort", onAbort);
      }
    }
    return this.native.executeSync(commands);
  }

  /**
   * Execute bash commands asynchronously, returning a Promise.
   *
   * Non-blocking for the Node.js event loop.
   *
   * @example
   * ```typescript
   * const result = await bash.execute('echo hello');
   * console.log(result.stdout); // hello\n
   * ```
   */
  async execute(commands: string): Promise<ExecResult> {
    return this.native.execute(commands);
  }

  /**
   * Execute bash commands synchronously. Throws `BashError` on non-zero exit.
   */
  executeSyncOrThrow(
    commands: string,
    options?: { signal?: AbortSignal },
  ): ExecResult {
    const result = this.executeSync(commands, options);
    if (result.exitCode !== 0) {
      throw new BashError(result);
    }
    return result;
  }

  /**
   * Execute bash commands asynchronously. Throws `BashError` on non-zero exit.
   */
  async executeOrThrow(commands: string): Promise<ExecResult> {
    const result = await this.native.execute(commands);
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
   * Reset interpreter to fresh state, preserving configuration.
   */
  reset(): void {
    this.native.reset();
  }

  // Snapshot / Resume

  /**
   * Serialize interpreter state (variables, VFS, counters) to a Uint8Array.
   *
   * The snapshot can be persisted to disk, sent over the network, and later
   * used with `Bash.fromSnapshot()` to restore the session.
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
  snapshot(): Uint8Array {
    return this.native.snapshot();
  }

  /**
   * Restore interpreter state from a previously captured snapshot.
   * Preserves current configuration (limits, builtins) but replaces
   * shell state and VFS contents.
   */
  restoreSnapshot(data: Uint8Array): void {
    this.native.restoreSnapshot(Buffer.from(data));
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
  static fromSnapshot(data: Uint8Array): Bash {
    const instance = new Bash();
    instance.native = NativeBash.fromSnapshot(Buffer.from(data));
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
  readDir(
    path: string,
  ): Array<{
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

  /** Get a JsFileSystem handle for direct VFS operations. */
  fs(): any {
    return this.native.fs();
  }

  /** Mount a host directory into the VFS. Read-only by default; pass writable: true to enable writes. */
  mount(hostPath: string, vfsPath: string, writable?: boolean): void {
    this.native.mount(hostPath, vfsPath, writable);
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

  constructor(options?: BashOptions) {
    const resolved = resolveFilesSync(options?.files);
    this.native = new NativeBashTool(toNativeOptions(options, resolved));
  }

  /**
   * Create a BashTool instance with support for async file providers.
   */
  static async create(options?: BashOptions): Promise<BashTool> {
    const resolved = await resolveFiles(options?.files);
    const instance = Object.create(BashTool.prototype) as BashTool;
    instance.native = new NativeBashTool(toNativeOptions(options, resolved));
    return instance;
  }

  /**
   * Execute bash commands synchronously and return the result.
   */
  executeSync(
    commands: string,
    options?: { signal?: AbortSignal },
  ): ExecResult {
    if (options?.signal) {
      const signal = options.signal;
      if (signal.aborted) {
        return {
          stdout: "",
          stderr: "",
          exitCode: 1,
          error: "execution cancelled",
          stdoutTruncated: false,
          stderrTruncated: false,
          finalEnv: undefined,
          success: false,
        };
      }
      const onAbort = () => this.native.cancel();
      signal.addEventListener("abort", onAbort, { once: true });
      try {
        return this.native.executeSync(commands);
      } finally {
        signal.removeEventListener("abort", onAbort);
      }
    }
    return this.native.executeSync(commands);
  }

  /**
   * Execute bash commands asynchronously, returning a Promise.
   */
  async execute(commands: string): Promise<ExecResult> {
    return this.native.execute(commands);
  }

  /**
   * Execute bash commands synchronously. Throws `BashError` on non-zero exit.
   */
  executeSyncOrThrow(
    commands: string,
    options?: { signal?: AbortSignal },
  ): ExecResult {
    const result = this.executeSync(commands, options);
    if (result.exitCode !== 0) {
      throw new BashError(result);
    }
    return result;
  }

  /**
   * Execute bash commands asynchronously. Throws `BashError` on non-zero exit.
   */
  async executeOrThrow(commands: string): Promise<ExecResult> {
    const result = await this.native.execute(commands);
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
   * Reset interpreter to fresh state, preserving configuration.
   */
  reset(): void {
    this.native.reset();
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
  readDir(
    path: string,
  ): Array<{
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

  /** Get a JsFileSystem handle for direct VFS operations. */
  fs(): any {
    return this.native.fs();
  }

  /** Mount a host directory into the VFS. Read-only by default; pass writable: true to enable writes. */
  mount(hostPath: string, vfsPath: string, writable?: boolean): void {
    this.native.mount(hostPath, vfsPath, writable);
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
   * This method will deadlock if any registered tool callback is invoked.
   * Use `execute()` (async) instead for scripts that call registered tools.
   * Only use this for scripts that don't invoke any registered tools
   * (e.g., pure bash without tool calls).
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
    return this.native.execute(commands);
  }

  /**
   * Execute synchronously. Throws `BashError` on non-zero exit.
   *
   * Same caveats as `executeSync()` — use `executeOrThrow()` instead.
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
    const result = await this.native.execute(commands);
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
