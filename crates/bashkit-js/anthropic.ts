/**
 * Anthropic SDK adapter for Bashkit.
 *
 * Returns a ready-to-use `{ system, tools, handler }` object for Claude's
 * `messages.create()` API, eliminating boilerplate for tool integration.
 *
 * @example
 * ```typescript
 * import Anthropic from "@anthropic-ai/sdk";
 * import { bashTool } from "@everruns/bashkit/anthropic";
 *
 * const client = new Anthropic();
 * const bash = bashTool();
 *
 * const response = await client.messages.create({
 *   model: "claude-haiku-4-5-20251001",
 *   max_tokens: 1024,
 *   system: bash.system,
 *   tools: bash.tools,
 *   messages: [{ role: "user", content: "List files in /home" }],
 * });
 *
 * for (const block of response.content) {
 *   if (block.type === "tool_use") {
 *     const result = await bash.handler(block);
 *     // send result back as tool_result
 *   }
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
}

/** Anthropic tool definition (matches the `tools` array in messages.create). */
interface AnthropicTool {
  name: string;
  description: string;
  input_schema: {
    type: "object";
    properties: Record<string, unknown>;
    required: string[];
  };
}

/** Anthropic tool_use content block. */
interface ToolUseBlock {
  type: "tool_use";
  id: string;
  name: string;
  input: Record<string, unknown>;
}

/** Result from handling a tool call, ready to send back as tool_result. */
export interface ToolResult {
  type: "tool_result";
  tool_use_id: string;
  content: string;
  is_error?: boolean;
}

/** Return value of `bashTool()`. */
export interface BashToolAdapter {
  /** System prompt describing bash capabilities and constraints. */
  system: string;
  /** Tool definitions for Anthropic's messages.create() API. */
  tools: AnthropicTool[];
  /** Handler that executes a tool_use block and returns a tool_result. */
  handler: (toolUse: ToolUseBlock) => Promise<ToolResult>;
  /** The underlying BashTool instance for direct access. */
  bash: BashTool;
}

function formatOutput(result: ExecResult): string {
  let output = result.stdout;
  if (result.stderr) {
    output += (output ? "\n" : "") + `STDERR: ${result.stderr}`;
  }
  if (result.exitCode !== 0) {
    output += (output ? "\n" : "") + `[Exit code: ${result.exitCode}]`;
  }
  return output || "(no output)";
}

/**
 * Create a bash tool adapter for the Anthropic SDK.
 *
 * Returns `{ system, tools, handler }` that plugs directly into
 * `client.messages.create()`.
 *
 * @param options - Configuration for the bash interpreter
 *
 * @example
 * ```typescript
 * import Anthropic from "@anthropic-ai/sdk";
 * import { bashTool } from "@everruns/bashkit/anthropic";
 *
 * const client = new Anthropic();
 * const bash = bashTool({ files: { "/data.txt": "hello" } });
 *
 * const response = await client.messages.create({
 *   model: "claude-haiku-4-5-20251001",
 *   max_tokens: 256,
 *   system: bash.system,
 *   tools: bash.tools,
 *   messages: [{ role: "user", content: "Read /data.txt" }],
 * });
 * ```
 */
export function bashTool(options?: BashToolOptions): BashToolAdapter {
  const { files, ...bashOptions } = options ?? {};

  const bash = new BashTool(bashOptions);

  // Pre-populate VFS files
  if (files) {
    for (const [path, content] of Object.entries(files)) {
      bash.writeFile(path, content);
    }
  }

  const system = bash.systemPrompt();

  const tools: AnthropicTool[] = [
    {
      name: "bash",
      description: bash.description(),
      input_schema: {
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
  ];

  const handler = async (toolUse: ToolUseBlock): Promise<ToolResult> => {
    const commands = (toolUse.input as { commands?: string }).commands;
    if (!commands) {
      return {
        type: "tool_result",
        tool_use_id: toolUse.id,
        content: "Error: missing 'commands' parameter",
        is_error: true,
      };
    }

    try {
      const result = await bash.execute(commands);
      return {
        type: "tool_result",
        tool_use_id: toolUse.id,
        content: formatOutput(result),
        is_error: result.exitCode !== 0,
      };
    } catch (err) {
      return {
        type: "tool_result",
        tool_use_id: toolUse.id,
        content: `Execution error: ${err instanceof Error ? err.message : String(err)}`,
        is_error: true,
      };
    }
  };

  return { system, tools, handler, bash };
}
