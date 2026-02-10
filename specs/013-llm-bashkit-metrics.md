# Spec 013: LLM-Bashkit Metrics

## Purpose

Define the metrics that `bashkit-eval` produces when measuring how LLMs use Bashkit. Three audiences: library consumers evaluating Bashkit for their agent stack, model developers benchmarking bash tool-use, Bashkit maintainers identifying interpreter gaps.

## Data Model

Metrics live at four granularity levels, each represented by a struct in `crates/bashkit-eval/src/`.

### Per-Call: `ToolCallResult` (agent.rs)

One record per bash invocation within a task.

| Field | Type | Source |
|-------|------|--------|
| `commands` | String | Bash command text sent by LLM |
| `stdout` | String | Standard output from execution |
| `stderr` | String | Standard error from execution |
| `exit_code` | i32 | 0 = success, >0 = error |

### Per-Task: `AgentTrace` (agent.rs) + `TaskScore` (scorer.rs)

One record per eval task. `AgentTrace` captures execution; `TaskScore` captures correctness.

**AgentTrace:**

| Field | Type | Computation |
|-------|------|-------------|
| `messages` | Vec\<Message\> | Full LLM conversation history |
| `tool_calls` | Vec\<ToolCallResult\> | All bash invocations |
| `tool_call_count` | usize | `tool_calls.len()` |
| `turns` | usize | Number of `provider.chat()` calls |
| `last_tool_response` | Option\<ToolCallResult\> | Most recent call (used by `exit_code` check) |
| `natural_stop` | bool | `true` if LLM sent stop token; `false` if hit turn limit |
| `total_input_tokens` | u32 | Sum of input tokens across all turns |
| `total_output_tokens` | u32 | Sum of output tokens across all turns |
| `duration_ms` | u64 | Wall-clock milliseconds for entire task |

**TaskScore:**

| Field | Type | Computation |
|-------|------|-------------|
| `task_id` | String | Task identifier from dataset |
| `results` | Vec\<ScoreResult\> | One per expectation check |
| `score` | f64 | Sum of weight for each passed check |
| `max_score` | f64 | Sum of all check weights |

Derived methods: `all_passed()` = every check passed. `rate()` = `score / max_score`.

**ScoreResult** (one per expectation check):

| Field | Type | Meaning |
|-------|------|---------|
| `check` | String | Check string (e.g., `exit_code:0`) |
| `passed` | bool | Whether this expectation was met |
| `detail` | String | Human-readable explanation |
| `weight` | f64 | Weight in task score (default 1.0) |

### Per-Category: `CategorySummary` (report.rs)

Aggregated from all tasks sharing a `category` field.

| Field | Type | Computation |
|-------|------|-------------|
| `tasks` | usize | Count of tasks in category |
| `passed` | usize | Count where `all_passed() == true` |
| `score` | f64 | Sum of `task.score` |
| `max_score` | f64 | Sum of `task.max_score` |
| `rate` | f64 | `score / max_score` |

### Per-Run: `EvalSummary` (report.rs)

Aggregated from all tasks in one eval run. Built by `build_report()`.

**Task completion:**

| Field | Type | Computation |
|-------|------|-------------|
| `total_tasks` | usize | `results.len()` |
| `total_passed` | usize | Count where `all_passed()` |
| `total_score` | f64 | Sum of all `task.score` |
| `total_max_score` | f64 | Sum of all `task.max_score` |
| `overall_rate` | f64 | `total_score / total_max_score` |

**Tool execution:**

| Field | Type | Computation |
|-------|------|-------------|
| `total_tool_calls` | usize | Sum of `trace.tool_call_count` |
| `tool_calls_ok` | usize | Count of calls where `exit_code == 0` |
| `tool_calls_error` | usize | `total_tool_calls - tool_calls_ok` |
| `tool_call_success_rate` | f64 | `tool_calls_ok / total_tool_calls` |

**Reasoning efficiency:**

| Field | Type | Computation |
|-------|------|-------------|
| `total_turns` | usize | Sum of `trace.turns` |
| `avg_turns_per_task` | f64 | `total_turns / total_tasks` |
| `avg_tool_calls_per_task` | f64 | `total_tool_calls / total_tasks` |

**Token economics:**

