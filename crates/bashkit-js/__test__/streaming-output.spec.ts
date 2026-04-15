import { AsyncLocalStorage } from "node:async_hooks";
import test, { type ExecutionContext } from "ava";
import { Bash, BashTool } from "../wrapper.js";

// Decision: queue a second execute() behind a same-instance sleep so abort lands
// after JS installs AbortSignal listeners but before Rust begins the next exec.
const SCRIPT = `
for i in 1 2 3; do
  echo "out-$i"
  echo "err-$i" >&2
done
`;

function assertChunksMatchResult(
  t: ExecutionContext,
  result: { stdout: string; stderr: string },
  chunks: Array<[string, string]>,
) {
  t.true(chunks.length > 0);
  t.is(chunks.map(([stdoutChunk]) => stdoutChunk).join(""), result.stdout);
  t.is(chunks.map(([, stderrChunk]) => stderrChunk).join(""), result.stderr);
}

for (const [label, create] of [
  ["Bash", () => new Bash()],
  ["BashTool", () => new BashTool()],
] as const) {
  test(`${label}: executeSync rejects Promise-returning onOutput`, (t) => {
    const shell = create();
    const error = t.throws(() =>
      shell.executeSync(SCRIPT, {
        onOutput() {
          return Promise.reject(new Error("async exploded"));
        },
      }),
    );

    t.truthy(error);
    t.regex(
      error.message,
      /onOutput must be synchronous and must not return a Promise/,
    );
  });

  test(`${label}: executeSync onOutput matches final result`, (t) => {
    const shell = create();
    const chunks: Array<[string, string]> = [];

    const result = shell.executeSync(SCRIPT, {
      onOutput({ stdout: stdoutChunk, stderr: stderrChunk }) {
        chunks.push([stdoutChunk, stderrChunk]);
      },
    });

    t.is(result.exitCode, 0);
    assertChunksMatchResult(t, result, chunks);
  });

  test(`${label}: execute onOutput matches final result`, async (t) => {
    const shell = create();
    const chunks: Array<[string, string]> = [];

    const result = await shell.execute(SCRIPT, {
      onOutput({ stdout: stdoutChunk, stderr: stderrChunk }) {
        chunks.push([stdoutChunk, stderrChunk]);
      },
    });

    t.is(result.exitCode, 0);
    assertChunksMatchResult(t, result, chunks);
  });

  test(`${label}: queued async execute honors AbortSignal before start`, async (t) => {
    const shell = create();
    const blocker = shell.execute("sleep 0.05");

    await new Promise((resolve) => setTimeout(resolve, 10));

    const controller = new AbortController();
    const pending = shell.execute("echo should-not-run", {
      signal: controller.signal,
    });
    controller.abort();

    t.is((await blocker).exitCode, 0);

    const result = await pending;
    t.is(result.exitCode, 1);
    t.is(result.error, "execution cancelled");
    t.is(result.stdout, "");
  });

  test(`${label}: queued async execute with onOutput honors AbortSignal before start`, async (t) => {
    const shell = create();
    const blocker = shell.execute("sleep 0.05");

    await new Promise((resolve) => setTimeout(resolve, 10));

    const controller = new AbortController();
    const chunks: Array<[string, string]> = [];
    const pending = shell.execute("echo should-not-run", {
      signal: controller.signal,
      onOutput({ stdout, stderr }) {
        chunks.push([stdout, stderr]);
      },
    });
    controller.abort();

    t.is((await blocker).exitCode, 0);

    const result = await pending;
    t.is(result.exitCode, 1);
    t.is(result.error, "execution cancelled");
    t.deepEqual(chunks, []);
    t.is(result.stdout, "");
  });

  test(`${label}: execute rejects Promise-returning onOutput`, async (t) => {
    const shell = create();
    const error = await t.throwsAsync(() =>
      shell.execute(SCRIPT, {
        onOutput() {
          return Promise.reject(new Error("async exploded"));
        },
      }),
    );

    t.truthy(error);
    t.regex(
      error.message,
      /onOutput must be synchronous and must not return a Promise/,
    );
  });

  test(`${label}: executeSync onOutput error propagates`, (t) => {
    const shell = create();
    const error = t.throws(() =>
      shell.executeSync(SCRIPT, {
        onOutput() {
          throw new Error("onOutput exploded");
        },
      }),
    );

    t.truthy(error);
    t.regex(error.message, /onOutput exploded/);
  });

  test(`${label}: executeSync onOutput error does not poison future calls`, (t) => {
    const shell = create();

    const error = t.throws(() =>
      shell.executeSync(SCRIPT, {
        onOutput() {
          throw new Error("onOutput exploded");
        },
      }),
    );

    t.truthy(error);
    t.regex(error.message, /onOutput exploded/);

    const result = shell.executeSync("echo after-error");
    t.is(result.exitCode, 0);
    t.is(result.stdout, "after-error\n");
  });

  test(`${label}: executeSync onOutput error does not clear future explicit cancel`, (t) => {
    const shell = create();

    const error = t.throws(() =>
      shell.executeSync(SCRIPT, {
        onOutput() {
          throw new Error("onOutput exploded");
        },
      }),
    );

    t.truthy(error);
    t.regex(error.message, /onOutput exploded/);

    shell.cancel();
    const result = shell.executeSync("echo after-error");
    t.is(result.exitCode, 1);
    t.is(result.error, "execution cancelled");
    t.is(result.stdout, "");
  });

  test(`${label}: execute onOutput error propagates`, async (t) => {
    const shell = create();
    const error = await t.throwsAsync(() =>
      shell.execute(SCRIPT, {
        onOutput() {
          throw new Error("onOutput exploded");
        },
      }),
    );

    t.truthy(error);
    t.regex(error.message, /onOutput exploded/);
  });

  test(`${label}: execute rejects async onOutput`, async (t) => {
    const shell = create();
    const error = await t.throwsAsync(() =>
      shell.execute(SCRIPT, {
        async onOutput() {
          throw new Error("async exploded");
        },
      }),
    );

    t.truthy(error);
    t.regex(
      error.message,
      /onOutput must be synchronous and must not return a Promise/,
    );
  });

  test(`${label}: async onOutput preserves AsyncLocalStorage context`, async (t) => {
    const shell = create();
    const requestContext = new AsyncLocalStorage<string>();
    const seenStores = new Set<string | undefined>();

    const result = await requestContext.run(`${label}-request`, async () =>
      shell.execute(SCRIPT, {
        onOutput() {
          seenStores.add(requestContext.getStore());
        },
      }),
    );

    t.is(result.exitCode, 0);
    t.deepEqual([...seenStores], [`${label}-request`]);
  });

  test(`${label}: execute onOutput error does not poison future calls`, async (t) => {
    const shell = create();

    const error = await t.throwsAsync(() =>
      shell.execute(SCRIPT, {
        onOutput() {
          throw new Error("onOutput exploded");
        },
      }),
    );

    t.truthy(error);
    t.regex(error.message, /onOutput exploded/);

    const result = await shell.execute("echo after-error");
    t.is(result.exitCode, 0);
    t.is(result.stdout, "after-error\n");
  });

  test(`${label}: execute onOutput error does not clear future explicit cancel`, async (t) => {
    const shell = create();

    const error = await t.throwsAsync(() =>
      shell.execute(SCRIPT, {
        onOutput() {
          throw new Error("onOutput exploded");
        },
      }),
    );

    t.truthy(error);
    t.regex(error.message, /onOutput exploded/);

    shell.cancel();
    const result = await shell.execute("echo after-error");
    t.is(result.exitCode, 1);
    t.is(result.error, "execution cancelled");
    t.is(result.stdout, "");
  });
}
