import test from "ava";
import { BashTool, getVersion } from "../wrapper.js";

// ============================================================================
// BashTool — metadata getters
// ============================================================================

test("BashTool: name is bashkit", (t) => {
  const tool = new BashTool();
  t.is(tool.name, "bashkit");
});

test("BashTool: version matches getVersion", (t) => {
  const tool = new BashTool();
  t.is(tool.version, getVersion());
});

test("BashTool: version is a semver string", (t) => {
  const tool = new BashTool();
  t.regex(tool.version, /^\d+\.\d+\.\d+/);
});

test("BashTool: shortDescription is non-empty string", (t) => {
  const tool = new BashTool();
  t.is(typeof tool.shortDescription, "string");
  t.true(tool.shortDescription.length > 0);
});

// ============================================================================
// BashTool — description and help
// ============================================================================

test("BashTool: description is non-empty", (t) => {
  const tool = new BashTool();
  const d = tool.description();
  t.is(typeof d, "string");
  t.true(d.length > 10);
});

test("BashTool: help is non-empty", (t) => {
  const tool = new BashTool();
  const h = tool.help();
  t.is(typeof h, "string");
  t.true(h.length > 10);
});

test("BashTool: description and help are different", (t) => {
  const tool = new BashTool();
  t.not(tool.description(), tool.help());
});

// ============================================================================
// BashTool — system prompt
// ============================================================================

test("BashTool: systemPrompt is non-empty", (t) => {
  const tool = new BashTool();
  const sp = tool.systemPrompt();
  t.is(typeof sp, "string");
  t.true(sp.length > 10);
});

test("BashTool: systemPrompt mentions bash", (t) => {
  const tool = new BashTool();
  const sp = tool.systemPrompt().toLowerCase();
  t.true(sp.includes("bash"));
});

// ============================================================================
// BashTool — schemas
// ============================================================================

test("BashTool: inputSchema is valid JSON", (t) => {
  const tool = new BashTool();
  const schema = JSON.parse(tool.inputSchema());
  t.is(typeof schema, "object");
  t.truthy(schema);
});

test("BashTool: inputSchema has type property", (t) => {
  const tool = new BashTool();
  const schema = JSON.parse(tool.inputSchema());
  t.truthy(schema.type || schema.properties);
});

test("BashTool: inputSchema contains commands property", (t) => {
  const tool = new BashTool();
  const schema = JSON.parse(tool.inputSchema());
  // The schema should reference a "commands" input
  const str = JSON.stringify(schema);
  t.true(str.includes("command"));
});

test("BashTool: outputSchema is valid JSON", (t) => {
  const tool = new BashTool();
  const schema = JSON.parse(tool.outputSchema());
  t.is(typeof schema, "object");
  t.truthy(schema);
});

test("BashTool: schemas are stable across calls", (t) => {
  const tool = new BashTool();
  t.is(tool.inputSchema(), tool.inputSchema());
  t.is(tool.outputSchema(), tool.outputSchema());
});

test("BashTool: schemas are same across instances", (t) => {
  const a = new BashTool();
  const b = new BashTool();
  t.is(a.inputSchema(), b.inputSchema());
  t.is(a.outputSchema(), b.outputSchema());
});

// ============================================================================
// BashTool — metadata stability after execution
// ============================================================================

test("BashTool: metadata unchanged after execution", (t) => {
  const tool = new BashTool();
  const nameBefore = tool.name;
  const versionBefore = tool.version;
  const schemaBefore = tool.inputSchema();

  tool.executeSync("echo hello");
  tool.executeSync("X=1; Y=2; echo $((X+Y))");

  t.is(tool.name, nameBefore);
  t.is(tool.version, versionBefore);
  t.is(tool.inputSchema(), schemaBefore);
});

test("BashTool: metadata unchanged after reset", (t) => {
  const tool = new BashTool();
  const schemaBefore = tool.inputSchema();
  const descBefore = tool.description();

  tool.executeSync("echo hello");
  tool.reset();

  t.is(tool.inputSchema(), schemaBefore);
  t.is(tool.description(), descBefore);
});

test("BashTool: systemPrompt reflects configured home path", (t) => {
  const tool = new BashTool({ username: "agent", hostname: "sandbox" });
  const prompt = tool.systemPrompt();
  t.true(prompt.includes("agent"));
  t.true(prompt.includes("/home/agent"));
});

// ============================================================================
// BashTool — execution (basic coverage, deep tests in basic.spec.ts)
// ============================================================================

test("BashTool: execute returns ExecResult shape", (t) => {
  const tool = new BashTool();
  const r = tool.executeSync("echo test");
  t.is(typeof r.stdout, "string");
  t.is(typeof r.stderr, "string");
  t.is(typeof r.exitCode, "number");
});

test("BashTool: execute with options", (t) => {
  const tool = new BashTool({ username: "agent", hostname: "sandbox" });
  t.is(tool.executeSync("whoami").stdout.trim(), "agent");
  t.is(tool.executeSync("hostname").stdout.trim(), "sandbox");
});

test("BashTool: reset preserves config", (t) => {
  const tool = new BashTool({ username: "keep" });
  tool.executeSync("VAR=gone");
  tool.reset();
  t.is(tool.executeSync("echo ${VAR:-unset}").stdout.trim(), "unset");
  t.is(tool.executeSync("whoami").stdout.trim(), "keep");
});
