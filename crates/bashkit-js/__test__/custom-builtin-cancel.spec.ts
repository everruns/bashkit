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

// Regression: the JS timeout mirror and the native deadline observe the same
// duration, so either can win. Only the mirror-wins direction was handled, so
// when native resolved first the mirror was cleared unfired and a builtin that
// outlived the deadline never saw `ctx.signal` abort — its `ctx.fetch` kept
// hanging on after `execute()` had already returned 124. Blocking the event
// loop past the deadline from inside the builtin forces the native-wins order.
test("execution timeout aborts the builtin signal when the native deadline wins", async (t) => {
  let aborted = false;
  let signal: AbortSignal | undefined;
  const bash = new Bash({
    timeoutMs: 300,
    customBuiltins: {
      hold: async (ctx: any) => {
        signal = ctx.signal;
        ctx.onCancel(() => {
          aborted = true;
        });
        // Starve the loop so the mirror's 300ms timer cannot run before the
        // native deadline delivers its result.
        const until = Date.now() + 700;
        while (Date.now() < until) {
          /* busy-wait: deliberately blocking */
        }
        await new Promise((r) => setTimeout(r, 5000));
        return "x";
      },
    },
  });
  const timedOut = await bash.execute("hold");
  t.is(timedOut.exitCode, 124);
  t.true(aborted, "builtin onCancel ran before execute() resolved");
  t.true(signal?.aborted, "ctx.signal is aborted once the deadline passed");
  const next = await bash.execute("echo alive");
  t.is(next.exitCode, 0);
  t.true(next.stdout.includes("alive"));
});

// The cleanup abort keys off exit 124 *and* elapsed time, not elapsed time
// alone. A builtin that returns gracefully right around the deadline reports
// exit 0, and pairing that with an aborted signal would be neither a clean
// success nor a timeout. The builtin here returns just under a short deadline,
// so the cleanup is reached at or past it with a successful result.
test("a graceful return near the deadline leaves the builtin signal intact", async (t) => {
  let aborted = false;
  let signal: AbortSignal | undefined;
  const bash = new Bash({
    timeoutMs: 120,
    customBuiltins: {
      quick: async (ctx: any) => {
        signal = ctx.signal;
        ctx.onCancel(() => {
          aborted = true;
        });
        await new Promise((r) => setTimeout(r, 90));
        return "done";
      },
    },
  });
  const result = await bash.execute("quick");
  // Only assert the contract when the run really did succeed; a loaded machine
  // may push it past the deadline, which is a genuine timeout, not this bug.
  if (result.exitCode === 0) {
    t.false(aborted, "a successful run must not fire onCancel");
    t.false(signal?.aborted, "a successful run must not abort ctx.signal");
    t.true(result.stdout.includes("done"));
  } else {
    t.is(result.exitCode, 124);
    t.pass("run exceeded the deadline; timeout path covered elsewhere");
  }
});

/**
 * Resolve true when `signal` settles, false if `ms` elapses first. The timer is
 * always cleared, so a won race leaves nothing holding the event loop open.
 */
async function raceWithTimeout(
  signal: Promise<unknown>,
  ms: number,
): Promise<boolean> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  try {
    return await Promise.race([
      signal.then(() => true),
      new Promise<boolean>((resolve) => {
        timer = setTimeout(() => resolve(false), ms);
      }),
    ]);
  } finally {
    if (timer !== undefined) clearTimeout(timer);
  }
}

test("Ctrl+C aborts a held server connection via ctx.fetch: exit 130", async (t) => {
  const http = await import("node:http");
  // This test cancels an *in-flight* request, so it has to know the request is
  // actually in flight before cancelling. It used to assume so after a fixed
  // 200ms, which is a race: `bash.execute` has to reach the native
  // interpreter, dispatch the builtin, and let undici open the connection
  // first, and sibling tests in this file show builtin dispatch taking
  // 1.3-1.5s on `windows-latest`. When 200ms ran out first, `cancel()` landed
  // before the builtin ran, so no request was ever sent, the handler never
  // ran, and nothing could ever report a disconnect — the assertion failed on
  // a request that did not exist. That is the windows-latest flake in JS runs
  // 45816668178 and 45980836967, and it is why a longer wait for the
  // disconnect did not help: the two Windows lanes disagree run to run, so
  // roughly half the time the 200ms won.
  //
  // So both halves now wait on events instead of the clock: `requestArrived`
  // gates the cancel, and `disconnected` resolves on the first signal the
  // client went away. Watching the socket as well as the request drops the
  // assumption that `req` 'close' is the event that lands first on every
  // platform.
  let sawRequest!: () => void;
  const requestArrived = new Promise<void>((resolve) => {
    sawRequest = resolve;
  });
  let sawDisconnect!: () => void;
  const disconnected = new Promise<void>((resolve) => {
    sawDisconnect = resolve;
  });
  const server = http.createServer((req, _res) => {
    // Hold the response indefinitely; detect the client going away.
    sawRequest();
    req.on("close", sawDisconnect);
    req.on("aborted", sawDisconnect);
    req.socket.on("close", sawDisconnect);
    req.socket.on("error", sawDisconnect);
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
    const pending = bash.execute("fetchHold");
    // Wait for the server to actually hold the request, so the cancel below
    // aborts something that exists. Bounded so a request that never arrives
    // fails here, where the cause is obvious, rather than as a mystery
    // disconnect timeout further down.
    const arrived = await raceWithTimeout(requestArrived, 30000);
    t.true(arrived, "server received the held request before cancel");
    // Timed from the cancel, not from `execute`: the bound below is about how
    // fast cancellation unwinds, and starting the clock earlier would fold in
    // however long dispatch and connection setup took on a slow runner.
    const start = Date.now();
    bash.cancel(); // programmatic Ctrl+C
    const cancelled = await pending;
    const elapsed = Date.now() - start;
    t.is(cancelled.exitCode, 130);
    t.true(elapsed < 10000, `cancellation took ${elapsed}ms`);
    const serverAborted = await raceWithTimeout(disconnected, 20000);
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