| Field | Type | Computation |
|-------|------|-------------|
| `total_input_tokens` | u32 | Sum of `trace.total_input_tokens` |
| `total_output_tokens` | u32 | Sum of `trace.total_output_tokens` |

**Latency:**

| Field | Type | Computation |
|-------|------|-------------|
| `total_duration_ms` | u64 | Sum of `trace.duration_ms` |
| `avg_duration_ms` | f64 | `total_duration_ms / total_tasks` |

**Category breakdown:**

| Field | Type | Computation |
|-------|------|-------------|
| `by_category` | HashMap\<String, CategorySummary\> | Group tasks by `task.category` |

## Computation Flow

```
JSONL dataset
  │
  ▼  [per task]
run_agent_loop(bash, provider, task)
  │  ├─ Each provider.chat() call increments turns
  │  ├─ Each tool_use block → bash.exec() → ToolCallResult
  │  ├─ Token counts from provider response usage
  │  └─ Wall-clock timer wraps entire loop
  │
  ▼
AgentTrace { tool_calls, turns, tokens, duration, natural_stop }
  │
  ▼
score_task(trace, vfs, expectations)
  │  ├─ Each expectation → evaluate_check() → ScoreResult
  │  ├─ Checks read from trace (stdout, stderr, exit_code, tool_call_count)
  │  └─ Checks read from VFS (file_exists, dir_exists, file_contains)
  │
  ▼
TaskScore { results, score, max_score }
  │
  ▼  [collect all tasks]
build_report(results)
  │  ├─ Sum/count across all tasks → EvalSummary
  │  └─ Group by category → HashMap<String, CategorySummary>
  │
  ▼
EvalReport { summary, results, metadata }
  │
  ├─ print_terminal_report()  [always]
  └─ save_report()            [--save flag]
       ├─ JSON (full struct with traces)
       └─ Markdown (human-readable report)
```

## Expectation Checks

Checks validate against two data sources: **agent trace** (runtime behavior) and **VFS** (persistent artifacts).

| Check | Format | Data Source | Validates |
|-------|--------|------------|-----------|
| `exit_code` | `exit_code:N` | `trace.last_tool_response.exit_code` | Last call exit code equals N |
| `stdout_contains` | `stdout_contains:text` | `trace.tool_calls[].stdout` | Any call's stdout contains text |
| `stdout_regex` | `stdout_regex:pattern` | `trace.tool_calls[].stdout` | Any call's stdout matches regex |
| `stderr_empty` | `stderr_empty` | `trace.tool_calls[].stderr` | Every call has empty stderr |
| `file_exists` | `file_exists:/path` | `vfs.stat(path)` | Path exists post-execution |
| `dir_exists` | `dir_exists:/path` | `vfs.stat(path) + FileType::Directory` | Directory exists post-execution |
| `file_contains` | `file_contains:/path:text` | `vfs.read_file(path)` | File content includes text |
| `tool_calls_min` | `tool_calls_min:N` | `trace.tool_call_count` | At least N calls made |
| `tool_calls_max` | `tool_calls_max:N` | `trace.tool_call_count` | At most N calls made |
| `llm_judge` | `llm_judge:prompt` | — | Stub (weight=0, always passes) |

Dual-source validation catches models that produce correct stdout but skip writing to disk (or vice versa).

## Task Categories

37 tasks across 10 categories. Category-level metrics reveal model strengths and Bashkit coverage gaps.

| Category | Tasks | What it tests |
|----------|-------|--------------|
| file_operations | 3 | mkdir, cp, mv, find, rm — basic VFS interaction |
| text_processing | 4 | grep, sed, awk on structured data |
| pipelines | 3 | Multi-stage pipes, command substitution |
| scripting | 4 | Variables, arrays, loops, functions |
| data_transformation | 5 | CSV↔JSON, log parsing, column reordering |
| error_recovery | 2 | Missing files, broken JSON — graceful failure |
| system_info | 2 | whoami, date, env — sandbox introspection |
| archive_operations | 2 | tar, gzip workflows |
| json_processing | 8 | jq queries, transforms, merges, NDJSON |
| complex_tasks | 4 | Multi-step scenarios combining categories |

## Key Metrics Explained

### Overall Rate vs. Pass Rate

