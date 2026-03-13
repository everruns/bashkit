# Bashkit Examples

## realfs_mount.sh

Mount host directories into a sandboxed bashkit session. Demonstrates
`--mount-ro` (readonly) and `--mount-rw` (read-write) CLI flags.

```bash
cargo build -p bashkit-cli --features realfs
bash examples/realfs_mount.sh
```

## Python

Python examples use [PEP 723](https://peps.python.org/pep-0723/) inline script metadata.
`uv run` resolves dependencies automatically — bashkit installs from PyPI as a pre-built wheel (no Rust toolchain needed).

### treasure_hunt_agent.py

LangChain agent with Bashkit sandbox.

```bash
export ANTHROPIC_API_KEY=your_key
uv run examples/treasure_hunt_agent.py
```

### deepagent_coding_agent.py

Deep Agents with Bashkit middleware + backend.

```bash
export ANTHROPIC_API_KEY=your_key
uv run examples/deepagent_coding_agent.py
```

## JavaScript / TypeScript

JS examples import `@everruns/bashkit`. Install from npm or build locally:

```bash
# From npm
npm install @everruns/bashkit

# Or build locally
cd crates/bashkit-js && npm install && npm run build
# Then run with NODE_PATH=crates/bashkit-js
```

### bash_basics.mjs

Core features: execution, pipelines, variables, loops, jq, error handling, reset.

```bash
node examples/bash_basics.mjs
```

### data_pipeline.mjs

Real-world data tasks: CSV processing, JSON transformation, log analysis, report generation.

```bash
node examples/data_pipeline.mjs
```

### llm_tool.mjs

Wire BashTool into any AI framework: tool definition, simulated tool-call loop, generic adapter.

```bash
node examples/llm_tool.mjs
```

### openai_tool.mjs

OpenAI function calling with manual tool-call loop.

```bash
npm install openai
export OPENAI_API_KEY=sk-...
node examples/openai_tool.mjs
```

### vercel_ai_tool.mjs

Vercel AI SDK `tool()` + `generateText()` with automatic tool-call loop.

```bash
npm install ai @ai-sdk/openai zod
export OPENAI_API_KEY=sk-...
node examples/vercel_ai_tool.mjs
```

### langchain_agent.mjs

LangChain.js ReAct agent with `DynamicStructuredTool`.

```bash
npm install @langchain/core @langchain/langgraph @langchain/openai zod
export OPENAI_API_KEY=sk-...
node examples/langchain_agent.mjs
```
