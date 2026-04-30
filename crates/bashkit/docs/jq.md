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
| `--arg name value` | Bind `$name` to a string |
| `--argjson name json` | Bind `$name` to a parsed JSON value |
| `-V`, `--version` | Print the version |
| `-h`, `--help` | Print help |
| Combined flags like `-snr` | Treated as the union of the individual flags |

## Variables

| Variable | Behaviour |
|----------|-----------|
| `$ENV` | Bound to the shell environment as an object — same map as the `env` filter. (#1486) |
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
| `input_filename` | Stub returning `null` (#1486). Bashkit reads inputs as a single concatenated stream via shell redirection, so per-input filenames are not tracked. |
| `input_line_number` | Stub returning `0` (#1486). Per-line input tracking is not implemented. |
| `input` / `inputs` | Real jaq implementations — pull from the shared input iterator. |
| Most other 1.7/1.8 stdlib filters | Forwarded from `jaq-std` (`getpath`, `paths`, `to_entries`, `group_by`, `ltrimstr`/`rtrimstr`, `splits`, `test`, `now`, `debug`, `limit`, etc.). |

## Errors

Filter parse failures and runtime errors return exit code `3` and `5`
respectively, matching jq. Long error operands are summarised so failures
do not blow up an LLM context window — see
[#1485](https://github.com/everruns/bashkit/issues/1485).

## Known gaps

Bashkit's jq is intentionally minimal in places where the host model differs
from upstream jq:

- File inputs are concatenated into a single stream by the shell, so
  per-file metadata (`input_filename`, `input_line_number`) is stubbed.
- Exotic numeric formatting modes (`@base32`, `@base64d`, etc.) follow
  whatever `jaq-json` ships.

If you hit a missing builtin, please open an issue with the failing filter.

## See also

- [`compatibility_scorecard`](crate::compatibility_scorecard) — overall
  builtin coverage table.
- [`threat_model`](crate::threat_model) — security model for `jq` against
  malicious input.
