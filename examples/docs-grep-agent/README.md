# Bashkit Docs Grep Agent

Minimal console app for a public Bashkit docs chat.

The LangChain 1.0 `create_agent` helper builds the LangGraph agent directly
with `bashkit.langchain.create_bash_tool()`. The agent writes the bash script
itself, and the script runs inside a bashkit interpreter with real docs mounted
read-only at `/docs/public` and `/docs/rustdoc`, plus a curated `/docs/examples`
view that excludes local generated artifacts. The full bashkit filesystem is
also read-only, so commands cannot copy docs into `/tmp` or create scratch
files. By default the console only streams the final answer. Pass
`--show-tools` to also print each bash script as a one-liner to stderr.

## Run

```bash
cd examples/docs-grep-agent
export OPENAI_API_KEY=sk-...
uv run docs-grep-agent "what is bashkit"
uv run docs-grep-agent "give me example on how to use bashkit cli"
```

Interactive mode:

```bash
uv run docs-grep-agent
```

Show tool calls:

```bash
uv run docs-grep-agent --show-tools "how do read-only mounts work?"
```

Default model is `gpt-5.5-low`, parsed as `model=gpt-5.5` with low reasoning
effort. Override with `--model` or `BASHKIT_DOCS_MODEL`.

## Smoke Test

The self-test does not require an API key. It verifies the docs corpus, the
read-only bashkit mount, blocked `/tmp` copies, and direct bash tool execution:

```bash
uv run docs-grep-agent --self-test
```
