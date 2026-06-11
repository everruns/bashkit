import test from "ava";
import { Bash, BashTool } from "../wrapper.js";
import type { BuiltinContext } from "../wrapper.js";

// ----------------------------------------------------------------------------
// Constructor-time registration
// ----------------------------------------------------------------------------

test("Bash constructor with customBuiltins (sync)", async (t) => {
  const bash = new Bash({
    customBuiltins: {
      greet: (ctx) => `hello ${ctx.argv[0] ?? "world"}\n`,
    },
  });
  const result = await bash.execute("greet Alice");
  t.is(result.stdout, "hello Alice\n");
  t.is(result.exitCode, 0);
});

test("Bash constructor with customBuiltins (async)", async (t) => {
  const bash = new Bash({
    customBuiltins: {
      greet: async (ctx) => {
        await new Promise((r) => setTimeout(r, 5));
        return `hello ${ctx.argv[0] ?? "world"}\n`;
      },
    },
  });
  const result = await bash.execute("greet Bob");
  t.is(result.stdout, "hello Bob\n");
  t.is(result.exitCode, 0);
});

test("BashTool constructor with customBuiltins", async (t) => {
  const tool = new BashTool({
    customBuiltins: {
      greet: (ctx) => `hello ${ctx.argv[0] ?? "world"}\n`,
    },
  });
  const result = await tool.execute("greet Carol");
  t.is(result.stdout, "hello Carol\n");
  t.is(result.exitCode, 0);
});

// ----------------------------------------------------------------------------
// BuiltinContext fields
// ----------------------------------------------------------------------------

test("BuiltinContext exposes name, argv, stdin, env, cwd", async (t) => {
  const seen: BuiltinContext[] = [];
  const bash = new Bash({
    customBuiltins: {
      inspect: (ctx) => {
        seen.push(ctx);
        return "";
      },
    },
  });
  // Export a var so we have a known env entry to assert on; bashkit only
  // exposes *exported* names in ctx.env (shell variables stay in ctx.variables
  // on the Rust side, which we don't surface to JS).
  await bash.execute("export FOO=bar; echo piped | inspect a b c");

  t.is(seen.length, 1);
  const ctx = seen[0]!;
  t.is(ctx.name, "inspect");
  t.deepEqual(ctx.argv, ["a", "b", "c"]);
  t.is(ctx.stdin, "piped\n");
  t.is(ctx.env["FOO"], "bar");
  t.true(ctx.cwd.startsWith("/"));
});

test("stdin is null when no pipe", async (t) => {
  let observed: string | null | undefined;
  const bash = new Bash({
    customBuiltins: {
      inspect: (ctx) => {
        observed = ctx.stdin;
        return "";
      },
    },
  });
  await bash.execute("inspect");
  t.is(observed, null);
});

// ----------------------------------------------------------------------------
// VFS persistence
// ----------------------------------------------------------------------------

test("customBuiltins share the VFS across execute() calls", async (t) => {
  const bash = new Bash({
    customBuiltins: {
      "get-order": (ctx) =>
        JSON.stringify({ id: ctx.argv[0] ?? "?", status: "shipped" }) + "\n",
    },
  });
  await bash.execute("mkdir -p /scratch");
  await bash.execute("get-order 42 > /scratch/order.json");
  const result = await bash.execute("cat /scratch/order.json");
  t.is(result.exitCode, 0);
  t.deepEqual(JSON.parse(result.stdout.trim()), {
    id: "42",
    status: "shipped",
  });
});

// ----------------------------------------------------------------------------
// Post-construction registration — the previous design rebuilt the
// interpreter here and wiped the VFS. With BuiltinRegistry the VFS survives.
// ----------------------------------------------------------------------------

test("addBuiltin after execute() preserves the VFS", async (t) => {
  const bash = new Bash();
  await bash.execute("mkdir -p /scratch && echo seed > /scratch/seed.txt");

  bash.addBuiltin("double", (ctx) => `${Number(ctx.argv[0]) * 2}\n`);

  // New builtin works…
  const r1 = await bash.execute("double 21");
  t.is(r1.stdout, "42\n");

  // …and the pre-existing VFS contents are still there.
  const r2 = await bash.execute("cat /scratch/seed.txt");
  t.is(r2.stdout, "seed\n");
});

test("addBuiltin works on BashTool too", async (t) => {
  const tool = new BashTool();
  tool.addBuiltin("greet", (ctx) => `hi ${ctx.argv[0]}\n`);
  const result = await tool.execute("greet Dana");
  t.is(result.stdout, "hi Dana\n");
});

test("removeBuiltin makes the command unavailable", async (t) => {
  const bash = new Bash();
  bash.addBuiltin("tmp", () => "ok\n");
  t.is((await bash.execute("tmp")).stdout, "ok\n");

  bash.removeBuiltin("tmp");
  const r = await bash.execute("tmp");
  t.is(r.exitCode, 127);
});

// ----------------------------------------------------------------------------
// reset() preserves host-registered builtins
// ----------------------------------------------------------------------------

