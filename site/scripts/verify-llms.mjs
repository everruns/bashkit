// Decision: verify the generated llms.txt / llms-full.txt from build output so
// the coding-agent entry points survive future route or content changes.
// Doc metadata is parsed from _meta.ts source text to match verify-doc-routes.
import { existsSync, readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const scriptDir = path.dirname(fileURLToPath(import.meta.url));
const siteRoot = path.resolve(scriptDir, "..");
const distRoot = path.join(siteRoot, "dist");
const metaPath = path.join(siteRoot, "src/pages/docs/_meta.ts");

const llmsPath = path.join(distRoot, "llms.txt");
const llmsFullPath = path.join(distRoot, "llms-full.txt");

for (const filePath of [llmsPath, llmsFullPath]) {
  if (!existsSync(filePath)) {
    throw new Error(`Missing llms output: ${filePath}`);
  }
}

const llms = readFileSync(llmsPath, "utf8");
const llmsFull = readFileSync(llmsFullPath, "utf8");

// llms.txt must follow the llmstxt.org shape: an H1 title and a blockquote.
if (!/^# Bashkit\n/.test(llms)) {
  throw new Error("llms.txt must start with an `# Bashkit` H1.");
}
if (!/\n> /.test(llms)) {
  throw new Error("llms.txt must include a `>` summary blockquote.");
}
if (!llms.includes("https://bashkit.sh/llms-full.txt")) {
  throw new Error("llms.txt must link to llms-full.txt.");
}

// Every doc must be discoverable from both entry points.
const metaSource = readFileSync(metaPath, "utf8");
// Match each doc object by slug only, then pull title from the block — field
// order inside the object must not affect coverage (mirrors verify-doc-routes).
const objectPattern = /\{\s*slug:\s*"([^"]+)"[\s\S]*?\}/g;

function extractString(block, key) {
  return new RegExp(`${key}:\\s*"([^"]+)"`).exec(block)?.[1];
}

let count = 0;
let match;
while ((match = objectPattern.exec(metaSource)) !== null) {
  const block = match[0];
  const slug = match[1];
  const title = extractString(block, "title");
  const mdLink = `https://bashkit.sh/docs/${slug}.md`;

  if (!llms.includes(mdLink)) {
    throw new Error(`llms.txt missing doc link: ${mdLink}`);
  }
  if (!llmsFull.includes(`# ${title}\n`)) {
    throw new Error(`llms-full.txt missing inlined guide: ${title}`);
  }
  count += 1;
}

if (count === 0) {
  throw new Error("No docs parsed from _meta.ts; verify-llms is misconfigured.");
}

console.log(`Verified llms.txt and llms-full.txt (${count} guides indexed).`);
