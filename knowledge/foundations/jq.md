---
type: Subsystem Design
title: jq Input Compatibility
description: jq JSON input normalization, strict-boundary scope, and resource-accounting rules.
tags:
  - bashkit
  - jq
  - json
---

# jq Input Compatibility

## Status

Implemented.

## Decision

The `jq` builtin accepts literal U+0000 through U+001F controls inside JSON
strings. A jq-only input-boundary pass rewrites each such byte to its `\u00XX`
form before the strict serde stream parser sees it. The pass tracks JSON string
and escape state, preserves existing escapes and structural whitespace, and
handles NDJSON and concatenated JSON values without splitting the stream.

This is intentionally not a general JSON policy. The `json` builtin,
`serde_json` defaults, tool request/response contracts, `--argjson`, and
`--jsonargs` remain strict. jq raw-input modes do not parse JSON and therefore
do not normalize their bytes. Main jq input from stdin or files and
`--slurpfile` use the compatibility boundary; `--rawfile` remains raw.

Malformed quoting is rejected by the boundary with a stable jq-shaped error.
Controls outside strings remain untouched, so only JSON whitespace is accepted
there by the strict parser.

## Object key order

Real jq keeps object keys in input order and only sorts under `-S`. The jq
builtin therefore never routes JSON through `serde_json::Value`, whose map is
sorted unless the crate-wide `preserve_order` feature is on (flipping it would
change every other `serde_json` user in the workspace). Instead
`convert::parse_json_stream` and `convert::parse_json_value` deserialize
straight into `JqJson` with a depth-tracking `DeserializeSeed`, keeping keys as
an ordered `Vec`. A duplicate key keeps its first position and takes the last
value, matching jq. This covers main input, `--slurpfile`, `--argjson`,
`--jsonargs` and `$ARGS`. `$ENV`/`env` stay sorted by name for determinism.
The nesting cap (TM-DOS-027) is enforced while reading; a depth violation is
reported verbatim, not as a generic parse error.

## Number precision

We do not enable `serde_json/arbitrary_precision` because it changes `Number`
semantics crate-wide. Cargo feature unification can still turn it on when
another crate in the build enables it. With it on, non-integer numbers no
longer arrive via `visit_f64`; they arrive as a single-entry map with the
private key `$serde_json::private::Number` whose value is the number text.
`JqSeed::visit_map` detects that shape only when the build actually enabled
the feature (detected once at runtime via `"2.50"` round-trip) and returns
`JqJson::Number` with the exact text, so `jq` input, `--argjson`, `--jsonargs`,
and streams keep working in either build. A real single-entry object with that
private key stays an `Object` when the feature is off; with it on, the two are
indistinguishable (same as `serde_json::Value`) and it becomes a `Number`.
With the feature on, the exact token is preserved (for
example `2.50` stays `2.50`, matching real jq); without it, floats normalize
through `f64` (for example `2.50` becomes `2.5`).

## Resource accounting

Normalization is a bounded single pass. It charges input length to the shared
request work budget before scanning. Inputs without affected controls remain
borrowed and allocate nothing. On the first rewrite it reserves the exact
current normalized size from the live-intermediate budget before allocation;
each later five-byte expansion grows that lease before the buffer grows. The
lease remains live until strict parsing finishes (TM-DOS-100).

## Verification

`builtins::jq::tests` covers all 32 controls, newline/tab/CR, adjacent existing
escapes, structural controls, NDJSON/concatenated values, malformed quoting,
stdin/file/slurpfile paths, exact work/live-memory boundaries, debug-leak
invariants, and differential filter/output semantics against real jq.
`spec_cases/jq/jq.test.sh` `jq_object_key_order_*` cases pin key order for
input, `del`, duplicate keys, `-S`, `--argjson` and `--slurpfile`.
`builtins::jq::convert::tests::arbitrary_precision_number_shape_becomes_number`
pins the private-token map shape (forced on, so green in any build),
`token_led_object_with_second_key_stays_object` pins the two-entry fall-through,
and `single_entry_token_object_matches_build_mode` pins that a single-entry
private-token object stays an `Object` without the feature and becomes a
`Number` with it.
`builtins::json::tests::literal_control_in_string_remains_invalid_json` proves
the exception does not cross into the strict `json` builtin.

## See also

- [Builtin Commands](builtins.md), builtin execution and request-budget context
- [Known Limitations](../operations/limitations.md), jq divergences and strict-boundary stance
- [Threat Model](../security/threat-model.md), TM-DOS-100 resource-exhaustion mitigation
