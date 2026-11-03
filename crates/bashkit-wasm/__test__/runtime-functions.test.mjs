// Optional runtimes are tested against the real single-threaded wasm binary.
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { AsyncLocalStorage } from "node:async_hooks";
import { Worker } from "node:worker_threads";
import { execFileSync } from "node:child_process";
const variant = process.env.BASHKIT_WASM_VARIANT;
if (variant === "runtimes") {
  const { initBashkit, Bash } = await import("../pkg/runtimes/index.js");
  await initBashkit(
    await readFile(
      new URL("../pkg/runtimes/bashkit_wasm_bg.wasm", import.meta.url),
    ),
  );
  for (const language of ["python", "typescript"]) {
    const call =
      language === "python"
        ? `python -c 'print(lookup(21, multiplier=2))'`
        : `ts -c 'await lookup(21, 2)'`;
    const options = (handler) =>
      language === "python"
        ? {
            python: true,
            externalFunctions: ["lookup"],
            externalHandler: handler,
          }
        : {
            typescript: true,
            typescriptExternalFunctions: ["lookup"],
            typescriptExternalHandler: (name, args, context) =>
              handler(name, args.slice(0, 1), { multiplier: args[1] }, context),
          };
    test(`${language} browser host functions, reset and VFS`, async () => {
      const bash = new Bash(
        options(async (name, args, kwargs, { signal }) => {
          assert.equal(name, "lookup");
          assert.equal(signal.aborted, false);
          await new Promise((resolve) => setTimeout(resolve, 1));
          return args[0] * kwargs.multiplier;
        }),
      );
      for (let i = 0; i < 2; i++) {
        const result = await bash.execute(call);
        assert.equal(result.exitCode, 0, result.stderr);
        assert.equal(result.stdout, "42\n");
        assert.notEqual(bash.executeSync(call).exitCode, 0);
        bash.reset();
      }
    });
    test(`${language} browser pending host work aborts on timeout`, async () => {
      let aborted = false,
        started = false;
      const bash = new Bash({
        ...options(async (_name, _args, _kwargs, { signal }) => {
          started = true;
          await new Promise((_resolve, reject) =>
            signal.addEventListener(
              "abort",
              () => {
                aborted = true;
                reject(new Error("aborted"));
              },
              { once: true },
            ),
          );
        }),
        timeoutMs: 200,
      });
      await bash.execute(call).then(
        (r) => assert.notEqual(r.exitCode, 0),
        () => {},
      );
      assert.equal(started, true);
      assert.equal(aborted, true);
      assert.equal((await bash.execute("echo reusable")).stdout, "reusable\n");
    });

    test(`${language} browser cancellation and busy access are isolated`, async () => {
      let ready;
      const started = new Promise((r) => {
        ready = r;
      });
      let signal;
      const bash = new Bash(
        options(async (_name, _args, _kwargs, context) => {
          signal = context.signal;
          ready();
          await new Promise((_resolve, reject) =>
            signal.addEventListener(
              "abort",
              () => reject(new Error("aborted")),
              { once: true },
            ),
          );
        }),
      );
      const controller = new AbortController();
      const execution = bash.execute(call, { signal: controller.signal });
      await started;
      await assert.rejects(bash.execute("echo concurrent"), /busy/);
      assert.throws(() => bash.reset(), /busy/);
      controller.abort();
      await execution.catch(() => {});
      assert.equal(signal.aborted, true);
      bash.clearCancel();
      assert.equal((await bash.execute("echo reusable")).stdout, "reusable\n");
    });

    test(`${language} browser recursive execution and invalid values fail safely`, async () => {
      let bash;
      bash = new Bash(options(() => bash.execute("echo recursive")));
      assert.notEqual((await bash.execute(call)).exitCode, 0);
      for (const value of [NaN, new Date(), () => 42, 2 ** 60]) {
        const invalid = new Bash(options(() => value));
        assert.notEqual((await invalid.execute(call)).exitCode, 0);
      }
      let called = false;
      const badGuest = new Bash(
        options(() => {
          called = true;
          return 42;
        }),
      );
      const script =
        language === "python"
          ? `python -c 'lookup({1: 2})'`
          : `ts -c 'await lookup(() => 42)'`;
      assert.notEqual((await badGuest.execute(script)).exitCode, 0);
      assert.equal(called, false);
    });

    test(`${language} browser CPU deadlines use a real clock`, async () => {
      const bash = new Bash({ [language]: true, timeoutMs: 50 });
      const script =
        language === "python"
          ? `python -c 'while True: pass'`
          : `ts -c 'while (true) {}'`;
      const start = performance.now();
      await bash.execute(script).then(
        (r) => assert.notEqual(r.exitCode, 0),
        () => {},
      );
      assert.ok(performance.now() - start < 3000);
      assert.equal((await bash.execute("echo reusable")).stdout, "reusable\n");
    });
  }

  test("both browser runtimes share VFS and host functions survive checkout", async () => {
    const bash = new Bash({
      python: true,
      typescript: true,
      externalFunctions: ["lookup"],
      externalHandler: () => 21,
      typescriptExternalFunctions: ["double"],
      typescriptExternalHandler: (_name, args) => Number(args[0]) * 2,
    });
    const result = await bash.execute(
      `python -c 'from pathlib import Path; n = Path("/tmp/number").write_text(str(lookup()))'; ts -c 'await double(await readFile("/tmp/number"))'`,
    );
    assert.equal(result.stdout, "42\n");
    const commit = bash.commit();
    bash.reset();
    bash.checkout(commit.id, commit.objects);
    assert.equal(
      (
        await bash.execute(
          `ts -c 'await double(await readFile("/tmp/number"))'`,
        )
      ).stdout,
      "42\n",
    );
    assert.equal(
      (await bash.execute(`python -c 'print(lookup())'`)).stdout,
      "21\n",
    );
  });

  test("browser WASM callbacks stay independent across worker environments", async () => {
    await Promise.all(
      Array.from(
        { length: 4 },
        () =>
          new Promise((resolve, reject) => {
            const worker = new Worker(
              new URL("./runtime-worker.mjs", import.meta.url),
            );
            worker.on("error", reject);
            worker.on("exit", (code) =>
              code ? reject(new Error(`worker exited ${code}`)) : resolve(),
            );
          }),
      ),
    );
  });
  test("published archive includes slim and optional runtime entry points", () => {
    const [archive] = JSON.parse(
      execFileSync("npm", ["pack", "--dry-run", "--json", "--ignore-scripts"], {
        cwd: new URL("../pkg/", import.meta.url),
        encoding: "utf8",
        timeout: 30000,
      }),
    );
    const files = new Set(archive.files.map((file) => file.path));
    for (const prefix of ["", "runtimes/"]) {
      for (const name of [
        "index.js",
        "index.d.ts",
        "runtime-functions.js",
        "bashkit_wasm.js",
        "bashkit_wasm_bg.wasm",
        "build-info.json",
      ]) {
        assert.ok(files.has(prefix + name), `archive lacks ${prefix + name}`);
      }
    }
  });

  test("WASM host callbacks keep independent caller async contexts", async () => {
    const request = new AsyncLocalStorage();
    const make = () =>
      new Bash({
        python: true,
        typescript: true,
        externalFunctions: ["lookup"],
        externalHandler: async () => request.getStore(),
        typescriptExternalFunctions: ["lookup"],
        typescriptExternalHandler: async () => request.getStore(),
      });
    const a = make(),
      b = make();
    const results = await Promise.all([
      request.run("a", () => a.execute("python -c 'print(lookup())'")),
      request.run("b", () => b.execute("ts -c 'await lookup()'")),
    ]);
    assert.deepEqual(
      results.map((r) => r.stdout),
      ["a\n", "b\n"],
    );
  });

  test("one WASM execution can await concurrent host calls", async () => {
    let count = 0,
      release;
    const gate = new Promise((resolve) => {
      release = resolve;
    });
    const arrive = async () => {
      if (++count === 2) release();
      await gate;
      return 42;
    };
    const bash = new Bash({
      python: true,
      typescript: true,
      timeoutMs: 2000,
      externalFunctions: ["lookup"],
      externalHandler: arrive,
      typescriptExternalFunctions: ["lookup"],
      typescriptExternalHandler: arrive,
    });
    const result = await bash.execute(
      "python -c 'print(lookup())' & ts -c 'await lookup()' & wait",
    );
    assert.equal(result.exitCode, 0, result.stderr);
    assert.equal(count, 2);
    assert.deepEqual(result.stdout.trim().split("\n"), ["42", "42"]);
  });
  test("runtime streaming reports a callback failure once without host paths", async () => {
    const bash = new Bash({
      python: true,
      externalFunctions: ["lookup"],
      externalHandler: () => 42,
    });
    let calls = 0;
    await assert.rejects(
      bash.executeWithOutput(
        "echo before; python -c 'print(lookup())'; echo after",
        () => {
          calls++;
          throw new Error("output failed in /home/host/private.js");
        },
      ),
      (error) =>
        /output failed/.test(error.message) &&
        !error.message.includes("private.js"),
    );
    assert.equal(calls, 1);
    assert.equal((await bash.execute("echo reusable")).stdout, "reusable\n");
  });

  for (const language of ["python", "typescript"]) {
    const script =
      language === "python"
        ? "python -c 'print(lookup())'"
        : "ts -c 'await lookup()'";
    const runtime = (handler) =>
      language === "python"
        ? {
            python: true,
            externalFunctions: ["lookup"],
            externalHandler: (_n, _a, _k, context) => handler(context),
          }
        : {
            typescript: true,
            typescriptExternalFunctions: ["lookup"],
            typescriptExternalHandler: (_n, _a, context) => handler(context),
          };
    test(
      `${language} host functions share live commands, byte sinks and capture`,
      { timeout: 5000 },
      async () => {
        let signal;
        const bash = new Bash({
          ...runtime(async (context) => {
            signal = context.signal;
            await new Promise((resolve) => setTimeout(resolve, 1));
            return 42;
          }),
          commands: {
            async copy(ctx) {
              for await (const bytes of ctx.stdin)
                await ctx.stdout.write(bytes);
              return 0;
            },
          },
        });
        for (let i = 0; i < 2; i++) {
          const chunks = [];
          const result = await bash.execute(`${script} | copy`, {
            capture: 2,
            async stdout(bytes) {
              assert.equal(signal.aborted, false);
              await new Promise((resolve) => setTimeout(resolve, 1));
              assert.equal(signal.aborted, false);
              chunks.push(bytes);
            },
          });
          assert.equal(result.exitCode, 0, result.stderr);
          assert.equal(result.stdout, "42");
          assert.equal(result.stdoutTruncated, true);
          assert.equal(Buffer.concat(chunks).toString(), "42\n");
          assert.equal(signal.aborted, true);
          bash.reset();
        }
      },
    );
    test(
      `${language} live cancellation revokes pending host calls and permits reuse`,
      { timeout: 5000 },
      async () => {
        let ready, signal;
        const started = new Promise((resolve) => {
          ready = resolve;
        });
        const bash = new Bash(
          runtime(async (context) => {
            signal = context.signal;
            ready();
            await new Promise(() => {});
          }),
        );
        await assert.rejects(
          bash.execute(script, { capture: 64, signal: AbortSignal.abort() }),
          (error) => error.name === "AbortError",
        );
        assert.equal(signal, undefined);
        const controller = new AbortController();
        const pending = bash.execute(script, {
          capture: 64,
          signal: controller.signal,
        });
        await started;
        controller.abort();
        await assert.rejects(pending, /cancelled/);
        assert.equal(signal.aborted, true);
        assert.equal(
          (await bash.execute("echo recovered")).stdout,
          "recovered\n",
        );
      },
    );
    test(
      `${language} live sink deadlines revoke the host session`,
      { timeout: 5000 },
      async () => {
        let signal,
          delivered = false;
        const bash = new Bash({
          ...runtime((context) => {
            signal = context.signal;
            return 42;
          }),
          timeoutMs: 200,
        });
        await assert.rejects(
          bash.execute(script, {
            stdout: async () => {
              delivered = true;
              assert.equal(signal.aborted, false);
              await new Promise(() => {});
            },
          }),
          /timed out|timeout|deadline/i,
        );
        assert.equal(delivered, true);
        assert.equal(signal.aborted, true);
        assert.equal(
          (await bash.execute("echo reusable")).stdout,
          "reusable\n",
        );
      },
    );
  }
  test(
    "live execution preserves each host callback caller context",
    { timeout: 5000 },
    async () => {
      const request = new AsyncLocalStorage();
      const make = () =>
        new Bash({
          typescript: true,
          typescriptExternalFunctions: ["lookup"],
          typescriptExternalHandler: async () => {
            await new Promise((resolve) => setTimeout(resolve, 1));
            return request.getStore();
          },
        });
      const a = make(),
        b = make();
      const options = {
        capture: 64,
        async stdout() {
          await new Promise((resolve) => setTimeout(resolve, 1));
        },
      };
      const results = await Promise.all([
        request.run("a", () => a.execute("ts -c 'await lookup()'", options)),
        request.run("b", () => b.execute("ts -c 'await lookup()'", options)),
      ]);
      assert.deepEqual(
        results.map((result) => result.stdout),
        ["a\n", "b\n"],
      );
    },
  );
  test("text observers reject incompatible live options before host work", async () => {
    let calls = 0;
    const bash = new Bash({
      python: true,
      externalFunctions: ["lookup"],
      externalHandler: () => ++calls,
    });
    await assert.rejects(
      bash.execute("python -c 'print(lookup())'", {
        capture: 64,
        onOutput() {},
      }),
      /byte sinks/,
    );
    assert.equal(calls, 0);
    assert.equal((await bash.execute("echo reusable")).stdout, "reusable\n");
  });
}
