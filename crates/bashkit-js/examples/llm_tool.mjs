#!/usr/bin/env node
/**
 * BashTool as an LLM tool — shows how to wire bashkit into any AI framework.
 *
 * Demonstrates:
 * - Extracting tool definition (name, description, JSON schema)
 * - Simulating an LLM tool-call loop (no API key needed)
 * - Feeding results back as tool responses
 *
 * Run:
 *   cd crates/bashkit-js && npm run build && node examples/llm_tool.mjs
 */

import { BashTool } from "../wrapper.js";

function main() {
  const tool = new BashTool({ username: "agent", hostname: "sandbox" });

  // ─── 1. Tool definition (what you'd send to any LLM) ─────────────
  console.log("=== Tool Definition ===\n");

  const toolDef = {
    name: tool.name,
    description: tool.shortDescription,
    inputSchema: JSON.parse(tool.inputSchema()),
    outputSchema: JSON.parse(tool.outputSchema()),
  };

  console.log(`Name:        ${toolDef.name}`);
  console.log(`Description: ${toolDef.description}`);
  console.log(`Input keys:  ${Object.keys(toolDef.inputSchema.properties || {}).join(", ")}`);
  console.log();

  // ─── 2. System prompt (token-efficient instructions for the LLM) ──
  console.log("=== System Prompt (first 200 chars) ===\n");
  console.log(tool.systemPrompt().substring(0, 200) + "...\n");

  // ─── 3. Simulate an LLM tool-call loop ────────────────────────────
  console.log("=== Simulated Tool-Call Loop ===\n");

  // Pretend the LLM decided to call our tool with these commands
  const llmToolCalls = [
    'echo "Setting up project..."',
    "mkdir -p /tmp/project/src",
    'echo \'console.log("hello")\' > /tmp/project/src/index.js',
    "cat /tmp/project/src/index.js",
    "ls -la /tmp/project/src/",
  ];

  for (const commands of llmToolCalls) {
    console.log(`LLM calls: ${commands}`);
    const result = tool.executeSync(commands);

    // This is what you'd send back to the LLM as the tool response
    const toolResponse = {
      stdout: result.stdout,
      stderr: result.stderr,
      exit_code: result.exit_code,
    };

    if (result.exit_code === 0) {
      console.log(`  → stdout: ${result.stdout.trim() || "(empty)"}`);
    } else {
      console.log(`  → error (${result.exit_code}): ${result.stderr.trim()}`);
    }
    console.log();
  }

  // ─── 4. Generic tool adapter pattern ───────────────────────────────
  console.log("=== Generic Tool Adapter ===\n");

  // This function converts BashTool into a format any framework can use
  const adapter = createToolAdapter(tool);
  console.log(`Adapter name:   ${adapter.name}`);
  console.log(`Adapter schema: ${JSON.stringify(adapter.schema).substring(0, 80)}...`);

  // Execute through adapter
  const adapterResult = adapter.execute({ commands: "echo 'via adapter'" });
  console.log(`Adapter result: ${adapterResult.stdout.trim()}`);
  console.log();

  console.log("All examples passed.");
}

/**
 * Creates a generic tool adapter from BashTool — works with any LLM framework.
 *
 * Returns an object with:
 * - name, description, schema (for tool registration)
 * - execute(params) (for tool invocation)
 */
function createToolAdapter(bashTool) {
  return {
    name: bashTool.name,
    description: bashTool.description(),
    schema: JSON.parse(bashTool.inputSchema()),

    execute(params) {
      const commands = params.commands || params.command || "";
      const result = bashTool.executeSync(commands);
      return {
        stdout: result.stdout,
        stderr: result.stderr,
        exit_code: result.exit_code,
      };
    },
  };
}

main();
