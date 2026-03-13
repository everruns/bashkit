import {
  Bash as NativeBash,
  BashTool as NativeBashTool,
  getVersion as nativeGetVersion,
} from "./index.js";

import type { ExecResult, BashOptions } from "./index.js";

export type { ExecResult, BashOptions };

/**
 * Error thrown when a bash command execution fails.
 */
export class BashError extends Error {
  readonly exitCode: number;
  readonly stderr: string;

  constructor(result: ExecResult) {
    const message = result.error ?? result.stderr ?? `Exit code ${result.exit_code}`;
    super(message);
    this.name = "BashError";
    this.exitCode = result.exit_code;
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
  private native: NativeBash;

  constructor(options?: BashOptions) {
    this.native = new NativeBash(options);
  }

  /**
   * Execute bash commands synchronously and return the result.
   */
  executeSync(commands: string): ExecResult {
    return this.native.executeSync(commands);
  }

  /**
   * Execute bash commands synchronously. Throws `BashError` on non-zero exit.
   */
  executeSyncOrThrow(commands: string): ExecResult {
    const result = this.native.executeSync(commands);
    if (result.exit_code !== 0) {
      throw new BashError(result);
    }
    return result;
  }

  /**
   * Reset interpreter to fresh state, preserving configuration.
   */
  reset(): void {
    this.native.reset();
  }
}

/**
 * Bash interpreter with LLM tool metadata (schema, description, system_prompt).
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
 *
 * const result = tool.executeSync('echo hello');
 * console.log(result.stdout);       // hello\n
 * ```
 */
export class BashTool {
  private native: NativeBashTool;

  constructor(options?: BashOptions) {
    this.native = new NativeBashTool(options);
  }

  /**
   * Execute bash commands synchronously and return the result.
   */
  executeSync(commands: string): ExecResult {
    return this.native.executeSync(commands);
  }

  /**
   * Execute bash commands synchronously. Throws `BashError` on non-zero exit.
   */
  executeSyncOrThrow(commands: string): ExecResult {
    const result = this.native.executeSync(commands);
    if (result.exit_code !== 0) {
      throw new BashError(result);
    }
    return result;
  }

  /**
   * Reset interpreter to fresh state, preserving configuration.
   */
  reset(): void {
    this.native.reset();
  }

  /** Tool name. */
  get name(): string {
    return this.native.name;
  }

  /** Short description. */
  get shortDescription(): string {
    return this.native.shortDescription;
  }

  /** Full description. */
  description(): string {
    return this.native.description();
  }

  /** Help text. */
  help(): string {
    return this.native.help();
  }

  /** System prompt for LLMs. */
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
