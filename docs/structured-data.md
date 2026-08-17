# Structured data

Bashkit ships structured-data builtins for the formats scripts hit most often.
`jq` and `yq` share one jq-compatible transformation engine; narrower helpers
cover common CSV, JSON, and TOML operations. All read files or stdin.

| Builtin | Format | Reach for it when |
|---------|--------|-------------------|
| [`jq`](../crates/bashkit/docs/jq.md) | JSON | You need real JSON transformation, filters, construction, reduction. |
| [`yq`](../crates/bashkit/docs/yq.md) | YAML / JSON | You want the same jq expressions over YAML, conversion, or safe in-place updates. |
| `json` | JSON | You want a quick `get` / `set` / `keys` / `length` without jq syntax. |
| `csv` | CSV | Selecting columns, filtering rows, counting, sorting tabular data. |
| `tomlq` | TOML | Reading a value out of `Cargo.toml`, `pyproject.toml`, etc. |

## csv

Subcommands: `select`, `count`, `headers`, `filter`, `sort`. Use `-d` for a
custom delimiter and `--no-header` for headerless data.

```bash
csv select name,age data.csv      # project columns
csv filter age = 30 data.csv      # rows where age == 30
csv sort name data.csv            # sort by column
csv count data.csv                # row count
csv headers data.csv              # list column names
echo "alice,30" | csv --no-header count
```

## json

A lighter alternative to `jq` for everyday access. Subcommands: `get`, `set`,
`keys`, `length`, `type`, `format`, `pretty`.

```bash
echo '{"a":1}'     | json get .a       # 1
echo '{"a":1}'     | json set .b 2      # {"a":1,"b":2}
echo '{"a":1,"b":2}' | json keys        # a, b
echo '[1,2,3]'     | json length        # 3
echo '{"a":1}'     | json format        # pretty-print
```

## yq

Use jq-style expressions over YAML or JSON. The `jq` Cargo feature enables it.
YAML mapping keys are emitted in deterministic sorted order; source order,
comments, styles, and anchors are not preserved. Custom tags, non-string keys,
and non-finite numbers fail closed at the JSON-value boundary.

```bash
yq '.server.port' config.yml
yq '.items[] | select(.enabled) | .name' config.yml
yq -o=json -I=0 config.yml
yq -i '.server.port = 8080' config.yml
```

## tomlq

Query TOML by dot-separated path. `-r` emits raw (unquoted) string values.

```bash
tomlq server.port config.toml
tomlq -r dependencies.serde.version Cargo.toml
cat config.toml | tomlq server.port
```

## Composing them

Because every builtin reads stdin, they pipe into each other and into the text
tools:

```bash
# CSV → JSON-ish summary
csv select name,price products.csv | csv sort price

# Pull a value out of config, then use it
port=$(yq '.server.port' config.yml)
echo "starting on $port"
```

## See also

- [jq builtin](../crates/bashkit/docs/jq.md), the full JSON query engine, with its own compatibility
  reference.
- [yq builtin](../crates/bashkit/docs/yq.md), YAML/JSON flags, in-place behavior, and explicit gaps.
- [Compatibility](../crates/bashkit/docs/compatibility.md), the complete builtin coverage matrix.
- [Browse all builtins](/builtins), every registered command.
