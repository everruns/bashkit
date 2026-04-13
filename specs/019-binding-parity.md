# 019 — Python ↔ Node Binding Parity Analysis

> Generated 2026-04-13. Deep comparison of feature coverage, test coverage,
> security coverage, and documentation between Python (`crates/bashkit-python/`)
> and Node (`crates/bashkit-js/`) bindings.

---

## 1  Executive Summary

| Dimension | Python | Node | Verdict |
|-----------|--------|------|---------|
| Core API surface | Good | Excellent | Node ahead — convenience VFS methods, snapshot/restore, AbortSignal |
| AI-framework adapters | 3 (LangChain, PydanticAI, DeepAgents) | 4 (Anthropic, OpenAI, Vercel AI, LangChain) | Different ecosystems; both cover their platform well |
| Test count | ~211 | ~340+ | Node has ~60% more tests |
| Security tests | ~127 | ~104 | Python has more (incl. Python-mode + shell-injection static analysis) |
| Threat-model references | Implicit | Explicit (TM-DOS-*, TM-ESC-*) | Node better traceability |
| README / docs | Good | Good | Roughly equal; structural differences |
| CI matrix | 3 Python versions | 3 runtimes × multiple versions | Node broader |
| Platform wheels | 7 targets | 5 NAPI targets + WASM | Comparable |

**Key takeaway:** Node bindings are a superset of Python bindings in raw API
surface. Python bindings compensate with deeper framework integrations
(DeepAgents full SandboxBackendProtocol) and more Python-mode security tests.
Several gaps are straightforward to close.

---

## 2  API Surface Parity

### 2.1  Core Classes

| Class | Python | Node | Notes |
|-------|--------|------|-------|
| `Bash` | ✅ | ✅ | |
| `BashTool` | ✅ | ✅ | |
| `ScriptedTool` | ✅ | ✅ | |
| `FileSystem` / `JsFileSystem` | ✅ | ✅ | |
| `ExecResult` | ✅ | ✅ | |
| `BashError` | ✅ | ✅ | |
| `get_version()` | ✅ | ✅ | |

### 2.2  Bash Constructor Options

| Option | Python | Node | Gap |
|--------|--------|------|-----|
| `username` | ✅ | ✅ | |
| `hostname` | ✅ | ✅ | |
| `max_commands` | ✅ | ✅ | |
| `max_loop_iterations` | ✅ | ✅ | |
| `timeout` | ✅ (seconds) | ✅ (ms) | Unit difference only |
| `files` | ✅ `dict[str,str]` | ✅ `Record<str, str\|()=>str\|()=>Promise<str>>` | **Node supports lazy/async file providers** |
| `mounts` | ✅ | ✅ | |
| `python` | ✅ | ✅ | |
| `external_functions` | ✅ `dict[str,callable]` | ✅ `string[]` | Different shape |
| `max_memory` | ✅ | ✅ | |

### 2.3  Bash Execution Methods

| Method | Python | Node | Gap |
|--------|--------|------|-----|
| `execute()` (async) | ✅ | ✅ | |
| `execute_sync()` | ✅ | ✅ | |
| `execute_or_throw()` (async) | ✅ | ✅ | |
| `execute_sync_or_throw()` | ✅ | ✅ | |
| `cancel()` | ✅ | ✅ | |
| `reset()` | ✅ | ✅ | |
| `fs()` | ✅ | ✅ | |
| `mount()` / `unmount()` | ✅ | ✅ | |
| `snapshot()` | ❌ | ✅ | **Python missing** |
| `restore_snapshot()` | ❌ | ✅ | **Python missing** |
| `from_snapshot()` (static) | ❌ | ✅ | **Python missing** |
| `create()` (static async) | ❌ | ✅ | For async file providers |
| AbortSignal support | ❌ | ✅ | **Python missing** |

### 2.4  Direct VFS Convenience Methods on Bash

Node exposes VFS operations directly on the `Bash` class for ergonomics.
Python requires going through `bash.fs()`.