test("customBuiltins survive reset()", async (t) => {
  const bash = new Bash({
    customBuiltins: { ping: () => "pong\n" },
  });
  t.is((await bash.execute("ping")).stdout, "pong\n");

  bash.reset();

  t.is((await bash.execute("ping")).stdout, "pong\n");
});

test("addBuiltin entries survive reset()", async (t) => {
  const bash = new Bash();
  bash.addBuiltin("ping", () => "pong\n");
  bash.reset();
  t.is((await bash.execute("ping")).stdout, "pong\n");
});

// ----------------------------------------------------------------------------
// Pipes / chains
// ----------------------------------------------------------------------------

test("custom builtin participates in pipes", async (t) => {
  const bash = new Bash({
    customBuiltins: {
      double: (ctx) => `${Number((ctx.stdin ?? "").trim()) * 2}\n`,
    },
  });
  const result = await bash.execute("echo 5 | double");
  t.is(result.stdout, "10\n");
});

test("sync + async builtins chain via pipe", async (t) => {
  const bash = new Bash({
    customBuiltins: {
      prefix: (ctx) => `tag-${(ctx.stdin ?? "").trim()}\n`,
      suffix: async (ctx) => {
        await Promise.resolve();
        return `${(ctx.stdin ?? "").trim()}-done\n`;
      },
    },
  });
  const result = await bash.execute("echo test | prefix | suffix");
  t.is(result.stdout, "tag-test-done\n");
});

// ----------------------------------------------------------------------------
// Error handling
// ----------------------------------------------------------------------------

test("sync throw becomes stderr + exit 1", async (t) => {
  const bash = new Bash({
    customBuiltins: {
      fail: () => {
        throw new Error("nope");
      },
    },
  });
  const r = await bash.execute("fail");
  t.is(r.exitCode, 1);
  t.true(r.stderr.includes("nope"), `stderr: ${r.stderr}`);
});

test("async reject becomes stderr + exit 1", async (t) => {
  const bash = new Bash({
    customBuiltins: {
      fail: async () => {
        throw new Error("async nope");
      },
    },
  });
  const r = await bash.execute("fail");
  t.is(r.exitCode, 1);
  t.true(r.stderr.includes("async nope"), `stderr: ${r.stderr}`);
});

// ----------------------------------------------------------------------------
// Override semantics
// ----------------------------------------------------------------------------

test("custom builtin overrides baked-in builtin of the same name", async (t) => {
  const bash = new Bash({
    customBuiltins: {
      echo: (ctx) => `OVERRIDE:${ctx.argv.join(",")}\n`,
    },
  });
  const r = await bash.execute("echo a b c");
  t.is(r.stdout, "OVERRIDE:a,b,c\n");
});

test("shell function still wins over custom builtin", async (t) => {
  const bash = new Bash({
    customBuiltins: {
      thing: () => "from-builtin\n",
    },
  });
  const r = await bash.execute("thing() { echo from-function; }\nthing");
  t.is(r.stdout, "from-function\n");
});

// ----------------------------------------------------------------------------
// addBuiltin after constructor option
// ----------------------------------------------------------------------------

test("addBuiltin coexists with constructor customBuiltins", async (t) => {
  const bash = new Bash({
    customBuiltins: { first: () => "1\n" },
  });
  bash.addBuiltin("second", () => "2\n");

  t.is((await bash.execute("first")).stdout, "1\n");
  t.is((await bash.execute("second")).stdout, "2\n");
});

test("addBuiltin replaces an existing custom builtin", async (t) => {
  const bash = new Bash({
    customBuiltins: { tag: () => "v1\n" },
  });
  t.is((await bash.execute("tag")).stdout, "v1\n");

  bash.addBuiltin("tag", () => "v2\n");
  t.is((await bash.execute("tag")).stdout, "v2\n");
});

// ----------------------------------------------------------------------------
// executeSync + custom builtin: fail fast instead of deadlocking
// ----------------------------------------------------------------------------

test("executeSync with custom builtin returns error instead of deadlocking", (t) => {
  let invoked = false;
  const bash = new Bash({
    customBuiltins: {
      mybuiltin: () => {
        invoked = true;
        return "should-not-run\n";
      },
    },
  });
  const result = bash.executeSync("mybuiltin");
  t.is(result.exitCode, 1);
  t.false(invoked, "JS callback must not run when the loop is blocked");
  t.regex(
    result.stderr,
    /mybuiltin: custom builtins require execute\(\) \(async\)/,
  );
});

test("executeSync with addBuiltin custom builtin returns error", (t) => {
  let invoked = false;
  const bash = new Bash();
  bash.addBuiltin("dyn", () => {
    invoked = true;
    return "should-not-run\n";
  });
  const result = bash.executeSync("dyn");
  t.is(result.exitCode, 1);
  t.false(invoked);
  t.regex(result.stderr, /dyn: custom builtins require execute\(\) \(async\)/);
});

