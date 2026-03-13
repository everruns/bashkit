#!/usr/bin/env node
/**
 * Bashkit as an OpenAI function-calling tool.
 *
 * Uses the official `openai` package to wire BashTool as a function
 * the model can call. The model decides when to execute bash commands,
 * and results are fed back for the next response.
 *
 * Prerequisites:
 *   npm install openai
 *   export OPENAI_API_KEY=sk-...
 *
 * Run:
 *   cd crates/bashkit-js && npm run build && node examples/openai_tool.mjs
 */

import OpenAI from "openai";
import { BashTool } from "../wrapper.js";

// ─── Setup ───────────────────────────────────────────────────────────

const openai = new OpenAI();
const bashTool = new BashTool({ username: "agent", hostname: "sandbox" });

// Define the tool for OpenAI function calling
const tools = [
  {
    type: "function",
    function: {
      name: "bash",
      description: bashTool.shortDescription,
      parameters: {
        type: "object",
        properties: {
          commands: {
            type: "string",
            description:
              "Bash commands to execute in a sandboxed virtual environment",
          },
        },
        required: ["commands"],
        additionalProperties: false,
      },
      strict: true,
    },
  },
];

// ─── Tool execution handler ──────────────────────────────────────────

function executeTool(name, args) {
  if (name !== "bash") {
    return JSON.stringify({ error: `Unknown tool: ${name}` });
  }
  const result = bashTool.executeSync(args.commands);
  return JSON.stringify({
    stdout: result.stdout,
    stderr: result.stderr,
    exit_code: result.exit_code,
  });
}

// ─── Agent loop ──────────────────────────────────────────────────────

async function runAgent(userMessage) {
  console.log(`\nUser: ${userMessage}\n`);

  const messages = [
    {
      role: "system",
      content: [
        "You have access to a sandboxed bash interpreter.",
        "Use the bash tool to run commands when needed.",
        bashTool.systemPrompt(),
      ].join("\n"),
    },
    { role: "user", content: userMessage },
  ];

  // Loop until the model produces a final text response
  for (let step = 0; step < 10; step++) {
    const response = await openai.chat.completions.create({
      model: "gpt-4.1-mini",
      messages,
      tools,
    });

    const choice = response.choices[0];
    messages.push(choice.message);

    // If no tool calls, we have the final answer
    if (!choice.message.tool_calls || choice.message.tool_calls.length === 0) {
      console.log(`Assistant: ${choice.message.content}\n`);
      return choice.message.content;
    }

    // Execute each tool call
    for (const toolCall of choice.message.tool_calls) {
      const args = JSON.parse(toolCall.function.arguments);
      console.log(`  [tool] ${toolCall.function.name}: ${args.commands}`);

      const result = executeTool(toolCall.function.name, args);
      const parsed = JSON.parse(result);
      if (parsed.stdout) console.log(`  [out]  ${parsed.stdout.trim()}`);
      if (parsed.stderr) console.log(`  [err]  ${parsed.stderr.trim()}`);

      messages.push({
        role: "tool",
        tool_call_id: toolCall.id,
        content: result,
      });
    }
  }

  console.log("(max steps reached)");
}

// ─── Main ────────────────────────────────────────────────────────────

async function main() {
  console.log("Bashkit + OpenAI Function Calling Example");
  console.log("=========================================");

  await runAgent(
    "Create a file /tmp/greeting.txt with 'Hello World', then count the words in it."
  );
}

main().catch((err) => {
  if (err.message?.includes("API key")) {
    console.error(
      "Set OPENAI_API_KEY to run this example. See the file header for details."
    );
    process.exit(1);
  }
  throw err;
});
