// Exercise real interpreters and caller contexts; option-forwarding mocks miss
// deadlocks, request-context leakage and callback cancellation.
import test from "ava";
import { AsyncLocalStorage } from "node:async_hooks";
import { Worker } from "node:worker_threads";
import { createServer } from "node:http";
import { execFile } from "node:child_process";
import { promisify } from "node:util";
import { fileURLToPath } from "node:url";
import { Bash, BashTool } from "../wrapper.js";
import { createBashTool } from "../langchain.js";
import { bashTool as aiBashTool } from "../ai.js";
import { bashTool as anthropicBashTool } from "../anthropic.js";
import { bashTool as openaiBashTool } from "../openai.js";

const request = new AsyncLocalStorage<string>();
const key = new Uint8Array(32).fill(7);
const delay = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

for (const language of ["python", "typescript"] as const) {
  const command =
    language === "python"
      ? `python -c 'print(lookup(21, multiplier=2)["value"])'`
      : `ts -c 'const r = await lookup(21, {multiplier: 2}); r.value'`;
  const call =
    language === "python"
      ? "python -c 'print(lookup())'"
      : "ts -c 'await lookup()'";
  const options = (handler: (...args: any[]) => any): any =>
    language === "python"
      ? {
          python: true,
          externalFunctions: ["lookup"],
          externalHandler: handler,
        }
      : {
          typescript: true,
          typescriptExternalFunctions: ["lookup"],
          typescriptExternalHandler: (name: string, args: any[], ctx: any) =>
            handler(name, args.slice(0, 1), args[1] ?? {}, ctx),
        };

  for (const Factory of [Bash, BashTool]) {
    test(`${Factory.name}: ${language} host functions`, async (t) => {
      const seen: unknown[] = [];
      const bash = new Factory(
        options(async (name, args, kwargs) => {
          seen.push([name, args, kwargs]);
          await delay(1);
          return { value: args[0] * kwargs.multiplier };
        }),
      );
      const result = await bash.execute(command);
      t.is(result.exitCode, 0, result.stderr);
      t.is(result.stdout, "42\n");
      t.deepEqual(seen, [["lookup", [21], { multiplier: 2 }]]);
    });

    test(`${Factory.name}: ${language} callbacks retain queued request contexts`, async (t) => {
      const bash = new Factory(
        options(async () => {
          const before = request.getStore();
          await delay(2);
          t.is(request.getStore(), before);
          return before;
        }),
      );
      const results = await Promise.all(
        Array.from({ length: 8 }, (_, i) =>
          request.run(String(i), () => bash.execute(call)),
        ),
      );
      t.deepEqual(
        results.map((r) => r.stdout.trim()),
        Array.from({ length: 8 }, (_, i) => String(i)),
      );
    });

    test(`${Factory.name}: ${language} reset and snapshots retain supplied handlers`, async (t) => {
      const config = options(() => ({ value: 42 }));
      const bash = new Factory(config);
      bash.writeFile("/tmp/persisted", "data");
      const snapshot = bash.snapshot({ hmacKey: key });
      const restored =
        Factory === Bash
          ? Bash.fromSnapshot(snapshot, { hmacKey: key }, config)
          : BashTool.fromSnapshot(snapshot, config, { hmacKey: key });
      t.is(restored.readFile("/tmp/persisted"), "data");
      for (let i = 0; i < 2; i++) {
        t.is((await restored.execute(command)).stdout, "42\n");
        restored.reset();
      }
    });

    test(`${Factory.name}: ${language} timeout aborts host work and permits reuse`, async (t) => {
      let aborted = false;
      const bash = new Factory({
        ...options(async (_name, _args, _kwargs, ctx) => {
          await new Promise((_resolve, reject) =>
            ctx.signal.addEventListener(
              "abort",
              () => {
                aborted = true;
                reject(new Error("aborted"));
              },
              { once: true },
            ),
          );
        }),
        timeoutMs: 2_000,
      });
      t.not((await bash.execute(call)).exitCode, 0);
      await delay(0);
      t.true(aborted);
      t.is((await bash.execute("echo reusable")).stdout, "reusable\n");
    });

    test(`${Factory.name}: ${language} cancellation is scoped to the active request`, async (t) => {
      let started!: () => void;
      const ready = new Promise<void>((resolve) => {
        started = resolve;
      });
      let signal: AbortSignal | undefined;
      const bash = new Factory(
        options(async (_name, _args, _kwargs, ctx) => {
          signal = ctx.signal;
          started();
          await new Promise((_resolve, reject) =>
            ctx.signal.addEventListener(
              "abort",
              () => reject(new Error("cancelled")),
              { once: true },
            ),
          );
        }),
      );
      const controller = new AbortController();
      const active = bash.execute(call, { signal: controller.signal });
      await ready;
      const queued = bash.execute("echo next");
      controller.abort();
      t.not((await active).exitCode, 0);
      t.true(signal!.aborted);
      t.is((await queued).stdout, "next\n");
    });

    test(`${Factory.name}: ${language} recursive access fails without deadlocking`, async (t) => {
      let bash: Bash | BashTool;
      bash = new Factory(
        options(async () => {
          await bash.execute("echo recursive");
        }),
      );
      const result = await bash.execute(call);
      t.not(result.exitCode, 0);
      t.regex(result.stderr + (result.error ?? ""), /re-enter/);
      t.is((await bash.execute("echo reusable")).stdout, "reusable\n");
    });

    test(`${Factory.name}: ${language} retained live filesystem access fails promptly`, async (t) => {
      let fs: ReturnType<Bash["fs"]>;
      const bash = new Factory(options(() => fs.readFile("/tmp/state")));
      bash.writeFile("/tmp/state", "data");
      fs = bash.fs();
      const result = await bash.execute(call);
      t.not(result.exitCode, 0);
      t.regex(result.stderr + (result.error ?? ""), /re-enter|would block/);
      t.is(fs.readFile("/tmp/state"), "data");
    });

    test(`${Factory.name}: ${language} sync access before a host call cannot deadlock`, async (t) => {
      let started!: () => void, release!: () => void;
      const ready = new Promise<void>((resolve) => {
        started = resolve;
      });
      const gate = new Promise<void>((resolve) => {
        release = resolve;
      });
      const bash = new Factory({
        ...options(() => 42),
        customBuiltins: {
          wait_host: async () => {
            started();
            await gate;
            return "";
          },
        },
      });
      bash.writeFile("/tmp/state", "ready");
      const executing = bash.execute(`wait_host; ${call}`, {
        onOutput: () => {},
      });
      await ready;
      // Hold the shell before its runtime call. A blocking read would wait on
      // the interpreter while the JS event loop is needed to release this gate.
      t.throws(() => bash.readFile("/tmp/state"), { message: /would block/ });
      release();
      const result = await executing;
      t.is(result.exitCode, 0, result.stderr);
      t.is(result.stdout, "42\n");
    });

    test(`${Factory.name}: ${language} sync invocation rejects host calls`, (t) => {
      let called = false;
      const bash = new Factory(
        options(() => {
          called = true;
          return 42;
        }),
      );
      const result = bash.executeSync(call);
      t.not(result.exitCode, 0);
      t.regex(result.stderr + (result.error ?? ""), /execute\(\).*async/);
      t.false(called);
      t.is(bash.executeSync("echo normal").stdout, "normal\n");
    });
  }

  for (const Factory of [Bash, BashTool]) {
    test(`${Factory.name}: ${language} unrelated sync calls fail while async calls queue`, async (t) => {
      let started!: () => void, release!: () => void;
      const ready = new Promise<void>((resolve) => {
        started = resolve;
      });
      const gate = new Promise<void>((resolve) => {
        release = resolve;
      });
      const bash = new Factory(
        options(async () => {
          started();
          await gate;
          return 42;
        }),
      );
      bash.writeFile("/tmp/state", "data");
      const fs = bash.fs();
      const active = bash.execute(call);
      await ready;
      for (const operation of [
        () => bash.executeSync("echo unsafe"),
        () => bash.readFile("/tmp/state"),
        () => fs.readFile("/tmp/state"),
        () => bash.snapshot({ hmacKey: key }),
        () => bash.reset(),
      ]) {
        t.throws(operation, { message: /would block/ });
      }
      const queued = bash.execute("echo queued");
      release();
      t.is((await active).stdout, "42\n");
      t.is((await queued).stdout, "queued\n");
    });

    test(`${Factory.name}: ${language} queued cancellation preserves the active callback`, async (t) => {
      let started!: () => void, release!: () => void;
      let activeSignal!: AbortSignal;
      const ready = new Promise<void>((resolve) => {
        started = resolve;
      });
      const gate = new Promise<void>((resolve) => {
        release = resolve;
      });
      const bash = new Factory(
        options(async (_name, _args, _kwargs, ctx) => {
          activeSignal = ctx.signal;
          started();
          await gate;
          return 42;
        }),
      );
      const active = bash.execute(call);
      await ready;
      const controller = new AbortController();
      const queued = bash.execute("echo forbidden > /tmp/cancelled", {
        signal: controller.signal,
      });
      controller.abort();
      t.false(activeSignal.aborted);
      release();
      t.is((await active).stdout, "42\n");
      t.not((await queued).exitCode, 0);
      t.false(bash.exists("/tmp/cancelled"));
      const restored = Factory.fromSnapshotKeyed(
        bash.snapshotKeyed(key),
        key,
        options(() => 21),
      );
      t.is((await restored.execute(call)).stdout, "21\n");
    });
  }

  test(`${language} host functions preserve HTTP request contexts and async I/O`, async (t) => {
    const upstream = createServer((req, res) => {
      res.setHeader("Content-Type", "text/plain; charset=utf-8");
      res.end(req.url!.slice(1));
    });
    await new Promise<void>((resolve) =>
      upstream.listen(0, "127.0.0.1", resolve),
    );
    const upstreamPort = (upstream.address() as { port: number }).port;
    const bash = new BashTool(
      options(async (_name, _args, _kwargs, ctx) => {
        const id = request.getStore();
        const response = await fetch(`http://127.0.0.1:${upstreamPort}/${id}`, {
          signal: ctx.signal,
        });
        t.is(request.getStore(), id);
        return response.text();
      }),
    );
    const server = createServer((req, res) => {
      res.setHeader("Content-Type", "text/plain; charset=utf-8");
      void request.run(req.url!.slice(1), async () => {
        try {
          res.end((await bash.execute(call)).stdout.trim());
        } catch (error) {
          res.writeHead(500).end(String(error));
        }
      });
    });
    await new Promise<void>((resolve) =>
      server.listen(0, "127.0.0.1", resolve),
    );
    const port = (server.address() as { port: number }).port;
    try {
      const responses = await Promise.all(
        Array.from({ length: 8 }, (_, i) =>
          fetch(`http://127.0.0.1:${port}/${i}`).then((r) => r.text()),
        ),
      );
      t.deepEqual(
        responses,
        Array.from({ length: 8 }, (_, i) => String(i)),
      );
    } finally {
      await Promise.all(
        [server, upstream].map(
          (service) =>
            new Promise<void>((resolve) => service.close(() => resolve())),
        ),
      );
    }
  });

  test(`${language} callbacks remain safe across worker environments and exit`, async (t) => {
    const workers = Array.from(
      { length: 6 },
      () =>
        new Worker(
          new URL("./fixtures/runtime-functions-worker.mjs", import.meta.url),
          { workerData: language },
        ),
    );
    await Promise.all(
      workers.map(
        (worker) =>
          new Promise<void>((resolve, reject) => {
            worker.on("error", reject);
            worker.on("exit", (code) =>
              code === 0
                ? resolve()
                : reject(new Error(`worker exited ${code}`)),
            );
          }),
      ),
    );
    t.pass();
  });

  test(`${language} rejects unsupported guest arguments before invoking the host`, async (t) => {
    let invoked = false;
    const bash = new Bash(
      options(() => {
        invoked = true;
        return 42;
      }),
    );
    for (const script of language === "python"
      ? ["lookup(b'bytes')", "lookup({1: 2})", "lookup(9007199254740993)"]
      : ["await lookup(() => 42)", "await lookup(9007199254740993)"]) {
      const result = await bash.execute(
        `${language === "python" ? "python" : "ts"} -c ${JSON.stringify(script)}`,
      );
      t.not(result.exitCode, 0);
    }
    t.false(invoked);
  });

  test(`${language} exception messages hide host stacks and paths`, async (t) => {
    const bash = new Bash(
      options(() => {
        throw new Error(
          "failed at /home/host/secret.ts and C:\\private\\file.ts",
        );
      }),
    );
    const result = await bash.execute(call);
    t.not(result.exitCode, 0);
    t.false((result.stderr + result.error).includes("secret.ts"));
    t.false((result.stderr + result.error).includes("file.ts"));
  });

  test(`LangChain forwards ${language} runtime callbacks`, async (t) => {
    const tool = createBashTool(options(() => ({ value: 42 })));
    t.is(await tool.invoke({ commands: command }), "42\n");
  });

  test(`AI adapters execute ${language} runtime callbacks`, async (t) => {
    const config = options(() => ({ value: 42 }));
    const ai = aiBashTool(config);
    t.is((await ai.tools.bash.execute({ commands: command })).trim(), "42");
    const anthropic = anthropicBashTool(config);
    t.is(
      (
        await anthropic.handler({
          type: "tool_use",
          id: "host",
          name: "bash",
          input: { commands: command },
        })
      ).content.trim(),
      "42",
    );
    const openai = openaiBashTool(config);
    t.is(
      (
        await openai.handler({
          id: "host",
          type: "function",
          function: {
            name: "bash",
            arguments: JSON.stringify({ commands: command }),
          },
        })
      ).content.trim(),
      "42",
    );
  });

  test(`LangChain cancellation reaches ${language} host work`, async (t) => {
    let ready!: () => void;
    const started = new Promise<void>((resolve) => {
      ready = resolve;
    });
    let signal: AbortSignal | undefined;
    const tool = createBashTool(
      options(async (_name, _args, _kwargs, ctx) => {
        signal = ctx.signal;
        ready();
        await new Promise((_resolve, reject) =>
          ctx.signal.addEventListener(
            "abort",
            () => reject(new Error("cancelled")),
            { once: true },
          ),
        );
      }),
    );
    const controller = new AbortController();
    const execution = tool.invoke(
      { commands: call },
      { signal: controller.signal },
    );
    // LangChain itself may reject first; attach immediately to avoid an unhandled rejection.
    const settled = execution.catch(() => undefined);
    await started;
    controller.abort();
    await settled;
    for (let i = 0; i < 100 && !signal?.aborted; i++) await delay(5);
    t.true(signal!.aborted);
  });

  test(`${language} callback errors and invalid values remain script failures`, async (t) => {
    for (const handler of [
      () => {
        throw new Error("host failure");
      },
      () => new Date(),
      () => NaN,
      () => 2 ** 60,
      () => () => 42,
      () => {
        const value: unknown[] = [];
        value.push(value);
        return value;
      },
    ]) {
      const bash = new Bash(options(handler));
      t.not((await bash.execute(call)).exitCode, 0);
      t.is((await bash.execute("echo recovered")).stdout, "recovered\n");
    }
  });
}

