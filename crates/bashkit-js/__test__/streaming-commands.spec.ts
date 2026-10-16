// Exercise the public wrapper against the real native addon.
import test from "ava";
import { Bash, BashTool, type CommandContext } from "../wrapper.js";
import { native } from "../internal.js";

test("native command I/O rejects forged and mismatched handles", (t) => {
  const fs = native.__createFileSystem();
  for (const handle of [undefined, null, {}, fs]) {
    t.throws(() => native.__commandReadChunk(handle), {
      message: /external|External/,
    });
    t.throws(() => native.__commandWrite(handle, Uint8Array.of(1)), {
      message: /external|External/,
    });
  }
});

for (const [label, create] of [
  ["Bash", () => new Bash()],
  ["BashTool", () => new BashTool()],
] as const) {
  test(`${label}: final filter delivers before the host producer completes`, async (t) => {
    const shell = create();
    let release!: () => void;
    const gate = new Promise<void>((resolve) => {
      release = resolve;
    });
    shell.addCommand("produce", async (ctx) => {
      await ctx.stdout.write(new TextEncoder().encode("error: first\n"));
      await gate;
      await ctx.stdout.write(new TextEncoder().encode("error: last\n"));
      return 0;
    });
    shell.addCommand("copy", async (ctx) => {
      for await (const chunk of ctx.stdin) await ctx.stdout.write(chunk);
      return { exitCode: 0 };
    });
    const chunks: Uint8Array[] = [];
    const result = await shell.execute("produce | copy | grep error", {
      capture: 64,
      async stdout(bytes) {
        chunks.push(bytes);
        release();
        await new Promise((resolve) => setTimeout(resolve, 1));
      },
    });
    t.is(result.exitCode, 0);
    t.is(result.stdout, "error: first\nerror: last\n");
    t.is(Buffer.concat(chunks).toString(), result.stdout);
  });

  test(`${label}: binary capture and revoked handles`, async (t) => {
    const shell = create();
    let saved!: CommandContext;
    shell.addCommand("binary", async (ctx) => {
      saved = ctx;
      await ctx.stdout.write(Uint8Array.of(0, 255, 97, 98));
      return 7;
    });
    const result = await shell.execute("binary", { capture: 3 });
    t.deepEqual([...result.stdoutBytes], [0, 255, 97]);
    t.true(result.stdoutTruncated);
    t.is(result.exitCode, 7);
    await t.throwsAsync(() => saved.stdout.write(Uint8Array.of(1)), {
      message: /request closed/,
    });
    await t.throwsAsync(() => saved.stdin.readChunk(), {
      message: /request closed/,
    });
  });

  test(`${label}: writes snapshot mutable JavaScript byte arrays`, async (t) => {
    const shell = create();
    shell.addCommand("snapshot", async (ctx) => {
      const bytes = Uint8Array.of(65);
      const pending = ctx.stdout.write(bytes);
      bytes[0] = 66;
      await pending;
      return 0;
    });
    t.is((await shell.execute("snapshot", { capture: 8 })).stdout, "A");
  });

  test(`${label}: sink failure releases execution and sanitizes diagnostics`, async (t) => {
    const shell = create();
    const result = await shell.execute("echo hello", {
      stdout() {
        throw new Error("failed at /host/private.ts:2");
      },
    });
    t.is(result.exitCode, 1);
    t.truthy(result.error);
    t.false(result.stderr.includes("/host/"));
    t.is(
      (await shell.execute("echo recovered", { capture: 64 })).stdout,
      "recovered\n",
    );
  });

  test(`${label}: callbacks cannot re-enter their own session`, async (t) => {
    const shell = create();
    const result = await shell.execute("echo hello", {
      stdout: async () => {
        await t.throwsAsync(() => shell.execute("echo nested"), {
          message: /cannot re-enter/,
        });
      },
    });
    t.is(result.exitCode, 0);
    t.is(result.stdout, "");
  });

  test(`${label}: abort interrupts a pending sink and the session recovers`, async (t) => {
    const shell = create();
    const controller = new AbortController();
    const result = await shell.execute("echo hello", {
      signal: controller.signal,
      stdout: async () => {
        controller.abort();
        await new Promise<void>(() => {});
      },
    });
    t.is(result.exitCode, 1);
    t.is(result.error, "execution cancelled");
    t.is(
      (await shell.execute("echo recovered", { capture: 64 })).stdout,
      "recovered\n",
    );
  });
}
