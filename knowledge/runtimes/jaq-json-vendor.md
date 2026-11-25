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

### Experimental owned boundary

`experimental_jq` is an opt-in Cargo feature on `bashkit` and `bashkit-cli`.
It implies `jq`; no default or existing feature enables it. Both jq and yq
use the selected value runtime. The first owned layer is array/object storage
in `builtins/jq/owned.rs`, compiled and linted as Bashkit code outside the
vendored module. This deliberately reuses jaq's parser, evaluator, standard
library, value arithmetic and Bashkit's compatibility definitions. It is not
a replacement jq language implementation or a performance claim.

The [first paired benchmark](../../crates/bashkit/benches/results/criterion-jq-owned-cloud-linux-x86_64-1795639828.md)
records about 9% overhead for array updates and 13% for object updates in an
exploratory single pair. Keep the feature disabled by default; these results
do not establish a speedup.

`Val` selects `Container<T>` instead of raw `Metered<T>` bodies under the
feature. Its private body and immutable `Deref` prohibit mutable entry handles,
mutable child references and unchecked growth. Type-specific set, insert,
splice, append, remove and merge operations check child depth before attachment,
invalidate depth on mutation, admit capacity growth before reserve, and resync
charges before returning. COW cloning admits the copied body before allocation.
Array/object collection, `fromjson`, input/global conversion and whole-object
updates build incrementally charged containers. Subtraction uses a charged
sorted reference index. The low-level meter and
string representation remain shared with the established path. Conservative
map-growth admission can fail earlier near the byte limit; resource-limit
differences are explicit exceptions to real-jq compatibility.

Consuming a body transfers its storage charge to its owning iterator. Partial
consumption, exhaustion without dropping the iterator, early errors and unwind
must retain/refund that charge with the underlying buffer, rather than refunding
as soon as the body is converted into an iterator.

The experiment also polls the shared stop check during value equality,
ordering, hashing, containment, byte conversion and formatting. Depth-bounded
shared trees can still require exponential traversal; a nesting bound alone
does not make those operations interruptible. Deterministic stop-check tests
use small independently constructed shared trees, so a polling regression
fails without hanging the test process. Object ordering/hashing scratch
storage is admitted and charged for its lifetime.

Evaluator safety remains a separate boundary: `InputDataRef::clone` caps live
contexts and polls the execution deadline, while value operations poll the
same stop check. In experimental builds the stop check also checks the shared
request budget for cancellation, closed requests and prior budget failures;
the typed request error survives the unwind instead of becoming a timeout.
This experiment does not replace the evaluator controls or remove
the `panic = "unwind"` requirement for in-process interruption. A future
evaluator replacement needs its own audit and lifecycle tests.

Run the same jq differential, depth, memory, cancellation, streaming and yq
suites with `jq` and with `experimental_jq`. Owned API unit tests additionally
pin rejection before COW, pre-attachment depth checks, incremental capacity
charging, owning-iterator retention, release on drop and cancellation during a
synchronous non-emitting loop followed by session recovery.
The host resets its cancellation token before that next request, as required
by the public token contract.
`diff_owned_container_lifecycle` compares
construction, mutation/COW, merge, traversal, comparison, formatting and error
cleanup with real jq. CI exercises the experimental configuration separately.
Keep vendor sync patches at the selection/call sites; `owned.rs` is maintained
by Bashkit rather than overwritten from upstream.

```bash
cargo test -p bashkit --no-default-features --features experimental_jq --lib --test integration -- jq yq
cargo bench -p bashkit --no-default-features --features experimental_jq --bench jq
# From crates/bashkit/fuzz, with cargo-fuzz and a supported nightly:
cargo +nightly fuzz run jq_fuzz --features experimental_jq
```

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

- String bodies are `meter::Str`: `Val::utf8_str`/`byte_str` charge the
  length, clones share the charge, and the last one dropped releases it. The
  charge sits beside a plain `Bytes`, not inside it as the buffer owner, so a
  uniquely held string is appended to in place (`. + $x`, `join`). An owner
  wrapper made every append copy, and `join` over 300k strings went from
  0.5 s to over 30 s.
