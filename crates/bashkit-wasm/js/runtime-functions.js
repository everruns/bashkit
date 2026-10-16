// JSON interop follows the native JS package; this module has no Node imports.
function runtimeValue(value, depth = 0) {
  if (depth > 64)
    throw new RangeError("host value nesting depth exceeds maximum of 64");
  if (value == null) return null;
  if (typeof value === "string" || typeof value === "boolean") return value;
  if (typeof value === "number") {
    if (!Number.isFinite(value))
      throw new TypeError("host numbers must be finite");
    if (Number.isInteger(value) && !Number.isSafeInteger(value)) {
      throw new RangeError("host integers must be safe JavaScript integers");
    }
    return value;
  }
  if (Array.isArray(value))
    return value.map((item) => runtimeValue(item, depth + 1));
  if (
    typeof value === "object" &&
    [null, Object.prototype].includes(Object.getPrototypeOf(value))
  ) {
    // fromEntries creates own data properties, including the __proto__ key.
    return Object.fromEntries(
      Object.entries(value).map(([key, item]) => [
        key,
        runtimeValue(item, depth + 1),
      ]),
    );
  }
  throw new TypeError("external handlers require JSON values");
}
async function abortable(invoke, signal) {
  if (signal.aborted) throw new Error("host execution is no longer active");
  let abort;
  const cancelled = new Promise((_resolve, reject) => {
    abort = () => reject(new Error("host execution is no longer active"));
    signal.addEventListener("abort", abort, { once: true });
  });
  try {
    return await Promise.race([Promise.resolve().then(invoke), cancelled]);
  } finally {
    signal.removeEventListener("abort", abort);
  }
}

export function validateHandlers(options = {}) {
  for (const [runtime, names, handler] of [
    ["python", "externalFunctions", "externalHandler"],
    ["typescript", "typescriptExternalFunctions", "typescriptExternalHandler"],
  ]) {
    if (options[names]?.length && typeof options[handler] !== "function")
      throw new TypeError(`${handler} is required with external functions`);
    if (
      options[handler] !== undefined &&
      (typeof options[handler] !== "function" || options[runtime] !== true)
    )
      throw new TypeError(
        `${handler} requires ${runtime}: true and a callable handler`,
      );
  }
}

function dispatchRequest(options, signal) {
  return async (request) => {
    try {
      const { language, name, args, kwargs } = JSON.parse(request);
      const values = runtimeValue(args);
      const context = { signal };
      const result = await abortable(
        () =>
          language === "python"
            ? options.externalHandler(
                name,
                values,
                runtimeValue(kwargs),
                context,
              )
            : options.typescriptExternalHandler(name, values, context),
        signal,
      );
      return JSON.stringify({ status: "ok", value: runtimeValue(result) });
    } catch (error) {
      // THREAT[TM-INF-028]: no host paths or stack traces cross this boundary.
      const message = (error instanceof Error ? error.message : String(error))
        .replace(/file:\/\/[^\s]*/g, "<path>")
        .replace(/(?<!\w)\/[^\s]*/g, "<path>")
        .replace(/[A-Za-z]:[\\/][^\s]*/g, "<path>")
        .slice(0, 256);
      return JSON.stringify({
        status: "error",
        value: message || "host callback failed",
      });
    }
  };
}

// THREAT[TM-ISO-029]: wasm-bindgen shares a task wakeup queue between instances.
// Register the
// receiving promise in the execute caller's context, then launch each host
// invocation there. This preserves AsyncLocalStorage/async context in JS hosts
// without importing Node APIs, and lets independent host calls run concurrently.
export function createDispatcher(options, signal) {
  const dispatch = dispatchRequest(options, signal);
  let queue = [],
    wake;
  let incoming = new Promise((resolve) => {
    wake = resolve;
  });
  const abort = () => {
    for (const job of queue.splice(0))
      job.reject(new Error("host execution is no longer active"));
    wake();
  };
  signal.addEventListener("abort", abort, { once: true });
  void (async () => {
    try {
      while (!signal.aborted) {
        await incoming;
        if (signal.aborted) break;
        incoming = new Promise((resolve) => {
          wake = resolve;
        });
        for (const job of queue.splice(0)) {
          void dispatch(job.request).then(job.resolve, job.reject);
        }
      }
    } finally {
      signal.removeEventListener("abort", abort);
    }
  })();
  return (request) => {
    if (signal.aborted)
      return Promise.reject(new Error("host execution is no longer active"));
    return new Promise((resolve, reject) => {
      queue.push({ request, resolve, reject });
      wake();
    });
  };
}
