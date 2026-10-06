// Decision: `ScriptedTool` ships from the `@everruns/bashkit/scripted` entry,
// mirroring the Rust `bashkit-scripted-tool` crate; the root entry keeps the
// interpreter and `BashTool`. The root still re-exports it as a deprecated
// alias during the transition.

/**
 * Multi-tool orchestration for Bashkit (`ScriptedTool`).
 *
 * Register JS callbacks as bash builtins; one bash script composes them with
 * pipes, loops, and `jq` in a logic-only shell.
 *
 * @packageDocumentation
 */

import type {
  ExecResult,
  ScriptedTool as NativeScriptedToolType,
} from "./index.cjs";
import {
  BashError,
  DEFAULT_MAX_INPUT_BYTES,
  inputTooLargeExecResult,
  native,
  queueAsyncExecute,
  validateJsonNestingDepth,
} from "./internal.js";

export type { ExecResult };

const NativeScriptedTool: typeof NativeScriptedToolType = native.ScriptedTool;

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
 * import { ScriptedTool } from '@everruns/bashkit/scripted';
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
