import test from "ava";
import { Bash, BashTool, getVersion, BashError } from "../wrapper.js";

// ============================================================================
// Version
// ============================================================================

test("getVersion returns a semver string", (t) => {
  const v = getVersion();
  t.regex(v, /^\d+\.\d+\.\d+/);
});

// ============================================================================
// Bash — basic execution
// ============================================================================

test("Bash: echo command", (t) => {
  const bash = new Bash();
  const result = bash.executeSync('echo "hello"');
  t.is(result.exit_code, 0);
  t.is(result.stdout.trim(), "hello");
});

test("Bash: arithmetic", (t) => {
  const bash = new Bash();
  const result = bash.executeSync("echo $((10 * 5 - 3))");
  t.is(result.stdout.trim(), "47");
});

test("Bash: state persists between calls", (t) => {
  const bash = new Bash();
  bash.executeSync("X=42");
  const result = bash.executeSync("echo $X");
  t.is(result.stdout.trim(), "42");
});

test("Bash: file persistence across calls", (t) => {
  const bash = new Bash();
  bash.executeSync('echo "content" > /tmp/test.txt');
  const result = bash.executeSync("cat /tmp/test.txt");
  t.is(result.stdout.trim(), "content");
});

test("Bash: non-zero exit code on error", (t) => {
  const bash = new Bash();
  const result = bash.executeSync("false");
  t.not(result.exit_code, 0);
});

test("Bash: reset clears state", (t) => {
  const bash = new Bash();
  bash.executeSync("X=42");
  bash.reset();
  const result = bash.executeSync("echo ${X:-unset}");
  t.is(result.stdout.trim(), "unset");
});

// ============================================================================
// Bash — options
// ============================================================================

test("Bash: custom username", (t) => {
  const bash = new Bash({ username: "testuser" });
  const result = bash.executeSync("whoami");
  t.is(result.stdout.trim(), "testuser");
});

test("Bash: custom hostname", (t) => {
  const bash = new Bash({ hostname: "testhost" });
  const result = bash.executeSync("hostname");
  t.is(result.stdout.trim(), "testhost");
});

// ============================================================================
// Bash: executeSyncOrThrow
// ============================================================================

test("Bash: executeSyncOrThrow succeeds on exit 0", (t) => {
  const bash = new Bash();
  const result = bash.executeSyncOrThrow("echo ok");
  t.is(result.exit_code, 0);
});

test("Bash: executeSyncOrThrow throws on non-zero exit", (t) => {
  const bash = new Bash();
  const err = t.throws(
    () => bash.executeSyncOrThrow("false"),
    { instanceOf: BashError }
  );
  t.truthy(err);
});

// ============================================================================
// BashTool — metadata
// ============================================================================

test("BashTool: name is bashkit", (t) => {
  const tool = new BashTool();
  t.is(tool.name, "bashkit");
});

test("BashTool: version matches getVersion", (t) => {
  const tool = new BashTool();
  t.is(tool.version, getVersion());
});

test("BashTool: shortDescription is non-empty", (t) => {
  const tool = new BashTool();
  t.truthy(tool.shortDescription.length > 0);
});

test("BashTool: description is non-empty", (t) => {
  const tool = new BashTool();
  t.truthy(tool.description().length > 0);
});

test("BashTool: inputSchema is valid JSON", (t) => {
  const tool = new BashTool();
  const schema = JSON.parse(tool.inputSchema());
  t.truthy(schema);
  t.is(typeof schema, "object");
});

test("BashTool: outputSchema is valid JSON", (t) => {
  const tool = new BashTool();
  const schema = JSON.parse(tool.outputSchema());
  t.truthy(schema);
  t.is(typeof schema, "object");
});

test("BashTool: systemPrompt is non-empty", (t) => {
  const tool = new BashTool();
  t.truthy(tool.systemPrompt().length > 0);
});

test("BashTool: help is non-empty", (t) => {
  const tool = new BashTool();
  t.truthy(tool.help().length > 0);
});

// ============================================================================
// BashTool — execution
// ============================================================================

test("BashTool: execute echo", (t) => {
  const tool = new BashTool();
  const result = tool.executeSync('echo "hello from tool"');
  t.is(result.exit_code, 0);
  t.is(result.stdout.trim(), "hello from tool");
});

test("BashTool: execute with custom options", (t) => {
  const tool = new BashTool({ username: "agent", hostname: "ai-sandbox" });
  const result = tool.executeSync("whoami && hostname");
  t.true(result.stdout.includes("agent"));
  t.true(result.stdout.includes("ai-sandbox"));
});

test("BashTool: reset preserves config", (t) => {
  const tool = new BashTool({ username: "agent" });
  tool.executeSync("X=99");
  tool.reset();
  // State cleared
  const r1 = tool.executeSync("echo ${X:-unset}");
  t.is(r1.stdout.trim(), "unset");
  // Config preserved
  const r2 = tool.executeSync("whoami");
  t.is(r2.stdout.trim(), "agent");
});

// ============================================================================
// BashTool: multiline scripts
// ============================================================================

test("BashTool: multiline script", (t) => {
  const tool = new BashTool();
  const result = tool.executeSync(`
    add() {
      echo $(($1 + $2))
    }
    add 3 4
  `);
  t.is(result.stdout.trim(), "7");
});

// ============================================================================
// Multiple instances
// ============================================================================

test("Multiple Bash instances are isolated", (t) => {
  const a = new Bash();
  const b = new Bash();
  a.executeSync("X=from_a");
  b.executeSync("X=from_b");
  const ra = a.executeSync("echo $X");
  const rb = b.executeSync("echo $X");
  t.is(ra.stdout.trim(), "from_a");
  t.is(rb.stdout.trim(), "from_b");
});
