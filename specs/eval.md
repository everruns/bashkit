# bashkit-eval: LLM Evaluation Harness

## Status

Implemented

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

1. **`Bash` directly, not `BashTool`** — `BashTool::execute()` creates a fresh interpreter per call (no VFS persistence). The agent loop needs persistent VFS across turns. `BashTool::default()` used only for `input_schema()` / `system_prompt()` / `help()` introspection.
2. **One `Bash` per task** — fresh instance per dataset task; VFS persists across all tool calls within the task; scorer inspects final VFS state; instance dropped after scoring.
3. **Pre-populated VFS** — task `files: {}` map → `Bash::builder().mount_text(path, content)`.
4. **VFS inspection for scoring** — `bash.fs()` returns `Arc<dyn FileSystem>` (`exists()`, `read_file()`, `stat()`).
5. **Provider abstraction** — common `Message`/`ContentBlock` types normalize Anthropic Messages API, OpenAI Chat Completions API, and OpenAI Responses API differences. Agent loop is provider-agnostic.
6. **Sequential execution** — no concurrency, one task at a time.
7. **Optional persistence** — `--save` flag; otherwise terminal output only.

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

`system` = optional system-message override (null = BashTool default);
`expectations` = checks with optional weight (default 1.0).

## Expectation Check Types

`exit_code:N`, `stdout_contains:text`, `stdout_regex:pattern`,
`stderr_empty`, `file_exists:/path`, `dir_exists:/path`,
`file_contains:/path:text`, `file_line_regex:/path:pattern`,
`llm_judge:prompt` (stub — not yet implemented). Semantics in
`crates/bashkit-eval/src/scorer.rs`.

## Providers

- **Anthropic Messages API** — `ANTHROPIC_API_KEY`; `tool_use`/`tool_result` content blocks.
- **OpenAI Chat Completions** — `OPENAI_API_KEY`; `tool_calls` + `role: "tool"` messages.
- **OpenAI Responses API** — `OPENAI_API_KEY`; `function_call`/`function_call_output` input items. Required for codex models (e.g. `gpt-5.3-codex`); multi-turn via manual input chaining (appends response output + tool results to next input); sets `reasoning.effort: "high"` for codex models automatically.

## CLI

```
bashkit-eval run \
  --dataset <path.jsonl> \
  --provider <anthropic|openai|openresponses> \
  --model <model-name> \
  [--max-turns 10] [--save] \
  [--output crates/bashkit-eval/results] [--moniker <custom-id>]
```

`--moniker` defaults to `{provider}-{model}`.

## Output

Terminal (always): per-task PASS/FAIL with check details; summary table with
overall score and per-category breakdown. With `--save`:
`{output}/eval-{moniker}-{YYYY-MM-DD-HHmmss}.{json,md}` (full traces +
markdown report).

## Metrics

### Task-level
- **Score** — weighted sum of passed checks vs total weight
- **Turns** — LLM round-trips (each `provider.chat()` call = 1 turn)
- **Tool calls** — total bash invocations, split into ok (exit_code 0) and error
- **Tokens** — input/output counts
- **Duration** — wall-clock

### Summary-level
- **Tasks passed** — tasks where all checks pass
- **Overall score** — aggregate weighted score
- **Tool call success rate** — `tool_calls_ok / total_tool_calls`; low rates indicate bashkit compatibility gaps or invalid model commands
- **Tool/command count telemetry** — outer tool calls and (for `scripting-tool`) inner command invocations, tracked for historical trend analysis only, never scoring
- **Per-category breakdown** — pass rate per category

## Dataset Categories

Datasets live in `crates/bashkit-eval/data/`. Categories span file
operations, text processing, pipelines, scripting, data transformation,
error recovery, system info, archives, JSON processing, complex multi-step
tasks, code search, and environment handling — each with task-appropriate
pre-populated seed files.

## Results & Analysis

After `--save` runs, update `crates/bashkit-eval/README.md` with a concise
summary table, per-category comparison, key observations, and analysis date;
full per-task details stay in the saved reports under
`crates/bashkit-eval/results/`. Saved reports are also consumed by the site
`/benches` page — see `specs/performance-results.md` for the
result-location/aggregation contract.

## Scripting Tool Eval Mode

A second eval type tests `ScriptedTool` orchestration (see
`specs/scripted-tool-orchestration.md`), measuring how well LLMs orchestrate
multiple mock tools via bash scripts vs calling each tool individually.

### Modes

- **Scripted** — all mock tools composed into one `ScriptedTool`; LLM writes bash scripts. Measures tool-composition effectiveness.
- **Baseline** — each mock tool exposed as a separate LLM tool; control for comparison.

### Dataset Format

Same JSONL format plus per-task `tools` and `discovery_mode`:

```json
{
  "id": "mt-ecommerce",
  "category": "many_tools",
  "prompt": "Look up user 42 and summarize their last order",
  "discovery_mode": false,
  "tools": [
    {
      "name": "get_user",
      "description": "Fetch user by ID",
      "schema": {"type": "object", "properties": {"id": {"type": "integer"}}},
      "tags": ["read", "users"],
      "category": "users",
      "mock": {"param": "id", "responses": {"42": "{\"name\": \"Jane\"}"}}
    }
  ],
  "expectations": [{"check": "stdout_contains:Jane"}]
}
```

Mock behaviors: **Static** (`"mock": "fixed string"`) or **ByParam**
(`{"param": "key", "responses": {...}, "default": "fallback"}`). `tags` /
`category` feed `discover` filtering. `discovery_mode: true` uses
`ScriptingToolSet::with_discovery()`: tool names hidden from the system
prompt; the LLM must use the `discover`/`help` builtins.

Datasets: `crates/bashkit-eval/data/scripting-tool/` — `large-output.jsonl`,
`many-tools.jsonl` (15–20 tools), `paginated.jsonl`, `discovery.jsonl`.

### CLI

As above plus `--eval-type scripting-tool` and `[--baseline]`.

### Metrics (additional)

- **Raw tool output bytes** vs **tool output sent bytes** (after formatting)
- **Inner command telemetry** — per-task inner scripted command counts, split tool/help/discover
- **Per-mode comparison** — scripted vs baseline pass rate, tokens, turns

## Non-Goals

- No concurrency / parallelism
- No cost guardrails
- No comparison against real bash
- No streaming
- No retries on LLM content errors (retries only on 429/5xx with exponential backoff)
