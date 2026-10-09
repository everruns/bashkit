---
type: Subsystem Design
title: Evaluation Framework
description: LLM evaluation study design, dataset format, execution, and scoring.
tags:
  - bashkit
  - eval
  - llm
---

# bashkit-eval: mira Eval Study

## Status

Implemented (reimplemented on the [mira](https://github.com/everruns/mira) eval
framework; supersedes the original hand-rolled harness).

## Purpose

Evaluate how well LLM models use bashkit's bash tool in agentic workloads.
Measure model capability across bash feature categories, identify bashkit
compatibility gaps, and drive improvement.

## Architecture

`bashkit-eval` is a **mira study**: a binary that advertises its evals to the
`mira` host CLI over stdio. Mira owns the model matrix, scheduling, retries,
resume, and reporting (JSON/JUnit/Markdown/HTML). bashkit supplies the
subject-under-test and the scoring.

```
mira host CLI ──spawns──▶ bashkit-eval (study binary)
   │                          │
   │  model matrix            ├─ Sample   (one per JSONL task)
   │  scheduling/retries      ├─ Subject  (bashkit agent loop)
   │  reporting               └─ Scorer   (bashkit expectation checks)
```

Three pieces wire bashkit into mira (`src/mira_study.rs`):

1. **Samples**: each JSONL `EvalTask` / `ScriptingEvalTask` becomes a mira
   `Sample`. The full task rides in `sample.metadata["task"]` (the subject's
   source of truth); its `expectations` array in
   `sample.metadata["expectations"]` (the scorer's source of truth). Datasets
   are embedded via `include_str!` so there is no runtime path dependence.
2. **Subject**: `bash_subject` / `scripting_subject` run bashkit's existing
   agent loop against the case's target model (`cx.target.{provider,model}`),
   then pack the result into a mira `Transcript`.
3. **Scorer**: `expectations_scorer` replays the deterministic bashkit checks
   against the Transcript. A case passes iff **every** check passes (mirrors the
   original `TaskScore::all_passed`); the score value is the weighted pass rate.

### Key Design Decisions

1. **In-process Subject (mira "Path A"), not `mira-everruns`**: bashkit keeps
   its own provider stack (Anthropic Messages, OpenAI Chat Completions, OpenAI
   Responses) and agent loop. The study depends only on `mira-eval` (no
   `mira-everruns`/`everruns-runtime`), keeping the dependency tree small.
2. **`Bash` directly, not `BashTool`**: `BashTool::execute()` creates a fresh
   interpreter per call (no VFS persistence). The agent loop needs a persistent
   VFS across turns. `BashTool::builder()` is used only for
   `input_schema()` / `system_prompt()` introspection.
3. **One `Bash` per task**: fresh instance per sample; VFS persists across all
   tool calls within the task; the snapshot is taken after the loop; the
   instance is dropped after.
4. **Pre-populated VFS**: task `files: {}` map → `Bash::builder().mount_text`.
5. **Snapshot, not a live filesystem, is the scoring substrate**: a mira
   `Scorer` only sees `&Sample` + `&Transcript`. After the run, the subject
   walks the VFS into `transcript.files` (path → contents) and records a
   `Snapshot` (tool-call stdout/stderr/exit codes + directory set) in
   `transcript.metadata["bashkit"]`. The checks read those. See `src/snapshot.rs`.
6. **Model matrix via mira `Target`s**: targets are gated on their provider's
   API-key env var, so an offline run skips them all (CI stays green) and a
   keyed run lights up the subset whose credentials are present.
7. **No bespoke runner/report**: mira provides run orchestration, persistence
   (`./results/<run_id>/`), and reports. The original `runner.rs`, `report.rs`,
   and scripting equivalents are gone.

## Dataset Format (JSONL)

Unchanged from the original harness. One JSON object per line:

```json
{
  "id": "file_ops_01",
  "category": "file_operations",
  "description": "Create nested directory structure",
  "system": null,
  "prompt": "Create /project with src/ and tests/ subdirectories",
  "files": {"/data/input.txt": "hello world"},
  "expectations": [
    {"check": "dir_exists:/project/src", "weight": 1.0},
    {"check": "exit_code:0"}
  ]
}
```

`system` = optional system-message override (null = BashTool default);
`expectations` = checks with optional weight (default 1.0);
`setup` = optional shell script run after `files` are mounted and before the
agent starts (build a fixture git repo, create symlinks, `chmod +x`). It runs
in a subshell with `set -e`, its output is not a tool call, and a non-zero exit
is an infra error. The eval `Bash` has the sandboxed `git` builtin enabled
(author `Eval Agent <eval@bashkit-eval.invalid>`, TM-GIT-002). Both the agent
loop and the reference-solution test build the starting state with
`agent::build_task_bash`.

## Expectation Check Types

`exit_code:N`, `stdout_contains:text`, `stdout_regex:pattern`,
`stderr_empty`, `file_exists:/path`, `dir_exists:/path`,
`file_contains:/path:text`, `file_not_contains:/path:text` (file must exist),
`file_line_regex:/path:pattern`, `file_equals:/path:content` (golden file:
whole content equal, only trailing newlines forgiven; added for `bashkit_hard`),
`symlink:/path:target` (`/path` is a symlink;
its target resolves lexically to the same absolute path as `target`, so
`releases/v2` and `/abs/releases/v2` both pass and a copied directory fails),
`llm_judge:prompt` (stub, weight forced to 0). Semantics in
`crates/bashkit-eval/src/checks.rs` (ported verbatim from the original scorer,
with byte-for-byte the same pass/fail logic).

## Providers

Implemented under `src/provider/`, selected by the mira target's `provider` id:

- **Anthropic Messages API**: target `Target::anthropic(model)`; `ANTHROPIC_API_KEY`.
- **OpenAI Chat Completions**: target `Target::openai(model)`; `OPENAI_API_KEY`.
- **OpenRouter** (same Chat Completions wire format, second endpoint on
  `OpenAiProvider`): target `Target::cloud("openrouter", "<vendor>/<model>",
  "OPENROUTER_API_KEY")`, label `openrouter/moonshotai/kimi-k3`. Used for
  models bashkit has no provider for (Kimi, Meta Muse) and for Gemini, whose
  own host is unreachable from the cloud eval environment. Added 2026-10-09.
- **OpenAI Responses API**: target `Target::cloud("openresponses", model,
  "OPENAI_API_KEY")`. Required for codex models (e.g. `gpt-5.3-codex`);
  multi-turn via manual input chaining; sets `reasoning.effort: "high"` for
  codex models automatically.

## Evals

Five evals are advertised (`#[eval]` wrappers in `src/main.rs`):

| Eval | Samples | Notes |
|------|---------|-------|
| `bashkit_bash` | 58 tasks across 15 categories | Samples tagged by category; select with `--tag <category>` |
| `bashkit_smoke` | 3 tasks | Quick verification |
| `bashkit_repo` | 8 `repo_workflow` tasks | Multi-turn fixture repos; 20-turn budget (`REPO_MAX_TURNS`) |
| `bashkit_hard` | 10 hard tasks, 9 categories | Built not to saturate; 25-turn budget (`HARD_MAX_TURNS`) |
| `bashkit_scripting` | scripting-tool tasks | `mode` axis: `scripted` vs `baseline` |

## CLI

Run through the `mira` host (install via `cargo install mira-cli`):

```
mira --bin bashkit-eval list
mira --bin bashkit-eval run bashkit_bash
mira --bin bashkit-eval run bashkit_bash --targets anthropic/claude-opus-5-5 --tag json_processing
mira --bin bashkit-eval run bashkit_scripting --axis mode=scripted
mira --bin bashkit-eval run --format html --out report.html
mira --bin bashkit-eval run --resume <run_id>
```

`just eval`, `just eval-smoke`, `just eval-repo`, `just eval-hard`, `just eval-scripting`, and
`just eval-list` wrap these.

## Output / Metrics

Mira owns output (run folder under `./results/<run_id>/`, plus
JSON/JUnit/Markdown/HTML). The subject records operational telemetry on the
`Transcript` so mira surfaces it:

- **Score**: weighted pass rate of the `bashkit_expectations` scorer; pass iff
  all checks pass.
- **Usage**: input/output tokens (`transcript.usage`).
- **Timing**: wall-clock (`transcript.timing.duration_ms`).
- **Metrics** (`transcript.metrics`, open vocabulary): `turns`, `tool_calls`,
  `tool_calls_ok`, `tool_calls_err`, `natural_stop`. Scripting adds
  `baseline`, `inner_commands`, `inner_tool`, `inner_help`, `inner_discover`,
  `raw_tool_output_bytes`, `tool_output_sent_bytes`.

Low `tool_calls_ok / tool_calls` indicates bashkit compatibility gaps or invalid
model commands.

## Dataset Categories

Datasets live in `crates/bashkit-eval/data/`. Categories span file operations,
text processing, pipelines, scripting, data transformation, error recovery,
system info, archives, JSON processing, complex multi-step tasks, code search,
and environment handling, each with task-appropriate pre-populated seed files.

## Repo Workflow Eval (`bashkit_repo`)

Roadmap step "harder evals": repo-shaped, multi-turn tasks in
`data/repo-workflow.jsonl` (category `repo_workflow`). Each task's `setup`
turns its `files` into a git repo (`git init` + initial commit), then the model
explores, runs `make test`, fixes code/data/config, re-runs, and commits. The
tasks exercise the features agents combine in real repos, together:

| Task | Exercises |
|------|-----------|
| `repo_clone_fix_failing_test` | local `git clone`, `make test`, bug fix, commit, upstream untouched |
| `repo_symlink_release_switch` | `ln -sfn` on a link to a directory, config fix, `git add -A` |
| `repo_path_bin_tool_fix` | `export PATH=$PWD/bin:$PATH`, exec bits kept across `sed -i` |
| `repo_parallel_shards_background` | `cmd &`, `$!`, per-job `wait PID` exit codes, data fix, `make` target |
| `repo_report_sort_regression` | awk/sort pipeline vs golden file, `git commit -am` |
| `repo_jq_config_validation` | jq-driven validator, typed JSON edits |
| `repo_release_branch_bump` | `git checkout -b`, version bump across VERSION/package.json/CHANGELOG |
| `repo_parallel_runner_masks_failure` | runner that `wait`s without a PID hides failures; fix it, then the bug |

Python-dependent tasks are out of scope (bash, awk, sed, jq, make only).
Checks are deterministic: final file contents, `symlink:`, `.git/HEAD`, and
`.git/commits` lines (`hash|author|email|ts|message`, the sandboxed git's
storage format) for "a commit with message X exists" and "history kept". Each
task also checks the model did not edit tests/golden files instead of code.

**Reference solutions** (`data/repo-workflow-solutions.jsonl`, `src/reference.rs`):
one list of bash tool calls per task, replayed on the exact starting `Bash`
(`build_task_bash`) and scored by the same checks as a model run. `cargo test
-p bashkit-eval` requires every reference to pass all checks and the
untouched fixture (`make test; git status` only) to fail, so CI keeps every
task solvable and non-trivial without an LLM. Every reference was also run
against real bash 5.2 + git and gives the same file results. Building the
references found and fixed five bashkit gaps: `ln -n`/`-T`/directory
destinations, `mkdir`/`ln -s` under a symlinked directory, mode loss when
`sed -i` renames over a `mount_text` file, `git add -A` / `commit -am`, and
local-path `git clone` (see [Git Support](../integrations/git-support.md)).

`bashkit-replay` looks up mira tasks in `eval-tasks.jsonl` only and runs no
`setup`, so `bashkit_repo` runs are not in the gap corpus yet.

## Hard Eval (`bashkit_hard`)

Added 2026-10-09 when `bashkit_bash` saturated (top models 55/58). Ten tasks in
`data/hard-tasks.jsonl`, aimed at a 30-50% failure rate for frontier models,
still deterministic and solvable with bash, coreutils, awk, sed, jq, make and
the sandboxed git (no python, no network). Same subject, scorer and
`setup`-built starting state as `bashkit_repo`; 25-turn budget
(`HARD_MAX_TURNS`). Category = `--tag`.

| Task | Category | Why it is hard |
|------|----------|----------------|
| `hard_multi_bug_inventory` | `multi_bug_repo` | fail-fast runner shows one failing test at a time; 4 interacting bugs (`%d` cents, quoted CSV name with a comma, last line without newline, `[[ < ]]` string compare); tests/data frozen |
| `hard_quoting_collect` | `quoting` | file mover vs names with spaces, `[`, `*`, `?`, leading `--`; `ls`-parsing, empty-glob, directory named `*.txt`, cumulative sorted INDEX |
| `hard_pipefail_masking` | `error_handling` | `set -euo pipefail` defeated by `local x=$(..)`, `if ! f`, `\|\| echo`, truncating `> dist/..` in a failing pipeline; then a hidden `cut -f3,2` field-order bug |
| `hard_csv_reconcile` | `data_reconciliation` | RFC 4180 quotes/`""`, thousands separators, DD/MM vs ISO dates, ref normalisation, duplicate rows, exact-cent sums (`300.1*100` float trap), golden report |
| `hard_log_p95` | `log_analysis` | 3000-line generated log; query/trailing-slash normalisation, exclusions, malformed and `-` latency lines, nearest-rank p50/p95, 3-key ordering with real ties |
| `hard_jq_rollup` | `json_processing` | null vs missing customer/discount, floor math, latest-name lookup, per-month object, stable multi-key sorts, one pretty file per customer |
| `hard_rename_refactor` | `refactoring` | rename `get_cfg` but not `get_cfg_path`/`get_cfg_or`/`forget_cfg`, not frozen messages, yes inside `"$(..)"`, assoc-array dispatch values and `command -v` |
| `hard_makefile_deps` | `build_system` | stale output from missing prerequisites (awk scripts, map file) and a hardcoded source list; `$^` trap; checked with `make -q` + `touch -d` on a scratch copy |
| `hard_trap_publish` | `error_handling` | exit-code contract (2/3/4), EXIT trap reading a function `local`, overwritten trap, `exit 0` masking, `[a-z_]*=*` glob accepting `max conns=5`, last line without newline |
| `hard_toposort` | `algorithms` | deterministic Kahn order (smallest ready first, not `tsort` order), messy input, deps-only nodes, cycle report listing every blocked package |

Golden outputs (reports, jq files, orders) are computed independently by the
generator from the task inputs, not from the reference solution, and checked
with `file_equals`. Repo tasks also pin tests/data with `file_equals` so
editing a test instead of the code fails, and check `.git/commits`. Test
runners write intermediate values to logs (e.g. `total=53.13`) that are
checked too, so writing `PASS` into the report by hand does not score.

**Reference solutions** (`data/hard-tasks-solutions.jsonl`) run in `cargo test
-p bashkit-eval` with the repo set (`reference.rs` iterates both datasets:
references pass, untouched fixtures fail). Every reference was also replayed
under real bash 5.2 (GNU make 4.3, jq 1.7, mawk, git stubbed out) with the
same file results. Building the set found and fixed four bashkit gaps:
`set -e` inside a child `bash script` was disabled when the caller ran it in
an `if`/`||`/`!` context (`if bash test.sh` never saw test.sh fail),
`dirname -- PATH` treated `--` as an operand, `ls -A` was rejected, and
`stat -c %Y` printed `%Y` literally. Open gaps (tasks avoid them): `ls -a`
omits `.` and `..`, `touch -d 'YYYY-MM-DD HH:MM:SS UTC'` is rejected (no
zone suffix; ISO `...Z` and `@epoch` work), and under `set -eu` an unbound variable in a subshell's
EXIT trap leaves the subshell's status 0 (bash: 1).

## Scripting-Tool Eval

A second eval (`bashkit_scripting`) tests `ScriptedTool` orchestration (see
[Scripted Tool Orchestration](../integrations/scripted-tool-orchestration.md)), measuring how well LLMs orchestrate
multiple mock tools via bash scripts vs. calling each tool individually.

### Modes (the `mode` axis)

- **scripted**: all mock tools composed into one `ScriptedTool`; the LLM writes
  bash scripts. Measures tool-composition effectiveness.
- **baseline**: each mock tool exposed as a separate LLM tool; the control.

### Dataset Format

Same JSONL plus per-task `tools` and `discovery_mode`:

```json
{
  "id": "mt-ecommerce",
  "category": "many_tools",
  "prompt": "Look up user 42 and summarize their last order",
  "discovery_mode": false,
  "tools": [
    {
      "name": "get_user",
      "description": "Fetch user by ID",
      "schema": {"type": "object", "properties": {"id": {"type": "integer"}}},
      "tags": ["read", "users"],
      "category": "users",
      "mock": {"param": "id", "responses": {"42": "{\"name\": \"Jane\"}"}}
    }
  ],
  "expectations": [{"check": "stdout_contains:Jane"}]
}
```

Mock behaviors: **Static** (`"mock": "fixed string"`) or **ByParam**
(`{"param": "key", "responses": {...}, "default": "fallback"}`). `tags` /
`category` feed `discover` filtering. `discovery_mode: true` uses
`ScriptingToolSet::with_discovery()`: tool names hidden from the system prompt;
the LLM must use the `discover`/`help` builtins. Scripting tasks score against
mock-tool stdout (no VFS file checks).

Datasets: `crates/bashkit-eval/data/scripting-tool/`, `large-output.jsonl`,
`many-tools.jsonl` (15–20 tools), `paginated.jsonl`, `discovery.jsonl`.

## Gap Telemetry

`bashkit-replay` (bin in `crates/bashkit-eval`, `just gaps`) turns the stored
eval runs into a replay corpus. It loads every recorded bash tool call from
`results/eval-*.json` (task files embedded) and `results/mira/*/cases/*/result.json`
(task files looked up in `data/eval-tasks.jsonl`; samples no longer in the
dataset are skipped), replays each session in order on a fresh `Bash` built
like the eval agent's (`eval`@`bashkit-eval`, task files mounted, 10 s per
call), and classifies stderr lines into gaps:

| Kind | Matches | Key |
|------|---------|-----|
| missing command | `NAME: command not found` | `NAME` |
| unknown option | `CMD: unrecognized/invalid/unknown/illegal option/predicate ... X` | `CMD X` |
| parse error | `parse error` / `syntax error` | message without location prefixes, digits as `N` |
| unsupported | `CMD: ... not supported / not implemented / unsupported` | `CMD` |

Each gap counts at most once per call, in the recorded stderr and in today's
replay. The report (`results/gaps/gaps-<UTC timestamp>.{json,md}`) ranks the
top 20 gaps by today's hits, lists gaps fixed since recording, and counts
calls whose stdout, stderr and exit code are unchanged. `/benches` shows the
latest top 20 and the gap-call history (see [Performance Results](performance-results.md)).
The ranking is what orders coverage work; it needs no model and no network,
so rerun it after each fidelity change. Agent probes for host tools
(`busybox`, `gawk`, `sudo`, `python3`) rank too: they are real turns lost even
when the answer is a deliberate refusal.

## Non-Goals

- No bespoke concurrency / scheduling, mira owns it.
- No cost guardrails (mira budget scorers can be added if desired).
- No comparison against real bash.
- No streaming.
- No retries on LLM content errors. The providers retry only *transient* errors
  (rate-limit 429s, 5xx, Anthropic 529) with exponential backoff, and **fast-fail
  on permanent errors**: `insufficient_quota` / billing limits / auth (401/403),
  so an exhausted account errors immediately instead of hanging in a retry storm.
  All provider HTTP requests use a connect (15s) + total (300s) timeout so a
  single call can never stall a run. Provider/agent failures surface as
  `Transcript::infra_error` → the case scores N/A, not a model failure. mira adds
  its own bounded retry layer (`--max-retries`, default 4).
