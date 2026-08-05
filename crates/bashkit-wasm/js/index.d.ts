// Public TypeScript surface for @everruns/bashkit-wasm.

/** A live handle to a `Bash` instance's virtual filesystem. */
export interface FileSystem {
  readFile(path: string): string;
  readFileBytes(path: string): Uint8Array;
  writeFile(path: string, content: string): void;
  writeFileBytes(path: string, content: Uint8Array): void;
  appendFile(path: string, content: string): void;
  appendFileBytes(path: string, content: Uint8Array): void;
  exists(path: string): boolean;
  mkdir(path: string): void;
  remove(path: string): void;
  ls(path: string): string[];
}

/** Payload passed to a custom-builtin callback. */
export interface BuiltinRequest {
  /** Command name as invoked. */
  readonly name: string;
  /** Arguments, not including the command name. */
  readonly argv: string[];
  /** Piped stdin, or `null` when the builtin was not on the right of a pipe. */
  readonly stdin: string | null;
  /** Exported environment variables. */
  readonly env: Record<string, string>;
  /** Current working directory. */
  readonly cwd: string;
  /** Live handle to the interpreter's virtual filesystem (shared with the script). */
  readonly fs: FileSystem;
}

/**
 * A JS callback registered as a bash builtin. Return the builtin's stdout, or a
 * `Promise` of it. Async callbacks are only awaited by {@link Bash.execute}
 * (not `executeSync`). Throwing / rejecting becomes stderr with exit code 1.
 */
export type CustomBuiltin = (
  ctx: BuiltinRequest,
) => string | Promise<string>;

/** Closed set of named execution-policy baselines. */
export type ExecutionProfileName = "hardened" | "standard" | "interactive";

export declare const ExecutionProfile: Readonly<{
  Hardened: "hardened";
  Standard: "standard";
  Interactive: "interactive";
}>;

/** Entry kind reported by a {@link HostFileSystem}. */
export type HostFileType = "file" | "dir" | "directory" | "symlink" | "fifo";

/** Metadata a {@link HostFileSystem} reports for one entry. */
export interface HostStat {
  readonly type: HostFileType;
  /** Size in bytes. Defaults to 0. */
  readonly size?: number;
  /** Unix permission bits. Defaults to 0o644 for files, 0o755 for directories. */
  readonly mode?: number;
  /** Last modification time, ms since the epoch. Defaults to now. */
  readonly mtimeMs?: number;
}

/** One directory entry: a {@link HostStat} plus its name. */
export interface HostDirent extends HostStat {
  readonly name: string;
}

/**
 * A filesystem the embedder owns, used in place of the built-in in-memory VFS.
 *
 * Every method may return its value directly or as a `Promise`. Because a host
 * call can suspend the interpreter, a `Bash` constructed with `fs` must be
 * driven with {@link Bash.execute}; {@link Bash.executeSync} and the
 * synchronous `bash.readFile(...)` helpers report the suspension instead of
 * blocking.
 *
 * The host implements raw storage only. POSIX semantics — parent-directory
 * checks, "is a directory" errors, symlink resolution — are enforced above it,
 * so hosts stay small.
 *
 * To surface a bash-accurate error, throw (or reject with) an `Error` carrying
 * a `code` property: `ENOENT`, `EEXIST`, `EACCES`, `EPERM`, `EISDIR`,
 * `ENOTDIR`, `ENOTEMPTY`, `EXDEV`, or `ENOSYS`. Anything else becomes a generic
 * I/O error carrying the thrown message.
 */
export interface HostFileSystem {
  /** Read raw bytes. Throw `ENOENT` when absent. */
  read(path: string): Uint8Array | string | Promise<Uint8Array | string>;
  /** Write raw bytes, replacing any existing content. */
  write(path: string, content: Uint8Array): void | Promise<void>;
  /** Create a directory, with parents when `recursive`. */
  mkdir(path: string, recursive: boolean): void | Promise<void>;
  /** Remove an entry, with contents when `recursive`. */
  remove(path: string, recursive: boolean): void | Promise<void>;
  /** Metadata for one entry. Throw `ENOENT` when absent. */
  stat(path: string): HostStat | Promise<HostStat>;
  /** Directory listing. Throw `ENOENT` when absent. */
  readDir(path: string): HostDirent[] | Promise<HostDirent[]>;
  /** Whether a path exists. Must not throw for a plain miss. */
  exists(path: string): boolean | Promise<boolean>;

  /** Optional. Synthesized as read-modify-write when omitted. */
  append?(path: string, content: Uint8Array): void | Promise<void>;
  /** Optional. Synthesized as read-then-write when omitted. */
  copy?(from: string, to: string): void | Promise<void>;
  /** Optional. Synthesized as copy-then-remove when omitted. */
  rename?(from: string, to: string): void | Promise<void>;
  /** Optional. Scripts that create symlinks fail with `ENOSYS` when omitted. */
  symlink?(target: string, link: string): void | Promise<void>;
  /** Optional. Reading a symlink fails with `ENOSYS` when omitted. */
  readLink?(path: string): string | Promise<string>;
  /** Optional. Accepted and ignored when omitted, so `chmod +x` still works. */
  chmod?(path: string, mode: number): void | Promise<void>;
}

