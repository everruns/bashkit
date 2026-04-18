import { defineConfig } from "astro/config";

export default defineConfig({
  site: "https://bashkit.sh",
  output: "static",
  markdown: {
    shikiConfig: { theme: "github-light" },
  },
});
