// Benchmark customBuiltins called from bash. Crosses the Node<->Rust boundary
// (NAPI threadsafe function, async dispatch) — measure the per-call overhead
// for sync and async callbacks, and compare against a baked-in builtin
// (`:` / `true`) as a "no JS callback" baseline.
//
// These tests assert only very loose upper bounds so they don't flake on slow
// CI hardware. The interesting output is the printed timings — run with
// `pnpm exec ava __test__/custom-builtins-perf.spec.ts -v` to see them.

import test from "ava";
import { Bash } from "../wrapper.js";

const ITERATIONS_IN_LOOP = 500;
const EXECUTE_ROUNDTRIPS = 200;

function fmt(ms: number, calls: number): string {
  const perCall = ms / calls;
  return `${ms.toFixed(1)} ms total, ${perCall.toFixed(3)} ms/call (${calls} calls)`;
}

async function timeIt(label: string, fn: () => Promise<void>): Promise<number> {
  const t0 = performance.now();
  await fn();
  const elapsed = performance.now() - t0;
  console.log(`  ${label}: ${elapsed.toFixed(1)} ms`);
  return elapsed;
}

// ----------------------------------------------------------------------------
// In-bash loop: many builtin invocations within a single execute() call.
// Isolates the per-call cross-runtime overhead from parser/execute() setup.
// ----------------------------------------------------------------------------

test("perf: sync custom builtin in bash loop", async (t) => {
  let counter = 0;
  const bash = new Bash({
    customBuiltins: {
      tick: () => {
        counter++;
        return "";
      },
    },
  });

  const script = `for ((i=0; i<${ITERATIONS_IN_LOOP}; i++)); do tick; done`;
  const elapsed = await timeIt(
    `sync x${ITERATIONS_IN_LOOP} in-loop`,
    async () => {
      const r = await bash.execute(script);
      t.is(r.exitCode, 0);
    },
  );

  t.is(counter, ITERATIONS_IN_LOOP);
  console.log(`  -> ${fmt(elapsed, ITERATIONS_IN_LOOP)}`);
  t.true(elapsed < 60_000, `sync loop too slow: ${elapsed}ms`);
});

test("perf: async custom builtin in bash loop", async (t) => {
  let counter = 0;
  const bash = new Bash({
    customBuiltins: {
      tick: async () => {
        counter++;
        // Force a real microtask hop — the async path waits on a JS promise.
        await Promise.resolve();
        return "";
      },
    },
  });

  const script = `for ((i=0; i<${ITERATIONS_IN_LOOP}; i++)); do tick; done`;
  const elapsed = await timeIt(
    `async x${ITERATIONS_IN_LOOP} in-loop`,
    async () => {
      const r = await bash.execute(script);
      t.is(r.exitCode, 0);
    },
  );

  t.is(counter, ITERATIONS_IN_LOOP);
  console.log(`  -> ${fmt(elapsed, ITERATIONS_IN_LOOP)}`);
  t.true(elapsed < 60_000, `async loop too slow: ${elapsed}ms`);
});

test("perf: baseline baked-in `:` in bash loop (no JS callback)", async (t) => {
  const bash = new Bash();
  const script = `for ((i=0; i<${ITERATIONS_IN_LOOP}; i++)); do :; done`;
  const elapsed = await timeIt(
    `baseline x${ITERATIONS_IN_LOOP} in-loop`,
    async () => {
      const r = await bash.execute(script);
      t.is(r.exitCode, 0);
    },
  );
  console.log(`  -> ${fmt(elapsed, ITERATIONS_IN_LOOP)}`);
  t.true(elapsed < 60_000, `baseline loop too slow: ${elapsed}ms`);
});

// ----------------------------------------------------------------------------
// execute()-per-call: includes script parse + execute() setup each iteration.
// Closer to the "tool call per turn" shape an LLM agent would generate.
// ----------------------------------------------------------------------------

test("perf: sync custom builtin via repeated execute()", async (t) => {
  const bash = new Bash({
    customBuiltins: {
      tick: () => "",
    },
  });

  const elapsed = await timeIt(
    `sync x${EXECUTE_ROUNDTRIPS} execute() roundtrips`,
    async () => {
      for (let i = 0; i < EXECUTE_ROUNDTRIPS; i++) {
        const r = await bash.execute("tick");
        t.is(r.exitCode, 0);
      }
    },
  );
  console.log(`  -> ${fmt(elapsed, EXECUTE_ROUNDTRIPS)}`);
  t.true(elapsed < 60_000, `sync execute() too slow: ${elapsed}ms`);
});

test("perf: async custom builtin via repeated execute()", async (t) => {
  const bash = new Bash({
    customBuiltins: {
      tick: async () => {
        await Promise.resolve();
        return "";
      },
    },
  });

  const elapsed = await timeIt(
    `async x${EXECUTE_ROUNDTRIPS} execute() roundtrips`,
    async () => {
      for (let i = 0; i < EXECUTE_ROUNDTRIPS; i++) {
        const r = await bash.execute("tick");
        t.is(r.exitCode, 0);
      }
    },
  );
  console.log(`  -> ${fmt(elapsed, EXECUTE_ROUNDTRIPS)}`);
  t.true(elapsed < 60_000, `async execute() too slow: ${elapsed}ms`);
});

// ----------------------------------------------------------------------------
// Stdin/argv roundtrip: payload moves bytes in both directions through the
// JSON BuiltinContext envelope. Useful to see if argument marshaling
// dominates the per-call cost.
// ----------------------------------------------------------------------------

test("perf: sync custom builtin with argv + stdin payload", async (t) => {
  const bash = new Bash({
    customBuiltins: {
      passthru: (ctx) => `${ctx.argv.length}:${(ctx.stdin ?? "").length}\n`,
    },
  });

  const N = 200;
  const script = `for ((i=0; i<${N}; i++)); do echo "hello world" | passthru a b c d e; done`;
  const elapsed = await timeIt(`payload x${N} in-loop`, async () => {
    const r = await bash.execute(script);
    t.is(r.exitCode, 0);
    // Each call emits "5:12\n" (5 argv, "hello world\n" = 12 bytes).
    t.is(r.stdout, "5:12\n".repeat(N));
  });
  console.log(`  -> ${fmt(elapsed, N)}`);
  t.true(elapsed < 60_000, `payload loop too slow: ${elapsed}ms`);
});
