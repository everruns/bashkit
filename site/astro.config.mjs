import { defineConfig } from "astro/config";
import { unified } from "@astrojs/markdown-remark";
import rehypeSlug from "rehype-slug";
import rehypeAutolinkHeadings from "rehype-autolink-headings";
import sitemap from "@astrojs/sitemap";
import sitemapEnhance from "./integrations/sitemap-enhance.mjs";

// Decision: Rustdoc markdown is reused on the public site. Normalize rustdoc
// fence attributes and local guide links at render time instead of duplicating
// those guides under site-specific paths.
const DOC_LINKS = new Map([
  ["builtin_typescript.md", "/docs/builtin_typescript/"],
  ["clap-builtins.md", "/docs/clap-builtins/"],
  ["cli.md", "/docs/cli/"],
  ["compatibility.md", "/docs/compatibility/"],
  ["configuration.md", "/docs/configuration/"],
  ["credential-injection.md", "/docs/credential-injection/"],
  ["custom_builtins.md", "/docs/custom-builtins/"],
  ["custom_builtins_js.md", "/docs/custom-builtins-js/"],
  // embedding.md + targets.md were split into per-target quickstarts; keep the
  // old filenames mapped so any lingering markdown links still resolve.
  ["embedding.md", "/docs/start-rust/"],
  ["filesystem.md", "/docs/filesystem/"],
  ["git.md", "/docs/git/"],
  ["hooks.md", "/docs/hooks/"],
  ["jq.md", "/docs/jq/"],
  ["live_mounts.md", "/docs/live-mounts/"],
  ["live-mounts.md", "/docs/live-mounts/"],
  ["llm-tools.md", "/docs/llm-tools/"],
  ["logging.md", "/docs/logging/"],
  ["networking.md", "/docs/networking/"],
  ["python.md", "/docs/python/"],
  ["request-signing.md", "/docs/request-signing/"],
  ["scripted-tools.md", "/docs/scripted-tools/"],
  ["security.md", "/docs/security/"],
  ["snapshotting.md", "/docs/snapshotting/"],
  ["sqlite.md", "/docs/sqlite/"],
  ["ssh.md", "/docs/ssh/"],
  ["start.md", "/docs/start/"],
  ["start-rust.md", "/docs/start-rust/"],
  ["start-python.md", "/docs/start-python/"],
  ["start-node.md", "/docs/start-node/"],
  ["start-browser.md", "/docs/start-browser/"],
  ["start-pyodide.md", "/docs/start-pyodide/"],
  ["structured-data.md", "/docs/structured-data/"],
  ["targets.md", "/docs/start/"],
  ["threat-model.md", "/docs/security/"],
  ["typescript.md", "/docs/builtin_typescript/"],
]);

function normalizeGuideMarkdown() {
  return (tree) => {
    visit(tree, (node) => {
      if (node.type === "code" && typeof node.lang === "string") {
        node.lang = node.lang.split(",")[0];
      }

      if (node.type === "link" && typeof node.url === "string") {
        const hashIndex = node.url.indexOf("#");
        const urlPath = hashIndex >= 0 ? node.url.slice(0, hashIndex) : node.url;
        const hash = hashIndex >= 0 ? node.url.slice(hashIndex) : "";
        const target = DOC_LINKS.get(urlPath.split("/").pop());
        if (target) {
          node.url = `${target}${hash}`;
          return;
        }

        const repoTarget = repoUrl(node.url);
        if (repoTarget) {
          node.url = repoTarget;
        }
      }
    });
  };
}

function repoUrl(url) {
  const renderedDocsUrl = renderedDocsRepoUrl(url);
  if (renderedDocsUrl) {
    return renderedDocsUrl;
  }

  if (
    url.startsWith("http://") ||
    url.startsWith("https://") ||
    url.startsWith("/") ||
    url.startsWith("#")
  ) {
    return null;
  }

  const cleanUrl = url.replace(/^\.\.\//g, "").replace(/^\.\//, "");

  if (cleanUrl === "README.md" || cleanUrl === "SECURITY.md") {
    return `https://github.com/everruns/bashkit/blob/main/${cleanUrl}`;
  }

  const specsIndex = cleanUrl.indexOf("specs/");
  if (specsIndex >= 0) {
    return `https://github.com/everruns/bashkit/blob/main/${cleanUrl.slice(specsIndex)}`;
  }

  const examplesIndex = cleanUrl.indexOf("examples/");
  if (examplesIndex >= 0) {
    return `https://github.com/everruns/bashkit/blob/main/crates/bashkit/${cleanUrl.slice(examplesIndex)}`;
  }

  return null;
}

function renderedDocsRepoUrl(url) {
  const prefix = "/docs/";
  if (!url.startsWith(prefix)) {
    return null;
  }

  const docsPath = url.slice(prefix.length);
  if (docsPath === "README.md" || docsPath === "SECURITY.md") {
    return `https://github.com/everruns/bashkit/blob/main/${docsPath}`;
  }

  const specsIndex = docsPath.indexOf("specs/");
  if (specsIndex >= 0) {
    return `https://github.com/everruns/bashkit/blob/main/${docsPath.slice(specsIndex)}`;
  }

  const cratesDocsIndex = docsPath.indexOf("crates/bashkit/docs/");
  if (cratesDocsIndex >= 0) {
    const rustdocPath = docsPath.slice(cratesDocsIndex);
    return `https://github.com/everruns/bashkit/blob/main/${rustdocPath}`;
  }

  return null;
}

function rewriteRenderedLinks() {
  return (tree) => {
    visit(tree, (node) => {
      if (node.type !== "element" || node.tagName !== "a") {
        return;
      }

      const href = node.properties?.href;
      if (typeof href !== "string") {
        return;
      }

      const docTarget = DOC_LINKS.get(href.split("/").pop());
      if (docTarget) {
        node.properties.href = docTarget;
        return;
      }

      const repoTarget = repoUrl(href);
      if (repoTarget) {
        node.properties.href = repoTarget;
      }
    });
  };
}

function visit(node, visitor) {
  visitor(node);
  if (!Array.isArray(node.children)) {
    return;
  }
  for (const child of node.children) {
    visit(child, visitor);
  }
}

export default defineConfig({
  site: "https://bashkit.sh",
  output: "static",
  markdown: {
    shikiConfig: { theme: "github-light" },
    // Astro 7: remark/rehype plugins move under `processor: unified({...})`.
    // `shikiConfig` stays top-level (cross-cutting, processor-agnostic).
    processor: unified({
      remarkPlugins: [normalizeGuideMarkdown],
      // rehypeSlug assigns heading ids (github-slugger — same algorithm Astro
      // uses, so fragment links stay consistent); rehypeAutolinkHeadings then
      // appends a hover-revealed "#" link so any subsection is directly
      // linkable. Both run before rewriteRenderedLinks (whose "#id" hrefs it
      // leaves untouched).
      rehypePlugins: [
        rehypeSlug,
        [
          rehypeAutolinkHeadings,
          {
            behavior: "append",
            properties: {
              className: ["heading-anchor"],
              ariaLabel: "Link to this section",
            },
            content: { type: "text", value: "#" },
          },
        ],
        rewriteRenderedLinks,
      ],
    }),
  },
  integrations: [sitemap(), sitemapEnhance()],
});
