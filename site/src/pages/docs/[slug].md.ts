// Decision: docs Markdown responses are the canonical source files, not a
// lossy HTML-to-Markdown conversion. This keeps agents and humans reading the
// same maintained guide text.
import { readFile } from "node:fs/promises";
import path from "node:path";
import type { APIRoute, GetStaticPaths } from "astro";
import { DOC_META, type DocMeta } from "./_meta";

type Collection = "docs" | "rustdocs";

type MarkdownRouteProps = {
  meta: DocMeta;
  previous?: DocMeta;
  next?: DocMeta;
  sourcePath: string;
};

const siteRoot = process.cwd();
const repoRoot = path.resolve(siteRoot, "..");
const sourceDirs: Record<Collection, string> = {
  docs: path.join(repoRoot, "docs"),
  rustdocs: path.join(repoRoot, "crates/bashkit/docs"),
};

export const prerender = true;

export const getStaticPaths: GetStaticPaths = () =>
  DOC_META.map((meta, index) => {
    const collection = meta.collection ?? "docs";
    const sourceId = meta.sourceId ?? meta.slug;

    return {
      params: { slug: meta.slug },
      props: {
        meta,
        previous: DOC_META[index - 1],
        next: DOC_META[index + 1],
        sourcePath: path.join(sourceDirs[collection], `${sourceId}.md`),
      } satisfies MarkdownRouteProps,
    };
  });

export const GET: APIRoute<MarkdownRouteProps> = async ({ props }) => {
  const source = await readFile(props.sourcePath, "utf8");
  const markdown = renderMarkdownResponse(source, props.meta, props.previous, props.next);

  return new Response(markdown, {
    headers: {
      "Content-Type": "text/markdown; charset=utf-8",
    },
  });
};

function renderMarkdownResponse(
  source: string,
  meta: DocMeta,
  previous?: DocMeta,
  next?: DocMeta,
): string {
  const parts = [];
  const body = source.trimEnd();

  if (!hasYamlFrontmatter(body)) {
    parts.push(renderFrontmatter(meta));
  }

  parts.push(body, renderNavigation(previous, next));
  return `${parts.join("\n\n")}\n`;
}

function hasYamlFrontmatter(markdown: string): boolean {
  return /^---\r?\n/.test(markdown);
}

function renderFrontmatter(meta: DocMeta): string {
  return [
    "---",
    `title: ${yamlString(meta.title)}`,
    `description: ${yamlString(meta.seoDescription ?? meta.summary)}`,
    `section: ${yamlString(meta.section)}`,
    "---",
  ].join("\n");
}

function renderNavigation(previous?: DocMeta, next?: DocMeta): string {
  const links = ["- [All docs](/docs/)"];

  if (previous) {
    links.push(`- [Previous: ${previous.title}](/docs/${previous.slug}/)`);
  }

  if (next) {
    links.push(`- [Next: ${next.title}](/docs/${next.slug}/)`);
  }

  links.push("- [llms.txt](/llms.txt) - curated index of this site for agents");

  return ["---", "", "## Navigation", "", ...links].join("\n");
}

function yamlString(value: string): string {
  return JSON.stringify(value);
}
