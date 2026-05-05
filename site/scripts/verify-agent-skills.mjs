// Keep the published discovery index tied to the exact archived skill bytes.
import { createHash } from "node:crypto";
import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { gunzipSync } from "node:zlib";

const distDir = new URL("../dist/", import.meta.url);
const indexPath = new URL(".well-known/agent-skills/index.json", distDir);
const schema = "https://schemas.agentskills.io/discovery/0.2.0/schema.json";

function fail(message) {
  console.error(message);
  process.exit(1);
}

if (!existsSync(indexPath)) {
  fail("Agent Skills discovery index missing from dist.");
}

let index;
try {
  index = JSON.parse(readFileSync(indexPath, "utf8"));
} catch (error) {
  fail(`Agent Skills discovery index is not valid JSON: ${error.message}`);
}

if (index.$schema !== schema) {
  fail(`Agent Skills discovery index has wrong $schema: ${index.$schema}`);
}

if (!Array.isArray(index.skills) || index.skills.length === 0) {
  fail("Agent Skills discovery index must contain a non-empty skills array.");
}

for (const skill of index.skills) {
  for (const key of ["name", "type", "description", "url", "digest"]) {
    if (typeof skill[key] !== "string" || skill[key].length === 0) {
      fail(`Agent Skills entry is missing string field: ${key}`);
    }
  }

  if (!/^[a-z0-9]+(?:-[a-z0-9]+)*$/.test(skill.name)) {
    fail(`Agent Skills entry has invalid name: ${skill.name}`);
  }

  if (!["skill-md", "archive"].includes(skill.type)) {
    fail(`Agent Skills entry has invalid type: ${skill.type}`);
  }

  if (!/^sha256:[a-f0-9]{64}$/.test(skill.digest)) {
    fail(`Agent Skills entry has invalid digest: ${skill.digest}`);
  }

  if (!skill.url.startsWith("/.well-known/agent-skills/")) {
    fail(`Agent Skills entry should use the well-known path: ${skill.url}`);
  }

  const artifactPath = join(distDir.pathname, skill.url);
  if (!existsSync(artifactPath)) {
    fail(`Agent Skills artifact missing from dist: ${skill.url}`);
  }

  const artifact = readFileSync(artifactPath);
  const digest = `sha256:${createHash("sha256").update(artifact).digest("hex")}`;
  if (digest !== skill.digest) {
    fail(`Agent Skills digest mismatch for ${skill.name}: ${digest}`);
  }

  if (skill.type === "archive") {
    const tar = gunzipSync(artifact);
    if (!tarIncludesPath(tar, "SKILL.md")) {
      fail(`Agent Skills archive for ${skill.name} does not contain SKILL.md at root.`);
    }
  }
}

console.log(`Verified ${index.skills.length} Agent Skills discovery entr${index.skills.length === 1 ? "y" : "ies"}.`);

function tarIncludesPath(buffer, expectedPath) {
  for (let offset = 0; offset + 512 <= buffer.length; offset += 512) {
    const header = buffer.subarray(offset, offset + 512);
    if (header.every((byte) => byte === 0)) {
      return false;
    }

    const name = readTarString(header, 0, 100);
    const prefix = readTarString(header, 345, 155);
    const fullName = prefix ? `${prefix}/${name}` : name;
    const sizeOctal = readTarString(header, 124, 12).trim();
    const size = Number.parseInt(sizeOctal || "0", 8);

    if (fullName === expectedPath) {
      return true;
    }

    offset += Math.ceil(size / 512) * 512;
  }

  return false;
}

function readTarString(buffer, start, length) {
  const raw = buffer.subarray(start, start + length);
  const end = raw.indexOf(0);
  return raw.subarray(0, end === -1 ? raw.length : end).toString("utf8");
}
