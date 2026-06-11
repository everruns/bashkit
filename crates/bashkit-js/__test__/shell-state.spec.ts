import test from "ava";
import { Bash, BashTool } from "../wrapper.js";

// ----------------------------------------------------------------------------
// shellState() — lightweight inspection snapshot (parity with the Python
// binding's `shell_state()`).
// ----------------------------------------------------------------------------

test("shellState captures variables, env, and cwd", async (t) => {
  const bash = new Bash();
  await bash.execute(
    ["x=42", "export GREETING=hello", "mkdir -p /work", "cd /work"].join("\n"),
  );
  const state = bash.shellState();
  t.is(state.variables.x, "42");
  t.is(state.env.GREETING, "hello");
  t.is(state.cwd, "/work");
});

test("shellState captures lastExitCode", async (t) => {
  const bash = new Bash();
  await bash.execute("false");
  t.is(bash.shellState().lastExitCode, 1);
  await bash.execute("true");
  t.is(bash.shellState().lastExitCode, 0);
});

test("shellState captures indexed and associative arrays", async (t) => {
  const bash = new Bash();
  await bash.execute(
    ["arr=(a b c)", "arr[10]=sparse", "declare -A map", "map[key]=value"].join(
      "\n",
    ),
  );
  const state = bash.shellState();
  t.deepEqual(state.arrays.arr, {
    "0": "a",
    "1": "b",
    "2": "c",
    "10": "sparse",
  });
  t.deepEqual(state.assocArrays.map, { key: "value" });
});

test("shellState captures aliases and traps", async (t) => {
  const bash = new Bash();
  await bash.execute(["alias ll='ls -l'", "trap 'echo bye' EXIT"].join("\n"));
  const state = bash.shellState();
  t.is(state.aliases.ll, "ls -l");
  t.is(state.traps.EXIT, "echo bye");
});

test("shellState works on BashTool", async (t) => {
  const tool = new BashTool();
  await tool.execute("y=7");
  t.is(tool.shellState().variables.y, "7");
});

test("shellState reflects state after reset()", async (t) => {
  const bash = new Bash();
  await bash.execute("x=1");
  t.is(bash.shellState().variables.x, "1");
  bash.reset();
  t.false("x" in bash.shellState().variables);
});
