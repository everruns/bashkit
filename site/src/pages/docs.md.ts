// Decision: /docs has a Markdown representation for clients that prefer
// text/markdown. Keep it generated from the same curated metadata as HTML nav.
import type { APIRoute } from "astro";
import { DOC_META } from "./docs/_meta";

export const prerender = true;

const sectionOrder = [
  "Getting started",
  "Reference",
  "Features",
  "Runtimes",
  "Extending",
  "Operations",
];

export const GET: APIRoute = () => {
  const markdown = [
    "---",
    'title: "Bashkit docs"',
    'description: "User-facing guides for the bashkit CLI, security model, embedded runtimes, and extension APIs."',
    "---",
    "",
    "# Bashkit docs",
    "",
    "User-facing guides for the bashkit CLI, security model, embedded runtimes, and extension APIs.",
    "",
    ...sectionOrder.flatMap((section) => {
      const docs = DOC_META.filter((doc) => doc.section === section);
      if (docs.length === 0) {
        return [];
      }

      return [
        `## ${section}`,
        "",
        ...docs.map((doc) => `- [${doc.title}](/docs/${doc.slug}/) - ${doc.summary}`),
        "",
      ];
    }),
    "---",
    "",
    "For a curated, machine-readable index of the whole site, see [llms.txt](/llms.txt) (full text: [llms-full.txt](/llms-full.txt)).",
    "",
  ].join("\n");

  return new Response(markdown, {
    headers: {
      "Content-Type": "text/markdown; charset=utf-8",
    },
  });
};