test("runtime function configuration is validated", (t) => {
  t.throws(() => new Bash({ externalFunctions: ["lookup"] }), {
    message: /externalHandler/,
  });
  t.throws(() => new Bash({ typescriptExternalFunctions: ["lookup"] } as any), {
    message: /typescriptExternalHandler/,
  });
  t.throws(() => new Bash({ externalHandler: () => 42 } as any), {
    message: /python/,
  });
  t.throws(() => new Bash({ typescriptExternalHandler: () => 42 } as any), {
    message: /typescript/,
  });
});

test("both runtimes dispatch independently and advertise host functions", async (t) => {
  const bash = new BashTool({
    python: true,
    typescript: true,
    externalFunctions: ["lookup"],
    externalHandler: () => 21,
    typescriptExternalFunctions: ["lookup"],
    typescriptExternalHandler: (_name, args) => Number(args[0]) * 2,
  });
  const result = await bash.execute(
    `python -c 'from pathlib import Path; n = Path("/tmp/value").write_text(str(lookup()))'; ts -c 'await lookup(await readFile("/tmp/value"))'`,
  );
  t.is(result.stdout, "42\n");
  for (const metadata of [
    bash.description(),
    bash.help(),
    bash.systemPrompt(),
  ]) {
    t.regex(metadata, /Python host functions: lookup/);
    t.regex(metadata, /TypeScript host functions: lookup/);
  }
});

test("pending worker shutdown releases native runtimes without panics", async (t) => {
  const fixture = fileURLToPath(
    new URL("./fixtures/runtime-functions-shutdown.mjs", import.meta.url),
  );
  const { stdout, stderr } = await promisify(execFile)(
    process.execPath,
    [fixture],
    { timeout: 30000 },
  );
  t.is(stdout.trim(), "pending worker shutdown passed");
  t.is(stderr, "");
});
