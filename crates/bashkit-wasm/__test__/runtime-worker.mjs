import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { Bash, initBashkit } from "../pkg/runtimes/index.js";
await initBashkit(
  await readFile(
    new URL("../pkg/runtimes/bashkit_wasm_bg.wasm", import.meta.url),
  ),
);
const bash = new Bash({
  python: true,
  typescript: true,
  externalFunctions: ["lookup"],
  externalHandler: async () => 42,
  typescriptExternalFunctions: ["lookup"],
  typescriptExternalHandler: async () => 42,
});
for (let i = 0; i < 5; i++) {
  const result = await bash.execute(
    `python -c 'print(lookup())'; ts -c 'await lookup()'`,
  );
  assert.equal(result.stdout, "42\n42\n");
}
bash.free();