| Method | Python (on Bash) | Node (on Bash) |
|--------|-----------------|----------------|
| `read_file()` | ❌ (via `fs()`) | ✅ |
| `write_file()` | ❌ (via `fs()`) | ✅ |
| `append_file()` | ❌ (via `fs()`) | ✅ |
| `mkdir()` | ❌ (via `fs()`) | ✅ |
| `exists()` | ❌ (via `fs()`) | ✅ |
| `remove()` | ❌ (via `fs()`) | ✅ |
| `stat()` | ❌ (via `fs()`) | ✅ |
| `chmod()` | ❌ (via `fs()`) | ✅ |
| `symlink()` | ❌ (via `fs()`) | ✅ |
| `read_link()` | ❌ (via `fs()`) | ✅ |
| `read_dir()` | ❌ (via `fs()`) | ✅ |
| `ls()` | ❌ | ✅ |
| `glob()` | ❌ | ✅ |

### 2.5  FileSystem Object Methods

| Method | Python | Node | Notes |
|--------|--------|------|-------|
| `read_file()` | ✅ | ✅ | Python returns `bytes`, Node returns `string` |
| `write_file()` | ✅ | ✅ | Python takes `bytes`, Node takes `string` |
| `append_file()` | ✅ | ✅ | |
| `mkdir()` | ✅ | ✅ | |
| `remove()` | ✅ | ✅ | |
| `stat()` | ✅ | ✅ | |
| `exists()` | ✅ | ✅ | |
| `read_dir()` | ✅ | ✅ | |
| `symlink()` | ✅ | ✅ | |
| `read_link()` | ✅ | ✅ | |
| `chmod()` | ✅ | ✅ | |
| `rename()` | ✅ | ✅ | |
| `copy()` | ✅ | ✅ | |
| `FileSystem.new()` (static) | ✅ | ❌ | Python-only constructors |
| `FileSystem.real()` (static) | ✅ | ❌ | Python-only constructors |

### 2.6  ExecResult Fields

| Field | Python | Node |
|-------|--------|------|
| `stdout` | ✅ | ✅ |
| `stderr` | ✅ | ✅ |
| `exit_code` / `exitCode` | ✅ | ✅ |
| `error` | ✅ | ✅ |
| `success` | ✅ (method) | ✅ (property) |
| `stdout_truncated` | ✅ | ✅ |
| `stderr_truncated` | ✅ | ✅ |
| `final_env` / `finalEnv` | ✅ | ✅ |
| `to_dict()` | ✅ | ❌ |

### 2.7  AI Framework Adapters

| Framework | Python | Node |
|-----------|--------|------|
| LangChain | ✅ `bashkit.langchain` | ✅ `@everruns/bashkit/langchain` |
| Anthropic SDK | ❌ | ✅ `@everruns/bashkit/anthropic` |
| OpenAI SDK | ❌ | ✅ `@everruns/bashkit/openai` |
| Vercel AI SDK | ❌ | ✅ `@everruns/bashkit/ai` |
| PydanticAI | ✅ `bashkit.pydantic_ai` | N/A (Python-only framework) |
| Deep Agents | ✅ `bashkit.deepagents` | ❌ |

Deep Agents integration is significant — Python's `BashkitBackend` implements
the full `SandboxBackendProtocol` with sync/async file ops, glob, grep, upload,
download. No Node equivalent exists.

---

## 3  Test Coverage Parity

### 3.1  Test Counts by Category

| Category | Python | Node | Gap |
|----------|--------|------|-----|
| Core / basic functionality | ~72 | ~40 | Python more |
| Builtin commands | (in core) | ~30 | Node dedicated file |
| Control flow | (in core) | ~20 | Node dedicated file |
| Error handling | (in core) | ~15 | Node dedicated file |
| Strings / quoting | (in core) | ~20 | Node dedicated file |
| Scripts | (in core) | ~15 | Node dedicated file |
| VFS operations | (in core) | ~20 | Node dedicated file |
| Integration / workflows | ~29 | (in core) | Python dedicated file |
| Tool metadata | (in core) | ~20 | Node dedicated file |
| AI adapter tests | ~20 | ~15 | Python slightly more |
| Security (basic) | ~19 | ~104 | **Node 5× more** |
| Security (advanced) | ~70+ | (in security) | Python dedicated file |
| Python-mode security | ~30 | N/A | Python-only |
| Shell injection static analysis | ~8 | N/A | Python-only (deepagents.py) |
| Runtime compatibility | N/A | ~70 | Node-only (Bun, Deno) |
| **Total** | **~211** | **~340+** | **Node ~60% more** |

