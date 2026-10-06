# Embedded CPython (WebAssembly)

Bashkit can run real CPython 3.14 as `python`/`python3`. The interpreter is
compiled to WebAssembly (`wasm32-wasip1`), pre-initialized into a snapshot and
shipped inside the binary, so there is nothing to install and no network
access at build or run time. Each `python3` call runs in a fresh, isolated
instance that can only reach the Bashkit virtual filesystem and its own
stdio.

**See also:**
- [Embedded Python (Monty)](./python.md) - The lighter, Rust-native alternative
- [Threat Model](./threat-model.md) - Security considerations (TM-PY-CPY-*)
- [Compatibility Reference](./compatibility.md) - Bash feature support
- [`knowledge/runtimes/cpython-wasm.md`][spec] - Design, measurements and decisions

## Quick start

Enable the `cpython` cargo feature and register the builtins:

```toml
[dependencies]
bashkit = { version = "0.18.2", features = ["cpython"] }
```

```rust
use bashkit::Bash;

# #[tokio::main]
# async fn main() -> bashkit::Result<()> {
let mut bash = Bash::builder().cpython().build();

let r = bash
    .exec("python3 -c 'import json, sys; print(json.dumps({\"v\": sys.version_info[:2]}))'")
    .await?;
assert_eq!(r.stdout, "{\"v\": [3, 14]}\n");
# Ok(())
# }
```

No runtime opt-in variable is needed (unlike Monty's
`BASHKIT_ALLOW_INPROCESS_PYTHON`): calling `.cpython()` is the opt-in, because
the guest never runs native code in your process.

The interpreter loads on the first call of a process: the first `python3`
takes about 22 ms on the reference machine, later calls about 6 ms. To move
that one-time cost out of the first request, call
`bashkit::CPython::warm_up()` at startup.

## Why CPython instead of Monty

[Monty](./python.md) is a Python *subset* written in Rust and designed for
code-mode execution: evaluate a snippet, call back into host functions,
return a value. Scripts written by people and agents expect a Python
*command line* and the standard library behind it. CPython provides that:

| | Monty (`python` feature) | CPython (`cpython` feature) |
|---|---|---|
| Language | Subset (no classes, limited stdlib) | Full Python 3.14 |
| Stdlib | `math`, `pathlib`, `os.getenv`, `sys`, `typing`, ... | Full pure-Python stdlib plus `json`, `re`, `csv`, `sqlite3`, `zlib`, `hashlib`, `decimal`, `datetime`, `asyncio`, ... |
| CLI | `-c`, file, `-` | `-c`, `-m`, file, directory with `__main__.py`, `-`, stdin, `-x`, `-W`, `-V`, `-h` |
| Errors | Monty-specific text | CPython tracebacks, exit codes, `sys.exit` semantics |
| Isolation | In-process Rust interpreter | WebAssembly sandbox (memory-safe boundary), fresh instance per call |
| Start per call | ~15 µs | ~5-6 ms (first call in a process ~22 ms) |
| CPU-bound speed | Native | ~4-30x slower than Monty (interpreted wasm) |
| Host callbacks | Yes (external functions) | Not yet |

Pick CPython when scripts need real Python behavior; pick Monty when you
need host function callbacks or microsecond start-up for tiny snippets.
Both can be compiled in: the builder method called last owns
`python`/`python3`.

## What works

- `python3 -c CODE`, `python3 FILE`, `python3 -m MODULE`, `python3 DIR`
  (runs `DIR/__main__.py`), `python3 -` and programs piped on stdin.
- `sys.argv`, `sys.exit`, uncaught exceptions (exit 1, traceback on stderr),
  `os._exit`, `atexit`, `input()`, binary stdio via `sys.stdout.buffer`.
- Exported shell variables in `os.environ`; `PWD` becomes the working
  directory, so relative paths resolve like in a real shell.
