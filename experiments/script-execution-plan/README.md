# Experimental script execution plans

The `experimental_execution_plan` Cargo feature is disabled by default.
It compiles supported scalar arithmetic, assignments, branches and loops to
execution-owned instructions, with normal interpreter fallback. Scripts keep
ordinary Bash syntax; see the [activation and limits](../../crates/bashkit/docs/compatibility.md#experimental-execution-plans).

The full benchmark harness, matched lockfile, raw samples, allocation profiles,
validation records and reproduction instructions are preserved in the
[experiment archive](https://github.com/everruns/bashkit/tree/41591e58/experiments/script-execution-plan).
Those are historical Linux measurements against baseline `782a4c55`, rather
than measurements of every later shipping revision.

| Historical workload | Time change |
|---|---:|
| Short scripts | Previous 8% penalty eliminated |
| Straight arithmetic | 12% less |
| Supported loops | 26–63% less |
| Pipelines | Approximately flat |
| Substitutions | 4% more |

The substitution regression remains a gate for enabling the feature by default.
Same-binary controls did not reproduce that penalty, so its build-dependent
cause remains unresolved. Full source, samples and methodology are linked above.

Continuous feature-on/off coverage, security and fuzzing commands are described
in the [testing strategy](../../knowledge/operations/testing.md#experimental-execution-plan-coverage).
