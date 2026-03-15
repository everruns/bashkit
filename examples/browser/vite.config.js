import { defineConfig } from "vite";

export default defineConfig({
  server: {
    // Required for SharedArrayBuffer (WASI threads)
    headers: {
      "Cross-Origin-Opener-Policy": "same-origin",
      "Cross-Origin-Embedder-Policy": "require-corp",
    },
  },
});
