---
type: Subsystem Design
title: CPython WebAssembly Runtime
description: Real CPython 3.14 as python3, compiled to WASI, snapshotted with Wizer and run on Wasmtime's Pulley interpreter over the bashkit VFS.
tags:
  - bashkit
  - python
  - runtime
  - sandbox
  - wasm
---

# CPython WebAssembly Runtime

## Status
Implemented (`cpython` feature, off by default)

## Decision

`python`/`python3` can run real CPython 3.14 instead of Monty. Monty is a
code-mode Python subset (no classes, small stdlib, its own CLI); scripts
written by people and agents expect CPython's language, stdlib and command
line. Bashkit's main target is many tenants in one process, so CPython's own
memory safety cannot be the isolation boundary: CPython runs as a WebAssembly
guest, and the wasm boundary is the sandbox.

Public guide: `crates/bashkit/docs/cpython.md` (rustdoc `cpython_guide`).

### Shape

| Piece | Where | Role |
|-------|-------|------|
| `bashkit-cpython-wasm` crate | `crates/bashkit-cpython-wasm/` | Ships bytes only: precompiled module, stdlib zip, `engine_config()` |
| Guest build | `crates/bashkit-cpython-wasm/guest/build.sh` | Reproducible: CPython 3.14.8 + WASI SDK 24 + static zlib/sqlite3, Wizer snapshot |
| Guest entry | `guest/bashkit_main.c`, `guest/_bashkit_boot.py` | Reactor exports `wizer-initialize`, `bashkit_run`; Python CLI emulation |
| WASI host | `crates/bashkit/src/builtins/cpython/wasi.rs` | All 42 `wasi_snapshot_preview1` imports over the bashkit `FileSystem` |
| Builtin | `crates/bashkit/src/builtins/cpython/mod.rs` | Engine, pooled instances, limits, deadline/budget polling, exit mapping |

### Workload priority

The main workload is agents running `python3` for scripted logic: processing
a file, transforming data, gluing commands (user, 2026-10-07). Stdlib and
preload choices follow it: modules those scripts use (text, data formats,
paths, archives, HTTP) ship and the common ones are preloaded; modules with
a low chance of use in them (test runners, profilers, packaging and
interactive tools, legacy or macOS formats) are not shipped, even when they
would work. Adding one back needs a concrete agent use case.

### Startup is the product constraint

Nothing is compiled at run time, and CPython initialization never runs per
call:

1. **Wizer snapshot**: `wizer-initialize` runs `Py_InitializeFromConfig`,
   imports ~70 common stdlib modules, patches asyncio, then `gc.freeze()`s the
   heap. The resulting module starts with a running interpreter.
2. **Pulley AOT**: `build.rs` precompiles the snapshot to Wasmtime's portable
   Pulley bytecode (`pulley64`) with Cranelift as a build-dependency only. One
   artifact serves every 64-bit little-endian host; no executable memory is
   needed at run time. Native code is an opt-in instead (`cpython-native`,
   below).
3. **Zero-copy load**: the `.cwasm` is embedded 64 KiB-aligned and loaded with
   `Module::deserialize_raw`; wasmtime registers code by address, so only the
   first load in a process maps in place, later loads copy (`load_module`).
4. **Copy-on-write instances**: `memory_init_cow` +
   `memory_guaranteed_dense_image_size(128 MB)` map the ~40 MB snapshot heap
   instead of copying it (without the dense image every instantiation copied
   9.4 MB).
5. **`gc.freeze()` in the snapshot**: without it every call's exit-time
   `gc.collect()` walked the whole snapshot heap, dirtying its pages; calls
   cost ~100 ms instead of ~5 ms.
6. **Immortal snapshot objects**: every GC-tracked snapshot object and its
   direct referents are made immortal (`_Py_SetImmortal`) before freezing,
   so reference counting stops writing to snapshot pages. Copy-on-write
   faults per call dropped ~20% (341 to 273 for `print(1)`).
7. **Lean driver**: the per-call environment is installed from C
   (`_bashkit.load_environ`) instead of per-key `os.environ` updates.
