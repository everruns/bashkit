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

### 2026-02-06 — Initial Baseline

| Metric | Sonnet 4 | GPT-5.2 |
|--------|----------|---------|
| Tasks passed | 19/25 | 16/25 |
| Score | 91% | 80% |
| Tokens | 232K in / 25K out | 110K in / 12K out |
| Duration | ~5 min | ~10 min |

#### Per-Category Comparison

| Category | Sonnet 4 | GPT-5.2 |
|----------|----------|---------|
| file_operations | 3/3 (100%) | 3/3 (100%) |
| text_processing | 2/3 (88%) | 2/3 (88%) |
| pipelines | 1/2 (80%) | 2/2 (100%) |
| scripting | 1/3 (67%) | 0/3 (53%) |
| data_transformation | 3/3 (100%) | 2/3 (75%) |
| error_recovery | 2/2 (100%) | 1/2 (86%) |
| system_info | 2/2 (100%) | 2/2 (100%) |
| archive_operations | 2/2 (100%) | 1/2 (50%) |
| jq_mastery | 2/2 (100%) | 2/2 (100%) |
| complex_tasks | 1/3 (81%) | 1/3 (69%) |

#### Key Observations

- Both models ace file_operations, jq_mastery, system_info (100%)
- Both fail `text_awk_report` — bashkit awk field math limitation
- Scripting is the weakest category for both models
- GPT-5.2 failed `script_array_stats` with 0 tool calls (did not invoke the tool at all)
- GPT-5.2 struggled with `data_csv_to_json` (1/5 score) where Sonnet got 5/5
- Sonnet uses ~2x more tokens but achieves higher accuracy
- `script_function_lib` fails for both — bashkit `source` command limitation

Full per-task details in saved markdown reports under `eval-results/`.
