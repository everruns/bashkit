// Runtime host mutations — setEnv + mounts applied after construction, and
// their survival across reset() (issue #2291).
//
// The motivating shape is an "extension": a reusable function that mounts a
// filesystem, sets env, and registers a builtin on an existing instance. All
// three parts must behave the same way across reset(), or the extension is
// half-installed after a rebuild.

import test from "ava";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { Bash, BashTool, FileSystem } from "../wrapper.js";

// ----------------------------------------------------------------------------
// setEnv — live application
// ----------------------------------------------------------------------------

test("Bash: setEnv is visible to scripts", (t) => {
  const bash = new Bash();
  bash.setEnv("SKILL_PATH", "/skills/my-skill");
  t.is(bash.executeSync("echo $SKILL_PATH").stdout, "/skills/my-skill\n");
});

test("Bash: setEnv is exported", (t) => {
  const bash = new Bash();
  bash.setEnv("SKILL_PATH", "/skills/my-skill");
  t.is(
    bash.executeSync("env | grep '^SKILL_PATH='").stdout,
    "SKILL_PATH=/skills/my-skill\n",
  );
});

test("Bash: setEnv preserves existing shell state", (t) => {
  const bash = new Bash();
  bash.executeSync("export EXISTING=kept");
  bash.setEnv("ADDED", "new");
  t.is(bash.executeSync("echo $EXISTING $ADDED").stdout, "kept new\n");
});

test("Bash: setEnv overrides constructor env", (t) => {
  const bash = new Bash({ env: { SKILL_PATH: "/from-options" } });
  bash.setEnv("SKILL_PATH", "/from-setenv");
  t.is(bash.executeSync("echo $SKILL_PATH").stdout, "/from-setenv\n");
});

test("BashTool: setEnv is visible to scripts", (t) => {
  const tool = new BashTool();
  tool.setEnv("SKILL_PATH", "/skills/my-skill");
  t.is(tool.executeSync("echo $SKILL_PATH").stdout, "/skills/my-skill\n");
});

// ----------------------------------------------------------------------------
// reset() replay — env
// ----------------------------------------------------------------------------

test("Bash: reset preserves setEnv values", (t) => {
  const bash = new Bash();
  bash.setEnv("SKILL_PATH", "/skills/my-skill");
  bash.reset();
  t.is(bash.executeSync("echo $SKILL_PATH").stdout, "/skills/my-skill\n");
});

test("Bash: reset keeps last setEnv value for a key", (t) => {
  const bash = new Bash();
  bash.setEnv("SKILL_PATH", "/first");
  bash.setEnv("SKILL_PATH", "/second");
  bash.reset();
  t.is(bash.executeSync("echo $SKILL_PATH").stdout, "/second\n");
});

test("Bash: reset discards script-set env", (t) => {
  const bash = new Bash();
  bash.executeSync("export SCRIPT_ONLY=transient");
  bash.reset();
  t.is(bash.executeSync("echo [$SCRIPT_ONLY]").stdout, "[]\n");
});

test("Bash: reset keeps setEnv override of constructor env", (t) => {
  const bash = new Bash({ env: { SKILL_PATH: "/from-options" } });
  bash.setEnv("SKILL_PATH", "/from-setenv");
  bash.reset();
  t.is(bash.executeSync("echo $SKILL_PATH").stdout, "/from-setenv\n");
});

test("BashTool: reset preserves setEnv values", (t) => {
  const tool = new BashTool();
  tool.setEnv("SKILL_PATH", "/skills/my-skill");
  tool.reset();
  t.is(tool.executeSync("echo $SKILL_PATH").stdout, "/skills/my-skill\n");
});

// ----------------------------------------------------------------------------
// reset() replay — filesystem mounts
// ----------------------------------------------------------------------------

test("Bash: reset preserves a runtime FileSystem mount", (t) => {
  const data = new FileSystem();
  data.writeFile("/SKILL.md", "# my-skill\n");

  const bash = new Bash();
  bash.mount("/skills/my-skill", data);
  t.is(
    bash.executeSync("cat /skills/my-skill/SKILL.md").stdout,
    "# my-skill\n",
  );

  bash.reset();
  t.is(
    bash.executeSync("cat /skills/my-skill/SKILL.md").stdout,
    "# my-skill\n",
  );
});

test("Bash: unmount retracts the replay, so reset does not resurrect it", (t) => {
  const data = new FileSystem();
  data.writeFile("/SKILL.md", "# my-skill\n");

  const bash = new Bash();
  bash.mount("/skills/my-skill", data);
  bash.unmount("/skills/my-skill");
  bash.reset();

  t.not(bash.executeSync("cat /skills/my-skill/SKILL.md 2>&1").exitCode, 0);
});

test("Bash: reset preserves a runtime host directory mount", (t) => {
  const dir = mkdtempSync(path.join(tmpdir(), "bashkit-ext-"));
  try {
    writeFileSync(path.join(dir, "SKILL.md"), "# host skill\n");

    const bash = new Bash({ allowedMountPaths: [dir] });
    bash.mount(dir, "/skills/host-skill");
    t.is(
      bash.executeSync("cat /skills/host-skill/SKILL.md").stdout,
      "# host skill\n",
    );

    bash.reset();
    t.is(
      bash.executeSync("cat /skills/host-skill/SKILL.md").stdout,
      "# host skill\n",
    );
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("BashTool: reset preserves a runtime FileSystem mount", (t) => {
  const data = new FileSystem();
  data.writeFile("/SKILL.md", "# my-skill\n");

  const tool = new BashTool();
  tool.mount("/skills/my-skill", data);
  tool.reset();

  t.is(
    tool.executeSync("cat /skills/my-skill/SKILL.md").stdout,
    "# my-skill\n",
  );
});

// ----------------------------------------------------------------------------
// The whole extension shape, end to end
// ----------------------------------------------------------------------------

test("Bash: a mount+env+builtin bundle survives reset intact", async (t) => {
  const SKILL_PATH = "/skills/my-skill";
  const data = new FileSystem();
  data.writeFile("/SKILL.md", "# my-skill\n");

  const installSkill = (bash: Bash): void => {
    bash.mount(SKILL_PATH, data);
    bash.setEnv("SKILL_PATH", SKILL_PATH);
    bash.addBuiltin("my-skill", (ctx) =>
      ctx.fs.readFile(`${SKILL_PATH}/SKILL.md`),
    );
  };

  const bash = new Bash();
  installSkill(bash);
  bash.reset();

  // Custom builtins need the event loop, so this path is async.
  const result = await bash.execute('my-skill; cat "$SKILL_PATH/SKILL.md"');
  t.is(result.exitCode, 0);
  t.is(result.stdout, "# my-skill\n# my-skill\n");
});
