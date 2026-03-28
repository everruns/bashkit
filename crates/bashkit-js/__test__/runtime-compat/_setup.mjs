// Shared setup for runtime-compat tests.
// Loads the native NAPI binding via createRequire (works in Node, Bun, Deno).

import { createRequire } from "node:module";

const require = createRequire(import.meta.url);
const native = require("../../index.cjs");

export const { Bash, BashTool, getVersion } = native;
