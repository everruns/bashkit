import { createRequire } from "node:module";
import type {
  Bash as NativeBashType,
  BashTool as NativeBashToolType,
  ExecResult,
  BashOptions as NativeBashOptions,
} from "./index.cjs";

const require = createRequire(import.meta.url);
const native = require("./index.cjs");
const NativeBash: typeof NativeBashType = native.Bash;
const NativeBashTool: typeof NativeBashToolType = native.BashTool;
const nativeGetVersion: () => string = native.getVersion;

export type { ExecResult };

/**
 * A file value: either a string, a sync function returning a string,
 * or an async function returning a Promise<string>.
 *
 * Function values are resolved lazily on first read and cached.
 */
export type FileValue = string | (() => string) | (() => Promise<string>);

/**
 * Options for creating a Bash or BashTool instance.
 */
export interface BashOptions {
  username?: string;
  hostname?: string;
  maxCommands?: number;
  maxLoopIterations?: number;
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
    files: resolvedFiles,
  };
}

/**
 * Generate a heredoc write command with a randomized delimiter to prevent injection.
 */
function buildWriteCmd(filePath: string, content: string): string {
  const hex = Array.from(crypto.getRandomValues(new Uint8Array(8)))
    .map((b) => b.toString(16).padStart(2, "0"))
    .join("");
  const delimiter = `BASHKIT_EOF_${hex}`;
  // Single-quote the delimiter to prevent expansion inside the heredoc
  return `mkdir -p "$(dirname '${filePath.replace(/'/g, "'\\''")}')" && cat > '${filePath.replace(/'/g, "'\\''")}' << '${delimiter}'\n${content}\n${delimiter}`;
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
  executeSync(commands: string, options?: { signal?: AbortSignal }): ExecResult {
    if (options?.signal) {
      const signal = options.signal;
      if (signal.aborted) {
        return { stdout: "", stderr: "", exitCode: 1, error: "execution cancelled", stdoutTruncated: false, stderrTruncated: false };
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
  executeSyncOrThrow(commands: string, options?: { signal?: AbortSignal }): ExecResult {
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

  /**
   * List entries in a directory. Returns empty array if directory does not exist.
   */
  ls(path?: string): string[] {
    const target = path ?? ".";
    const result = this.executeSync(`ls '${target.replace(/'/g, "'\\''")}'`);
    if (result.exitCode !== 0) return [];
    return result.stdout
      .split("\n")
      .map((s) => s.trim())
      .filter((s) => s.length > 0);
  }

  /**
   * Find files matching a name pattern. Returns absolute paths.
   */
  glob(pattern: string): string[] {
    const result = this.executeSync(`find / -name '${pattern.replace(/'/g, "'\\''")}' -type f 2>/dev/null`);
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
  executeSync(commands: string, options?: { signal?: AbortSignal }): ExecResult {
    if (options?.signal) {
      const signal = options.signal;
      if (signal.aborted) {
        return { stdout: "", stderr: "", exitCode: 1, error: "execution cancelled", stdoutTruncated: false, stderrTruncated: false };
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
  executeSyncOrThrow(commands: string, options?: { signal?: AbortSignal }): ExecResult {
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
    return this.executeSync(`test -e '${path.replace(/'/g, "'\\''")}'`).exitCode === 0;
  }

  /**
   * Read file contents from the virtual filesystem.
   * Throws `BashError` if the file does not exist.
   */
  readFile(path: string): string {
    const result = this.executeSyncOrThrow(`cat '${path.replace(/'/g, "'\\''")}'`);
    return result.stdout;
  }

  /**
   * Write content to a file in the virtual filesystem.
   * Creates parent directories as needed.
   */
  writeFile(path: string, content: string): void {
    this.executeSyncOrThrow(buildWriteCmd(path, content));
  }

  /**
   * List entries in a directory. Returns empty array if directory does not exist.
   */
  ls(path?: string): string[] {
    const target = path ?? ".";
    const result = this.executeSync(`ls '${target.replace(/'/g, "'\\''")}'`);
    if (result.exitCode !== 0) return [];
    return result.stdout
      .split("\n")
      .map((s) => s.trim())
      .filter((s) => s.length > 0);
  }

  /**
   * Find files matching a name pattern. Returns absolute paths.
   */
  glob(pattern: string): string[] {
    const result = this.executeSync(`find / -name '${pattern.replace(/'/g, "'\\''")}' -type f 2>/dev/null`);
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
 * Get the bashkit version string.
 */
export function getVersion(): string {
  return nativeGetVersion();
}
