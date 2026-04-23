// Hand-curated metadata for each doc so the /docs index can group and
// summarise pages without requiring frontmatter in the canonical markdown.
// Order here drives navigation order (index cards, prev/next).

export type DocMeta = {
  slug: string;
  title: string;
  summary: string;
  section: string;
};

export const DOC_META: DocMeta[] = [
  {
    slug: "cli",
    title: "CLI",
    summary: "Run scripts with bashkit-cli: flags, exit codes, opt-in runtimes.",
    section: "Getting started",
  },
  {
    slug: "security",
    title: "Security",
    summary: "Sandbox boundaries, threat model, and what scripts cannot do.",
    section: "Operations",
  },
  {
    slug: "snapshotting",
    title: "Snapshotting",
    summary: "Serialize interpreter state and restore it for checkpoint/resume flows.",
    section: "Features",
  },
  {
    slug: "builtin_typescript",
    title: "TypeScript builtin",
    summary: "Embedded ZapCode TypeScript runtime shared with bash in-memory.",
    section: "Features",
  },
];

export const DOC_META_BY_SLUG: Record<string, DocMeta> = Object.fromEntries(
  DOC_META.map((doc) => [doc.slug, doc]),
);
