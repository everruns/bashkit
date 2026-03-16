import { defineConfig } from "vite";
import { fileURLToPath } from "node:url";
import path from "node:path";
import fs from "node:fs";

const __dirname = path.dirname(fileURLToPath(import.meta.url));

// Resolve the WASM browser entry point. The package may be symlinked (local
// dev) or installed from npm (published with WASM). Either way, we need the
// resolved absolute path so Vite can serve it.
function resolveBashkitBrowserEntry() {
  const symlink = path.resolve(
    __dirname,
    "node_modules/@everruns/bashkit/bashkit.wasi-browser.js",
  );
  try {
    const real = fs.realpathSync(symlink);
    if (fs.existsSync(real)) return real;
  } catch {
    // symlink broken or missing — fall through
  }
  return undefined;
}

const browserEntry = resolveBashkitBrowserEntry();

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
  resolve: {
    // Force the browser entry for @everruns/bashkit. Without this, Vite may
    // load wrapper.js (Node-only, uses createRequire) instead of the WASM
    // browser loader when optimizeDeps.exclude is set.
    ...(browserEntry ? { alias: { "@everruns/bashkit": browserEntry } } : {}),
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
