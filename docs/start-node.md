# Get started in Node, Bun & Deno

Embed the Bashkit sandbox in a server-side JavaScript or TypeScript runtime.
`@everruns/bashkit` is a NAPI native addon — the fastest JS binding — for Node
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

## Examples

Runnable Node examples in the repo:

- [`bash_basics.mjs`](https://github.com/everruns/bashkit/blob/main/examples/bash_basics.mjs) — first scripts and persistent state
- [`data_pipeline.mjs`](https://github.com/everruns/bashkit/blob/main/examples/data_pipeline.mjs) — pipes and data processing
- [`custom_builtins.mjs`](https://github.com/everruns/bashkit/blob/main/examples/custom_builtins.mjs) — registering JS callbacks as bash commands
- [`llm_tool.mjs`](https://github.com/everruns/bashkit/blob/main/examples/llm_tool.mjs) — exposing Bashkit as an LLM tool
- Agent integrations: [LangChain](https://github.com/everruns/bashkit/blob/main/examples/langchain_agent.mjs), [Vercel AI](https://github.com/everruns/bashkit/blob/main/examples/vercel_ai_tool.mjs), [OpenAI](https://github.com/everruns/bashkit/blob/main/examples/openai_tool.mjs)

## Next steps

- [Custom builtins (JS)](custom_builtins_js.md) — add your own JavaScript-backed commands.
- [Sandbox configuration & limits](configuration.md) — resource limits and sandbox options.
- [LLM tools](llm-tools.md) — expose Bashkit as a sandboxed tool for agent frameworks.