test("BashTool executeSync with custom builtin returns error", (t) => {
  let invoked = false;
  const tool = new BashTool({
    customBuiltins: {
      mybuiltin: () => {
        invoked = true;
        return "should-not-run\n";
      },
    },
  });
  const result = tool.executeSync("mybuiltin");
  t.is(result.exitCode, 1);
  t.false(invoked);
  t.regex(
    result.stderr,
    /mybuiltin: custom builtins require execute\(\) \(async\)/,
  );
});

test("async execute() still works after a guardrail-rejected executeSync", async (t) => {
  const bash = new Bash({
    customBuiltins: {
      mybuiltin: () => "via-async\n",
    },
  });
  const sync = bash.executeSync("mybuiltin");
  t.is(sync.exitCode, 1);

  const result = await bash.execute("mybuiltin");
  t.is(result.exitCode, 0);
  t.is(result.stdout, "via-async\n");
});

// ----------------------------------------------------------------------------
// ctx.fs — live VFS access inside custom builtins (parity with Python's
// BuiltinContext.fs, see PR #2010)
// ----------------------------------------------------------------------------

test("ctx.fs reads files written by the script", async (t) => {
  const bash = new Bash({
    customBuiltins: {
      "read-it": (ctx) => ctx.fs.readFile(ctx.argv[0]!),
    },
  });
  await bash.execute('echo "from bash" > /tmp/in.txt');
  const result = await bash.execute("read-it /tmp/in.txt");
  t.is(result.exitCode, 0);
  t.is(result.stdout, "from bash\n");
});

test("ctx.fs writes are visible to subsequent commands", async (t) => {
  const bash = new Bash({
    customBuiltins: {
      "write-it": (ctx) => {
        ctx.fs.writeFile("/tmp/out.txt", "from builtin\n");
        return "";
      },
    },
  });
  const result = await bash.execute("write-it && cat /tmp/out.txt");
  t.is(result.exitCode, 0);
  t.is(result.stdout, "from builtin\n");
});

test("ctx.fs writes are visible within the same pipeline read-back", async (t) => {
  // Live view: a write made by the builtin is observable by a later
  // ctx.fs read in another builtin invocation of the same execute() call.
  const bash = new Bash({
    customBuiltins: {
      put: (ctx) => {
        ctx.fs.writeFile("/tmp/live.txt", ctx.argv[0]! + "\n");
        return "";
      },
      get: (ctx) => ctx.fs.readFile("/tmp/live.txt"),
    },
  });
  const result = await bash.execute("put hello42 && get");
  t.is(result.exitCode, 0);
  t.is(result.stdout, "hello42\n");
});

test("ctx.fs supports mkdir/readDir/exists/stat", async (t) => {
  const seen: string[] = [];
  const bash = new Bash({
    customBuiltins: {
      probe: (ctx) => {
        ctx.fs.mkdir("/tmp/sub/dir", true);
        ctx.fs.writeFile("/tmp/sub/dir/a.txt", "x");
        seen.push(
          `exists=${ctx.fs.exists("/tmp/sub/dir/a.txt")}`,
          `entries=${ctx.fs
            .readDir("/tmp/sub/dir")
            .map((e) => e.name)
            .join(",")}`,
          `size=${ctx.fs.stat("/tmp/sub/dir/a.txt").size}`,
        );
        return "";
      },
    },
  });
  const result = await bash.execute("probe");
  t.is(result.exitCode, 0);
  t.deepEqual(seen, ["exists=true", "entries=a.txt", "size=1"]);
});

test("ctx.fs read of a missing file surfaces as exit 1 + stderr", async (t) => {
  const bash = new Bash({
    customBuiltins: {
      "read-missing": (ctx) => ctx.fs.readFile("/tmp/does-not-exist"),
    },
  });
  const result = await bash.execute("read-missing");
  t.is(result.exitCode, 1);
  t.regex(result.stderr, /does-not-exist|not found|No such/i);
});

test("ctx.fs works with addBuiltin registration", async (t) => {
  const bash = new Bash();
  bash.addBuiltin("read-it", (ctx) => ctx.fs.readFile("/tmp/added.txt"));
  await bash.execute('printf "added" > /tmp/added.txt');
  const result = await bash.execute("read-it");
  t.is(result.exitCode, 0);
  t.is(result.stdout, "added");
});

test("ctx.fs works on BashTool builtins", async (t) => {
  const tool = new BashTool({
    customBuiltins: {
      "read-it": (ctx) => ctx.fs.readFile("/tmp/tool.txt"),
    },
  });
  await tool.execute('printf "tool" > /tmp/tool.txt');
  const result = await tool.execute("read-it");
  t.is(result.exitCode, 0);
  t.is(result.stdout, "tool");
});

test("ctx.fs respects files mounted at construction", async (t) => {
  const bash = new Bash({
    files: { "/data/config.json": '{"ok":true}' },
    customBuiltins: {
      "read-config": (ctx) => ctx.fs.readFile("/data/config.json"),
    },
  });
  const result = await bash.execute("read-config");
  t.is(result.exitCode, 0);
  t.is(result.stdout, '{"ok":true}');
});
