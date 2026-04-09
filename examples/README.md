# Bashkit Examples

## realfs_mount.sh

Mount host directories into a sandboxed bashkit session. Demonstrates
`--mount-ro` (readonly) and `--mount-rw` (read-write) CLI flags.

```bash
cargo build -p bashkit-cli --features realfs
bash examples/realfs_mount.sh
```

## ticket-cli.sh

Run the [wedow/ticket](https://github.com/wedow/ticket) issue tracker inside
bashkit. Exercises plugin discovery via PATH, awk-heavy scripts, YAML
frontmatter parsing, dependency trees, and filtered listing — all interpreted.

```bash
cargo build -p bashkit-cli --features realfs
bash examples/ticket-cli.sh
```

## Python

Python examples use [PEP 723](https://peps.python.org/pep-0723/) inline script metadata.
`uv run` resolves dependencies automatically — bashkit installs from PyPI as a pre-built wheel (no Rust toolchain needed).

### bash_basics.py / k8s_orchestrator.py

Core features and ScriptedTool orchestration:

```bash
uv run crates/bashkit-python/examples/bash_basics.py
uv run crates/bashkit-python/examples/k8s_orchestrator.py
```

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

JS examples install `@everruns/bashkit` from npm. All dependencies are in
`examples/package.json`:

```bash
cd examples && npm install
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
export OPENAI_API_KEY=sk-...
node examples/openai_tool.mjs
```

### vercel_ai_tool.mjs

Vercel AI SDK `tool()` + `generateText()` with automatic tool-call loop.

```bash
export OPENAI_API_KEY=sk-...
node examples/vercel_ai_tool.mjs
```

### langchain_agent.mjs

LangChain.js ReAct agent with `DynamicStructuredTool`.

```bash
export OPENAI_API_KEY=sk-...
node examples/langchain_agent.mjs
```

### browser/

Bashkit running in the browser via WebAssembly. A minimal terminal UI that
lets you type bash commands and see output — all executed client-side in a
sandboxed WASM interpreter.

Requires `Cross-Origin-Opener-Policy` and `Cross-Origin-Embedder-Policy`
headers for `SharedArrayBuffer` support (Vite config handles this).

```bash
cd examples/browser
npm install
npm run dev
```
