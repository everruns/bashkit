// A tiny valid WASM fixture keeps CLI/budget regression coverage bounded.
import { test } from "node:test";
import assert from "node:assert/strict";
import { copyFileSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { spawnSync } from "node:child_process";

test("artifact CLI accepts --check with default package and enforces budgets", () => {
  const root = mkdtempSync(join(tmpdir(), "bashkit-artifact-"));
  try {
    const bench = join(root, "bench"), pkg = join(root, "pkg");
    mkdirSync(bench); mkdirSync(pkg);
    const script = join(bench, "artifact.mjs");
    copyFileSync(new URL("../bench/artifact.mjs", import.meta.url), script);
    writeFileSync(join(pkg, "index.js"), "export {};");
    writeFileSync(join(pkg, "bashkit_wasm.js"), "export {};");
    writeFileSync(join(pkg, "bashkit_wasm_bg.wasm"), Buffer.from([0, 97, 115, 109, 1, 0, 0, 0]));
    const budget = join(bench, "size-budget.json");
    writeFileSync(budget, '{"raw":1000,"gzip":1000,"brotli":1000}');
    const run = args => spawnSync(process.execPath, [script, ...args], { encoding: "utf8", timeout: 10000 });
    const result = run(["--check"]);
    assert.equal(result.status, 0, result.stderr);
    assert.equal(JSON.parse(result.stdout).total.raw, 28);
    const explicit = run([pkg, "--check"]);
    assert.equal(explicit.status, 0, explicit.stderr);
    assert.deepEqual(JSON.parse(explicit.stdout).total, JSON.parse(result.stdout).total);
    writeFileSync(budget, '{"raw":27}');
    const rejected = run(["--check"]);
    assert.notEqual(rejected.status, 0);
    assert.match(rejected.stderr, /28 exceeds 27 byte budget/);
  } finally { rmSync(root, { recursive: true, force: true }); }
});
