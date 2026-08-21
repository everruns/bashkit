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
- **`chrono-tz` behind the `tzdata` feature** (on by default). Worth 911 KB of
  the browser wasm binary. See
  [Gate rather than reimplement](#gate-rather-than-reimplement).

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
lever is a feature gate rather than an in-house rewrite.

`chrono-tz` is the worked example. It backs three lines of `builtins/date.rs`
(`TZ=` parsing) and is the largest single artifact in the tree — an 18.6 MB
rlib of compiled IANA tables that costs **911 KB (9.7%)** of the release
`wasm32-unknown-unknown` browser binary, 9,361,014 → 8,450,035 bytes.

That number is the contrast with the `idna_adapter` pin above, and the reason
the two are justified differently: ICU's tables are reachable only through code
paths LTO can prove dead, so they cost 15 KB; chrono-tz's are reached from a
`FromStr` over the whole zone table, so the linker must keep them. Whether a
large data dependency actually ships depends on how it is reached, which is why
the rule is to measure rather than reason from source size. Timezone rules are emphatically not a
reimplementation target, so it sits behind the **`tzdata`** feature, on by
default and listed explicitly by `bashkit-cli` and `bashkit-capi`, and
deliberately absent from `bashkit-wasm`.

Turning it off narrows the closed timezone set of `TM-INF-018` to UTC alone. It
does not open a hole: a named zone resolves the way an unrecognised one always
has — to UTC — rather than to host-local state, so `date` stays fail-closed.
The observable change is that `TZ=America/Chicago` reads as UTC instead of CST.
`tests/integration/date_timezone_no_tzdata_tests.rs` pins that side; the
`tzdata`-on modules beside it pin the other, and `main.rs` selects between them
so both configurations have coverage rather than one silently losing it.

The generalisable rule: when a dependency is large, narrow in use, and wraps a
domain with real correctness stakes (timezones, compression, crypto, Unicode),
gate it and give both sides of the gate tests. Do not reimplement it, and do not
leave the gated-off configuration untested.

## See also

* [Maintenance](maintenance.md) - Pre-release dependency and security maintenance requirements.
* [Coreutils Args Port](../runtimes/coreutils-args-port.md) - The in-house vendoring precedent, and its rewrite-rule machinery.
* [Browser Package](../runtimes/browser-package.md) - The slim wasm build whose feature set the trims are measured against.
