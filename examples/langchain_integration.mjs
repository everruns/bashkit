#!/usr/bin/env node
/**
 * Bashkit LangChain.js integration example.
 *
 * Uses the built-in `@everruns/bashkit/langchain` module to create
 * LangChain-compatible tools without manual wrapping.
 *
 * Prerequisites:
 *   npm install @langchain/core @langchain/langgraph @langchain/openai zod
 *   export OPENAI_API_KEY=sk-...
 *
 * Run:
 *   node examples/langchain_integration.mjs
 */

import { createBashTool, createScriptedTool } from "@everruns/bashkit/langchain";
import { ScriptedTool } from "@everruns/bashkit";
import { ChatOpenAI } from "@langchain/openai";
import { createReactAgent } from "@langchain/langgraph/prebuilt";

// ─── Basic BashTool via integration ─────────────────────────────────

console.log("=== Bashkit LangChain Integration Example ===\n");

// Create a LangChain tool in one line
const bashLangChainTool = createBashTool({ username: "agent", hostname: "sandbox" });
console.log(`Tool created: ${bashLangChainTool.name}`);
console.log(`Description: ${bashLangChainTool.description.slice(0, 80)}...`);

// ─── ScriptedTool via integration ───────────────────────────────────

const st = new ScriptedTool({ name: "data_api" });
st.addTool("get_users", "Get all users as JSON", () =>
  JSON.stringify([
    { id: 1, name: "Alice", role: "admin" },
    { id: 2, name: "Bob", role: "user" },
    { id: 3, name: "Charlie", role: "user" },
  ]) + "\n"
);
st.addTool("get_user", "Get a single user by ID", (params) => {
  const users = { 1: "Alice", 2: "Bob", 3: "Charlie" };
  const name = users[params.id];
  return name ? JSON.stringify({ id: params.id, name }) + "\n" : "User not found\n";
});

const scriptedLangChainTool = createScriptedTool(st);
console.log(`\nScripted tool created: ${scriptedLangChainTool.name}`);

// ─── Self-contained test (no API key needed) ────────────────────────

// Invoke bash tool directly
const bashResult = await bashLangChainTool.invoke({
  commands: 'echo "Hello from LangChain integration!"',
});
console.log(`\nBash tool result: ${bashResult.trim()}`);

// Invoke scripted tool directly
const scriptedResult = await scriptedLangChainTool.invoke({
  commands: 'get_users | jq -r ".[].name"',
});
console.log(`Scripted tool result:\n${scriptedResult.trim()}`);

// ─── Agent (requires OPENAI_API_KEY) ────────────────────────────────

async function runAgent() {
  const model = new ChatOpenAI({
    model: "gpt-5.4",
    modelKwargs: { reasoning_effort: "none" },
  });

  const agent = createReactAgent({
    llm: model,
    tools: [bashLangChainTool, scriptedLangChainTool],
  });

  console.log("\n--- Agent interaction ---");
  const result = await agent.invoke({
    messages: [
      {
        role: "user",
        content:
          "Use the data_api to get all users, then use bash to count how many there are.",
      },
    ],
  });

  const lastAi = result.messages
    .filter((m) => m._getType() === "ai" && m.content)
    .pop();
  if (lastAi) {
    console.log(`Agent: ${lastAi.content}`);
  }
}

if (process.env.OPENAI_API_KEY) {
  await runAgent();
} else {
  console.log("\nSkipping agent (no OPENAI_API_KEY). Self-contained tests passed.");
}
