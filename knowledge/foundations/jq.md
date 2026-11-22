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
form before strict serde validation of the requested value or token. The pass tracks JSON string
and escape state, preserves existing escapes and structural whitespace, and
handles NDJSON and concatenated JSON values without line-based splitting.

This is intentionally not a general JSON policy. The `json` builtin,
`serde_json` defaults, tool request/response contracts, `--argjson`, and
`--jsonargs` remain strict. jq raw-input modes do not parse JSON and therefore
do not normalize their bytes. Main jq input from stdin or files and
`--slurpfile` use the compatibility boundary; `--rawfile` remains raw.

Malformed quoting is rejected by the boundary with a stable jq-shaped error.
Controls outside strings remain untouched, so only JSON whitespace is accepted
there by the strict parser.
The ordinary value splitter and streaming token reader share that four-byte
whitespace definition. Other controls, including form-feed, stay in an invalid
scalar token so they cannot split a bad number into multiple valid inputs or
produce partial scalar output before rejection.

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
`--stream` emits keys and duplicate-key events in encounter order directly from
tokens, without constructing an object tree.

## Resource accounting

Normalization is a bounded single pass per requested value/token. It charges input length to the shared
request work budget before scanning. Inputs without affected controls remain
borrowed and allocate nothing. On the first rewrite it reserves the exact
current normalized size from the live-intermediate budget before allocation;
each later five-byte expansion grows that lease before the buffer grows. The
lease remains live until strict parsing finishes (TM-DOS-100).

## Filter compilation

One `Bash::exec` can reuse up to four compiled inline filters. A key combines
the filter source and ordered global binding names because compilation assigns
variable slots in that order. Each call supplies its current inputs, binding
values, environment, and evaluation limits. On insertion at capacity, the
least recently used plan is evicted. Each cached source can contain up to
1 KiB and bind up to 16 names totaling up to 1 KiB. The execution scope
drops the cache when the request completes or is cancelled. File filters
are read and compiled on every call. The cache lock covers only lookup
and replacement.

The fixed jaq library and Bashkit compatibility definitions are parsed once
per process and cloned into the loader when a filter needs compilation. The
helpers that read invocation globals stay in the main expression, preserving
the grammar that rejects module directives in user filters. The compiler
resolves and compiles each uncached filter independently. Benchmark repeated,
interleaved, distinct, and one-off filters through
`cargo bench -p bashkit --features jq --bench jq`.

## Verification

`builtins::jq::tests` covers all 32 controls, newline/tab/CR, adjacent existing
escapes, structural controls, NDJSON/concatenated values, malformed quoting,
stdin/file/slurpfile paths, exact work/live-memory boundaries, debug-leak
invariants, and differential filter/output semantics against real jq.
`spec_cases/jq/jq.test.sh` `jq_object_key_order_*` cases pin key order for
input, `del`, duplicate keys, `-S`, `--argjson` and `--slurpfile`.
`builtins::json::tests::literal_control_in_string_remains_invalid_json` proves
the exception does not cross into the strict `json` builtin.

## See also

- [Builtin Commands](builtins.md), builtin execution and request-budget context
- [Known Limitations](../operations/limitations.md), jq divergences and strict-boundary stance
- [Threat Model](../security/threat-model.md), TM-DOS-100 resource-exhaustion mitigation