### 3.2  Test Organization

| Aspect | Python | Node |
|--------|--------|------|
| Framework | pytest + pytest-asyncio | AVA (TS) + node:test (MJS) |
| File structure | 6 test files, coarser grouping | 10+ spec files, fine-grained |
| Naming convention | `test_*.py` | `*.spec.ts` + `*.test.mjs` |
| Async testing | `async def` with auto mode | AVA async + native test runner |

### 3.3  Notable Test Gaps

**Python missing:**
- Snapshot/restore tests (feature not implemented)
- AbortSignal / cancellation-with-signal tests
- Runtime compatibility tests (only CPython tested)
- Dedicated builtin command tests (merged into core tests)
- Output truncation flag behavior tests

**Node missing:**
- Python-mode security tests (N/A — different execution model)
- Deep Agents shell injection static analysis
- `to_dict()` serialization tests
- FileSystem static constructor tests

---

## 4  Security Coverage Parity

### 4.1  Threat Model Coverage

| Threat ID | Description | Python | Node |
|-----------|-------------|--------|------|
| TM-DOS-002 | Command limit enforcement | ✅ | ✅ (explicit ref) |
| TM-DOS-016 | Loop iteration limit | ✅ | ✅ (explicit ref) |
| TM-DOS-017 | Infinite while loop capping | ✅ | ✅ (explicit ref) |
| TM-DOS-018 | Nested loop multiplication | ✅ | ✅ (explicit ref) |
| TM-DOS-020 | Recursive function depth | ✅ | ✅ (explicit ref) |
| TM-DOS-021 | Fork bomb blocking | ✅ | ✅ (explicit ref) |
| TM-DOS-059 | Memory limit (string doubling) | ✅ | ✅ (explicit ref) |
| TM-ESC-001 | Exec cannot escape sandbox | ✅ | ✅ (explicit ref) |

**Gap:** Python tests don't include explicit `TM-*` references in test names.
Node tests use `TM-DOS-002`, `TM-ESC-001` etc. as test name prefixes for
traceability. Python should adopt this convention.

### 4.2  Security Test Categories

| Category | Python | Node |
|----------|--------|------|
| VFS isolation (host `/etc/passwd`) | ✅ | ✅ |
| `/proc` filesystem access | ✅ | Not tested |
| `/dev/tcp` network escape | ✅ | Not tested |
| `/dev/udp` network escape | ✅ | Not tested |
| `/dev/random` pseudo-device | ✅ | Not tested |
| Command injection prevention | ✅ | ✅ |
| Single-quote expansion | ✅ | ✅ |
| Backtick injection | ✅ | ✅ |
| Semicolon in variables | ✅ | ✅ |
| Resource limit enforcement | ✅ | ✅ |
| Fork bomb detection | ✅ | ✅ |
| Callback exception isolation | ✅ | ✅ |
| Instance isolation | ✅ | ✅ |
| Cancellation safety | ✅ | ✅ |
| JSON depth limits | ✅ | ✅ |
| Unicode / special chars | ✅ | ✅ |
| Heredoc delimiter collision | ✅ | Not tested |
| Large file handling (1GB) | ✅ | Not tested |
| Malformed bash syntax | ✅ | ✅ |
| Time-based side-channel | ✅ | Not tested |
| Concurrent instance isolation | ✅ | ✅ |
| Output truncation flags | ✅ | ✅ |
| XML boundary sanitization | Not tested | ✅ |
| Command limit recovery | ✅ | ✅ |
| Python-mode sandbox escape | ✅ | N/A |
| External function handler security | ✅ | N/A |
| Monty BigInt extraction | ✅ | N/A |
| GIL concurrent safety | ✅ | N/A |
| Deep Agents `shlex.quote` injection | ✅ (static) | N/A |
| Default memory limit without explicit config | Not tested | ✅ |

