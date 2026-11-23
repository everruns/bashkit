// Transfer bytes, not raw bytes alone, determine cold-start network cost.
import { readFileSync, existsSync, realpathSync } from "node:fs";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { createHash } from "node:crypto";
import { gzipSync, brotliCompressSync, constants } from "node:zlib";

export function artifact(pkg) {
  const files = ["index.js", "runtime-functions.js", "bashkit_wasm.js", "bashkit_wasm_bg.wasm"].filter(name => existsSync(resolve(pkg, name))).map(name => {
    const bytes = readFileSync(resolve(pkg, name));
    return { name, sha256: createHash("sha256").update(bytes).digest("hex"), raw: bytes.length,
      gzip: gzipSync(bytes, { level: 9 }).length,
      brotli: brotliCompressSync(bytes, { params: { [constants.BROTLI_PARAM_QUALITY]: 11 } }).length };
  });
  const bytes = readFileSync(resolve(pkg, "bashkit_wasm_bg.wasm"));
  let offset = 8;
  function leb() { let n = 0, shift = 0, b; do { b = bytes[offset++]; n += (b & 127) * 2 ** shift; shift += 7; } while (b & 128); return n; }
  const sections = [];
  while (offset < bytes.length) {
    const id = bytes[offset++], length = leb(), start = offset;
    let name = ({ 10: "code", 11: "data" })[id] ?? `section-${id}`;
    if (id === 0) { const n = leb(); name = `custom:${bytes.subarray(offset, offset + n).toString()}`; }
    sections.push({ name, raw: length });
    offset = start + length;
  }
  return { files, total: Object.fromEntries(["raw", "gzip", "brotli"].map(k => [k, files.reduce((n, f) => n + f[k], 0)])), sections };
}

// The optional compiler has its own transfer allowance. The default published
// package keeps its original caps; identity comes from build.sh's metadata.
export function artifactBudget(pkg) {
  const base = JSON.parse(readFileSync(new URL("./size-budget.json", import.meta.url)));
  const metadata = resolve(pkg, "build-info.json");
  if (!existsSync(metadata)) return { profile: "default", limits: base };
  const { features } = JSON.parse(readFileSync(metadata));
  if (!Array.isArray(features) || features.some(feature => typeof feature !== "string")) {
    throw new Error("invalid build-info features");
  }
  const compiler = features.includes("experimental_execution_plan") || features.includes("bashkit/experimental_execution_plan");
  return compiler
    ? { profile: "execution-plan", limits: { ...base, ...JSON.parse(readFileSync(new URL("./size-budget-execution-plan.json", import.meta.url))) } }
    : { profile: "default", limits: base };
}

export function checkArtifactBudget(report, budget) {
  for (const [kind, limit] of Object.entries(budget.limits)) {
    if (report.total[kind] > limit) throw new Error(`${kind}: ${report.total[kind]} exceeds ${limit} byte budget (${budget.profile})`);
  }
}
// Node canonicalizes the module URL, but argv can retain a symlink (e.g. /var
// versus /private/var on macOS). Imports must never run the CLI accidentally.
if (process.argv[1] && existsSync(process.argv[1]) && realpathSync(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const paths = process.argv.slice(2).filter(arg => arg !== "--check");
  if (paths.length > 1) throw new Error("expected one package directory");
  const pkg = resolve(paths[0] ?? new URL("../pkg/", import.meta.url).pathname);
  const report = artifact(pkg);
  const budget = process.argv.includes("--check") ? artifactBudget(pkg) : null;
  if (budget) report.budgetProfile = budget.profile;
  console.log(JSON.stringify(report, null, 2));
  if (budget) checkArtifactBudget(report, budget);
}