8. **Bytecode-only stdlib**: `build.sh` compiles the stdlib zip to
   unchecked-hash `.pyc` with the native build interpreter and ships no
   `.py`. Compiling source on Pulley cost 3-7 s per non-preloaded import
   (`import http.client` 7.2 s, now ~0.5 s); sources plus bytecode would
   exceed the crates.io 10 MiB crate cap. Modules that cannot work in the
   guest (FFI, TLS, sockets, TTY) and pure-Python twins of C modules are
   not shipped (L-CPY-009), nor are low-use modules per the workload
   priority above (`unittest`, `doctest`, profilers, `dbm`, `bz2`/`lzma`
   shims, ...; stdlib zip 3.58 to 3.16 MB). `pdb` is a stand-in
   (`guest/pdb.py`): `breakpoint()` prints a notice and continues.
9. **xz snapshot artifact**: `artifacts/python.wasm.xz` (xz -9e) instead of
   gzip: 4.99 MB vs ~6.7 MB for the same snapshot. `build.rs` decodes it
   with `lzma-rs` (pure Rust build-dependency; ~2.5 s in a debug build
   script, nothing at run time). Rejected: zstd/brotli (C or no pure-Rust
   encoder parity) and splitting the stdlib into a second crate (single
   companion crate is a product decision).
10. **Preload budget**: what the snapshot imports is chosen by per-call
   import cost on Pulley against crate size. Preloaded beyond the core set:
   bashkit's requests/httpx, `urllib.request` (with http.client and email,
   0.47 s per call before), `tomllib`, `configparser`,
   `xml.etree.ElementTree` (0.1-0.12 s each). Each costs snapshot bytes,
   not per-call time (`print(1)` stayed ~4.1 ms). `mailbox` is not shipped.

Measured on a 4-vCPU x86-64 VM (see `criterion-python-*` results under
`crates/bashkit/benches/results/`): first `python3` in a fresh process
~19 ms (one-time engine/module load and first-touch page faults), then
`python3 -c 'print(1)'` ~4.4 ms per call warm; Monty ~15 µs (first call ~0.4 ms).
CPU-bound Python is ~4-30x slower than Monty. 1024 concurrent tenants × 4
calls: 0 failures, ~165 calls/s on 4 vCPUs, 3.0 GB peak RSS (2026-10-07,
after immortal objects; was ~90 calls/s, 4.7 GB). Importing a module outside
the snapshot costs ~0.1-0.2 s (`zoneinfo` 0.11 s, `tarfile` 0.1 s,
`pickle` 0.08 s); `email`, `http.client` and `urllib.request` are preloaded
(~0 per call, was 0.27-0.47 s).

### Native code (opt-in, `cpython-native`)

`cpython-native` (bashkit) / `native` (companion crate) makes `build.rs`
compile the snapshot to machine code for Cargo's `TARGET` instead of Pulley.
Still nothing is compiled at run time and it is still one crate; the trade:

- **Speed** (4-vCPU x86-64, warm calls, 2026-10-07): `print(1)` 0.95 ms vs
  3.7 ms, `import http.client` 34 ms vs 461 ms, `fib(20)` 6.4 ms vs 69 ms.
