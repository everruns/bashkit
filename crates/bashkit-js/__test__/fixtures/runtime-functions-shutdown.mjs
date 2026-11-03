// Run in a subprocess: native runtime panics must not disappear into AVA's
// worker stderr. Host functions defer invocation beyond the NAPI stack frame.
import assert from "node:assert/strict";
import {
  isMainThread,
  parentPort,
  Worker,
  workerData,
} from "node:worker_threads";
import { Bash, BashTool } from "../../wrapper.js";
import { ScriptedTool } from "../../scripted.js";

if (isMainThread) {
  for (const kind of ["bash", "tool"]) {
    for (const language of ["python", "typescript"]) {
      for (const style of ["pending", "busy"]) {
        const worker = new Worker(new URL(import.meta.url), {
          workerData: { kind, language, style },
        });
        await new Promise((resolve, reject) => {
          const timer = setTimeout(
            () =>
              reject(
                new Error(
                  `host handler did not start: ${kind}/${language}/${style}`,
                ),
              ),
            15000,
          );
          worker.once("message", () => {
            clearTimeout(timer);
            resolve();
          });
          worker.once("error", (error) => {
            clearTimeout(timer);
            reject(error);
          });
        });
        await worker.terminate();
      }
    }
  }
  // ScriptedTool has a synchronous callback ABI; verify its ordinary completed
  // worker teardown, without interrupting V8 inside that native callback frame.
  const worker = new Worker(new URL(import.meta.url), {
    workerData: { kind: "scripted" },
  });
  await new Promise((resolve, reject) => {
    let seen = false;
    worker.once("message", (stdout) => {
      assert.equal(stdout.trim(), "42");
      seen = true;
    });
    worker.once("error", reject);
    worker.once("exit", (code) =>
      code === 0 && seen
        ? resolve()
        : reject(new Error(`scripted worker exited ${code}`)),
    );
  });
  assert.equal(
    (await new Bash().execute("echo reusable")).stdout,
    "reusable\n",
  );
  console.log("pending worker shutdown passed");
} else if (workerData.kind === "scripted") {
  const tool = new ScriptedTool({ name: "shutdown" });
  tool.addTool("lookup", "Return a value", () => "42");
  void tool
    .execute("lookup")
    .then((result) => parentPort.postMessage(result.stdout));
} else {
  const handler = () => {
    parentPort.postMessage("started");
    if (workerData.style === "busy") {
      while (true) {
        /* host JS interrupted by Worker.terminate */
      }
    }
    return new Promise(() => {});
  };
  const Factory = workerData.kind === "bash" ? Bash : BashTool;
  const python = workerData.language === "python";
  const bash = new Factory(
    python
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
  void bash.execute(python ? "python -c 'lookup()'" : "ts -c 'await lookup()'");
}
