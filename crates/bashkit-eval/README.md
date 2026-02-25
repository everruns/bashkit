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

### 2026-02-25 — Post-Interpreter Fixes (37 tasks, latest)

Major interpreter improvements since last eval: awk arithmetic accumulation, pipe-to-while-loop
variable scoping, tail -n +N, sed capture groups, grep BRE/ERE mode, script execution via path,
plus new features (declare -n/-l/-u, set -x, shopt, select, let, trap -p, FUNCNAME). All four
models show significant gains — Haiku leads at 35/37 (98%), Sonnet close behind at 34/37 (97%).

| Metric | Haiku 4.5 | Sonnet 4 | Opus 4.6 | GPT-5.2 |
|--------|-----------|----------|----------|---------|
| Tasks passed | **35/37** | 34/37 | 33/37 | 27/37 |
| Score | **98%** | 97% | 93% | 86% |
| Tool calls | 104 (100 ok, 4 err) | 151 (144 ok, 7 err) | 169 (152 ok, 17 err) | 102 (74 ok, 28 err) |
| Tool call success | **96%** | 95% | 90% | 73% |
| Tokens | 171K in / 21K out | 197K in / 25K out | 276K in / 33K out | 87K in / 14K out |
| Duration | 3.2 min | 8.7 min | 11.2 min | 4.1 min |

#### Improvement vs Previous Run

| Model | Previous | Current | Delta | Tool Success Δ |
|-------|----------|---------|-------|----------------|
| Haiku 4.5 | 32/37 (95%) | **35/37 (98%)** | +3 | 81% → 96% (+15pp) |
| Sonnet 4 | 32/37 (93%) | **34/37 (97%)** | +2 | 89% → 95% (+6pp) |
| Opus 4.6 | 29/37 (87%) | **33/37 (93%)** | +4 | 82% → 90% (+8pp) |
| GPT-5.2 | 23/37 (80%) | **27/37 (86%)** | +4 | 71% → 73% (+2pp) |

#### Per-Category Comparison

| Category | Haiku 4.5 | Sonnet 4 | Opus 4.6 | GPT-5.2 |
|----------|-----------|----------|----------|---------|
| archive_operations | 100% | 100% | 100% | 50% |
| complex_tasks | **100%** | **100%** | 71% | 88% |
| data_transformation | **100%** | **100%** | **100%** | 90% |
| error_recovery | 100% | 100% | 100% | 100% |
| file_operations | 100% | 100% | 100% | 100% |
| json_processing | 96% | 96% | 92% | 92% |
| pipelines | 100% | 100% | 100% | 90% |
| scripting | 89% | 84% | 89% | 63% |
| system_info | 100% | 100% | 100% | 100% |
| text_processing | **100%** | **100%** | **100%** | 69% |

#### Cross-Model Failure Analysis

| Task | Haiku 4.5 | Sonnet 4 | Opus 4.6 | GPT-5.2 | Root Cause |
|------|-----------|----------|----------|---------|------------|
| script_function_lib | FAIL | FAIL | FAIL | FAIL | bashkit `tr` character class bug (Opus now gets `HELLO WORLD` but not `strlen`) |
| json_to_csv_export | FAIL | FAIL | FAIL | FAIL | jq `@csv` quoting — values double-quoted vs eval expects unquoted |
| script_health_check | PASS | FAIL | FAIL | PASS | exit code 1 despite correct output — model-specific script logic |
| complex_release_notes | PASS | PASS | FAIL | PASS | Opus exhausts turn budget on sed/awk approach |
| complex_todo_app | PASS | PASS | PASS | FAIL | GPT exit code 2 on final call |
| complex_markdown_toc | PASS | PASS | PASS | FAIL | GPT misses lowercase anchors in TOC links |
| text_multifile_replace | PASS | PASS | PASS | FAIL | GPT sed approach doesn't persist changes |
| pipe_dedup_merge | PASS | PASS | PASS | FAIL | GPT misses emails from second file |
| archive_create_extract | PASS | PASS | PASS | FAIL | GPT tar errors, file not created |
| archive_selective | PASS | PASS | PASS | FAIL | GPT tar extraction content mismatch |
| data_log_summarize | PASS | PASS | PASS | FAIL | GPT awk output omits counts |
| script_array_stats | PASS | PASS | PASS | FAIL | GPT makes 0 tool calls (no bash invocation) |

Only `script_function_lib` and `json_to_csv_export` still fail across all four models —
both are bashkit interpreter limitations. Previous all-model blockers `complex_markdown_toc`,
`text_csv_revenue`, and `complex_release_notes` are now fixed.

#### Interpreter Bugs Fixed Since Last Eval

| Bug | Fix Impact |
|-----|-----------|
| `awk` `$2 * $3` accumulation wrong result | `text_csv_revenue` now PASS for all models |
| Variables empty inside `while read` in pipe subshell | `complex_markdown_toc` now PASS for 3/4 models |
| `tail -n +N` returns wrong content | `complex_markdown_toc` unblocked |
| `grep` BRE/ERE metachar handling | `complex_release_notes` now PASS for 3/4 models |
| `sed` capture group substitution `\1`/`\2` | `complex_release_notes` unblocked |
| Script execution via `chmod +x` + path | `complex_release_notes` unblocked |

#### Remaining Interpreter Bugs

| Bug | Affected Tasks | Impact |
|-----|---------------|--------|
| `tr '[:lower:]' '[:upper:]'` character class from pipe | script_function_lib | Blocks all models |
| jq `@csv` adds double-quotes around values | json_to_csv_export | Eval expects unquoted CSV |

#### Model Behavior

- **Haiku 4.5** best overall: 98% pass rate, 96% tool call success, fastest (3.2 min), lowest token usage — adapts to bashkit quirks with simpler constructs
- **Sonnet 4** close second at 97%; highest Anthropic tool call success (95%); efficient at complex multi-step tasks (100% complex_tasks)
- **Opus 4.6** biggest improvement (+4 tasks, +8pp tool success); still struggles on `complex_release_notes` turn budget; 100% on text_processing and data_transformation
- **GPT-5.2** improved +4 tasks but still lowest at 86%; tends to make 0 tool calls or repeat failing patterns; archive_operations remains weak (50%)

### Previous Results

<details>
<summary>2026-02-17 — Sonnet 4 Baseline (37 tasks)</summary>

First eval run with Claude Sonnet 4. Sonnet matches Haiku's pass rate (32/37) while achieving
the highest tool call success rate (89%) of any model tested.

| Metric | Sonnet 4 | Haiku 4.5 | Opus 4.6 | GPT-5.2 |
|--------|----------|-----------|----------|---------|
| Tasks passed | 32/37 | 32/37 | 29/37 | 23/37 |
| Score | 93% | **95%** | 87% | 80% |
| Tool calls | 182 (162 ok, 20 err) | 150 (121 ok, 29 err) | 198 (163 ok, 35 err) | 108 (77 ok, 31 err) |
| Tool call success | **89%** | 81% | 82% | 71% |
| Tokens | 248K in / 30K out | 286K in / 35K out | 315K in / 31K out | 119K in / 17K out |
| Duration | 10.2 min | 6.4 min | 25.2 min | 4.8 min |

</details>

<details>
<summary>2026-02-09 — Expanded Dataset (37 tasks)</summary>

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
