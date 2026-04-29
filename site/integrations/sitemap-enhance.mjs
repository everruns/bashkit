// Decision: keep /sitemap.xml as the only crawl target and remove Astro's
// generated sitemap index so crawlers do not discover a redundant URL.
// Decision: use build date for <lastmod>; Cloudflare builds may not have full
// git history, and freshness is enough for this small static site.
import { existsSync, readFileSync, unlinkSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

export default function sitemapEnhance() {
  return {
    name: "sitemap-enhance",
    hooks: {
      "astro:build:done": async ({ dir }) => {
        const outDir = fileURLToPath(dir);
        const sitemapSrc = join(outDir, "sitemap-0.xml");

        let content;
        try {
          content = readFileSync(sitemapSrc, "utf-8");
        } catch {
          console.warn("[sitemap-enhance] sitemap-0.xml not found, skipping");
          return;
        }

        const lastmod = new Date().toISOString().split("T")[0];
        const enhanced = content.replace(
          /<url><loc>(.*?)<\/loc><\/url>/g,
          (_match, url) =>
            `<url><loc>${url}</loc><lastmod>${lastmod}</lastmod></url>`,
        );

        const sitemapDest = join(outDir, "sitemap.xml");
        const sitemapIndexDest = join(outDir, "sitemap-index.xml");
        writeFileSync(sitemapDest, enhanced);

        unlinkSync(sitemapSrc);
        if (existsSync(sitemapIndexDest)) {
          unlinkSync(sitemapIndexDest);
        }

        const entryCount = (enhanced.match(/<url>/g) || []).length;
        console.log(
          `[sitemap-enhance] sitemap.xml written with ${entryCount} entries (lastmod: ${lastmod})`,
        );
      },
    },
  };
}
