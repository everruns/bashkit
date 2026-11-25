# Mixed-plan browser build: retain execution speed

The compiler-enabled package uses the existing `wasm-opt -Oz` pipeline. Its
bounded gzip budget is 3,120,000 bytes; the ordinary package keeps 3,100,000.
Both retain the same 8,000,000 raw and 2,220,000 Brotli limits. The optional
profile is selected only by explicit compiler features in build metadata.
Package tests check both feature spellings, default/unknown/malformed identity,
all three caps and CLI rejection of an oversized valid WASM custom section.

| Compiler-enabled artifact | Raw bytes | gzip-9 bytes | Brotli-11 bytes |
|---|---:|---:|---:|
| Selected ordinary `-Oz` | 7,892,028 | 3,110,020 | 2,205,715 |
| Rejected extra pass | 7,836,917 | 3,099,884 | 2,199,388 |

Adding `--flatten --rereloop -Oz --converge` saved about 10 KB gzip and passed
functionality checks. It was rejected after execution probes: four rotated,
CPU-2-pinned warm jq comparisons measured **4.093 → 4.445 ms** (+8.6%).
Each process used 25 warmups and 50 measured fresh shells, checked exact output,
then freed results and instances. Early-call probes were noisier and showed a
larger jq penalty. Host variation prevents a precise regression estimate; this
small transfer saving does not justify risking execution throughput.

The selected package passes **86 Node tests**, **24 GNU Bash comparisons** and
its transfer caps. Rust sources are `9cab5a45`; later changes affect packaging
policy, tests and reports. No interpreter semantics, builtin inventory, panic
policy or runtime resource limit was changed for size.

[Raw samples, artifact hashes and toolchain identity](wasm-mixed-plan-2026-11-23.json).
These are exploratory runtime probes, not wall-clock CI thresholds or a native
benchmark/site snapshot. The normal package's transfer policy is unchanged.
