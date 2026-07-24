// Integration tests for the browser wasm bundle (@everruns/bashkit-wasm).
//
// Run against the real built package (crates/bashkit-wasm/pkg, produced by
// scripts/build.sh) under Node's built-in test runner:
//
//   node --test crates/bashkit-wasm/__test__/
//
// Feeding the .wasm bytes straight to init (no fetch, no DOM, no COOP/COEP
// headers) exercises the same "no host configuration required" contract the
// browser relies on.

import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

import { initBashkit, Bash } from "../pkg/index.js";

// Init once for the whole suite.
const wasmBytes = readFileSync(
  fileURLToPath(new URL("../pkg/bashkit_wasm_bg.wasm", import.meta.url)),
);
await initBashkit(wasmBytes);

// --- Sync execution --------------------------------------------------------

test("sync: pipes and text tools", () => {
  const bash = new Bash();
  const r = bash.executeSync('echo "hello world" | tr a-z A-Z');
  assert.equal(r.stdout, "HELLO WORLD\n");
  assert.equal(r.exitCode, 0);
  assert.equal(r.success, true);
});

test("sync: jq is available", () => {
  const bash = new Bash();
  const r = bash.executeSync(`echo '{"a":1,"b":2}' | jq -c '.a + .b'`);
  assert.equal(r.stdout.trim(), "3");
});

test("sync: multi-line script with loop and variables", () => {
  const bash = new Bash();
  const r = bash.executeSync(`
    total=0
    for n in 1 2 3 4; do total=$((total + n)); done
    echo "sum=$total"
  `);
  assert.equal(r.stdout.trim(), "sum=10");
});

test("sync: non-zero exit surfaces", () => {
  const bash = new Bash();
  const r = bash.executeSync("false");
  assert.equal(r.exitCode, 1);
  assert.equal(r.success, false);
});

test("sync: stderr is captured", () => {
  const bash = new Bash();
  const r = bash.executeSync("echo oops >&2");
  assert.equal(r.stderr, "oops\n");
});

// --- Child shell (bash / sh builtin) ---------------------------------------
// Regression: the bash/sh builtin re-parses its child script. On
// wasm32-unknown-unknown that parse must run inline — the native path wraps it
// in tokio::time::timeout + spawn_blocking, whose timer calls std::time, which
// panics ("time not implemented") and poisons the entire wasm module. A single
// `echo hi` at the top level takes a different path, so this only surfaces when
// a subshell is spawned. See execute_shell in the interpreter.

test("child shell: bash -c runs without panicking", async () => {
  const bash = new Bash();
  const r = await bash.execute("bash -c 'echo from-child'");
  assert.equal(r.stdout, "from-child\n");
  assert.equal(r.exitCode, 0);
});

test("child shell: sh -c with a pipe", async () => {
  const bash = new Bash();
  const r = await bash.execute("sh -c 'echo hi | tr a-z A-Z'");
  assert.equal(r.stdout, "HI\n");
  assert.equal(r.exitCode, 0);
});

test("child shell: running a script file", async () => {
  const bash = new Bash({
    files: { "/demo.sh": 'for i in 1 2 3; do echo "iter $i"; done\necho done' },
  });
  const r = await bash.execute("bash /demo.sh");
  assert.equal(r.stdout, "iter 1\niter 2\niter 3\ndone\n");
  assert.equal(r.exitCode, 0);
});

test("child shell: piping a script into bash", async () => {
  const bash = new Bash();
  const r = await bash.execute("echo 'echo piped' | bash");
  assert.equal(r.stdout, "piped\n");
  assert.equal(r.exitCode, 0);
});

// --- Timers, jobs, awk file I/O --------------------------------------------
// Each of these used to panic and poison the whole module on wasm32: they
// reached for a tokio timer (sleep/timeout), tokio::spawn (background jobs), or
// std::thread::spawn (awk file redirects). On single-threaded wasm they must
// run inline instead. See specs/browser-package.md.

