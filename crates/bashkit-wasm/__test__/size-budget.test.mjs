import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, writeFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { artifactBudget, checkArtifactBudget } from "../bench/artifact.mjs";

function withPackage(run) {
  const pkg = mkdtempSync(join(tmpdir(), "bashkit-budget-"));
  try { run(pkg); } finally { rmSync(pkg, { recursive: true, force: true }); }
}

test("size: only an explicit compiler feature selects the compiler budget profile", () => {
  withPackage(pkg => {
    const ordinary = artifactBudget(pkg);
    assert.equal(ordinary.profile, "default");
    assert.equal(ordinary.limits.gzip, 3120000);
    for (const features of [[], ["terminal"], ["bashkit/experimental_execution_plan_extra"]]) {
      writeFileSync(join(pkg, "build-info.json"), JSON.stringify({ features }));
      assert.deepEqual(artifactBudget(pkg), ordinary);
    }
    for (const feature of ["experimental_execution_plan", "bashkit/experimental_execution_plan"]) {
      writeFileSync(join(pkg, "build-info.json"), JSON.stringify({ features: ["terminal", feature] }));
      const compiled = artifactBudget(pkg);
      assert.equal(compiled.profile, "execution-plan");
      assert.deepEqual(compiled.limits, { ...ordinary.limits, gzip: 3140000, brotli: 2230000 });
      const report = { total: { raw: 7900000, gzip: 3130000, brotli: 2225000 } };
      assert.throws(() => checkArtifactBudget(report, ordinary), /gzip/);
      // Main added output metering after this experiment. The default
      // package retains its limits; only the optional compiler gets extra room.
      for (const [kind, limit] of Object.entries(ordinary.limits)) {
        assert.throws(() => checkArtifactBudget({ total: { ...report.total, gzip: 3110000, brotli: 2200000, [kind]: limit + 1 } }, ordinary), new RegExp(kind));
      }
      checkArtifactBudget(report, compiled);
      for (const [kind, limit] of Object.entries(compiled.limits)) {
        assert.throws(() => checkArtifactBudget({ total: { ...report.total, [kind]: limit + 1 } }, compiled), new RegExp(kind));
      }
    }
  });
});

test("size: malformed feature identity fails rather than selecting a cap", () => {
  withPackage(pkg => {
    for (const features of [undefined, "experimental_execution_plan", [null]]) {
      writeFileSync(join(pkg, "build-info.json"), JSON.stringify({ features }));
      assert.throws(() => artifactBudget(pkg), /invalid build-info features/);
    }
  });
});

test("size: the artifact CLI enforces the selected build profile", () => {
  withPackage(pkg => {
    const header = Buffer.from([0, 97, 115, 109, 1, 0, 0, 0]);
    // A valid custom section exceeds the raw cap while compressing easily.
    const padding = Buffer.alloc(8000001);
    const leb = [];
    for (let size = padding.length; ; size >>>= 7) {
      leb.push((size & 127) | (size > 127 ? 128 : 0));
      if (size <= 127) break;
    }
    const oversized = Buffer.concat([header, Buffer.from([0, ...leb]), padding]);
    for (const features of [[], ["bashkit/experimental_execution_plan"]]) {
      writeFileSync(join(pkg, "bashkit_wasm_bg.wasm"), header);
      writeFileSync(join(pkg, "build-info.json"), JSON.stringify({ features }));
      const command = [fileURLToPath(new URL("../bench/artifact.mjs", import.meta.url)), pkg, "--check"];
      const run = spawnSync(process.execPath, command, { encoding: "utf8" });
      assert.equal(run.status, 0, run.stderr);
      assert.equal(JSON.parse(run.stdout).budgetProfile, features.length ? "execution-plan" : "default");
      writeFileSync(join(pkg, "bashkit_wasm_bg.wasm"), oversized);
      const rejected = spawnSync(process.execPath, command, { encoding: "utf8" });
      assert.notEqual(rejected.status, 0);
      assert.match(rejected.stderr, /raw: .* exceeds 8000000 byte budget/);
    }
  });
});
