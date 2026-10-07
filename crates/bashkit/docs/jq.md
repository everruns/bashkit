# jq builtin

Bashkit ships an embedded `jq` JSON processor backed by [jaq] with a thin
compatibility shim layered on top. This guide documents which jq features
are supported so callers (and LLM agents that generate jq filters against
bashkit) can avoid surprises.

[jaq]: https://github.com/01mf02/jaq

## Reported version

`jq --version` prints `jq-1.8`. Filters generated for stedfan/jq 1.7 and 1.8
are the intended target.

## Command-line flags

Implemented:

| Flag | Description |
|------|-------------|
| `-r`, `--raw-output` | Strings are written without quotes |
| `-R`, `--raw-input` | Each line of input becomes a JSON string |
| `-s`, `--slurp` | Read every input value into one array |
| `-n`, `--null-input` | Use `null` as the (single) input value |
| `-c`, `--compact-output` | One JSON value per line, no pretty-printing |
| `-S`, `--sort-keys` | Sort object keys recursively |
| `-e`, `--exit-status` | Set exit code based on the output |
| `-j` | Like `-r` but suppresses trailing newlines |
| `--tab` | Use tabs for indentation |
| `-a`, `--ascii-output` | Escape non-ASCII characters as `\uXXXX` (strings stay quoted even with `-r`) |
| `--raw-output0` | Like `-r` with a NUL after each output; a string containing NUL is an error |
| `--seq` | Write RS (0x1e) before each output; RS separates input values |
| `--stream` | Feed each input as path events (`[path, leaf]`, `[path]`), as `tostream` does |
| `--arg name value` | Bind `$name` to a string |
| `--argjson name json` | Bind `$name` to a parsed JSON value |
| `-f FILE`, `--from-file FILE` | Read the filter from FILE (VFS); all positionals become input files |
| `-V`, `--version` | Print the version |
| `-h`, `--help` | Print help |
| Combined flags like `-snr` | Treated as the union of the individual flags |

## Variables

| Variable | Behaviour |
|----------|-----------|
| `$ENV` | Bound to the shell environment as an object, same map as the `env` filter. (#1486) |
| `$name` | Variables defined with `--arg` / `--argjson` are passed through. |

## Notable filters

The full [jq stdlib] is mostly available via `jaq-std`. The compatibility
shim adds or overrides:

[jq stdlib]: https://jqlang.github.io/jq/manual/

| Filter | Notes |
|--------|-------|
| `env` | Reads from the shell env map (not the host process env), avoiding the unsafe `std::env::set_var` path. |
| `setpath(p; v)` | Bashkit ships a recursive definition because jaq's stdlib doesn't expose one. |
| `leaf_paths` | Defined as `paths(scalars)` since jaq's stdlib lacks it. |
| `match(re; flags)` / `match(re)` | Overridden to add `"name": null` to unnamed captures, matching jq output. |
| `scan(re; flags)` / `scan(re)` | Overridden so `scan` defaults to global ("g") matching, matching jq. |
| `input_filename` | Name of the file the current value came from (`null` for stdin). |
| `input_line_number` | Lines read so far, as jq counts them (jq reads a line at a time). |
| `input` / `inputs` | Pull from the same stream as the main loop; with `-n` the whole stream is theirs. `input` fails with `No more inputs` at the end. |
| Most other 1.7/1.8 stdlib filters | Forwarded from `jaq-std` (`getpath`, `paths`, `to_entries`, `group_by`, `ltrimstr`/`rtrimstr`, `splits`, `test`, `now`, `debug`, `limit`, etc.). |

## Numbers

Computed numbers print like jq: `pow(2;10)` is `1024`, `1e17*1` is `1e+17`,
`0.00001*1` is `1e-05`, NaN is `null` and infinities print as the largest
double. Input literals print as read until arithmetic changes them (`1.0`
stays `1.0`). Division by zero is an error, `%` truncates its operands to
integers, a fractional array index is truncated, and `gamma` is the log-gamma
function, all as in jq. Math and index errors use jq's wording, so
`try ("a"+1) catch .` gives `string ("a") and number (1) cannot be added`.

## Errors

Filter compile failures exit `3`. A runtime error is reported as
`jq: error (at FILE:LINE): ...` (`<stdin>`, or `<unknown>` under `-n`) and jq
moves on to the next input; the exit status follows the last input (`5` if
it failed). `error("msg")` prints `msg` unquoted, other values get jq's
`(not a string)` marker. Values before a JSON syntax error are processed,
then the error exits `5`. A file that cannot be opened prints
`Could not open file F: reason`, is skipped, and makes the exit status `2`.
With `-e` the last output decides: `1` for `null`/`false`, `4` when nothing
was output. Long error operands are summarised so failures
do not blow up an LLM context window, see
[#1485](https://github.com/everruns/bashkit/issues/1485).

## Resource limits

A filter cannot allocate without bound. Every live string, array and object
counts against `ExecutionLimits::max_live_intermediate_bytes` (32 MB by
default); growing past it fails before allocating with
`jq: error: value size limit (N bytes) exceeded` and exit 5, and `try` cannot
catch its way around it. Output is capped by `max_stdout_bytes`, and a filter
that loops without emitting (`until(false; .)`) stops at the execution timeout
with `jq: execution timed out`.

The limit counts real in-memory size, which is several times the JSON text:
a 5.6 MB array of 100,000 small objects fits the default, much larger inputs
need a larger `max_live_intermediate_bytes`.

## Location

`$__loc__` is `{"file":"<top-level>","line":N}`, N being the filter line it
appears on.

## Messages and halt

`stderr`, `debug` and `halt_error` write to the jq command's stderr.
`halt` and `halt_error` end the jq command (not the shell or host) with
their exit code, skipping any remaining input.

## Known gaps

Bashkit's jq is intentionally minimal in places where the host model differs
from upstream jq:

- `--seq` reads RS as whitespace; jq's rules for abandoned text between
  separators are not reproduced.
- Exotic numeric formatting modes (`@base32`, `@base64d`, etc.) follow
  whatever `jaq-json` ships.

If you hit a missing builtin, please open an issue with the failing filter.

## See also

- [`compatibility_scorecard`](crate::compatibility_scorecard), overall
  builtin coverage table.
- [`threat_model`](crate::threat_model), security model for `jq` against
  malicious input.
