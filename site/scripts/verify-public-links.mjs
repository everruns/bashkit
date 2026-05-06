// Decision: generated public HTML must not link to repo-internal markdown paths.
// Internal docs/specs files are valid in GitHub, but bashkit.sh does not serve
// raw .md routes, so local markdown hrefs become crawler-visible 404s.
import { readdirSync, readFileSync, statSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const SITE_URL = "https://bashkit.sh";
const scriptDir = path.dirname(fileURLToPath(import.meta.url));
const siteRoot = path.resolve(scriptDir, "..");
const distRoot = path.join(siteRoot, "dist");
const localMarkdownLinks = [];

collectHtmlFiles(distRoot);

if (localMarkdownLinks.length > 0) {
  const details = localMarkdownLinks
    .map(({ filePath, href }) => `${path.relative(distRoot, filePath)} -> ${href}`)
    .join("\n");
  throw new Error(`Generated HTML contains local markdown links:\n${details}`);
}

console.log("Verified generated HTML has no local markdown links.");

function collectHtmlFiles(dir) {
  for (const name of readdirSync(dir)) {
    const filePath = path.join(dir, name);
    const stats = statSync(filePath);

    if (stats.isDirectory()) {
      collectHtmlFiles(filePath);
      continue;
    }

    if (!name.endsWith(".html")) {
      continue;
    }

    const html = readFileSync(filePath, "utf8");
    for (const href of html.matchAll(/\shref="([^"]+\.md(?:#[^"]*)?)"/g)) {
      if (isLocalMarkdownHref(href[1])) {
        localMarkdownLinks.push({ filePath, href: href[1] });
      }
    }
  }
}

function isLocalMarkdownHref(href) {
  if (href.startsWith(`${SITE_URL}/`)) {
    return true;
  }

  return (
    !href.startsWith("http://") &&
    !href.startsWith("https://") &&
    !href.startsWith("mailto:") &&
    !href.startsWith("#")
  );
}
