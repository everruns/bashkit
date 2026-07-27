import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";
import test from "node:test";

const packageJson = JSON.parse(
  readFileSync(new URL("package.json", import.meta.url), "utf8"),
);

test("dependencies are reproducible", () => {
  for (const [name, version] of Object.entries({
    ...packageJson.dependencies,
    ...packageJson.devDependencies,
  })) {
    assert.match(version, /^\d+\.\d+\.\d+$/, `${name} must use an exact version`);
  }

  assert.ok(
    existsSync(new URL("pnpm-lock.yaml", import.meta.url)),
    "pnpm-lock.yaml must be committed",
  );
});
