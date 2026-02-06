# 011: Python Builtin (Monty)

## Status
Implemented

## Decision

BashKit provides sandboxed Python execution via `python` and `python3` builtins,
powered by the [Monty](https://github.com/pydantic/monty) embedded Python
interpreter written in Rust.

### Feature Flag

Enable with:
```toml
[dependencies]
bashkit = { version = "0.1", features = ["python"] }
```

### Why Monty

- Pure Rust, no CPython dependency
- Sub-microsecond startup
- Built-in resource limits (memory, allocations, time, recursion depth)
- No filesystem/network access by design (sandbox-safe)
- Snapshotable execution state

### Supported Usage

```bash
# Inline code
python3 -c "print('hello')"

# Expression evaluation (REPL-like: last expression printed)
python3 -c "2 + 2"

# Script file (from VFS)
python3 script.py

# Stdin
echo "print('hello')" | python3
python3 - <<< "print('hi')"

# Version
python3 --version
python3 -V
```

### Resource Limits

Monty enforces its own resource limits independent of BashKit's shell limits:

| Limit | Default | Purpose |
|-------|---------|---------|
| Max allocations | 1,000,000 | Prevent memory exhaustion |
| Max duration | 30 seconds | Prevent infinite loops |
| Max memory | 64 MB | Prevent memory exhaustion |
| Max recursion | 200 | Prevent stack overflow |

### Python Feature Support

Monty implements a subset of Python 3.12:

**Supported:**
- Variables, assignments, augmented assignments
- Arithmetic, comparison, logical operators
- Control flow: if/elif/else, for, while, break, continue
- Functions: def, return, default args, *args, **kwargs
- Data structures: list, dict, tuple, set, frozenset
- List/dict/set comprehensions, generator expressions
- String operations, f-strings
- Exception handling: try/except/finally/raise
- Built-in functions: print, len, range, enumerate, zip, map, filter, sorted, reversed, sum, min, max, abs, round, int, float, str, bool, list, dict, tuple, set, type, isinstance, hasattr, getattr, id, repr, ord, chr, hex, oct, bin, all, any, input
- Standard modules: sys, typing

**Not supported (Monty limitations):**
- Classes (planned upstream)
- Match statements
- Import of third-party libraries
- Network I/O (no socket, urllib, requests)
- Most standard library modules

### VFS Bridging

Python `pathlib.Path` operations are bridged to BashKit's virtual filesystem
via Monty's OsCall pause/resume mechanism. This enables Python code to read
and write files that are shared with the bash environment.

```bash
# Write from bash, read from Python
echo "data" > /tmp/shared.txt
python3 -c "from pathlib import Path; print(Path('/tmp/shared.txt').read_text())"

# Write from Python, read from bash
python3 -c "from pathlib import Path; Path('/tmp/out.txt').write_text('hello\n')"
cat /tmp/out.txt

# Create directories, check existence
python3 -c "from pathlib import Path
Path('/tmp/new').mkdir()
print(Path('/tmp/new').is_dir())"

# Environment variables
python3 -c "import os; print(os.getenv('HOME'))"
```

**Supported operations:**
- `Path.read_text()`, `Path.read_bytes()` — read from VFS
- `Path.write_text()`, `Path.write_bytes()` — write to VFS
- `Path.exists()`, `Path.is_file()`, `Path.is_dir()`, `Path.is_symlink()`
- `Path.mkdir()` (with `parents=True`, `exist_ok=True` kwargs)
- `Path.unlink()`, `Path.rmdir()` — delete from VFS
- `Path.iterdir()` — list directory contents
- `Path.stat()` — file metadata (st_size, st_mode, st_mtime, etc.)
- `Path.rename()` — move/rename
- `Path.resolve()`, `Path.absolute()` — path resolution
- `os.getenv()`, `os.environ` — environment variable access

**Architecture:**
```
Python code → Monty VM → OsCall(ReadText, path) → BashKit VFS → resume
```

Monty pauses execution at filesystem operations, yields an `OsCall` event
with the operation type and arguments, BashKit bridges it to the VFS, and
resumes execution with the result (or a Python exception).

### Security

See `specs/006-threat-model.md` section "Python / Monty Security (TM-PY)"
for the full threat analysis.

#### Threat: Code injection via bash variable expansion
Bash variables are expanded before reaching the Python builtin. This is
by-design consistent with all other builtins. Use single quotes to prevent
expansion: `python3 -c 'print("hello")'`.

#### Threat: Resource exhaustion
Monty enforces independent resource limits. Even if BashKit's shell limits
are generous, Python code cannot exceed Monty's allocation/time/memory caps.

#### Threat: Sandbox escape via filesystem
All `pathlib.Path` operations go through BashKit's virtual filesystem.
Python code cannot access the real host filesystem. `/etc/passwd` in Python
reads from VFS (where it doesn't exist), not the host.

#### Threat: Sandbox escape via os/subprocess
Monty has no `os.system()`, `subprocess`, or `socket` implementations.
These modules raise errors when used.

#### Threat: Denial of service via large output
Python print output is captured in memory. The 64 MB memory limit on
Monty prevents unbounded output generation.

#### Threat: Path traversal
Relative paths are resolved against the shell's cwd. Path traversal via
`../..` is constrained by the VFS's path normalization.

### Error Handling

- Syntax errors: Exit code 1, Python traceback on stderr
- Runtime errors: Exit code 1, Python traceback on stderr, any stdout produced before error preserved
- File not found: Exit code 2, error on stderr
- Missing `-c` argument: Exit code 2, error on stderr
- Unknown option: Exit code 2, error on stderr

### Integration with BashKit

- `python`/`python3` both map to the same builtin
- Works in pipelines: `echo "data" | python3 -c "import sys; ..."`
  - Note: stdin piping provides code, not data (matches real python behavior for no-arg invocation)
- Works in command substitution: `result=$(python3 -c "print(42)")`
- Works in conditionals: `if python3 -c "1/0"; then ... else ... fi`
- Shebang lines (`#!/usr/bin/env python3`) are stripped automatically

## Verification

```bash
# Build with python feature
cargo build --features python

# Run unit tests
cargo test --features python --lib -- python

# Run spec tests
cargo test --features python --test spec_tests -- python

# Run security tests
cargo test --features python --test threat_model_tests -- python
```
