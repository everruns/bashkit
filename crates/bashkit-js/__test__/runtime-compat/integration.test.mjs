import { describe, it } from "node:test";
import assert from "node:assert/strict";
import { Bash, BashError, BashTool, ScriptedTool } from "./_setup.mjs";

describe("integration", () => {
  it("multi-step file workflow", () => {
    const bash = new Bash();
    bash.executeSync("echo 'initial content' > /tmp/workflow.txt");
    bash.executeSync("echo 'appended line' >> /tmp/workflow.txt");

    assert.equal(
      bash.executeSync("wc -l < /tmp/workflow.txt").stdout.trim(),
      "2",
    );
    assert.equal(
      bash.executeSync("head -1 /tmp/workflow.txt").stdout.trim(),
      "initial content",
    );
  });

  it("async then sync on same instance", async () => {
    const bash = new Bash();
    assert.equal((await bash.execute("export PHASE=async")).exitCode, 0);
    assert.equal(bash.executeSync("echo $PHASE").stdout.trim(), "async");
  });

  it("BashTool reset clears state", () => {
    const tool = new BashTool({ username: "compat" });
    tool.executeSync("export SECRET=123");
    tool.executeSync("echo data > /tmp/toolreset.txt");
    tool.reset();

    assert.equal(
      tool.executeSync("echo ${SECRET:-cleared}").stdout.trim(),
      "cleared",
    );
    assert.equal(tool.executeSync("whoami").stdout.trim(), "compat");
  });

  it("parse failure sets error field", () => {
    const bash = new Bash();
    const result = bash.executeSync("echo $(");

    assert.notEqual(result.exitCode, 0);
    assert.ok(result.error);
    assert.ok(result.stderr);
  });

  it("ScriptedTool pipe workflow", async () => {
    const tool = new ScriptedTool({ name: "transform" });
    tool.addTool("upper", "Uppercase stdin", (_params, stdin) =>
      (stdin ?? "").toUpperCase(),
    );
    tool.addTool(
      "prefix",
      "Add prefix",
      (_params, stdin) => `PREFIX:${stdin ?? ""}`,
    );

    const result = await tool.execute("echo hello | upper | prefix");
    assert.equal(result.exitCode, 0);
    assert.equal(result.stdout.trim(), "PREFIX:HELLO");
  });

  it("async error propagation", async () => {
    const bash = new Bash();
    const result = await bash.execute("exit 42");
    assert.equal(result.exitCode, 42);
    assert.equal(result.success, false);

    await assert.rejects(() => bash.executeOrThrow("exit 42"), BashError);
  });
});