### 4.3  Security Gaps to Close

**Node should add:**
- `/proc`, `/dev/tcp`, `/dev/udp`, `/dev/random` escape tests
- Heredoc delimiter collision tests
- Large file / resource exhaustion edge cases
- Time-based side-channel tests

**Python should add:**
- XML boundary sanitization tests (if `sanitize_output` option exists)
- Explicit TM-* threat model references in test names
- Default memory limit behavior test without explicit `max_memory`

---

## 5  Documentation Parity

### 5.1  README Comparison

| Section | Python README | Node README |
|---------|---------------|-------------|
| Feature overview | ✅ | ✅ |
| Installation | ✅ (pip) | ✅ (npm/bun/deno) |
| Basic usage (sync) | ✅ | ✅ |
| Basic usage (async) | ✅ | ✅ |
| Configuration options | ✅ | ✅ (in BashOptions) |
| VFS / FileSystem access | ✅ | ✅ |
| Files pre-initialization | ✅ | ✅ |
| Mount operations | ✅ | ✅ |
| Live mounts | ✅ | ❌ |
| BashTool | ✅ | ✅ (in API section) |
| ScriptedTool | ✅ | ✅ |
| Cancellation / AbortSignal | ❌ | ✅ |
| Error handling | ❌ (minimal) | ✅ (BashError section) |
| Snapshot/restore | N/A | ✅ |
| LangChain integration | ✅ | ❌ (separate docs?) |
| PydanticAI integration | ✅ | N/A |
| Deep Agents integration | ✅ | N/A |
| Anthropic/OpenAI/Vercel AI | N/A | ✅ (in README) |
| API Reference table | ✅ | ✅ |
| Platform support | ❌ | ✅ |
| How it works | ✅ | ❌ |
| Line count | ~276 | ~241 |

### 5.2  Inline Documentation

| Aspect | Python | Node |
|--------|--------|------|
| Docstrings / JSDoc on public API | ✅ (PyO3 doc attrs) | ✅ (JSDoc + TSDoc) |
| Type stubs / type definitions | ✅ (`_bashkit.pyi`) | ✅ (`wrapper.d.ts`, `index.d.cts`) |
| Examples in docstrings | Partial | ✅ (`@example` blocks) |
| Module-level documentation | ✅ (`__init__.py`) | ✅ (`wrapper.ts` header) |

### 5.3  Examples

| Example | Python | Node |
|---------|--------|------|
| Basic bash usage | ✅ `bash_basics.py` | ✅ `bash_basics.ts` |
| Data pipeline | ❌ | ✅ `data_pipeline.ts` |
| LLM tool usage | ❌ | ✅ `llm_tool.ts` |
| K8s orchestrator | ✅ `k8s_orchestrator.py` | ❌ |
| LangChain integration | ❌ | ✅ `langchain_integration.ts` |
| AI SDK examples | ❌ | ✅ (Anthropic, OpenAI, Vercel) |

**Gap:** Python has fewer runnable examples. Node has more diverse examples
covering LLM tool patterns and AI SDK integrations.

---

## 6  CI/CD Parity

### 6.1  CI Workflows

| Aspect | Python | Node |
|--------|--------|------|
| Workflow file | `python.yml` | `js.yml` |
| Lint job | ✅ (ruff + mypy) | ❌ (no dedicated lint job) |
| Test matrix | 3 versions (3.9, 3.12, 3.13) | 3 runtimes × multiple versions |
| Runtime coverage | CPython only | Node 20/22/24, Bun latest/canary, Deno 2.x/canary |
| Example smoke tests | ✅ (2 examples) | ✅ (multiple examples) |
| Wheel build verification | ✅ (twine) | N/A (NAPI binary) |
| Gate job | ✅ `python-check` | ✅ (branch protection) |
| Type checking in CI | ✅ (mypy) | ❌ (no tsc --noEmit job) |

### 6.2  Release/Publish Workflows

