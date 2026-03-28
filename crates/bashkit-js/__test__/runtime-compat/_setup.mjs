// Shared setup for runtime-compat tests.
// Loads the wrapper module (which re-exports native NAPI binding with
// executeSyncOrThrow, BashError, etc.) — works in Node, Bun, Deno.

export { Bash, BashTool, BashError, getVersion } from "../../wrapper.js";
