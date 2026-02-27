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

52 hand-curated tasks in JSONL format across 12 categories: file_operations, text_processing, pipelines, scripting, data_transformation, error_recovery, system_info, archive_operations, json_processing, complex_tasks, code_search, environment.

Smoke test dataset (`data/smoke-test.jsonl`) has 3 tasks for quick verification.

## Results

### 2026-02-27 — Expanded Dataset (52 tasks, latest)

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

#### Per-Category Comparison

Categories with `-` indicate the model did not run those tasks.

| Category | Haiku 4.5 | Sonnet 4.6 | Opus 4.6 | GPT-5.2 |
|----------|-----------|------------|----------|---------|
| archive_operations | **100%** | 50% | **100%** | 50% |
| code_search | 50% | - | - | 0% |
| complex_tasks | **100%** | 67% | 100% | 67% |
| data_transformation | 83% | **100%** | **100%** | 67% |
| environment | 50% | - | - | 50% |
| error_recovery | **100%** | **100%** | **100%** | **100%** |
| file_operations | 75% | **100%** | **100%** | 50% |
| json_processing | **100%** | **100%** | **100%** | 75% |
| pipelines | 80% | **100%** | **100%** | 40% |
| scripting | 57% | **100%** | **100%** | 43% |
| system_info | **100%** | 50% | **100%** | **100%** |
| text_processing | 83% | **100%** | **100%** | 83% |

#### Failure Analysis

| Task | Haiku 4.5 | Sonnet 4.6 | Opus 4.6 | GPT-5.2 | Root Cause |
|------|-----------|------------|----------|---------|------------|
| text_heredoc_config | FAIL | - | - | FAIL | bashkit heredoc redirect bug — `cat <<EOF > file` outputs to stdout (#345) |
| pipe_xargs_batch | FAIL | - | - | FAIL | bashkit xargs doesn't execute commands (#346) |
| search_find_replace | FAIL | - | - | FAIL | bashkit `$()` word splitting broken in for-loops (#347) |
| script_function_lib | FAIL | - | - | FAIL | bashkit `tr` character class bug |
| file_path_organizer | FAIL | - | - | FAIL | Model burns turns on edge cases, deletes own work |
| script_getopts_parser | FAIL | - | - | FAIL | getopts/wc interaction — wrong output |
| script_assoc_array | FAIL | - | - | FAIL | Associative array iteration format mismatch |
| env_source_export | FAIL | - | - | FAIL | Source/export propagation incomplete |
| data_regex_extract | FAIL | - | - | PASS | Haiku BASH_REMATCH extraction off-by-one |
| archive_selective | PASS | FAIL | - | FAIL | tar extraction content mismatch |
| sysinfo_env_report | PASS | FAIL | - | PASS | Sonnet env output format |
| complex_diff_report | PASS | FAIL | - | PASS | Sonnet diff report format |
| pipe_process_sub | PASS | - | - | FAIL | GPT process substitution approach |
| pipe_dedup_merge | PASS | - | - | FAIL | GPT misses entries from second file |
| complex_todo_app | PASS | - | - | FAIL | GPT exit code 2 on final call |
| complex_release_notes | PASS | - | - | FAIL | GPT exhausts turn budget |

#### New Interpreter Bugs Surfaced

| Bug | Issue | Affected Tasks |
|-----|-------|---------------|
| Heredoc redirect to file (`cat <<EOF > file`) | #345 | text_heredoc_config |
| xargs doesn't execute commands | #346 | pipe_xargs_batch |
| Word splitting on `$()` in for-loops | #347 | search_find_replace |

#### Model Behavior

- **Opus 4.6** perfect 23/23 (100%) on the tasks it ran — strong across all original categories
- **Sonnet 4.6** first eval for this model, 23/26 (94%) — new failures on archive_selective and sysinfo_env_report
- **Haiku 4.5** best on full dataset at 43/52 (92%) — new tasks expose bashkit interpreter gaps more than model gaps
- **GPT-5.2** 32/52 (79%) — weakest on pipelines (40%) and scripting (43%), struggles with bash-specific patterns

### Previous Results

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
