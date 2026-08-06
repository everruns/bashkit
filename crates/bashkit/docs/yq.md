# yq builtin

Bashkit ships a `yq` structured-data processor for the command shape agents
commonly use with mikefarah/yq. It parses YAML or JSON, evaluates the existing
`jq`/jaq expression engine, then emits YAML or JSON. Bashkit deliberately does
not maintain a second YAML-specific query language.

Enable the Cargo `jq` feature to register both `jq` and `yq`.

## Examples

```bash
yq '.server.port' config.yml
yq '.items[] | select(.enabled) | .name' config.yml
yq '.values | map(. * 2)' config.yml
yq -o=json -I=0 '.' config.yml
yq -p=json -o=yaml '.' data.json
yq -i '.server.port = 8080' config.yml
```

With no expression, `.` is used. Input comes from the listed VFS files or
stdin. YAML streams containing `---` are processed one document at a time;
`-s` presents all input documents to the filter as one array. The optional
`e` / `eval` subcommand alias is accepted for common generated invocations.

## Flags

| Flag | Behaviour |
|------|-----------|
| `-p`, `--input-format` | `auto`, `yaml`, or `json` |
| `-o`, `--output-format` | `yaml` or `json` |
| `-r`, `--raw-output` | Unwrap string results |
| `-c`, `--compact-output` | Compact JSON output |
| `-e`, `--exit-status` | Nonzero for no output, `null`, or `false` |
| `-s`, `--slurp` | Read all documents into an array |
| `-n`, `--null-input` | Evaluate once with `null` input |
| `-i`, `--inplace` | Atomically replace exactly one input file |
| `-I`, `--indent` | Set JSON indentation; `0` is compact |
| `-N`, `--no-doc` | Omit separators between YAML results |
| `--expression` | Force an otherwise ambiguous argument to be the expression |

Short boolean flags combine (`-rce`, `-sn`). Attached value forms such as
`-o=json`, `-p=json`, and `-I=0` are accepted.

In-place evaluation and serialization finish before a sibling temporary file
is written and renamed over the source. A parse, filter, output-limit, write,
or rename failure leaves the source unchanged.

## Compatibility boundary

The expression language is jq, not mikefarah/yq's node/style language. Common
selection, iteration, `select`, `map`, construction, reduction, and assignment
filters work. mikefarah/yq-only operators for comments, styles, anchors, tags,
file metadata, and cross-file evaluation are not implemented.

YAML custom tags, non-string mapping keys, and non-finite numbers are rejected
rather than silently losing information at the JSON-value boundary. Mapping
keys are sorted deterministically. Comments, scalar style, and anchors are not
retained after conversion. The parser follows YAML 1.1. TOML, CSV, and XML
conversion are not part of this builtin; Bashkit's separate `tomlq` and `csv`
helpers remain available for their existing narrow command surfaces.

## See also

- [`jq_guide`](crate::jq_guide) — the shared expression engine and its jq compatibility notes.
- [`threat_model`](crate::threat_model) — structured-input resource and information-disclosure controls.
