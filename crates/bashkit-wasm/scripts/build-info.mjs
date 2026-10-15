// Publish the exact source/build identity with the bundle; version alone stays
// unchanged between releases and cannot identify a source-tree build.
import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { readFileSync, writeFileSync, realpathSync } from "node:fs";
import { resolve } from "node:path";
const [pkg, profile, features, wasmOpt] = process.argv.slice(2);
const workspace = realpathSync(new URL("../../../", import.meta.url));
const command = (name, args) => execFileSync(name, args, {
  cwd: workspace, encoding: "utf8", stdio: ["ignore", "pipe", "pipe"],
}).trim();
let sourceRevision = null;
let sourceDirty = null;
try {
  if (realpathSync(command("git", ["rev-parse", "--show-toplevel"])) !== workspace) {
    throw new Error("source archive inside another repository");
  }
  sourceRevision = command("git", ["rev-parse", "HEAD"]);
  sourceDirty = command("git", ["status", "--porcelain"]).length > 0;
} catch {
  // Source archives have no Git identity. Record absence instead of guessing
  // from the package version; CI/release checkouts always have both fields.
}
const info = {
  version: JSON.parse(readFileSync(resolve(pkg, "package.json"))).version,
  sourceRevision,
  sourceDirty,
  profile,
  features: features.split(",").filter(Boolean).sort(),
  rustc: command("rustc", ["--version"]),
  wasmBindgen: command("wasm-bindgen", ["--version"]),
  wasmOpt,
  wasmSha256: createHash("sha256").update(readFileSync(resolve(pkg, "bashkit_wasm_bg.wasm"))).digest("hex"),
};
writeFileSync(resolve(pkg, "build-info.json"), JSON.stringify(info, null, 2) + "\n");
