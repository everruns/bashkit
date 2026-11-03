// Node, Bun and Deno CI exercise the shipped NAPI wrapper.
import { it } from "node:test";
import assert from "node:assert/strict";
import { Bash, BashTool } from "./_setup.mjs";
import { native } from "../../internal.js";

it("native command I/O rejects forged and mismatched handles", () => {
  const fs = native.__createFileSystem();
  for (const handle of [undefined, null, {}, fs]) {
    assert.throws(() => native.__commandReadChunk(handle), /external|External/);
    assert.throws(
      () => native.__commandWrite(handle, Uint8Array.of(1)),
      /external|External/,
    );
  }
});

for (const [name, create] of [
  ["Bash", () => new Bash()],
  ["BashTool", () => new BashTool()],
]) {
  it(
    `${name}: awaited byte commands deliver through a final filter`,
    { timeout: 5000 },
    async () => {
      const shell = create();
      let release;
      const gate = new Promise((resolve) => {
        release = resolve;
      });
      shell.addCommand("produce", async (ctx) => {
        await ctx.stdout.write(new TextEncoder().encode("error: first\n"));
        await gate;
        await ctx.stdout.write(new TextEncoder().encode("error: last\n"));
        return 0;
      });
      shell.addCommand("copy", async (ctx) => {
        for await (const bytes of ctx.stdin) await ctx.stdout.write(bytes);
        return { exitCode: 0 };
      });
      const chunks = [];
      const result = await shell.execute("produce | copy | grep error", {
        capture: 64,
        async stdout(bytes) {
          chunks.push(...bytes);
          release();
          await new Promise((resolve) => setTimeout(resolve, 1));
        },
      });
      assert.equal(result.stdout, "error: first\nerror: last\n");
      assert.equal(
        new TextDecoder().decode(new Uint8Array(chunks)),
        result.stdout,
      );
      assert.equal(result.stdoutTruncated, false);
    },
  );
  it(
    `${name}: binary capture, failure, cancellation and reuse`,
    { timeout: 5000 },
    async () => {
      const shell = create();
      let saved;
      shell.addCommand("binary", async (ctx) => {
        saved = ctx;
        await ctx.stdout.write(Uint8Array.of(0, 255, 97, 98));
        return 7;
      });
      const binary = await shell.execute("binary", { capture: 3 });
      assert.deepEqual([...binary.stdoutBytes], [0, 255, 97]);
      assert.equal(binary.stdoutTruncated, true);
      await assert.rejects(
        saved.stdout.write(Uint8Array.of(1)),
        /request closed/,
      );
      const failed = await shell.execute("echo hello", {
        stdout() {
          throw new Error("sink failed");
        },
      });
      assert.notEqual(failed.exitCode, 0);
      const controller = new AbortController();
      const aborted = await shell.execute("echo hello", {
        signal: controller.signal,
        async stdout() {
          controller.abort();
          await new Promise(() => {});
        },
      });
      assert.equal(aborted.error, "execution cancelled");
      assert.equal(
        (await shell.execute("echo recovered", { capture: 64 })).stdout,
        "recovered\n",
      );
    },
  );
}
