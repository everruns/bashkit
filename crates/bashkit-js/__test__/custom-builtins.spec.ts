import test from "ava";
import { Bash, BashTool } from "../wrapper.js";
import type { BuiltinContext } from "../wrapper.js";

test("Bash constructor with customBuiltins", async (t) => {
  const bash = new Bash({
    customBuiltins: {
      greet: (ctx: BuiltinContext) =>
        `hello ${ctx.argv[0] ?? "world"}\n`,
    },
  });
  const result = await bash.execute("greet Alice");
  t.is(result.stdout, "hello Alice\n");
  t.is(result.exitCode, 0);
});

test("Bash customBuiltins with argv and stdin", async (t) => {
  const bash = new Bash({
    customBuiltins: {
      echo_args: (ctx: BuiltinContext) =>
        `name=${ctx.name} argv=${ctx.argv.join(",")} stdin=${ctx.stdin ?? "null"}\n`,
    },
  });
  const result = await bash.execute("echo foo | echo_args bar baz");
  t.true(result.stdout.includes("stdin=foo\n"));
});

test("Bash customBuiltins — VFS persistence across calls", async (t) => {
  const bash = new Bash({
    customBuiltins: {
      "get-order": (ctx: BuiltinContext) =>
        JSON.stringify({ id: ctx.argv[0] ?? "?", status: "shipped" }) +
        "\n",
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

test("Bash customBuiltins survive reset()", async (t) => {
  const bash = new Bash({
    customBuiltins: {
      "test-cmd": () => "ok\n",
    },
  });
  const r1 = await bash.execute("test-cmd");
  t.is(r1.stdout, "ok\n");

  bash.reset();

  const r2 = await bash.execute("test-cmd");
  t.is(r2.stdout, "ok\n");
});

test("Bash addBuiltin after construction", async (t) => {
  const bash = new Bash();
  bash.addBuiltin("double", (ctx: BuiltinContext) => {
    const n = parseInt(ctx.argv[0] ?? "0", 10);
    return `${n * 2}\n`;
  });
  const result = await bash.execute("double 21");
  t.is(result.stdout, "42\n");
  t.is(result.exitCode, 0);
});

test("Bash customBuiltins with env", async (t) => {
  const bash = new Bash({
    customBuiltins: {
      show_env: (ctx: BuiltinContext) =>
        `HOME=${ctx.env["HOME"] ?? "?"}\n`,
    },
  });
  const result = await bash.execute("show_env");
  t.true(result.stdout.startsWith("HOME="));
});

test("Bash customBuiltins with cwd", async (t) => {
  const bash = new Bash({
    customBuiltins: {
      whereami: (ctx: BuiltinContext) => `${ctx.cwd}\n`,
    },
  });
  const result = await bash.execute("whereami");
  t.true(result.stdout.includes("/"), "cwd should be a path");
});

test("BashTool constructor with customBuiltins", async (t) => {
  const tool = new BashTool({
    customBuiltins: {
      greet: (ctx: BuiltinContext) =>
        `hello ${ctx.argv[0] ?? "world"}\n`,
    },
  });
  const result = await tool.execute("greet Alice");
  t.is(result.stdout, "hello Alice\n");
  t.is(result.exitCode, 0);
});

test("Bash customBuiltins — pipe between tools", async (t) => {
  const bash = new Bash({
    customBuiltins: {
      double: (ctx: BuiltinContext) => {
        const n = Math.trunc(Number(ctx.stdin));
        return `${n * 2}\n`;
      },
    },
  });
  const result = await bash.execute("echo 5 | double");
  t.is(result.stdout, "10\n");
  t.is(result.exitCode, 0);
});

test("Bash customBuiltins — multiple builtins", async (t) => {
  const bash = new Bash({
    customBuiltins: {
      mk: (ctx: BuiltinContext) =>
        JSON.stringify({ key: ctx.argv[0], value: ctx.argv[1] }) + "\n",
      get: (ctx: BuiltinContext) => {
        const parsed = JSON.parse(ctx.stdin ?? "{}") as Record<
          string,
          unknown
        >;
        return `value=${parsed["value"] ?? "?"}\n`;
      },
    },
  });
  const result = await bash.execute("mk one two | get");
  t.is(result.stdout, "value=two\n");
  t.is(result.exitCode, 0);
});
