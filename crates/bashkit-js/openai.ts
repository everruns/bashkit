/**
 * OpenAI SDK adapter for Bashkit.
 *
 * Returns a ready-to-use `{ system, tools, handler }` object for OpenAI's
 * `chat.completions.create()` API.
 *
 * @example
 * ```typescript
 * import OpenAI from "openai";
 * import { bashTool } from "@everruns/bashkit/openai";
 *
 * const client = new OpenAI();
 * const bash = bashTool();
 *
 * const response = await client.chat.completions.create({
 *   model: "gpt-4.1-mini",
 *   tools: bash.tools,
 *   messages: [
 *     { role: "system", content: bash.system },
 *     { role: "user", content: "Create a file with today's date" },
 *   ],
 * });
 *
 * for (const call of response.choices[0].message.tool_calls ?? []) {
 *   const result = await bash.handler(call);
 *   // send result back as tool message
 * }
 * ```
 *
 * @packageDocumentation
 */

import { BashTool } from "./wrapper.js";
import type { BashOptions, ExecResult } from "./wrapper.js";

/** Options for configuring the bash tool adapter. */
export interface BashToolOptions extends Omit<BashOptions, "files"> {
  /** Pre-populate VFS files. Keys are absolute paths, values are file contents. */
  files?: Record<string, string>;
  /**
   * Execution timeout in milliseconds.
   *
   * When set, this is passed to the underlying BashTool as `timeoutMs`.
   * Commands exceeding this duration are aborted with exit code 124.
   * Framework-level timeouts can be propagated here to ensure bashkit
   * stops execution when the framework cancels a tool call.
   */
  timeoutMs?: number;
  /**
   * Maximum output length in characters (default: 100000).
   *
   * Output exceeding this limit is truncated with a `[truncated]` marker.
   * Prevents context window flooding when scripts produce large output.
   */
  maxOutputLength?: number;
  /**
   * Wrap tool output in XML boundary markers (default: false).
   *
   * When enabled, output is wrapped in `<tool_output>...</tool_output>` tags
   * to help LLMs distinguish tool output data from instructions, reducing
   * prompt injection risk via tool output.
   */
  sanitizeOutput?: boolean;
}

/** OpenAI function tool definition (matches the `tools` array in chat.completions.create). */
interface OpenAITool {
  type: "function";
  function: {
    name: string;
    description: string;
    parameters: {
      type: "object";
      properties: Record<string, unknown>;
      required: string[];
    };
  };
}

/** OpenAI tool_call from a chat completion response. */
interface OpenAIToolCall {
  id: string;
  type: "function";
  function: {
    name: string;
    arguments: string;
  };
}

/** Result from handling a tool call, ready to send as a tool message. */
export interface ToolResult {
  role: "tool";
  tool_call_id: string;
  content: string;
}

/** Options for handler invocation. */
export interface HandlerOptions {
  /** AbortSignal to cancel execution when the framework aborts the tool call. */
  signal?: AbortSignal;
}

/** Return value of `bashTool()`. */
export interface BashToolAdapter {
  /** System prompt describing bash capabilities and constraints. */
  system: string;
  /** Tool definitions for OpenAI's chat.completions.create() API. */
  tools: OpenAITool[];
  /**
   * Handler that executes a tool_call and returns a tool message.
   *
   * Pass an AbortSignal via the options parameter to cancel execution
   * when the framework aborts the tool call:
   *
   * ```typescript
   * const controller = new AbortController();
   * const result = await bash.handler(call, { signal: controller.signal });
   * ```
   */
  handler: (
    toolCall: OpenAIToolCall,
    options?: HandlerOptions,
  ) => Promise<ToolResult>;
  /** The underlying BashTool instance for direct access. */
  bash: BashTool;
}

const DEFAULT_MAX_OUTPUT_LENGTH = 100_000;

function formatOutput(
  result: ExecResult,
  maxOutputLength: number = DEFAULT_MAX_OUTPUT_LENGTH,
  sanitize: boolean = false,
): string {
  let output = result.stdout;
  if (result.stderr) {
    output += (output ? "\n" : "") + `STDERR: ${result.stderr}`;
  }
  if (result.exitCode !== 0) {
    output += (output ? "\n" : "") + `[Exit code: ${result.exitCode}]`;
  }
  output = output || "(no output)";
  if (output.length > maxOutputLength) {
    output = output.slice(0, maxOutputLength) + "\n[truncated]";
  }
  if (sanitize) {
    output = `<tool_output>\n${output}\n</tool_output>`;
  }
  return output;
}

/**
 * Create a bash tool adapter for the OpenAI SDK.
 *
 * Returns `{ system, tools, handler }` that plugs directly into
 * `client.chat.completions.create()`.
 *
 * @param options - Configuration for the bash interpreter
 *
 * @example
 * ```typescript
 * import OpenAI from "openai";
 * import { bashTool } from "@everruns/bashkit/openai";
 *
 * const client = new OpenAI();
 * const bash = bashTool({ files: { "/data.txt": "42" } });
 *
 * const response = await client.chat.completions.create({
 *   model: "gpt-4.1-nano",
 *   tools: bash.tools,
 *   messages: [
 *     { role: "system", content: bash.system },
 *     { role: "user", content: "What's in /data.txt?" },
 *   ],
 * });
 * ```
 */
export function bashTool(options?: BashToolOptions): BashToolAdapter {
  const { files, maxOutputLength, sanitizeOutput, ...bashOptions } =
    options ?? {};

  const bash = new BashTool(bashOptions);

  if (files) {
    for (const [path, content] of Object.entries(files)) {
      bash.writeFile(path, content);
    }
  }

  const system = bash.systemPrompt();

  const tools: OpenAITool[] = [
    {
      type: "function",
      function: {
        name: "bash",
        description: bash.description(),
        parameters: {
          type: "object",
          properties: {
            commands: {
              type: "string",
              description:
                "Bash commands to execute. State persists between calls.",
            },
          },
          required: ["commands"],
        },
      },
    },
  ];

  const handler = async (
    toolCall: OpenAIToolCall,
    handlerOptions?: HandlerOptions,
  ): Promise<ToolResult> => {
    let commands: string;
    try {
      const args = JSON.parse(toolCall.function.arguments);
      commands = args.commands;
    } catch {
      return {
        role: "tool",
        tool_call_id: toolCall.id,
        content: "Error: invalid JSON in function arguments",
      };
    }

    if (!commands) {
      return {
        role: "tool",
        tool_call_id: toolCall.id,
        content: "Error: missing 'commands' parameter",
      };
    }

    // Wire up AbortSignal to cancel bashkit execution when the
    // framework (or caller) aborts the tool call.
    const signal = handlerOptions?.signal;
    if (signal?.aborted) {
      return {
        role: "tool",
        tool_call_id: toolCall.id,
        content: "Execution cancelled",
      };
    }

    let onAbort: (() => void) | undefined;
    if (signal) {
      onAbort = () => bash.cancel();
      signal.addEventListener("abort", onAbort, { once: true });
    }

    try {
      const result = await bash.execute(commands);
      return {
        role: "tool",
        tool_call_id: toolCall.id,
        content: formatOutput(result, maxOutputLength, sanitizeOutput),
      };
    } catch (err) {
      return {
        role: "tool",
        tool_call_id: toolCall.id,
        content: `Execution error: ${err instanceof Error ? err.message : String(err)}`,
      };
    } finally {
      if (signal && onAbort) {
        signal.removeEventListener("abort", onAbort);
        if (signal.aborted) {
          bash.clearCancel();
        }
      }
    }
  };

  return { system, tools, handler, bash };
}
