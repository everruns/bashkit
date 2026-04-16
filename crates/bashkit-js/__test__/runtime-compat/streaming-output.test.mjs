import { describe, it } from "node:test";
import assert from "node:assert/strict";
import { Bash, BashTool } from "./_setup.mjs";

// Decision: queue a second execute() behind a same-instance sleep so abort lands
// after JS installs AbortSignal listeners but before Rust begins the next exec.
const SCRIPT = `
for i in 1 2 3; do
  echo "out-$i"
  echo "err-$i" >&2
done
`;

function assertChunksMatchResult(result, chunks) {
  assert.ok(chunks.length > 0);
  assert.equal(
    chunks.map(([stdoutChunk]) => stdoutChunk).join(""),
    result.stdout,
  );
  assert.equal(
    chunks.map(([, stderrChunk]) => stderrChunk).join(""),
    result.stderr,
  );
}

for (const [label, create] of [
  ["Bash", () => new Bash()],
  ["BashTool", () => new BashTool()],
]) {
  describe(`${label} streaming output`, () => {
    it("sync rejects Promise-returning onOutput", () => {
      const shell = create();

      assert.throws(
        () =>
          shell.executeSync(SCRIPT, {
            onOutput() {
              return Promise.reject(new Error("async exploded"));
            },
          }),
        /onOutput must be synchronous and must not return a Promise/,
      );
    });

    it("sync chunks reassemble to final result", () => {
      const shell = create();
      const chunks = [];

      const result = shell.executeSync(SCRIPT, {
        onOutput({ stdout: stdoutChunk, stderr: stderrChunk }) {
          chunks.push([stdoutChunk, stderrChunk]);
        },
      });

      assert.equal(result.exitCode, 0);
      assertChunksMatchResult(result, chunks);
    });

    it("async chunks reassemble to final result", async () => {
      const shell = create();
      const chunks = [];

      const result = await shell.execute(SCRIPT, {
        onOutput({ stdout: stdoutChunk, stderr: stderrChunk }) {
          chunks.push([stdoutChunk, stderrChunk]);
        },
      });

      assert.equal(result.exitCode, 0);
      assertChunksMatchResult(result, chunks);
    });

    it("queued async execute honors AbortSignal before start", async () => {
      const shell = create();
      const blocker = shell.execute("sleep 0.05");

      await new Promise((resolve) => setTimeout(resolve, 10));

      const controller = new AbortController();
      const pending = shell.execute("echo should-not-run", {
        signal: controller.signal,
      });
      controller.abort();

      assert.equal((await blocker).exitCode, 0);

      const result = await pending;
      assert.equal(result.exitCode, 1);
      assert.equal(result.error, "execution cancelled");
      assert.equal(result.stdout, "");
    });

    it("queued async execute with onOutput honors AbortSignal before start", async () => {
      const shell = create();
      const blocker = shell.execute("sleep 0.05");

      await new Promise((resolve) => setTimeout(resolve, 10));

      const controller = new AbortController();
      const chunks = [];
      const pending = shell.execute("echo should-not-run", {
        signal: controller.signal,
        onOutput({ stdout, stderr }) {
          chunks.push([stdout, stderr]);
        },
      });
      controller.abort();

      assert.equal((await blocker).exitCode, 0);

      const result = await pending;
      assert.equal(result.exitCode, 1);
      assert.equal(result.error, "execution cancelled");
      assert.deepEqual(chunks, []);
      assert.equal(result.stdout, "");
    });

    it("async rejects Promise-returning onOutput", async () => {
      const shell = create();

      await assert.rejects(
        () =>
          shell.execute(SCRIPT, {
            onOutput() {
              return Promise.reject(new Error("async exploded"));
            },
          }),
        /onOutput must be synchronous and must not return a Promise/,
      );
    });

    it("sync callback errors abort without poisoning later calls", () => {
      const shell = create();

      assert.throws(
        () =>
          shell.executeSync(SCRIPT, {
            onOutput() {
              throw new Error("onOutput exploded");
            },
          }),
        /onOutput exploded/,
      );

      const result = shell.executeSync("echo after-error");
      assert.equal(result.exitCode, 0);
      assert.equal(result.stdout, "after-error\n");
    });

    it("sync callback errors do not clear future explicit cancel", () => {
      const shell = create();

      assert.throws(
        () =>
          shell.executeSync(SCRIPT, {
            onOutput() {
              throw new Error("onOutput exploded");
            },
          }),
        /onOutput exploded/,
      );

      shell.cancel();
      const result = shell.executeSync("echo after-error");
      assert.equal(result.exitCode, 1);
      assert.equal(result.error, "execution cancelled");
      assert.equal(result.stdout, "");
    });

    it("async callback errors abort without poisoning later calls", async () => {
      const shell = create();

      await assert.rejects(
        () =>
          shell.execute(SCRIPT, {
            onOutput() {
              throw new Error("onOutput exploded");
            },
          }),
        /onOutput exploded/,
      );

      const result = await shell.execute("echo after-error");
      assert.equal(result.exitCode, 0);
      assert.equal(result.stdout, "after-error\n");
    });

    it("async callback errors do not clear future explicit cancel", async () => {
      const shell = create();

      await assert.rejects(
        () =>
          shell.execute(SCRIPT, {
            onOutput() {
              throw new Error("onOutput exploded");
            },
          }),
        /onOutput exploded/,
      );

      shell.cancel();
      const result = await shell.execute("echo after-error");
      assert.equal(result.exitCode, 1);
      assert.equal(result.error, "execution cancelled");
      assert.equal(result.stdout, "");
    });

    it("async rejects async onOutput", async () => {
      const shell = create();

      await assert.rejects(
        () =>
          shell.execute(SCRIPT, {
            async onOutput() {
              throw new Error("async exploded");
            },
          }),
        /onOutput must be synchronous and must not return a Promise/,
      );
    });
  });
}
