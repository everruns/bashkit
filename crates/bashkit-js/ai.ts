/**
 * Vercel AI SDK adapter for Bashkit.
 *
 * Returns `{ system, tools }` that plugs directly into `generateText()` /
 * `streamText()` with zero boilerplate. Tools include built-in `execute`
 * functions, so the AI SDK auto-executes tool calls in its `maxSteps` loop.
 *
 * @example
 * ```typescript
 * import { generateText } from "ai";
 * import { anthropic } from "@ai-sdk/anthropic";
 * import { bashTool } from "@everruns/bashkit/ai";
 *
 * const bash = bashTool({
 *   files: { "/home/user/data.csv": "name,age\nAlice,30\nBob,25" },
 * });
 *
 * const { text } = await generateText({
 *   model: anthropic("claude-haiku-4-5-20251001"),
 *   system: bash.system,
 *   tools: bash.tools,
 *   maxSteps: 5,
 *   prompt: "Analyze the CSV file and tell me the average age",
 * });
 * ```
 *
 * @packageDocumentation
 */

import { BashTool } from "./wrapper.js";
import type { BashOptions, ExecResult } from "./wrapper.js";

// Vercel AI SDK tool types — we define them inline to avoid requiring
// the `ai` package as a dependency (it's a peer dependency).
// These match the ai@4.x Tool interface.

/** Vercel AI SDK CoreTool-compatible object. */
interface AiTool {
  type?: "function";
  description: string;
  parameters: {
    type: "object";
    properties: Record<string, unknown>;
    required: string[];
    additionalProperties?: boolean;
    $schema?: string;
  };
  execute: (args: Record<string, unknown>) => Promise<string>;
}

/** Options for configuring the bash tool adapter. */
export interface BashToolOptions extends Omit<BashOptions, "files"> {
  /** Pre-populate VFS files. Keys are absolute paths, values are file contents. */
  files?: Record<string, string>;
}

/** Return value of `bashTool()`. */
export interface BashToolAdapter {
  /** System prompt describing bash capabilities and constraints. */
  system: string;
  /** Tool definitions for Vercel AI SDK's generateText/streamText. */
  tools: Record<string, AiTool>;
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
 * Create a bash tool adapter for the Vercel AI SDK.
 *
 * Returns `{ system, tools }` that plugs directly into `generateText()` or
 * `streamText()`. The tool includes a built-in `execute` function, so tool
 * calls are auto-executed when using `maxSteps`.
 *
 * @param options - Configuration for the bash interpreter
 *
 * @example
 * ```typescript
 * import { generateText } from "ai";
 * import { anthropic } from "@ai-sdk/anthropic";
 * import { bashTool } from "@everruns/bashkit/ai";
 *
 * const bash = bashTool({ files: { "/test.txt": "hello world" } });
 *
 * const { text } = await generateText({
 *   model: anthropic("claude-haiku-4-5-20251001"),
 *   system: bash.system,
 *   tools: bash.tools,
 *   maxSteps: 3,
 *   prompt: "Read /test.txt and tell me what it says",
 * });
 * ```
 */
export function bashTool(options?: BashToolOptions): BashToolAdapter {
  const { files, ...bashOptions } = options ?? {};

  const bash = new BashTool(bashOptions);

  if (files) {
    for (const [path, content] of Object.entries(files)) {
      bash.writeFile(path, content);
    }
  }

  const system = bash.systemPrompt();

  const tools: Record<string, AiTool> = {
    bash: {
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
        additionalProperties: false,
        $schema: "http://json-schema.org/draft-07/schema#",
      },
      execute: async (args: Record<string, unknown>): Promise<string> => {
        const commands = args.commands as string;
        if (!commands) {
          return "Error: missing 'commands' parameter";
        }

        try {
          const result = await bash.execute(commands);
          return formatOutput(result);
        } catch (err) {
          return `Execution error: ${err instanceof Error ? err.message : String(err)}`;
        }
      },
    },
  };

  return { system, tools, bash };
}
