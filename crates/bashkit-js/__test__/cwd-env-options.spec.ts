import test from "ava";
import { Bash, BashTool } from "../wrapper.js";

// ----------------------------------------------------------------------------
// cwd / env constructor options (issue #2068) — set the starting directory and
// initial environment without a leading `cd`/`export` prelude.
// ----------------------------------------------------------------------------

test("cwd option sets the starting working directory", async (t) => {
  const bash = new Bash({ cwd: "/home/user" });
  const result = await bash.execute("pwd");
  t.is(result.stdout, "/home/user\n");
  t.is(bash.shellState().cwd, "/home/user");
});

test("env option exposes variables without an export prelude", async (t) => {
  const bash = new Bash({ env: { HOME: "/home/user", LANG: "en_US.UTF-8" } });
  const result = await bash.execute('echo "$HOME $LANG"');
  t.is(result.stdout, "/home/user en_US.UTF-8\n");
  t.is(bash.shellState().env.HOME, "/home/user");
});

test("cwd and env survive reset()", async (t) => {
  const bash = new Bash({ cwd: "/home/user", env: { TOKEN: "abc" } });
  await bash.execute("cd / ; unset TOKEN");
  bash.reset();
  const result = await bash.execute('pwd ; echo "$TOKEN"');
  t.is(result.stdout, "/home/user\nabc\n");
});

test("cwd and env options work on BashTool", async (t) => {
  const tool = new BashTool({ cwd: "/srv", env: { APP: "bashkit" } });
  const result = await tool.execute('pwd ; echo "$APP"');
  t.is(result.stdout, "/srv\nbashkit\n");
});

test("env option appears in BashTool help", (t) => {
  const tool = new BashTool({ env: { API_BASE: "https://example.com" } });
  t.true(tool.help().includes("API_BASE"));
});
