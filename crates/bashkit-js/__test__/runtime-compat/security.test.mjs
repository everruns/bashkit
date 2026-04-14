// Security: resource limits, sandbox escape, VFS path traversal, recovery.

import { describe, it } from "node:test";
import assert from "node:assert/strict";
import { Bash, ScriptedTool } from "./_setup.mjs";

function sleepMs(ms) {
  const signal = new Int32Array(new SharedArrayBuffer(4));
  Atomics.wait(signal, 0, 0, ms);
}

function nestedSchemaArray(depth) {
  let value = 1;
  for (let i = 0; i < depth; i++) {
    value = [value];
  }
  return value;
}

describe("security", () => {
  it("command limit enforced", () => {
    const bash = new Bash({ maxCommands: 5 });
    const r = bash.executeSync(
      "true; true; true; true; true; true; true; true; true; true",
    );
    assert.ok(r.exitCode !== 0 || r.error !== undefined);
  });

  it("loop iteration limit enforced", () => {
    const bash = new Bash({ maxLoopIterations: 5 });
    const r = bash.executeSync(
      "for i in 1 2 3 4 5 6 7 8 9 10; do echo $i; done",
    );
    assert.ok(r.exitCode !== 0 || r.error !== undefined);
  });

  it("infinite while loop capped", () => {
    const bash = new Bash({ maxLoopIterations: 10 });
    const r = bash.executeSync("i=0; while true; do i=$((i+1)); done");
    assert.ok(r.exitCode !== 0 || r.error !== undefined);
  });

  it("recursive function depth limited", () => {
    const bash = new Bash({ maxCommands: 10000 });
    const r = bash.executeSync("bomb() { bomb; }; bomb");
    assert.ok(r.exitCode !== 0 || r.error !== undefined);
  });

  it("sandbox escape blocked", () => {
    const bash = new Bash();
    assert.notEqual(bash.executeSync("exec /bin/bash").exitCode, 0);
    assert.notEqual(bash.executeSync("cat /proc/self/maps 2>&1").exitCode, 0);
    assert.notEqual(bash.executeSync("cat /etc/passwd 2>&1").exitCode, 0);
    assert.notEqual(
      bash
        .executeSync("echo test > /dev/udp/127.0.0.1/53 2>&1; echo $?")
        .stdout.trim(),
      "0",
    );
  });

  it("VFS path traversal blocked", () => {
    const bash = new Bash();
    bash.executeSync('echo "secret" > /home/data.txt');
    assert.notEqual(
      bash.executeSync("cat /home/../../../etc/shadow 2>&1").exitCode,
      0,
    );
  });

  it("recovery after exceeding limits", () => {
    const bash = new Bash({ maxCommands: 3 });
    bash.executeSync("true; true; true; true; true; true");
    const r = bash.executeSync("echo recovered");
    assert.equal(r.exitCode, 0);
    assert.equal(r.stdout.trim(), "recovered");
  });

  it("direct VFS write rejects file above size limit", () => {
    const bash = new Bash();
    assert.throws(
      () => bash.writeFile("/tmp/too-large.txt", "X".repeat(10_000_001)),
      /file too large/i,
    );
  });

  it("ScriptedTool slow callback completes", async () => {
    const tool = new ScriptedTool({ name: "slow" });
    tool.addTool("slow", "Slow callback", () => {
      sleepMs(50);
      return "done\n";
    });

    const result = await tool.execute("slow");
    assert.equal(result.exitCode, 0);
    assert.equal(result.stdout.trim(), "done");
  });

  it("ScriptedTool stdin injection stays literal", async () => {
    const tool = new ScriptedTool({ name: "echo_stdin" });
    tool.addTool("echo_stdin", "Echo stdin", (_params, stdin) => stdin ?? "");

    const result = await tool.execute("echo '$(echo injected)' | echo_stdin");
    assert.equal(result.exitCode, 0);
    assert.equal(result.stdout.trim(), "$(echo injected)");
  });

  it("ScriptedTool schema array nesting bomb is rejected", () => {
    const tool = new ScriptedTool({ name: "array_bomb" });
    assert.throws(
      () =>
        tool.addTool("test", "Array bomb", () => "ok\n", nestedSchemaArray(70)),
      /nesting depth exceeds maximum of 64/i,
    );
  });

  it("environment stays isolated across instances", () => {
    const first = new Bash();
    first.executeSync("export EVIL=payload");
    assert.equal(first.executeSync("echo $EVIL").stdout.trim(), "payload");

    const second = new Bash();
    assert.equal(
      second.executeSync("echo ${EVIL:-clean}").stdout.trim(),
      "clean",
    );
  });
});
