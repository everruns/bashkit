# bashkit-eval: mira Eval Study

## Status

Implemented (reimplemented on the [mira](https://github.com/everruns/mira) eval
framework; supersedes the original hand-rolled harness).

## Purpose

Evaluate how well LLM models use bashkit's bash tool in agentic workloads.
Measure model capability across bash feature categories, identify bashkit
compatibility gaps, and drive improvement.

## Architecture

`bashkit-eval` is a **mira study**: a binary that advertises its evals to the
`mira` host CLI over stdio. Mira owns the model matrix, scheduling, retries,
resume, and reporting (JSON/JUnit/Markdown/HTML). bashkit supplies the
subject-under-test and the scoring.

```
mira host CLI ──spawns──▶ bashkit-eval (study binary)
   │                          │
   │  model matrix            ├─ Sample   (one per JSONL task)
   │  scheduling/retries      ├─ Subject  (bashkit agent loop)
   │  reporting               └─ Scorer   (bashkit expectation checks)
```

Three pieces wire bashkit into mira (`src/mira_study.rs`):

1. **Samples** — each JSONL `EvalTask` / `ScriptingEvalTask` becomes a mira
   `Sample`. The full task rides in `sample.metadata["task"]` (the subject's
   source of truth); its `expectations` array in
   `sample.metadata["expectations"]` (the scorer's source of truth). Datasets
   are embedded via `include_str!` so there is no runtime path dependence.
2. **Subject** — `bash_subject` / `scripting_subject` run bashkit's existing
   agent loop against the case's target model (`cx.target.{provider,model}`),
   then pack the result into a mira `Transcript`.
3. **Scorer** — `expectations_scorer` replays the deterministic bashkit checks
   against the Transcript. A case passes iff **every** check passes (mirrors the
   original `TaskScore::all_passed`); the score value is the weighted pass rate.

### Key Design Decisions

1. **In-process Subject (mira "Path A"), not `mira-everruns`** — bashkit keeps
   its own provider stack (Anthropic Messages, OpenAI Chat Completions, OpenAI
   Responses) and agent loop. The study depends only on `mira-eval` (no
   `mira-everruns`/`everruns-runtime`), keeping the dependency tree small.
2. **`Bash` directly, not `BashTool`** — `BashTool::execute()` creates a fresh
   interpreter per call (no VFS persistence). The agent loop needs a persistent
   VFS across turns. `BashTool::builder()` is used only for
   `input_schema()` / `system_prompt()` introspection.
3. **One `Bash` per task** — fresh instance per sample; VFS persists across all
   tool calls within the task; the snapshot is taken after the loop; the
   instance is dropped after.
4. **Pre-populated VFS** — task `files: {}` map → `Bash::builder().mount_text`.
5. **Snapshot, not a live filesystem, is the scoring substrate** — a mira
   `Scorer` only sees `&Sample` + `&Transcript`. After the run, the subject
   walks the VFS into `transcript.files` (path → contents) and records a
   `Snapshot` (tool-call stdout/stderr/exit codes + directory set) in
   `transcript.metadata["bashkit"]`. The checks read those. See `src/snapshot.rs`.
6. **Model matrix via mira `Target`s** — targets are gated on their provider's
   API-key env var, so an offline run skips them all (CI stays green) and a
   keyed run lights up the subset whose credentials are present.
7. **No bespoke runner/report** — mira provides run orchestration, persistence
   (`./results/<run_id>/`), and reports. The original `runner.rs`, `report.rs`,
   and scripting equivalents are gone.

## Dataset Format (JSONL)

Unchanged from the original harness. One JSON object per line:

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
`llm_judge:prompt` (stub — weight forced to 0). Semantics in
`crates/bashkit-eval/src/checks.rs` (ported verbatim from the original scorer,
with byte-for-byte the same pass/fail logic).

## Providers

Implemented under `src/provider/`, selected by the mira target's `provider` id:

- **Anthropic Messages API** — target `Target::anthropic(model)`; `ANTHROPIC_API_KEY`.
- **OpenAI Chat Completions** — target `Target::openai(model)`; `OPENAI_API_KEY`.
- **OpenAI Responses API** — target `Target::cloud("openresponses", model,
  "OPENAI_API_KEY")`. Required for codex models (e.g. `gpt-5.3-codex`);
  multi-turn via manual input chaining; sets `reasoning.effort: "high"` for
  codex models automatically.

## Evals

Three evals are advertised (`#[eval]` wrappers in `src/main.rs`):

| Eval | Samples | Notes |
|------|---------|-------|
| `bashkit_bash` | 58 tasks across 15 categories | Samples tagged by category; select with `--tag <category>` |
| `bashkit_smoke` | 3 tasks | Quick verification |
| `bashkit_scripting` | scripting-tool tasks | `mode` axis: `scripted` vs `baseline` |

## CLI

Run through the `mira` host (install via `cargo install mira-cli`):

```
mira --bin bashkit-eval list
mira --bin bashkit-eval run bashkit_bash
mira --bin bashkit-eval run bashkit_bash --targets anthropic/claude-opus-4-8 --tag json_processing
mira --bin bashkit-eval run bashkit_scripting --axis mode=scripted
mira --bin bashkit-eval run --format html --out report.html
mira --bin bashkit-eval run --resume <run_id>
```

`just eval`, `just eval-smoke`, `just eval-scripting`, and `just eval-list`
wrap these.

## Output / Metrics

Mira owns output (run folder under `./results/<run_id>/`, plus
JSON/JUnit/Markdown/HTML). The subject records operational telemetry on the
`Transcript` so mira surfaces it:

- **Score** — weighted pass rate of the `bashkit_expectations` scorer; pass iff
  all checks pass.
- **Usage** — input/output tokens (`transcript.usage`).
- **Timing** — wall-clock (`transcript.timing.duration_ms`).
- **Metrics** (`transcript.metrics`, open vocabulary): `turns`, `tool_calls`,
  `tool_calls_ok`, `tool_calls_err`, `natural_stop`. Scripting adds
  `baseline`, `inner_commands`, `inner_tool`, `inner_help`, `inner_discover`,
  `raw_tool_output_bytes`, `tool_output_sent_bytes`.

Low `tool_calls_ok / tool_calls` indicates bashkit compatibility gaps or invalid
model commands.

## Dataset Categories

Datasets live in `crates/bashkit-eval/data/`. Categories span file operations,
text processing, pipelines, scripting, data transformation, error recovery,
system info, archives, JSON processing, complex multi-step tasks, code search,
and environment handling — each with task-appropriate pre-populated seed files.

## Scripting-Tool Eval

A second eval (`bashkit_scripting`) tests `ScriptedTool` orchestration (see
`specs/scripted-tool-orchestration.md`), measuring how well LLMs orchestrate
multiple mock tools via bash scripts vs. calling each tool individually.

### Modes (the `mode` axis)

- **scripted** — all mock tools composed into one `ScriptedTool`; the LLM writes
  bash scripts. Measures tool-composition effectiveness.
- **baseline** — each mock tool exposed as a separate LLM tool; the control.

### Dataset Format

Same JSONL plus per-task `tools` and `discovery_mode`:

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
`ScriptingToolSet::with_discovery()`: tool names hidden from the system prompt;
the LLM must use the `discover`/`help` builtins. Scripting tasks score against
mock-tool stdout (no VFS file checks).

Datasets: `crates/bashkit-eval/data/scripting-tool/` — `large-output.jsonl`,
`many-tools.jsonl` (15–20 tools), `paginated.jsonl`, `discovery.jsonl`.

## Non-Goals

- No bespoke concurrency / scheduling — mira owns it.
- No cost guardrails (mira budget scorers can be added if desired).
- No comparison against real bash.
- No streaming.
- No retries on LLM content errors. The providers retry only *transient* errors
  (rate-limit 429s, 5xx, Anthropic 529) with exponential backoff, and **fast-fail
  on permanent errors** — `insufficient_quota` / billing limits / auth (401/403) —
  so an exhausted account errors immediately instead of hanging in a retry storm.
  All provider HTTP requests use a connect (15s) + total (300s) timeout so a
  single call can never stall a run. Provider/agent failures surface as
  `Transcript::infra_error` → the case scores N/A, not a model failure. mira adds
  its own bounded retry layer (`--max-retries`, default 4).
