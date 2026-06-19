// Decision: verify Cloudflare static asset headers from build output because
// bashkit.sh is static and homepage discovery must survive deploy packaging.
import { existsSync, readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const scriptDir = path.dirname(fileURLToPath(import.meta.url));
const siteRoot = path.resolve(scriptDir, "..");
const headersPath = path.join(siteRoot, "dist", "_headers");

if (!existsSync(headersPath)) {
  throw new Error(`Missing Cloudflare headers output: ${headersPath}`);
}

const blocks = parseHeaders(readFileSync(headersPath, "utf8"));
const homeHeaders = blocks.get("/");

if (!homeHeaders) {
  throw new Error("_headers must define a homepage rule for /");
}

const linkHeaders = homeHeaders.get("link") ?? [];
const linkValue = linkHeaders.join(", ");
const expectedLinks = [
  '<https://bashkit.sh/>; rel="canonical"',
  '</index.md>; rel="alternate"; type="text/markdown"',
  '</llms.txt>; rel="alternate"; type="text/plain"',
  '</.well-known/agent-skills/index.json>; rel="service-desc"; type="application/json"',
  '</docs/>; rel="service-doc"; type="text/html"',
  '</docs.md>; rel="service-doc"; type="text/markdown"',
];

for (const expectedLink of expectedLinks) {
  if (!linkValue.includes(expectedLink)) {
    throw new Error(`Homepage Link header missing: ${expectedLink}`);
  }
}

console.log("Verified homepage Link response headers.");

function parseHeaders(source) {
  const blocks = new Map();
  let currentPattern = null;

  for (const rawLine of source.split(/\r?\n/)) {
    const line = rawLine.trimEnd();

    if (line.trim().length === 0 || line.trimStart().startsWith("#")) {
      continue;
    }

    if (!/^\s/.test(line)) {
      currentPattern = line.trim();
      if (!blocks.has(currentPattern)) {
        blocks.set(currentPattern, new Map());
      }
      continue;
    }

    if (!currentPattern) {
      throw new Error(`Header declared before a path pattern: ${line}`);
    }

    const separator = line.indexOf(":");
    if (separator < 0) {
      throw new Error(`Header line must use "Name: value": ${line}`);
    }

    const name = line.slice(0, separator).trim().toLowerCase();
    const value = line.slice(separator + 1).trim();
    const headers = blocks.get(currentPattern);
    const values = headers.get(name) ?? [];
    values.push(value);
    headers.set(name, values);
  }

  return blocks;
}
