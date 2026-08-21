---
type: Playbook
title: Dependency Policy
description: Which third-party crates Bashkit keeps, which it trims, and which are deliberately not reimplemented in-house.
tags:
  - bashkit
  - dependencies
  - supply-chain
  - build
---

# Dependency Policy

## Status
Implemented

Bashkit is embedded by other people's programs, so every crate in the tree is a
crate a downstream consumer inherits, audits, and receives advisories for. The
default posture is therefore to keep the graph small — but "small" means *audit
surface*, not binary size, and the two are measured separately because they do
not move together.

## What the graph actually costs

Measured on `bashkit` at the browser feature set
(`--no-default-features --features scripted_tool,jq`):

| Configuration | Crates |
|---|---|
| `--no-default-features` | 112 → 91 after the trims below |
| `+ jq` | +21 |
| `+ http_client` | +36 |
| `+ typescript` | +47 |
| `+ python` | +67 |
| `+ ssh` | +89 |
| `+ sqlite` | +89 |

The embedded-runtime features dominate, and each is already off by default and
gated. The always-on core is where trimming is worth the effort.

## Trims in effect

Each is documented at its definition in the root `Cargo.toml`; the rationale
lives there, next to the pin, so it cannot drift away from what it explains.

- **`idna_adapter` pinned to 1.0.0** (lockfile-only). Removes the 21-crate
  ICU4X stack that `url` pulls in via `idna`. Consequence: non-ASCII domain
  names are rejected rather than normalised per UTS 46 — hardening for a
  sandbox allowlist, since it removes the homograph and punycode-confusion
  class from host matching.
- **`clap` with `default-features = false`.** Drops `color` and `suggestions`:
  7 crates whose job is to detect and paint a host TTY. Builtins write to the
  sandbox's virtual stdout, which is never a terminal.
- **`futures-util` with `default-features = false`.** Bashkit uses `StreamExt`,
  `FutureExt`, and `select`/`pin_mut!` only.

## Measure size and audit surface separately

The `idna_adapter` pin removes 21 crates but only **15 KB (0.16%)** from the
release `wasm32-unknown-unknown` browser build, because LTO already dead-strips
the ICU tables — they sit behind lazy statics the linker proves unreachable.
Source-tree size is not a proxy for shipped bytes: `icu_properties_data` is
1.9 MB of source and near-zero shipped weight.

So: justify a dependency trim by audit surface, cold build time, and build
output size — and measure the binary before claiming anything about it. The
same pin is worth ~10% off a cold `cargo check` and ~70 MB of build output.

A trim also may not apply to every configuration. `turso_core` pulls
`icu_collator`, so any build with the `sqlite` feature keeps the ICU stack
regardless of the `idna_adapter` pin.

## Deliberately not reimplemented

Re-implementing a dependency in-house is occasionally right — the vendored
uucore argument surfaces in [Coreutils Args Port](../runtimes/coreutils-args-port.md)
are the precedent — but for the current tree it is not, and the reasons are
worth keeping so the question does not get re-litigated from scratch.

- **`url`** — the strongest-looking candidate and the wrong one. `reqwest`
  depends on `url` regardless, so under `http_client` a hand-rolled parser
  does not leave the tree; it only introduces a **parser differential between
  the URL Bashkit validates against the allowlist and the URL that actually
  gets fetched**, which is an SSRF allowlist bypass. `network/allowlist.rs`
  carries no `cfg(http_client)` gate — it is always compiled and always public
  API. After the `idna_adapter` pin `url` costs 6 crates. Keep it.
- **`flate2`, `bzip2`** — already pure Rust (`miniz_oxide`, `libbz2-rs-sys`),
  4 and 1 unique crates. Reimplementing DEFLATE or bzip2 is weeks of work
  against a compatibility surface with no margin for error.
- **`clap`** — used across ~25 files and emitted against by the coreutils port
  codegen. Trim its features; do not replace it.
- **`bigdecimal` / `num-bigint`** — back `printf`'s arbitrary precision. Three
  unique crates is a poor trade against bignum decimal correctness.
- **`md-5`, `sha1`, `sha2`, `hmac`** — zero unique crates each; they share the
  RustCrypto `digest` core. Reimplementing saves nothing and it is crypto.
- **`os_display`, `unit-prefix`** — genuinely trivial (3 crates, and
  `shell_quote` in `builtins/generated/format_support.rs` already covers most
  of `Quotable`), but they are imported by *generated* coreutils-port files, so
  removal means carrying rewrite rules in the porter. Low return.

## Gate rather than reimplement

Where a dependency is large but its domain is not something to reimplement, the
lever is a feature gate. `chrono-tz` is the open example: three lines of use in
`builtins/date.rs` for `TZ=` parsing, and the largest single artifact in the
tree (an 18.6 MB rlib of IANA tables). Timezone rules are not a reimplementation
target; a `tzdata` feature, off for the browser build, is.

## See also

* [Maintenance](maintenance.md) - Pre-release dependency and security maintenance requirements.
* [Coreutils Args Port](../runtimes/coreutils-args-port.md) - The in-house vendoring precedent, and its rewrite-rule machinery.
* [Browser Package](../runtimes/browser-package.md) - The slim wasm build whose feature set the trims are measured against.
