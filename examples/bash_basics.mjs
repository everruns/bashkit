#!/usr/bin/env node
/**
 * Basic usage of the Bash and BashTool interfaces.
 *
 * Demonstrates core features: command execution, pipelines, variables,
 * loops, virtual filesystem persistence, jq, error handling, and reset.
 *
 * Run:
 *   node examples/bash_basics.mjs
 */

import { Bash, BashTool, getVersion, BashError } from "@everruns/bashkit";

function demoBasics() {
  console.log("=== Basic Execution ===\n");

  const bash = new Bash();

  // Simple command
  const r1 = bash.executeSync('echo "Hello from Bashkit!"');
  console.log(`echo: ${r1.stdout.trim()}`);
  assert(r1.exitCode === 0);

  // Pipeline
  const r2 = bash.executeSync("echo -e 'banana\\napple\\ncherry' | sort");
  console.log(`sort: ${r2.stdout.trim()}`);
  assert(r2.stdout.trim() === "apple\nbanana\ncherry");

  // Arithmetic
  const r3 = bash.executeSync("echo $((10 * 5 - 3))");
  console.log(`math: ${r3.stdout.trim()}`);
  assert(r3.stdout.trim() === "47");

  console.log();
}

function demoState() {
  console.log("=== State Persistence ===\n");

  const bash = new Bash();

  // Variables persist across calls
  bash.executeSync("MY_VAR='persistent'");
  const r1 = bash.executeSync("echo $MY_VAR");
  console.log(`var:  ${r1.stdout.trim()}`);
  assert(r1.stdout.trim() === "persistent");

  // Virtual filesystem persists
  bash.executeSync("mkdir -p /tmp/demo && echo 'data' > /tmp/demo/file.txt");
  const r2 = bash.executeSync("cat /tmp/demo/file.txt");
  console.log(`file: ${r2.stdout.trim()}`);
  assert(r2.stdout.trim() === "data");

  // Loops and arithmetic
  const r3 = bash.executeSync(`
    total=0
    for i in 1 2 3 4 5; do
      total=$((total + i))
    done
    echo $total
  `);
  console.log(`sum:  ${r3.stdout.trim()}`);
  assert(r3.stdout.trim() === "15");

  // Reset clears state
  bash.reset();
  const r4 = bash.executeSync("echo ${MY_VAR:-unset}");
  console.log(`reset: ${r4.stdout.trim()}`);
  assert(r4.stdout.trim() === "unset");

  console.log();
}

function demoErrorHandling() {
  console.log("=== Error Handling ===\n");

  const bash = new Bash();

  // Exit codes
  const r1 = bash.executeSync("exit 42");
  console.log(`exit: code=${r1.exitCode}`);
  assert(r1.exitCode === 42);

  // executeSyncOrThrow
  try {
    bash.executeSyncOrThrow("exit 1");
    assert(false, "should have thrown");
  } catch (e) {
    console.log(`thrown: ${e instanceof BashError ? "BashError" : "other"}`);
    assert(e instanceof BashError);
    console.log(`display: ${e.display()}`);
  }

  // Recover after error
  const r2 = bash.executeSync("echo 'recovered'");
  console.log(`recovered: ${r2.stdout.trim()}`);
  assert(r2.exitCode === 0);

  console.log();
}

function demoJq() {
  console.log("=== JSON Processing (jq) ===\n");

  const bash = new Bash();

  // Create JSON data
  bash.executeSync(`cat > /tmp/users.json << 'EOF'
[
  {"name": "Alice", "role": "admin"},
  {"name": "Bob", "role": "user"},
  {"name": "Carol", "role": "admin"}
]
EOF`);

  // Query with jq
  const r1 = bash.executeSync(
    'cat /tmp/users.json | jq \'[.[] | select(.role == "admin")] | length\''
  );
  console.log(`admins: ${r1.stdout.trim()}`);
  assert(r1.stdout.trim() === "2");

  // Extract names
  const r2 = bash.executeSync(
    "cat /tmp/users.json | jq -r '.[].name' | sort"
  );
  console.log(`names: ${r2.stdout.trim()}`);
  assert(r2.stdout.trim() === "Alice\nBob\nCarol");

  console.log();
}

function demoConfig() {
  console.log("=== Configuration ===\n");

  const bash = new Bash({ username: "agent", hostname: "sandbox" });
  console.log(`whoami:   ${bash.executeSync("whoami").stdout.trim()}`);
  console.log(`hostname: ${bash.executeSync("hostname").stdout.trim()}`);
  assert(bash.executeSync("whoami").stdout.trim() === "agent");
  assert(bash.executeSync("hostname").stdout.trim() === "sandbox");

  console.log();
}

function demoBashTool() {
  console.log("=== BashTool Metadata ===\n");

  const tool = new BashTool();
  console.log(`name:    ${tool.name}`);
  console.log(`version: ${tool.version}`);
  console.log(`short:   ${tool.shortDescription}`);
  console.log(`schema:  ${tool.inputSchema().substring(0, 60)}...`);
  assert(tool.name === "bashkit");
  assert(tool.version === getVersion());

  // Execute through BashTool
  const r = tool.executeSync("echo 'hello from BashTool'");
  console.log(`exec:    ${r.stdout.trim()}`);
  assert(r.exitCode === 0);

  console.log();
}

// ============================================================================

function assert(condition, msg = "assertion failed") {
  if (!condition) throw new Error(msg);
}

function main() {
  console.log(`Bashkit v${getVersion()} — Bash basics examples\n`);
  demoBasics();
  demoState();
  demoErrorHandling();
  demoJq();
  demoConfig();
  demoBashTool();
  console.log("All examples passed.");
}

main();
