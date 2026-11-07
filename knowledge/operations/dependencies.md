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
(`--no-default-features --features bash_tool,jq`; the former `scripted_tool`
feature was an alias for `bash_tool`, so the numbers are unchanged):

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

## Monty forks

Monty is not taken from crates.io. Upstream 1.x replaced the custom tracker
trait with a concrete tracker and per-VM memory accounting with a process-global
allocator probe, which breaks bashkit's in-process multi-tenant contract. Bashkit
embeds Monty 1.4.0 as two workspace forks, `bashkit-monty` and
`bashkit-monty-types` (lib names `monty`/`monty_types`), that add a memory probe
and a checkpoint hook. They are published with each release before `bashkit`.
Upgrade by re-copying upstream source and re-applying the `bashkit fork:`
edits. See [Python Builtin](../runtimes/python-builtin.md#monty-14-embedding-forks-per-vm-metering-checkpoint-hook).

`scripts/check_monty_fork.py` (CI lint job, `just check-monty-fork`) diffs both
forks against the SHA-256-pinned upstream 1.4.0 `.crate` files and fails on any
changed region without a `bashkit fork:` marker in it, in the comment or
attribute block attached to it (however long), or up to three lines above it,
on fork-only files without a marker header, on upstream files missing from
the fork (upstream tests excepted), and on any dependency or feature in a fork
`Cargo.toml` that differs from upstream's (the `monty-types` rename aside). Dependabot does not bump
crates only the forks declare, so upstream security fixes arrive by re-sync:

1. `scripts/check_monty_upstream.py` (daily `monty-upstream.yml`,
   `just check-monty-upstream`) goes red when crates.io has a newer stable
   `monty`/`monty-types` than the fork base, or when OSV (RustSec + GitHub
   advisories) lists a vulnerability for crates.io `monty`/`monty-types` or
   PyPI `pydantic-monty` at the base version. Name-matching tools (cargo
   audit/deny, Dependabot) miss both because the fork crates are renamed.
   A triaged advisory that does not affect bashkit goes in the script's
   `ACKNOWLEDGED` with its reason. `cargo deny` still covers advisories
   against the fork's own dependency versions (the Audit job).
2. For a fix: re-copy the new upstream release into both forks, re-apply every
   `bashkit fork:` edit (the drift check lists what moved), bump `VERSION` and
   the two hashes in `check_monty_fork.py`, and copy upstream's new
   dependencies and features into each fork `Cargo.toml` (the check compares
   them with upstream's normalized manifest).
3. Run `python_fuzz` and the tenant stress soak (`BASHKIT_STRESS_ROUNDS=300`,
   as nightly runs it) before shipping, as the fork's own regression net.

### A fork dependency may be marked ahead of upstream

A dependency's *version* may diverge when the fork's `Cargo.toml` line carries
a `bashkit fork: <why>` marker, in the comment block directly above it or at
the end of the line. Adding or dropping a dependency is never markable: re-sync
cannot reconcile a crate upstream does not have. A marker on a dependency that
matches upstream is itself an error, so a note left behind by a re-sync cannot
silently excuse the next bump. Unlike source edits there is no few-plain-lines
fallback: a dependency table is one entry per line, so the comment block is the
whole window, and one note cannot cover the entries around it.

This exists for the nine crates the forks share with bashkit's own crates
(`chrono`, `fancy-regex`, `getrandom`, `indexmap`, `num-bigint`, `num-traits`,
`serde`, `sha1`, `sha2`). They are declared twice, once in a fork and once in
the workspace, and Dependabot deliberately does not ignore them — bashkit has
to be able to move them and to receive their advisories. But a proposal for one
edits the fork manifest as well, so a strict fork-equals-upstream rule made
every such bump unmergeable: #2860 (fancy-regex 0.17 -> 0.20, getrandom
0.3 -> 0.4) is the shape, and it would have come back every week. Holding the
fork at upstream's version instead put two majors of fancy-regex in the graph.

`fancy-regex` and `getrandom` are marked today. Taking a shared bump therefore
means moving the fork line too, marking it, and re-applying whatever source
adaptation the new major needs — fancy-regex 0.20 made `Captures` generic over
the haystack type, so `re_match.rs` and `re_pattern.rs` carry marked
`Captures<'_, str>` edits. Because the `re` module is reachable from untrusted
Python, a major there can move Python-visible behavior:
`python_integration_tests::regex_semantics` pins the cases 0.20 changed
(`\xHH` above 0x7F matching the codepoint, word-boundary alternation) against
what CPython prints. fancy-regex's subroutine calls, whose compile-time
inlining 0.20's `max_prog_size` exists to bound, are not enabled by the fork's
default feature set, so no new compile-time blowup is reachable (TM-PY-033
still covers runtime backtracking).

Moving off 0.0.19 dropped the `jiter` Git patch (Monty 1.4 uses published
jiter 0.17 with pyo3 0.29), the `get-size2 =0.10.1` hold (Monty's ruff AST
moved to `compact_str` 0.10), and the `ruff_python_*` direct dependencies (the
`re` import gate is gone). The new third-party crates entered cargo-vet as
exemptions. `deny.toml` still bans `jiter/python`: its optional ASCII Python
string allocator dereferences `PyUnicode_New` without checking allocation
failure, and the feature stays out of the graph. The Python binding's
`num-bigint` must still match Monty's 0.4 line.

A workspace crate that is published to crates.io needs the same `policy.*`
entry from the other direction: `audit-as-crates-io = false`. cargo-vet calls a
member first-party by provenance -- it was not fetched from crates.io -- so once
the same name and version is also published, the two descriptions collide and
`cargo vet` refuses to run until an entry says which to believe. The gap is
quiet where it matters: CI runs `cargo vet --locked`, which never reaches that
check, so the Audit job stays green while the command is broken for every
developer and for refreshing `supply-chain/imports.lock` -- the non-locked run
is the only way to record publisher facts, which `[[trusted.*]]` crates such as
`tokio` need on every bump. `bashkit-scripted-tool` went public in 0.20.0 with
no entry and nothing failed until #2792 needed that refresh.
`scripts/tests/test_ci_supply_chain.py` derives the published set from the
`release-check` recipe and fails when one of those crates has no entry; a new
published crate must be added in both places.

## JavaScript test-tool audit boundary

The native JavaScript package constrains AVA's transitive file-tracing tools to
patched `glob` and `minimatch` versions. The upstream `@vercel/nft` constraint
still selects vulnerable glob 13, so the package's pnpm overrides carry the
security floor until upstream catches up. These dependencies belong to test
instrumentation, not the published runtime. Keep the normal release cooldown
and audit gate; never suppress the cancellation-bypass or pattern-amplification
advisories. See [package overrides](../../crates/bashkit-js/package.json).

## YAML maintenance boundary

The `jq` feature's `yq` builtin uses `serde_norway` and
`unsafe-libyaml-norway`, replacing `serde_yaml_ng` and the archived
`unsafe-libyaml` (RUSTSEC-2024-0449). This changes maintenance ownership rather
than the YAML contract: Serde values, multi-document iteration and YAML
serialization remain, as does the parser's recursion ceiling of 128.
Bashkit still rejects alias tokens before deserialization, limits conversion
depth to 100, and shares the jq evaluator's existing limits (TM-DOS-101).

The initial fork review compared the published `unsafe-libyaml` 0.2.11 library
with `unsafe-libyaml-norway` 0.2.15: implementation files are identical; the
library delta adds one lint allowance, while test executables also rename
imports and simplify one loop. Dependency vet exemptions retain the prior
criteria at exact replacement versions; no audit advisory was suppressed.
Future version bumps still require source and contract review.

## Feature-unification edges

`branches` is a direct dependency of `bashkit` under the `sqlite` feature and is
never referenced by any Bashkit code. It exists only to turn its `std` feature on
for the whole graph. `turso_core` depends on `branches` with
`default-features = false`, and the resulting `no_std` body of `branches::abort()`
is `core::intrinsics::abort()`, an unstable intrinsic rustc removed in
1.101.0-nightly. Stable CI stayed green; the nightly AddressSanitizer job went red
on an unchanged tree (nightly run #242, 2026-10-04), because that job is the only
one that both tracks floating nightly and builds `turso_core` — `bashkit-cli`
enables `sqlite` by default, and Miri only builds `bashkit --lib` without it.

There is no version to bump to: `branches` 0.5.0, its latest release, carries the
same line, and `turso_core` requires `^0.4.3` regardless. Cargo has no way to add
a feature to a transitive dependency other than a direct edge, so the edge is the
fix. Enabling `std` is also the behaviour this build wants on its own terms —
`abort()` becomes `std::process::abort()`, which raises `SIGABRT` and honours
registered abort handlers, instead of a bare trap — and it drops the unstable
intrinsic from the graph entirely. The edge adds no crate to the tree and enables
no runtime surface by itself.

Remove it when `branches` compiles on nightly without `std`.

## oxc is held at 0.117.x by the vendored ZapCode parser

`bashkit` declares five oxc crates (`oxc_parser`, `oxc_ast`, `oxc_allocator`,
`oxc_span`, `oxc_syntax`) for the `wasm32-unknown-unknown` TypeScript path,
where `zapcode_browser/parser` walks the oxc AST directly rather than going
through the published `zapcode-core`. The versions are chosen to match what
upstream ZapCode vendors, so the two sides of the same parser pipeline agree.

`zapcode-core` 1.5.1 requires `oxc ^0.117`, so it does not move when bashkit's
declaration does. #2758 bumped the declaration to 0.154.0 and Cargo resolved a
**second** copy of all 13 oxc crates — `oxc_parser`, `oxc_ast` and the rest
each appearing at both 0.117.0 and 0.154.0 — putting the vendored parser and
`zapcode-core` 37 minor versions apart. Two further costs came with it: oxc
0.154 requires rustc 1.97.0 against the 1.95.0 in `rust-toolchain.toml` (the
`WASM web package` job failed at resolution, the same shape as the wasmtime
49.x failure in #2708), and it added 19 unvetted crates, including
`compact_str` 0.10, `phf` 0.14 and `oxc_index` 5.0.

`oxc_*` is therefore in dependabot's `ignore` list outright, not by
update-type: every release past 0.117.x splits the graph the same way, so there
is no version to bump to. Lifting it waits on a `zapcode-core` release built
against a newer oxc, and then means bumping the pinned toolchain and porting
the vendored parser in one deliberate change — not inside an aggregated
`chore(deps)` PR.

## Dependabot rules that keep its jobs runnable

- No prerelease requirement over a stable lock: `turso_core = "0.8.0-pre.1"`
  with 0.8.3 locked made Dependabot treat 0.9.0-pre.2 as in range and fail the
  whole "/" job. Require the stable version; bump to a prerelease by hand.
- Directories that path-depend on `crates/bashkit` (hyperlight guest, the two
  random-fs fixtures) see bashkit's workspace deps as their own. Their entry
  `allow`s only the crates their manifests declare, with
  committed lockfiles (cargo accepts only `auto`/`lockfile-only` as
  `versioning-strategy`; `increase` made the whole config fail to parse).
- Crates only the Monty forks declare are ignored in the "/" entry: the forks
  keep Monty 1.4.0's versions until upstream source is re-copied. Shared deps
  stay bumpable.

`scripts/tests/test_dependabot_coverage.py` enforces all three.

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

## Build-only dependencies of `bashkit-cpython-wasm`

- `lzma-rs` (pure Rust, MIT) decodes the xz-compressed CPython snapshot in
  `build.rs`. Never linked into a binary. xz keeps the crate under the
  crates.io 10 MiB cap with more modules preloaded (see
  [CPython WebAssembly Runtime](../runtimes/cpython-wasm.md)).

## Vendored crates

- **`jaq-json`** is vendored into the jq builtin so its value operations can
  enforce memory limits (TM-DOS-110); `[patch.crates-io]` would not reach
  crates.io users. Its own dependencies (`bstr`, `bytes`, `foldhash`,
  `hifijson`, `indexmap`, `num-bigint`, `ryu`, `self_cell`) became direct
  optional deps of the `jq` feature, with upstream's version ranges — except
  `foldhash`, held at `0.2` against upstream's `^0.1` so the build links one
  copy instead of two (`hashbrown` requires 0.2). See
  [Vendored jaq-json](../runtimes/jaq-json-vendor.md) for the sync procedure.

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

Turning it off narrows the closed timezone set of `TM-INF-018` to UTC alone.
`date` then **reports that it cannot honour a named zone and exits 1**, rather
than formatting in UTC and returning a timestamp that looks right. Degrading
silently is the tempting choice and the wrong one: the caller cannot tell a
correct answer from a wrong one, and the failure surfaces later as bad data.
Unset, empty, `TZ=UTC`, and path-style values are unaffected — path-style forms
are a rejected request for a host zoneinfo file, not a request for a named
zone, so they resolve to UTC in every build and must not diverge with the
feature.

The diagnostic never echoes the `TZ` value. A `TZ` can be path-shaped, and
reflecting it back is exactly how host state leaks into output; the existing
`invalid_timezone_fails_closed_without_echoing_host_state` guards the same
property on the other side of the gate.

`tests/integration/date_timezone_no_tzdata_tests.rs` pins the off side; the
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
