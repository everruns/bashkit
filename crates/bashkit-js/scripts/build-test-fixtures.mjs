// Build test fixtures from the package root so release artifact jobs can keep `npm test` Rust-free.

import { spawnSync } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";

const packageRoot = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  "..",
);
const fixtureDir = path.join(packageRoot, "test-fixtures", "random-fs");
const bin = process.platform === "win32" ? "napi.cmd" : "napi";
const napi = path.join(packageRoot, "node_modules", ".bin", bin);

const result = spawnSync(napi, ["build", "--platform", "--release"], {
  cwd: fixtureDir,
  stdio: "inherit",
});

process.exit(result.status ?? 1);
