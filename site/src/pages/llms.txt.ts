// Decision: publish /llms.txt (llmstxt.org) so coding agents have a single,
// curated entry point that links straight to the Markdown representations of
// every page. Generated from the same content tables as the HTML/Markdown nav
// so the agent index can't drift from the site.
import type { APIRoute } from "astro";
import {
  agentSteps,
  homeHero,
  languages,
  resources,
} from "../content/home";
import { DOC_META } from "./docs/_meta";

export const prerender = true;

const SITE = "https://bashkit.sh";

const sectionOrder = [
  "Getting started",
  "Reference",
  "Features",
  "Runtimes",
  "Extending",
  "Operations",
];

export const GET: APIRoute = () => {
  const docSections = sectionOrder.flatMap((section) => {
    const docs = DOC_META.filter((doc) => doc.section === section);
    if (docs.length === 0) {
      return [];
    }

    return [
      `## ${section}`,
      "",
      ...docs.map(
        (doc) => `- [${doc.title}](${SITE}/docs/${doc.slug}.md): ${doc.summary}`,
      ),
      "",
    ];
  });

  const text = [
    "# Bashkit",
    "",
    `> ${homeHero.description}`,
    "",
    "Doc links below point at the Markdown representation of each page so it can",
    "be fetched and read directly; links under Optional may be HTML pages or",
    "external resources. For the full text of every guide inlined into one",
    "document, fetch /llms-full.txt.",
    "",
    "## Quickstarts",
    "",
    ...languages.map(
      (lang) => `- [${lang.eyebrow}: ${lang.title}](${SITE}/index.md): \`${lang.install}\``,
    ),
    "",
    "## Agent skill",
    "",
    ...agentSteps.map((step) =>
      step.command
        ? `- ${step.title}: ${step.detail} (\`${step.command}\`)`
        : `- ${step.title}: ${step.detail}`,
    ),
    `- [Agent Skills discovery index](${SITE}/.well-known/agent-skills/index.json): machine-readable skill manifest for coding agents.`,
    "",
    ...docSections,
    "## Optional",
    "",
    `- [All docs (Markdown index)](${SITE}/docs.md): grouped index of every guide.`,
    `- [Full text](${SITE}/llms-full.txt): every guide inlined into one document.`,
    `- [Builtins](${SITE}/builtins): the full sandboxed command surface.`,
    ...resources.map(
      (item) =>
        `- [${item.title}](${item.href.startsWith("/") ? SITE + item.href : item.href}): ${item.detail}`,
    ),
    "",
  ].join("\n");

  return new Response(text, {
    headers: {
      "Content-Type": "text/plain; charset=utf-8",
    },
  });
};
