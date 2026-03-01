# BashKit Eval

LLM evaluation harness for bashkit tool usage. Measures how well models use bashkit's bash tool in agentic workloads.

## Usage

```bash
# Run eval (terminal output only)
ANTHROPIC_API_KEY=... cargo run -p bashkit-eval -- run \
  --dataset crates/bashkit-eval/data/eval-tasks.jsonl \
  --provider anthropic --model claude-sonnet-4-20250514

# Run and save results (Chat Completions API)
OPENAI_API_KEY=... cargo run -p bashkit-eval -- run \
  --dataset crates/bashkit-eval/data/eval-tasks.jsonl \
  --provider openai --model gpt-5.2 --save

# Run against OpenAI Responses API (required for codex models)
OPENAI_API_KEY=... cargo run -p bashkit-eval -- run \
  --dataset crates/bashkit-eval/data/eval-tasks.jsonl \
  --provider openresponses --model gpt-5.3-codex --save

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
| `--provider <name>` | `anthropic`, `openai`, or `openresponses` |
| `--model <name>` | Model name (e.g., `claude-sonnet-4-20250514`, `gpt-5.2`, `gpt-5.3-codex`) |
| `--max-turns <n>` | Max agent turns per task (default: 10) |
| `--save` | Save JSON + Markdown results to disk |
| `--output <dir>` | Output directory (default: `crates/bashkit-eval/results`) |
| `--moniker <id>` | Custom run identifier (default: `{provider}-{model}`) |

## Dataset

58 hand-curated tasks in JSONL format across 15 categories: file_operations, text_processing, pipelines, scripting, data_transformation, error_recovery, system_info, archive_operations, json_processing, complex_tasks, code_search, environment, database_operations, config_management, build_simulation.

Smoke test dataset (`data/smoke-test.jsonl`) has 3 tasks for quick verification.

## Results

### 2026-02-28 — Post v0.1.7 Interpreter Fixes (58 tasks, latest)

Dataset expanded from 52 to 58 tasks with 3 new categories (database_operations, config_management,
build_simulation). 20+ interpreter fixes since v0.1.7 release: heredoc redirects (#370), xargs
execution (#364), IFS splitting (#374), ANSI-C quoting (#371), stderr redirects (#377), subshell
isolation (#376), find -exec (#386), tr/cut features (#391), and more.

All 5 models ran the full 58-task dataset.

| Metric | Haiku 4.5 | Sonnet 4.6 | Opus 4.6 | GPT-5.3-Codex | GPT-5.2 |
|--------|-----------|------------|----------|---------------|---------|
| Tasks passed | **54/58** | 48/58 | 50/58 | 51/58 | 41/58 |
| Score | **97%** | 93% | 91% | 91% | 77% |
| Tool calls | 238 (209 ok, 29 err) | 261 (222 ok, 39 err) | 269 (236 ok, 33 err) | 186 (154 ok, 32 err) | 156 (105 ok, 51 err) |
| Tool call success | **88%** | 85% | **88%** | 83% | 67% |
| Tokens | 547K in / 69K out | 561K in / 67K out | 518K in / 61K out | 239K in / 69K out | 201K in / 29K out |
| Duration | 8.6 min | 20.5 min | 20.1 min | 19.6 min | 7.0 min |

#### Delta from v0.1.7 Release

Comparison on the shared 37-task subset from the v0.1.7 release (2026-02-25). Interpreter fixes
unblocked `json_to_csv_export` (jq `@csv`) and `script_function_lib` (tr character classes) across
models.

| Model | v0.1.7 (37 tasks) | Current (37 tasks) | Delta | Newly Passing |
|-------|-------------------|--------------------|-------|---------------|
| Haiku 4.5 | 35/37 (98%) | **37/37 (100%)** | +2pp | json_to_csv_export, script_function_lib |
| Opus 4.6 | 33/37 (93%) | 34/37 (96%) | +3pp | script_function_lib, script_health_check |
| GPT-5.2 | 27/37 (86%) | 30/37 (86%) | +0pp | archive_create_extract, complex_todo_app, data_log_summarize, pipe_dedup_merge |
| Sonnet 4→4.6 | 34/37 (97%) | 33/37 (95%) | -2pp | json_to_csv_export, script_health_check |
| GPT-5.3-Codex | — | 35/37 (97%) | NEW | — |

Note: Sonnet upgraded from 4 to 4.6 between releases; delta reflects both interpreter and model changes.
GPT-5.2 gained 3 more tasks despite unchanged percentage due to rounding.

#### Per-Category Comparison

| Category | Haiku 4.5 | Sonnet 4.6 | Opus 4.6 | GPT-5.3-Codex | GPT-5.2 |
|----------|-----------|------------|----------|---------------|---------|
| archive_operations | **100%** | 50% | **100%** | **100%** | 50% |
| build_simulation | **100%** | 50% | 50% | 50% | 0% |
| code_search | **100%** | **100%** | **100%** | **100%** | **100%** |
| complex_tasks | **100%** | **100%** | 67% | **100%** | 50% |
| config_management | 50% | 50% | 50% | 0% | 0% |
| data_transformation | **100%** | **100%** | **100%** | 67% | 83% |
| database_operations | 50% | **100%** | 50% | **100%** | **100%** |
| environment | **100%** | **100%** | **100%** | **100%** | **100%** |
| error_recovery | **100%** | **100%** | **100%** | **100%** | **100%** |
| file_operations | 75% | 50% | 75% | 75% | 75% |
| json_processing | **100%** | **100%** | 88% | **100%** | 88% |
| pipelines | **100%** | 80% | **100%** | **100%** | 80% |
| scripting | 86% | 57% | 86% | 86% | 43% |
| system_info | **100%** | 50% | **100%** | **100%** | **100%** |
| text_processing | **100%** | **100%** | **100%** | **100%** | 83% |

#### Failure Analysis

| Task | Haiku 4.5 | Sonnet 4.6 | Opus 4.6 | GPT-5.3-Codex | GPT-5.2 | Root Cause |
|------|-----------|------------|----------|---------------|---------|------------|
| config_ini_merge | FAIL | FAIL | FAIL | FAIL | FAIL | INI merging requires complex awk — models struggle with section-aware logic |
| file_path_organizer | FAIL | FAIL | FAIL | FAIL | FAIL | Models burn turns on edge cases, delete own work |
| build_script_generator | PASS | FAIL | FAIL | FAIL | FAIL | Complex Makefile-like dependency graph generation |
| script_getopts_parser | FAIL | FAIL | FAIL | PASS | FAIL | getopts/wc interaction produces wrong output |
| archive_selective | PASS | FAIL | PASS | PASS | FAIL | tar extraction content mismatch |
| complex_release_notes | PASS | PASS | FAIL | PASS | FAIL | Model exhausts turn budget |
| complex_todo_app | PASS | PASS | FAIL | PASS | PASS | Opus exit code issue |
| json_to_csv_export | PASS | PASS | FAIL | PASS | FAIL | jq `@csv` quoting edge case |
| sysinfo_env_report | PASS | FAIL | PASS | PASS | PASS | Sonnet env output format |
| pipe_process_sub | PASS | FAIL | PASS | PASS | PASS | Sonnet process substitution approach |
| data_column_transform | PASS | PASS | PASS | FAIL | PASS | Codex awk column formatting |
| data_regex_extract | PASS | PASS | PASS | FAIL | FAIL | BASH_REMATCH extraction approach |
| config_env_template | PASS | PASS | PASS | FAIL | FAIL | Template variable substitution |

#### Model Behavior

- **Haiku 4.5** leads at 54/58 (97%) — perfect 37/37 on the v0.1.7 task subset, strong across all categories
- **GPT-5.3-Codex** impressive 51/58 (91%) — matches Opus despite using fewer tool calls; excels at complex tasks and JSON
- **Opus 4.6** solid 50/58 (91%) — highest tool call success rate tied with Haiku; struggles with turn-budget-intensive tasks
- **Sonnet 4.6** at 48/58 (93%) — weakest on scripting (57%) and system_info (50%); triggers bashkit awk Unicode panic on some tasks
- **GPT-5.2** at 41/58 (77%) — lowest tool call success (67%), weakest on build_simulation (0%), config_management (0%), scripting (43%)

### Previous Results

<details>
<summary>2026-02-27 — Expanded Dataset (52 tasks)</summary>

Dataset expanded from 37 to 52 tasks with 2 new categories (code_search, environment) and new
tasks in existing categories (heredoc, getopts, associative arrays, process substitution, xargs,
comm, trap). Format-sensitive expectations relaxed to use `stdout_regex` — focus on job done, not
exact output format.

Haiku 4.5 and GPT-5.2 ran on full 52-task dataset. Sonnet 4.6 and Opus 4.6 ran partial datasets
(26 and 23 tasks respectively) due to Anthropic API credit exhaustion during parallel runs.

| Metric | Haiku 4.5 (52) | Sonnet 4.6 (26†) | Opus 4.6 (23†) | GPT-5.2 (52) |
|--------|----------------|------------------|----------------|--------------|
| Tasks passed | **43/52** | 23/26 | **23/23** | 32/52 |
| Score | **92%** | 94% | **100%** | 79% |
| Tool calls | 223 (207 ok, 16 err) | 104 (90 ok, 14 err) | 95 (86 ok, 9 err) | 127 (112 ok, 15 err) |
| Tool call success | **93%** | 87% | 91% | 88% |
| Tokens | 397K in / 46K out | 211K in / 27K out | 143K in / 16K out | 123K in / 20K out |
| Duration | 7.3 min | 6.5 min | 6.1 min | 5.9 min |

† Partial run — API credits exhausted. Covers original 37-task core subset.

</details>

<details>
<summary>2026-02-27 — GPT-5.3-Codex via Responses API (37 tasks)</summary>

First eval using the OpenAI Responses API (`--provider openresponses`). GPT-5.3-Codex scores
30/37 (93%) — a significant jump over GPT-5.2's 27/37 (86%) via Chat Completions. Notably
fixes `json_to_csv_export` and `script_function_lib` which blocked all previous models.

| Metric | Haiku 4.5 | Sonnet 4 | Opus 4.6 | GPT-5.2 | GPT-5.3-Codex |
|--------|-----------|----------|----------|---------|---------------|
| Tasks passed | **35/37** | 34/37 | 33/37 | 27/37 | 30/37 |
| Score | **98%** | 97% | 93% | 86% | 93% |
| Tool calls | 104 (100 ok, 4 err) | 151 (144 ok, 7 err) | 169 (152 ok, 17 err) | 102 (74 ok, 28 err) | 95 (68 ok, 27 err) |
| Tool call success | **96%** | 95% | 90% | 73% | 72% |
| Tokens | 171K in / 21K out | 197K in / 25K out | 276K in / 33K out | 87K in / 14K out | 97K in / 33K out |
| Duration | 3.2 min | 8.7 min | 11.2 min | 4.1 min | 10.6 min |

GPT-5.3-Codex is the first model to pass `script_function_lib` and `json_to_csv_export` (previously
blocked across all models due to bashkit interpreter bugs). It works around the `tr` character class
issue and avoids jq `@csv` quoting. However, it introduces new failures on tasks other models pass
(e.g., `data_csv_to_json`, `error_graceful_parse`, `file_ops_find_and_delete`).

</details>

<details>
<summary>2026-02-25 — Post-Interpreter Fixes (37 tasks)</summary>

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

</details>

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
