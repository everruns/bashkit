// This suite runs on Node, Bun and Deno against the real native binding.
import test from "node:test";
import assert from "node:assert/strict";
import { AsyncLocalStorage } from "node:async_hooks";
import { Bash, BashTool } from "../../wrapper.js";

for (const Factory of [Bash, BashTool]) {
  for (const language of ["python", "typescript"]) {
    test(`${Factory.name}: ${language} host functions retain async request context`, async () => {
      const context = new AsyncLocalStorage();
      const handler = async () => {
        await new Promise((resolve) => setTimeout(resolve, 1));
        return context.getStore();
      };
      const bash = new Factory(
        language === "python"
          ? {
              python: true,
              externalFunctions: ["lookup"],
              externalHandler: handler,
            }
          : {
              typescript: true,
              typescriptExternalFunctions: ["lookup"],
              typescriptExternalHandler: handler,
            },
      );
      const command =
        language === "python"
          ? "python -c 'print(lookup())'"
          : "ts -c 'await lookup()'";
      const results = await Promise.all(
        ["first", "second"].map((id) =>
          context.run(id, () => bash.execute(command)),
        ),
      );
      assert.deepEqual(
        results.map((r) => r.stdout),
        ["first\n", "second\n"],
      );
      for (const capture of [undefined, 64]) {
        const chunks = [];
        const live = await context.run("live", () =>
          bash.execute(`${command} | cat`, {
            capture,
            stdout: async (bytes) => {
              await new Promise((resolve) => setTimeout(resolve, 1));
              assert.equal(context.getStore(), "live");
              chunks.push(new TextDecoder().decode(bytes));
            },
          }),
        );
        assert.equal(live.exitCode, 0, live.stderr);
        assert.equal(chunks.join(""), "live\n");
        assert.equal(live.stdout, capture === undefined ? "" : "live\n");
      }
      assert.notEqual(bash.executeSync(command).exitCode, 0);
      bash.reset();
      assert.equal(
        (await context.run("reset", () => bash.execute(command))).stdout,
        "reset\n",
      );
    });
  }
}
