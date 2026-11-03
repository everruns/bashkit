// Exercise the archive's public contract, not just an unpacked build directory.
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { createHash } from "node:crypto";
import { execFileSync } from "node:child_process";
import { Terminal } from "../pkg/index.js";
const pkg = new URL("../pkg/", import.meta.url);
const read = name => readFileSync(new URL(name, pkg));

test("package records verifiable source and artifact identity", () => {
  const info = JSON.parse(read("build-info.json"));
  if (info.sourceRevision === null) {
    assert.equal(info.sourceDirty, null);
    assert.notEqual(process.env.GITHUB_ACTIONS, "true", "CI/release must record Git identity");
  } else {
    assert.match(info.sourceRevision, /^[0-9a-f]{40}$/);
    assert.equal(typeof info.sourceDirty, "boolean");
    assert.equal(info.sourceRevision, execFileSync("git", ["rev-parse", "HEAD"], { encoding: "utf8" }).trim());
  }
  assert.equal(info.version, JSON.parse(read("package.json")).version);
  assert.equal(info.wasmSha256, createHash("sha256").update(read("bashkit_wasm_bg.wasm")).digest("hex"));
  assert.equal(info.profile, "wasm-release");
  assert.equal(info.features.includes("terminal"), Terminal !== undefined);
});

test("npm archive contains every runtime file and build identity", () => {
  const [archive] = JSON.parse(execFileSync("npm", ["pack", "--dry-run", "--json", "--ignore-scripts"], {
    cwd: pkg, encoding: "utf8", stdio: ["ignore", "pipe", "pipe"], timeout: 30000,
  }));
  const files = new Set(archive.files.map(file => file.path));
  for (const name of ["index.js", "index.d.ts", "bashkit_wasm.js", "bashkit_wasm.d.ts", "bashkit_wasm_bg.wasm", "build-info.json"]) assert.ok(files.has(name), `archive lacks ${name}`);
});