**Overall rate** (`overall_rate`): Weighted score across all checks. A task achieving 4/5 checks contributes 80% of its weight. Soft metric — measures partial progress.

**Pass rate** (`total_passed / total_tasks`): Strict binary. A task passes only when every check passes. Measures complete task achievement.

Both matter. A model at 95% overall rate but 78% pass rate succeeds partially on many tasks but misses full completion. A model at 87% overall with 87% pass rate either fully succeeds or clearly fails.

### Tool Call Success Rate

`tool_calls_ok / total_tool_calls`. The sharpest signal for Bashkit compatibility.

When a model issues bash and gets exit_code != 0, either:
- Model wrote buggy bash (model problem)
- Bashkit doesn't support the syntax/builtin (interpreter gap)

Comparing across models disambiguates: if all models fail on the same commands, it's Bashkit. If one model fails and others don't, it's the model.

### Natural Stop

`trace.natural_stop == true` means the LLM decided it was done. `false` means it hit the turn limit (default 10). A high rate of `natural_stop == false` signals models getting stuck in retry loops — burning tokens without converging.

### Turns vs. Tool Calls

A turn is one `provider.chat()` round-trip. A turn may produce multiple `tool_use` blocks (batch execution). Turns measure LLM reasoning steps; tool calls measure interpreter utilization. Fewer turns at the same pass rate = better planning.

## Current Baseline (2026-02-09, 37 tasks)

| Metric | Haiku 4.5 | Opus 4.6 | GPT-5.2 |
|--------|-----------|----------|---------|
| Tasks passed | 32/37 | 29/37 | 23/37 |
| Overall rate | **95%** | 87% | 80% |
| Tool calls (ok/err) | 150 (121/29) | 198 (163/35) | 108 (77/31) |
| Tool call success | 81% | **82%** | 71% |
| Avg turns/task | 4.8 | 5.6 | 3.8 |
| Tokens (in/out) | 286K/35K | 315K/31K | 119K/17K |
| Duration | 6.4 min | 25.2 min | 4.8 min |

### Per-Category Rates

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

### Trend (25→37 tasks, interpreter fixes)

| Metric | 2026-02-07 (baseline) | 2026-02-08 (fixes) | 2026-02-09 (expanded) |
|--------|----------------------|--------------------|-----------------------|
| Haiku pass rate | 92% | 98% | 95% |
| Opus pass rate | 87% | 93% | 87% |
| GPT-5.2 pass rate | 87% | 81% | 80% |
| Haiku tool success | 80% | 87% | 81% |
| GPT-5.2 tool success | 57% | 78% | 71% |

Interpreter fixes between 02-07 and 02-08 lifted tool call success rates significantly (+7% Haiku, +21% GPT-5.2). Expanded dataset on 02-09 revealed new friction points, pulling rates back down slightly.

## Interpreting Results

### Healthy Profile
- Overall rate > 90%
- Tool call success rate > 80%
- Avg turns per task < 5
- No category below 70%
- `natural_stop` on most tasks

### Friction Signals
- Tool call success rate < 75% → Bashkit compatibility gaps
- Single category below 50% → Missing builtins or syntax for that domain
- Avg turns per task > 7 → Models retrying due to errors
- High token usage with low pass rate → Stuck in retry loops
- `natural_stop == false` frequently → Turn limit reached, model didn't converge

### Cross-Model Diagnostics

| Pattern | Diagnosis |
|---------|-----------|
| All models fail same task, similar stderr | Bashkit limitation |
| One model passes, others fail | Model capability difference |
| Same pass rate, different tool call counts | Reasoning efficiency gap |
| Same pass rate, different token counts | Cost efficiency gap |
| Category fails for all models | Bashkit missing builtins for domain |

### Improvement Levers
- **Raise tool call success rate** → Fix interpreter gaps (benefits all models)
- **Add expectations to underspecified tasks** → Better scoring signal
- **Add tasks for uncovered builtins** → Broader coverage
- **Compare across Bashkit versions** → Track regression/improvement

## See Also

- [009-tool-contract.md](009-tool-contract.md) — Tool trait that LLMs interact with
- [012-eval.md](012-eval.md) — Eval harness architecture and dataset format
- [009-implementation-status.md](009-implementation-status.md) — Feature coverage
