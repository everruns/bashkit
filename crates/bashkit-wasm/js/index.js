// Hand-authored entry point for @everruns/bashkit-wasm.
//
// Wraps the wasm-bindgen glue so consumers get a single, bundler-friendly
// module. The .wasm URL is resolved relative to this file via import.meta.url,
// so it loads correctly whether served from a CDN, a bundler, or a plain
// <script type="module">. No SharedArrayBuffer, no COOP/COEP headers required.

import init, * as glue from "./bashkit_wasm.js";

const { ExecResult } = glue;

export class Bash extends glue.Bash {
  constructor(options) {
    super(options);
    for (const [name, callback] of Object.entries(options?.commands ?? {})) this.addCommand(name, callback);
  }
  async execute(commands, options) {
    const signal = options?.signal;
    if (signal?.aborted) throw new DOMException("execution cancelled", "AbortError");
    let aborted = false;
    const onAbort = () => { aborted = true; this.cancel(); };
    signal?.addEventListener("abort", onAbort, { once: true });
    try { return await super.execute(commands, options); }
    finally {
      signal?.removeEventListener("abort", onAbort);
      if (aborted) this.clearCancel();
    }
  }
  executeSync(commands, options) {
    if (options && (options.capture !== undefined || options.stdout || options.stderr)) {
      throw new TypeError("streaming I/O requires async execute()");
    }
    return super.executeSync(commands);
  }
}

/**
 * Interactive terminal session, or `undefined` when the bundle was built
 * without the `terminal` cargo feature.
 */
export const Terminal = glue.Terminal;

let initPromise;

/** Typed named execution-policy selectors for BashOptions.profile. */
export const ExecutionProfile = Object.freeze({
  Hardened: "hardened",
  Standard: "standard",
  Interactive: "interactive",
});

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

export { ExecResult };
