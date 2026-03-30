// Error handling: exit codes, stderr, BashError, executeSyncOrThrow,
// recovery, syntax errors, parse errors.

import { describe, it } from "node:test";
import assert from "node:assert/strict";
import { Bash, BashError } from "./_setup.mjs";

describe("error handling", () => {
  it("failed command has non-zero exit code", () => {
    assert.notEqual(new Bash().executeSync("false").exitCode, 0);
  });

  it("exit with specific codes", () => {
    const bash = new Bash();
    assert.equal(bash.executeSync("exit 0").exitCode, 0);
    assert.equal(bash.executeSync("exit 1").exitCode, 1);
    assert.equal(bash.executeSync("exit 127").exitCode, 127);
  });

  it("stderr captured separately", () => {
    const bash = new Bash();
    const r = bash.executeSync("echo out; echo err >&2");
    assert.ok(r.stdout.includes("out"));
    assert.ok(r.stderr.includes("err"));
  });

  it("executeSyncOrThrow succeeds on exit 0", () => {
    const bash = new Bash();
    const r = bash.executeSyncOrThrow("echo ok");
    assert.equal(r.exitCode, 0);
    assert.equal(r.stdout.trim(), "ok");
  });

  it("executeSyncOrThrow throws on failure", () => {
    const bash = new Bash();
    assert.throws(() => bash.executeSyncOrThrow("exit 42"), (err) => {
      assert.equal(err.name, "BashError");
      assert.equal(err.exitCode, 42);
      assert.equal(typeof err.message, "string");
      assert.ok(err.display().includes("BashError"));
      return true;
    });
  });

  it("interpreter usable after error, state preserved", () => {
    const bash = new Bash();
    bash.executeSync("X=before");
    bash.executeSync("false");
    assert.equal(bash.executeSync("echo $X").stdout.trim(), "before");
    assert.equal(bash.executeSync("echo recovered").stdout.trim(), "recovered");
  });

  it("syntax error returns non-zero", () => {
    assert.notEqual(new Bash().executeSync("if then fi").exitCode, 0);
  });

  it("pre-exec parse error surfaces in stderr", () => {
    const bash = new Bash();
    const r = bash.executeSync("echo $(");
    assert.notEqual(r.exitCode, 0);
    assert.ok(r.error);
    assert.ok(r.stderr.length > 0);
  });
});
