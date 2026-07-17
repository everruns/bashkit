// Headless smoke test for the browser wasm bundle.
//
// Runs under Node (no DOM, no fetch, no COOP/COEP) by feeding the .wasm bytes
// straight to init — proving the bundle needs no special host configuration.
// Run: node crates/bashkit-wasm/scripts/smoke-test.mjs  (after build.sh)

import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import assert from "node:assert/strict";

import { initBashkit, Bash } from "../pkg/index.js";

const wasmBytes = readFileSync(
  fileURLToPath(new URL("../pkg/bashkit_wasm_bg.wasm", import.meta.url)),
);
await initBashkit(wasmBytes);

let passed = 0;
function check(name, cond) {
  assert.ok(cond, name);
  passed++;
  console.log(`  ok - ${name}`);
}

// 1. Sync execution + pipes.
{
  const bash = new Bash();
  const r = bash.executeSync('echo "hello world" | tr a-z A-Z');
  check("sync echo|tr", r.stdout === "HELLO WORLD\n" && r.success && r.exitCode === 0);
}

// 2. jq is available.
{
  const bash = new Bash();
  const r = bash.executeSync(`echo '{"a":1,"b":2}' | jq -c '.a + .b'`);
  check("jq arithmetic", r.stdout.trim() === "3");
}

// 3. Non-zero exit surfaces.
{
  const bash = new Bash();
  const r = bash.executeSync("false");
  check("non-zero exit", r.exitCode === 1 && !r.success);
}

// 4. Options: env + cwd.
{
  const bash = new Bash({ env: { GREETING: "hi" }, cwd: "/tmp" });
  const r = bash.executeSync('echo "$GREETING from $(pwd)"');
  check("env + cwd", r.stdout === "hi from /tmp\n");
}

// 5. Virtual filesystem helpers.
{
  const bash = new Bash();
  bash.mkdir("/data");
  bash.writeFile("/data/x.txt", "abc\n");
  check("vfs write/read", bash.readFile("/data/x.txt") === "abc\n");
  check("vfs exists", bash.exists("/data/x.txt") === true);
  check("vfs ls", JSON.stringify(bash.ls("/data")) === JSON.stringify(["x.txt"]));
  const r = bash.executeSync("cat /data/x.txt");
  check("vfs visible to script", r.stdout === "abc\n");
}

// 6. Seeded files.
{
  const bash = new Bash({ files: { "/config.json": '{"debug":true}' } });
  const r = bash.executeSync("jq -c .debug /config.json");
  check("seeded file", r.stdout.trim() === "true");
}

// 7. Async custom builtin awaited by execute().
{
  const bash = new Bash({
    customBuiltins: {
      fetchy: async (ctx) => {
        await Promise.resolve();
        return `got:${ctx.argv[0]}:${ctx.stdin ?? ""}`;
      },
    },
  });
  const r = await bash.execute('echo -n payload | fetchy 42 | tr a-z A-Z');
  check("async builtin via execute", r.stdout === "GOT:42:PAYLOAD");
}

// 8. Async builtin under executeSync fails fast (does not hang).
{
  const bash = new Bash({
    customBuiltins: { slowly: async () => "nope" },
  });
  const r = bash.executeSync("slowly");
  check("async builtin under sync fails cleanly", r.exitCode === 1 && /execute\(\)/.test(r.stderr));
}

// 9. Sync builtin works under executeSync.
{
  const bash = new Bash({
    customBuiltins: { shout: (ctx) => (ctx.argv[0] ?? "").toUpperCase() + "\n" },
  });
  const r = bash.executeSync("shout hello");
  check("sync builtin under sync", r.stdout === "HELLO\n");
}

// 10. Builtin that throws -> stderr, exit 1.
{
  const bash = new Bash({
    customBuiltins: {
      boom: () => {
        throw new Error("kaboom");
      },
    },
  });
  const r = await bash.execute("boom");
  check("throwing builtin", r.exitCode === 1 && /kaboom/.test(r.stderr));
}

// 11. reset() clears state.
{
  const bash = new Bash();
  bash.executeSync("export X=1; echo keep > /f.txt");
  bash.reset();
  const r = bash.executeSync("echo [${X-unset}] $(cat /f.txt 2>/dev/null || echo gone)");
  check("reset clears env + vfs", r.stdout === "[unset] gone\n");
}

console.log(`\n${passed} checks passed.`);
