// Security: resource limits, sandbox escape, VFS path traversal, recovery.

import { describe, it } from "node:test";
import assert from "node:assert/strict";
import { Bash } from "./_setup.mjs";

describe("security", () => {
  it("command limit enforced", () => {
    const bash = new Bash({ maxCommands: 5 });
    const r = bash.executeSync("true; true; true; true; true; true; true; true; true; true");
    assert.ok(r.exitCode !== 0 || r.error !== undefined);
  });

  it("loop iteration limit enforced", () => {
    const bash = new Bash({ maxLoopIterations: 5 });
    const r = bash.executeSync("for i in 1 2 3 4 5 6 7 8 9 10; do echo $i; done");
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
  });

  it("VFS path traversal blocked", () => {
    const bash = new Bash();
    bash.executeSync('echo "secret" > /home/data.txt');
    assert.notEqual(bash.executeSync("cat /home/../../../etc/shadow 2>&1").exitCode, 0);
  });

  it("recovery after exceeding limits", () => {
    const bash = new Bash({ maxCommands: 3 });
    bash.executeSync("true; true; true; true; true; true");
    const r = bash.executeSync("echo recovered");
    assert.equal(r.exitCode, 0);
    assert.equal(r.stdout.trim(), "recovered");
  });
});
