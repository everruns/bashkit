# Embedded Python (Monty)

> **Experimental.** Monty is an early-stage Python interpreter that may have
> undiscovered crash or security bugs. BashKit mitigates crashes via subprocess
> isolation, but the integration should be treated as experimental. Use with
> caution when processing untrusted input.

Bashkit embeds the [Monty](https://github.com/pydantic/monty) Python interpreter,
a pure-Rust implementation of Python 3.12. Python runs entirely in-memory with
configurable resource limits and no host access.

**See also:**
- [Threat Model](./threat-model.md) - Security considerations (TM-PY-*)
- [Custom Builtins](./custom_builtins.md) - Writing your own builtins
- [Compatibility Reference](./compatibility.md) - Bash feature support
- [`specs/011-python-builtin.md`][spec] - Full specification

## Quick Start

Enable the `python` feature and register via builder:

```rust,ignore
use bashkit::Bash;

let mut bash = Bash::builder().python().build();

let result = bash.exec("python3 -c \"print('hello from Monty')\"").await?;
assert_eq!(result.stdout, "hello from Monty\n");
```

## Usage Patterns

### Inline Code

```bash
python3 -c "print(2 ** 10)"
# Output: 1024
```

### Expression Evaluation

When no `print()` is called, the last expression is displayed (REPL behavior):

```bash
python3 -c "2 + 2"
# Output: 4
```

### Script Files (from VFS)

```bash
cat > /tmp/script.py << 'EOF'
data = [1, 2, 3, 4, 5]
print(f"sum={sum(data)}, avg={sum(data)/len(data)}")
EOF
python3 /tmp/script.py
```

### Pipelines and Command Substitution

```bash
result=$(python3 -c "print(42 * 3)")
echo "Result: $result"

echo "print('piped')" | python3
```

## Virtual Filesystem (VFS) Bridging

Python `pathlib.Path` operations are bridged to Bashkit's virtual filesystem.
Files created by bash are readable from Python and vice versa.

### Bash → Python

```bash
echo "important data" > /tmp/shared.txt
python3 -c "
from pathlib import Path
content = Path('/tmp/shared.txt').read_text()
print(f'Got: {content.strip()}')
"
```

### Python → Bash

```bash
python3 -c "
from pathlib import Path
_ = Path('/tmp/result.txt').write_text('computed by python\n')
"
cat /tmp/result.txt
```

### Supported Path Operations

| Operation | Example |
|-----------|---------|
| Read text | `Path('f.txt').read_text()` |
| Read bytes | `Path('f.txt').read_bytes()` |
| Write text | `Path('f.txt').write_text('data')` |
| Write bytes | `Path('f.txt').write_bytes(b'data')` |
| Exists | `Path('f.txt').exists()` |
| Is file/dir | `Path('f.txt').is_file()`, `.is_dir()` |
| Mkdir | `Path('d').mkdir(parents=True, exist_ok=True)` |
| Delete | `Path('f.txt').unlink()` |
| List dir | `Path('.').iterdir()` |
| Stat | `Path('f.txt').stat().st_size` |
| Rename | `Path('old').rename('new')` |
| Env vars | `os.getenv('KEY')`, `os.environ` |

### Architecture

```text
Python code → Monty VM → OsCall(ReadText, path) → Bashkit VFS → resume
```

Monty pauses at filesystem operations, Bashkit bridges them to the VFS, then
resumes execution with the result (or a Python exception like `FileNotFoundError`).

## Resource Limits

Default limits prevent runaway Python code. Customize via `PythonLimits`:

```rust,ignore
use bashkit::{Bash, PythonLimits};
use std::time::Duration;

let bash = Bash::builder()
    .python_with_limits(
        PythonLimits::default()
            .max_duration(Duration::from_secs(5))
            .max_memory(16 * 1024 * 1024)   // 16 MB
            .max_allocations(100_000)
            .max_recursion(50)
    )
    .build();
```

| Limit | Default | Purpose |
|-------|---------|---------|
| Allocations | 1,000,000 | Heap allocation cap |
| Duration | 30 seconds | Execution timeout |
| Memory | 64 MB | Heap memory cap |
| Recursion | 200 | Call stack depth |

## LLM Tool Integration

When using `BashTool` for AI agents, call `.python()` on the tool builder:

```rust,ignore
use bashkit::BashTool;

let tool = BashTool::builder()
    .python()
    .build();

// help() and system_prompt() automatically document Python limitations
let help = tool.help();  // Includes NOTES section with Python hints
```

The builtin's `llm_hint()` is automatically included in the tool's documentation,
so LLMs know not to generate code using `open()`, HTTP requests, or classes.

## Limitations

**No `open()` builtin.** Monty does not implement Python's `open()`. Use `pathlib.Path` instead:

```python
# Won't work:
# f = open('data.txt')

# Use instead:
from pathlib import Path
content = Path('data.txt').read_text()
```

**No HTTP/network.** No `socket`, `urllib`, `requests`, or `http.client` modules.
Monty has no network primitives and no OsCall variants for network operations.

**No classes.** Class definitions are not yet supported by Monty (planned upstream).

**No third-party imports.** Only builtin modules (`sys`, `typing`, `os`, `pathlib`)
are available. No `pip install`, no `import numpy`.

**No `str.format()`.** Use f-strings instead: `f"value={x}"` not `"value={}".format(x)`.

## Security

All Python execution is sandboxed:

- **No host filesystem access** — all paths resolve through the VFS
- **No network access** — no sockets, HTTP, or DNS
- **No process spawning** — no `os.system()`, `subprocess`, or `__import__('os')`
- **Resource limited** — allocation, time, memory, and recursion caps
- **Path traversal safe** — `../..` is resolved by VFS path normalization

See threat IDs TM-PY-001 through TM-PY-021 in the [threat model](./threat-model.md).

[spec]: https://github.com/anthropics/bashkit/blob/main/specs/011-python-builtin.md