- In-place appending only survives if the accumulator is **uniquely held at
  the add**, which constrains how a folding filter may be written, not just
  how the meter is built. `Val::add` reuses the left operand's buffer through
  `BytesMut::from(Bytes)`, which copies unless that `Bytes` is unique, so a
  fold whose accumulator is also reachable from somewhere else in the same
  expression is quadratic. Measured in an unoptimized build, 100k -> 300k
  iterations (linear would be 3.0x):

  | filter body | 100k | 300k | ratio |
  | --- | --- | --- | --- |
  | `. + "x"` | 0.28 s | 0.83 s | 2.9x |
  | `. + ("," + "x")` | 0.48 s | 1.79 s | 3.7x |
  | `. + "," + "x"` | 0.91 s | 5.20 s | 5.7x |
  | `. + "x" + "y"` | 1.02 s | 7.49 s | 7.4x |

  Rule: the accumulator appears **once**, as the left operand of a **single**
  add, with the right operand fully parenthesized. `(. + a) + b` keeps `.`
  alive to evaluate `b`, so the inner add sees a shared buffer and copies it;
  `. + (a + b)` does not. Testing the accumulator in a condition
  (`if . == null then ... else . + $x end`) shares it the same way. This is
  why the prelude's `join` (`builtins/jq/compat.rs`) coerces the elements into
  an array first and then folds with `. + ($x + $s)`, instead of jq's own
  `(if . == null then "" else . + $x end) + ...` shape: that shape cost 68 s
  for 300k elements where the fold above costs 14 s and scales linearly.
- Array and object bodies are `Metered<Vec<Val>>` / `Metered<Map>`: charged
  on creation and clone, re-charged by `resync()` after every in-place
  mutation, released on drop.
- Growth operations check the meter *before* allocating and fail with the jq
  error `value size limit (N bytes) exceeded` (exit 5). The limit is
  `ExecutionLimits::max_live_intermediate_bytes` (default 32 MB).
- Ordinary inputs parse one top-level value on demand and drop its temporary
  tree after conversion. `input`/`inputs` and the outer loop share that cursor;
  an unread malformed tail is not parsed. `--slurp` retains converted values,
  but never a second collection of all parsed trees. Raw input lines are lazy
  too; `--seq` normalizes separators only for JSON input and preserves them
  in raw modes. Span scanning and line-number bookkeeping poll shared work/cancellation
  in bounded chunks, without per-value offset tables.
- `--stream` tokenizes the source directly into path events inside the metered
  evaluation. It never builds a top-level JSON tree or event list. Ancestor
  keys are shared `Val` strings; container cursors, path and event arrays are
  admitted before growth. Tokens retain normalization and depth validation;
  scanners and traversal check shared work/cancellation/deadlines even for
  `empty`. A container's scalar is emitted only after its separator validates,
  matching jq's partial output on syntax errors. `--slurp` runs its filter only
  after all input parses successfully, and retains events against the meter.

  VFS input sources remain buffered; incremental parsing does not imply
  streaming filesystem reads. UTF-8 files transfer their buffers into source
  text without a second byte/text cache. `just stress-files 64` measures these
  paths; allocation regressions also cover a large single JSON array, JSONL,
  and giant strings using explicitly raised execution/VFS/HTTP limits.
- The meter is sticky: once tripped, `try ... catch` cannot keep a loop going.
- Where there is no error channel (infallible `FromIterator`, `From<String>`)
  and for loops that never emit a value (`until(false; .)`), the run is
  aborted by `resume_unwind` with a `meter::Abort` payload (no panic hook
  runs) and the jq builtin catches it. Value operations `tick`; every 4096
  ticks the execution deadline is polled, so non-emitting loops end at the
  timeout (`jq: execution timed out`).
- Output conversion is capped by `max_stdout_bytes`, so a value built from
  shared parts (`reduce range(40) as $i (1; [., .])`) cannot expand when
  rendered. Runtime error messages render at most 64 KiB of a value. Short error previews (`jqmsg::dump_trunc`, object-key and cannot-index messages) render through a capped sink that aborts the traversal once the budget is spent, so even the intermediate preview never materializes a huge value's full dump (and never walks the rest of it just to discard the output).

Builds with `panic = "abort"` cannot unwind. There the fallible checks still
apply, over-limit infallible constructions are cut short with the meter
tripped, and a non-emitting loop is not interrupted.

### Structural depth (#3013)

Values have an independent ceiling of 100 nested containers (scalars have
depth zero). `metered` admits every new array/object, and index updates check
the incoming child's depth before attachment. Concatenation, slicing and
recursive object merge preserve the maximum depth of their admitted operands.
The separate `fromjson` reader checks depth **before descending**; checking
only completed containers would leave parsing itself able to overflow.
Exceeding the ceiling trips the run's sticky meter and reports
`jq: error: value nesting depth limit (100) exceeded`, exit 5.

`Metered` caches exact depth lazily. Mutable access invalidates the cache;
Rc copy-on-write protects ancestors sharing an old container. This avoids
walking every descendant at each wrapper, exponential visits to shared trees,
and rescanning a wide object on every scalar update. Removing a deep child
reduces the computed depth; a historical high-water mark would wrongly reject
later valid wrapping. Panic-abort fallbacks discard rejected attachments.

#### Why the earlier limits missed it

The value representation inherits recursive destruction from `Rc` and its
containers. A flat `reduce range(100000) as $i (1; [.]) | length` retains
only linear heap storage and evaluator state, emits one small scalar, then
recursively destroys the discarded accumulator. Input depth, output bytes,
live bytes and evaluator context counts therefore all miss the native-stack
growth. `catch_unwind` does not contain a stack overflow.

