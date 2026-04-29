import { defineConfig } from "astro/config";
import sitemap from "@astrojs/sitemap";
import sitemapEnhance from "./integrations/sitemap-enhance.mjs";

export default defineConfig({
  site: "https://bashkit.sh",
  output: "static",
  markdown: {
    shikiConfig: { theme: "github-light" },
  },
  integrations: [sitemap(), sitemapEnhance()],
});
