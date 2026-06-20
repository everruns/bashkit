// Registry for the self-hosted package API references — the docs.rs analog for
// the PyPI and npm packages. Rust keeps using docs.rs (external); Python and
// TypeScript render generated markdown from the `apidocs` content collection
// through the same branded DocsLayout. See specs/documentation.md.

export type ApiRef = {
  // Matches the `apidocs` collection entry id and the /api/<slug> route.
  slug: string;
  title: string;
  language: string;
  summary: string;
  packageName: string;
  packageUrl: string;
  registry: string;
  // Script that regenerates the markdown (provenance). Optional until a
  // language's generator lands.
  generator?: string;
  seoTitle: string;
  seoDescription: string;
};

// External reference (Rust → docs.rs). Listed on the index, no local page.
export type ExternalApiRef = {
  language: string;
  title: string;
  summary: string;
  packageName: string;
  registry: string;
  href: string;
};

export const EXTERNAL_API_REFS: ExternalApiRef[] = [
  {
    language: "Rust",
    title: "Rust API",
    summary: "Full rustdoc for the bashkit crate, hosted on docs.rs.",
    packageName: "bashkit",
    registry: "crates.io",
    href: "https://docs.rs/bashkit",
  },
];

export const API_REFS: ApiRef[] = [
  {
    slug: "python",
    title: "Python API reference",
    language: "Python",
    summary: "Classes and functions exported from the bashkit PyPI package.",
    packageName: "bashkit",
    packageUrl: "https://pypi.org/project/bashkit/",
    registry: "PyPI",
    generator: "scripts/gen_python_apidocs.py",
    seoTitle: "Bashkit Python API reference",
    seoDescription:
      "API reference for the bashkit PyPI package: Bash, BashTool, ScriptedTool, FileSystem, ExecResult, and framework integrations.",
  },
  {
    slug: "typescript",
    title: "TypeScript API reference",
    language: "TypeScript",
    summary: "Types and functions exported from the @everruns/bashkit npm package.",
    packageName: "@everruns/bashkit",
    packageUrl: "https://www.npmjs.com/package/@everruns/bashkit",
    registry: "npm",
    seoTitle: "Bashkit TypeScript API reference",
    seoDescription:
      "API reference for the @everruns/bashkit npm package: Bash, BashTool, ScriptedTool, FileSystem, and framework integrations.",
  },
];

export const API_REFS_BY_SLUG: Record<string, ApiRef> = Object.fromEntries(
  API_REFS.map((ref) => [ref.slug, ref]),
);
