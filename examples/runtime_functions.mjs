// Named host functions let sandboxed Python/TypeScript call application services.
import assert from "node:assert/strict";
import { BashTool } from "@everruns/bashkit";

const bash = new BashTool({
  python: true,
  externalFunctions: ["lookup"],
  externalHandler: async (_name, args, kwargs, { signal }) => {
    assert.equal(signal.aborted, false);
    return { value: Number(args[0]) * Number(kwargs.multiplier) };
  },
  typescript: true,
  typescriptExternalFunctions: ["double"],
  typescriptExternalHandler: (_name, args) => Number(args[0]) * 2,
});
assert.equal(
  (await bash.execute(`python -c 'print(lookup(21, multiplier=2)["value"])'`)).stdout,
  "42\n",
);
assert.equal((await bash.execute(`ts -c 'await double(21)'`)).stdout, "42\n");
console.log("Python and TypeScript host functions: 42");
