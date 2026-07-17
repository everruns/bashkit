// Public TypeScript surface for @everruns/bashkit-web.

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
}

/**
 * A JS callback registered as a bash builtin. Return the builtin's stdout, or a
 * `Promise` of it. Async callbacks are only awaited by {@link Bash.execute}
 * (not `executeSync`). Throwing / rejecting becomes stderr with exit code 1.
 */
export type CustomBuiltin = (
  ctx: BuiltinRequest,
) => string | Promise<string>;

/** Options for constructing a {@link Bash} instance. */
export interface BashOptions {
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
  /** Max interpreter memory in bytes. */
  maxMemory?: number;
  /** Pre-created files seeded into the virtual filesystem (string contents). */
  files?: Record<string, string>;
  /** JS callbacks registered as bash builtins. */
  customBuiltins?: Record<string, CustomBuiltin>;
}

/** Result of a bash execution. */
export interface ExecResult {
  readonly stdout: string;
  readonly stderr: string;
  readonly exitCode: number;
  readonly success: boolean;
  readonly stdoutTruncated: boolean;
  readonly stderrTruncated: boolean;
}

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
  /** Reset to a fresh state, keeping options and registered custom builtins. */
  reset(): void;
  readFile(path: string): string;
  writeFile(path: string, content: string): void;
  exists(path: string): boolean;
  mkdir(path: string): void;
  ls(path: string): string[];
}

/**
 * Initialize the WebAssembly module. Must resolve before constructing `Bash`.
 * Idempotent.
 */
export function initBashkit(
  input?: RequestInfo | URL | Response | BufferSource | WebAssembly.Module,
): Promise<void>;
