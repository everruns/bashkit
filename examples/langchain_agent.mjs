#!/usr/bin/env node
/**
 * Bashkit as a LangChain.js tool with a ReAct agent.
 *
 * Uses `@langchain/core` and `@langchain/langgraph` to create a ReAct agent
 * that can execute bash commands via BashTool. The agent reasons about which
 * commands to run and iterates until it has the answer.
 *
 * Prerequisites:
 *   npm install @langchain/core @langchain/langgraph @langchain/openai zod
 *   export OPENAI_API_KEY=sk-...
 *
 * Run:
 *   node examples/langchain_agent.mjs
 */

import { DynamicStructuredTool } from "@langchain/core/tools";
import { ChatOpenAI } from "@langchain/openai";
import { createReactAgent } from "@langchain/langgraph/prebuilt";
import { z } from "zod";
import { BashTool } from "@everruns/bashkit";

// ─── Setup ───────────────────────────────────────────────────────────

const bashTool = new BashTool({ username: "agent", hostname: "sandbox" });

// Wrap BashTool as a LangChain DynamicStructuredTool
const bashLangChainTool = new DynamicStructuredTool({
  name: "bash",
  description: [
    bashTool.shortDescription,
    "Execute bash commands in a sandboxed virtual filesystem.",
    "State persists between calls. Use for file operations, text processing, and scripting.",
  ].join(" "),
  schema: z.object({
    commands: z
      .string()
      .describe("Bash commands to execute"),
  }),
  func: async ({ commands }) => {
    const result = bashTool.executeSync(commands);
    return JSON.stringify({
      stdout: result.stdout,
      stderr: result.stderr,
      exit_code: result.exitCode,
    });
  },
});

// ─── Agent ───────────────────────────────────────────────────────────

async function runAgent(userMessage) {
  console.log(`\nUser: ${userMessage}\n`);

  const model = new ChatOpenAI({
    model: "gpt-4.1-mini",
    temperature: 0,
  });

  const agent = createReactAgent({
    llm: model,
    tools: [bashLangChainTool],
  });

  const result = await agent.invoke({
    messages: [{ role: "user", content: userMessage }],
  });

  // Print tool calls and final response
  for (const msg of result.messages) {
    if (msg._getType() === "ai" && msg.tool_calls?.length > 0) {
      for (const tc of msg.tool_calls) {
        console.log(`  [tool] ${tc.name}: ${tc.args.commands}`);
      }
    }
    if (msg._getType() === "tool") {
      const parsed = JSON.parse(msg.content);
      if (parsed.stdout) console.log(`  [out]  ${parsed.stdout.trim()}`);
      if (parsed.stderr) console.log(`  [err]  ${parsed.stderr.trim()}`);
    }
  }

  // Final AI message
  const lastAi = result.messages
    .filter((m) => m._getType() === "ai" && m.content)
    .pop();
  if (lastAi) {
    console.log(`\nAssistant: ${lastAi.content}\n`);
  }

  return lastAi?.content;
}

// ─── Main ────────────────────────────────────────────────────────────

async function main() {
  console.log("Bashkit + LangChain.js ReAct Agent Example");
  console.log("==========================================");

  await runAgent(
    "Create a CSV file at /tmp/employees.csv with 5 employees (name, department, salary). Then find the highest paid employee and their department."
  );
}

main().catch((err) => {
  if (err.message?.includes("API key") || err.message?.includes("OPENAI")) {
    console.error(
      "Set OPENAI_API_KEY to run this example. See the file header for details."
    );
    process.exit(1);
  }
  throw err;
});
