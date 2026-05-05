// Decision: route metadata is hand-curated, so verify it against markdown
// sources to catch docs that exist on disk but are invisible on the site.
import { existsSync, readdirSync, readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const scriptDir = path.dirname(fileURLToPath(import.meta.url));
const siteRoot = path.resolve(scriptDir, "..");
const repoRoot = path.resolve(siteRoot, "..");
const metaPath = path.join(siteRoot, "src/pages/docs/_meta.ts");
const publicDocsDir = path.join(repoRoot, "docs");
const rustDocsDir = path.join(repoRoot, "crates/bashkit/docs");

const metaSource = readFileSync(metaPath, "utf8");
const objectPattern = /\{\s*slug:\s*"([^"]+)"[\s\S]*?\}/g;

const entries = [];
const slugs = new Set();
let match;

while ((match = objectPattern.exec(metaSource)) !== null) {
  const block = match[0];
  const slug = match[1];
  const collection = extractString(block, "collection") ?? "docs";
  const sourceId = extractString(block, "sourceId") ?? slug;
  const seoTitle = extractString(block, "seoTitle");
  const seoDescription = extractString(block, "seoDescription");

  if (slugs.has(slug)) {
    throw new Error(`Duplicate docs slug in _meta.ts: ${slug}`);
  }

  if (!seoTitle) {
    throw new Error(`/docs/${slug} is missing seoTitle`);
  }

  if (!seoDescription) {
    throw new Error(`/docs/${slug} is missing seoDescription`);
  }

  if (seoTitle.length < 30 || seoTitle.length > 65) {
    throw new Error(
      `/docs/${slug} seoTitle should be 30-65 characters, got ${seoTitle.length}`,
    );
  }

  if (seoDescription.length < 70 || seoDescription.length > 160) {
    throw new Error(
      `/docs/${slug} seoDescription should be 70-160 characters, got ${seoDescription.length}`,
    );
  }

  slugs.add(slug);
  entries.push({ slug, collection, sourceId });
}

const publicDocIds = readdirSync(publicDocsDir)
  .filter((name) => name.endsWith(".md"))
  .map((name) => name.replace(/\.md$/, ""));

for (const id of publicDocIds) {
  const hasRoute = entries.some(
    (entry) => entry.collection === "docs" && entry.sourceId === id,
  );

  if (!hasRoute) {
    throw new Error(`docs/${id}.md is missing from docs route metadata`);
  }
}

for (const entry of entries) {
  const sourceDir = entry.collection === "rustdocs" ? rustDocsDir : publicDocsDir;
  const sourcePath = path.join(sourceDir, `${entry.sourceId}.md`);

  if (!existsSync(sourcePath)) {
    throw new Error(
      `/docs/${entry.slug} points at missing ${entry.collection}/${entry.sourceId}.md`,
    );
  }
}

console.log(`Verified ${entries.length} docs route metadata entries.`);

function extractString(block, key) {
  const fieldPattern = new RegExp(`${key}:\\s*"([^"]+)"`);
  return fieldPattern.exec(block)?.[1];
}
