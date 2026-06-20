# Structured data

Beyond `jq`, Bashkit ships small builtins for the formats scripts hit most often:
CSV, JSON, YAML, and TOML. They cover the common "pull a field out, filter, count"
operations without reaching for a full query language, and they all read from a
file argument or from stdin so they pipe naturally.

| Builtin | Format | Reach for it when |
|---------|--------|-------------------|
| [`jq`](jq.md) | JSON | You need real JSON transformation — filters, construction, reduction. |
| `json` | JSON | You want a quick `get` / `set` / `keys` / `length` without jq syntax. |
| `csv` | CSV | Selecting columns, filtering rows, counting, sorting tabular data. |
| `yaml` | YAML | Reading a value out of a config file by dotted path. |
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

## yaml

Query YAML by dot-separated path. Subcommands: `get`, `keys`, `length`, `type`.

```bash
yaml get server.port config.yml
yaml keys config.yml
cat config.yml | yaml get server.port
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
port=$(yaml get server.port config.yml)
echo "starting on $port"
```

## See also

- [jq builtin](jq.md) — the full JSON query engine, with its own compatibility
  reference.
- [Compatibility](compatibility.md) — the complete builtin coverage matrix.
- [Browse all builtins](/builtins) — every registered command.
