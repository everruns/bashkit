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

// ----------------------------------------------------------------------------
// Session-wide limits: maxExecCalls / maxSessionCommands. The core caps
// execute() calls per instance (default 1,000); hosts can raise or lower it.
// ----------------------------------------------------------------------------

async function runOrError(
  bash: {
    execute(
      s: string,
    ): Promise<{ exitCode: number; stderr: string; error?: string | null }>;
  },
  script: string,
): Promise<{ exitCode: number; text: string }> {
  try {
    const r = await bash.execute(script);
    return { exitCode: r.exitCode, text: `${r.error ?? ""}${r.stderr}` };
  } catch (e) {
    return { exitCode: -1, text: String(e) };
  }
}

test("maxExecCalls caps execute() calls per session", async (t) => {
  const bash = new Bash({ maxExecCalls: 2 });
  t.is((await runOrError(bash, "true")).exitCode, 0);
  t.is((await runOrError(bash, "true")).exitCode, 0);
  const third = await runOrError(bash, "true");
  t.not(third.exitCode, 0);
  t.true(
    third.text.includes("session exec() call limit exceeded (2 calls)"),
    third.text,
  );
});

test("maxExecCalls raises the default cap and survives reset()", async (t) => {
  const bash = new Bash({ maxExecCalls: 1_200 });
  for (let i = 0; i < 1_100; i++) {
    const r = await runOrError(bash, "true");
    if (r.exitCode !== 0) t.fail(r.text);
  }
  const small = new Bash({ maxExecCalls: 1 });
  t.is((await runOrError(small, "true")).exitCode, 0);
  t.not((await runOrError(small, "true")).exitCode, 0);
  small.reset();
  t.is((await runOrError(small, "true")).exitCode, 0);
  t.pass();
});

test("maxSessionCommands caps commands across calls", async (t) => {
  const bash = new Bash({ maxSessionCommands: 3 });
  t.is((await runOrError(bash, "true; true")).exitCode, 0);
  const second = await runOrError(bash, "true; true");
  t.not(second.exitCode, 0);
  t.true(second.text.includes("session command limit exceeded"), second.text);
});

test("BashTool accepts maxExecCalls", async (t) => {
  const tool = new BashTool({ maxExecCalls: 1 });
  t.is((await runOrError(tool, "true")).exitCode, 0);
  t.not((await runOrError(tool, "true")).exitCode, 0);
});
