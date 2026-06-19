// Decision: publish /llms-full.txt (llmstxt.org) so an agent can pull the
// entire documentation set in a single fetch. The body of each guide is the
// canonical Markdown source file — the same text served at /docs/<slug>.md —
// concatenated in navigation order so it stays in sync with the site.
import { readFile } from "node:fs/promises";
import path from "node:path";
import type { APIRoute } from "astro";
import { homeHero } from "../content/home";
import { DOC_META, type DocMeta } from "./docs/_meta";

export const prerender = true;

const SITE = "https://bashkit.sh";

const siteRoot = process.cwd();
const repoRoot = path.resolve(siteRoot, "..");
const sourceDirs: Record<"docs" | "rustdocs", string> = {
  docs: path.join(repoRoot, "docs"),
  rustdocs: path.join(repoRoot, "crates/bashkit/docs"),
};

function sourcePath(meta: DocMeta): string {
  const collection = meta.collection ?? "docs";
  const sourceId = meta.sourceId ?? meta.slug;
  return path.join(sourceDirs[collection], `${sourceId}.md`);
}

function stripFrontmatter(markdown: string): string {
  const match = markdown.match(/^---\r?\n[\s\S]*?\r?\n---\r?\n/);
  return match ? markdown.slice(match[0].length) : markdown;
}

export const GET: APIRoute = async () => {
  const guides = await Promise.all(
    DOC_META.map(async (meta) => {
      const source = await readFile(sourcePath(meta), "utf8");
      return [
        `# ${meta.title}`,
        "",
        `Source: ${SITE}/docs/${meta.slug}.md`,
        "",
        stripFrontmatter(source).trim(),
      ].join("\n");
    }),
  );

  const text = [
    "# Bashkit — full documentation",
    "",
    `> ${homeHero.description}`,
    "",
    "This file inlines every published guide in navigation order. The canonical",
    "Markdown for each guide is also available individually at /docs/<slug>.md.",
    "",
    guides.join("\n\n---\n\n"),
    "",
  ].join("\n");

  return new Response(text, {
    headers: {
      "Content-Type": "text/plain; charset=utf-8",
    },
  });
};
