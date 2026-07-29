// Static script analysis surfaced to JS: shape, evasion cases, and the
// permission-gate pattern it exists for.

import test from "ava";
import { Bash, BashTool } from "../wrapper.js";
import type { ScriptAnalysis } from "../wrapper.js";

const READ_ONLY = ["cat", "grep", "ls", "echo", "wc", "head"];

/** The documented host gate: allowlisted commands in a transparent script. */
function allowed(analysis: ScriptAnalysis): boolean {
  if (analysis.isOpaque) return false;
  return analysis.commands.every(
    (c) => c.isAssignmentOnly || (c.name != null && READ_ONLY.includes(c.name)),
  );
}

// ============================================================================
// Shape
// ============================================================================

test("analyze: reports commands, args, and redirects", (t) => {
  const bash = new Bash();
  const analysis = bash.analyze("cat notes.txt | grep -i todo > out.txt");

  t.deepEqual(analysis.commandNames, ["cat", "grep"]);
  t.deepEqual(analysis.commands[0].args, ["notes.txt"]);
  t.deepEqual(analysis.commands[1].args, ["-i", "todo"]);
  t.is(analysis.commands[0].context, "direct");
  t.is(analysis.redirects.length, 1);
  t.is(analysis.redirects[0].path, "out.txt");
  t.is(analysis.redirects[0].mode, "write");
  t.true(analysis.redirects[0].isWrite);
  t.false(analysis.isOpaque);
  t.false(analysis.hasDynamicCommands);
  t.false(analysis.hasEval);
  t.false(analysis.hasCommandSubstitution);
  t.false(analysis.truncated);
  t.deepEqual(analysis.functions, []);
});

test("analyze: redirect modes", (t) => {
  const bash = new Bash();
  const analysis = bash.analyze("cat < in >> log 2>&1");
  t.is(analysis.redirects.length, 2, "fd duplication names no file");
  t.is(analysis.redirects[0].mode, "read");
  t.false(analysis.redirects[0].isWrite);
  t.is(analysis.redirects[1].mode, "append");
  t.true(analysis.redirects[1].isWrite);
});

test("analyze: non-literal words are null, not partial strings", (t) => {
  const bash = new Bash();
  const analysis = bash.analyze('rm "$target" /tmp/fixed /tmp/$name.txt');
  const [command] = analysis.commands;

  t.is(command.name, "rm");
  t.is(command.args[0], null);
  t.is(command.args[1], "/tmp/fixed");
  t.is(command.args[2], null, "partial literals are not reconstructed");
  t.false(
    analysis.hasDynamicCommands,
    "a dynamic arg is not a dynamic command",
  );
});

test("analyze: function definitions and contexts", (t) => {
  const bash = new Bash();
  const analysis = bash.analyze("cleanup() { rm -rf /data; }\necho $(date)");

  t.deepEqual(analysis.functions, ["cleanup"]);
  t.is(analysis.commands[0].name, "rm");
  t.is(analysis.commands[0].context, "function_body");
  t.is(analysis.commands[1].context, "substitution");
  t.is(analysis.commands[2].context, "direct");
  t.true(analysis.hasCommandSubstitution);
});

test("analyze: assignment-only command is not dynamic", (t) => {
  const bash = new Bash();
  const analysis = bash.analyze("FOO=1");

  t.is(analysis.commands[0].name, null);
  t.deepEqual(analysis.commands[0].assignments, ["FOO"]);
  t.true(analysis.commands[0].isAssignmentOnly);
  t.false(analysis.hasDynamicCommands);
  t.false(analysis.isOpaque);
});

test("analyze: prefix assignments ride on the command", (t) => {
  const bash = new Bash();
  const analysis = bash.analyze("FOO=1 BAR=2 env");
  t.is(analysis.commands[0].name, "env");
  t.deepEqual(analysis.commands[0].assignments, ["FOO", "BAR"]);
  t.false(analysis.commands[0].isAssignmentOnly);
});

// ============================================================================
// Evasion — must surface as unknown, never as safe
// ============================================================================

test("analyze: dynamic dispatch is opaque", (t) => {
  const bash = new Bash();
  for (const script of [
    "$cmd -rf /data",
    "c=rm; $c -rf /data",
    "$(echo rm) -rf /data",
  ]) {
    const analysis = bash.analyze(script);
    t.true(analysis.hasDynamicCommands, script);
    t.true(analysis.isOpaque, script);
    t.false(allowed(analysis), script);
  }
});

