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

## 7  Implementation Roadmap

### Design principle

Both bindings should be **idiomatic mirrors**: same capabilities, same test
structure, same security coverage — adapted to each language's conventions.
Python uses `snake_case`, `async def`, `pytest`. Node uses `camelCase`,
`Promise`, AVA/node:test. The *shape* should match; the *style* should be native.

### Intentionally ecosystem-specific (no parity needed)

- **Python-only:** PydanticAI, Deep Agents, GIL tests, Monty/BigInt, `to_dict()`
- **Node-only:** Bun/Deno runtime-compat, WASM browser, AbortSignal, Vercel AI/Anthropic/OpenAI adapters
- **`FileSystem.new()`/`.real()`** — Python-only constructors (idiomatic)

---

### Phase 1 — API Parity (Python catching up to Node)

**PR 1a: Snapshot/Restore for Python** (Medium effort)

Add to `PyBash` and `PyBashTool` in `crates/bashkit-python/src/lib.rs`:
- `snapshot() -> bytes` — serialize interpreter state
- `restore_snapshot(data: bytes) -> None` — restore from snapshot
- `Bash.from_snapshot(data: bytes, **kwargs) -> Bash` — static factory

Add to `_bashkit.pyi` type stubs. Add tests in `test_basic.py` (see Phase 2
for file rename).

**PR 1b: Direct VFS convenience methods on Bash/BashTool** (Low effort)

Add delegate methods on `PyBash` and `PyBashTool` that forward to the internal
`FileSystem`:

```python
# On Bash class directly:
bash.read_file(path: str) -> str
bash.write_file(path: str, content: str) -> None
bash.append_file(path: str, content: str) -> None
bash.mkdir(path: str, recursive: bool = False) -> None
bash.exists(path: str) -> bool
bash.remove(path: str, recursive: bool = False) -> None
bash.stat(path: str) -> dict
bash.chmod(path: str, mode: int) -> None
bash.symlink(target: str, link: str) -> None
bash.read_link(path: str) -> str
bash.read_dir(path: str) -> list[dict]
bash.ls(path: str = ".") -> list[str]
bash.glob(pattern: str) -> list[str]
```

Note: These return `str` (not `bytes` like `FileSystem.read_file`), matching
Node's behavior. The `fs()` accessor remains for advanced use.

Update `_bashkit.pyi`, tests, README.

**PR 1c: Lazy file providers for Python** (Low effort)

Allow `files` dict values to be `Callable[[], str]` in addition to `str`:

```python
bash = Bash(files={
    "/data/config.json": lambda: load_config(),  # called on first read
})
```

Node supports sync callables and async callables. Python should support sync
callables only (async would require `await` in constructor, which breaks the
sync `Bash()` constructor). Document that `Bash.create()` is not needed in
Python since Python callables can be sync wrappers around async code via
`asyncio.run()`.

---

### Phase 2 — Test Structure Alignment

**PR 2a: Restructure Python tests to mirror Node** (Medium effort)

Current Python structure (coarse):
```
tests/
  test_bashkit.py          # 129 tests — everything mixed
  test_integration.py      # 42 tests
  test_security.py         # 28 tests
  test_security_advanced.py # 102 tests
  test_python_security.py  # 88 tests
  test_shell_injection.py  # 9 tests
  test_frameworks.py       # 27 tests
```

Target Python structure (mirrors Node):
```
tests/
  test_basic.py              # ← Node: basic.spec.ts
  test_builtins.py           # ← Node: builtins.spec.ts
  test_control_flow.py       # ← Node: control-flow.spec.ts
  test_error_handling.py     # ← Node: error-handling.spec.ts
  test_strings_and_quoting.py # ← Node: strings-and-quoting.spec.ts
  test_scripts.py            # ← Node: scripts.spec.ts
  test_vfs.py                # ← Node: vfs.spec.ts
  test_tool_metadata.py      # ← Node: tool-metadata.spec.ts
  test_security.py           # ← Node: security.spec.ts (merge basic + advanced)
  test_ai_adapters.py        # ← Node: ai-adapters.spec.ts (rename frameworks)
  test_python_security.py    # Python-only (keep)
  test_shell_injection.py    # Python-only (keep)
  test_integration.py        # Keep — cross-cutting workflow tests
```

