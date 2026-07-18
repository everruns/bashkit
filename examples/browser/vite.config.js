import { defineConfig } from "vite";

// @everruns/bashkit-web is a slim, single-threaded wasm build. It needs NO
// SharedArrayBuffer and NO cross-origin isolation, so — unlike the old
// wasm32-wasip1-threads example — there are no COOP/COEP headers here.
//
// The package resolves its .wasm via `new URL("./…", import.meta.url)`.
// Excluding it from dep pre-bundling lets Vite serve that asset untouched so
// the URL resolves in both dev and build.
export default defineConfig({
  optimizeDeps: {
    exclude: ["@everruns/bashkit-web"],
  },
  build: {
    target: "esnext",
  },
});
