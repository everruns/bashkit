import test from "ava";
import { bashTool as aiSdkBashTool } from "../ai.js";
import { bashTool as anthropicBashTool } from "../anthropic.js";
import { bashTool as openAiBashTool } from "../openai.js";

// ============================================================================
// Issue #990: AI adapters must use a single interpreter instance.
// Files written via the exposed `bash` handle must be visible to tool
// execution, and vice versa — no state divergence.
// ============================================================================

// --- Vercel AI SDK adapter ---------------------------------------------------

test("ai: files option is readable via execute", async (t) => {
  const adapter = aiSdkBashTool({ files: { "/data.txt": "hello" } });
  const result = await adapter.tools.bash.execute({ commands: "cat /data.txt" });
  t.is(result, "hello");
});

test("ai: files written via bash.writeFile are visible in execute", async (t) => {
  const adapter = aiSdkBashTool();
  adapter.bash.writeFile("/x.txt", "from-api");
  const result = await adapter.tools.bash.execute({ commands: "cat /x.txt" });
  t.is(result, "from-api");
});

test("ai: files created via execute are readable via bash.readFile", async (t) => {
  const adapter = aiSdkBashTool();
  await adapter.tools.bash.execute({ commands: "echo -n created > /y.txt" });
  t.is(adapter.bash.readFile("/y.txt"), "created");
});

// --- Anthropic SDK adapter ---------------------------------------------------

test("anthropic: files option is readable via handler", async (t) => {
  const adapter = anthropicBashTool({ files: { "/data.txt": "hello" } });
  const result = await adapter.handler({
    type: "tool_use",
    id: "t1",
    name: "bash",
    input: { commands: "cat /data.txt" },
  });
  t.is(result.content, "hello");
  t.false(result.is_error);
});

test("anthropic: files written via bash.writeFile are visible in handler", async (t) => {
  const adapter = anthropicBashTool();
  adapter.bash.writeFile("/x.txt", "from-api");
  const result = await adapter.handler({
    type: "tool_use",
    id: "t2",
    name: "bash",
    input: { commands: "cat /x.txt" },
  });
  t.is(result.content, "from-api");
});

test("anthropic: files created via handler are readable via bash.readFile", async (t) => {
  const adapter = anthropicBashTool();
  await adapter.handler({
    type: "tool_use",
    id: "t3",
    name: "bash",
    input: { commands: "echo -n created > /y.txt" },
  });
  t.is(adapter.bash.readFile("/y.txt"), "created");
});

// --- OpenAI SDK adapter ------------------------------------------------------

test("openai: files option is readable via handler", async (t) => {
  const adapter = openAiBashTool({ files: { "/data.txt": "hello" } });
  const result = await adapter.handler({
    id: "c1",
    type: "function",
    function: {
      name: "bash",
      arguments: JSON.stringify({ commands: "cat /data.txt" }),
    },
  });
  t.is(result.content, "hello");
});

test("openai: files written via bash.writeFile are visible in handler", async (t) => {
  const adapter = openAiBashTool();
  adapter.bash.writeFile("/x.txt", "from-api");
  const result = await adapter.handler({
    id: "c2",
    type: "function",
    function: {
      name: "bash",
      arguments: JSON.stringify({ commands: "cat /x.txt" }),
    },
  });
  t.is(result.content, "from-api");
});

test("openai: files created via handler are readable via bash.readFile", async (t) => {
  const adapter = openAiBashTool();
  await adapter.handler({
    id: "c3",
    type: "function",
    function: {
      name: "bash",
      arguments: JSON.stringify({ commands: "echo -n created > /y.txt" }),
    },
  });
  t.is(adapter.bash.readFile("/y.txt"), "created");
});

// ============================================================================
// Issue #1185: Framework timeout propagation
// ============================================================================

// --- Timeout via timeoutMs option -------------------------------------------

test("anthropic: timeoutMs option propagates to interpreter", async (t) => {
  const adapter = anthropicBashTool({ timeoutMs: 500 });
  const result = await adapter.handler({
    type: "tool_use",
    id: "t-timeout",
    name: "bash",
    input: { commands: "i=0; while true; do i=$((i+1)); done" },
  });
  // Timed-out execution should produce a non-success result
  t.true(
    result.is_error === true ||
      result.content.includes("124") ||
      result.content.includes("timeout"),
  );
});

test("openai: timeoutMs option propagates to interpreter", async (t) => {
  const adapter = openAiBashTool({ timeoutMs: 500 });
  const result = await adapter.handler({
    id: "c-timeout",
    type: "function",
    function: {
      name: "bash",
      arguments: JSON.stringify({
        commands: "i=0; while true; do i=$((i+1)); done",
      }),
    },
  });
  // Timed-out execution should produce an error or exit 124
  t.true(
    result.content.includes("124") ||
      result.content.includes("timeout") ||
      result.content.includes("Exit code"),
  );
});

// --- AbortSignal cancellation -----------------------------------------------

test("anthropic: handler respects pre-aborted signal", async (t) => {
  const adapter = anthropicBashTool();
  const controller = new AbortController();
  controller.abort();
  const result = await adapter.handler(
    {
      type: "tool_use",
      id: "t-abort",
      name: "bash",
      input: { commands: "echo should-not-run" },
    },
    { signal: controller.signal },
  );
  t.is(result.content, "Execution cancelled");
  t.true(result.is_error);
});

test("openai: handler respects pre-aborted signal", async (t) => {
  const adapter = openAiBashTool();
  const controller = new AbortController();
  controller.abort();
  const result = await adapter.handler(
    {
      id: "c-abort",
      type: "function",
      function: {
        name: "bash",
        arguments: JSON.stringify({ commands: "echo should-not-run" }),
      },
    },
    { signal: controller.signal },
  );
  t.is(result.content, "Execution cancelled");
});
