#!/usr/bin/env node
/**
 * Bashkit as an OpenAI function-calling tool.
 *
 * Uses the official `openai` package with the Responses API to wire
 * BashTool as a function the model can call. The model decides when
 * to execute bash commands, and results are fed back for the next turn.
 *
 * Prerequisites:
 *   npm install openai
 *   export OPENAI_API_KEY=sk-...
 *
 * Run:
 *   node examples/openai_tool.mjs
 */

import OpenAI from "openai";
import { BashTool } from "@everruns/bashkit";

// ─── Setup ───────────────────────────────────────────────────────────

const openai = new OpenAI();
const bashTool = new BashTool({ username: "agent", hostname: "sandbox" });

// Define the tool for OpenAI Responses API
const tools = [
  {
    type: "function",
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
    exit_code: result.exitCode,
  });
}

// ─── Agent loop ──────────────────────────────────────────────────────

async function runAgent(userMessage) {
  console.log(`\nUser: ${userMessage}\n`);

  const instructions = [
    "You have access to a sandboxed bash interpreter.",
    "Use the bash tool to run commands when needed.",
    bashTool.systemPrompt(),
  ].join("\n");

  let input = [{ role: "user", content: userMessage }];
  let previousResponseId = null;

  // Loop until the model produces a final text response
  for (let step = 0; step < 10; step++) {
    const response = await openai.responses.create({
      model: "gpt-5.4",
      reasoning: { effort: "none" },
      instructions,
      input,
      tools,
      ...(previousResponseId && { previous_response_id: previousResponseId }),
    });

    previousResponseId = response.id;

    // Collect function calls from output
    const functionCalls = response.output.filter(
      (item) => item.type === "function_call"
    );

    // If no tool calls, find the text output
    if (functionCalls.length === 0) {
      const textOutput = response.output.find(
        (item) => item.type === "message"
      );
      const text =
        textOutput?.content
          ?.filter((c) => c.type === "output_text")
          .map((c) => c.text)
          .join("") ?? "(no response)";
      console.log(`Assistant: ${text}\n`);
      return text;
    }

    // Execute each tool call and build input for next turn
    input = [];
    for (const call of functionCalls) {
      const args = JSON.parse(call.arguments);
      console.log(`  [tool] ${call.name}: ${args.commands}`);

      const result = executeTool(call.name, args);
      const parsed = JSON.parse(result);
      if (parsed.stdout) console.log(`  [out]  ${parsed.stdout.trim()}`);
      if (parsed.stderr) console.log(`  [err]  ${parsed.stderr.trim()}`);

      input.push({
        type: "function_call_output",
        call_id: call.call_id,
        output: result,
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