test("sleep returns immediately (no timer driver on wasm)", async () => {
  const bash = new Bash();
  const r = await bash.execute("sleep 2; echo slept");
  assert.equal(r.stdout, "slept\n");
  assert.equal(r.exitCode, 0);
});

test("timeout runs the command (no wall-clock enforcement on wasm)", async () => {
  const bash = new Bash();
  const r = await bash.execute("timeout 5 echo hi");
  assert.equal(r.stdout, "hi\n");
  assert.equal(r.exitCode, 0);
});

test("background job runs and wait collects it", async () => {
  const bash = new Bash();
  const r = await bash.execute("echo bg & wait");
  assert.equal(r.stdout, "bg\n");
  assert.equal(r.exitCode, 0);
});

test("awk redirects a write into the VFS", async () => {
  const bash = new Bash();
  const r = await bash.execute(
    'awk \'BEGIN{print "line" > "/out.txt"}\'; cat /out.txt',
  );
  assert.equal(r.stdout, "line\n");
  assert.equal(r.exitCode, 0);
});

test("awk getline reads from the VFS", async () => {
  const bash = new Bash({ files: { "/in.txt": "alpha\nbeta\n" } });
  const r = await bash.execute(
    'awk \'BEGIN{while((getline l < "/in.txt") > 0) print "got:" l}\'',
  );
  assert.equal(r.stdout, "got:alpha\ngot:beta\n");
  assert.equal(r.exitCode, 0);
});

// --- Options ---------------------------------------------------------------

test("options: env and cwd", () => {
  const bash = new Bash({ env: { GREETING: "hi" }, cwd: "/tmp" });
  const r = bash.executeSync('echo "$GREETING from $(pwd)"');
  assert.equal(r.stdout, "hi from /tmp\n");
});

test("options: username and hostname", () => {
  const bash = new Bash({ username: "agent", hostname: "sandbox" });
  assert.equal(bash.executeSync("whoami").stdout.trim(), "agent");
  assert.equal(bash.executeSync("hostname").stdout.trim(), "sandbox");
});

test("options: maxLoopIterations bounds runaway loops", () => {
  const bash = new Bash({ maxLoopIterations: 100 });
  // The loop is capped, so this throws a resource-limit error rather than
  // hanging — the browser build's answer to the absence of a wall-clock timeout.
  assert.throws(
    () => bash.executeSync("while true; do :; done"),
    /loop iterations/,
  );
});

test("options: seeded files", () => {
  const bash = new Bash({ files: { "/config.json": '{"debug":true}' } });
  assert.equal(bash.executeSync("jq -c .debug /config.json").stdout.trim(), "true");
});

// --- Virtual filesystem (Bash helpers) -------------------------------------

test("vfs: write / read / exists / ls / remove", () => {
  const bash = new Bash();
  bash.mkdir("/data");
  bash.writeFile("/data/x.txt", "abc\n");
  assert.equal(bash.readFile("/data/x.txt"), "abc\n");
  assert.equal(bash.exists("/data/x.txt"), true);
  assert.deepEqual(bash.ls("/data"), ["x.txt"]);
  bash.appendFile("/data/x.txt", "def\n");
  assert.equal(bash.readFile("/data/x.txt"), "abc\ndef\n");
  bash.remove("/data/x.txt");
  assert.equal(bash.exists("/data/x.txt"), false);
});

test("vfs: files created by helpers are visible to scripts (and vice versa)", () => {
  const bash = new Bash();
  bash.writeFile("/from-js.txt", "js\n");
  assert.equal(bash.executeSync("cat /from-js.txt").stdout, "js\n");
  bash.executeSync("echo script > /from-script.txt");
  assert.equal(bash.readFile("/from-script.txt"), "script\n");
});

test("vfs: bash.fs() returns a live handle", () => {
  const bash = new Bash();
  const fs = bash.fs();
  fs.writeFile("/via-handle.txt", "handle\n");
  assert.equal(bash.executeSync("cat /via-handle.txt").stdout, "handle\n");
});

// --- Async custom builtins -------------------------------------------------

