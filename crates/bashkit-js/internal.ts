// Shared runtime helpers for wrapper.ts and scripted.ts.
//
// Decision: helpers used by both the root entry (Bash/BashTool) and the
// `@everruns/bashkit/scripted` entry live here so neither entry imports the
// other (no ESM cycle). Not a public entry point: package.json exports only
// ".", "./scripted" and the framework adapters.

import { createRequire } from "node:module";
import type { ExecResult } from "./index.cjs";

const require = createRequire(import.meta.url);
/** @internal Native addon module. */
export const native = require("./index.cjs");

export const MAX_JSON_NESTING_DEPTH = 64;

export const DEFAULT_MAX_INPUT_BYTES = 10_000_000;
const MAX_PENDING_ASYNC_EXECUTIONS = 8;
const ASYNC_EXECUTE_QUEUE_FULL_ERROR =
  "too many pending async execute calls for this instance";
interface AsyncExecuteQueueState {
  tail: Promise<void>;
  pending: number;
}
const asyncExecuteQueues = new WeakMap<object, AsyncExecuteQueueState>();
export function errorExecResult(error: string): ExecResult {
  return {
    stdout: "",
    stdoutBytes: [],
    stderr: error,
    stderrBytes: Array.from(Buffer.from(error, "utf8")),
    exitCode: 1,
    error,
    stdoutTruncated: false,
    stderrTruncated: false,
    finalEnv: undefined,
    success: false,
  };
}

export function inputTooLargeExecResult(
  commands: string,
  maxInputBytes: number,
): ExecResult | undefined {
  const inputBytes = Buffer.byteLength(commands, "utf8");
  if (inputBytes <= maxInputBytes) {
    return undefined;
  }
  return errorExecResult(
    `input too large: ${inputBytes} bytes exceeds maxInputBytes ${maxInputBytes}`,
  );
}

// Decision: serialize async execute() per instance in JS so queued AbortSignal
// listeners only attach once a call reaches the front of the line. Also bound
// the backlog before retaining large command strings in queued closures.
export function queueAsyncExecute<T>(
  owner: object,
  run: () => Promise<T>,
): Promise<T> {
  let state = asyncExecuteQueues.get(owner);
  if (!state) {
    state = { tail: Promise.resolve(), pending: 0 };
    asyncExecuteQueues.set(owner, state);
  }
  if (state.pending >= MAX_PENDING_ASYNC_EXECUTIONS) {
    return Promise.reject(new Error(ASYNC_EXECUTE_QUEUE_FULL_ERROR));
  }
  state.pending += 1;
  const previous = state.tail;
  const completion = previous.then(
    () => run(),
    () => run(),
  );
  state.tail = completion.then(
    () => undefined,
    () => undefined,
  );
  state.tail.finally(() => {
    state.pending -= 1;
    if (state.pending === 0 && asyncExecuteQueues.get(owner) === state) {
      asyncExecuteQueues.delete(owner);
    }
  });
  return completion;
}

export function validateJsonNestingDepth(value: unknown, depth = 0): void {
  if (depth > MAX_JSON_NESTING_DEPTH) {
    throw new RangeError(
      `JSON nesting depth exceeds maximum of ${MAX_JSON_NESTING_DEPTH}`,
    );
  }

  if (Array.isArray(value)) {
    for (const item of value) {
      validateJsonNestingDepth(item, depth + 1);
    }
    return;
  }

  if (value && typeof value === "object") {
    for (const item of Object.values(value as Record<string, unknown>)) {
      validateJsonNestingDepth(item, depth + 1);
    }
  }
}

/**
 * Error thrown when a bash command execution fails.
 */
export class BashError extends Error {
  readonly exitCode: number;
  readonly stderr: string;

  constructor(result: ExecResult) {
    const message =
      result.error ?? result.stderr ?? `Exit code ${result.exitCode}`;
    super(message);
    this.name = "BashError";
    this.exitCode = result.exitCode;
    this.stderr = result.stderr;
  }

  display(): string {
    return `BashError(exit_code=${this.exitCode}): ${this.message}`;
  }
}
