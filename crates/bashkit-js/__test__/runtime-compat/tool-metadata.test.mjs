// BashTool metadata: name, version, schemas, description, help, systemPrompt,
// stability, execution, reset, isolation.

import { describe, it } from "node:test";
import assert from "node:assert/strict";
import { BashTool, getVersion } from "./_setup.mjs";

describe("BashTool metadata", () => {
  it("name, version, shortDescription", () => {
    const tool = new BashTool();
    assert.equal(tool.name, "bashkit");
    assert.match(tool.version, /^\d+\.\d+\.\d+/);
    assert.equal(tool.version, getVersion());
    assert.ok(tool.shortDescription.length > 0);
  });

  it("description, help, systemPrompt", () => {
    const tool = new BashTool();
    assert.ok(tool.description().length > 10);
    assert.ok(tool.help().length > 10);
    assert.notEqual(tool.description(), tool.help());
    assert.ok(tool.systemPrompt().toLowerCase().includes("bash"));
  });

  it("inputSchema and outputSchema are valid JSON", () => {
    const tool = new BashTool();
    const input = JSON.parse(tool.inputSchema());
    const output = JSON.parse(tool.outputSchema());
    assert.equal(typeof input, "object");
    assert.equal(typeof output, "object");
    assert.ok(JSON.stringify(input).includes("command"));
  });

  it("schemas stable across calls and instances", () => {
    const a = new BashTool();
    const b = new BashTool();
    assert.equal(a.inputSchema(), a.inputSchema());
    assert.equal(a.inputSchema(), b.inputSchema());
    assert.equal(a.outputSchema(), b.outputSchema());
  });

  it("metadata unchanged after execution and reset", () => {
    const tool = new BashTool();
    const nameBefore = tool.name;
    const schemaBefore = tool.inputSchema();
    tool.executeSync("echo hello");
    tool.reset();
    assert.equal(tool.name, nameBefore);
    assert.equal(tool.inputSchema(), schemaBefore);
  });

  it("systemPrompt reflects configured username", () => {
    const tool = new BashTool({ username: "agent", hostname: "sandbox" });
    const prompt = tool.systemPrompt();
    assert.ok(prompt.includes("agent"));
    assert.ok(prompt.includes("/home/agent"));
  });

  it("BashTool execution and reset", () => {
    const tool = new BashTool({ username: "keep" });
    tool.executeSync("VAR=gone");
    tool.reset();
    assert.equal(tool.executeSync("echo ${VAR:-unset}").stdout.trim(), "unset");
    assert.equal(tool.executeSync("whoami").stdout.trim(), "keep");
  });

  it("BashTool instances are isolated", () => {
    const a = new BashTool();
    const b = new BashTool();
    a.executeSync("VAR=toolA");
    b.executeSync("VAR=toolB");
    assert.equal(a.executeSync("echo $VAR").stdout.trim(), "toolA");
    assert.equal(b.executeSync("echo $VAR").stdout.trim(), "toolB");
  });
});
