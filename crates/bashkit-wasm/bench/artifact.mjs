// Transfer bytes, not raw bytes alone, determine cold-start network cost.
import { readFileSync, existsSync, realpathSync } from "node:fs";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { createHash } from "node:crypto";
import { gzipSync, brotliCompressSync, constants } from "node:zlib";

export function artifact(pkg) {
  const files = ["index.js", "bashkit_wasm.js", "bashkit_wasm_bg.wasm"].map(name => {
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
// Node canonicalizes the module URL, but argv can retain a symlink (e.g. /var
// versus /private/var on macOS). Imports must never run the CLI accidentally.
if (process.argv[1] && existsSync(process.argv[1]) && realpathSync(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const paths = process.argv.slice(2).filter(arg => arg !== "--check");
  if (paths.length > 1) throw new Error("expected one package directory");
  const pkg = resolve(paths[0] ?? new URL("../pkg/", import.meta.url).pathname);
  const report = artifact(pkg);
  console.log(JSON.stringify(report, null, 2));
  if (process.argv.includes("--check")) {
    const budgets = JSON.parse(readFileSync(new URL("./size-budget.json", import.meta.url)));
    for (const [kind, budget] of Object.entries(budgets)) {
      if (report.total[kind] > budget) throw new Error(`${kind}: ${report.total[kind]} exceeds ${budget} byte budget`);
    }
  }
}
