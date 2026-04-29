// Decision: verify sitemap coverage from generated HTML files so new pages are
// checked automatically without duplicating route lists in tests.
import { existsSync, readdirSync, readFileSync, statSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const SITE_URL = "https://bashkit.sh";
const scriptDir = path.dirname(fileURLToPath(import.meta.url));
const siteRoot = path.resolve(scriptDir, "..");
const distRoot = path.join(siteRoot, "dist");
const sitemapPath = path.join(distRoot, "sitemap.xml");
const sitemapIndexPath = path.join(distRoot, "sitemap-index.xml");
const intermediateSitemapPath = path.join(distRoot, "sitemap-0.xml");

if (!existsSync(sitemapPath)) {
  throw new Error(`Missing sitemap output: ${sitemapPath}`);
}

if (existsSync(intermediateSitemapPath)) {
  throw new Error(
    `Intermediate sitemap should be removed: ${intermediateSitemapPath}`,
  );
}

if (existsSync(sitemapIndexPath)) {
  throw new Error(`Sitemap index should not be generated: ${sitemapIndexPath}`);
}

const sitemap = readFileSync(sitemapPath, "utf8");

if (!sitemap.includes("<lastmod>")) {
  throw new Error("sitemap.xml does not include lastmod entries");
}

const htmlRoutes = [];

function collectHtmlRoutes(dir) {
  for (const name of readdirSync(dir)) {
    const filePath = path.join(dir, name);
    const stats = statSync(filePath);

    if (stats.isDirectory()) {
      collectHtmlRoutes(filePath);
      continue;
    }

    if (!name.endsWith(".html")) {
      continue;
    }

    const relativePath = path.relative(distRoot, filePath);
    const routePath =
      relativePath === "index.html"
        ? "/"
        : `/${relativePath.replace(/(?:\/index)?\.html$/, "")}`;
    htmlRoutes.push(routePath);
  }
}

collectHtmlRoutes(distRoot);

for (const route of htmlRoutes) {
  const loc = `${SITE_URL}${route === "/" ? "/" : route}`;
  const trailingSlashLoc = route === "/" ? loc : `${loc}/`;

  if (
    !sitemap.includes(`<loc>${loc}</loc>`) &&
    !sitemap.includes(`<loc>${trailingSlashLoc}</loc>`)
  ) {
    throw new Error(`Generated route missing from sitemap.xml: ${route}`);
  }
}

console.log(
  `Verified ${htmlRoutes.length} generated HTML route(s) in sitemap.xml.`,
);
