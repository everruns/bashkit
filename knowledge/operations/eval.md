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
`mode` = `agent` (default; bash as the model's tool) or `runtime` (bashkit as
the execution runtime: python3, sqlite3, state across calls);
`difficulty` = `basic` (default), `repo` or `hard`; `max_turns` = optional
per-task agent-turn budget (default `MAX_TURNS` = 10). `category`, `mode` and
`difficulty` become mira sample tags (`--tag runtime`, `--tag hard`, ...);
`verify` = optional hidden shell script run after the agent finishes (see
[Runtime Tasks](#runtime-tasks-moderuntime));
`setup` = optional shell script run after `files` are mounted and before the
agent starts (build a fixture git repo, create symlinks, `chmod +x`). It runs
in a subshell with `set -e`, its output is not a tool call, and a non-zero exit
is an infra error. The eval `Bash` has the sandboxed `git` builtin enabled
(author `Eval Agent <eval@bashkit-eval.invalid>`, TM-GIT-002), real CPython
3.14 as `python`/`python3` (`cpython` feature, `BashBuilder::cpython()`, default
`CPythonLimits`) and `sqlite`/`sqlite3` (`sqlite` feature,
`BASHKIT_ALLOW_INPROCESS_SQLITE=1`), for every task (`agent::with_eval_runtimes`).
Both the agent loop and the reference-solution test build the starting state
with `agent::build_task_bash`. The tool description and system prompt the model
sees come from a `BashTool` registering the same builtins (`agent::eval_tool`),
so they list python3/sqlite3 with their hints instead of the default
"python/python3 not available" warning.

## Expectation Check Types

`exit_code:N`, `stdout_contains:text`, `stdout_regex:pattern`,
`stderr_empty`, `file_exists:/path`, `dir_exists:/path`,
`file_contains:/path:text`, `file_not_contains:/path:text` (file must exist),
`file_line_regex:/path:pattern`, `file_equals:/path:content` (golden file:
whole content equal, only trailing newlines forgiven; added for the hard tasks),
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

Evals advertised (`#[eval]` wrappers in `src/main.rs`):

| Eval | Samples | Notes |
|------|---------|-------|
| `bashkit_bash` | 88 tasks in `data/eval-tasks.jsonl` | One tagged dataset: 58 `basic` agent tasks (15 categories), 8 `repo`, 10 `hard`, 12 `runtime`; select with `--tag <category\|mode\|difficulty>` |
| `bashkit_smoke` | 3 tasks | Quick verification |
| `bashkit_scripting` | scripting-tool tasks | `mode` axis: `scripted` vs `baseline` |
| `bashkit_generate` | 15 one-shot tasks (`basic`/`hard`) | One reply, one script run, no feedback (`just eval-generate`); see "Generate Eval" |

Reference solutions for every non-`basic` or non-`agent` task live in
`data/solutions.jsonl` (`src/reference.rs`, see below). `just eval-repo`,
`just eval-hard` and `just eval-runtime` run the tag slices.

## CLI

Run through the `mira` host (install via `cargo install mira-cli`):

```
mira list --study-bin bashkit-eval
mira run --study-bin bashkit-eval bashkit_bash
mira run --study-bin bashkit-eval bashkit_bash --targets anthropic/claude-opus-5-5 --tag json_processing
mira run --study-bin bashkit-eval bashkit_scripting --axis mode=scripted
mira run --study-bin bashkit-eval --format html --out report.html
mira run --study-bin bashkit-eval --resume <run_id>
```

`just eval`, `just eval-smoke`, `just eval-repo`, `just eval-hard`, `just eval-generate`, `just eval-scripting`, and
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

## Repo Workflow Tasks (`difficulty=repo`)

Roadmap step "harder evals": repo-shaped, multi-turn tasks (category
`repo_workflow`, 20-turn budget via `max_turns`). Each task's `setup`
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

**Reference solutions** (`data/solutions.jsonl`, `src/reference.rs`):
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

`bashkit-replay` runs no `setup` (and no `verify`), so replays of repo, hard
and runtime tasks start from the mounted files only.

## Hard Tasks (`difficulty=hard`, agent)

Added 2026-10-09 when the basic set saturated (top models 55/58). Ten tasks,
aimed at a 30-50% failure rate for frontier models,
still deterministic and solvable with bash, coreutils, awk, sed, jq, make and
the sandboxed git (no python, no network). Same subject, scorer and
`setup`-built starting state as the repo tasks; 25-turn budget
(`max_turns`). Category = `--tag`.

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

**Reference solutions** (`data/solutions.jsonl`) run in `cargo test
-p bashkit-eval` with the repo set (references pass, untouched fixtures fail). Every reference was also replayed
under real bash 5.2 (GNU make 4.3, jq 1.7, mawk, git stubbed out) with the
same file results. Building the set found and fixed four bashkit gaps:
`set -e` inside a child `bash script` was disabled when the caller ran it in
an `if`/`||`/`!` context (`if bash test.sh` never saw test.sh fail),
`dirname -- PATH` treated `--` as an operand, `ls -A` was rejected, and
`stat -c %Y` printed `%Y` literally. Open gaps (tasks avoid them): `ls -a`
omits `.` and `..`, `touch -d 'YYYY-MM-DD HH:MM:SS UTC'` is rejected (no
zone suffix; ISO `...Z` and `@epoch` work), and under `set -eu` an unbound variable in a subshell's
EXIT trap leaves the subshell's status 0 (bash: 1).

## Generate Eval (`bashkit_generate`)

Added 2026-10-09. `bashkit_bash` is an agent loop: the model probes, sees
errors and retries, which hides how often its **first** script fails on
bashkit. `bashkit_generate` measures one-shot generation. Decision: a separate
eval because scoring differs (one script, one run, no feedback); otherwise
evals stay one tagged dataset.

- **Subject** (`src/generate.rs`, `generate_subject`): one provider call with
  no tools offered (the providers omit `tools` when the list is empty, which
  Chat Completions requires), under the fixed `GENERATE_SYSTEM_PROMPT`
  (bashkit virtual bash, the builtin set incl. coreutils/awk/sed/jq/grep/find,
  HOME=/home/eval, no network, no python/node/perl unless stated, answer as
  one ```` ```bash ```` block). The task's `system` field is not used. The
  script runs once via `bash.exec` on `build_task_bash(task)` (same starting
  state as the agent eval: files, git, `setup`), bounded by `SCRIPT_TIMEOUT`
  (60 s; a timeout records exit 124).
- **Extraction rule** (`extract_script`): the first fenced block tagged
  `bash`/`sh`/`shell` (case-insensitive); else the first untagged fenced
  block; else, when the reply has no fence at all, the whole reply trimmed.
  Blocks in other languages are never run; an unclosed fence (truncated
  reply) runs to the end of the text; a closing fence must start a line.
- **Scoring**: the transcript has the same shape as `bash_subject`'s (one
  `ToolOutput` in the `Snapshot`, expectation-relevant VFS files), so the
  shared `expectations_scorer` scores it unchanged.
- **Failure split**: provider/HTTP error or a failing `setup` = infra error
  (N/A). No script (empty reply, empty block, only non-shell fences) = model
  failure: empty snapshot and no files, so every check fails and the score
  is 0. A script that errors, exits non-zero or times out is scored normally.
- **Metrics**: `script_found`, `extracted` (1 fenced / 0 raw), `script_bytes`,
  `exit_code`, `timed_out`, `generate_ms`, `exec_ms`, plus usage tokens;
  `transcript.metadata["generate"]` carries `extraction` and `timed_out`.

Dataset `data/generate-tasks.jsonl` (same `EvalTask` schema, `mode:
"generate"`, difficulty `basic`/`hard` as tags) is validated by its own test
(`generate::tests::dataset_is_valid`), not the `bashkit_bash` tag test.
Every path under `/home/eval`; prompts show each starting file verbatim
because the model cannot look. Reference scripts in
`data/generate-solutions.jsonl` (one `steps` entry each) run in `cargo test
-p bashkit-eval` through the subject's own run path and must pass every
check; the do-nothing script `true` must fail every task. All 15 references
also pass under real bash 5.2.21 (jq 1.7, mawk 1.3.4) with `/home/eval`
rewritten to a temp dir, and `true` fails each there too.

| Task | Difficulty | What it exercises |
|------|------------|-------------------|
| `gen_log_status_report` | basic | status-code histogram and 5xx list from an access log |
| `gen_log_rotate` | hard | numbered rotation with a keep count, gaps, lookalike `app.log.old` |
| `gen_csv_to_jsonl` | basic | CSV to typed JSON Lines, last line without newline |
| `gen_batch_rename` | basic | recursive `*.JPG` rename, lowercase + spaces to `_`, dirs keep names |
| `gen_config_template` | hard | `${VAR}` from an env file, unknown placeholders and `$5` intact, `/` in values |
| `gen_getopts_cli` | hard | getopts tool with repeatable/required options, usage + exit 2, then exercised |
| `gen_retry_backoff` | hard | retry with 2^(n-1) backoff, exact log, preserved exit code |
| `gen_parallel_jobs` | hard | background jobs, `wait` per PID, aggregated exit codes, script exit 1 |
| `gen_printf_report` | basic | TSV aggregation into an aligned printf table |
| `gen_jq_rollup` | basic | jq rollup of paid orders per customer, top SKU |
| `gen_heredoc_site` | basic | quoted vs unquoted here-docs, unquoted env values with spaces |
| `gen_trap_atomic` | hard | EXIT trap with exit status, `mktemp -d`, atomic replace, abort keeps old output |
| `gen_assoc_inventory` | basic | associative-array aggregation by SKU and warehouse |
| `gen_path_parts` | basic | dir/base/stem/ext via parameter expansion (dotfiles, `v1.2/`, `.tar.gz`) |
| `gen_semver_bump` | hard | semver bump from conventional changes (9 -> 10), changelog insert |

Building the references found two bashkit gaps. Fixed: `jq -n` failed on
non-JSON stdin it never reads (`jq -cn ...` inside `while read; done <
file.csv`); jq 1.7 reads stdin lazily under `-n`, now only a pulled parse
error counts. Open (the reference avoids it): a double-quoted pattern with
`\$` inside `${var//pattern/repl}` is not unescaped (`"${c//"\${K}"/V}"`
replaces nothing; bash replaces `${K}`), and the unquoted form
`${c//\${K\}/V}` is a parse error.

## Runtime Tasks (`mode=runtime`)

Added 2026-10-09. Bashkit's main use is agents running scripted logic inside
it as a runtime, and the top priority for embedded python3 is agents
processing files with scripts (see [CPython](../runtimes/cpython-wasm.md)).
The agent tasks never exercised that: no python, no sqlite. Twelve
`mode=runtime` tasks do, with the same subject/scorer, per-task `max_turns`
(15 basic, 20-25 hard) and reference solutions.

**Runtimes on for every task.** `with_eval_runtimes` enables CPython and
sqlite in every eval `Bash` (and in `bashkit-replay`), not only runtime
tasks, so the model sees one environment and one system prompt across the
dataset. CPython (not Monty) because agents write CPython scripts (classes,
full stdlib, CLI). Cost: building a `Bash` stays ~0.5 ms (the engine loads
lazily); the first `python3` call in a process ~50 ms in a debug test build,
later calls ~20-40 ms; tasks whose python does little CPU work replay in
60-250 ms, the same range as bash-only references. CPU-bound python is the
slow part: Pulley runs a 12k-line regex/aggregate pass in a few seconds in a
debug build (generation in `rt_py_large_log` setup ~5 s), which is why that
task is capped at 12k lines (30k took 12 s + 23 s, close to the 30 s
per-call limit).

**`verify` (hidden post-run probe).** Runtime tasks often ask the model to
build something reusable (a CLI, an ingester, a migration, a report script).
Checking only the files it produced on the visible input lets a hardcoded
answer pass. `verify` runs after the agent loop (and after a reference's
steps), in a fresh `Bash` sharing only the task's filesystem (the model's
`set -e`, aliases, functions and cwd cannot change the probe), and its
stdout+stderr lands in `/.eval/verify.out` for `file_equals`. Verify scripts
run the model's tool on unseen input, re-run an ingester on an already-seen
batch (idempotence), apply a migration file to a fresh v1 database with extra
rows, and query the model's database through the sqlite3 builtin. Exit codes
are echoed explicitly and stderr redirected where the wording differs between
bashkit and real tools.

| Task | Difficulty | Exercises |
|------|-----------|-----------|
| `rt_py_sales_rollup` | hard | csv quoted thousands, two date formats + impossible dates, last-wins dedup after rejects, half-up cents per order (float trap), rejects with line numbers |
| `rt_py_jsonl_flatten` | basic | dotted flattening, JSON scalar forms (`2.50`->`2.5`, `1e3`->`1000.0`, `true`), compact arrays with literal non-ASCII, minimal CSV quoting with `\n` (csv module default is `\r\n`) |
| `rt_py_sessionize` | hard | ISO timestamps with mixed offsets, naive/impossible timestamps skipped, unsorted input, gap of exactly 30:00 stays in session |
| `rt_sqlite_hr_report` | basic | load CSV into a given schema without `.import` (quoted comma + apostrophe names), LEFT JOIN with empty and orphan groups, ties; verify queries the db |
| `rt_sqlite_migration` | hard | migration as a reusable SQL file: split names, ISO dates, case-insensitive email dedup with order re-pointing, UNIQUE; verify applies it to a fresh v1 db with hidden rows |
| `rt_py_sqlite_stats` | basic | python reads a db built by the sqlite3 builtin; median with half-up (Python `round` is banker's), tie-broken top customer, sorted pretty JSON |
| `rt_state_csvq_tool` | hard | build an executable Python CLI on PATH (`#!/usr/bin/env python3`), use it for two reports; verify runs it on hidden CSV (embedded newlines, `=` in values, stable desc sort, unknown column exit 2) |
| `rt_pipeline_mail_domains` | basic | reusable bash script driving python: RFC 5322 address lists with quoted commas, names with spaces, invalid JSON skipped to stderr; verify runs it on a hidden inbox |
| `rt_py_large_log` | hard | 12k-line generated log: route normalisation, nearest-rank p50/p95, half-up error %, malformed count |
| `rt_py_encodings` | hard | byte-level BOM / UTF-16 / UTF-8 / latin-1 detection, CRLF and lone CR, final newline, char counts |
| `rt_state_ledger_ingest` | hard | idempotent python ingester into SQLite (python `sqlite3` module); verify re-runs a seen batch plus a hidden one and reads balances with the sqlite3 builtin |
| `rt_py_toml_render` | basic | `tomllib` deep merge with an unset marker, arrays of tables replaced, dates to ISO strings, non-ASCII JSON |

Goldens are computed by the task generator from the inputs, independently of
the reference solutions. Every reference passes in bashkit (`cargo test -p
bashkit-eval`) and was replayed on the host with real python 3.13, sqlite3
3.45.1 and bash 5.2 (paths re-rooted, verify included) with the same file
results. The untouched-fixture test covers runtime tasks too: no step does the
work, so outputs are missing and verify probes fail.

Building the set found two bashkit gaps, both fixed: an executable script
with `#!/usr/bin/env python3` ran as bash (parse error; now a shebang naming a
registered builtin runs it, see [Builtins](../foundations/builtins.md)), and
Python's `sqlite3` module could not open a db the sqlite builtin wrote ("file
is not a database": Turso writes a WAL-mode header, the guest SQLite has no
WAL; see [SQLite Builtin](../runtimes/sqlite-builtin.md)). Open gaps (tasks
avoid them): `printf '\x00'` drops NUL bytes (bash writes them), sqlite `csv`
quoting and error exit codes differ from the sqlite3 shell, no `.import`.

Product question, not changed here: the default `BashTool` prompt only knows
what the embedder registered. With `.cpython()`/`.sqlite()` configured through
`BashTool::builder().configure(..)` the builtins are not in `builtin_names`,
so the prompt still says "python/python3 not available"; the eval registers
them with `.builtin(..)` to get the hints. The sqlite hint also tells the model
to set `BASHKIT_ALLOW_INPROCESS_SQLITE=1` even when the embedder already did.

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
like the eval agent's (`eval`@`bashkit-eval`, python3 + sqlite3, task files
mounted, 10 s per call), and classifies stderr lines into gaps:

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
(`busybox`, `gawk`, `sudo`) rank too: they are real turns lost even when the
answer is a deliberate refusal. Since 2026-10-09 `python3`/`sqlite3` exist in
the replay, so old probes for them replay as fixed.

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
