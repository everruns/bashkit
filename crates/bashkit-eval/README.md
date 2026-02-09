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
| `--output <dir>` | Output directory (default: `crates/bashkit-eval/results`) |
| `--moniker <id>` | Custom run identifier (default: `{provider}-{model}`) |

## Dataset

37 hand-curated tasks in JSONL format across 10 categories: file_operations, text_processing, pipelines, scripting, data_transformation, error_recovery, system_info, archive_operations, json_processing, complex_tasks.

Smoke test dataset (`data/smoke-test.jsonl`) has 3 tasks for quick verification.

## Results

### 2026-02-09 — Expanded Dataset (37 tasks, latest)

Added 12 new scenarios: 6 JSON processing (config merge, NDJSON aggregation, schema migration, JSON→CSV, package.json update, group-by aggregation) and 6 gap-fillers (dedup merge, multi-file replace, health check, column transform, release notes, CSV join). Removed tool-steering from all prompts. Renamed `jq_mastery` → `json_processing`.

| Metric | Haiku 4.5 | Opus 4.6 | GPT-5.2 |
|--------|-----------|----------|---------|
| Tasks passed | 32/37 | 29/37 | 23/37 |
| Score | **95%** | 87% | 80% |
| Tool calls | 150 (121 ok, 29 err) | 198 (163 ok, 35 err) | 108 (77 ok, 31 err) |
| Tool call success | 81% | **82%** | 71% |
| Tokens | 286K in / 35K out | 315K in / 31K out | 119K in / 17K out |
| Duration | 6.4 min | 25.2 min | 4.8 min |

#### Per-Category Comparison

| Category | Haiku 4.5 | Opus 4.6 | GPT-5.2 |
|----------|-----------|----------|---------|
| archive_operations | 100% | 100% | 17% |
| complex_tasks | 92% | 54% | 67% |
| data_transformation | 93% | 90% | 90% |
| error_recovery | 100% | 100% | 100% |
| file_operations | 100% | 100% | 100% |
| json_processing | 92% | 91% | 89% |
| pipelines | 100% | 100% | 80% |
| scripting | 95% | 95% | 53% |
| system_info | 100% | 100% | 100% |
| text_processing | 92% | 69% | 69% |

#### New Scenario Performance

| Task | Haiku 4.5 | Opus 4.6 | GPT-5.2 |
|------|-----------|----------|---------|
| json_config_merge | PASS | FAIL | PASS |
| json_ndjson_error_aggregate | PASS | PASS | PASS |
| json_api_schema_migration | PASS | PASS | PASS |
| json_to_csv_export | FAIL | PASS | FAIL |
| json_package_update | PASS | PASS | FAIL |
| json_order_totals | PASS | PASS | PASS |
| pipe_dedup_merge | PASS | PASS | FAIL |
| text_multifile_replace | PASS | FAIL | FAIL |
| script_health_check | PASS | PASS | PASS |
| data_column_transform | FAIL | FAIL | PASS |
| complex_release_notes | PASS | FAIL | FAIL |
| data_csv_join | PASS | PASS | PASS |

No single new scenario fails across all three models — failures are model-specific, not bashkit limitations. `data_column_transform` and `text_multifile_replace` trip up two of three models each.

#### Model Behavior

- **Haiku 4.5** remains the best score/cost ratio — adapts to bashkit quirks, retries with simpler constructs
- **Opus 4.6** struggles on multi-step complex_tasks (54%) but strong on JSON processing; slowest due to longer reasoning
- **GPT-5.2** tends to repeat failing patterns and often omits writing output to files

### Previous Results (25 tasks)

<details>
<summary>2026-02-08 — Multi-Model Comparison</summary>

| Metric | Haiku 4.5 | Opus 4.6 | GPT-5.2 |
|--------|-----------|----------|---------|
| Tasks passed | 23/25 | 21/25 | 18/25 |
| Score | **98%** | 93% | 81% |
| Tool calls | 93 (81 ok, 12 err) | 143 (125 ok, 18 err) | 103 (80 ok, 23 err) |
| Tool call success | **87%** | **87%** | 78% |
| Tokens | 167K in / 19K out | 242K in / 26K out | 84K in / 10K out |
| Duration | 2.9 min | 8.7 min | 3.4 min |

</details>

<details>
<summary>2026-02-07 — Baseline (pre-interpreter fixes)</summary>

| Metric | Opus 4.6 | Haiku 4.5 | GPT-5.2 |
|--------|----------|-----------|---------|
| Tasks passed | 17/25 | 19/25 | 19/25 |
| Score | 87% | 92% | 87% |
| Tool calls | 141 (106 ok, 35 err) | 116 (93 ok, 23 err) | 84 (48 ok, 36 err) |
| Tool call success | 75% | 80% | 57% |
| Tokens | 319K in / 27K out | 312K in / 29K out | 148K in / 15K out |
| Duration | ~9.4 min | ~4.1 min | ~4.2 min |

</details>

Full per-task traces in saved markdown/JSON reports under [`results/`](results/).