| Aspect | Python | Node |
|--------|--------|------|
| Publish workflow | `publish-python.yml` | (NPM publish — not reviewed) |
| Platform matrix | 7 targets (linux/mac/win × arch) | 5 NAPI targets + WASM |
| OIDC trusted publishing | ✅ (PyPI) | ❌ (NPM token-based?) |
| Smoke test built artifacts | ✅ | ? |

---

## 7  Priority Recommendations

### 7.1  High Priority (Feature Gaps)

| # | Gap | Binding | Effort | Impact |
|---|-----|---------|--------|--------|
| 1 | **Add snapshot/restore to Python** | Python | Medium | Enables stateful workflows, parity with Node |
| 2 | **Add direct VFS convenience methods to Python Bash** | Python | Low | Ergonomics parity — `bash.read_file()` vs `bash.fs().read_file()` |
| 3 | **Add glob() to Python Bash** | Python | Low | Useful utility, Node has it |

### 7.2  Medium Priority (Test/Security Gaps)

| # | Gap | Binding | Effort | Impact |
|---|-----|---------|--------|--------|
| 4 | **Add TM-* threat model refs to Python test names** | Python | Low | Traceability |
| 5 | **Add /proc, /dev/tcp, /dev/udp escape tests to Node** | Node | Low | Security coverage |
| 6 | **Add heredoc delimiter collision tests to Node** | Node | Low | Security coverage |
| 7 | **Add large file / resource exhaustion tests to Node** | Node | Low | Edge case coverage |
| 8 | **Add XML boundary sanitization tests to Python** | Python | Low | If feature exists |

### 7.3  Low Priority (Docs/DX)

| # | Gap | Binding | Effort | Impact |
|---|-----|---------|--------|--------|
| 9 | **Add data_pipeline and llm_tool examples to Python** | Python | Low | Developer experience |
| 10 | **Add error handling section to Python README** | Python | Low | Documentation completeness |
| 11 | **Add platform support section to Python README** | Python | Low | Documentation completeness |
| 12 | **Add cancellation docs to Python README** | Python | Low | Feature discoverability |
| 13 | **Add lint/typecheck CI job for Node** | Node | Low | CI rigor |

### 7.4  Not Applicable / Ecosystem-Specific

These are intentionally different and don't need parity:

- **Python-only:** PydanticAI, Deep Agents, GIL tests, Monty/BigInt tests
- **Node-only:** Bun/Deno runtime compat, WASM browser build, AbortSignal (JS-specific API), Vercel AI/Anthropic/OpenAI adapters
- **`to_dict()` (Python-only):** Pythonic serialization pattern, not needed in JS
- **`FileSystem.new()` / `.real()` (Python-only):** Different construction pattern

---

## 8  Feature Matrix Summary

```
                          Python    Node
                          ------    ----
Core interpreter            ✅        ✅
Sync execution              ✅        ✅
Async execution             ✅        ✅
Throw-on-error variants     ✅        ✅
Cancel execution            ✅        ✅
Snapshot/restore            ❌        ✅   ← Python gap
AbortSignal                 ❌        ✅   ← JS-specific
Direct VFS on Bash          ❌        ✅   ← Python gap
VFS via fs() object         ✅        ✅
FileSystem constructors     ✅        ❌   ← Node gap (minor)
Real FS mounts              ✅        ✅
Glob on Bash                ❌        ✅   ← Python gap
Resource limits             ✅        ✅
Memory limits               ✅        ✅
Timeout                     ✅        ✅
Pre-init files              ✅        ✅
Lazy/async file providers   ❌        ✅   ← Node-specific
ExecResult.to_dict()        ✅        ❌   ← Python-specific
Truncation flags            ✅        ✅
Final env capture           ✅        ✅
Tool metadata (BashTool)    ✅        ✅
ScriptedTool                ✅        ✅
Python mode                 ✅        ✅
LangChain adapter           ✅        ✅
Anthropic SDK adapter       ❌        ✅
OpenAI SDK adapter          ❌        ✅
Vercel AI adapter           ❌        ✅
PydanticAI adapter          ✅        ❌   ← ecosystem-specific
Deep Agents adapter         ✅        ❌   ← ecosystem-specific
WASM browser build          ❌        ✅   ← Node-specific
Multi-runtime               ❌        ✅   ← Node-specific
```