/** Options for constructing a {@link Bash} instance. */
export interface BashOptions {
  /** Resource-policy baseline; individual limit options override it. */
  profile?: ExecutionProfileName;
  username?: string;
  hostname?: string;
  /** Initial working directory (avoids a leading `cd`). */
  cwd?: string;
  /** Initial environment variables. */
  env?: Record<string, string>;
  /** Max commands per execution (resource limit). */
  maxCommands?: number;
  /** Max iterations per loop (resource limit). */
  maxLoopIterations?: number;
  /** Wall-clock execution deadline in milliseconds. */
  timeoutMs?: number;
  /** Max interpreter memory in bytes. */
  maxMemory?: number;
  /** Pre-created files seeded into the virtual filesystem (string contents). */
  files?: Record<string, string>;
  /**
   * Filesystem the embedder owns, used instead of the built-in in-memory VFS.
   * Cannot be combined with {@link BashOptions.files} — seed through the host
   * instead. Requires {@link Bash.execute} (not `executeSync`).
   */
  fs?: HostFileSystem;
  /** JS callbacks registered as bash builtins. */
  customBuiltins?: Record<string, CustomBuiltin>;
}

/** Result of a bash execution. */
export interface ExecResult {
  readonly stdout: string;
  /** Exact stdout bytes. Use this for binary output. */
  readonly stdoutBytes: Uint8Array;
  readonly stderr: string;
  /** Exact stderr bytes. Use this for binary output. */
  readonly stderrBytes: Uint8Array;
  readonly exitCode: number;
  readonly success: boolean;
  readonly stdoutTruncated: boolean;
  readonly stderrTruncated: boolean;
}

export interface AnalyzedCommand {
  readonly name: string | null;
  readonly args: Array<string | null>;
  readonly context: "direct" | "substitution" | "function_body";
  readonly assignments: string[];
  readonly isAssignmentOnly: boolean;
}

export interface AnalyzedRedirect {
  readonly path: string | null;
  readonly mode: "read" | "write" | "append";
  readonly isWrite: boolean;
}

export interface ScriptAnalysis {
  readonly commands: AnalyzedCommand[];
  readonly redirects: AnalyzedRedirect[];
  readonly functions: string[];
  readonly commandNames: string[];
  readonly hasDynamicCommands: boolean;
  readonly hasCommandSubstitution: boolean;
  readonly hasInterpreterReentry: boolean;
  readonly truncated: boolean;
  readonly isOpaque: boolean;
}

export interface CommitOptions {
  parents?: string[];
  meta?: Record<string, string>;
  have?: string[];
  excludeFilesystem?: boolean;
  excludeFunctions?: boolean;
}

export interface PackedCommit {
  readonly id: string;
  readonly objectCount: number;
  readonly storedBytes: number;
  readonly selfContained: boolean;
  readonly packed?: Uint8Array;
  readonly objects: Record<string, Uint8Array>;
}

export type CheckoutPolicy = "strict" | "superset" | "force";

/** Sandboxed bash interpreter running entirely in the browser. */
export class Bash {
  constructor(options?: BashOptions);
  /**
   * Execute synchronously. Valid only for scripts that complete without
   * suspending (plain bash and `jq`). Throws if the script invokes an async
   * custom builtin or otherwise yields — use {@link Bash.execute} instead.
   */
  executeSync(commands: string): ExecResult;
  /** Execute asynchronously; supports async custom builtins. */
  execute(commands: string): Promise<ExecResult>;
  /** Execute asynchronously and receive incremental stdout/stderr chunks. */
  executeWithOutput(
    commands: string,
    onOutput: (stdout: string, stderr: string) => void,
  ): Promise<ExecResult>;
  /** Static, advisory pre-execution analysis for permission gates. */
  analyze(script: string): ScriptAnalysis;
  /** Cancel at the next command boundary. Sticky until {@link clearCancel}. */
  cancel(): void;
  clearCancel(): void;
  /** Capture content-addressed persistent state. */
  commit(options?: CommitOptions): PackedCommit;
  /** Restore a commit from its content-addressed objects. */
  checkout(
    commitId: string,
    objects: Record<string, Uint8Array>,
    policy?: CheckoutPolicy,
  ): void;
  /** Reset to a fresh state, keeping options and registered custom builtins. */
  reset(): void;
  /** Live handle to the virtual filesystem (same VFS the script sees). */
  fs(): FileSystem;
  readFile(path: string): string;
  readFileBytes(path: string): Uint8Array;
  writeFile(path: string, content: string): void;
  writeFileBytes(path: string, content: Uint8Array): void;
  appendFile(path: string, content: string): void;
  appendFileBytes(path: string, content: Uint8Array): void;
  exists(path: string): boolean;
  mkdir(path: string): void;
  remove(path: string): void;
  ls(path: string): string[];
}

/**
 * Initialize the WebAssembly module. Must resolve before constructing `Bash`.
 * Idempotent.
 */
export function initBashkit(
  input?: RequestInfo | URL | Response | BufferSource | WebAssembly.Module,
): Promise<void>;
