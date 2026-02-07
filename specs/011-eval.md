# Spec 011: bashkit-eval — LLM Evaluation Harness

## Purpose

Evaluate how well LLM models use bashkit's bash tool in agentic workloads. Measure model capability across bash feature categories, identify bashkit compatibility gaps, and drive improvement.

## Architecture

```
JSONL Dataset → Runner → Agent Loop (per task) → Scorer → Report
                           ↕                        ↕
                     Provider (LLM)           Bash VFS inspection
                           ↕
                     Bash (bashkit)
```

### Key Design Decisions

1. **`Bash` directly, not `BashTool`** — `BashTool::execute()` creates a fresh interpreter per call (no VFS persistence). Agent loop needs persistent VFS across turns. `BashTool::default()` used only for `input_schema()`, `system_prompt()`, `llmtext()` introspection.

2. **One `Bash` per task** — Each dataset task gets a fresh `Bash` instance. VFS persists across all tool calls within that task. Scorer inspects final VFS state. Instance dropped after scoring.

3. **Pre-populated VFS** — Dataset tasks specify `files: {}` map. Each entry → `Bash::builder().mount_text(path, content)`.

4. **VFS inspection for scoring** — `bash.fs()` returns `Arc<dyn FileSystem>` with `exists()`, `read_file()`, `stat()`. Scorer checks file state after agent loop.

5. **Provider abstraction** — Common `Message`/`ContentBlock` types normalize Anthropic Messages API and OpenAI Chat Completions API differences. Agent loop is provider-agnostic.

6. **Sequential execution** — No concurrency. One task at a time. Simple.

7. **Optional persistence** — `--save` flag. Without it, terminal output only.

## Dataset Format (JSONL)

One JSON object per line:

```json
{
  "id": "file_ops_01",
  "category": "file_operations",
  "description": "Create nested directory structure",
  "system": null,
  "prompt": "Create /project with src/ and tests/ subdirectories",
  "files": {"/data/input.txt": "hello world"},
  "expectations": [
    {"check": "dir_exists:/project/src", "weight": 1.0},
    {"check": "exit_code:0"}
  ]
}
```

Fields:
- `id` — unique task identifier
- `category` — grouping for reporting
- `description` — human-readable
- `system` — optional system message override (null = BashTool default)
- `prompt` — user message sent to LLM
- `files` — map of path→content to pre-populate in VFS
- `expectations` — list of checks with optional weight (default 1.0)

## Expectation Check Types

| Check | Format | Description |
|-------|--------|-------------|
| exit_code | `exit_code:N` | Last tool call exit code equals N |
| stdout_contains | `stdout_contains:text` | Any tool result contains text |
| stdout_regex | `stdout_regex:pattern` | Any tool result matches regex |
| stderr_empty | `stderr_empty` | No stderr in any tool call |
| file_exists | `file_exists:/path` | VFS path exists |
| dir_exists | `dir_exists:/path` | VFS directory exists |
| file_contains | `file_contains:/path:text` | File content contains text |
| tool_calls_min | `tool_calls_min:N` | At least N tool calls made |
| tool_calls_max | `tool_calls_max:N` | At most N tool calls made |
| llm_judge | `llm_judge:prompt` | Stub — not yet implemented |

## Providers

### Anthropic Messages API
- Endpoint: `https://api.anthropic.com/v1/messages`
- Auth: `ANTHROPIC_API_KEY` env var
- Tool format: content blocks with `type: "tool_use"` / `"tool_result"`

### OpenAI Chat Completions API
- Endpoint: `https://api.openai.com/v1/chat/completions`
- Auth: `OPENAI_API_KEY` env var
- Tool format: `tool_calls` array + `role: "tool"` messages

## CLI

```
bashkit-eval run \
  --dataset <path.jsonl> \
  --provider <anthropic|openai> \
  --model <model-name> \
  [--max-turns 10] \
  [--save] \
  [--output eval-results] \
  [--moniker <custom-id>]
```

- `--moniker` — optional custom identifier for the run. Default: auto-generated from `{provider}-{model}`.

## Output

### Terminal (always)
Per-task PASS/FAIL with check details. Summary table with overall score and per-category breakdown.

### Saved (--save flag)
- `{output}/eval-{moniker}-{YYYY-MM-DD-HHmmss}.json` — full results with traces
- `{output}/eval-{moniker}-{YYYY-MM-DD-HHmmss}.md` — markdown report

Moniker defaults to `{provider}-{model}`, overridable via `--moniker`.

## Dataset Categories

| Category | Tests | Pre-populated files |
|----------|-------|-------------------|
| file_operations | Create, copy, move, delete, find | Some tasks have seed files |
| text_processing | grep, sed, awk on data | Log files, CSV, config files |
| pipelines | Multi-stage pipes, command substitution | Text files |
| scripting | Variables, arrays, loops, functions | None |
| data_transformation | CSV↔JSON, log parsing | CSV, JSON, log files |
| error_recovery | Handle missing files, bad input | Broken files |
| system_info | whoami, date, env queries | None |
| archive_operations | tar, gzip workflows | Project files |
| jq_mastery | Complex jq queries | Nested JSON |
| complex_tasks | Multi-step real-world scenarios | Various |

## Results & Analysis

After running evals with `--save`, update `crates/bashkit-eval/README.md` with:

1. **Summary table** — pass rate, score, token usage, duration per model
2. **Per-category comparison** — highlights where models differ
3. **Key observations** — notable failures, bashkit gaps surfaced, model behavioral differences
4. **Date of analysis** — when the results were collected

Keep README highlights concise. Full per-task details live in the saved markdown reports under `eval-results/`.

## Non-Goals

- No concurrency / parallelism
- No cost guardrails
- No comparison against real bash
- No streaming
- No retries on LLM errors