- `open()`, `os`, `pathlib`, `shutil`, `glob`, `tempfile`, `sqlite3` database
  files, `gzip`/`zipfile`/`tarfile` on the virtual filesystem. Files the
  script leaves open are flushed when it exits.
- Local modules next to the script or in the working directory import as
  usual.
- `asyncio` (single-threaded event loop, timers, queues, gather).

## Limits

```rust
use bashkit::{Bash, CPythonLimits};
use std::time::Duration;

let bash = Bash::builder()
    .cpython_with_limits(
        CPythonLimits::default()
            .max_duration(Duration::from_secs(5)) // wall clock per call (default 30 s)
            .max_memory(128 * 1024 * 1024)        // guest memory (default 256 MB, max 1 GB)
            .max_recursion(500)                   // sys.getrecursionlimit() (default 1000)
            .max_output(1024 * 1024),             // stdout + stderr bytes (default 16 MB)
    )
    .build();
```

- **Time**: a call past its deadline stops with exit code 124 and
  `python3: execution timed out ...`. A tighter Bashkit `ExecutionLimits`
  timeout or cancellation wins.
- **Memory**: allocation past the cap raises `MemoryError` inside Python.
- **Output**: bytes past the cap are dropped and stderr ends with
  `python3: output truncated at N bytes`.
- **Files**: Bashkit filesystem limits (file size, total bytes, file count)
  apply; violations raise `OSError`.
- **Concurrency**: up to 512 CPython calls run at once per process; further
  calls wait for a free slot within their own deadline.

## Limitations

- **No subprocesses**: `subprocess`, `os.system`, `os.fork`, `os.popen` raise
  `OSError`/`AttributeError`. Python cannot call back into the shell yet.
- **No network**: `socket`, `urllib.request`, `http.client` cannot connect.
  Use the shell's `curl`/`http` builtins.
- **No threads**: `threading.Thread.start()` raises `RuntimeError`;
  `multiprocessing` and `concurrent.futures.ProcessPoolExecutor` are absent.
  `asyncio` works.
- **No native extensions or pip**: only the bundled stdlib. `ctypes`,
  `numpy`, `requests` and other third-party packages are unavailable; `ssl`,
  `_hashlib` (OpenSSL), `tkinter`, `curses`, `readline`, `dbm.gnu` are not
  built. `hashlib` still provides md5, sha1, sha2, sha3 and blake2.
- **No interactive mode**: `python3` with no program reads one from stdin;
  there is no REPL.
- **Symlinks are not followed**, like everywhere in the Bashkit VFS.
- **`errno` numbers are WASI's** (`ENOENT` is 44, not 2). Exception types
  (`FileNotFoundError`, ...) and messages are correct; code comparing
  `e.errno == errno.ENOENT` works because the `errno` module matches.
- **Fixed hash seed**: `hash()` of `str`/`bytes` is the same in every call
  (the seed is baked into the snapshot). `random` is re-seeded per call.
- **Deep C-level recursion** (for example `repr` of a list nested 100 000
  levels) ends the call with `python3: fatal error: stack overflow in the
  interpreter` instead of `RecursionError`.
- **CPU-bound code is slow**: the guest runs on Wasmtime's portable Pulley
  interpreter, roughly 4-30x slower than Monty and far slower than native
  CPython. Start-up, not throughput, is what this runtime is tuned for.
- **No host callbacks** (Monty's external functions) or `ToolDef`
  integration yet.
- Interpreter-start options (`-E`, `-I`, `-s`, `-S`, `-B`, `-u`, `-O`, `-q`,
  `-X ...`) are accepted and ignored.

## Binary size

The `cpython` feature adds about 45 MB to a binary: the precompiled
interpreter snapshot (~41 MB, mostly the pre-initialized 40 MB heap image so
it can be mapped copy-on-write) and the zipped stdlib (~2.5 MB) are embedded,
plus the Wasmtime runtime. Pages are mapped on demand, so resident memory per
process is far smaller.

[spec]: https://github.com/everruns/bashkit/blob/main/knowledge/runtimes/cpython-wasm.md
