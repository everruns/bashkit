/**
 * LangChain.js integration for Bashkit.
 *
 * Provides LangChain-compatible tools wrapping BashTool and ScriptedTool
 * for use with LangChain agents and chains.
 *
 * @example
 * ```typescript
 * import { createBashTool, createScriptedTool } from '@everruns/bashkit/langchain';
 *
 * // Basic bash tool
 * const tool = createBashTool();
 * const result = await tool.invoke({ commands: "echo hello" });
 *
 * // Scripted tool
 * import { ScriptedTool } from '@everruns/bashkit';
 * const st = new ScriptedTool({ name: "api" });
 * st.addTool("greet", "Greet user", (p) => `hello ${p.name}\n`);
 * const langchainTool = createScriptedTool(st);
 * ```
 *
 * @packageDocumentation
 */

import { DynamicStructuredTool } from "@langchain/core/tools";
import { z } from "zod";
import { BashTool, ScriptedTool } from "./wrapper.js";
import type { BashOptions } from "./wrapper.js";

const bashInputSchema = z.object({
  commands: z
    .string()
    .describe("Bash commands to execute (like `bash -c 'commands'`)"),
});

/**
 * Format an execution result for LangChain tool output.
 */
function formatResult(result: {
  stdout: string;
  stderr: string;
  exitCode: number;
  error?: string | null;
}): string {
  if (result.error) {
    throw new Error(`Execution error: ${result.error}`);
  }

  let output = result.stdout;
  if (result.stderr) {
    output += `\nSTDERR: ${result.stderr}`;
  }
  if (result.exitCode !== 0) {
    output += `\n[Exit code: ${result.exitCode}]`;
  }
  return output;
}

/**
 * Create a LangChain-compatible Bashkit tool.
 *
 * Returns a `DynamicStructuredTool` that can be passed directly to
 * LangChain agents like `createReactAgent`.
 *
 * @param options - BashTool configuration (username, hostname, limits)
 *
 * @example
 * ```typescript
 * import { createBashTool } from '@everruns/bashkit/langchain';
 * import { createReactAgent } from '@langchain/langgraph/prebuilt';
 *
 * const tool = createBashTool({ username: "agent" });
 * const agent = createReactAgent({ llm: model, tools: [tool] });
 * ```
 */
export function createBashTool(
  options?: Omit<BashOptions, "files">,
): DynamicStructuredTool {
  const bashTool = new BashTool(options);

  return new DynamicStructuredTool({
    name: bashTool.name,
    description: [
      bashTool.shortDescription,
      "Execute bash commands in a sandboxed virtual filesystem.",
      "State persists between calls. Use for file operations, text processing, and scripting.",
    ].join(" "),
    schema: bashInputSchema,
    func: async ({ commands }: { commands: string }) => {
      const result = bashTool.executeSync(commands);
      return formatResult(result);
    },
  });
}

/**
 * Create a LangChain-compatible tool from a configured ScriptedTool.
 *
 * The ScriptedTool should already have tools registered via `addTool()`.
 *
 * @param scriptedTool - A ScriptedTool with registered tool callbacks
 *
 * @example
 * ```typescript
 * import { ScriptedTool } from '@everruns/bashkit';
 * import { createScriptedTool } from '@everruns/bashkit/langchain';
 *
 * const st = new ScriptedTool({ name: "api" });
 * st.addTool("get_data", "Fetch data", (p) => JSON.stringify({ id: p.id }));
 * const tool = createScriptedTool(st);
 * ```
 */
export function createScriptedTool(
  scriptedTool: ScriptedTool,
): DynamicStructuredTool {
  return new DynamicStructuredTool({
    name: scriptedTool.name,
    description: scriptedTool.systemPrompt(),
    schema: bashInputSchema,
    func: async ({ commands }: { commands: string }) => {
      const result = await scriptedTool.execute(commands);
      return formatResult(result);
    },
  });
}
