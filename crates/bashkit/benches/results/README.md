# Criterion bench results

Historical output from `cargo bench --bench <name>` against benches in
`crates/bashkit/benches/`.

## Naming

`criterion-<bench>-<moniker>-<timestamp>.md`

- `<bench>` — matches the bench name (`hotpath`, `file_ops`, `parallel`,
  `sqlite`, ...). May carry a label suffix when comparing variants
  (e.g. `hotpath-perf`, `hotpath-attrs+shopt`).
- `<moniker>` — `<hostname>-<os>-<arch>` or a shorter tag (`vm-linux-x86_64`).
- `<timestamp>` — Unix seconds, set by the writer (`date +%s`).

## Don't confuse with `crates/bashkit-bench/results/`

That sibling directory holds **`bashkit-bench`** harness output — the
runner that executes bashkit against real bash for parity/perf comparison.
Those files use the `bench-<runner>-<moniker>-<timestamp>.{json,md}`
pattern. Keep the two streams separated; do not save criterion `.md` files
there.