Migration plan:
1. Create new files with correct test names
2. Move tests from `test_bashkit.py` into category-appropriate files
3. Rename `test_frameworks.py` → `test_ai_adapters.py`
4. Merge `test_security.py` + `test_security_advanced.py` → single `test_security.py`
5. Keep `test_python_security.py` and `test_shell_injection.py` (Python-specific)
6. Delete empty `test_bashkit.py`

**PR 2b: Add missing test categories to Python** (Medium effort)

Tests that exist in Node but not in Python:

| Node test | Python equivalent needed |
|-----------|------------------------|
| `builtins.spec.ts` — `cat`, `head`, `tail`, `wc`, `grep`, `sed`, `awk`, `sort`, `uniq`, `tr`, `cut`, `printf`, `base64`, `seq`, `jq`, `md5sum`, `sha256sum` | `test_builtins.py` — dedicated tests for each builtin |
| `strings-and-quoting.spec.ts` — heredoc, string replacement, case conversion, arrays | `test_strings_and_quoting.py` — explicit coverage |
| `scripts.spec.ts` — `count lines`, `find and replace`, `extract unique`, `JSON pipeline`, `config generator`, `loop with accumulator`, `data transformation`, `error handling with \|\|`, `conditional file creation` | `test_scripts.py` — real-world script patterns |

**PR 2c: Add missing test categories to Node** (Low effort)

Tests that exist in Python but not in Node:

| Python test | Node equivalent needed |
|-------------|----------------------|
| `test_integration.py` — multi-step workflows, CRUD, concurrent instances, interleaved sync/async | `integration.spec.ts` — cross-cutting tests |
| GIL / threading tests | N/A (not applicable to Node) |
| `test_shell_injection.py` — static analysis of adapter code | Could add static analysis of `langchain.ts` etc. |

---

### Phase 3 — Security Test Alignment

**PR 3a: Add TM-* threat model references to Python** (Low effort)

Rename Python security tests to include threat-model IDs:

```python
# Before:
def test_fork_bomb_prevented(): ...
def test_max_loop_iterations_enforced(): ...

# After:
def test_tm_dos_021_fork_bomb_prevented(): ...
def test_tm_dos_016_loop_iteration_limit_enforced(): ...
```

Full mapping (apply to all applicable tests):
- `TM-DOS-002`: command limit
- `TM-DOS-005`: large file write
- `TM-DOS-006`: VFS file count
- `TM-DOS-012`: deep directory nesting
- `TM-DOS-013`: long filename/path
- `TM-DOS-016`: loop iteration limit
- `TM-DOS-017`: while true capping
- `TM-DOS-018`: nested loop multiplication
- `TM-DOS-020`: recursive function depth
- `TM-DOS-021`: fork bomb
- `TM-DOS-029`: arithmetic overflow/div-by-zero
- `TM-DOS-059`: memory limit
- `TM-ESC-001`: exec escape
- `TM-ESC-002`: process substitution
- `TM-ESC-003`: /proc access
- `TM-ESC-005`: signal trap
- `TM-INF-001`: /etc/passwd
- `TM-INF-002`: env var leak
- `TM-INJ-005`: path traversal
- `TM-ISO-001`: variable isolation
- `TM-ISO-002`: filesystem isolation
- `TM-ISO-003`: function isolation
- `TM-INT-001`: host path leak
- `TM-INT-002`: memory address / stack trace leak
- `TM-NET-001`: /dev/tcp escape
- `TM-UNI-002`: zero-width chars
- `TM-UNI-003`: homoglyph
- `TM-UNI-004`: RTL override

**PR 3b: Add missing security tests to Node** (Low effort)

Node security.spec.ts currently has 99 tests. Add:
- `/dev/udp` network escape attempt
- Heredoc delimiter collision
- Large file handling (1GB boundary)
- Time-based side-channel (execution timing consistency)
- Callback with stdin injection (exists in Python `test_security_advanced.py`)
- JSON array nesting bomb

**PR 3c: Add missing security tests to Python** (Low effort)

