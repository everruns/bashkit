---
type: Playbook
title: Binary Size
description: Binary size budget, measurement with cargo-bsize, and which size levers are adopted or rejected.
tags:
  - bashkit
  - maintenance
  - operations
  - performance
---

# Binary Size

## Status
Implemented

## Abstract

The `bashkit` CLI is a static binary that embeds a SQL engine, a Python
interpreter, a regex engine, a TLS stack, and ~300 builtins. That makes size a
standing budget rather than a one-off cleanup: releases ship it as a
`cargo binstall` target and as the payload of the container images, so growth is
paid for on every download.

This document records how size is measured, what the binary is currently made
of, and which levers are adopted, rejected, or blocked — so a future pass does
not re-derive the same rejections.

## Tooling

[`cargo-bsize`](https://github.com/Boshen/cargo-bsize) attributes a linked
binary back to crates, generics, formatting, panics, and constants. It builds
into its own `target/bsize` directory with debug info retained, so the numbers
it reports are for the *shipped* (stripped) size while the attribution comes
from the debug info.

```console
$ just bsize                  # full report
$ just bsize --limit 40       # deeper ranked lists
$ just bsize --what-if        # rebuild under each size lever and measure it
$ just bsize-check            # just the stripped byte count
```

The first `just bsize` is a full release build with debug info: roughly 12
minutes and several GB under `target/bsize`. Reruns off that cache take about
20 seconds, so iterate with reruns and only pay the rebuild when the code
changes.

Two properties of the report are easy to misread:

- The **Features** table resolves the whole workspace graph, not the one
  binary. `pyo3`, `napi`, and `tokio/test-util` appear there because other
  workspace members ask for them; they are not linked into the CLI. Use the
  **By crate, where the code is defined** table for what actually shipped.
- "Reached by no reference the graph can see" (~65% of the binary) is not dead
  code. The reference graph cannot follow vtables or the builtin dispatch
  table, and LTO has already dropped what is genuinely unreachable.

## Budget

| Target | Budget |
|---|---|
| `bashkit` CLI, release, stripped, x86_64-unknown-linux-gnu | ≤ 34 MB |

A maintenance pass that finds the binary over budget must either land a
reduction or record why the growth is accepted, in the table below.

### Measured baseline

`bashkit-cli` default features (`python`, `sqlite`, `interactive`) plus
`http_client`, `git`, `jq`; rustc 1.95.0; `x86_64-unknown-linux-gnu`.

| Date | Version | Stripped | Note |
|---|---|---:|---|
| 2026-08-19 | 0.16.0 | 33,034,880 B (31.50 MiB) | first recorded baseline |
| 2026-08-19 | 0.16.0 | 33,016,768 B (31.49 MiB) | turso `encryption` feature dropped |

Composition of the 31.5 MiB baseline:

| Share | Category |
|------:|---|
| 68.8% | code (`.text`) |
| 12.7% | read-only data |
|  8.0% | unwind and exception tables |
|  5.4% | data |
|  5.1% | dynamic relocations |

Where the code is defined, largest first: `turso_core` 6.1 MiB (19.4%),
`bashkit` 3.7 MiB (11.8%), `core` 3.0 MiB (9.5%), `monty` 1.6 MiB (5.2%),
`turso_parser` 527 KiB, `regex_automata` 407 KiB, `rustls` 387 KiB,
`clap_builder` 271 KiB, `ruff_python_parser` 253 KiB.

Rolled up by the feature that pulls them in:

| Feature | Cost | Share |
|---|---:|---:|
| `sqlite` (`turso_core` + `turso_parser`) | ~6.6 MiB | ~21% |
| `python` (`monty`, `monty-types`, `ruff_python_parser`, `unicode_names2` tables) | ~2.7 MiB | ~9% |
| `http_client` (`rustls`, `reqwest`, `ring`, `hyper`) | ~0.7 MiB | ~2% |

**The single largest fact about this binary is that one-fifth of it is the
embedded SQL engine, and `sqlite` is a `bashkit-cli` default feature while the
library keeps it off by default.** Any serious size decision starts there, and
it is a product decision, not a build-configuration one.

## Levers

### Adopted

| Lever | Effect |
|---|---|
| `strip = "symbols"` on `[profile.release]` | drops `.symtab`/`.strtab` and all debug sections; without it the binary is 333 MiB |
| `codegen-units = 1` on `[profile.release]` | fewer duplicated instantiations across CGUs |
| `lto = "thin"` on `[profile.release]` | cross-crate inlining and dead-code elimination |
| `turso_core` `default-features = false` | drops the `encryption` cipher paths (see [SQLite Builtin](../runtimes/sqlite-builtin.md)) |

### Rejected, and why

Every row carries the size it would actually return, measured by
`cargo bsize --what-if` (2026-08-19, against the 31.5 MiB baseline). The
rejections are about cost, not about the saving being small — do not
re-propose these without new information.

| Lever | Would save | Why not |
|---|---:|---|
| `opt-level = "z"` | **-7.3 MiB (23.3%)** | The largest lever available, and still the wrong trade. Bashkit's pitch is being faster than spawning `bash`; the functions that shrink most under it are the interpreter's sorts and `monty`'s VM loop, i.e. the hot paths [Performance Results](performance-results.md) is measured on. A static binary is downloaded once and run continuously. |
| `panic = "abort"` | **-3.2 MiB (10.1%)** | The interpreter relies on `catch_unwind` around builtins; aborting turns a contained builtin panic into a process kill for the whole host. The 8% spent on unwind tables buys that containment. See [Threat Model](../security/threat-model.md). |
| `-Zfmt-debug=none` | **-1.3 MiB (4.1%)** | Nightly-only, and the toolchain is pinned to stable in `rust-toolchain.toml`. Worth re-checking if it ever stabilises: `Debug` impls are 962 KiB (3.0%) and builtins are already forbidden from emitting `{:?}` (TM-INF-022). |
| `-Zlocation-detail=none`, `build-std` | not measured | Nightly-only, same reason. |
| Linker ICF (`--icf=all`) | ~68.7 KiB (0.2%) | Only 68.7 KiB of byte-identical function bodies and 11.0 KiB of duplicate constants — not worth a non-default linker in the release path. |
| Dropping turso's `fs` feature | unknown | `turso_core` 0.8.0-pre.4 does not compile without it: the no-`fs` path drops `Database::open_file_with_flags`, which `vdbe/vacuum.rs`, `vdbe/execute.rs`, `database.rs`, and `connection.rs` (ATTACH) still call unconditionally. Bashkit never uses turso's host-filesystem `IO`, so re-check this on every turso bump — it is a size win *and* removes a host-filesystem path from the sandbox. |

### Blocked upstream

- **AEGIS/AES-GCM cipher matrix (~150 KiB of `state_mac_final` / `decrypt_detached`
  instantiations, plus 21 `libaegis_*_implementation` relocation tables).**
  Turning off turso's `encryption` feature gates the Rust call sites but the
  `aegis` and `aes-gcm` crates are unconditional dependencies of `turso_core`,
  and libaegis's C objects link regardless. Only ~18 KB came back. Recovering
  the rest needs turso to make those dependencies optional.
- **`unicode_names2` tables (666 KiB of constants, 2.1%).** Pulled by `monty`
  for `unicodedata.name()`. Not reachable from a downstream feature flag.
- **Duplicate `fancy-regex` (0.17 from `monty`, 0.19 from `bashkit`) — 300 KiB
  for two copies of the same engine.** Resolves when `monty` bumps.

## When to Run

- During every pre-release maintenance pass, as part of
  [Maintenance](maintenance.md) § Binary Size.
- After adding a dependency that is not behind a feature flag.
- After enabling a new default feature on `bashkit` or `bashkit-cli`.

A size regression is reviewed the same way a benchmark regression is: measured,
attributed with `just bsize --baseline`, and either fixed or recorded above with
its justification.

## See also

- [Maintenance](maintenance.md), the pre-release pass that runs this check
- [Performance Results](performance-results.md), the sibling budget for speed
- [SQLite Builtin](../runtimes/sqlite-builtin.md), the largest single contributor
- [Python Builtin](../runtimes/python-builtin.md), the second largest
- [Release Process](release-process.md), where the binary is published
