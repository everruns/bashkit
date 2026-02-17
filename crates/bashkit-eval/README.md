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

### 2026-02-17 — Sonnet 4 Baseline (37 tasks, latest)

First eval run with Claude Sonnet 4. Sonnet matches Haiku's pass rate (32/37) while achieving
the highest tool call success rate (89%) of any model tested. Notably fixes `data_column_transform`
and `complex_diff_report` that tripped up other models, but shares the same systemic bashkit-bug
failures (`text_csv_revenue`, `script_function_lib`, `complex_markdown_toc`).

| Metric | Sonnet 4 | Haiku 4.5 | Opus 4.6 | GPT-5.2 |
|--------|----------|-----------|----------|---------|
| Tasks passed | 32/37 | 32/37 | 29/37 | 23/37 |
| Score | 93% | **95%** | 87% | 80% |
| Tool calls | 182 (162 ok, 20 err) | 150 (121 ok, 29 err) | 198 (163 ok, 35 err) | 108 (77 ok, 31 err) |
| Tool call success | **89%** | 81% | 82% | 71% |
| Tokens | 248K in / 30K out | 286K in / 35K out | 315K in / 31K out | 119K in / 17K out |
| Duration | 10.2 min | 6.4 min | 25.2 min | 4.8 min |

#### Per-Category Comparison

| Category | Sonnet 4 | Haiku 4.5 | Opus 4.6 | GPT-5.2 |
|----------|----------|-----------|----------|---------|
| archive_operations | 100% | 100% | 100% | 17% |
| complex_tasks | 62% | 92% | 54% | 67% |
| data_transformation | **100%** | 93% | 90% | 90% |
| error_recovery | 100% | 100% | 100% | 100% |
| file_operations | 100% | 100% | 100% | 100% |
| json_processing | 96% | 92% | 91% | 89% |
| pipelines | 100% | 100% | 100% | 80% |
| scripting | 95% | 95% | 95% | 53% |
| system_info | 100% | 100% | 100% | 100% |
| text_processing | 92% | 92% | 69% | 69% |

#### Cross-Model Failure Analysis

| Task | Sonnet 4 | Haiku 4.5 | Opus 4.6 | GPT-5.2 | Root Cause |
|------|----------|-----------|----------|---------|------------|
| text_csv_revenue | FAIL | FAIL | PASS | PASS | bashkit `awk` arithmetic bug |
| script_function_lib | FAIL | FAIL | FAIL | FAIL | bashkit `tr` character class bug |
| complex_markdown_toc | FAIL | FAIL | FAIL | FAIL | bashkit pipe-to-while-loop + turn budget |
| json_to_csv_export | FAIL | FAIL | PASS | FAIL | jq `@csv` quoting vs eval expectations |
| complex_release_notes | FAIL | PASS | FAIL | FAIL | bashkit `grep`/`sed`/`awk` regex bugs |
| data_column_transform | PASS | FAIL | FAIL | PASS | model-specific |
| data_csv_to_json | PASS | PASS | FAIL | PASS | model-specific |
| complex_todo_app | PASS | PASS | FAIL | PASS | model-specific |
| json_config_merge | PASS | PASS | FAIL | PASS | model-specific |
| text_multifile_replace | PASS | PASS | FAIL | FAIL | model-specific |

Two tasks (`script_function_lib`, `complex_markdown_toc`) fail across **all four models** — these
are bashkit interpreter limitations, not model weaknesses. Three more fail on 3/4 models
(`json_to_csv_export`, `complex_release_notes`, `text_csv_revenue`), also driven by interpreter bugs.

#### Bashkit Interpreter Bugs Surfaced

| Bug | Affected Tasks | Impact |
|-----|---------------|--------|
| `tr '[:lower:]' '[:upper:]'` produces empty output from pipe | script_function_lib | Blocks all models |
| Variables empty inside `while read` in pipe subshell | complex_markdown_toc | Blocks all models |
| `awk` `$2 * $3` accumulation returns wrong result | text_csv_revenue | Wrong math (204 vs 329) |
| `grep` treats `(` as ERE metachar in default BRE mode | complex_release_notes | Requires `\(` escaping |
| `sed` capture group substitution `\1`/`\2` has no effect | complex_release_notes | Silent no-op |
| `awk match()` with capture array unsupported | complex_release_notes, complex_markdown_toc | Error on valid GNU awk |
| `tail -n +N` returns wrong content | complex_markdown_toc | Returns only last section |
| Script execution via `chmod +x` + path fails | complex_release_notes | "command not found" |

#### Model Behavior

- **Sonnet 4** highest tool call success rate (89%); efficient token usage; shares Haiku's failure
  profile on bashkit-bug tasks; struggles on `complex_release_notes` due to cascading interpreter bugs
- **Haiku 4.5** best score/cost ratio (95% score, fastest) — adapts to bashkit quirks, retries with simpler constructs
- **Opus 4.6** struggles on multi-step complex_tasks (54%) but strong on JSON processing; slowest due to longer reasoning
- **GPT-5.2** tends to repeat failing patterns and often omits writing output to files

### Previous Results

<details>
<summary>2026-02-09 — Expanded Dataset (37 tasks)</summary>

Added 12 new scenarios: 6 JSON processing (config merge, NDJSON aggregation, schema migration, JSON-CSV, package.json update, group-by aggregation) and 6 gap-fillers (dedup merge, multi-file replace, health check, column transform, release notes, CSV join). Removed tool-steering from all prompts. Renamed `jq_mastery` to `json_processing`.

| Metric | Haiku 4.5 | Opus 4.6 | GPT-5.2 |
|--------|-----------|----------|---------|
| Tasks passed | 32/37 | 29/37 | 23/37 |
| Score | **95%** | 87% | 80% |
| Tool calls | 150 (121 ok, 29 err) | 198 (163 ok, 35 err) | 108 (77 ok, 31 err) |
| Tool call success | 81% | **82%** | 71% |
| Tokens | 286K in / 35K out | 315K in / 31K out | 119K in / 17K out |
| Duration | 6.4 min | 25.2 min | 4.8 min |

</details>

<details>
<summary>2026-02-08 — Multi-Model Comparison (25 tasks)</summary>

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