Python is missing some tests Node has:
- `TM-DOS-005`: large file write limited
- `TM-DOS-006`: VFS file count limit
- `TM-DOS-012`: deep directory nesting limited
- `TM-DOS-013`: long filename/path rejected
- Default memory limit without explicit `max_memory`
- `TM-INF-002`: env vars don't leak host info
- Direct VFS API injection tests (after PR 1b adds convenience methods)
- `TM-ESC-005`: signal trap commands
- `TM-DOS-029`: arithmetic overflow, div-by-zero, mod-by-zero
- Username/hostname injection tests
- Mounted files with crafted paths / null bytes
- CRLF line endings in scripts

---

### Phase 4 — Documentation Alignment

**PR 4a: Align README structure** (Low effort)

Both READMEs should follow this canonical structure:

```
1. Feature overview (badge + bullet list)
2. Installation
3. Quick Start
   3a. Sync execution
   3b. Async execution
4. Configuration (constructor options table)
5. Virtual Filesystem
   5a. Direct methods (read_file, write_file, ...)
   5b. FileSystem accessor (fs())
   5c. Pre-initialized files
   5d. Real filesystem mounts
6. Error Handling (BashError)
7. Cancellation
8. BashTool (LLM integration)
9. ScriptedTool (multi-tool orchestration)
10. Snapshot/Restore (after PR 1a)
11. Framework Integrations
    - [ecosystem-specific entries]
12. API Reference (condensed table)
13. Platform Support
14. How It Works (optional)
```

**Python README gaps to fill:** Error Handling section, Cancellation section,
Platform Support section, Snapshot/Restore (after PR 1a).

**Node README gaps to fill:** Live mounts section, How It Works section.

**PR 4b: Add missing examples to Python** (Low effort)

Add Python equivalents of Node examples:

| Example | Node file | Python file to create |
|---------|-----------|----------------------|
| Data pipeline | `data_pipeline.ts` | `data_pipeline.py` |
| LLM tool usage | `llm_tool.ts` | `llm_tool.py` |

Add Node equivalent of Python example:

| Example | Python file | Node file to create |
|---------|-------------|---------------------|
| K8s orchestrator | `k8s_orchestrator.py` | `k8s_orchestrator.ts` |

**PR 4c: Align inline documentation** (Low effort)

- Add `@example` blocks to Python type stubs (`_bashkit.pyi`) for all public methods
- Ensure JSDoc `@example` blocks exist for all Node public methods

---

### Phase 5 — CI Alignment

**PR 5a: Add TypeScript type-check job to Node CI** (Low effort)

Add to `js.yml`:
```yaml
lint:
  runs-on: ubuntu-latest
  steps:
    - uses: actions/checkout@v4
    - uses: actions/setup-node@v4
    - run: npm ci
    - run: npx tsc --noEmit
```

---

## 8  Phase Sequencing & Dependencies

```
Phase 1 (API)        Phase 2 (Tests)       Phase 3 (Security)
─────────────        ────────────────       ──────────────────
PR 1a ──────────────→ PR 2a (restructure)─→ PR 3a (TM refs)
PR 1b ──────────┐     PR 2b (add Python) ─→ PR 3c (add Python)
PR 1c           │     PR 2c (add Node)   ─→ PR 3b (add Node)
                │
                └──→ Tests for new VFS methods go in PR 2a's test_vfs.py

Phase 4 (Docs)        Phase 5 (CI)
──────────────        ────────────
PR 4a (READMEs) ──→ independent
PR 4b (examples)──→ independent
PR 4c (inline)  ──→ independent
                      PR 5a (tsc) ──→ independent
```

Phases 4 and 5 can proceed in parallel with any other phase.
Phases 1→2→3 have sequential dependencies.

**Estimated total:** 12 PRs, each small and independently shippable.

---

## 9  Success Criteria

When all phases are complete:

1. **API surface:** `diff <(python_methods) <(node_methods)` shows only
   ecosystem-specific items (AbortSignal, to_dict, etc.)
2. **Test files:** Python and Node have 1:1 matching test file names (with
   language-appropriate suffixes)
3. **Security tests:** Both have TM-* references; union of all security tests
   present in both (minus ecosystem-specific)
4. **READMEs:** Same section headings in same order
5. **Examples:** Each binding has at least 4 runnable examples covering basics,
   data pipelines, LLM tools, and orchestration
6. **CI:** Both have lint, typecheck, test matrix, example smoke tests

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
