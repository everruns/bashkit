# Mixed-plan browser rebase validation

Rebased onto `d07ba00d`, including conditional-regex scratch reuse (#2984)
and compound-output memory metering (#2996). Built with Rust 1.100.0,
wasm-bindgen 0.2.128, Binaryen 125 and the unchanged `wasm-release` pipeline.
The compiler remains opt-in. [Build identity and measurements](wasm-mixed-plan-rebase-2026-11-24.json).

| Transfer format | Measured compiler bundle | Compiler cap | Default cap (unchanged) |
|---|---:|---:|---:|
| Raw | 7,936,464 | 8,000,000 | 8,000,000 |
| Gzip | 3,130,093 | 3,140,000 | 3,120,000 |
| Brotli | 2,221,467 | 2,230,000 | 2,220,000 |

The combined compiler and output-metering build exceeded the prior experimental
caps by 10,093 gzip bytes and 1,467 Brotli bytes. Only the compiler profile's
compressed allowances change; default-package caps and runtime limits remain
unchanged. No additional optimization pass or runtime feature removal was used.
This is a size/compatibility check, not a browser speedup measurement.

Validation: 86 Node tests, 24 GNU Bash comparisons and three budget tests pass.
The budget tests verify explicit profile selection, malformed metadata rejection,
every limit in both profiles, and CLI enforcement. Run with:

```sh
BASHKIT_WASM_FEATURES=terminal,bashkit/experimental_execution_plan \
  bash crates/bashkit-wasm/scripts/build.sh release
node --test 'crates/bashkit-wasm/__test__/*.test.mjs'
node crates/bashkit-wasm/bench/compat.mjs
node crates/bashkit-wasm/bench/artifact.mjs crates/bashkit-wasm/pkg --check
```
