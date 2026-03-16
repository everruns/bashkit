import { defineConfig } from "vite";
import { fileURLToPath } from "node:url";
import path from "node:path";

const __dirname = path.dirname(fileURLToPath(import.meta.url));

export default defineConfig({
  server: {
    // Required for SharedArrayBuffer (WASI threads)
    headers: {
      "Cross-Origin-Opener-Policy": "same-origin",
      "Cross-Origin-Embedder-Policy": "require-corp",
    },
    fs: {
      // The bashkit package is symlinked from node_modules into
      // crates/bashkit-js/. Vite follows symlinks and resolves @fs/ URLs
      // which are blocked by default. Allow the project root so the WASM
      // binary and worker files are accessible.
      allow: [path.resolve(__dirname, "../..")],
    },
  },
  build: {
    // bashkit.wasi-browser.js uses top-level await
    target: "esnext",
  },
  optimizeDeps: {
    // Don't pre-bundle the WASM browser entry — it uses top-level await and
    // dynamic worker instantiation that Vite's optimizer can't handle.
    exclude: ["@everruns/bashkit"],
  },
});
