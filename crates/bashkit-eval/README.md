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

25 hand-curated tasks in JSONL format across 10 categories: file_operations, text_processing, pipelines, scripting, data_transformation, error_recovery, system_info, archive_operations, jq_mastery, complex_tasks.

Smoke test dataset (`data/smoke-test.jsonl`) has 3 tasks for quick verification.

## Results

### 2026-02-08 — Multi-Model Comparison (latest)

| Metric | Haiku 4.5 | Opus 4.6 | GPT-5.2 |
|--------|-----------|----------|---------|
| Tasks passed | 23/25 | 21/25 | 18/25 |
| Score | **98%** | 93% | 81% |
| Tool calls | 93 (81 ok, 12 err) | 143 (125 ok, 18 err) | 103 (80 ok, 23 err) |
| Tool call success | **87%** | **87%** | 78% |
| Tokens | 167K in / 19K out | 242K in / 26K out | 84K in / 10K out |
| Duration | 2.9 min | 8.7 min | 3.4 min |

#### Per-Category Comparison

| Category | Opus 4.6 | Haiku 4.5 | GPT-5.2 |
|----------|----------|-----------|---------|
| archive_operations | 100% | 100% | 50% |
| complex_tasks | 69% | 100% | 88% |
| data_transformation | 94% | 100% | 62% |
| error_recovery | 100% | 100% | 86% |
| file_operations | 100% | 94% | 100% |
| jq_mastery | 100% | 100% | 100% |
| pipelines | 100% | 100% | 80% |
| scripting | 93% | 93% | 53% |
| system_info | 100% | 100% | 100% |
| text_processing | 100% | 100% | 100% |

#### Impact of Interpreter Fixes

Tool call success (how often bashkit executes what models generate) improved significantly after recent fixes:

| Model | Before | After | Delta |
|-------|--------|-------|-------|
| Claude Opus 4.6 | 79% | 87% | **+8%** |
| Claude Haiku 4.5 | 77% | 87% | **+10%** |
| GPT-5.2 | 59% | 78% | **+19%** |

Key fixes: `date -d` compound expressions/quote stripping (eliminated 10k command limit exhaustion), awk field math.

#### Remaining Bashkit Gaps

Failures that occur across all models (interpreter limitations, not model quality):

| Gap | Impact | Example |
|-----|--------|---------|
| Compound commands in pipelines | ~6 errors | `cmd \| while read line; do ... done` |
| Awk associative arrays | ~9 errors | `arr[$key]=$val` |
| Heredoc-to-file redirect | ~10 errors | `cat > file <<'EOF'` writes to stdout instead |
| `source`/`.` function loading | ~5 errors | Functions from sourced files not in caller scope |
| `chmod` symbolic modes | ~6 errors | `chmod +x file` → "invalid mode" |
| Parser fuel / `[[ ]]` | ~25 errors | Complex conditionals exhaust parser budget |

#### Model Behavior

- **Claude models** adapt when bashkit rejects a command — retry with simpler constructs (e.g., `[[ ]]` → `[ ]`, pipelines → temp files)
- **GPT-5.2** tends to repeat failing patterns, leading to lower tool success despite fewer total calls
- **Haiku 4.5** best score/cost ratio — fewer tokens, faster, highest pass rate

#### Baseline (2026-02-07, pre-fixes)

<details>
<summary>Previous results before interpreter improvements</summary>

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
