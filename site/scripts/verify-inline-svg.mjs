// Decision: CommonMark ends raw HTML blocks at blank lines, so multiline SVG
// diagrams must stay contiguous or their remaining elements render as code.
import { readFileSync, readdirSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const siteDir = path.dirname(path.dirname(fileURLToPath(import.meta.url)));
const repositoryDir = path.dirname(siteDir);
const docsDirs = [
  path.join(repositoryDir, "docs"),
  path.join(repositoryDir, "crates", "bashkit", "docs"),
];
const failures = [];

for (const docsDir of docsDirs) {
  for (const entry of readdirSync(docsDir, { withFileTypes: true })) {
    if (!entry.isFile() || !entry.name.endsWith(".md")) {
      continue;
    }

    const filePath = path.join(docsDir, entry.name);
    const markdown = readFileSync(filePath, "utf8");
    for (const match of markdown.matchAll(/<svg\b[\s\S]*?<\/svg>/gi)) {
      const blankLine = /\r?\n[\t ]*\r?\n/.exec(match[0]);
      if (!blankLine) {
        continue;
      }

      const offset = match.index + blankLine.index;
      const line = markdown.slice(0, offset).split(/\r?\n/).length + 1;
      failures.push(`${path.relative(repositoryDir, filePath)}:${line}`);
    }
  }
}

if (failures.length > 0) {
  throw new Error(
    `Inline SVG blocks contain blank lines and will be split by CommonMark:\n${failures
      .map((failure) => `- ${failure}`)
      .join("\n")}`,
  );
}

console.log("Verified inline SVG blocks are CommonMark-safe.");
