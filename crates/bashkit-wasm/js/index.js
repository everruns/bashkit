// Hand-authored entry point for @everruns/bashkit-wasm.
//
// Wraps the wasm-bindgen glue so consumers get a single, bundler-friendly
// module. The .wasm URL is resolved relative to this file via import.meta.url,
// so it loads correctly whether served from a CDN, a bundler, or a plain
// <script type="module">. No SharedArrayBuffer, no COOP/COEP headers required.

import init, * as glue from "./bashkit_wasm.js";

import { validateHandlers, createDispatcher } from "./runtime-functions.js";
const { ExecResult } = glue;
const configs = new WeakMap();

/** Browser interpreter. One active execution per instance; callbacks are execution-local. */
export class Bash extends glue.Bash {
  constructor(options = {}) {
    options ??= {};
    validateHandlers(options);
    super(options);
    configs.set(this, { options: { ...options }, running: false });
    for (const [name, callback] of Object.entries(options.commands ?? {}))
      this.addCommand(name, callback);
  }

  execute(commands, options = {}) {
    const state = configs.get(this);
    if (state.running)
      return Promise.reject(
        new Error(
          "bash instance is busy (reentrant execution is not supported)",
        ),
      );
    if (options.signal?.aborted)
      return Promise.reject(
        new DOMException("execution cancelled", "AbortError"),
      );
    if (
      options.onOutput &&
      (options.signal ||
        options.capture !== undefined ||
        options.stdout ||
        options.stderr)
    )
      return Promise.reject(
        new TypeError(
          "use stdout/stderr byte sinks instead of combining onOutput with live execution options",
        ),
      );
    const controller = new AbortController();
    let aborted = false;
    const cancel = () => {
      aborted = true;
      this.cancel();
      controller.abort();
    };
    state.running = true;
    options.signal?.addEventListener("abort", cancel, { once: true });
    try {
      const host =
        state.options.externalHandler ||
        state.options.typescriptExternalHandler;
      const result = host
        ? super.executeWithRuntime(
            commands,
            createDispatcher(state.options, controller.signal),
            options.onOutput,
            options,
          )
        : options.onOutput
          ? super.executeWithOutput(commands, options.onOutput)
          : super.execute(commands, options);
      return result.finally(() => {
        state.running = false;
        options.signal?.removeEventListener("abort", cancel);
        if (aborted) this.clearCancel();
        controller.abort();
      });
    } catch (error) {
      state.running = false;
      options.signal?.removeEventListener("abort", cancel);
      if (aborted) this.clearCancel();
      controller.abort();
      return Promise.reject(error);
    }
  }

  executeSync(commands, options) {
    if (
      options &&
      (options.capture !== undefined || options.stdout || options.stderr)
    )
      throw new TypeError("streaming I/O requires async execute()");
    return super.executeSync(commands);
  }

  executeWithOutput(commands, onOutput) {
    return this.execute(commands, { onOutput });
  }

  reset() {
    if (configs.get(this).running) throw new Error("bash instance is busy");
    return super.reset();
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
