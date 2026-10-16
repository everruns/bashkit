// Host dispatch is execution-local: bind request AsyncLocalStorage before a
// call can queue, and revoke its AbortSignal when native execution ends.
// A task-local marker rejects recursive calls while unrelated requests queue.
import { AsyncLocalStorage, AsyncResource } from "node:async_hooks";
import type { ExecResult } from "./index.cjs";

export type RuntimeValue =
  | null
  | boolean
  | number
  | string
  | RuntimeValue[]
  | { [key: string]: RuntimeValue };

export interface HostFunctionContext {
  /** Aborted when execution is cancelled, times out, or completes. */
  signal: AbortSignal;
}

export type ExternalHandler = (
  name: string,
  args: RuntimeValue[],
  kwargs: Record<string, RuntimeValue>,
  context: HostFunctionContext,
) => RuntimeValue | undefined | Promise<RuntimeValue | undefined>;

export type TypeScriptExternalHandler = (
  name: string,
  args: RuntimeValue[],
  context: HostFunctionContext,
) => RuntimeValue | undefined | Promise<RuntimeValue | undefined>;

export interface RuntimeFunctionOptions {
  python?: boolean;
  typescript?: boolean;
  externalFunctions?: string[];
  externalHandler?: ExternalHandler;
  typescriptExternalFunctions?: string[];
  typescriptExternalHandler?: TypeScriptExternalHandler;
}

const handlers = new WeakMap<object, RuntimeFunctionOptions>();
const callbackOwners = new AsyncLocalStorage<ReadonlySet<object>>();

export function assertRuntimeAccess(owner: object): void {
  if (callbackOwners.getStore()?.has(owner)) {
    throw new Error(
      "external handler cannot re-enter the same interpreter or its live filesystem",
    );
  }
}

export function validateRuntimeFunctions(
  options?: RuntimeFunctionOptions,
): void {
  for (const [language, names, handler, label] of [
    [
      "python",
      options?.externalFunctions,
      options?.externalHandler,
      "externalHandler",
    ],
    [
      "typescript",
      options?.typescriptExternalFunctions,
      options?.typescriptExternalHandler,
      "typescriptExternalHandler",
    ],
  ] as const) {
    if (names?.length && !handler)
      throw new TypeError(`${label} is required with external functions`);
    if (handler !== undefined) {
      if (typeof handler !== "function")
        throw new TypeError(`${label} must be callable`);
      if (!options?.[language])
        throw new TypeError(`${label} requires ${language}: true`);
    }
  }
}

export function configureRuntimeFunctions<T extends object>(
  native: T,
  options?: RuntimeFunctionOptions,
): T {
  validateRuntimeFunctions(options);
  if (!options?.externalHandler && !options?.typescriptExternalHandler)
    return native;
  // Protect all public instance operations, including methods used by adapters.
  // Native sync entry points separately protect retained/raw filesystem handles.
  const owner = new Proxy(native, {
    get(target, key) {
      if (key !== "cancel" && key !== "clearCancel") assertRuntimeAccess(owner);
      const value = Reflect.get(target, key, target);
      return typeof value === "function"
        ? (...args: unknown[]) => {
            if (key !== "cancel" && key !== "clearCancel")
              assertRuntimeAccess(owner);
            return Reflect.apply(value, target, args);
          }
        : value;
    },
  });
  handlers.set(owner, { ...options });
  return owner;
}

// Build a fresh JSON tree: reject non-finite/unsafe numbers, functions,
// arbitrary objects and cyclic/deep values rather than silently coercing them.
function runtimeValue(value: unknown, depth = 0): RuntimeValue {
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

async function abortable<T>(
  invoke: () => T | Promise<T>,
  signal: AbortSignal,
): Promise<T> {
  if (signal.aborted) throw new Error("host execution is no longer active");
  let abort!: () => void;
  const cancelled = new Promise<never>((_resolve, reject) => {
    abort = () => reject(new Error("host execution is no longer active"));
    signal.addEventListener("abort", abort, { once: true });
  });
  try {
    return await Promise.race([Promise.resolve().then(invoke), cancelled]);
  } finally {
    signal.removeEventListener("abort", abort);
  }
}

export interface RuntimeSession {
  dispatch: (request: string) => Promise<string>;
  finish(): void;
}

export function captureRuntimeSession(
  owner: object,
): RuntimeSession | undefined {
  assertRuntimeAccess(owner);
  const config = handlers.get(owner);
  if (!config) return undefined;
  const controller = new AbortController();
  const context = { signal: controller.signal };
  const dispatch = AsyncResource.bind(async (request: string) => {
    try {
      if (context.signal.aborted)
        throw new Error("host execution is no longer active");
      const payload = JSON.parse(request);
      const args = runtimeValue(payload.args) as RuntimeValue[];
      const kwargs = runtimeValue(payload.kwargs ?? {}) as Record<
        string,
        RuntimeValue
      >;
      const owners = new Set(callbackOwners.getStore());
      owners.add(owner);
      const result = await callbackOwners.run(owners, () =>
        abortable(() => {
          if (payload.language === "python" && config.externalHandler) {
            return config.externalHandler(payload.name, args, kwargs, context);
          }
          if (
            payload.language === "typescript" &&
            config.typescriptExternalHandler
          ) {
            return config.typescriptExternalHandler(
              payload.name,
              args,
              context,
            );
          }
          throw new Error("no handler configured for this runtime");
        }, context.signal),
      );
      return JSON.stringify({ status: "ok", value: runtimeValue(result) });
    } catch (error) {
      // THREAT[TM-INF-028]: expose a bounded message, never a host stack/path.
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
  });
  return { dispatch, finish: () => controller.abort() };
}

export function executeNativeRuntime(
  native: {
    execute(commands: string): Promise<ExecResult>;
    executeWithOutput(commands: string, output: any): Promise<ExecResult>;
  },
  commands: string,
  output: unknown,
  session?: RuntimeSession,
): Promise<ExecResult> {
  if (session) {
    return (
      native as typeof native & {
        executeWithRuntime(
          commands: string,
          dispatch: RuntimeSession["dispatch"],
          output?: unknown,
        ): Promise<ExecResult>;
      }
    ).executeWithRuntime(commands, session.dispatch, output);
  }
  return output
    ? native.executeWithOutput(commands, output)
    : native.execute(commands);
}
