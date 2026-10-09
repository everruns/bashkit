---
type: Subsystem Design
title: Wasm Coreutils
description: Real uutils/coreutils programs compiled to wasm32-wasip1, precompiled to Pulley and run as sandboxed builtins over the bashkit VFS on the shared WASI host.
tags:
  - bashkit
  - builtins
  - coreutils
  - runtime
  - sandbox
  - wasm
---

# Wasm Coreutils

## Status
Experimental (`wasm-coreutils` feature, off by default)

## Decision

Bashkit can run real, precompiled coreutils programs (uutils 0.12) instead of
reimplementing them, without giving up in-process multi-tenant isolation:
the programs are compiled to `wasm32-wasip1` and run as WebAssembly guests
on the same WASI host as CPython ([CPython WebAssembly Runtime](cpython-wasm.md)).
The wasm boundary is the sandbox; the guest can only reach the VFS, captured
stdio, clocks and a random source.

Public guide: `crates/bashkit/docs/wasm-coreutils.md` (rustdoc
`wasm_coreutils_guide`).

### Why not native binaries (LiteBox and similar)

Running unmodified ELF binaries in-process was evaluated with
[microsoft/litebox](https://github.com/microsoft/litebox) (2026-10-08) and
rejected for bashkit's model (many tenants in one process, fast startup):

- Its Linux userland platform installs a process-wide seccomp filter
  (`apply_filter_all_threads`), confining the embedding host too.
- A library OS runs guest machine code in the host's address space: a
  malicious binary can read other tenants' memory and injected secrets, or
  corrupt memory an unfiltered thread later uses. Seccomp limits syscalls,
  not memory.
- Global signal-handler state (one guest per process), Linux x86-64 only.

Native binaries would need one sandbox process per call or tenant. Wasm
gives memory isolation in-process on every platform, so "real programs" in
bashkit means programs compiled to WASI.

### Shape

| Piece | Where | Role |
|-------|-------|------|
| `bashkit-coreutils-wasm` crate | `crates/bashkit-coreutils-wasm/` | Ships bytes only: precompiled module, utility list, `engine_config()` |
| Guest | `crates/bashkit-coreutils-wasm/guest/` | Multicall crate over `uu_*` 0.12.0; `gen.py` generates it from `utils.txt`, `build.sh` builds + xz-compresses it |
| Shared WASI host | `crates/bashkit/src/builtins/wasi_host/` | `wasi.rs` (all preview1 imports over the VFS) + `mod.rs` (engine, pool, fuel/deadline/budget polling, exit mapping); used by CPython too |
| Builtin | `crates/bashkit/src/builtins/wasm_coreutils.rs` | `WasmCoreutil` per utility + `coreutils` multicall, limits, gap list |

### Registration

- `BashBuilder::wasm_coreutils()` registers the multicall entry
  `coreutils <util> [args...]` (any guest utility, `coreutils --list`) plus
  the guest utilities with no native builtin (`MISSING_NATIVE`: `csplit`,
  `dir`, `dircolors`, `pathchk`, `ptx`, `shred`, `vdir`). Native builtins
  stay the default: they are faster and integrate with streaming and
  bashkit's own options.
- `wasm_coreutils_replace_native(limits)` registers every guest utility by
  its own name, replacing the native builtin (GNU-compatible behavior, e.g.
  `sort -h`). Mainly for differential testing.
- `MISSING_NATIVE` is checked by a unit test against the guest list and the
  native builtin set, so adding either side updates it.

### Guest

- Own `main`, not uutils' `coreutils` binary: wasi-libc starts every process
  at `/`, so the guest applies `PWD` with `set_current_dir` before dispatch;
  `argv[0]` names the utility; unknown names exit 127.
- Utilities: uutils' `feat_wasm` set minus those that only report on or wait
  for the host (`uname`, `arch`, `tty`, `hostid`, `nice`, `sleep`, `yes`):
  71 utilities. `tac` builds but panics on stdin (needs
  `std::env::temp_dir`, which panics on WASI); `stat`, `du`, `df`, `id`,
  `install`, `chown`, `timeout` and similar do not build for wasip1.
- No Wizer snapshot: a Rust `main` has no interpreter state worth
  pre-initializing.
- Locale strings are embedded by uucore (English); no files are read.

### Startup and engine

Same rules as CPython: nothing compiled at run time (`build.rs` compiles the
xz-decoded guest to Pulley with Cranelift as a build-dependency), the
`.cwasm` (15.8 MB) is embedded 64 KiB-aligned and mapped in place on first
load, instances come from a pooling allocator (512 slots). Differences:

- 256 MiB `memory_reservation` (CPython: 1 GiB); default `max_memory`
  64 MiB, `max_duration` 10 s, `max_output` 16 MiB.
- 1 MiB wasm stack; entry is the WASI command `_start`, the status comes
  from `proc_exit`.
- A separate engine and pool from CPython's (the modules have different
  compile-affecting configs).

