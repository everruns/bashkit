#!/usr/bin/env node
/**
 * Custom builtins on Bash / BashTool.
 *
 * Register JS callbacks as persistent bash builtins that share the instance's
 * virtual filesystem. Files survive across `execute()` calls, so a builtin's
 * output can be redirected into the VFS and consumed by a later command.
 *
 * Custom builtins use NAPI threadsafe functions; the JS event loop must be
 * free to dispatch them, which means **all calls below use `await execute()`**.
 * `executeSync()` deadlocks if the script invokes a custom builtin.
 *
 * Run:
 *   node examples/custom_builtins.mjs
 */

import { Bash, BashTool } from "@everruns/bashkit";

function assert(condition, msg = "assertion failed") {
  if (!condition) throw new Error(msg);
}

async function demoConstructor() {
  console.log("=== Constructor-time registration ===\n");

  const bash = new Bash({
    customBuiltins: {
      greet: (ctx) => `hello ${ctx.argv[0] ?? "world"}\n`,
      upper: (ctx) => (ctx.stdin ?? "").toUpperCase(),
    },
  });

  const r1 = await bash.execute("greet Alice");
  console.log(`greet: ${r1.stdout.trim()}`);
  assert(r1.stdout === "hello Alice\n");

  // Pipe through a custom builtin
  const r2 = await bash.execute("echo hello | upper");
  console.log(`upper: ${r2.stdout.trim()}`);
  assert(r2.stdout === "HELLO\n");
}

async function demoVfsPersistence() {
  console.log("\n=== VFS persistence across execute() calls ===\n");

  const bash = new Bash({
    customBuiltins: {
      "get-order": (ctx) =>
        JSON.stringify({ id: ctx.argv[0] ?? "?", status: "shipped" }) + "\n",
    },
  });

  await bash.execute("mkdir -p /scratch");
  await bash.execute("get-order 42 > /scratch/order.json");

  // Separate call — the file is still there
  const r = await bash.execute("cat /scratch/order.json");
  const parsed = JSON.parse(r.stdout);
  console.log(`order: ${JSON.stringify(parsed)}`);
  assert(parsed.id === "42" && parsed.status === "shipped");
}

async function demoAsync() {
  console.log("\n=== Async callbacks ===\n");

  const bash = new Bash({
    customBuiltins: {
      fetch: async (ctx) => {
        // Simulate async work (real code: fetch, DB query, etc.)
        await new Promise((r) => setTimeout(r, 10));
        return `result for ${ctx.argv[0]}\n`;
      },
      double: async (ctx) =>
        `${Number((ctx.stdin ?? "").trim()) * 2}\n`,
    },
  });

  const r1 = await bash.execute("fetch user-1");
  console.log(`fetch: ${r1.stdout.trim()}`);
  assert(r1.stdout === "result for user-1\n");

  // Sync + async in one pipeline
  const r2 = await bash.execute("echo 21 | double");
  console.log(`double: ${r2.stdout.trim()}`);
  assert(r2.stdout === "42\n");
}

async function demoPostConstruction() {
  console.log("\n=== addBuiltin / removeBuiltin after construction ===\n");

  const bash = new Bash();

  // Build up some state first…
  await bash.execute("mkdir -p /work && echo seed > /work/seed.txt");

  // …then register a builtin. The VFS is NOT disturbed.
  bash.addBuiltin("tally", (ctx) => `args=${ctx.argv.length}\n`);
  const r1 = await bash.execute("tally a b c");
  console.log(`tally: ${r1.stdout.trim()}`);
  assert(r1.stdout === "args=3\n");

  // Pre-existing file is still there
  const r2 = await bash.execute("cat /work/seed.txt");
  console.log(`seed: ${r2.stdout.trim()}`);
  assert(r2.stdout === "seed\n");

  // Remove when done
  bash.removeBuiltin("tally");
  const r3 = await bash.execute("tally a b c");
  assert(r3.exitCode === 127, "removed builtin should be unknown");
  console.log("tally after remove: command not found ✓");
}

async function demoOverride() {
  console.log("\n=== Override precedence ===\n");

  // Custom builtins win over baked-in builtins of the same name
  const bashEcho = new Bash({
    customBuiltins: {
      echo: (ctx) => `OVERRIDE:${ctx.argv.join(",")}\n`,
    },
  });
  const r1 = await bashEcho.execute("echo a b c");
  console.log(`echo override: ${r1.stdout.trim()}`);
  assert(r1.stdout === "OVERRIDE:a,b,c\n");

  // …but a shell function defined in the script still wins over the custom builtin
  const bashThing = new Bash({
    customBuiltins: {
      thing: () => "from-builtin\n",
    },
  });
  const r2 = await bashThing.execute(
    "thing() { printf 'from-function\\n'; }\nthing",
  );
  console.log(`shell-function: ${r2.stdout.trim()}`);
  assert(r2.stdout === "from-function\n");
}

async function demoErrorHandling() {
  console.log("\n=== Error handling ===\n");

  const bash = new Bash({
    customBuiltins: {
      fail: () => {
        throw new Error("nope");
      },
      "async-fail": async () => {
        throw new Error("async nope");
      },
    },
  });

  const r1 = await bash.execute("fail");
  console.log(`sync throw → exit ${r1.exitCode}, stderr: "${r1.stderr.trim()}"`);
  assert(r1.exitCode === 1);
  assert(r1.stderr.includes("nope"));

  const r2 = await bash.execute("async-fail");
  console.log(
    `async reject → exit ${r2.exitCode}, stderr: "${r2.stderr.trim()}"`,
  );
  assert(r2.exitCode === 1);
  assert(r2.stderr.includes("async nope"));
}

async function demoBashTool() {
  console.log("\n=== BashTool integration ===\n");

  // Same API on BashTool — useful when exposing to an LLM as a tool.
  const tool = new BashTool({
    customBuiltins: {
      "get-weather": async (ctx) => {
        const city = ctx.argv[0] ?? "unknown";
        return JSON.stringify({ city, temp: 72, sky: "clear" }) + "\n";
      },
    },
  });

  const r = await tool.execute(
    "get-weather 'San Francisco' | jq -r '.temp'",
  );
  console.log(`weather temp: ${r.stdout.trim()}°F`);
  assert(r.stdout.trim() === "72");
}

async function main() {
  await demoConstructor();
  await demoVfsPersistence();
  await demoAsync();
  await demoPostConstruction();
  await demoOverride();
  await demoErrorHandling();
  await demoBashTool();
  console.log("\n✓ all customBuiltins demos passed");
}

await main();
