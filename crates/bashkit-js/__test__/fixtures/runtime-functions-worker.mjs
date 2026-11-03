// A native callback must belong to this worker's environment, even when the
// process-wide Tokio runtime is shared with other workers and they exit first.
import assert from "node:assert/strict";
import { workerData } from "node:worker_threads";
import { AsyncLocalStorage } from "node:async_hooks";
import { BashTool } from "../../wrapper.js";

const context = new AsyncLocalStorage();
const python = workerData === "python";
const handler = async () => {
  await new Promise((resolve) => setTimeout(resolve, 1));
  assert.equal(context.getStore(), "worker");
  return 42;
};
const bash = new BashTool(
  python
    ? { python: true, externalFunctions: ["lookup"], externalHandler: handler }
    : {
        typescript: true,
        typescriptExternalFunctions: ["lookup"],
        typescriptExternalHandler: handler,
      },
);
for (let i = 0; i < 10; i++) {
  const result = await context.run("worker", () =>
    bash.execute(
      python ? "python -c 'print(lookup())'" : "ts -c 'await lookup()'",
    ),
  );
  assert.equal(result.stdout, "42\n");
  assert.equal(result.exitCode, 0);
}
