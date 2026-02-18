# Python Package

## Abstract

Bashkit ships Python bindings as pre-built binary wheels on PyPI. Users install with
`pip install bashkit` and get a native extension — no Rust toolchain needed.

## Package Layout

```
crates/bashkit-python/
├── Cargo.toml              # Rust crate (cdylib via PyO3)
├── pyproject.toml           # Python package metadata (maturin build backend)
├── src/lib.rs               # PyO3 bindings (BashTool, ExecResult)
├── bashkit/
│   ├── __init__.py          # Re-exports from native module
│   ├── _bashkit.pyi         # Type stubs (PEP 561)
│   ├── py.typed             # Marker for typed package
│   ├── langchain.py         # LangChain integration
│   ├── deepagents.py        # Deep Agents integration
│   └── pydantic_ai.py       # PydanticAI integration
└── tests/
    └── test_bashkit.py      # Pytest suite for bindings
```

## Build System

- **Build backend**: [maturin](https://github.com/PyO3/maturin) (1.4–2.0)
- **Rust bindings**: [PyO3](https://pyo3.rs/) 0.24 with `extension-module` feature
- **Async bridge**: `pyo3-async-runtimes` (tokio runtime)
- **Module name**: `bashkit._bashkit` (native), re-exported as `bashkit`

## Versioning

Python package version is read dynamically from workspace `Cargo.toml` via maturin.
`pyproject.toml` declares `dynamic = ["version"]` — no manual sync needed.

The version chain: `Cargo.toml` (workspace) → `Cargo.toml` (bashkit-python, inherits)
→ maturin reads it → wheel metadata.

## Supported Platforms

### Python Versions

3.9, 3.10, 3.11, 3.12, 3.13

### Wheel Matrix

| OS | Architecture | Variant | CI Runner |
|----|-------------|---------|-----------|
| Linux | x86_64 | manylinux (glibc) | ubuntu-latest |
| Linux | aarch64 | manylinux (glibc) | ubuntu-latest (cross) |
| Linux | x86_64 | musllinux_1_1 | ubuntu-latest (Docker) |
| Linux | aarch64 | musllinux_1_1 | ubuntu-latest (Docker) |
| macOS | x86_64 | — | macos-latest (cross) |
| macOS | aarch64 | — | macos-latest (native) |
| Windows | x86_64 | MSVC | windows-latest |

Total: ~35 wheels (7 platforms × 5 Python versions).

## PyPI Publishing

### Workflow

File: `.github/workflows/publish-python.yml`

```
GitHub Release published
    ├── build-sdist     (source distribution)
    ├── build           (7 platform variants × 5 Python versions)
    ├── inspect         (twine check all artifacts)
    ├── test-builds     (smoke test on Linux/macOS/Windows)
    └── publish         (uv publish → PyPI via OIDC)
```

### Authentication

Uses PyPI trusted publishing (OIDC) — no API tokens needed.

Prerequisites:
1. GitHub environment `release-python` exists in repo settings
2. PyPI trusted publisher configured:
   - Owner: `everruns`, Repo: `bashkit`
   - Workflow: `publish-python.yml`, Environment: `release-python`

### Smoke Test

Each platform runs after wheel build:
```python
from bashkit import BashTool
t = BashTool()
r = t.execute_sync('echo hello')
assert r.exit_code == 0
```

## Public API

### BashTool

Primary class. Wraps the Rust `Bash` interpreter with `Arc<Mutex<>>` for thread safety.

```python
from bashkit import BashTool

tool = BashTool(
    username="user",           # optional, default "user"
    hostname="sandbox",        # optional, default "sandbox"
    max_commands=10000,        # optional
    max_loop_iterations=100000 # optional
)

# Async
result = await tool.execute("echo hello")

# Sync
result = tool.execute_sync("echo hello")

# Reset state
tool.reset()

# LLM metadata
tool.name              # "bashkit"
tool.short_description # str
tool.description()     # full description
tool.help()            # LLM help text
tool.system_prompt()   # system prompt
tool.input_schema()    # JSON schema string
tool.output_schema()   # JSON schema string
tool.version           # from Rust crate
```

### ExecResult

```python
result.stdout     # str
result.stderr     # str
result.exit_code  # int
result.error      # Optional[str]
result.success    # bool (exit_code == 0)
result.to_dict()  # dict
```

### create_langchain_tool_spec()

Returns dict with `name`, `description`, `args_schema` for LangChain integration.

## Optional Dependencies

```
pip install bashkit[langchain]     # + langchain-core, langchain-anthropic
pip install bashkit[deepagents]    # + deepagents, langchain-anthropic
pip install bashkit[pydantic-ai]   # + pydantic-ai
pip install bashkit[dev]           # + pytest, pytest-asyncio
```

## CI

File: `.github/workflows/python.yml`

Runs on push to main and PRs (path-filtered to `crates/bashkit-python/`, `crates/bashkit/`,
`Cargo.toml`, `Cargo.lock`).

```
PR / push to main
    ├── lint          (ruff check + ruff format --check)
    ├── test          (maturin develop + pytest, Python 3.9/3.12/3.13)
    ├── build-wheel   (maturin build + twine check)
    └── python-check  (gate job for branch protection)
```

## Linting

- **Linter/formatter**: [ruff](https://docs.astral.sh/ruff/) (config in `pyproject.toml`)
- **Rules**: E (pycodestyle), F (pyflakes), W (warnings), I (isort), UP (pyupgrade)
- **Target**: Python 3.9, line-length 120

```bash
ruff check crates/bashkit-python        # lint
ruff format --check crates/bashkit-python  # format check
ruff format crates/bashkit-python        # auto-format
```

## Local Development

```bash
cd crates/bashkit-python
pip install maturin
maturin develop          # debug build, installs into current venv
maturin develop --release  # optimized build
pip install pytest pytest-asyncio
pytest tests/ -v         # run tests
ruff check .             # lint
ruff format .            # format
```

## Design Decisions

- **No PGO**: Profile-guided optimization adds build complexity for minimal gain.
  Bashkit is a thin PyO3 wrapper — hot paths are in Rust, not Python dispatch.
  Can revisit if profiling shows benefit.
- **No exotic architectures**: armv7, ppc64le, s390x, i686 omitted. Target audience
  is AI agent developers on standard server/desktop platforms.
- **Dynamic version**: Eliminates version drift between Rust and Python packages.
- **Trusted publishing**: No secrets to rotate. OIDC tokens are scoped per-workflow.
