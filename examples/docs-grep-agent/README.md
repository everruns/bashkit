# Bashkit Docs Search Agent

Minimal console app for a public Bashkit docs chat.

The LangChain 1.0 `create_agent` helper builds the LangGraph agent directly
with `bashkit.langchain.create_bash_tool()`. The agent writes the bash script
itself, and the script runs inside a bashkit interpreter with real docs mounted
read-only at `/docs/public` and `/docs/rustdoc`, plus a curated `/docs/examples`
view that excludes local generated artifacts and lockfiles. The full bashkit
filesystem is also read-only, so commands cannot copy docs into `/tmp` or
create scratch files. Tool output is capped before it reaches the model, and
the prompt steers the agent toward compact search pipelines. By default the
console only streams the final answer. Pass `--show-tools` to also print each
bash script as a one-liner to stderr.

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

## Search Strategy

The agent uses Bashkit's builtin shell tools directly:

- `rg -i -n PATTERN ... | head -20` for broad discovery.
- `rg -i -l PATTERN ... | head -10` to identify likely files before reading.
- `grep -R -i -n -C 1 -m 3 -- PATTERN ...` for contextual evidence snippets.
- `sed -n 'START,ENDp' FILE` after a relevant file and line range are known,
  with ranges kept under 120 lines.
- `find` only for targeted filename discovery.

Bashkit `rg` is a compact ripgrep-style builtin, not full ripgrep. It is useful
for first-pass searches, while `grep` is still better when context flags or
include/exclude filters matter.

The prompt tells the agent not to use `cat` for docs or examples. The LangChain
tool also truncates each bash call at 8000 characters as a final guardrail.

## Smoke Test

The self-test does not require an API key. It verifies the docs corpus, the
read-only bashkit mount, blocked `/tmp` copies, direct bash tool execution, and
prints approximate token counts for focused retrieval versus a full-file read:

```bash
uv run docs-grep-agent --self-test
```