test("analyze: eval, source, and shell -c are opaque", (t) => {
  const bash = new Bash();
  for (const script of [
    'eval "$payload"',
    "source /tmp/x.sh",
    ". /tmp/x.sh",
    "bash -c 'rm -rf /data'",
    'sh -c "$payload"',
  ]) {
    const analysis = bash.analyze(script);
    t.true(analysis.hasEval, script);
    t.true(analysis.isOpaque, script);
  }
});

test("analyze: commands hidden in substitutions are reported", (t) => {
  const bash = new Bash();
  for (const script of [
    "echo $(rm -rf /data)",
    "echo `rm -rf /data`",
    "cat <(rm -rf /data)",
  ]) {
    const analysis = bash.analyze(script);
    t.true(analysis.commandNames.includes("rm"), script);
    t.false(allowed(analysis), script);
  }
});

test("analyze: commands hidden in branches, loops, and functions are reported", (t) => {
  const bash = new Bash();
  for (const script of [
    "if true; then rm -rf /data; fi",
    "for f in a b; do rm $f; done",
    "case x in a) rm y ;; esac",
    "cleanup() { rm -rf /data; }; cleanup",
  ]) {
    t.true(bash.analyze(script).commandNames.includes("rm"), script);
  }
});

test("analyze: parse errors throw", (t) => {
  const bash = new Bash();
  t.throws(() => bash.analyze("if true; then"));
  t.throws(() => bash.analyze("echo 'unterminated"));
});

test("analyze: oversized scripts truncate and stay opaque", (t) => {
  const bash = new Bash();
  const analysis = bash.analyze("echo x;".repeat(5000));
  t.true(analysis.truncated);
  t.true(analysis.isOpaque);
  t.false(allowed(analysis));
});

// ============================================================================
// Behavior
// ============================================================================

test("analyze: does not execute or mutate state", (t) => {
  const bash = new Bash();
  bash.analyze("mkdir -p /analyzed && echo marker > /analyzed/f");
  t.false(bash.exists("/analyzed"));

  bash.analyze("FOO=bar");
  t.is(bash.executeSync('echo "[$FOO]"').stdout.trim(), "[]");
});

test("analyze: honors the instance input-size limit", (t) => {
  const strict = new Bash({ maxInputBytes: 16 });
  t.notThrows(() => strict.analyze("echo hi"));
  t.throws(() => strict.analyze(`echo ${"x".repeat(100)}`));
  t.notThrows(() => new Bash().analyze(`echo ${"x".repeat(100)}`));
});

test("analyze: allows a transparent read-only script", (t) => {
  const bash = new Bash();
  t.true(allowed(bash.analyze("cat notes.txt | grep -i todo | wc -l")));
  t.true(allowed(bash.analyze("N=3; ls | head -n 3")));
  t.false(allowed(bash.analyze("rm -rf /data")));
});

test("analyze: derives fine-grained permission keys", (t) => {
  const bash = new Bash();
  const key = (script: string): string | null => {
    const command = bash
      .analyze(script)
      .commands.find((c) => c.name === "mydata");
    const [resource, action] = command?.args ?? [];
    if (resource == null || action == null) return null;
    if (["list", "get", "query"].includes(action)) return null;
    return `mydata:${resource}:${action}`;
  };

  t.is(key("mydata record query 42"), null);
  t.is(key("mydata record delete 42"), "mydata:record:delete");
  t.is(key("cat x | mydata doc write 7"), "mydata:doc:write");
  t.is(key("mydata record $action"), null, "computed action is not a key");
});

test("analyze: custom builtins are visible to analysis", (t) => {
  const bash = new Bash({
    customBuiltins: { mydata: () => "ok\n" },
  });
  const analysis = bash.analyze("mydata record delete 1");
  t.deepEqual(analysis.commandNames, ["mydata"]);
  t.deepEqual(analysis.commands[0].args, ["record", "delete", "1"]);
});

test("BashTool: analyze is available", (t) => {
  const tool = new BashTool();
  const analysis = tool.analyze("ls -la /tmp");
  t.deepEqual(analysis.commandNames, ["ls"]);
  t.false(analysis.isOpaque);
  t.throws(() => tool.analyze("if true; then"));
});