test("async builtin: awaited by execute(), with argv + stdin", async () => {
  const bash = new Bash({
    customBuiltins: {
      fetchy: async (ctx) => {
        await Promise.resolve();
        return `got:${ctx.argv[0]}:${ctx.stdin ?? ""}`;
      },
    },
  });
  const r = await bash.execute("echo -n payload | fetchy 42 | tr a-z A-Z");
  assert.equal(r.stdout, "GOT:42:PAYLOAD");
});

test("builtin ctx: env is a plain object, not a Map", async () => {
  let seenEnv;
  const bash = new Bash({
    env: { TOKEN: "secret" },
    customBuiltins: {
      inspect: (ctx) => {
        seenEnv = ctx.env;
        return "";
      },
    },
  });
  await bash.execute("inspect");
  assert.equal(typeof seenEnv, "object");
  assert.ok(!(seenEnv instanceof Map));
  assert.equal(seenEnv.TOKEN, "secret");
});

test("builtin ctx.fs: reads and writes the same VFS as the script", async () => {
  const bash = new Bash({
    customBuiltins: {
      // Reads an input file via ctx.fs, writes a derived output file.
      transform: (ctx) => {
        const input = ctx.fs.readFile(ctx.argv[0]);
        ctx.fs.writeFile("/out.txt", input.toUpperCase());
        return "done\n";
      },
    },
  });
  bash.writeFile("/in.txt", "hello\n");
  const r = await bash.execute("transform /in.txt");
  assert.equal(r.stdout, "done\n");
  assert.equal(bash.readFile("/out.txt"), "HELLO\n");
});

test("async builtin under executeSync fails fast without invoking callback", () => {
  let invoked = false;
  const bash = new Bash({
    customBuiltins: {
      slowly: async () => {
        invoked = true;
        return "nope";
      },
    },
  });
  const r = bash.executeSync("slowly");
  assert.equal(r.exitCode, 1);
  assert.match(r.stderr, /execute\(\)/);
  assert.equal(invoked, false);
});

test("sync builtin works under executeSync", () => {
  const bash = new Bash({
    customBuiltins: { shout: (ctx) => (ctx.argv[0] ?? "").toUpperCase() + "\n" },
  });
  assert.equal(bash.executeSync("shout hello").stdout, "HELLO\n");
});

test("throwing builtin -> stderr, exit 1", async () => {
  const bash = new Bash({
    customBuiltins: {
      boom: () => {
        throw new Error("kaboom");
      },
    },
  });
  const r = await bash.execute("boom");
  assert.equal(r.exitCode, 1);
  assert.match(r.stderr, /kaboom/);
});

test("builtins compose in a pipeline with jq", async () => {
  const bash = new Bash({
    customBuiltins: {
      graphql: async (ctx) => {
        await Promise.resolve();
        // Pretend this issued ctx.stdin as a query and got JSON back.
        return JSON.stringify({ data: { echo: ctx.stdin?.trim() } });
      },
    },
  });
  const r = await bash.execute('echo -n "ping" | graphql | jq -r .data.echo');
  assert.equal(r.stdout, "ping\n");
});

// --- Lifecycle -------------------------------------------------------------

test("reset clears env and VFS", () => {
  const bash = new Bash();
  bash.executeSync("export X=1; echo keep > /f.txt");
  bash.reset();
  const r = bash.executeSync(
    "echo [${X-unset}] $(cat /f.txt 2>/dev/null || echo gone)",
  );
  assert.equal(r.stdout, "[unset] gone\n");
});

test("reset preserves registered custom builtins", async () => {
  const bash = new Bash({
    customBuiltins: { ping: () => "pong\n" },
  });
  bash.reset();
  const r = await bash.execute("ping");
  assert.equal(r.stdout, "pong\n");
});

test("multiple independent instances do not share state", () => {
  const a = new Bash();
  const b = new Bash();
  a.executeSync("export SHARED=a; echo a > /marker.txt");
  assert.equal(b.executeSync("echo [${SHARED-none}]").stdout, "[none]\n");
  assert.equal(b.exists("/marker.txt"), false);
});
