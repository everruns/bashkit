# Criterion jq LRU and Filter Compilation Benchmark

Compares the one-entry compiled-filter cache with a four-entry LRU and with
that LRU plus pre-parsed Bashkit compatibility definitions. Each script runs
through `Bash::exec` in a warmed process and asserts the exact output.
The interleaved case cycles through four distinct filters. The mixed case
alternates one hot filter with new filter source on every other call.

Run with `cargo bench --locked -p bashkit --features jq --bench jq`.

## Results

All three builds used Rust 1.100.0 with thin LTO and eight codegen units on
Linux x86-64. The benchmark process ran on the same pinned Xeon Platinum
8370C CPU with a one-second warmup, a three-second measurement target, and
30 Criterion samples per workload. Times are Criterion point estimates per
script. The baseline already reused the parsed jaq library definitions.

| Benchmark | One entry | Four-entry LRU | After | Change |
|---|---:|---:|---:|---:|
| `jq/single_lookup` | 0.879 ms | 0.797 ms | 0.618 ms | -29.7% |
| `jq/repeated_filter` | 2.710 ms | 2.613 ms | 2.318 ms | -14.5% |
| `jq/interleaved_filters` | 48.320 ms | 4.532 ms | 3.969 ms | -91.8% |
| `jq/hot_and_unique_filters` | 51.333 ms | 25.442 ms | 20.265 ms | -60.5% |
| `jq/distinct_filters` | 50.599 ms | 49.472 ms | 37.086 ms | -26.7% |
| `jq/aggregate_4096_rows` | 15.054 ms | 12.710 ms | 13.622 ms | -9.5% |

The first 4,096-row run showed a 7.2% slowdown against the LRU-only build.
Two isolated reruns with reversed ordering gave 13.99 to 13.77 ms and 13.07
to 12.55 ms. Each rerun used 40 Criterion samples per build. Both reruns
favored the pre-parsed build.

## Cache-miss profile

The distinct-filter workload collected 10,584 user-space CPU samples from
the LRU-only build. Named jaq compiler and compiled-graph code accounted for
32% of samples, and the lexer and parser accounted for 11%. Pre-parsing the
fixed compatibility definitions cut the distinct-filter benchmark by 25%.
The follow-up profile collected 4,601 samples from the combined build, with
1.2% in the lexer and parser and 40% in compiler and compiled-graph code.
Each miss still rebuilds jaq's compiled program graph.

## Inventory fan-out

The archived 100-warehouse script ran unchanged with the same in-process
fixture tools. Each comparison alternated the builds across three rounds,
with 100 timed samples per build per round. Every run returned zero with
empty stderr and matched the expected output.

The one-entry and four-entry builds had medians of 9.30 and 9.15 ms.
In a separate run the four-entry build took 9.09 ms and the combined
build took 8.78 ms. The script reuses one main filter in its loop, so
it sees more of the compile-path improvement than the LRU expansion.