The memory-growth fix (#2454, commit `51f5d001`) explicitly retained this
known gap. The subsequent evaluator recursion fix (#2467, `a4fc5b33`) bounded
contexts, not value structure; this document's old gap description also failed
to reflect that distinction. The report demonstrates an unresolved containment
gap, rather than evidence of a new Python binding regression.

Regression coverage now separates input depth, evaluator recursion, generated
value depth and heap/output growth. `memory_growth_security_tests::jq` covers
exact boundaries, array/object updates, removal/COW cache invalidation, shared
trees, `fromjson`, sticky failure and session recovery. Python's
`test_jq_depth.py` runs the original 100,000-iteration case and related cleanup
paths in timed child processes. `jq_fuzz` generates structured reductions near
the boundary in addition to mutating filter text.
`jq_value_depth_tests::generated_depth_bounded` replays the full-scale
filters in the Rust suite inside a supervised child on a 2 MiB stack, so a
regression fails one test instead of aborting the integration binary.
`jq_value_depth_tests::container_mutation_sites_are_reviewed` inventories
every line under `builtins/jq` that creates an `Rc`/`Metered` body or reaches
into one mutably (`make_mut`, `get_mut`, `iter_mut`, ...). A vendor sync or
new native that adds such a site fails that test; admit depth at the site (or
show the operation preserves the maximum depth of admitted operands), then add
it to the inventory. `MAX_JQ_JSON_DEPTH <= MAX_VALUE_DEPTH` is a compile-time
assertion so accepted input always passes value admission. Treat unbounded stack growth
in any lifecycle phase (parse/build/traverse/format/compare/drop/unwind) as an
open containment defect, even when another resource dimension is bounded.

## jq compatibility patches

Besides the meter, a few `BASHKIT PATCH` hunks make values behave like jq
1.7 where jaq differs, so `catch` and `tostring` see jq's text too:

- `num.rs` `jq_format_f64`: floats print like jq (`1024`, `1e+17`, `1e-05`,
  NaN as `null`, infinities as the largest double).
- `jqmsg.rs` (bashkit-only): jq's wording for math and index errors
  (`string ("a") and number (1) cannot be added`, operands cut at 11 bytes).
- `mod.rs`: division and `%` by zero fail, `%` truncates operands, a
  fractional array index truncates, an object indexed by a non-string fails.
- `mod.rs` path updates: assigning through null creates the container
  (`null | .a.b = 1`, `.[2] = 1`, slices), stays null when the update yields
  nothing, an index past the end pads with null (capped like jq: `Array
  index too large`), a negative index before the start fails, and `.[]` on a
  scalar says `Cannot iterate over number (1)`.
- `mod.rs` `from_map`: object keys must be strings
  (`Cannot use null (null) as object key`).
- `defs.jq`: `nan`/`infinite` without dividing by zero (and the jaq-std
  filters built on them), `gamma` as log-gamma.

## Syncing upstream

Part of the [maintenance](../operations/maintenance.md) pass:

1. `scripts/sync-jaq-json.sh --check` reports whether a newer jaq-json is out.
2. `scripts/sync-jaq-json.sh <version>` adapts the vendored and the new
   release the same way and applies the upstream diff onto the vendored
   copy. `BASHKIT PATCH` hunks survive; conflicts are left as `.rej` files.
3. Re-check every `BASHKIT PATCH`: new growth paths (new natives, new
   container mutations) need a meter check or `resync()`. New containers must
   pass `metered`; mutations attaching arbitrary values must check child depth
   and invalidate cached depth. `container_mutation_sites_are_reviewed`
   flags every new site; update its inventory only after that review. Review recursive parsers before descent and
   cover cleanup during errors as well as successful output.
4. Match jaq-json's `Cargo.toml` dependency ranges in
   `crates/bashkit/Cargo.toml`, and keep `jaq-core`/`jaq-std` at the versions
   that release requires. One deliberate exception: `foldhash` is declared
   `0.2` while jaq-json 2.0.3 still asks for `^0.1`. `hashbrown` already pulls
   `foldhash` 0.2 in, so matching upstream would lock two copies of it into
   every build. The vendored code only uses `fast::RandomState` and
   `fast::FixedState`, which are unchanged across the bump, and `hash_nums`
   asserts hash *relations* rather than fixed values, so it does not care which
   is linked. Do not "restore" this to `^0.1` during a sync; drop the exception
   only once upstream itself moves to 0.2.
5. Run the jq tests (upstream ones included) and `memory_growth_security_tests`.

## See also

- [Threat Model](../security/threat-model.md), TM-DOS-110
- [Dependencies](../operations/dependencies.md)
- [Maintenance](../operations/maintenance.md)
- [Coreutils Argument Port](coreutils-args-port.md), the other vendoring precedent
