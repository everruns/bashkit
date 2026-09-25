---
type: Subsystem Design
title: Vendored jaq-json
description: Why and how jq's JSON value type is vendored from upstream jaq-json, the memory meter patched into it, and how to sync upstream releases.
tags:
  - bashkit
  - jq
  - vendoring
  - security
---

# Vendored jaq-json

## Status
Active. Vendored release: `crates/bashkit/src/builtins/jq/jaq_json/UPSTREAM_VERSION`.

## Decision

The jq builtin runs on the jaq crates. `jaq-core` and `jaq-std` stay normal
dependencies; `jaq-json`, which defines the JSON value type `Val` and its
operations (`+`, `*`, arrays, objects, `tojson`), is copied into
`crates/bashkit/src/builtins/jq/jaq_json/`.

Why vendor (#2444, TM-DOS-110): a jq value could grow inside one evaluation
(`"x" | until(false; . + .)`, `"x" * 1e12`, `[range(1e12)]`) until the host's
allocator failed and the embedding process aborted. The value operations live in
jaq-json and have no size hook. The alternatives were weighed and rejected:

- A `[patch.crates-io]` override is ignored when bashkit is used from
  crates.io, so it would protect only builds from this repository.
- A wrapper value type in bashkit would re-implement jaq-json's native
  functions and still miss the growth inside jaq-core's string interpolation.
- Waiting for an upstream hook leaves every package exposed until a release.

The fix is also offered upstream; once jaq ships an equivalent hook the vendored
copy can go back to being a dependency.

## Layout

| Path | Content |
|---|---|
| `jaq_json/*.rs`, `defs.jq` | Upstream sources, mechanically adapted (below) plus `BASHKIT PATCH` hunks |
| `jaq_json/meter.rs` | Bashkit-only: the live-memory meter |
| `jaq_json/tests/` | Upstream's integration tests, run as unit tests |
| `jaq_json/UPSTREAM_VERSION` | The upstream release the copy tracks |
| `scripts/sync-jaq-json.sh` | Adaptation and sync tool |

The module is declared with `#[rustfmt::skip]` and lints allowed, so it stays
byte-close to upstream. Credit and license: jaq-json is MIT licensed,
by Michael Färber (<https://github.com/01mf02/jaq>); see `NOTICE` and
`THIRD_PARTY_LICENSES/MIT.txt`.

### Mechanical adaptation

`scripts/sync-jaq-json.sh` turns the published crate into a module, so the
same rewrite applies to every release:

- `crate::` becomes `self::` / `super::`, `alloc::` becomes `std::`.
- Cargo features resolve statically: `std` always on, `sync` never, `serde`
  only under `cfg(test)` for the upstream tests.
- `#[macro_export]` is dropped, so the writer macros do not leak into
  bashkit's public API; `$crate::` paths point at the module.

## The meter

`meter.rs` holds the bytes live values own right now, per jq run:

- Every freshly allocated string (`From<String>`, concatenation, repetition,
  `tojson`, parsed strings, natives via `from_utf8_bytes`) is wrapped in a
  `Bytes` owner that charges its length and releases it on drop.
- Array and object bodies are `Metered<Vec<Val>>` / `Metered<Map>`: charged
  on creation and clone, re-charged by `resync()` after every in-place
  mutation, released on drop.
- Growth operations check the meter *before* allocating and fail with the jq
  error `value size limit (N bytes) exceeded` (exit 5). The limit is
  `ExecutionLimits::max_live_intermediate_bytes` (default 32 MB).
- The meter is sticky: once tripped, `try ... catch` cannot keep a loop going.
- Where there is no error channel (infallible `FromIterator`, `From<String>`)
  and for loops that never emit a value (`until(false; .)`), the run is
  aborted by `resume_unwind` with a `meter::Abort` payload (no panic hook
  runs) and the jq builtin catches it. Value operations `tick`; every 4096
  ticks the execution deadline is polled, so non-emitting loops end at the
  timeout (`jq: execution timed out`).
- Output conversion is capped by `max_stdout_bytes`, so a value built from
  shared parts (`reduce range(40) as $i (1; [., .])`) cannot expand when
  rendered. Runtime error messages render at most 64 KiB of a value.

Builds with `panic = "abort"` cannot unwind. There the fallible checks still
apply, over-limit infallible constructions are cut short with the meter
tripped, and a non-emitting loop is not interrupted.

### Known gaps

- Filter recursion without value operations (`def f: f; f`) and dropping
  very deeply nested values overflow the stack. This predates the vendoring
  and is a stack-depth issue, not memory growth.

## Syncing upstream

Part of the [maintenance](../operations/maintenance.md) pass:

1. `scripts/sync-jaq-json.sh --check` reports whether a newer jaq-json is out.
2. `scripts/sync-jaq-json.sh <version>` adapts the vendored and the new
   release the same way and applies the upstream diff onto the vendored
   copy. `BASHKIT PATCH` hunks survive; conflicts are left as `.rej` files.
3. Re-check every `BASHKIT PATCH`: new growth paths (new natives, new
   container mutations) need a meter check or `resync()`.
4. Match jaq-json's `Cargo.toml` dependency ranges in
   `crates/bashkit/Cargo.toml`, and keep `jaq-core`/`jaq-std` at the versions
   that release requires.
5. Run the jq tests (upstream ones included) and `memory_growth_security_tests`.

## See also

- [Threat Model](../security/threat-model.md), TM-DOS-110
- [Dependencies](../operations/dependencies.md)
- [Maintenance](../operations/maintenance.md)
- [Coreutils Argument Port](coreutils-args-port.md), the other vendoring precedent
