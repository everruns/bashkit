import test from "ava";
import { Bash, BashTool } from "../wrapper.js";

test("BuiltinContext exposes AbortSignal and helpers", async (t) => {
  let seen: any = {};
  const bash = new Bash({
    customBuiltins: {
      probe: async (ctx: any) => {
        seen.signal = ctx.signal;
        seen.helpers = [
          typeof ctx.throwIfCancelled,
          typeof ctx.onCancel,
          typeof ctx.fetch,
        ];
        return "ok";
      },
    },
  });
  const result = await bash.execute("probe");
  t.is(result.exitCode, 0);
  t.true(seen.signal instanceof AbortSignal);
  t.deepEqual(seen.helpers, ["function", "function", "function"]);
  t.false(seen.signal.aborted);
});

test("cancel aborts hanging builtin, preserves state, next runs", async (t) => {
  let aborted = false;
  const bash = new Bash({
    customBuiltins: {
      hold: async (ctx: any) => {
        await new Promise<void>((_, reject) => {
          ctx.onCancel(() => {
            aborted = true;
            reject(new Error("cancelled"));
          });
        });
        return "never";
      },
    },
  });
  await bash.execute(
    "MARKER=kept; mkdir -p /tmp/cancel && echo data > /tmp/cancel/file.txt",
  );
  const pending = bash.execute("hold");
  await new Promise((r) => setTimeout(r, 50));
  bash.cancel();
  const cancelled = await pending;
  t.is(cancelled.exitCode, 130);
  t.true(aborted);
  const next = await bash.execute("echo next:$MARKER; cat /tmp/cancel/file.txt");
  t.is(next.exitCode, 0);
  t.true(next.stdout.includes("next:kept"));
  t.true(next.stdout.includes("data"));
});

test("execution timeout aborts the builtin signal and reports exit 124", async (t) => {
  let aborted = false;
  const bash = new Bash({
    timeoutMs: 300,
    customBuiltins: {
      hold: async (ctx: any) => {
        // Records the signal but otherwise ignores it: the native deadline
        // must still enforce exit 124 deterministically.
        ctx.onCancel(() => {
          aborted = true;
        });
        await new Promise((r) => setTimeout(r, 5000));
        return "x";
      },
    },
  });
  const timedOut = await bash.execute("hold");
  t.true(aborted);
  t.is(timedOut.exitCode, 124);
  const next = await bash.execute("echo alive");
  t.is(next.exitCode, 0);
  t.true(next.stdout.includes("alive"));
});

test("Ctrl+C aborts a held server connection via ctx.fetch: exit 130", async (t) => {
  const http = await import("node:http");
  let serverAborted = false;
  const server = http.createServer((req, _res) => {
    // Hold the response indefinitely; detect the client going away.
    req.on("close", () => {
      serverAborted = true;
    });
  });
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", () => resolve()));
  t.teardown(() => server.close());
  const port = (server.address() as any).port;
  try {
    const bash = new Bash({
      customBuiltins: {
        fetchHold: async (ctx: any) => {
          // ctx.fetch combines the execution signal: cancel aborts the request.
          await ctx.fetch(`http://127.0.0.1:${port}/hold`);
          return "unreachable";
        },
      },
    });
    await bash.execute("MARKER=kept");
    const start = Date.now();
    const pending = bash.execute("fetchHold");
    await new Promise((r) => setTimeout(r, 200));
    bash.cancel(); // programmatic Ctrl+C
    const cancelled = await pending;
    const elapsed = Date.now() - start;
    t.is(cancelled.exitCode, 130);
    t.true(elapsed < 10000, `cancellation took ${elapsed}ms`);
    for (let i = 0; i < 100 && !serverAborted; i++) {
      await new Promise((r) => setTimeout(r, 20));
    }
    t.true(serverAborted, "server saw the connection abort");
    const next = await bash.execute("echo next:$MARKER");
    t.is(next.exitCode, 0);
    t.true(next.stdout.includes("next:kept"));
  } finally {
    server.close();
  }
});

// The timeout-mirror coercion (graceful exit 0 -> 124) was open-coded in
// `Bash.execute`'s no-caller-signal branch only. Three of the four async
// `executeNative` call sites skipped it, so the same race reported 124 from
// `Bash` without a signal and 0 from `BashTool`, or from `Bash` whenever the
// caller also passed a signal. These pin the contract on every path: an
// execution that exceeded its deadline is never reported as success.

/** Ignores cancellation entirely; the native deadline enforces 124. */
const holdForever = {
  hold: async (ctx: any) => {
    ctx.onCancel(() => {});
    await new Promise((r) => setTimeout(r, 5000));
    return "x";
  },
};

/**
 * Treats cancellation as a graceful finish. This is the shape that lets the
 * JS-side mirror beat native enforcement: the abort resolves the builtin
 * *successfully*, so the script can reach exit 0 before the native deadline
 * lands, and only the coercion turns that back into 124.
 */
const gracefulOnCancel = {
  hold: async (ctx: any) => {
    await new Promise<void>((resolve) => ctx.onCancel(() => resolve()));
    return "finished";
  },
};

test("execution timeout reports 124 from BashTool, not just Bash", async (t) => {
  const viaBash = await new Bash({
    timeoutMs: 300,
    customBuiltins: holdForever,
  }).execute("hold");
  const viaTool = await new BashTool({
    timeoutMs: 300,
    customBuiltins: holdForever,
  }).execute("hold");
  t.is(viaBash.exitCode, 124);
  t.is(viaTool.exitCode, viaBash.exitCode);
});

test("a builtin resolving on abort cannot mask the deadline", async (t) => {
  const viaBash = await new Bash({
    timeoutMs: 200,
    customBuiltins: gracefulOnCancel,
  }).execute("hold");
  const viaTool = await new BashTool({
    timeoutMs: 200,
    customBuiltins: gracefulOnCancel,
  }).execute("hold");
  t.not(viaBash.exitCode, 0);
  t.not(viaTool.exitCode, 0);
  t.is(viaBash.exitCode, 124);
  t.is(viaTool.exitCode, 124);
});

test("a caller signal does not disable the timeout mirror", async (t) => {
  // A signal that never aborts: its only effect is taking the caller-signal
  // branch, which used to return executeNative() straight through.
  const never = new AbortController();
  const viaBash = await new Bash({
    timeoutMs: 200,
    customBuiltins: gracefulOnCancel,
  }).execute("hold", { signal: never.signal });
  const viaTool = await new BashTool({
    timeoutMs: 200,
    customBuiltins: gracefulOnCancel,
  }).execute("hold", { signal: never.signal });
  t.is(viaBash.exitCode, 124);
  t.is(viaTool.exitCode, 124);
});
