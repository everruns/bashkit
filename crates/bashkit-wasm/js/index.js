// Hand-authored entry point for @everruns/bashkit-wasm.
//
// Wraps the wasm-bindgen glue so consumers get a single, bundler-friendly
// module. The .wasm URL is resolved relative to this file via import.meta.url,
// so it loads correctly whether served from a CDN, a bundler, or a plain
// <script type="module">. No SharedArrayBuffer, no COOP/COEP headers required.

import init, { Bash, ExecResult } from "./bashkit_wasm.js";

let initPromise;

/**
 * Initialize the WebAssembly module. Must resolve before constructing `Bash`.
 * Idempotent — repeated calls return the same promise.
 *
 * @param {RequestInfo | URL | Response | BufferSource | WebAssembly.Module} [input]
 *   Optional override for the .wasm source (advanced). Defaults to the bundled
 *   binary resolved relative to this module.
 * @returns {Promise<void>}
 */
export function initBashkit(input) {
  if (!initPromise) {
    const module_or_path =
      input ?? new URL("./bashkit_wasm_bg.wasm", import.meta.url);
    initPromise = init({ module_or_path }).then(() => undefined);
  }
  return initPromise;
}

export { Bash, ExecResult };
