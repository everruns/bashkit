// Static script analysis: shape and evasion cases across Node, Bun, and Deno.

import { describe, it } from "node:test";
import assert from "node:assert/strict";
import { Bash, BashTool } from "./_setup.mjs";

describe("analyze", () => {
  it("reports commands, args, and redirects", () => {
    const analysis = new Bash().analyze(
      "cat notes.txt | grep -i todo > out.txt",
    );
    assert.deepEqual(analysis.commandNames, ["cat", "grep"]);
    assert.deepEqual(analysis.commands[1].args, ["-i", "todo"]);
    assert.equal(analysis.commands[0].context, "direct");
    assert.equal(analysis.redirects[0].path, "out.txt");
    assert.equal(analysis.redirects[0].mode, "write");
    assert.equal(analysis.redirects[0].isWrite, true);
    assert.equal(analysis.isOpaque, false);
  });

  it("reports non-literal words as null", () => {
    const analysis = new Bash().analyze('rm "$target" /tmp/fixed');
    assert.equal(analysis.commands[0].name, "rm");
    assert.equal(analysis.commands[0].args[0], null);
    assert.equal(analysis.commands[0].args[1], "/tmp/fixed");
  });

  it("flags dynamic dispatch as opaque", () => {
    const analysis = new Bash().analyze("$(echo rm) -rf /data");
    assert.equal(analysis.hasDynamicCommands, true);
    assert.equal(analysis.isOpaque, true);
  });

  it("flags eval as opaque", () => {
    const analysis = new Bash().analyze('eval "$payload"');
    assert.equal(analysis.hasEval, true);
    assert.equal(analysis.isOpaque, true);
  });

  it("flags shell -c as opaque", () => {
    const analysis = new Bash().analyze("bash -c 'rm -rf /data'");
    assert.equal(analysis.hasEval, true);
    assert.equal(analysis.isOpaque, true);
  });

  it("reports commands hidden in substitutions", () => {
    const analysis = new Bash().analyze("echo $(rm -rf /data)");
    assert.ok(analysis.commandNames.includes("rm"));
    assert.equal(analysis.commands[0].context, "substitution");
    assert.equal(analysis.hasCommandSubstitution, true);
  });

  it("reports functions and tags their bodies", () => {
    const analysis = new Bash().analyze("cleanup() { rm -rf /d; }\nls");
    assert.deepEqual(analysis.functions, ["cleanup"]);
    assert.equal(analysis.commands[0].context, "function_body");
    assert.equal(analysis.commands[1].context, "direct");
  });

  it("treats a bare assignment as naming no command", () => {
    const analysis = new Bash().analyze("FOO=1");
    assert.equal(analysis.commands[0].name, null);
    assert.equal(analysis.commands[0].isAssignmentOnly, true);
    assert.equal(analysis.hasDynamicCommands, false);
  });

  it("throws on parse errors", () => {
    assert.throws(() => new Bash().analyze("if true; then"));
  });

  it("does not execute the script", () => {
    const bash = new Bash();
    bash.analyze("mkdir -p /analyzed");
    assert.equal(bash.exists("/analyzed"), false);
  });

  it("is available on BashTool", () => {
    assert.deepEqual(new BashTool().analyze("ls -la").commandNames, ["ls"]);
  });
});