- **Executable memory** is required at run time; Pulley needs none.
- **First load copies** the module into executable memory (native code
  cannot run from the binary's read-only static): `CPython::warm_up()` takes
  ~47 ms instead of ~15 ms. Call it at startup.
- **Baseline ISA**: compiled for the target triple with no host CPU feature
  detection, so a binary built on one machine runs on any CPU of that
  architecture. A cross-compiled build gets the cross target's code.
- **Same memory configuration** as Pulley (1 GiB reservation, 64 KiB guard,
  explicit bounds checks), so pooled slots and limits are identical.
- CI runs the full CPython suites on both builds (x86-64 Linux).

Default stays Pulley: portable artifact, no executable memory, smallest
attack surface (TM-PY-CPY-011).

### Guest contract (snapshot invariants)

- The snapshot cached the preopen table: the host must present exactly one
  preopen, `/`, at fd 3 (`PREOPEN_FD`).
- The snapshot cached the stdlib zip directory: the host must serve the exact
  bytes of `STDLIB_ZIP` at `/usr/local/lib/python314.zip`. Both artifacts come
  from one `build.sh` run.
- One instance serves one call; state is never reused.

### HTTP bridge

Python HTTP leaves the guest at the HTTP level, never as sockets:
`_bashkit.http(method, url, headers, body, timeout)` (C, `bashkit_main.c`)
calls the host import `bashkit.http_request`, which decodes the request and
runs it through `HttpClient::request_with_timeouts`, the same pipeline as
`curl` (allowlist, SSRF precheck, `before_http` hooks and credential
injection, signing, the embedder's `HttpTransport`, response cap,
`after_http`). Rejected: socket emulation (needs OpenSSL and TLS in the guest,
and policy could only see host:port) and shelling out to `curl` (needs a
process bridge, quoting on every request).

- Wire format: u32 little-endian length-prefixed fields. The response comes
  back in two steps (`http_request` reports the size, `http_take` copies),
  so the guest allocates once and the copy counts against its memory limit.
- Host validation before dispatch: methods GET/POST/PUT/DELETE/HEAD/PATCH,
  URL <= 8 KiB with no controls or spaces, <= 128 headers and 64 KiB,
  token header names, no CR/LF/NUL in values, body <= 16 MiB. Host-owned
  headers (`Host`, framing, hop-by-hop, proxy) are dropped, so a guest
  cannot retarget the request past the allowlist or smuggle a second one.
  `check_request_invariants` is the fuzz entry (`cpython_http_fuzz`).
- Timeout = min(request timeout, call deadline); `CPythonLimits::max_http_requests`
  (default 100) caps requests per call. Redirects are followed by Python,
  so every hop is a new, re-checked request.
- No network configured, or no `http_client` feature: every request fails
  with "network access not configured". Errors reach Python as
  `ConnectionError`, `TimeoutError` or `ValueError` (invalid request), text
  capped at 512 bytes.
- Stdlib adapter (`_bashkit_http.py`): http.client keeps its own request
  and response code; only the socket is swapped for a bridge socket that
  parses the bytes http.client writes (incl. chunked uploads), sends them
  once complete, and serves the reply as HTTP/1.1 bytes the real
  `HTTPResponse` parses. `HTTPSConnection` is defined although `ssl` is not
  built, so urllib.request registers https. Applied by a footer `build.sh`
  appends to `http/client.py`, so scripts that never import it pay nothing.
- The guest is linked with the `bashkit` imports; `wasmtime wizer` and the
  build smoke test run with `-W unknown-imports-trap=y` since init never
  calls them.

### `requests` and `httpx` (bashkit's own modules)

`requests`, `httpx` and `httpx2` in the guest are bashkit's own compact
implementations of the common API (`guest/requests/`, `guest/httpx.py`,
`guest/httpx2.py`), sharing `guest/_bashkit_webcore.py`, and call
`_bashkit.http` directly. Upstream packages were rejected: vendored
requests + urllib3 + idna + charset_normalizer imported 155 modules and
cost ~3.1 s per call on Pulley (urllib3 1.3 s, mostly module-level regex
compiles and class creation; http.client alone ~0.47 s), and since each
call starts from the snapshot that cost repeats every call. Preloading the
upstream stack instead would have grown the snapshot by far more and kept
the dependency on stdlib http.client/email.

- Only snapshot-preloaded stdlib is imported (json, urllib.parse, base64,
  zlib, `http` for status phrases), and the modules are preloaded
  themselves: `import requests, httpx` adds nothing to a call (criterion
  `python_import/cpython/requests_httpx` ~= `python_call/cpython/print`).
- Always available with `cpython` (no extra feature); without a network
  allowlist every request fails like `curl`.
- Redirects are followed in the guest, so each hop is a new host-checked
  request; `Authorization` is dropped when a redirect changes host. Cookies
  from responses are kept per host. Both are convenience, not the
  boundary: the host allowlist and credential injection are.
- The host does not decompress (TM-NET-013); gzip/deflate bodies are
  decoded in the guest within its memory limit.
- Scope: verbs, sessions/clients, params/data/json/files/headers/cookies/
  basic auth/timeouts/redirects, response helpers, upstream exception
  hierarchies, httpx `AsyncClient` (sequential) and `MockTransport`.
  Out of scope: retries, proxies, client certs, HTTP/2, digest auth,
  streaming uploads, OPTIONS (not in the host's method set). Versions
  report `2.32.0+bashkit` / `0.28.1+bashkit`.
- `httpx2` (pydantic/httpx2) is the `httpx` module under a second name.

### WASI host decisions

- Only `wasi_snapshot_preview1`, only against bashkit state: VFS, captured
  stdin bytes, in-memory stdout/stderr, host RNG and clocks. Socket calls
  return `ENOTSUP`; there is no process API.
- One more import module, `bashkit` (`builtins/cpython/http.rs`), carries
  HTTP. See [HTTP bridge](#http-bridge).
- Files are whole in-memory buffers written back on close/sync/exit. Buffers
  are bounded by `max_file_size` and, in total, by the call's memory budget.
- Paths normalize lexically and clamp at `/`. Symlinks resolve inside the
  VFS only (TM-ESC-002); `path_filestat_get` honors WASI `SYMLINK_FOLLOW`, so
  `lstat`/`os.path.islink` see the link itself, and unlink/rmdir act on the
  link, never its target.
- The stdlib zip is a read-only overlay at a fixed path. Writes, renames,
  truncation, unlink and directory changes there fail (`EROFS`/`ENOENT`/
  `EACCES`); tenant files placed next to it are not on `sys.path`.
- `poll_oneoff` sleeps in 250 ms slices and never past the call deadline.

### Builtin decisions

- **No runtime opt-in env var**: unlike Monty, no native interpreter runs in
  the host, so `BashBuilder::cpython()` is the opt-in.
- **Executable scripts**: a file with `#!/usr/bin/env python3` (or any
  `#!.../python3`) run by path or `$PATH` runs CPython with the script path
  as `argv[0]`, like `python3 FILE` (interpreter shebang dispatch, see
  [Builtins](../foundations/builtins.md)). Before 2026-10-09 the content ran
  as bash and failed with a parse error.
- **sqlite3 module interop**: the guest SQLite has no WAL, so it can only
  open databases the `sqlite` builtin wrote because the builtin persists
  them in rollback-journal mode (see [SQLite Builtin](sqlite-builtin.md)).
- **Only exported variables** reach `os.environ`, plus `PWD`; `__BASHKIT_*`
  names are filtered (the recursion limit is passed as one and removed by the
  guest).
- **Fuel** is set effectively unlimited with a 100K-instruction async yield
  interval. Each poll re-checks the call deadline and the request's
  `ExecutionBudget` (cancellation, request timeout), so a busy loop cannot
  pin a worker thread. Budget work units: 4096 per call plus one per 1024
  guest instructions.
- **Pooling allocator**: 512 slots per process, 1 GiB max memory per slot
  (`max_memory` above that is clamped), 64 MiB kept resident, `pagemap_scan`
  on Linux. A semaphore makes call 513 wait for a slot inside its deadline.
  If the pool cannot reserve address space, the engine falls back to on-demand
  allocation. Engine reservation is 1 GiB with a 64 KiB guard: Pulley
  bounds-checks every access, so large virtual reservations buy nothing.
- **Exit mapping**: `bashkit_run` return value or `proc_exit` code; timeout
  124; traps become `python3: fatal error: ...` (Display only, TM-INF-022)
  with exit 1; open files are flushed even after a trap or timeout.
- **Output cap** drops bytes past `max_output` and appends
  `python3: output truncated at N bytes` to stderr.

### Guest driver (`_bashkit_boot.py`)

Emulates CPython's command line on an already-initialized interpreter:
option parsing (`-c`, `-m`, file, directory, `-`, `-x`, `-W`, `-V`/`-VV`,
`-h`, ignored init-only flags), fresh `__main__` per call, `sys.argv` and
`sys.path[0]` rules, tracebacks with driver frames stripped, `SystemExit`
semantics, `atexit`, then clearing `__main__` + `gc.collect()` so unclosed
files flush. `random` is re-seeded per call, lazily: the module instance
seeds from `os.urandom` on first use in a call (~0.2 ms saved on calls that
never use it). asyncio's self-pipe is disabled
(no sockets, threads or signals exist to need it).

## Testing

| Layer | Location |
|-------|----------|
| Unit | `builtins/cpython/tests.rs`, `wasi.rs` tests, `bashkit-cpython-wasm` lib tests |
| Integration (CLI, stdio, env, VFS, isolation, bash interop) | `tests/integration/cpython_integration_tests.rs` |
| Capability (~85 language/stdlib programs) | `tests/integration/cpython_capability_tests.rs`; the same table is diffed against the host `python3` when present |
| Security (threats below, limits, crash containment, proptest fuzz) | `tests/integration/cpython_security_tests.rs` |
| HTTP bridge, requests, httpx | `tests/integration/cpython_http_tests.rs` (fake transport: allowlist, redirects, cookies/auth scoping, caps, timeouts) |
| requests/httpx API parity | `guest/tests/differential.py`: the same scenarios against upstream `requests`/`httpx` (patched transport, shared fake server), outputs must match |
| Fuzz | `crates/bashkit/fuzz/fuzz_targets/cpython_fuzz.rs`, `cpython_http_fuzz.rs` (`--features cpython`, nightly job) |
| Bench / load | `benches/python.rs`, `examples/python_startup.rs`, `examples/cpython_load.rs`, `just bench-python` |

CI: the Test job runs `cargo test -p bashkit --features cpython,http_client
--lib --test integration -- cpython` and the requests/httpx differential
(pinned upstream versions in a venv); the examples job runs
`cpython_scripts`; fuzz-check builds `cpython_fuzz` and `cpython_http_fuzz`.

## Rebuilding the guest

`crates/bashkit-cpython-wasm/guest/build.sh [WORK_DIR]` downloads pinned
sources (CPython, WASI SDK, wasmtime CLI for `wizer`, zlib, sqlite), builds,
snapshots, smoke-tests with the reference runtime and rewrites `artifacts/`
(`python.wasm.xz`, `python314.zip`, `MANIFEST` with checksums). Commit all
three together. Changing `engine_config()` or the wasmtime version requires
no guest rebuild; `build.rs` recompiles the `.cwasm`.

wasmtime is pinned (`=48.0.5`): a `.cwasm` loads only into the version and
compile-affecting configuration that produced it. 49.x needs rustc 1.96.

## Known gaps

- Python cannot call back into the shell (subprocess bridge) or host
  functions (`ToolDef`). TODO: design a `bashkit` module over a host import.
- Where a warm call goes (`print(1)`, 2026-10-07 profile): ~4 ms Pulley
  interpreting ~500K guest instructions (two thirds driver: compile, env,
  `random` reseed, exit `gc.collect`), ~1 ms in ~270 page faults. The
  one-time first-call cost is ~13 ms building the copy-on-write memory image
  (wasmtime copies the 40 MB snapshot span, 14 MB of real data, into a
  memfd). The `cpython-native` opt-in removes most of the interpretation
  cost (below).
- ~650 page faults per call (host Pulley stack + guest CoW writes); kernel
  fault cost grows under concurrency, so 4 vCPUs reach ~2x, not 4x, single
  thread throughput. TODO: profile which pages fault; consider pooling the
  Pulley stack upstream.
- macOS has no `memfd`; CoW falls back to copying the image per instance.
- See [Limitations](../operations/limitations.md) `L-CPY-*` and
  [Threat Model](../security/threat-model.md) `TM-PY-CPY-*`.

## See also

- [Python Builtin](python-builtin.md), the Monty runtime this complements
- [ZapCode Runtime](zapcode-runtime.md), the other embedded language VM
- [Builtins](../foundations/builtins.md), builtin trait and registration
- [Threat Model](../security/threat-model.md)
- [Performance Results](../operations/performance-results.md)
