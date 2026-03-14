import test from "ava";
import { Bash, BashTool, BashError } from "../wrapper.js";

// ============================================================================
// ExecResult error fields
// ============================================================================

test("successful command has no error", (t) => {
  const bash = new Bash();
  const r = bash.executeSync("echo ok");
  t.falsy(r.error);
  t.is(r.exitCode, 0);
});

test("failed command has non-zero exit code", (t) => {
  const bash = new Bash();
  const r = bash.executeSync("false");
  t.not(r.exitCode, 0);
});

test("exit with specific code", (t) => {
  const bash = new Bash();
  t.is(bash.executeSync("exit 0").exitCode, 0);
  t.is(bash.executeSync("exit 1").exitCode, 1);
  t.is(bash.executeSync("exit 127").exitCode, 127);
});

test("command not found produces stderr", (t) => {
  const bash = new Bash();
  const r = bash.executeSync("nonexistent_command_xyz");
  t.not(r.exitCode, 0);
  t.true(r.stderr.length > 0);
});

test("stderr captured separately from stdout", (t) => {
  const bash = new Bash();
  const r = bash.executeSync("echo out; echo err >&2");
  t.true(r.stdout.includes("out"));
  t.true(r.stderr.includes("err"));
});

// ============================================================================
// BashError class
// ============================================================================

test("BashError is instanceof Error", (t) => {
  const bash = new Bash();
  try {
    bash.executeSyncOrThrow("exit 1");
    t.fail("should have thrown");
  } catch (e) {
    t.true(e instanceof Error);
    t.true(e instanceof BashError);
  }
});

test("BashError.name is BashError", (t) => {
  const bash = new Bash();
  try {
    bash.executeSyncOrThrow("exit 1");
    t.fail("should throw");
  } catch (e) {
    t.is((e as BashError).name, "BashError");
  }
});

test("BashError.exitCode matches exit code", (t) => {
  const bash = new Bash();
  for (const code of [1, 2, 42, 127]) {
    try {
      bash.executeSyncOrThrow(`exit ${code}`);
      t.fail("should throw");
    } catch (e) {
      t.is((e as BashError).exitCode, code);
    }
  }
});

test("BashError.stderr contains error output", (t) => {
  const bash = new Bash();
  try {
    bash.executeSyncOrThrow("echo oops >&2; exit 1");
    t.fail("should throw");
  } catch (e) {
    t.true((e as BashError).stderr.includes("oops"));
  }
});

test("BashError.display() returns formatted string", (t) => {
  const bash = new Bash();
  try {
    bash.executeSyncOrThrow("exit 3");
    t.fail("should throw");
  } catch (e) {
    const display = (e as BashError).display();
    t.true(display.includes("BashError"));
    t.true(display.includes("3"));
  }
});

test("BashError.message is string", (t) => {
  const bash = new Bash();
  try {
    bash.executeSyncOrThrow("false");
    t.fail("should throw");
  } catch (e) {
    t.is(typeof (e as BashError).message, "string");
  }
});

// ============================================================================
// executeSyncOrThrow does not throw on success
// ============================================================================

test("executeSyncOrThrow returns result on success", (t) => {
  const bash = new Bash();
  const r = bash.executeSyncOrThrow("echo hello");
  t.is(r.exitCode, 0);
  t.is(r.stdout.trim(), "hello");
});

test("BashTool executeSyncOrThrow returns result on success", (t) => {
  const tool = new BashTool();
  const r = tool.executeSyncOrThrow("echo from_tool");
  t.is(r.exitCode, 0);
  t.is(r.stdout.trim(), "from_tool");
});

test("BashTool executeSyncOrThrow throws on failure", (t) => {
  const tool = new BashTool();
  t.throws(() => tool.executeSyncOrThrow("false"), {
    instanceOf: BashError,
  });
});

// ============================================================================
// Error recovery
// ============================================================================

test("interpreter usable after error", (t) => {
  const bash = new Bash();
  bash.executeSync("false");
  const r = bash.executeSync("echo recovered");
  t.is(r.exitCode, 0);
  t.is(r.stdout.trim(), "recovered");
});

test("state preserved after error", (t) => {
  const bash = new Bash();
  bash.executeSync("X=before");
  bash.executeSync("false");
  t.is(bash.executeSync("echo $X").stdout.trim(), "before");
});

test("executeSyncOrThrow does not corrupt state", (t) => {
  const bash = new Bash();
  bash.executeSync("Y=safe");
  try {
    bash.executeSyncOrThrow("false");
  } catch {
    // expected
  }
  t.is(bash.executeSync("echo $Y").stdout.trim(), "safe");
});

// ============================================================================
// Multiple sequential errors
// ============================================================================

test("multiple errors in sequence", (t) => {
  const bash = new Bash();
  const r1 = bash.executeSync("false");
  const r2 = bash.executeSync("exit 2");
  const r3 = bash.executeSync("echo ok");
  t.not(r1.exitCode, 0);
  t.is(r2.exitCode, 2);
  t.is(r3.exitCode, 0);
  t.is(r3.stdout.trim(), "ok");
});

// ============================================================================
// Syntax errors
// ============================================================================

test("syntax error returns non-zero", (t) => {
  const bash = new Bash();
  const r = bash.executeSync("if then fi");
  t.not(r.exitCode, 0);
});

test("unclosed quote returns non-zero or handles gracefully", (t) => {
  const bash = new Bash();
  const r = bash.executeSync('echo "unclosed');
  // Should either error or handle gracefully
  t.is(typeof r.exitCode, "number");
});

// ============================================================================
// Pre-exec failure stderr surfacing (issue #606)
// ============================================================================

test("pre-exec parse error surfaces in stderr (Bash)", (t) => {
  const bash = new Bash();
  const r = bash.executeSync("echo $(");
  t.not(r.exitCode, 0);
  t.truthy(r.error, "error field should contain the parse error");
  t.truthy(r.stderr, "stderr must not be empty on pre-exec failure");
  t.true(r.stderr.includes(r.error!), "stderr should contain the error message");
});

test("pre-exec parse error surfaces in stderr (BashTool)", (t) => {
  const tool = new BashTool();
  const r = tool.executeSync("echo $(");
  t.not(r.exitCode, 0);
  t.truthy(r.error, "error field should contain the parse error");
  t.truthy(r.stderr, "stderr must not be empty on pre-exec failure");
  t.true(r.stderr.includes(r.error!), "stderr should contain the error message");
});
