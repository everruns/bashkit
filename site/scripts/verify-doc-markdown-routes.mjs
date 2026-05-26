// Decision: public site pages with Markdown representations are verified from
// build output plus Worker helpers so static assets and negotiation cannot drift.
import { existsSync, readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const scriptDir = path.dirname(fileURLToPath(import.meta.url));
const siteRoot = path.resolve(scriptDir, "..");
const repoRoot = path.resolve(siteRoot, "..");
const metaPath = path.join(siteRoot, "src/pages/docs/_meta.ts");
const distDir = path.join(siteRoot, "dist");
const publicDocsDir = path.join(repoRoot, "docs");
const rustDocsDir = path.join(repoRoot, "crates/bashkit/docs");
const workerPath = path.join(siteRoot, "src/worker.js");

const entries = parseDocEntries(readFileSync(metaPath, "utf8"));

const homeMarkdownPath = path.join(distDir, "index.md");
if (!existsSync(homeMarkdownPath)) {
  throw new Error("Missing generated homepage Markdown route: dist/index.md");
}

const homeMarkdown = readFileSync(homeMarkdownPath, "utf8");
assertYamlFrontmatter(homeMarkdown, "dist/index.md");
for (const expectedLink of [
  "[GitHub repository](https://github.com/everruns/bashkit)",
  "[Bashkit agent skill](https://bashkit.sh/.well-known/agent-skills/index.json)",
  "[Everruns](https://everruns.com)",
  "[Docs](/docs/)",
  "[Builtins](/builtins)",
]) {
  if (!homeMarkdown.includes(expectedLink)) {
    throw new Error(`Homepage Markdown navigation missing: ${expectedLink}`);
  }
}

const docsIndexPath = path.join(distDir, "docs.md");
if (!existsSync(docsIndexPath)) {
  throw new Error("Missing generated docs Markdown index: dist/docs.md");
}

const docsIndex = readFileSync(docsIndexPath, "utf8");
assertYamlFrontmatter(docsIndex, "dist/docs.md");
for (const entry of entries) {
  const markdownPath = path.join(distDir, "docs", `${entry.slug}.md`);
  if (!existsSync(markdownPath)) {
    throw new Error(`Missing generated Markdown route: dist/docs/${entry.slug}.md`);
  }

  const sourceDir = entry.collection === "rustdocs" ? rustDocsDir : publicDocsDir;
  const sourcePath = path.join(sourceDir, `${entry.sourceId}.md`);
  const source = readFileSync(sourcePath, "utf8");
  const generated = readFileSync(markdownPath, "utf8");
  if (!generated.includes(source.trimEnd())) {
    throw new Error(`Generated Markdown route omits source body: /docs/${entry.slug}.md`);
  }

  if (!sourceHasYamlFrontmatter(source)) {
    assertYamlFrontmatter(generated, `dist/docs/${entry.slug}.md`);
    for (const expected of [
      `title: ${JSON.stringify(entry.title)}`,
      `description: ${JSON.stringify(entry.seoDescription ?? entry.summary)}`,
      `section: ${JSON.stringify(entry.section)}`,
    ]) {
      if (!generated.startsWith(`---\n`) || !generated.includes(`\n${expected}\n`)) {
        throw new Error(`Generated Markdown front matter missing ${expected}`);
      }
    }
  }

  for (const expectedLink of navigationLinksFor(entry, entries)) {
    if (!generated.includes(expectedLink)) {
      throw new Error(`/docs/${entry.slug}.md navigation missing: ${expectedLink}`);
    }
  }

  if (!docsIndex.includes(`(/docs/${entry.slug}/)`)) {
    throw new Error(`docs Markdown index does not link /docs/${entry.slug}/`);
  }
}

const { markdownAssetPath, prefersMarkdown } = await import(
  pathToFileURL(workerPath).href
);

const cases = [
  ["text/markdown", true],
  ["text/markdown, text/html;q=0.9", true],
  ["text/html, text/markdown;q=0.9", false],
  ["text/markdown;q=0", false],
  ["*/*", false],
  ["", false],
];

for (const [accept, expected] of cases) {
  const actual = prefersMarkdown(accept);
  if (actual !== expected) {
    throw new Error(
      `prefersMarkdown(${JSON.stringify(accept)}) returned ${actual}, expected ${expected}`,
    );
  }
}

const pathCases = [
  ["/", "/index.md"],
  ["/docs", "/docs.md"],
  ["/docs/", "/docs.md"],
  ["/docs/cli", "/docs/cli.md"],
  ["/docs/cli/", "/docs/cli.md"],
  ["/docs/cli.md", null],
  ["/builtins", null],
];

for (const [pathname, expected] of pathCases) {
  const actual = markdownAssetPath(pathname);
  if (actual !== expected) {
    throw new Error(
      `markdownAssetPath(${JSON.stringify(pathname)}) returned ${actual}, expected ${expected}`,
    );
  }
}

console.log(`Verified ${entries.length} docs Markdown routes and negotiation helpers.`);

function parseDocEntries(source) {
  const objectPattern = /\{\s*slug:\s*"([^"]+)"[\s\S]*?\}/g;
  const entries = [];
  let match;

  while ((match = objectPattern.exec(source)) !== null) {
    const block = match[0];
    const slug = match[1];
    entries.push({
      slug,
      title: extractString(block, "title"),
      summary: extractString(block, "summary"),
      section: extractString(block, "section"),
      seoDescription: extractString(block, "seoDescription"),
      collection: extractString(block, "collection") ?? "docs",
      sourceId: extractString(block, "sourceId") ?? slug,
    });
  }

  return entries;
}

function extractString(block, key) {
  const fieldPattern = new RegExp(`${key}:\\s*"([^"]+)"`);
  return fieldPattern.exec(block)?.[1];
}

function sourceHasYamlFrontmatter(source) {
  return /^---\r?\n/.test(source);
}

function assertYamlFrontmatter(markdown, label) {
  if (!/^---\n[\s\S]+?\n---\n/.test(markdown)) {
    throw new Error(`${label} is missing YAML front matter`);
  }
}

function navigationLinksFor(entry, allEntries) {
  const index = allEntries.indexOf(entry);
  const links = ["- [All docs](/docs/)"];
  const previous = allEntries[index - 1];
  const next = allEntries[index + 1];

  if (previous) {
    links.push(`- [Previous: ${previous.title}](/docs/${previous.slug}/)`);
  }

  if (next) {
    links.push(`- [Next: ${next.title}](/docs/${next.slug}/)`);
  }

  return links;
}
