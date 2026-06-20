import { defineConfig } from "astro/config";
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
  ["credential-injection.md", "/docs/credential-injection/"],
  ["custom_builtins.md", "/docs/custom-builtins/"],
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
  ["structured-data.md", "/docs/structured-data/"],
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
        const target = DOC_LINKS.get(node.url.split("/").pop());
        if (target) {
          node.url = target;
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
    remarkPlugins: [normalizeGuideMarkdown],
    rehypePlugins: [rewriteRenderedLinks],
  },
  integrations: [sitemap(), sitemapEnhance()],
});
