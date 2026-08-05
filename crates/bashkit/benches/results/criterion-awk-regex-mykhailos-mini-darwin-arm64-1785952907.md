# Criterion AWK Runtime Regex Benchmark

## System Information

- **Moniker**: `Mykhailos-Mini.lan-darwin-arm64`
- **Hostname**: Mykhailos-Mini.lan
- **OS**: Darwin
- **Architecture**: arm64
- **CPUs**: 12
- **Timestamp**: 1785952907
- **Profile**: release (`opt-level=3`, fat LTO, one codegen unit)

## Workload

50,000 generated access-log lines. Field-expression cases scan six fields per
line (300,000 expression evaluations); the top-level pattern runs once per line.

| Benchmark | Time | 95% confidence interval | Throughput |
|-----------|-------:|------------------------:|-----------:|
| `expression_anchored` (`$i ~ /^bytes=/`) | 392.37 ms | 281.97–448.22 ms | 0.76 M eval/s |
| `expression_unanchored` (`$i ~ /bytes=1/`) | 153.91 ms | 145.66–473.49 ms | 1.95 M eval/s |
| `index_prefix` (`index($i,"bytes=")==1`) | 117.55 ms | 109.27–166.90 ms | 2.55 M eval/s |
| `top_level_pattern` (`/bytes=1/`) | 39.01 ms | 30.17–62.97 ms | 1.28 M lines/s |

## Notes

- `expression_anchored` is the regression workload: runtime compilation is
  cached, so its remaining cost is expression evaluation and regex matching.
- `top_level_pattern` remains the parser-compiled control path.
- Run with `cargo bench -p bashkit --bench awk_regex`.
