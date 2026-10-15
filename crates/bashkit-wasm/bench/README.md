# Browser package experiments

These experiments complement the full package integration suite. Run from the
repository root after `just build-wasm`. They require GNU Bash 5+ on `PATH` (on
macOS, `/bin/bash` 3.2 is unsuitable) and Node 22+.

- `node crates/bashkit-wasm/bench/compat.mjs [pkg ...]`: 24 deterministic,
  self-contained shell cases against a fresh GNU Bash oracle and fresh Bashkit
  instances. Exact stdout bytes and status; stderr is not scored. Worker lifetime
  is capped at 30 seconds; each oracle process at two seconds. One package is a
  pass/fail CI gate; multiple packages produce a comparison without failing for
  an old release's known gaps. This is a regression sample, not a population
  compatibility percentage. No corpus or package is fetched by the gate.
- `node crates/bashkit-wasm/bench/artifact.mjs [pkg] [--check]`: raw, gzip-9,
  Brotli-11 bytes for the actual JS + WASM load graph, SHA-256 identities, and
  WASM code/data/other sections. `--check` applies `size-budget.json`, with about
  4-5% headroom above the selected shipping profile. Review new measurements
  before deliberately raising a budget. No wall-clock threshold in CI.
- `node crates/bashkit-wasm/bench/runtime.mjs pkg`: arithmetic, substitution,
  jq, and text pipelines; one warmup and seven measured fresh-instance runs.
  Output is checked for status and consistency. Compare on the same host,
  without competing builds. Times from a busy development host are exploratory.
- `TRIALS=5 node crates/bashkit-wasm/bench/cold-start.mjs pkg-before pkg-after`:
  Chromium, Brotli-11 server, fresh HTTP cache per trial, rotated package order,
  loopback plus CDP Fast 4G (1,012,500 B/s, 165 ms) and Slow 4G (180,000 B/s,
  562.5 ms), page CPU throttled 4x. Requires an existing Playwright installation
  and Chrome; `PLAYWRIGHT_MODULE` can name its `index.mjs` and `CHROME_CHANNEL`
  selects a browser channel. Trials are limited to 1..20. Every visit must
  initialize the real package, execute `echo hi`, drive `Terminal` when present,
  and paint output over two animation frames. Each navigation/wait is capped at
  60 seconds; browser contexts and the loopback server close on completion/error.

The cold-start page intentionally has no framework, renderer, shared terminal
WASM, or worker dependency. It measures the package, not brush-web's complete
navigation-to-prompt path. All execution is on the page; CDP CPU throttling
therefore covers this experiment's interpreter (unlike a worker experiment).
Fresh contexts guarantee cold HTTP caches, but browser-process compiled-code
caches may be shared. Network measurements remain representative; do not infer
physical-phone compile time. The JSON records every resource and milestone,
Chromium/Node versions, bytes, hashes, and raw samples.

Redirect JSON output to retain a run. Historical investigation results are in
`results/`; these specialized package experiments are linked here rather than
mixed with the native bashkit-bench or Criterion result schemas.

See also: [package guide](../README.md),
[browser design](../../../knowledge/runtimes/browser-package.md).
