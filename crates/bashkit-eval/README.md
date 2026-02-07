# BashKit Eval

LLM evaluation harness for bashkit tool usage. Measures how well models use bashkit's bash tool in agentic workloads.

## Usage

```bash
# Run eval (terminal output only)
ANTHROPIC_API_KEY=... cargo run -p bashkit-eval -- run \
  --dataset crates/bashkit-eval/data/eval-tasks.jsonl \
  --provider anthropic --model claude-sonnet-4-20250514

# Run and save results
OPENAI_API_KEY=... cargo run -p bashkit-eval -- run \
  --dataset crates/bashkit-eval/data/eval-tasks.jsonl \
  --provider openai --model gpt-5.2 --save

# Custom moniker
cargo run -p bashkit-eval -- run \
  --dataset crates/bashkit-eval/data/eval-tasks.jsonl \
  --provider anthropic --model claude-sonnet-4-20250514 \
  --save --moniker my-test-run

# Via just
just eval
just eval-save
```

## Options

| Option | Description |
|--------|-------------|
| `--dataset <path>` | Path to JSONL dataset file |
| `--provider <name>` | `anthropic` or `openai` |
| `--model <name>` | Model name (e.g., `claude-sonnet-4-20250514`, `gpt-5.2`) |
| `--max-turns <n>` | Max agent turns per task (default: 10) |
| `--save` | Save JSON + Markdown results to disk |
| `--output <dir>` | Output directory (default: `eval-results`) |
| `--moniker <id>` | Custom run identifier (default: `{provider}-{model}`) |

## Dataset

25 hand-curated tasks in JSONL format across 10 categories: file_operations, text_processing, pipelines, scripting, data_transformation, error_recovery, system_info, archive_operations, jq_mastery, complex_tasks.

Smoke test dataset (`data/smoke-test.jsonl`) has 3 tasks for quick verification.

## Results

### 2026-02-07 — Multi-Model Comparison

| Metric | Opus 4.6 | Haiku 4.5 | GPT-5.2 |
|--------|----------|-----------|---------|
| Tasks passed | 17/25 | 19/25 | 19/25 |
| Score | 87% | 92% | 87% |
| Tool calls | 141 (106 ok, 35 err) | 116 (93 ok, 23 err) | 84 (48 ok, 36 err) |
| Tool call success | 75% | 80% | 57% |
| Tokens | 319K in / 27K out | 312K in / 29K out | 148K in / 15K out |
| Duration | ~9.4 min | ~4.1 min | ~4.2 min |

#### Per-Category Comparison

| Category | Opus 4.6 | Haiku 4.5 | GPT-5.2 |
|----------|----------|-----------|---------|
| file_operations | 3/3 (100%) | 3/3 (100%) | 3/3 (100%) |
| text_processing | 2/3 (88%) | 1/3 (50%) | 2/3 (88%) |
| pipelines | 1/2 (80%) | 2/2 (100%) | 2/2 (100%) |
| scripting | 2/3 (87%) | 3/3 (100%) | 1/3 (67%) |
| data_transformation | 2/3 (94%) | 2/3 (94%) | 2/3 (94%) |
| error_recovery | 2/2 (100%) | 2/2 (100%) | 2/2 (100%) |
| system_info | 1/2 (71%) | 2/2 (100%) | 2/2 (100%) |
| archive_operations | 2/2 (100%) | 2/2 (100%) | 2/2 (100%) |
| jq_mastery | 2/2 (100%) | 2/2 (100%) | 2/2 (100%) |
| complex_tasks | 0/3 (56%) | 0/3 (75%) | 1/3 (56%) |

#### Key Observations

- All models ace file_operations, error_recovery, jq_mastery, archive_operations (100%)
- All fail `text_awk_report` — bashkit awk field math limitation
- GPT-5.2 has lowest tool call success rate (57%) — many bash calls fail due to incompatibility
- Haiku 4.5 achieves highest task pass rate (19/25) and tool call success (80%) with lowest cost
- Opus 4.6 uses most tokens but doesn't lead on pass rate — complex_tasks drag it down
- `sysinfo_date_calc` fails for Opus (0/7 calls succeeded) — bashkit `date -d` limitation
- `complex_todo_app` fails for all — exact output format mismatch
- `script_function_lib` fails for Opus and GPT — bashkit `source` limitation; Haiku worked around it

Full per-task details in saved markdown reports under `eval-results/`.
