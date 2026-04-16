#!/usr/bin/env node
/**
 * Bashkit as a Vercel AI SDK tool.
 *
 * Uses the `ai` package with `@ai-sdk/openai` to wire BashTool as a
 * tool that the model can invoke. The AI SDK handles the tool-call loop
 * automatically via `generateText` with `maxSteps`.
 *
 * Prerequisites:
 *   npm install ai @ai-sdk/openai
 *   export OPENAI_API_KEY=sk-...
 *
 * Run:
 *   node examples/vercel_ai_tool.mjs
 */

import { generateText, tool, jsonSchema } from "ai";
import { openai } from "@ai-sdk/openai";
import { BashTool } from "@everruns/bashkit";

// ─── Setup ───────────────────────────────────────────────────────────

const bashTool = new BashTool({ username: "agent", hostname: "sandbox" });

// Define bashkit as a Vercel AI SDK tool
const bashkitTool = tool({
  description: bashTool.shortDescription,
  inputSchema: jsonSchema({
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
  }),
  execute: async ({ commands }) => {
    const result = bashTool.executeSync(commands);
    return {
      stdout: result.stdout,
      stderr: result.stderr,
      exit_code: result.exitCode,
    };
  },
});

// ─── Agent ───────────────────────────────────────────────────────────

async function runAgent(userMessage) {
  console.log(`\nUser: ${userMessage}\n`);

  const result = await generateText({
    model: openai("gpt-5.4"),
    system: [
      "You have access to a sandboxed bash interpreter.",
      "Use the bash tool to run commands when needed.",
      bashTool.systemPrompt(),
    ].join("\n"),
    prompt: userMessage,
    tools: { bash: bashkitTool },
    maxSteps: 10,
    onStepFinish: ({ toolResults }) => {
      for (const tr of toolResults) {
        console.log(`  [tool] bash: ${tr.args?.commands ?? "(no args)"}`);
        if (tr.result?.stdout) console.log(`  [out]  ${tr.result.stdout.trim()}`);
        if (tr.result?.stderr) console.log(`  [err]  ${tr.result.stderr.trim()}`);
      }
    },
  });

  console.log(`\nAssistant: ${result.text}\n`);
  console.log(
    `Steps: ${result.steps.length}, Tool calls: ${result.steps.reduce((n, s) => n + (s.toolCalls?.length ?? 0), 0)}`
  );
  return result.text;
}

// ─── Main ────────────────────────────────────────────────────────────

async function main() {
  console.log("Bashkit + Vercel AI SDK Example");
  console.log("===============================");

  await runAgent(
    "Create a JSON file at /tmp/config.json with a 'name' field set to 'my-app' and 'port' set to 3000. Then read it back and tell me the port number."
  );
}

function shouldSkipOpenAiExample(err) {
  const message = err?.message ?? "";
  return (
    message.includes("API key") ||
    message.includes("OPENAI") ||
    message.includes("insufficient_quota") ||
    message.includes("rate limit") ||
    err?.status === 429 ||
    err?.code === "insufficient_quota" ||
    err?.type === "insufficient_quota"
  );
}

main().catch((err) => {
  if (shouldSkipOpenAiExample(err)) {
    console.error(
      "Skipping example: OpenAI credentials are missing or the project is out of quota."
    );
    process.exit(0);
  }
  throw err;
});
