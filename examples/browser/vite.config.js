import { defineConfig } from "vite";

export default defineConfig({
  server: {
    // Required for SharedArrayBuffer (WASI threads)
    headers: {
      "Cross-Origin-Opener-Policy": "same-origin",
      "Cross-Origin-Embedder-Policy": "require-corp",
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
