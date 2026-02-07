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
| Tasks passed | 21/25 | 20/25 | 16/25 |
| Score | 92% | 92% | 77% |
| Tokens | 347K in / 31K out | 271K in / 26K out | 149K in / 12K out |
| Duration | ~10 min | ~3.4 min | ~3.6 min |

#### Per-Category Comparison

| Category | Opus 4.6 | Haiku 4.5 | GPT-5.2 |
|----------|----------|-----------|---------|
| file_operations | 3/3 (100%) | 3/3 (100%) | 3/3 (100%) |
| text_processing | 2/3 (88%) | 1/3 (50%) | 2/3 (88%) |
| pipelines | 2/2 (100%) | 2/2 (100%) | 1/2 (80%) |
| scripting | 3/3 (100%) | 2/3 (87%) | 0/3 (20%) |
| data_transformation | 3/3 (100%) | 2/3 (94%) | 2/3 (88%) |
| error_recovery | 2/2 (100%) | 2/2 (100%) | 2/2 (100%) |
| system_info | 1/2 (71%) | 2/2 (100%) | 2/2 (100%) |
| archive_operations | 2/2 (100%) | 2/2 (100%) | 1/2 (50%) |
| jq_mastery | 2/2 (100%) | 2/2 (100%) | 2/2 (100%) |
| complex_tasks | 1/3 (69%) | 2/3 (88%) | 1/3 (69%) |

#### Key Observations

- All models ace file_operations, error_recovery, jq_mastery (100%)
- All fail `text_awk_report` — bashkit awk field math limitation
- Opus 4.6 is the only model to pass all scripting tasks (3/3)
- Haiku 4.5 matches Opus 4.6 at 92% pass rate with ~25% fewer tokens and ~3x faster
- GPT-5.2 struggles most with scripting (20%) — failed `script_array_stats` with 0 tool calls
- Haiku 4.5 uniquely failed `text_sed_config` (sed-in-file replacement)
- Opus 4.6 uniquely failed `sysinfo_date_calc` (date arithmetic)
- Haiku 4.5 is the only model to pass `complex_markdown_toc`
- `complex_todo_app` fails for all models — exact output format mismatch
- `script_function_lib` fails for Haiku and GPT — bashkit `source` limitation; Opus worked around it

### 2026-02-06 — Initial Baseline

| Metric | Sonnet 4 | GPT-5.2 |
|--------|----------|---------|
| Tasks passed | 19/25 | 16/25 |
| Score | 91% | 80% |
| Tokens | 232K in / 25K out | 110K in / 12K out |
| Duration | ~5 min | ~10 min |

Full per-task details in saved markdown reports under `eval-results/`.
