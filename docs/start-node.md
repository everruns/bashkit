# Get started in Node, Bun & Deno

Embed the Bashkit sandbox in a server-side JavaScript or TypeScript runtime.
`@everruns/bashkit` is a NAPI native addon, the fastest JS binding, for Node
(≥ 18), Bun, and Deno. For browsers and edge runtimes, use [Get started in the
browser](start-browser.md) instead.

## Install

```bash
npm i @everruns/bashkit
```

## First script

```typescript
import { Bash } from "@everruns/bashkit";

const bash = new Bash();
const result = bash.executeSync('echo "Hello, World!"');
console.log(result.stdout);
```

## Python and TypeScript host functions

Enable either embedded runtime and register named host functions. The same options
work with `Bash`, `BashTool`, and the LangChain, OpenAI, Anthropic and Vercel AI adapters.

```typescript
const bash = new Bash({
  python: true,
  externalFunctions: ["lookup"],
  externalHandler: async (name, args, kwargs, { signal }) => {
    const response = await fetch(`https://api.example.com/items/${encodeURIComponent(String(args[0]))}`, { signal });
    return response.json();
  },
  typescript: true,
  typescriptExternalFunctions: ["double"],
  typescriptExternalHandler: (_name, args) => Number(args[0]) * 2,
});
await bash.execute(`python -c 'print(lookup(21))'`);
console.log((await bash.execute(`ts -c 'await double(21)'`)).stdout); // 42
```

Use `execute()` for scripts that invoke JavaScript handlers. `executeSync()`
returns a script error for such calls because it blocks the JavaScript event loop.
Handlers may return a value or a promise. Python receives positional arguments and
keyword arguments separately; TypeScript receives positional arguments.

Values use the JSON subset: null, booleans, finite numbers, strings, arrays, and
objects with string keys, with nesting limited to 64. Integral values must fit
JavaScript's safe integer range. Python tuples become arrays; TypeScript
`undefined` and an omitted host return become null. Bytes, functions, arbitrary
class instances and cyclic values produce script errors.

Each execution captures the caller's `AsyncLocalStorage` context before queuing.
Its handler signal aborts on cancellation, timeout or completion; pass it to host
I/O. A callback cannot reenter its own interpreter or live filesystem. Use another
instance for nested execution. Independent asynchronous requests can share an
instance and queue safely. Synchronous access during asynchronous execution fails
promptly. Independent workers may own instances, including workers stopped with
a pending host function. Callbacks survive reset; supply the options again when restoring a
snapshot, since host code and request context are never serialized.

Host functions run trusted application code. Enforce service authorization and
I/O limits there; registering a function explicitly grants scripts that capability.

## Persistent state

A `Bash` instance keeps its environment and virtual filesystem across calls:

```typescript
const bash = new Bash();
bash.executeSync("X=42");
console.log(bash.executeSync("echo $X").stdout); // 42
```

## Sandbox options

Pass resource limits, identity, and seed files to the constructor:

```typescript
const bash = new Bash({
  cwd: "/home/agent",
  env: { HOME: "/home/agent" },
  maxCommands: 1000,
  maxLoopIterations: 10000,
  maxMemory: 64 * 1024 * 1024,
});
```

See [Sandbox configuration & limits](configuration.md) for the full set.

To run against storage you own rather than the in-memory filesystem, pass an
`fs` object, see
[Host-backed filesystem](filesystem.md#host-backed-filesystem-js).

## Examples

Runnable Node examples in the repo:

- [`bash_basics.mjs`](https://github.com/everruns/bashkit/blob/main/examples/bash_basics.mjs), first scripts and persistent state
- [`data_pipeline.mjs`](https://github.com/everruns/bashkit/blob/main/examples/data_pipeline.mjs), pipes and data processing
- [`custom_builtins.mjs`](https://github.com/everruns/bashkit/blob/main/examples/custom_builtins.mjs), registering JS callbacks as bash commands
- [`llm_tool.mjs`](https://github.com/everruns/bashkit/blob/main/examples/llm_tool.mjs), exposing Bashkit as an LLM tool
- Agent integrations: [LangChain](https://github.com/everruns/bashkit/blob/main/examples/langchain_agent.mjs), [Vercel AI](https://github.com/everruns/bashkit/blob/main/examples/vercel_ai_tool.mjs), [OpenAI](https://github.com/everruns/bashkit/blob/main/examples/openai_tool.mjs)

## Next steps

- [Custom builtins (JS)](custom_builtins_js.md), add your own JavaScript-backed commands.
- [Sandbox configuration & limits](configuration.md), resource limits and sandbox options.
- [LLM tools](llm-tools.md), expose Bashkit as a sandboxed tool for agent frameworks.