### Shared WASI host

The CPython WASI host moved from `builtins/cpython/wasi.rs` to
`builtins/wasi_host/` and is compiled with either feature. Generalized:

- The read-only file overlay (CPython's stdlib zip) is optional
  (`GuestConfig::overlay`).
- The HTTP bridge state exists only with `cpython`.
- Engine/pool setup, the deadline- and budget-checked poll loop, slot
  waiting, exit and trap mapping are one `wasi_host::run` used by both
  builtins. Trap messages are `<name>: fatal error: ...` with the guest noun
  (`interpreter`/`program`), Display-only (TM-INF-022).

Any further WASI program (jq, sqlite CLI, busybox applets...) needs only a
bytes crate and a thin builtin on top of `wasi_host`.

## Measurements

See `criterion-wasm-coreutils-*` under `crates/bashkit/benches/results/`
(`just bench-wasm-coreutils`).

4-vCPU x86-64 VM, release build, 2026-10-09
(`criterion-wasm-coreutils-vm-linux-x86_64-1791511758.md`):

| Measure | Native builtin | Wasm (uutils on Pulley) |
|---------|----------------|-------------------------|
| First call in a fresh process (engine + module load) | 0.04 ms | 6.7 ms |
| `echo` / `cat` / `wc -l` / `sha256sum`, small file, warm | 10-25 µs | 2.3-3.9 ms |
| `sort`, 100 lines | 32 µs | 7.8 ms |
| `cat \| sort -r \| head` | 105 µs | 14.9 ms |
| `cat` 1 MiB | 519 MiB/s | 165 MiB/s |
| `wc` / `sha256sum` 1 MiB | ~187 MiB/s | 4.5-5.2 MiB/s |
| `sort` 1 MiB | 56 MiB/s | 1.4 MiB/s |
| New `Bash` + one `sort` | 90 µs | 6.8 ms |
| 64 concurrent sessions, one call each | 30.5 k/s | 322 /s |

Instantiation is ~9 µs (pooled, copy-on-write). The per-call floor is
uucore's localization setup: `true` executes ~451 k guest instructions,
~11.5 k of them its own work; skipping the setup cut a call from 2.2 ms to
0.14 ms, but then `--help` and some errors print message keys
(`expr-error-missing-operand`), so it stays (measured with a temporary
guest switch). Follow-up: pre-parse uucore's shared Fluent resources in a
Wizer snapshot (needs a small uucore patch in the guest workspace). Compute
is 40-50x slower than native on Pulley; a `cpython-native`-style opt-in
would close part of that.

The guest build is reproducible: rebuilding from `guest/` with the pinned
toolchain gives the same `coreutils.wasm.xz` hash.

## Limitations

L-WCU-* in [Limitations](../operations/limitations.md); threats TM-WCU-* in
the [threat model](../security/threat-model.md).

## Testing

| Layer | Where |
|-------|-------|
| Unit | `builtins/wasm_coreutils.rs` tests (limits, gap list vs native set, runtime loads), `wasi_host` trap/timeout messages, `bashkit-coreutils-wasm` lib tests |
| Integration + security | `tests/integration/wasm_coreutils_tests.rs` (registration, multicall, VFS read/write/cwd, pipelines, exit codes, env, host isolation, limits, tenant isolation, concurrency) |
| CPython regression | the CPython suites run unchanged on the shared host |
| Bench | `benches/wasm_coreutils.rs` (asserts native and wasm outputs match before timing), `examples/wasm_coreutils_startup.rs` |

## Rebuilding the guest

`crates/bashkit-coreutils-wasm/guest/build.sh` (needs rustup with the pinned
toolchain, python3, xz). Edit `guest/utils.txt` to change the utility set;
`gen.py` regenerates `Cargo.toml` and `src/main.rs`. `artifacts/MANIFEST`
records the uutils version, rustc and hashes.
