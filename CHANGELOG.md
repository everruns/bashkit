# Changelog

## [0.14.0] - 2026-07-17

### Highlights

- **Browser-native `@everruns/bashkit-web` package** — a slim,
  single-threaded WebAssembly build runs Bashkit directly in browsers without
  `SharedArrayBuffer` or COOP/COEP headers, with async custom builtins and live
  VFS access ([#2175](https://github.com/everruns/bashkit/pull/2175)).
- **Generic filesystem namespaces** — mount independent filesystems under
  virtual path prefixes, with longest-prefix routing, rebasing, and sandboxed
  namespace examples ([#2174](https://github.com/everruns/bashkit/pull/2174)).
- **VFS and glob hardening** — directory limits now apply to `add_dir`, and
  glob backtracking correctly restores bracket-match cache state
  ([#2168](https://github.com/everruns/bashkit/pull/2168),
  [#2167](https://github.com/everruns/bashkit/pull/2167)).

### What's Changed

* feat(wasm): slim single-threaded browser package (@everruns/bashkit-web) ([#2175](https://github.com/everruns/bashkit/pull/2175)) by @chaliy
* feat(fs): add generic filesystem namespaces ([#2174](https://github.com/everruns/bashkit/pull/2174)) by @chaliy
* feat(site): add Google Analytics tag ([#2173](https://github.com/everruns/bashkit/pull/2173)) by @chaliy
* fix(ci): install prebuilt cargo-audit to unblock Audit job ([#2171](https://github.com/everruns/bashkit/pull/2171)) by @chaliy
* test(fuzzer): avoid host env dependent assertion ([#2170](https://github.com/everruns/bashkit/pull/2170)) by @chaliy
* fix(ci): add cargo-tarpaulin tool input to coverage workflow ([#2169](https://github.com/everruns/bashkit/pull/2169)) by @chaliy
* fix(vfs): enforce add_dir directory limits ([#2168](https://github.com/everruns/bashkit/pull/2168)) by @chaliy
* fix(glob): restore bracket cache on '*' backtrack ([#2167](https://github.com/everruns/bashkit/pull/2167)) by @chaliy
* chore(deps): bump turso_core from 0.6.1 to 0.7.0 ([#2166](https://github.com/everruns/bashkit/pull/2166)) by @dependabot
* chore(ci): bump the github-actions group with 3 updates ([#2165](https://github.com/everruns/bashkit/pull/2165)) by @dependabot
* chore(deps): bump rustls from 0.23.41 to 0.23.42 ([#2164](https://github.com/everruns/bashkit/pull/2164)) by @dependabot
* ci(apidocs-drift): build index.cjs so typedoc can resolve wrapper imports ([#2163](https://github.com/everruns/bashkit/pull/2163)) by @chaliy
* chore(deps): bump the rust-dependencies group with 5 updates ([#2162](https://github.com/everruns/bashkit/pull/2162)) by @dependabot
* chore: sync uutils/coreutils argument surfaces and integrate new i18n-decimal gate ([#2161](https://github.com/everruns/bashkit/pull/2161)) by @chaliy
* chore(ci): bump the github-actions group with 3 updates ([#2160](https://github.com/everruns/bashkit/pull/2160)) by @dependabot
* ci: repair scheduled drift guards ([#2159](https://github.com/everruns/bashkit/pull/2159)) by @chaliy
* docs(threat-model): sync public doc with spec ledger and drift guard ([#2158](https://github.com/everruns/bashkit/pull/2158)) by @chaliy
* chore: pre-release maintenance pass ([#2156](https://github.com/everruns/bashkit/pull/2156)) by @chaliy
* chore(js): upgrade bashkit-js to TypeScript 7 ([#2154](https://github.com/everruns/bashkit/pull/2154)) by @chaliy
* chore: standardize PR descriptions on functional change and before/after proof ([#2153](https://github.com/everruns/bashkit/pull/2153)) by @chaliy
* fix(site): lengthen crawler meta descriptions ([#2152](https://github.com/everruns/bashkit/pull/2152)) by @chaliy

**Full Changelog**: https://github.com/everruns/bashkit/compare/v0.13.0...v0.14.0

## [0.13.0] - 2026-07-07

### Highlights

- **Pluggable `HttpTransport` for curl/wget** — embedding hosts can direct all
  sandbox HTTP through their own boundary (egress gateway, proxy, audit layer)
  by injecting a transport via `BashBuilder::http_transport`. bashkit keeps
  enforcing policy (allowlist, SSRF precheck, hooks, credential injection,
  bot-auth signing, response caps) before every dispatch; the transport
  receives the merged signed headers, timeouts, the SSRF precheck's pinned
  addresses, and the response size cap, and returns typed errors
  (`Denied`/`Timeout`/`TooLarge`/`Transport`) that map onto curl exit codes
  (7/28/63/1). Mirrors fetchkit's transport injection so one host egress
  implementation can back both libraries. See `specs/http-transport.md`
  ([#2147](https://github.com/everruns/bashkit/pull/2147)).
- **`xargs -P` parallel execution** — `-P/--max-procs` runs command batches
  concurrently, with `--process-slot-var` exposing the slot index to each child
  ([#2126](https://github.com/everruns/bashkit/pull/2126)).
- **`ls -d/--directory`** — list directories themselves instead of their
  contents ([#2134](https://github.com/everruns/bashkit/pull/2134)).
- **`bashkit-eval` reimplemented on the mira framework** — the LLM eval harness
  now runs on mira ([#2129](https://github.com/everruns/bashkit/pull/2129)).
- **Linear-time glob `*` matching (TM-DOS-031)** — replaces the pathological
  backtracking that allowed exponential blowup on adversarial patterns
  ([#2142](https://github.com/everruns/bashkit/pull/2142)).

### Breaking Changes

- **`HttpHandler` removed.** `BashBuilder::http_handler` and
  `HttpClient::set_handler` are replaced by `BashBuilder::http_transport` /
  `HttpClient::set_transport` (no compatibility shim, per repo policy).
  Migration: implement `HttpTransport` — wrap the old
  `(method, url, body, headers)` logic in `execute(HttpTransportRequest)`
  and return `HttpTransportError` variants instead of `String`.
  - Before: `.http_handler(Box::new(MyHandler))`
  - After: `.http_transport(Arc::new(MyTransport))`

### What's Changed

* fix(test): exclude host-dependent vars from differential fuzzer ([#2150](https://github.com/everruns/bashkit/pull/2150)) by @chaliy
* fix(eval): bound transcript VFS snapshots ([#2149](https://github.com/everruns/bashkit/pull/2149)) by @chaliy
* feat(network): pluggable HttpTransport for host-routed HTTP egress ([#2147](https://github.com/everruns/bashkit/pull/2147)) by @chaliy
* chore(ci): bump the github-actions group with 3 updates ([#2146](https://github.com/everruns/bashkit/pull/2146)) by @dependabot
* chore(deps): bump the rust-dependencies group with 5 updates ([#2145](https://github.com/everruns/bashkit/pull/2145)) by @dependabot
* fix(ls): render actual UTC calendar dates in long format ([#2144](https://github.com/everruns/bashkit/pull/2144)) by @a0preetham
* fix(security): update fuzz cmov lockfile ([#2143](https://github.com/everruns/bashkit/pull/2143)) by @chaliy
* fix(glob): linear-time * matching to stop exponential blowup (TM-DOS-031) ([#2142](https://github.com/everruns/bashkit/pull/2142)) by @chaliy
* fix(wasm): route clock reads through web-time so bashkit runs on wasm32-unknown-unknown ([#2140](https://github.com/everruns/bashkit/pull/2140)) by @a0preetham
* chore(deps): bump the rust-dependencies group with 6 updates ([#2138](https://github.com/everruns/bashkit/pull/2138)) by @dependabot
* chore(ci): bump the github-actions group with 4 updates ([#2137](https://github.com/everruns/bashkit/pull/2137)) by @dependabot
* fix(deps): bump langgraph-checkpoint to 4.1.1 (GHSA-fjqc-hq36-qh5p) ([#2136](https://github.com/everruns/bashkit/pull/2136)) by @chaliy
* fix(vfs): provision user home directory so $HOME is writable ([#2135](https://github.com/everruns/bashkit/pull/2135)) by @chaliy
* feat(ls): support -d/--directory to list directories themselves ([#2134](https://github.com/everruns/bashkit/pull/2134)) by @chaliy
* chore(security): drop stale pyo3 ignore from audit.toml + bump site ws/yaml ([#2131](https://github.com/everruns/bashkit/pull/2131)) by @chaliy
* fix(deps): remove stale pyo3 advisory ignores ([#2130](https://github.com/everruns/bashkit/pull/2130)) by @chaliy
* feat(eval): reimplement bashkit-eval on the mira framework ([#2129](https://github.com/everruns/bashkit/pull/2129)) by @chaliy
* feat(xargs): support -P/--max-procs and --process-slot-var ([#2126](https://github.com/everruns/bashkit/pull/2126)) by @chaliy
* fix(js): bump js-yaml to >=4.2.0 (GHSA-h67p-54hq-rp68) ([#2124](https://github.com/everruns/bashkit/pull/2124)) by @chaliy
* fix(parser): treat do/done as words inside for/select in-list ([#2123](https://github.com/everruns/bashkit/pull/2123)) by @chaliy
* fix(deps): bump pyo3 to 0.29 to resolve two security advisories ([#2122](https://github.com/everruns/bashkit/pull/2122)) by @chaliy

**Full Changelog**: https://github.com/everruns/bashkit/compare/v0.12.0...v0.13.0

## [0.12.0] - 2026-06-23

### Highlights

- **`ExecOptions` request struct + `exec_with_options` entry point** — a single,
  extensible request object for configuring a run, replacing ad-hoc parameter
  threading at the library boundary
  ([#2093](https://github.com/everruns/bashkit/pull/2093)).
- **`BashToolBuilder::configure` for full `BashBuilder` access** — LLM-tool
  callers can now reach the complete builder surface when wiring up a tool
  ([#2104](https://github.com/everruns/bashkit/pull/2104)).
- **Self-hosted Python + TypeScript API references at `/api`** — the binding API
  docs are now built and hosted from the repo, with new public guides and a
  reorganized docs navigation
  ([#2109](https://github.com/everruns/bashkit/pull/2109),
  [#2111](https://github.com/everruns/bashkit/pull/2111)).
- **`agents`-friendly site surfaces** — `llms.txt` agent entry points, linked
  skill sources, and cross-linked Markdown surfaces make the site easier for
  agents to consume ([#2099](https://github.com/everruns/bashkit/pull/2099),
  [#2106](https://github.com/everruns/bashkit/pull/2106)).
- **Interpreter correctness & security sweep** — typed-state refactors of the
  directory stack, `$!`, and `getopts` cursors, plus a batch of parser/expansion
  fixes and several dependency security-advisory patches.

### Breaking Changes

- **`PythonLimits` / `TypeScriptLimits` fields moved under a shared `common: RuntimeLimits`.**
  The two embedded-VM limit types now share a `RuntimeLimits` core (duration,
  memory, allocations, call depth)
  ([#2095](https://github.com/everruns/bashkit/pull/2095)). The fluent builder
  API is unchanged, but code that read the `pub max_*` fields directly must now
  go through `.common`. SQLite is unaffected.
  - Before: `let mem = limits.max_memory;`
  - After: `let mem = limits.common.max_memory;`

### What's Changed

* test(parallel): verify 1000-session fan-out and extend scaling bench ([#2120](https://github.com/everruns/bashkit/pull/2120)) by @chaliy
* chore(site): upgrade to Astro 7 ([#2119](https://github.com/everruns/bashkit/pull/2119)) by @chaliy
* chore(ci): bump the github-actions group with 5 updates ([#2118](https://github.com/everruns/bashkit/pull/2118)) by @dependabot
* chore(deps): bump the rust-dependencies group with 4 updates ([#2117](https://github.com/everruns/bashkit/pull/2117)) by @dependabot
* fix(read): preserve implicit REPLY whitespace ([#2116](https://github.com/everruns/bashkit/pull/2116)) by @chaliy
* fix(interpreter): reset getopts cursor across shell boundaries ([#2115](https://github.com/everruns/bashkit/pull/2115)) by @chaliy
* fix(snapshot): validate restored last background pid ([#2114](https://github.com/everruns/bashkit/pull/2114)) by @chaliy
* fix(snapshot): validate restored directory stack ([#2113](https://github.com/everruns/bashkit/pull/2113)) by @chaliy
* fix(deps): bump langsmith 0.8.5 -> 0.8.18 (GHSA-f4xh-w4cj-qxq8) ([#2112](https://github.com/everruns/bashkit/pull/2112)) by @chaliy
* docs(site): add six public guides and reorganize docs nav ([#2111](https://github.com/everruns/bashkit/pull/2111)) by @chaliy
* fix(site): expand too-short meta descriptions on docs and builtins pages ([#2110](https://github.com/everruns/bashkit/pull/2110)) by @chaliy
* feat(site): self-host Python + TypeScript API references at /api ([#2109](https://github.com/everruns/bashkit/pull/2109)) by @chaliy
* fix(builtins): include special builtins in inventory ([#2108](https://github.com/everruns/bashkit/pull/2108)) by @chaliy
* fix(expansion): fail closed on quote marker collision ([#2107](https://github.com/everruns/bashkit/pull/2107)) by @chaliy
* feat(site): link skill source and enrich llms.txt for agents ([#2106](https://github.com/everruns/bashkit/pull/2106)) by @chaliy
* docs(site): cross-link Markdown surfaces to llms.txt + document contract ([#2105](https://github.com/everruns/bashkit/pull/2105)) by @chaliy
* feat(tool): add BashToolBuilder::configure for full BashBuilder access ([#2104](https://github.com/everruns/bashkit/pull/2104)) by @chaliy
* refactor(dirstack): move directory stack to typed interpreter state ([#2103](https://github.com/everruns/bashkit/pull/2103)) by @chaliy
* docs(skills): list http_client, ssh, jq, bot-auth in rust install features ([#2102](https://github.com/everruns/bashkit/pull/2102)) by @chaliy
* docs(site): add LLM tools guide ([#2101](https://github.com/everruns/bashkit/pull/2101)) by @chaliy
* docs(site): add Embedding getting-started guide ([#2100](https://github.com/everruns/bashkit/pull/2100)) by @chaliy
* feat(site): add llms.txt agent entry points ([#2099](https://github.com/everruns/bashkit/pull/2099)) by @chaliy
* docs(python): fix BashKit → Bashkit in FileSystem docstring ([#2098](https://github.com/everruns/bashkit/pull/2098)) by @chaliy
* refactor(interpreter): move $! to typed state, drop dead _BG_EXIT_CODE ([#2097](https://github.com/everruns/bashkit/pull/2097)) by @chaliy
* refactor(interpreter): move getopts cluster cursor to typed state ([#2096](https://github.com/everruns/bashkit/pull/2096)) by @chaliy
* refactor(builtins): share RuntimeLimits core across Python/TypeScript VMs ([#2095](https://github.com/everruns/bashkit/pull/2095)) by @chaliy
* refactor(interpreter): decompose monolith + group scoped shell state ([#2094](https://github.com/everruns/bashkit/pull/2094)) by @chaliy
* feat(lib): add ExecOptions request struct + exec_with_options entry point ([#2093](https://github.com/everruns/bashkit/pull/2093)) by @chaliy
* refactor(interpreter): remove _EVAL_CMD magic-variable channel ([#2092](https://github.com/everruns/bashkit/pull/2092)) by @chaliy
* feat(tool): gate BashTool wrapper behind default `bash_tool` feature ([#2091](https://github.com/everruns/bashkit/pull/2091)) by @chaliy
* fix(deps): patch newly-disclosed dependency security advisories ([#2090](https://github.com/everruns/bashkit/pull/2090)) by @chaliy
* fix(eval): require balanced CSV quote matches ([#2089](https://github.com/everruns/bashkit/pull/2089)) by @chaliy
* fix(rg): skip option values in delimiter scan ([#2088](https://github.com/everruns/bashkit/pull/2088)) by @chaliy
* fix(read): trim trailing IFS whitespace when assigning final variable ([#2087](https://github.com/everruns/bashkit/pull/2087)) by @chaliy
* fix(parser): keep literal case patterns unexpanded ([#2086](https://github.com/everruns/bashkit/pull/2086)) by @chaliy
* fix(interpreter): suppress ERR trap in conditions ([#2085](https://github.com/everruns/bashkit/pull/2085)) by @chaliy
* fix(strings): preserve double-dash delimiter ([#2084](https://github.com/everruns/bashkit/pull/2084)) by @chaliy
* fix(interpreter): preserve mixed word IFS boundaries ([#2083](https://github.com/everruns/bashkit/pull/2083)) by @chaliy
* fix(deps): patch npm security advisories in lockfiles ([#2082](https://github.com/everruns/bashkit/pull/2082)) by @chaliy
* fix(parser): preserve expanded backslashes in glob dirs ([#2081](https://github.com/everruns/bashkit/pull/2081)) by @chaliy
* fix(expansion): preserve quoted operands when markers collide ([#2076](https://github.com/everruns/bashkit/pull/2076)) by @chaliy
* fix(fs): avoid duplicate lower hide accounting ([#2074](https://github.com/everruns/bashkit/pull/2074)) by @chaliy

**Full Changelog**: https://github.com/everruns/bashkit/compare/v0.11.0...v0.12.0

## [0.11.0] - 2026-06-16

### Highlights

- **`cwd` and `env` construction options for the Node and Python bindings** —
  callers can now set the starting working directory and initial environment
  directly at `Bash` construction time, instead of paying for a leading
  `cd`/`export` prelude on every run
  ([#2072](https://github.com/everruns/bashkit/pull/2072)).

### What's Changed

* feat(site): span runtime surface across full hero width ([#2079](https://github.com/everruns/bashkit/pull/2079)) by @chaliy
* Add GitHub sponsor username ([#2078](https://github.com/everruns/bashkit/pull/2078)) by @chaliy
* chore: pre-release maintenance — deps update, vet refresh, threat-model & changelog sync ([#2077](https://github.com/everruns/bashkit/pull/2077)) by @chaliy
* feat(site): simplify hero runtime snippet card ([#2075](https://github.com/everruns/bashkit/pull/2075)) by @chaliy
* chore: set package author metadata across PyPI, npm, and crates.io ([#2073](https://github.com/everruns/bashkit/pull/2073)) by @chaliy
* feat(bindings): expose cwd and env options to Node and Python ([#2072](https://github.com/everruns/bashkit/pull/2072)) by @chaliy
* chore(deps): bump zeroize to 1.9.0 and napi to 3.9.2 ([#2071](https://github.com/everruns/bashkit/pull/2071)) by @chaliy
* chore(ci): bump github-actions group (setup-uv, codecov, taiki-e) ([#2070](https://github.com/everruns/bashkit/pull/2070)) by @chaliy
* fix(deps): patch esbuild and PyO3 security advisories ([#2069](https://github.com/everruns/bashkit/pull/2069)) by @chaliy
* fix(paste): handle trailing -d flag ([#2057](https://github.com/everruns/bashkit/pull/2057)) by @chaliy
* fix(fs): preserve recursive delete child whiteouts ([#2056](https://github.com/everruns/bashkit/pull/2056)) by @chaliy
* fix(awk): cap multi-subscript arrays ([#2055](https://github.com/everruns/bashkit/pull/2055)) by @chaliy
* fix(parser): escape ANSI-C NUL sentinel collisions ([#2053](https://github.com/everruns/bashkit/pull/2053)) by @chaliy
* test(specs): back limitations.md stance rows with evidence tests ([#2051](https://github.com/everruns/bashkit/pull/2051)) by @chaliy
* fix(compgen): list builtins from the live registry, not hardcoded copies ([#2040](https://github.com/everruns/bashkit/pull/2040)) by @chaliy
* chore(specs): threat-model ledger — backfill 12 code-cited IDs, add drift lint, compress 21 KB ([#2039](https://github.com/everruns/bashkit/pull/2039)) by @chaliy
* feat: generated builtin inventory, limitations negative spec, spec/agent-config compression ([#2038](https://github.com/everruns/bashkit/pull/2038)) by @chaliy

**Full Changelog**: https://github.com/everruns/bashkit/compare/v0.10.0...v0.11.0

## [0.10.0] - 2026-06-11

### Highlights

- **Python custom builtins can now read and write the VFS** via a new `ctx.fs` handle on `BuiltinContext` — a Python `custom_builtins` callback gets a live, sandbox-respecting view of the interpreter's filesystem, just like the embedded `python3` builtin ([#2010](https://github.com/everruns/bashkit/pull/2010)). Huge thanks to first-time external contributor **[@dedeswim](https://github.com/dedeswim)** (Edoardo Debenedetti) for designing, testing, and landing this. 🎉
- **JS ↔ Python binding parity** — the JS bindings close the remaining gaps with the Python API (`ctx.fs`, network access, `shellState`) so custom builtins behave consistently across hosts ([#2036](https://github.com/everruns/bashkit/pull/2036)).
- **Real PCRE for `grep -P`** via `fancy-regex`, plus GNU long-option aliases ([#1846](https://github.com/everruns/bashkit/pull/1846)).
- **Broad security & resource-safety hardening sweep** — a deep DoS/panic audit ([#2006](https://github.com/everruns/bashkit/pull/2006)) plus dozens of targeted caps and budget-enforcement fixes across the interpreter, parser, and builtins (`rg`, `grep`, `awk`, `curl`, `find`, `tar`, `sqlite`, `bc`, `tr`, `iconv`), the VFS (CSPRNG random devices, lazy-materialization limits, FIFO file-count caps), streaming callbacks, and snapshot/restore.

### Contributors

Welcome and thank you to our first external contributor this cycle, **[@dedeswim](https://github.com/dedeswim)**, whose [#2010](https://github.com/everruns/bashkit/pull/2010) brings VFS access to Python custom builtins.

### What's Changed

* feat(js): close Python-binding parity gaps (ctx.fs, network, shellState) ([#2036](https://github.com/everruns/bashkit/pull/2036)) by @chaliy
* chore: maintenance pass — cargo update, vet refresh, doc/spec sync ([#2035](https://github.com/everruns/bashkit/pull/2035)) by @chaliy
* fix(history): bound persistent command history ([#2024](https://github.com/everruns/bashkit/pull/2024)) by @chaliy
* fix(awk): stream redirected output through vfs to enforce quotas ([#2023](https://github.com/everruns/bashkit/pull/2023)) by @chaliy
* fix(interpreter): preserve array budget for local shadows ([#2018](https://github.com/everruns/bashkit/pull/2018)) by @chaliy
* fix(readlink): cap symlink canonicalization paths ([#2015](https://github.com/everruns/bashkit/pull/2015)) by @chaliy
* fix(find): enforce 1 MiB output cap for -printf and default output ([#2034](https://github.com/everruns/bashkit/pull/2034)) by @chaliy
* fix(git): contain inspection pathspecs and refs ([#2032](https://github.com/everruns/bashkit/pull/2032)) by @chaliy
* fix(js): prevent ScriptedTool.executeSync deadlock on registered-tool invocation ([#2033](https://github.com/everruns/bashkit/pull/2033)) by @chaliy
* fix(expand): cap output bytes to prevent unbounded allocation by @chaliy
* fix(vfs): enforce file-count limit on FIFO creation by @chaliy
* fix(curl): cap multipart body assembly at request body limit by @chaliy
* fix(grep): enforce max-count limit to prevent unbounded output by @chaliy
* fix(rg): cap passthrough output at 10 MB per invocation by @chaliy
* feat(python): expose VFS to custom builtins via BuiltinContext.fs ([#2010](https://github.com/everruns/bashkit/pull/2010)) by @dedeswim
* fix(tree): bound traversal resources by @chaliy
* fix(vfs): use CSPRNG for random devices by @chaliy
* fix(awk): reject oversized single output writes by @chaliy
* fix(python): avoid async callback GIL deadlock via unbounded work channel by @chaliy
* fix(builtins): reject zero join fields ([#2027](https://github.com/everruns/bashkit/pull/2027)) by @chaliy
* fix(parser): avoid brace range budget overflow ([#2020](https://github.com/everruns/bashkit/pull/2020)) by @chaliy
* fix(parser): avoid quadratic quote marker insertion ([#2013](https://github.com/everruns/bashkit/pull/2013)) by @chaliy
* fix(interpreter): shadow arrays for bare local declarations ([#2011](https://github.com/everruns/bashkit/pull/2011)) by @chaliy
* fix(python): restore deterministic teardown for async-callback machinery ([#2009](https://github.com/everruns/bashkit/pull/2009)) by @chaliy
* fix(python): keep private-loop worker off Python during interpreter exit ([#2008](https://github.com/everruns/bashkit/pull/2008)) by @chaliy
* fix: harden DoS and panic surfaces found in deep security audit ([#2006](https://github.com/everruns/bashkit/pull/2006)) by @chaliy
* fix(ci): repair drift-workflow YAML and fix GIL deadlocks hanging Coverage ([#2007](https://github.com/everruns/bashkit/pull/2007)) by @chaliy
* fix(ci): suppress phantom failure for coreutils-args-drift on push events by @chaliy
* fix(interpreter): restore scoped local arrays ([#1936](https://github.com/everruns/bashkit/pull/1936)) by @chaliy
* fix(interpreter): clear BASH_SOURCE after cancelled exec ([#1931](https://github.com/everruns/bashkit/pull/1931)) by @chaliy
* fix(rg): cap colorized output growth by @chaliy
* fix(python): yield private async callbacks for timeouts ([#1918](https://github.com/everruns/bashkit/pull/1918)) by @chaliy
* fix(interpreter): resolve array default subscripts consistently ([#1965](https://github.com/everruns/bashkit/pull/1965)) by @chaliy
* fix(rg): emit all passthru only matches ([#1957](https://github.com/everruns/bashkit/pull/1957)) by @chaliy
* fix(ci): pin release action SHAs and scope permissions to job level ([#2001](https://github.com/everruns/bashkit/pull/2001)) by @chaliy
* fix(limits): count exec calls before parsing ([#2000](https://github.com/everruns/bashkit/pull/2000)) by @chaliy
* fix(parser): split unquoted mixed-quote suffix expansions ([#1969](https://github.com/everruns/bashkit/pull/1969)) by @chaliy
* fix(redirect): scope fd3 pending buffers ([#1923](https://github.com/everruns/bashkit/pull/1923)) by @chaliy
* fix(rg): merge context windows before expansion to prevent CPU DoS ([#1905](https://github.com/everruns/bashkit/pull/1905)) by @chaliy
* fix(expansion): bound operand quote marker search ([#1999](https://github.com/everruns/bashkit/pull/1999)) by @chaliy
* fix(grep): preserve recursive indexed search semantics ([#1987](https://github.com/everruns/bashkit/pull/1987)) by @chaliy
* fix(rg): correct quiet files-without-match status ([#1956](https://github.com/everruns/bashkit/pull/1956)) by @chaliy
* fix(builtins): reject checksum options ([#1945](https://github.com/everruns/bashkit/pull/1945)) by @chaliy
* fix(interpreter): bound explicit subshell nesting ([#1941](https://github.com/everruns/bashkit/pull/1941)) by @chaliy
* fix(interpreter): clear errexit_suppressed at subshell/function boundaries ([#1986](https://github.com/everruns/bashkit/pull/1986)) by @chaliy
* fix(interpreter): escape quoted expansions adjacent to unquoted globs ([#1972](https://github.com/everruns/bashkit/pull/1972)) by @chaliy
* fix(examples): avoid provider symlink clobber ([#1970](https://github.com/everruns/bashkit/pull/1970)) by @chaliy
* fix(read): treat adjacent mixed IFS delimiters as one sequence ([#1964](https://github.com/everruns/bashkit/pull/1964)) by @chaliy
* fix(test): isolate real bash spec comparisons ([#1995](https://github.com/everruns/bashkit/pull/1995)) by @chaliy
* fix(jq): bind setpath arguments before recursion ([#1993](https://github.com/everruns/bashkit/pull/1993)) by @chaliy
* fix(builtins): resolve readlink canonical symlinks ([#1992](https://github.com/everruns/bashkit/pull/1992)) by @chaliy
* fix(bench): isolate I/O benchmark file writes ([#1991](https://github.com/everruns/bashkit/pull/1991)) by @chaliy
* fix(bench): secure parallel benchmark cache ([#1990](https://github.com/everruns/bashkit/pull/1990)) by @chaliy
* fix(pi): isolate bashkit state per agent start ([#1989](https://github.com/everruns/bashkit/pull/1989)) by @chaliy
* fix(security): remove repo-controlled Claude startup hook ([#1988](https://github.com/everruns/bashkit/pull/1988)) by @chaliy
* fix(iconv): reject unsupported target suffixes ([#1974](https://github.com/everruns/bashkit/pull/1974)) by @chaliy
* fix(fuzz): exercise template renderer in template_fuzz ([#1973](https://github.com/everruns/bashkit/pull/1973)) by @chaliy
* fix(builtins): gate jq command metadata ([#1971](https://github.com/everruns/bashkit/pull/1971)) by @chaliy
* fix(alias): preserve mixed quoted glob reparse ([#1968](https://github.com/everruns/bashkit/pull/1968)) by @chaliy
* fix(parser): ignore subscript equals in array appends ([#1967](https://github.com/everruns/bashkit/pull/1967)) by @chaliy
* fix(strings): preserve dash-prefixed filenames ([#1966](https://github.com/everruns/bashkit/pull/1966)) by @chaliy
* fix(fs): enforce POSIX mount path prefixes ([#1963](https://github.com/everruns/bashkit/pull/1963)) by @chaliy
* fix(bench): avoid predictable sqlite temp file ([#1962](https://github.com/everruns/bashkit/pull/1962)) by @chaliy
* fix(rg): honor rgignore precedence over gitignore ([#1961](https://github.com/everruns/bashkit/pull/1961)) by @chaliy
* fix(interpreter): clear BASH_SOURCE transient state ([#1951](https://github.com/everruns/bashkit/pull/1951)) by @chaliy
* fix(curl): validate multipart URLs before upload reads ([#1943](https://github.com/everruns/bashkit/pull/1943)) by @chaliy
* fix(awk): bound getline file cache ([#1932](https://github.com/everruns/bashkit/pull/1932)) by @chaliy
* fix(vfs): preserve UTF-8 file decoding ([#1985](https://github.com/everruns/bashkit/pull/1985)) by @chaliy
* fix(find): consume negated type predicate ([#1978](https://github.com/everruns/bashkit/pull/1978)) by @chaliy
* fix(api): clear tty state when disabled ([#1984](https://github.com/everruns/bashkit/pull/1984)) by @chaliy
* fix(sort): preserve stable equal-key order ([#1983](https://github.com/everruns/bashkit/pull/1983)) by @chaliy
* fix(interpreter): honor errexit for final and-or failures ([#1982](https://github.com/everruns/bashkit/pull/1982)) by @chaliy
* fix(builtins): enforce head byte limit for utf8 stdin ([#1981](https://github.com/everruns/bashkit/pull/1981)) by @chaliy
* fix(interpreter): scope errexit suppression ([#1980](https://github.com/everruns/bashkit/pull/1980)) by @chaliy
* fix(parser): decode escaped-dollar sentinel in literal continuations ([#1979](https://github.com/everruns/bashkit/pull/1979)) by @chaliy
* fix(js): restore BashTool VFS compatibility APIs ([#1976](https://github.com/everruns/bashkit/pull/1976)) by @chaliy
* fix(builtins): preserve read tail delimiters ([#1977](https://github.com/everruns/bashkit/pull/1977)) by @chaliy
* fix(interpreter): reset all set short option state ([#1975](https://github.com/everruns/bashkit/pull/1975)) by @chaliy
* fix(interpreter): use CallFrame::new in test to avoid field drift by @chaliy
* fix(rg): honor `--` delimiter when checking help/version flags ([#1960](https://github.com/everruns/bashkit/pull/1960)) by @chaliy
* fix(rg): escape glob class set operators ([#1959](https://github.com/everruns/bashkit/pull/1959)) by @chaliy
* fix(rg): sort metadata across explicit paths ([#1958](https://github.com/everruns/bashkit/pull/1958)) by @chaliy
* fix(rg): preserve indexed explicit binary inputs ([#1955](https://github.com/everruns/bashkit/pull/1955)) by @chaliy
* fix(tests): target consolidated integration harness ([#1954](https://github.com/everruns/bashkit/pull/1954)) by @chaliy
* fix(find): honor -print0, support negated -type, and fail on dangling -not ([#1953](https://github.com/everruns/bashkit/pull/1953)) by @chaliy
* fix(interpreter): respect parentheses in conditional precedence ([#1952](https://github.com/everruns/bashkit/pull/1952)) by @chaliy
* fix(curl): escape multipart backslashes ([#1950](https://github.com/everruns/bashkit/pull/1950)) by @chaliy
* fix(interpreter): isolate RANDOM state in child contexts ([#1949](https://github.com/everruns/bashkit/pull/1949)) by @chaliy
* fix(expansion): keep operand quote state out of variable data ([#1948](https://github.com/everruns/bashkit/pull/1948)) by @chaliy
* fix(cli): reserve removed mcp command ([#1947](https://github.com/everruns/bashkit/pull/1947)) by @chaliy
* fix(tar): enforce limits for stdout extraction ([#1946](https://github.com/everruns/bashkit/pull/1946)) by @chaliy
* fix(base64): preserve binary data ([#1996](https://github.com/everruns/bashkit/pull/1996)) by @chaliy
* test(redirects): verify combined file redirection ([#1994](https://github.com/everruns/bashkit/pull/1994)) by @chaliy
* fix(eval): enforce CSV row expectations ([#1997](https://github.com/everruns/bashkit/pull/1997)) by @chaliy
* fix(interpreter): propagate shell opts through subshells by @chaliy
* fix(interpreter): reset fd redirect state after subshell exec failure by @chaliy
* fix(python): expose keyed snapshot restore APIs by @chaliy
* fix(tool): honor timeouts in ScriptedTool ([#1944](https://github.com/everruns/bashkit/pull/1944)) by @chaliy
* fix(bc): cap scale precision ([#1942](https://github.com/everruns/bashkit/pull/1942)) by @chaliy
* fix(interpreter): guard malformed nameref array targets ([#1940](https://github.com/everruns/bashkit/pull/1940)) by @chaliy
* fix(fs): allow snapshot restores at file count limit ([#1939](https://github.com/everruns/bashkit/pull/1939)) by @chaliy
* fix(builtins): bound tr unicode range expansion ([#1938](https://github.com/everruns/bashkit/pull/1938)) by @chaliy
* fix(trace): clear events after failed exec ([#1937](https://github.com/everruns/bashkit/pull/1937)) by @chaliy
* fix(grep): validate indexed recursive search paths ([#1934](https://github.com/everruns/bashkit/pull/1934)) by @chaliy
* fix(history): persist history clear immediately ([#1935](https://github.com/everruns/bashkit/pull/1935)) by @chaliy
* fix(fs): reject live self mounts ([#1933](https://github.com/everruns/bashkit/pull/1933)) by @chaliy
* fix(interpreter): avoid quadratic invalid bracket glob scans ([#1930](https://github.com/everruns/bashkit/pull/1930)) by @chaliy
* fix(interpreter): suppress streaming for captured EXIT traps ([#1929](https://github.com/everruns/bashkit/pull/1929)) by @chaliy
* fix(interpreter): keep local arrays scoped to functions ([#1928](https://github.com/everruns/bashkit/pull/1928)) by @chaliy
* fix(vfs): enforce lazy file materialization limits ([#1927](https://github.com/everruns/bashkit/pull/1927)) by @chaliy
* fix(curl): defer data-file reads until URL/network validation and cap request body ([#1926](https://github.com/everruns/bashkit/pull/1926)) by @chaliy
* fix(typescript): cap VM timeout by Bash deadline ([#1925](https://github.com/everruns/bashkit/pull/1925)) by @chaliy
* fix(awk): constant-time output accounting + redirect-target cap ([#1924](https://github.com/everruns/bashkit/pull/1924)) by @chaliy
* fix(limits): preserve strict zero budgets ([#1922](https://github.com/everruns/bashkit/pull/1922)) by @chaliy
* fix(realfs): reject movable symlink parent targets ([#1920](https://github.com/everruns/bashkit/pull/1920)) by @chaliy
* fix(ci): verify pinned ripgrep archive digest ([#1904](https://github.com/everruns/bashkit/pull/1904)) by @chaliy
* fix(rg): contain followed symlink targets ([#1903](https://github.com/everruns/bashkit/pull/1903)) by @chaliy
* fix(rg): restore multiline match early exit ([#1901](https://github.com/everruns/bashkit/pull/1901)) by @chaliy
* fix(python): use VFS append API ([#1900](https://github.com/everruns/bashkit/pull/1900)) by @chaliy
* fix(grep): stream only-matching ranges ([#1899](https://github.com/everruns/bashkit/pull/1899)) by @chaliy
* fix(tool): sanitize ToolImpl callback errors ([#1917](https://github.com/everruns/bashkit/pull/1917)) by @chaliy
* fix(glob): keep quoted expansion metachars literal ([#1916](https://github.com/everruns/bashkit/pull/1916)) by @chaliy
* fix(fs): check overlay mtime limits before lower reads ([#1915](https://github.com/everruns/bashkit/pull/1915)) by @chaliy
* fix(python): bound lazy file provider materialization ([#1914](https://github.com/everruns/bashkit/pull/1914)) by @chaliy
* fix(streaming): enforce stdout/stderr caps for live callbacks ([#1912](https://github.com/everruns/bashkit/pull/1912)) by @chaliy
* fix(parser): bound process substitution body parsing ([#1911](https://github.com/everruns/bashkit/pull/1911)) by @chaliy
* fix(interpreter): restore timeout function depth baseline ([#1910](https://github.com/everruns/bashkit/pull/1910)) by @chaliy
* fix(python): bound direct glob traversal ([#1909](https://github.com/everruns/bashkit/pull/1909)) by @chaliy
* fix(interpreter): reset fd3 capture state across execs ([#1908](https://github.com/everruns/bashkit/pull/1908)) by @chaliy
* fix(sqlite): strip BOM before policy parsing ([#1907](https://github.com/everruns/bashkit/pull/1907)) by @chaliy
* fix(coreutils-port): reject shadowable `value_parser!` macros ([#1906](https://github.com/everruns/bashkit/pull/1906)) by @chaliy
* fix(ci): verify release tag integrity before creating GitHub release ([#1897](https://github.com/everruns/bashkit/pull/1897)) by @chaliy
* fix(ci): pin publish.yml action refs to immutable commit SHAs by @chaliy
* fix(ci): pin publish-python.yml action refs to immutable commit SHAs ([#1895](https://github.com/everruns/bashkit/pull/1895)) by @chaliy
* fix(ci): add --ignore-scripts to pnpm add in publish-js.yml ([#1894](https://github.com/everruns/bashkit/pull/1894)) by @chaliy
* fix(ci): pin publish-js.yml action refs to immutable commit SHAs ([#1893](https://github.com/everruns/bashkit/pull/1893)) by @chaliy
* fix(ci): verify publish source is on main before running code with secrets in publish-js.yml ([#1892](https://github.com/everruns/bashkit/pull/1892)) by @chaliy
* fix(ci): pin js.yml action refs to immutable commit SHAs ([#1891](https://github.com/everruns/bashkit/pull/1891)) by @chaliy
* fix(ci): restrict secret-bearing steps to push-to-main only in js.yml ([#1890](https://github.com/everruns/bashkit/pull/1890)) by @chaliy
* fix(ci): restrict secret-bearing steps to push events only in ci.yml ([#1889](https://github.com/everruns/bashkit/pull/1889)) by @chaliy
* fix(ci): prevent shell injection via workflow_dispatch duration input in fuzz.yml ([#1887](https://github.com/everruns/bashkit/pull/1887)) by @chaliy
* fix(ci): pin nightly.yml actions to immutable commit SHAs ([#1888](https://github.com/everruns/bashkit/pull/1888)) by @chaliy
* fix(ci): pin fuzz.yml actions to immutable commit SHAs ([#1886](https://github.com/everruns/bashkit/pull/1886)) by @chaliy
* fix(ci): pin coverage.yml actions to immutable commit SHAs ([#1885](https://github.com/everruns/bashkit/pull/1885)) by @chaliy
* fix(ci): pin coreutils-args-drift.yml actions to immutable commit SHAs ([#1884](https://github.com/everruns/bashkit/pull/1884)) by @chaliy
* fix(ci): pin cli-binaries.yml actions to immutable commit SHAs ([#1883](https://github.com/everruns/bashkit/pull/1883)) by @chaliy
* fix(ci): pin ci.yml actions to immutable commit SHAs ([#1882](https://github.com/everruns/bashkit/pull/1882)) by @chaliy
* fix(parser): cap nested parameter expansion lexing ([#1881](https://github.com/everruns/bashkit/pull/1881)) by @chaliy
* fix(ci): harden release and CLI tag validation ([#1879](https://github.com/everruns/bashkit/pull/1879)) by @chaliy
* fix(sqlite): bound .dump output cumulatively across all tables ([#1880](https://github.com/everruns/bashkit/pull/1880)) by @chaliy
* fix(limits): prevent exec counter overflow via saturating arithmetic ([#1878](https://github.com/everruns/bashkit/pull/1878)) by @chaliy
* fix(js): escape tool output XML delimiters in openai wrapper ([#1877](https://github.com/everruns/bashkit/pull/1877)) by @chaliy
* chore(ci): pin site workflow actions to commit SHAs ([#1875](https://github.com/everruns/bashkit/pull/1875)) by @chaliy
* fix(js): suppress stack traces from onOutput callback errors ([#1873](https://github.com/everruns/bashkit/pull/1873)) by @chaliy
* fix(js): escape tool output XML delimiters in anthropic wrapper ([#1876](https://github.com/everruns/bashkit/pull/1876)) by @chaliy
* chore(ci): pin python workflow actions to commit SHAs ([#1874](https://github.com/everruns/bashkit/pull/1874)) by @chaliy
* fix(examples): enforce timeout parameter in bashkit-pi bash tool ([#1872](https://github.com/everruns/bashkit/pull/1872)) by @chaliy
* chore(deepsec): upgrade scanner ([#1871](https://github.com/everruns/bashkit/pull/1871)) by @chaliy
* feat(grep): real PCRE -P via fancy-regex and GNU long-option aliases ([#1846](https://github.com/everruns/bashkit/pull/1846)) by @chaliy
* fix(ci): drop redundant version stanza from Homebrew formula ([#1845](https://github.com/everruns/bashkit/pull/1845)) by @chaliy


**Full Changelog**: https://github.com/everruns/bashkit/compare/v0.9.0...v0.10.0

## [0.9.0] - 2026-06-02

### Highlights

- **Pyodide/Emscripten (wasm32) Python wheel** — a reduced-feature `wasm32-unknown-emscripten` Python wheel now ships, enabling the embedded Python builtin in browser/wasm hosts ([#1811](https://github.com/everruns/bashkit/pull/1811)).
- **Broad resource-safety hardening sweep** — fuel/budget enforcement and memory caps across the interpreter, parser, `rg`, `sqlite`, snapshot, and expansion paths (arithmetic expansion bounds, coproc/process-substitution budgets, heredoc reinjection fuel, brace-step overflow, compound-array limits, replacement-growth caps, sqlite result memory cap).
- **JS BashTool snapshot authentication** plus keyed snapshot APIs and snapshot counter/byte-accounting fixes for correct resume.
- Benches snapshot site page and CI hardening for the release/publish workflows.

### What's Changed

* chore(ci): pin Pyodide wheel toolchain and document browser verification ([#1843](https://github.com/everruns/bashkit/pull/1843)) by @chaliy
* fix(js): authenticate BashTool snapshots ([#1838](https://github.com/everruns/bashkit/pull/1838)) by @chaliy
* fix(interpreter): bound arithmetic variable expansion ([#1828](https://github.com/everruns/bashkit/pull/1828)) by @chaliy
* fix(streaming): clear output callback on cancellation by @chaliy
* fix(snapshot): account function source bytes by @chaliy
* fix(awk): reject chained range patterns by @chaliy
* fix(snapshot): preserve counters on resume by @chaliy
* fix(sqlite): cap result memory growth by @chaliy
* fix(sqlite): invalidate engine cache on snapshot restore by @chaliy
* fix(scripted-tool): isolate extension invocation traces by @chaliy
* fix(tool_def): bound parsed array flag inputs by @chaliy
* fix(rg): cap replacement expansion by @chaliy
* fix(rg): stream summary scans to avoid eager line allocation by @chaliy
* fix(parser): charge heredoc reinjection to parser fuel by @chaliy
* fix(ci): scope Doppler token to secret fetch steps by @chaliy
* fix(interpreter): prevent brace step overflow by @chaliy
* fix(parser): enforce coproc parser limits by @chaliy
* fix(limits): enforce budgets for local compound arrays by @chaliy
* fix(interpreter): scope deferred process substitutions by @chaliy
* fix(interpreter): gate allexport env updates by @chaliy
* fix(interpreter): cap word-split array assignments by @chaliy
* fix(ci): validate CLI release tag input by @chaliy
* fix(interpreter): avoid IFS nameref recursion by @chaliy
* fix(js): expose keyed snapshot APIs by @chaliy
* fix(interpreter): clear transient stdin after timeouts by @chaliy
* fix(ci): verify release publish refs by @chaliy
* fix(parser): debit nested process substitution budgets by @chaliy
* fix(expansion): cap per-element replacement growth by @chaliy
* fix(rg): bound ignore rule resource use by @chaliy
* fix(ci): use integration test binary for cat/tac spec tests in drift workflow by @chaliy
* feat(python): add Pyodide/Emscripten (wasm32) wheel ([#1811](https://github.com/everruns/bashkit/pull/1811)) by @chaliy
* chore(deps): bump the rust-dependencies group with 3 updates by @dependabot
* fix(interpreter): preserve legacy nameref targets ([#1810](https://github.com/everruns/bashkit/pull/1810)) by @chaliy
* feat(site): add benches snapshot page ([#1789](https://github.com/everruns/bashkit/pull/1789)) by @chaliy
* chore(python): bump monty to v0.0.18 ([#1809](https://github.com/everruns/bashkit/pull/1809)) by @chaliy

**Full Changelog**: https://github.com/everruns/bashkit/compare/v0.8.0...v0.9.0

## [0.8.0] - 2026-05-28

### Highlights

- **Python `open()` support** — VFS-backed `open()` / `Path.open()` read, write, and append now work in the embedded Python builtin, so LLM-generated `with open("/tmp/...")` scripts run instead of failing. Host filesystem and network stay unavailable to Python ([#1800](https://github.com/everruns/bashkit/pull/1800)).
- Further `rg` parity and hardening fixes (default type globs, JSON context fanout cap, root-arg allocation) plus interpreter fixes for variable attribute/nameref persistence and persistent file descriptor validation.

### What's Changed

* ci: reclaim runner disk before disk-hungry scheduled jobs ([#1807](https://github.com/everruns/bashkit/pull/1807)) by @chaliy
* fix(rg): align r and tf default type globs with ripgrep ([#1805](https://github.com/everruns/bashkit/pull/1805)) by @chaliy
* fix(rg): cap JSON context event fanout ([#1804](https://github.com/everruns/bashkit/pull/1804)) by @chaliy
* fix(interpreter): persist var attrs and namerefs across shell state restore ([#1803](https://github.com/everruns/bashkit/pull/1803)) by @chaliy
* fix(interpreter): reject negative persistent file descriptors ([#1802](https://github.com/everruns/bashkit/pull/1802)) by @chaliy
* fix(rg): avoid root arg string cloning across candidates ([#1801](https://github.com/everruns/bashkit/pull/1801)) by @chaliy
* feat(python): support vfs-backed open ([#1800](https://github.com/everruns/bashkit/pull/1800)) by @chaliy
* feat(site): add bashkit logo assets ([#1799](https://github.com/everruns/bashkit/pull/1799)) by @chaliy
* fix(ci): bypass pnpm `--` separator that breaks napi build flag forwarding ([#1798](https://github.com/everruns/bashkit/pull/1798)) by @chaliy
* fix(site): add homepage canonical link header ([#1797](https://github.com/everruns/bashkit/pull/1797)) by @chaliy

**Full Changelog**: https://github.com/everruns/bashkit/compare/v0.7.2...v0.8.0

## [0.7.2] - 2026-05-27

### Highlights

- **Maintenance release** — rolls up ~25 `rg` hardening fixes, a CoW subshell-snapshot perf landmark ([#1767](https://github.com/everruns/bashkit/pull/1767)), security tightening (FD cap [#1780](https://github.com/everruns/bashkit/pull/1780), tool-hook enforcement for `command` [#1781](https://github.com/everruns/bashkit/pull/1781)), a `BuiltinHelper` refactor ([#1788](https://github.com/everruns/bashkit/pull/1788)), and test/build hygiene work.

### What's Changed

* chore(tests): move OverlayFs path-validation tests inline ([#1795](https://github.com/everruns/bashkit/pull/1795)) by @chaliy
* chore(tests): consolidate integration test binaries into one ([#1794](https://github.com/everruns/bashkit/pull/1794)) by @chaliy
* chore(build): slim test binaries, document cargo test --all-features hazard ([#1793](https://github.com/everruns/bashkit/pull/1793)) by @chaliy
* feat(site): add homepage markdown negotiation by @chaliy
* fix(interpreter): isolate bash -c / sh -c from parent shell state ([#1791](https://github.com/everruns/bashkit/pull/1791)) by @chaliy
* fix(interpreter): resolve array elements in arithmetic param expansion ([#1790](https://github.com/everruns/bashkit/pull/1790)) by @chaliy
* refactor(builtins): BuiltinHelper trait, centralized limits, split ls/awk ([#1788](https://github.com/everruns/bashkit/pull/1788)) by @chaliy
* fix(rg): resolve type filters after type db mutations ([#1787](https://github.com/everruns/bashkit/pull/1787)) by @chaliy
* chore: normalize Bashkit capitalization ([#1786](https://github.com/everruns/bashkit/pull/1786)) by @chaliy
* fix(rg): apply global sort for explicit path sets ([#1785](https://github.com/everruns/bashkit/pull/1785)) by @chaliy
* fix(rg): harden hyperlink URL interpolation ([#1784](https://github.com/everruns/bashkit/pull/1784)) by @chaliy
* fix(rg): bound color match amplification for dense patterns ([#1783](https://github.com/everruns/bashkit/pull/1783)) by @chaliy
* fix(rg): bound --colors parsing and sanitize invalid spec echo ([#1782](https://github.com/everruns/bashkit/pull/1782)) by @chaliy
* fix(interpreter): enforce tool hooks for command host-builtins ([#1781](https://github.com/everruns/bashkit/pull/1781)) by @chaliy
* fix(security): cap persistent file descriptors ([#1780](https://github.com/everruns/bashkit/pull/1780)) by @chaliy
* chore(bench): refresh runtime comparison + add in-proc reminder ([#1778](https://github.com/everruns/bashkit/pull/1778)) by @chaliy
* chore(bench): move criterion results to crates/bashkit/benches/results ([#1774](https://github.com/everruns/bashkit/pull/1774)) by @chaliy
* chore(eval): refresh model lineup with Opus 4.7 and GPT-5.5 ([#1773](https://github.com/everruns/bashkit/pull/1773)) by @chaliy
* chore(skills): hide private workflow skills ([#1772](https://github.com/everruns/bashkit/pull/1772)) by @chaliy
* test(bench): cover VarAttrs/BashFlags + add VFS/rg/glob benches ([#1770](https://github.com/everruns/bashkit/pull/1770)) by @chaliy
* chore(deps): bump serde_json to 1.0.150 and reqwest to 0.13.4 ([#1769](https://github.com/everruns/bashkit/pull/1769)) by @dependabot
* chore(ci): bump pnpm/action-setup from 4 to 6 ([#1768](https://github.com/everruns/bashkit/pull/1768)) by @dependabot
* perf(interpreter): CoW subshell snapshots, attribute bitset, flag cache ([#1767](https://github.com/everruns/bashkit/pull/1767)) by @chaliy
* chore(js): migrate packages to pnpm ([#1766](https://github.com/everruns/bashkit/pull/1766)) by @chaliy
* fix(rg): honor explicit file path parity ([#1765](https://github.com/everruns/bashkit/pull/1765)) by @chaliy
* chore(deps): bump turso_core from 0.6.0 to 0.6.1 ([#1764](https://github.com/everruns/bashkit/pull/1764)) by @dependabot
* ci(nightly): raise ASAN timeout to 90 minutes ([#1761](https://github.com/everruns/bashkit/pull/1761)) by @chaliy
* fix(tac): preserve unterminated last line on reversal ([#1760](https://github.com/everruns/bashkit/pull/1760)) by @chaliy
* fix(rg): match output mode precedence ([#1759](https://github.com/everruns/bashkit/pull/1759)) by @chaliy
* chore(site): allow AI content signals ([#1758](https://github.com/everruns/bashkit/pull/1758)) by @chaliy
* fix(docs-grep-agent): stop mounting lockfile-bearing example dirs ([#1757](https://github.com/everruns/bashkit/pull/1757)) by @chaliy
* test(rg): cover binary default reporting for explicit inputs and stdin ([#1756](https://github.com/everruns/bashkit/pull/1756)) by @chaliy
* fix(rg): preserve collected diagnostics in quiet match paths ([#1755](https://github.com/everruns/bashkit/pull/1755)) by @chaliy
* fix(rg): count unrestricted flags independently ([#1754](https://github.com/everruns/bashkit/pull/1754)) by @chaliy
* fix(rg): skip indexed prefilter for --crlf searches ([#1753](https://github.com/everruns/bashkit/pull/1753)) by @chaliy
* fix(rg): apply max-count after multiline invert ([#1752](https://github.com/everruns/bashkit/pull/1752)) by @chaliy
* fix(rg): align -u ignore classes with --no-ignore ([#1751](https://github.com/everruns/bashkit/pull/1751)) by @chaliy
* fix(rg): clear explicit line-number state on negation ([#1750](https://github.com/everruns/bashkit/pull/1750)) by @chaliy
* fix(rg): honor --no-context-separator between files ([#1749](https://github.com/everruns/bashkit/pull/1749)) by @chaliy
* fix(rg): avoid false --generate detection in value arguments ([#1748](https://github.com/everruns/bashkit/pull/1748)) by @chaliy
* fix(rg): skip indexed prefilter for --no-unicode non-literal queries ([#1747](https://github.com/everruns/bashkit/pull/1747)) by @chaliy
* fix(rg): keep parent dot-ignore active for --no-ignore-vcs ([#1746](https://github.com/everruns/bashkit/pull/1746)) by @chaliy
* fix(rg): bound ignore rule parsing and traversal memory ([#1745](https://github.com/everruns/bashkit/pull/1745)) by @chaliy
* fix(rg): avoid quadratic glob toggle recompilation ([#1744](https://github.com/everruns/bashkit/pull/1744)) by @chaliy
* fix(rg): cap brace alternation recursion depth ([#1743](https://github.com/everruns/bashkit/pull/1743)) by @chaliy
* fix(rg): avoid eager match vector allocation ([#1742](https://github.com/everruns/bashkit/pull/1742)) by @chaliy
* fix(ci): close DOPPLER_AVAILABLE bypass in examples workflow ([#1741](https://github.com/everruns/bashkit/pull/1741)) by @chaliy
* feat(site): negotiate markdown docs by @chaliy
* docs(site): add agent development quickstart by @chaliy
* docs(readme): remove stale install version pins by @chaliy

**Full Changelog**: https://github.com/everruns/bashkit/compare/v0.7.1...v0.7.2

## [0.7.1] - 2026-05-25

### Fixed

* fix(ci): skip publish-js AI examples step on Windows + Node 24. The libuv assertion `!(handle->flags & UV_HANDLE_CLOSING)` fires during process shutdown of the example scripts on that combination only, blocking the npm publish in v0.7.0. The same scripts pass on every other platform/Node combination, so the step is gated off for `runner.os == 'Windows' && matrix.node == '24'`.

**Full Changelog**: https://github.com/everruns/bashkit/compare/v0.7.0...v0.7.1

## [0.7.0] - 2026-05-25

### Highlights

- **Ripgrep (`rg`) parity push** — ~80 PRs landed expanding the `rg` builtin to near-feature-parity with upstream ripgrep: pcre2, multiline, encoding, glob brace/character-class/globstar, comprehensive default file types, ignore-file precedence (parent + global git ignore), preprocessor controls, hyperlink prefixes, ansi/hex colour styles, sort-by-modified, stats, max-filesize, mmap/engine flags, gzip search, follow-symlinks, and many bug fixes for output-mode precedence and binary-search behaviour.
- **Host-owned `BuiltinRegistry` API** — embedders (JS, Python, Rust) can now register and remove builtins at any point in the interpreter's lifetime via `Bash::builder().builtin_registry(...)` and `addBuiltin` / `removeBuiltin` on `Bash` / `BashTool`. Replaces the rebuild-on-register approach (which silently wiped the in-memory VFS). Brought to Python parity via `add_builtin` / `remove_builtin` ([#1721](https://github.com/everruns/bashkit/pull/1721), [#1732](https://github.com/everruns/bashkit/pull/1732), [#1733](https://github.com/everruns/bashkit/pull/1733)).
- **VFS read-only enforcement for live mounts** — `readonly_filesystem` now reliably blocks runtime mount writes ([#1691](https://github.com/everruns/bashkit/pull/1691)).
- **Maintenance pass** — turso 0.5.3→0.6.0, langchain examples to v1.x, fuzz leak-guard hardening, dependency-edge cleanup ([#1632](https://github.com/everruns/bashkit/pull/1632), [#1635](https://github.com/everruns/bashkit/pull/1635), [#1636](https://github.com/everruns/bashkit/pull/1636), [#1639](https://github.com/everruns/bashkit/pull/1639)).

### What's Changed

* fix(rg): avoid eager rg allocations in low-output/search-json paths ([#1690](https://github.com/everruns/bashkit/pull/1690)) by @chaliy
* fix(rg): cap replacement expansion output ([#1650](https://github.com/everruns/bashkit/pull/1650)) by @chaliy
* feat(python): wire custom builtins through BuiltinRegistry ([#1733](https://github.com/everruns/bashkit/pull/1733)) by @chaliy
* chore(ci): scope homebrew-tap push to dedicated PAT ([#1735](https://github.com/everruns/bashkit/pull/1735)) by @chaliy
* fix(rg): match explicit binary search ([#1736](https://github.com/everruns/bashkit/pull/1736)) by @chaliy
* test(js): benchmark customBuiltins callback overhead from bash ([#1734](https://github.com/everruns/bashkit/pull/1734)) by @chaliy
* fix(ci): close fork-PR secret exfiltration in examples job (TM-INF-026) ([#1728](https://github.com/everruns/bashkit/pull/1728)) by @chaliy
* docs(js): add customBuiltins example, README sections, and public guide ([#1726](https://github.com/everruns/bashkit/pull/1726)) by @chaliy
* feat(js): guardrail against executeSync + custom builtin deadlock ([#1732](https://github.com/everruns/bashkit/pull/1732)) by @chaliy
* fix(rg): avoid quadratic multiline matching hot path ([#1689](https://github.com/everruns/bashkit/pull/1689)) by @chaliy
* fix(rg): bound gzip decompression in search-zip mode ([#1687](https://github.com/everruns/bashkit/pull/1687)) by @chaliy
* fix(fs): enforce readonly_filesystem for runtime live mounts ([#1691](https://github.com/everruns/bashkit/pull/1691)) by @chaliy
* fix(rg): gate explicit symlink dereference behind --follow ([#1688](https://github.com/everruns/bashkit/pull/1688)) by @chaliy
* fix(rg): reject empty pattern with --only-matching ([#1649](https://github.com/everruns/bashkit/pull/1649)) by @chaliy
* fix(rg): match quiet output precedence by @chaliy
* fix(rg): match json event modes ([#1730](https://github.com/everruns/bashkit/pull/1730)) by @chaliy
* fix(rg): match only-output modes ([#1729](https://github.com/everruns/bashkit/pull/1729)) by @chaliy
* fix(rg): match zip extension behavior ([#1727](https://github.com/everruns/bashkit/pull/1727)) by @chaliy
* feat(core,js): host-owned mutable BuiltinRegistry ([#1721](https://github.com/everruns/bashkit/pull/1721)) by @chaliy
* fix(rg): match pre and zip precedence ([#1723](https://github.com/everruns/bashkit/pull/1723)) by @chaliy
* feat(rg): sort by modified time ([#1720](https://github.com/everruns/bashkit/pull/1720)) by @chaliy
* fix(ci): harden python wheel builds ([#1722](https://github.com/everruns/bashkit/pull/1722)) by @chaliy
* feat(rg): support pcre2 patterns ([#1719](https://github.com/everruns/bashkit/pull/1719)) by @chaliy
* feat(rg): complete real file types ([#1718](https://github.com/everruns/bashkit/pull/1718)) by @chaliy
* feat(rg): add document file types ([#1717](https://github.com/everruns/bashkit/pull/1717)) by @chaliy
* feat(rg): add project file types ([#1716](https://github.com/everruns/bashkit/pull/1716)) by @chaliy
* feat(rg): add metadata file types ([#1715](https://github.com/everruns/bashkit/pull/1715)) by @chaliy
* feat(rg): add more real file types ([#1714](https://github.com/everruns/bashkit/pull/1714)) by @chaliy
* feat(rg): add format file types ([#1713](https://github.com/everruns/bashkit/pull/1713)) by @chaliy
* feat(rg): add common real rg file types ([#1712](https://github.com/everruns/bashkit/pull/1712)) by @chaliy
* feat(rg): add early real rg file types ([#1711](https://github.com/everruns/bashkit/pull/1711)) by @chaliy
* feat(rg): add more real rg file types ([#1710](https://github.com/everruns/bashkit/pull/1710)) by @chaliy
* feat(rg): add additional default file types ([#1709](https://github.com/everruns/bashkit/pull/1709)) by @chaliy
* feat(rg): add more language file types ([#1708](https://github.com/everruns/bashkit/pull/1708)) by @chaliy
* feat(rg): support globstar directory prefixes ([#1707](https://github.com/everruns/bashkit/pull/1707)) by @chaliy
* feat(rg): add more default file types ([#1706](https://github.com/everruns/bashkit/pull/1706)) by @chaliy
* feat(rg): support escaped glob metacharacters ([#1705](https://github.com/everruns/bashkit/pull/1705)) by @chaliy
* feat(rg): support glob brace alternation ([#1704](https://github.com/everruns/bashkit/pull/1704)) by @chaliy
* feat(rg): support glob character classes ([#1703](https://github.com/everruns/bashkit/pull/1703)) by @chaliy
* feat(rg): add common file types ([#1702](https://github.com/everruns/bashkit/pull/1702)) by @chaliy
* feat(rg): support hex ansi color numbers ([#1701](https://github.com/everruns/bashkit/pull/1701)) by @chaliy
* feat(rg): support ansi color numbers ([#1700](https://github.com/everruns/bashkit/pull/1700)) by @chaliy
* feat(rg): support highlight colors ([#1699](https://github.com/everruns/bashkit/pull/1699)) by @chaliy
* feat(rg): validate sort choices ([#1698](https://github.com/everruns/bashkit/pull/1698)) by @chaliy
* feat(rg): support all file types ([#1697](https://github.com/everruns/bashkit/pull/1697)) by @chaliy
* feat(rg): honor option delimiter ([#1696](https://github.com/everruns/bashkit/pull/1696)) by @chaliy
* feat(rg): parse separator escapes as bytes ([#1695](https://github.com/everruns/bashkit/pull/1695)) by @chaliy
* feat(rg): emit hyperlink prefixes ([#1694](https://github.com/everruns/bashkit/pull/1694)) by @chaliy
* feat(rg): expand custom color styles ([#1693](https://github.com/everruns/bashkit/pull/1693)) by @chaliy
* feat(rg): honor custom color styles ([#1692](https://github.com/everruns/bashkit/pull/1692)) by @chaliy
* feat(rg): add ansi color output ([#1686](https://github.com/everruns/bashkit/pull/1686)) by @chaliy
* feat(rg): honor global git ignore files ([#1685](https://github.com/everruns/bashkit/pull/1685)) by @chaliy
* feat(rg): honor parent ignore files ([#1684](https://github.com/everruns/bashkit/pull/1684)) by @chaliy
* feat(rg): expand generated completion flags ([#1683](https://github.com/everruns/bashkit/pull/1683)) by @chaliy
* feat(rg): gate preprocessors with pre-glob ([#1682](https://github.com/everruns/bashkit/pull/1682)) by @chaliy
* feat(rg): honor ascii regex mode ([#1681](https://github.com/everruns/bashkit/pull/1681)) by @chaliy
* feat(rg): add generate output mode ([#1680](https://github.com/everruns/bashkit/pull/1680)) by @chaliy
* feat(rg): add reset compatibility flags ([#1679](https://github.com/everruns/bashkit/pull/1679)) by @chaliy
* feat(rg): accept diagnostic controls ([#1678](https://github.com/everruns/bashkit/pull/1678)) by @chaliy
* feat(rg): accept preprocessor controls ([#1677](https://github.com/everruns/bashkit/pull/1677)) by @chaliy
* feat(rg): add gzip search mode ([#1676](https://github.com/everruns/bashkit/pull/1676)) by @chaliy
* feat(rg): follow symlink search paths ([#1675](https://github.com/everruns/bashkit/pull/1675)) by @chaliy
* feat(rg): add context separator controls ([#1674](https://github.com/everruns/bashkit/pull/1674)) by @chaliy
* feat(rg): add null-data record search ([#1673](https://github.com/everruns/bashkit/pull/1673)) by @chaliy
* feat(rg): add color and engine compatibility flags ([#1672](https://github.com/everruns/bashkit/pull/1672)) by @chaliy
* feat(rg): add compatibility flag aliases ([#1671](https://github.com/everruns/bashkit/pull/1671)) by @chaliy
* feat(rg): add ignore toggle parity ([#1670](https://github.com/everruns/bashkit/pull/1670)) by @chaliy
* feat(rg): add max depth aliases ([#1669](https://github.com/everruns/bashkit/pull/1669)) by @chaliy
* feat(rg): add ignore file control flags ([#1668](https://github.com/everruns/bashkit/pull/1668)) by @chaliy
* feat(rg): add max-filesize filtering ([#1667](https://github.com/everruns/bashkit/pull/1667)) by @chaliy
* feat(rg): add case-insensitive glob filters ([#1666](https://github.com/everruns/bashkit/pull/1666)) by @chaliy
* fix(rg): handle multiline invert matches ([#1665](https://github.com/everruns/bashkit/pull/1665)) by @chaliy
* feat(rg): add multiline mode ([#1664](https://github.com/everruns/bashkit/pull/1664)) by @chaliy
* feat(rg): add crlf mode ([#1663](https://github.com/everruns/bashkit/pull/1663)) by @chaliy
* feat(rg): add encoding support ([#1662](https://github.com/everruns/bashkit/pull/1662)) by @chaliy
* feat(rg): add engine and mmap flags ([#1661](https://github.com/everruns/bashkit/pull/1661)) by @chaliy
* feat(rg): add path separator output ([#1660](https://github.com/everruns/bashkit/pull/1660)) by @chaliy
* feat(rg): add config and git ignore controls ([#1659](https://github.com/everruns/bashkit/pull/1659)) by @chaliy
* feat(rg): add unrestricted filter mode ([#1658](https://github.com/everruns/bashkit/pull/1658)) by @chaliy
* feat(rg): add stats output ([#1657](https://github.com/everruns/bashkit/pull/1657)) by @chaliy
* feat(rg): add message controls ([#1656](https://github.com/everruns/bashkit/pull/1656)) by @chaliy
* feat(rg): add max columns modes ([#1655](https://github.com/everruns/bashkit/pull/1655)) by @chaliy
* feat(rg): add binary text modes ([#1654](https://github.com/everruns/bashkit/pull/1654)) by @chaliy
* feat(examples): reduce docs agent token waste by @chaliy
* feat(rg): add ignore file support ([#1652](https://github.com/everruns/bashkit/pull/1652)) by @chaliy
* feat(rg): add type definition flags ([#1651](https://github.com/everruns/bashkit/pull/1651)) by @chaliy
* feat(rg): add hidden and type filters ([#1648](https://github.com/everruns/bashkit/pull/1648)) by @chaliy
* feat(rg): add output modes ([#1645](https://github.com/everruns/bashkit/pull/1645)) by @chaliy
* feat(rg): add next parity flags ([#1644](https://github.com/everruns/bashkit/pull/1644)) by @chaliy
* feat(examples): improve docs agent search by @chaliy
* feat(rg): improve ripgrep parity by @chaliy
* feat(rg): support context and glob search by @chaliy
* feat(examples): add docs grep agent by @chaliy
* refactor(deps): remove unused dependency edges ([#1639](https://github.com/everruns/bashkit/pull/1639)) by @chaliy
* fix(date): force UTC formatting under virtual clock modes ([#1637](https://github.com/everruns/bashkit/pull/1637)) by @chaliy
* chore(deps): bump turso_core 0.5.3 → 0.6.0 ([#1636](https://github.com/everruns/bashkit/pull/1636)) by @chaliy
* fix(testing): prevent fuzz leak guard false negatives on clap invalid-value lines ([#1631](https://github.com/everruns/bashkit/pull/1631)) by @chaliy
* chore: deep-maintenance follow-ups — just vet, count drift, TM-INF-018, deps ([#1635](https://github.com/everruns/bashkit/pull/1635)) by @chaliy
* chore(examples): bump @langchain/* to v1.x major ([#1633](https://github.com/everruns/bashkit/pull/1633)) by @chaliy
* chore: deep-maintenance pass — fuzz fix, dep bumps, doc sync ([#1632](https://github.com/everruns/bashkit/pull/1632)) by @chaliy

**Full Changelog**: https://github.com/everruns/bashkit/compare/v0.6.0...v0.7.0

## [0.6.0] - 2026-05-16

### Highlights

- **Continue coreutils adoption experiment** — Extends the codegen pipeline beyond `uu_app()` argument surfaces to vendor whole upstream uutils modules with a manifest and drift-detection CI. `tee`, `mktemp`, `realpath`, `stat`, and `od` now flow through codegen; `printf` runs on a vendored copy of uucore's format implementation; and `env` is ported through a virtual-env shim (TM-INF-024) ([#1592](https://github.com/everruns/bashkit/pull/1592), [#1593](https://github.com/everruns/bashkit/pull/1593), [#1594](https://github.com/everruns/bashkit/pull/1594)).
- **MCP server mode removed from CLI** — The `bashkit mcp` server mode has been removed. The recommended path for MCP integrations is now to embed bashkit via the library bindings.
- **Security hardening across the sandbox** — Fail-closed fixes across realfs (no-follow resolver for stat/read_link/remove; reject leaf-symlink writes), snapshot/restore (atomic, fail-closed `vfs_restore`), sqlite (reject `VACUUM`/`VACUUM INTO`; row caps; engine cache invalidation), network (SSRF precheck fails closed; IPv4-mapped IPv6 normalization), jq (replace `halt` to stop sandbox escape via `process::exit`; fancy-regex execution + file-binding caps), and ssh (shell-escape sftp `ls`; try `none`-auth before password/key). The final 6 OPEN entries in the threat model are now marked mitigated ([#1568](https://github.com/everruns/bashkit/pull/1568), [#1581](https://github.com/everruns/bashkit/pull/1581), [#1582](https://github.com/everruns/bashkit/pull/1582), [#1583](https://github.com/everruns/bashkit/pull/1583), [#1584](https://github.com/everruns/bashkit/pull/1584), [#1585](https://github.com/everruns/bashkit/pull/1585), [#1586](https://github.com/everruns/bashkit/pull/1586), [#1587](https://github.com/everruns/bashkit/pull/1587), [#1588](https://github.com/everruns/bashkit/pull/1588), [#1589](https://github.com/everruns/bashkit/pull/1589), [#1590](https://github.com/everruns/bashkit/pull/1590), [#1591](https://github.com/everruns/bashkit/pull/1591), [#1599](https://github.com/everruns/bashkit/pull/1599), [#1601](https://github.com/everruns/bashkit/pull/1601), [#1613](https://github.com/everruns/bashkit/pull/1613), [#1615](https://github.com/everruns/bashkit/pull/1615)).

### Breaking Changes

- **CLI MCP server mode removed**: The `bashkit mcp` subcommand and the MCP server transport bundled in the CLI have been removed.
  - Before: `bashkit mcp --transport stdio`
  - After: embed bashkit via the library bindings (`bashkit` crate, `@everruns/bashkit` on npm, `bashkit` on PyPI) and expose tools through your own MCP server.

### What's Changed

* fix(coreutils-port): constrain uu_app builder macro arguments ([#1629](https://github.com/everruns/bashkit/pull/1629)) by @chaliy
* fix(coreutils-port): accept localized-Command let-binding in uu_app ([#1628](https://github.com/everruns/bashkit/pull/1628)) by @chaliy
* chore(deps): bump the rust-dependencies group with 3 updates ([#1626](https://github.com/everruns/bashkit/pull/1626)) by @dependabot
* fix(fuzz): strip real-shell error lines from stderr before banned-shape check ([#1623](https://github.com/everruns/bashkit/pull/1623)) by @chaliy
* fix(fuzz): drop arithmetic_fuzz inputs that contain banned debug shapes ([#1622](https://github.com/everruns/bashkit/pull/1622)) by @chaliy
* fix(fuzz): drop glob_fuzz inputs that contain banned debug shapes ([#1621](https://github.com/everruns/bashkit/pull/1621)) by @chaliy
* fix(coreutils-port): allow safe clap macros in uu_app validator ([#1620](https://github.com/everruns/bashkit/pull/1620)) by @chaliy
* fix(bashkit-eval): make rustls provider init idempotent ([#1619](https://github.com/everruns/bashkit/pull/1619)) by @chaliy
* fix(printf): cap float exponent magnitude in format validation ([#1618](https://github.com/everruns/bashkit/pull/1618)) by @chaliy
* fix(coreutils-port): harden uu_app builder validation ([#1617](https://github.com/everruns/bashkit/pull/1617)) by @chaliy
* fix(sqlite): enforce row cap while stepping ([#1615](https://github.com/everruns/bashkit/pull/1615)) by @chaliy
* fix(ci): isolate coreutils drift external execution ([#1614](https://github.com/everruns/bashkit/pull/1614)) by @chaliy
* fix(jq): cap file binding memory ([#1613](https://github.com/everruns/bashkit/pull/1613)) by @chaliy
* fix(ci): sandbox coreutils drift generation ([#1611](https://github.com/everruns/bashkit/pull/1611)) by @chaliy
* fix(export): continue after invalid identifier to avoid stale env sync ([#1610](https://github.com/everruns/bashkit/pull/1610)) by @chaliy
* fix(js): correct sqlite maxMemory unit handling ([#1609](https://github.com/everruns/bashkit/pull/1609)) by @chaliy
* chore(specs): mark TM-DOS-057 partial on WASM ([#1607](https://github.com/everruns/bashkit/pull/1607)) by @chaliy
* fix(bashkit-eval): install rustls provider for library providers ([#1606](https://github.com/everruns/bashkit/pull/1606)) by @chaliy
* fix(python): preserve credential placeholder env on snapshot restore ([#1605](https://github.com/everruns/bashkit/pull/1605)) by @chaliy
* fix(export): sync successful exports when readonly args fail ([#1604](https://github.com/everruns/bashkit/pull/1604)) by @chaliy
* fix(tool_def): reject bare array flags without values ([#1603](https://github.com/everruns/bashkit/pull/1603)) by @chaliy
* fix(jq): enforce fancy-regex execution limits ([#1601](https://github.com/everruns/bashkit/pull/1601)) by @chaliy
* fix(bindings): derive sqlite limits from host time and memory caps ([#1600](https://github.com/everruns/bashkit/pull/1600)) by @chaliy
* fix(sqlite): invalidate cached engine when VFS file changes ([#1599](https://github.com/everruns/bashkit/pull/1599)) by @chaliy
* fix(tool_def): bound aggregate JSON flag coercion ([#1598](https://github.com/everruns/bashkit/pull/1598)) by @chaliy
* fix(scripted-tool): isolate and bound extension invocation traces ([#1597](https://github.com/everruns/bashkit/pull/1597)) by @chaliy
* fix(scripts): follow redirects and bump just to 1.50.0 in init-cloud-env ([#1595](https://github.com/everruns/bashkit/pull/1595)) by @chaliy
* refactor(builtins): port tee/mktemp/realpath/stat/od to codegen args ([#1594](https://github.com/everruns/bashkit/pull/1594)) by @chaliy
* feat(coreutils-port): add module-vendor mode with manifest and drift CI ([#1593](https://github.com/everruns/bashkit/pull/1593)) by @chaliy
* feat(builtins): port uutils env-default surface via virtual-env shim (TM-INF-024) ([#1592](https://github.com/everruns/bashkit/pull/1592)) by @chaliy
* fix(network): fail closed in SSRF precheck and document handler responsibility ([#1591](https://github.com/everruns/bashkit/pull/1591)) by @chaliy
* fix(network): normalize IPv4-mapped IPv6 in is_private_ip to block SSRF ([#1590](https://github.com/everruns/bashkit/pull/1590)) by @chaliy
* fix(jq): replace halt native to stop sandbox-escape via process::exit ([#1589](https://github.com/everruns/bashkit/pull/1589)) by @chaliy
* fix(sqlite): reject VACUUM to block VFS escape via VACUUM INTO ([#1588](https://github.com/everruns/bashkit/pull/1588)) by @chaliy
* fix(interop): mark filesystem import unsafe and own the foreign vtable ([#1587](https://github.com/everruns/bashkit/pull/1587)) by @chaliy
* fix(snapshot): make vfs_restore fail closed and apply atomically ([#1586](https://github.com/everruns/bashkit/pull/1586)) by @chaliy
* fix(realfs): reject leaf-symlink writes to block dangling-symlink escape ([#1585](https://github.com/everruns/bashkit/pull/1585)) by @chaliy
* fix(realfs): use no-follow resolver for stat/read_link/remove ([#1584](https://github.com/everruns/bashkit/pull/1584)) by @chaliy
* fix(ln): surface remove failure under -f instead of falling through to symlink ([#1583](https://github.com/everruns/bashkit/pull/1583)) by @chaliy
* fix(ssh): try none-auth before password/key to avoid leaking defaults ([#1582](https://github.com/everruns/bashkit/pull/1582)) by @chaliy
* fix(ssh): shell-escape sftp ls path to prevent remote command injection ([#1581](https://github.com/everruns/bashkit/pull/1581)) by @chaliy
* docs(threat-model): mark final 6 OPEN entries mitigated ([#1568](https://github.com/everruns/bashkit/pull/1568)) by @chaliy
* fix(coreutils-port): accept let-bound Command chain in uu_app validator by @chaliy
* fix(fuzz): strip uutils clap error chrome before banned-shape check by @chaliy
* chore(ci): bump artifact actions by @dependabot
* feat(printf): vendor uucore format by @chaliy
* fix(truncate): enforce VFS limits before resize by @chaliy
* fix(shuf): cap range and repeat output allocation by @chaliy
* fix(cli): remove MCP server mode by @chaliy
* chore(maintenance): add deepsec scanning workspace by @chaliy

**Full Changelog**: https://github.com/everruns/bashkit/compare/v0.5.0...v0.6.0

## [0.5.0] - 2026-05-06

### Highlights

- **Coreutils argument surface via codegen** — New POC pipeline ports uutils' `uu_app()` clap definitions into bashkit so builtins share the real coreutils argument shape; `cat`, `tac`, `truncate`, `shuf`, and `readlink` now flow through this surface, with a coreutils differential testing harness to catch parity drift. The codegen pipeline reads a single pinned uutils revision so generated builtins, the differential harness, and CI all agree on the upstream source of truth ([#1529](https://github.com/everruns/bashkit/pull/1529), [#1535](https://github.com/everruns/bashkit/pull/1535), [#1536](https://github.com/everruns/bashkit/pull/1536), [#1537](https://github.com/everruns/bashkit/pull/1537), [#1538](https://github.com/everruns/bashkit/pull/1538), [#1542](https://github.com/everruns/bashkit/pull/1542)).
- **Site updates** — Bashkit agent skill is now published on the site, alongside rustdoc guides and content signal declarations for discoverability.

### What's Changed

* refactor(builtins): migrate readlink to codegen-ported argument surface ([#1542](https://github.com/everruns/bashkit/pull/1542)) by @chaliy
* chore(site): publish bashkit agent skill ([#1541](https://github.com/everruns/bashkit/pull/1541)) by @chaliy
* chore(site): declare content signals by @chaliy
* docs(site): publish rustdoc guides by @chaliy
* feat(builtins): add shuf via codegen with helper-fn inlining ([#1538](https://github.com/everruns/bashkit/pull/1538)) by @chaliy
* chore(builtins): pin uutils revision as single source of truth ([#1537](https://github.com/everruns/bashkit/pull/1537)) by @chaliy
* feat(builtins): add truncate via codegen-ported argument surface ([#1536](https://github.com/everruns/bashkit/pull/1536)) by @chaliy
* test(builtins): add coreutils differential testing harness ([#1535](https://github.com/everruns/bashkit/pull/1535)) by @chaliy
* feat(builtins): port uutils argument surfaces via codegen (POC: cat, tac) ([#1529](https://github.com/everruns/bashkit/pull/1529)) by @chaliy
* feat(tool_def): accept --flag key=value... syntax for object/array flags ([#1528](https://github.com/everruns/bashkit/pull/1528)) by @chaliy
* fix(tool_def): coerce stringified JSON for array/object flag schemas ([#1527](https://github.com/everruns/bashkit/pull/1527)) by @chaliy

**Full Changelog**: https://github.com/everruns/bashkit/compare/v0.4.1...v0.5.0

## [0.4.1] - 2026-05-04

### Highlights

- **Fix `bashkit` v0.4.0 crates.io publish** — Move `docs/clap-builtins.md` into `crates/bashkit/docs/` (matching the rustdoc-guides convention) and update the `include_str!` path so the guide is packaged inside the crate. v0.4.0 was published to PyPI, npm, and Homebrew but the crates.io publish failed because the guide lived outside the crate directory.

### What's Changed

* fix(docs): move clap-builtins guide inside bashkit crate so cargo publish includes it by @chaliy

**Full Changelog**: https://github.com/everruns/bashkit/compare/v0.4.0...v0.4.1

## [0.4.0] - 2026-05-04

### Highlights

- **Builtin extension abstraction** — New public `Extension` trait groups related builtins for one-call registration on `BashBuilder`/`BashToolBuilder`. TypeScript registration now flows through `TypeScriptExtension`, and `ScriptedTool` reuses a shared `ToolDefExtension` for its per-call logic shell ([#1515](https://github.com/everruns/bashkit/pull/1515), [#1518](https://github.com/everruns/bashkit/pull/1518)).
- **Clap-backed custom builtins** — Custom builtins can now be defined declaratively against a `clap` parser, replacing hand-rolled arg parsing for new integrations ([#1514](https://github.com/everruns/bashkit/pull/1514)).
- **SQLite session engine cache** — The `sqlite` builtin keeps a session-scoped engine alive across `exec()` calls, so transactions and prepared state survive multiple shell invocations within one session ([#1513](https://github.com/everruns/bashkit/pull/1513)).
- **SQLite hardening follow-up** — PRAGMA policy parsing now handles SQL comments and quoted/bracket/backtick identifiers (closing a `PRAGMA main."cache_size"` bypass), and `max_db_bytes` is enforced consistently across VFS writes/truncates and memory-backend persistence ([#1521](https://github.com/everruns/bashkit/pull/1521)).
- **Python + toolchain bumps** — Embedded Python (`monty`) bumped to `0.0.17` and Rust toolchain bumped to `1.95.0` across `rust-toolchain.toml` and matching CI workflow refs ([#1520](https://github.com/everruns/bashkit/pull/1520)).

### What's Changed

* bench(sqlite): add Criterion benchmark for sqlite builtin ([#1523](https://github.com/everruns/bashkit/pull/1523)) by @chaliy
* chore(python): bump monty to 49faa4c (0.0.17) and Rust to 1.95.0 ([#1520](https://github.com/everruns/bashkit/pull/1520)) by @chaliy
* fix(deps): bump postcss to 8.5.13 in browser example ([#1522](https://github.com/everruns/bashkit/pull/1522)) by @chaliy
* fix(sqlite): harden pragma policy and db caps ([#1521](https://github.com/everruns/bashkit/pull/1521)) by @chaliy
* test(ssh): retry live supabase smoke ([#1519](https://github.com/everruns/bashkit/pull/1519)) by @chaliy
* feat(scripted-tool): add ToolDef extension ([#1518](https://github.com/everruns/bashkit/pull/1518)) by @chaliy
* feat(sqlite): session-scoped engine cache for transactions across exec() ([#1513](https://github.com/everruns/bashkit/pull/1513)) by @chaliy
* feat(extension): add builtin extension abstraction ([#1515](https://github.com/everruns/bashkit/pull/1515)) by @chaliy
* feat(builtins): support clap-backed custom builtins ([#1514](https://github.com/everruns/bashkit/pull/1514)) by @chaliy
* test(sqlite): add differential tests vs host sqlite3 binary ([#1511](https://github.com/everruns/bashkit/pull/1511)) by @chaliy

**Full Changelog**: https://github.com/everruns/bashkit/compare/v0.3.0...v0.4.0

## [0.3.0] - 2026-05-02

### Highlights

- **Embedded SQLite via Turso** — New `sqlite` builtin with `MemoryIO` and `VfsIO` backends, dot-commands (`.tables`, `.schema`, `.dump`), and a hardened deny list that blocks `ATTACH`/`DETACH` and dangerous `PRAGMA`s. Exposed to Python and JS bindings ([#1502](https://github.com/everruns/bashkit/pull/1502), [#1507](https://github.com/everruns/bashkit/pull/1507), [#1510](https://github.com/everruns/bashkit/pull/1510)).
- **jq parity expansion** — Filter module restructured for parity with real jq, regex backend swapped from `regex` to `fancy-regex` for advanced patterns (lookaround, backreferences), `@tsv`/`@csv` defs and short jq-style runtime errors with cross-tool no-Debug-leak guard (TM-INF-022) ([#1501](https://github.com/everruns/bashkit/pull/1501), [#1503](https://github.com/everruns/bashkit/pull/1503), [#1508](https://github.com/everruns/bashkit/pull/1508)).
- **Scripted-tool logic-only mode** — Orchestrator scripts can now run in a restricted "code mode" that disables shell features unrelated to control flow, narrowing the attack surface for LLM-authored scripts.
- **Python network credentials** — Phase 2 of [#1348](https://github.com/everruns/bashkit/pull/1348) lands `credentials=` and `credential_placeholders=` kwargs on `Bash(network=...)`, completing the Python-side network policy surface ([#1499](https://github.com/everruns/bashkit/pull/1499)).
- **Toolchain pinning** — Rust toolchain pinned via `rust-toolchain.toml` and matched in CI workflow refs to prevent same-day rustc releases from breaking CI; RSA advisory mirrored locally and dead code gated ([#1509](https://github.com/everruns/bashkit/pull/1509)).

### What's Changed

* feat(bindings): expose sqlite builtin to Python and JS bindings ([#1510](https://github.com/everruns/bashkit/pull/1510)) by @chaliy
* chore(ci): pin Rust toolchain, mirror RSA advisory, gate dead code ([#1509](https://github.com/everruns/bashkit/pull/1509)) by @chaliy
* feat(jq): replace regex backend with fancy-regex for advanced patterns ([#1508](https://github.com/everruns/bashkit/pull/1508)) by @chaliy
* feat(sqlite): block ATTACH/DETACH, PRAGMA deny list, dependabot rule ([#1507](https://github.com/everruns/bashkit/pull/1507)) by @chaliy
* fix(ci): green up failing main-branch tests and example ([#1506](https://github.com/everruns/bashkit/pull/1506)) by @chaliy
* refactor: move git and ssh modules into builtins/ ([#1505](https://github.com/everruns/bashkit/pull/1505)) by @chaliy
* feat(jq): expand parity with real jq + restructure into module ([#1503](https://github.com/everruns/bashkit/pull/1503)) by @chaliy
* feat(sqlite): embedded SQLite via Turso (Phase 1 + Phase 2) ([#1502](https://github.com/everruns/bashkit/pull/1502)) by @chaliy
* fix(jq): @tsv/@csv defs + short jq-style errors; cross-tool no-Debug-leak guard (TM-INF-022) ([#1501](https://github.com/everruns/bashkit/pull/1501)) by @chaliy
* chore(deps): bump all workspace dependencies ([#1500](https://github.com/everruns/bashkit/pull/1500)) by @chaliy
* feat(python): add credentials + credential_placeholders to network= (phase 2 of #1348) ([#1499](https://github.com/everruns/bashkit/pull/1499)) by @chaliy
* fix(curl): use curl user agent by default ([#1498](https://github.com/everruns/bashkit/pull/1498)) by @chaliy
* fix(bashkit): gate Path import behind realfs ([#1497](https://github.com/everruns/bashkit/pull/1497)) by @chaliy
* fix(cli): enable python execution by default by @chaliy
* ci(python): build aarch64 wheels on native ubuntu-24.04-arm ([#1495](https://github.com/everruns/bashkit/pull/1495)) by @chaliy
* feat(scripted-tool): run scripts in logic-only code mode by @chaliy

**Full Changelog**: https://github.com/everruns/bashkit/compare/v0.2.1...v0.3.0

## [0.2.1] - 2026-04-30

### Highlights

- **Windows mount path validation** — `MountableFs::mount` now uses `Path::has_root` instead of `Path::is_absolute`, so POSIX-style mount points like `/workspace` are accepted on every host. The v0.2.0 interop FS roundtrip test (`vfs › filesystem external roundtrip mounts into bash`) regressed silently on Windows because PR CI runs Rust tests on Linux only ([#1492](https://github.com/everruns/bashkit/pull/1492)).
- **rustls switched to ring** — Replaces the `aws-lc-rs`/`aws-lc-sys` C crypto stack with pure-Rust ring across the workspace. Unblocks the aarch64 manylinux wheel build, which had failed in v0.2.0 with `'AT_HWCAP2' undeclared` from the cross-compiled `aws-lc-sys 0.39.1`. `cargo tree -i aws-lc-sys` now returns no match ([#1493](https://github.com/everruns/bashkit/pull/1493)).

### What's Changed

* fix(fs): validate mount paths with POSIX semantics on Windows ([#1492](https://github.com/everruns/bashkit/pull/1492)) by @chaliy
* fix(http): switch rustls crypto provider to ring ([#1493](https://github.com/everruns/bashkit/pull/1493)) by @chaliy

**Full Changelog**: https://github.com/everruns/bashkit/compare/v0.2.0...v0.2.1

## [0.2.0] - 2026-04-30

### Highlights

- **Filesystem interop ABI** — New `bashkit::interop::fs` (behind the `interop` cargo feature) exposes a versioned `repr(C)` filesystem handle + vtable, plus Python `FileSystem.from_capsule()` / `to_capsule()` and Node `FileSystem.fromExternal()` / `toExternal()` so downstream native extensions can mount custom filesystems without sharing addon-private layouts ([#1353](https://github.com/everruns/bashkit/pull/1353))
- **Python `network=` constructor kwarg** — `Bash(network=...)` now controls outbound network access at construction time, the first phase of the network policy work tracked in [#1348](https://github.com/everruns/bashkit/pull/1348) ([#1489](https://github.com/everruns/bashkit/pull/1489))
- **jq compatibility** — Added `input_filename` / `input_line_number` / `$ENV` stubs ([#1490](https://github.com/everruns/bashkit/pull/1490)) and bounded, jq-like runtime error messages that no longer dump full input values to stderr ([#1488](https://github.com/everruns/bashkit/pull/1488))

### What's Changed

* feat(jq): add input_filename / input_line_number / $ENV stubs ([#1490](https://github.com/everruns/bashkit/pull/1490)) by @chaliy
* feat(python): add network= constructor kwarg (phase 1 of #1348) ([#1489](https://github.com/everruns/bashkit/pull/1489)) by @chaliy
* fix(jq): format runtime errors without value dumps ([#1488](https://github.com/everruns/bashkit/pull/1488)) by @chaliy
* feat(site): add canonical sitemap generation ([#1485](https://github.com/everruns/bashkit/pull/1485)) by @chaliy
* chore(deps): bump the rust-dependencies group with 5 updates ([#1483](https://github.com/everruns/bashkit/pull/1483)) by @dependabot
* feat(interop): add filesystem ABI handles ([#1353](https://github.com/everruns/bashkit/pull/1353)) by @chaliy

**Full Changelog**: https://github.com/everruns/bashkit/compare/v0.1.21...v0.2.0

## [0.1.21] - 2026-04-25

### Highlights

- **Codex security run** — Large-scale security and hardening sweep landed 100+ fixes: panic-safety across `expr`/`sed`/`sort`/`date`/`extglob`, resource limits for `local`/allexport/lazy files/`yes`/`dirstack`, depth and size caps in `jq`/`sed`/`mktemp`/snapshot restore, RealFs symlink-escape blocks, bot-auth signatures bound to method+URI, JS real-FS mount allowlist, scripted-tool injection blocks, MCP stdin bounds, `unzip` path-traversal blocks, parser timeout/input limits in WASM/`eval`/`source`/nested-bash, and credential/log redaction
- **Site and docs** — New `bashkit.sh` homepage (Astro + Cloudflare), public CLI reference rewrite, canonical `/docs` rendering, builtins index, and refreshed homepage proof and sourcing
- **External contributions** — Thanks to @oliverlambson for `BuiltinResult` support in Python custom builtins ([#1427](https://github.com/everruns/bashkit/pull/1427)) and publishing Python 3.14 wheels to PyPI ([#1351](https://github.com/everruns/bashkit/pull/1351))

### What's Changed

* chore(deps): address security advisories ([#1481](https://github.com/everruns/bashkit/pull/1481)) by @chaliy
* fix(interpreter): restore errexit for mixed and-or lists ([#1480](https://github.com/everruns/bashkit/pull/1480)) by @chaliy
* fix(parser): preserve literal semantics for single-quoted =~ regex patterns ([#1479](https://github.com/everruns/bashkit/pull/1479)) by @chaliy
* fix(read): split on mixed whitespace/non-whitespace IFS ([#1478](https://github.com/everruns/bashkit/pull/1478)) by @chaliy
* fix(interpreter): assign array elements for parameter := expansion ([#1477](https://github.com/everruns/bashkit/pull/1477)) by @chaliy
* fix(strings): restore `-NUM` shorthand argument handling ([#1476](https://github.com/everruns/bashkit/pull/1476)) by @chaliy
* fix(find): preserve missing-path errors in -exec execution plans ([#1475](https://github.com/everruns/bashkit/pull/1475)) by @chaliy
* fix(awk): avoid double evaluation in boolean binops ([#1474](https://github.com/everruns/bashkit/pull/1474)) by @chaliy
* fix(interpreter): preserve truncating redirects with empty mixed fd output ([#1473](https://github.com/everruns/bashkit/pull/1473)) by @chaliy
* fix(interpreter): parse arithmetic parameter operators at first operator ([#1472](https://github.com/everruns/bashkit/pull/1472)) by @chaliy
* fix(js): enforce allowlist for real filesystem mounts ([#1471](https://github.com/everruns/bashkit/pull/1471)) by @chaliy
* fix(python): virtualize datetime clock in sandbox ([#1470](https://github.com/everruns/bashkit/pull/1470)) by @chaliy
* fix(credential): restrict placeholder replacement to credential headers ([#1469](https://github.com/everruns/bashkit/pull/1469)) by @chaliy
* fix(js): retain ScriptedTool callbacks for weak TSFN ([#1468](https://github.com/everruns/bashkit/pull/1468)) by @chaliy
* fix(snapshot): preserve legacy function snapshot deserialization ([#1467](https://github.com/everruns/bashkit/pull/1467)) by @chaliy
* fix(awk): close range when start and end match same record ([#1466](https://github.com/everruns/bashkit/pull/1466)) by @chaliy
* fix(grep): avoid quadratic context match lookup ([#1465](https://github.com/everruns/bashkit/pull/1465)) by @chaliy
* fix(logging): redact and sanitize execution errors before logging ([#1464](https://github.com/everruns/bashkit/pull/1464)) by @chaliy
* fix(jq): enforce depth limit for --argjson values ([#1463](https://github.com/everruns/bashkit/pull/1463)) by @chaliy
* fix(parser): preserve literal dollars in concatenated single quotes ([#1462](https://github.com/everruns/bashkit/pull/1462)) by @chaliy
* fix(builtins): harden mktemp against path collisions ([#1461](https://github.com/everruns/bashkit/pull/1461)) by @chaliy
* fix(sed): enforce grouped command depth at parse time ([#1460](https://github.com/everruns/bashkit/pull/1460)) by @chaliy
* fix(python): harden in-process opt-in against script overrides ([#1459](https://github.com/everruns/bashkit/pull/1459)) by @chaliy
* test: add regressions for #1414, #1421, #1401 hardening fixes ([#1458](https://github.com/everruns/bashkit/pull/1458)) by @chaliy
* fix(archive): reject malformed tar size headers ([#1457](https://github.com/everruns/bashkit/pull/1457)) by @chaliy
* fix(interpreter): prevent duplicate shell flags from leaking to parent state ([#1456](https://github.com/everruns/bashkit/pull/1456)) by @chaliy
* fix(expr): avoid UTF-8 panic in capture-group matches ([#1455](https://github.com/everruns/bashkit/pull/1455)) by @chaliy
* fix(interpreter): preserve control flow in case fallthrough ([#1454](https://github.com/everruns/bashkit/pull/1454)) by @chaliy
* fix(interpreter): restore env once per prefix assignment name ([#1453](https://github.com/everruns/bashkit/pull/1453)) by @chaliy
* fix(fs): enforce parent existence check in PosixFs::append_file ([#1452](https://github.com/everruns/bashkit/pull/1452)) by @chaliy
* fix(git): harden remote allowlist URL validation ([#1451](https://github.com/everruns/bashkit/pull/1451)) by @chaliy
* fix(parser): split assignments on first '=' to avoid += value regression ([#1450](https://github.com/everruns/bashkit/pull/1450)) by @chaliy
* fix(rg): propagate -m missing-value parse error ([#1449](https://github.com/everruns/bashkit/pull/1449)) by @chaliy
* fix(interpreter): restore full assoc subscript parameter expansion ([#1448](https://github.com/everruns/bashkit/pull/1448)) by @chaliy
* fix(fs): honor upper dir precedence in overlay read_dir ([#1447](https://github.com/everruns/bashkit/pull/1447)) by @chaliy
* fix(hooks): honor on_exit cancel and modified exit code ([#1446](https://github.com/everruns/bashkit/pull/1446)) by @chaliy
* fix(parsers): harden unicode and nesting guards ([#1445](https://github.com/everruns/bashkit/pull/1445)) by @chaliy
* fix(scripts): verify bootstrap binary integrity ([#1444](https://github.com/everruns/bashkit/pull/1444)) by @chaliy
* fix(grep): avoid panic in quoted include/exclude parsing ([#1443](https://github.com/everruns/bashkit/pull/1443)) by @chaliy
* docs(cli): rewrite public CLI reference ([#1442](https://github.com/everruns/bashkit/pull/1442)) by @chaliy
* fix(glob): honor dotglob during globstar directory traversal ([#1441](https://github.com/everruns/bashkit/pull/1441)) by @chaliy
* fix(parser): preserve quoting semantics for $'...' and $"..." ([#1440](https://github.com/everruns/bashkit/pull/1440)) by @chaliy
* fix(alias): preserve quoted words during alias reparse ([#1439](https://github.com/everruns/bashkit/pull/1439)) by @chaliy
* fix(parser): preserve quoted expansion semantics in mixed words ([#1438](https://github.com/everruns/bashkit/pull/1438)) by @chaliy
* fix(cli): keep network default-deny unless explicitly enabled ([#1437](https://github.com/everruns/bashkit/pull/1437)) by @chaliy
* fix(interpreter): suppress coproc output in streaming callbacks ([#1436](https://github.com/everruns/bashkit/pull/1436)) by @chaliy
* fix(js): keep cancellation working after reset ([#1435](https://github.com/everruns/bashkit/pull/1435)) by @chaliy
* fix(parser): enforce parser timeout in wasm parse path ([#1434](https://github.com/everruns/bashkit/pull/1434)) by @chaliy
* fix(parser): validate quoted command/process substitutions in budget pass ([#1433](https://github.com/everruns/bashkit/pull/1433)) by @chaliy
* fix(fs): enforce max_dir_count for recursive mkdir and dir CoW in OverlayFs ([#1432](https://github.com/everruns/bashkit/pull/1432)) by @chaliy
* fix(interpreter): isolate exec fd table across subshell contexts ([#1431](https://github.com/everruns/bashkit/pull/1431)) by @chaliy
* fix(interpreter): propagate [[ ]] expansion errors ([#1430](https://github.com/everruns/bashkit/pull/1430)) by @chaliy
* fix(cli): restore bounded limits for one-shot execution ([#1429](https://github.com/everruns/bashkit/pull/1429)) by @chaliy
* fix(interpreter): prevent fd3 pending output leakage ([#1428](https://github.com/everruns/bashkit/pull/1428)) by @chaliy
* fix(python): support BuiltinResult for custom builtins ([#1427](https://github.com/everruns/bashkit/pull/1427)) by @oliverlambson
* fix(python): validate timeout_seconds before Duration conversion ([#1425](https://github.com/everruns/bashkit/pull/1425)) by @chaliy
* fix(cli): disable interactive rc sourcing by default ([#1424](https://github.com/everruns/bashkit/pull/1424)) by @chaliy
* fix(interpreter): scope proc_sub cleanup to session-owned paths ([#1423](https://github.com/everruns/bashkit/pull/1423)) by @chaliy
* fix(trace): avoid unicode slice panic in equals-form redaction ([#1422](https://github.com/everruns/bashkit/pull/1422)) by @chaliy
* fix(snapshot): verify keyed HMAC in constant time ([#1421](https://github.com/everruns/bashkit/pull/1421)) by @chaliy
* fix(expansion): preserve quoted literalness in pattern operands ([#1420](https://github.com/everruns/bashkit/pull/1420)) by @chaliy
* fix(realfs): block symlink target escapes via host symlinks ([#1419](https://github.com/everruns/bashkit/pull/1419)) by @chaliy
* fix(hooks): enforce input size before before_exec hooks ([#1418](https://github.com/everruns/bashkit/pull/1418)) by @chaliy
* fix(network): revalidate HTTP URL after before_http hook rewrites ([#1417](https://github.com/everruns/bashkit/pull/1417)) by @chaliy
* fix(scripted-tool): sanitize custom dry-run handler errors ([#1416](https://github.com/everruns/bashkit/pull/1416)) by @chaliy
* fix(python): scope and cap direct glob traversal ([#1415](https://github.com/everruns/bashkit/pull/1415)) by @chaliy
* fix(interpreter): avoid overflow panic in array slice length ([#1414](https://github.com/everruns/bashkit/pull/1414)) by @chaliy
* fix(glob): avoid unbounded bracket-range expansion ([#1413](https://github.com/everruns/bashkit/pull/1413)) by @chaliy
* fix(interpreter): enforce nested bash parser input and timeout limits ([#1412](https://github.com/everruns/bashkit/pull/1412)) by @chaliy
* fix(awk): enforce configured loop iteration limits ([#1411](https://github.com/everruns/bashkit/pull/1411)) by @chaliy
* fix(interpreter): enforce parser timeout/input limits in eval/source ([#1410](https://github.com/everruns/bashkit/pull/1410)) by @chaliy
* fix(date): guard quote stripping against lone-quote panic ([#1409](https://github.com/everruns/bashkit/pull/1409)) by @chaliy
* fix(builtins): bound nl line-number width ([#1408](https://github.com/everruns/bashkit/pull/1408)) by @chaliy
* fix(python): require explicit opt-in for in-process Python execution ([#1407](https://github.com/everruns/bashkit/pull/1407)) by @chaliy
* fix(sed): bound grouped command recursion depth ([#1406](https://github.com/everruns/bashkit/pull/1406)) by @chaliy
* fix(split): reject zero line and byte sizes ([#1405](https://github.com/everruns/bashkit/pull/1405)) by @chaliy
* fix(builtins): cap retry attempts in retry builtin ([#1404](https://github.com/everruns/bashkit/pull/1404)) by @chaliy
* fix(builtins): block unzip path traversal entries ([#1403](https://github.com/everruns/bashkit/pull/1403)) by @chaliy
* fix(interpreter): clean up background jobs after each exec ([#1402](https://github.com/everruns/bashkit/pull/1402)) by @chaliy
* fix(security): keep unwind panics in release profile ([#1401](https://github.com/everruns/bashkit/pull/1401)) by @chaliy
* fix(fs): enforce limits for pre-mounted overlay files ([#1400](https://github.com/everruns/bashkit/pull/1400)) by @chaliy
* fix(interpreter): prevent alpha brace step wrap to zero ([#1399](https://github.com/everruns/bashkit/pull/1399)) by @chaliy
* fix(interpreter): avoid panic when truncating utf8 output ([#1398](https://github.com/everruns/bashkit/pull/1398)) by @chaliy
* fix(interpreter): bound and filter captured final_env ([#1397](https://github.com/everruns/bashkit/pull/1397)) by @chaliy
* fix(parser): reject leading pipeline and list operators ([#1396](https://github.com/everruns/bashkit/pull/1396)) by @chaliy
* fix(parser): require exact heredoc delimiter match ([#1395](https://github.com/everruns/bashkit/pull/1395)) by @chaliy
* fix(limits): close loop and pipeline resource-limit bypasses ([#1394](https://github.com/everruns/bashkit/pull/1394)) by @chaliy
* fix(mcp): bound stdin request line size ([#1393](https://github.com/everruns/bashkit/pull/1393)) by @chaliy
* fix(sed): avoid Unicode panic in command splitting ([#1392](https://github.com/everruns/bashkit/pull/1392)) by @chaliy
* fix(interpreter): restore call-stack and counters after timeout cancellation ([#1391](https://github.com/everruns/bashkit/pull/1391)) by @chaliy
* fix(timeout): prevent panic on oversized duration values ([#1390](https://github.com/everruns/bashkit/pull/1390)) by @chaliy
* fix(parser): fail fast when top-level parser makes no progress ([#1389](https://github.com/everruns/bashkit/pull/1389)) by @chaliy
* fix(vfs): preserve custom filesystem limits when mounting text files ([#1388](https://github.com/everruns/bashkit/pull/1388)) by @chaliy
* fix(interpreter): prevent recursive ERR trap reentry ([#1387](https://github.com/everruns/bashkit/pull/1387)) by @chaliy
* fix(builtins): cap yes output to prevent memory exhaustion ([#1386](https://github.com/everruns/bashkit/pull/1386)) by @chaliy
* fix(builtins): cap dirstack size from shell vars ([#1385](https://github.com/everruns/bashkit/pull/1385)) by @chaliy
* fix(interpreter): avoid UTF-8 panic in extglob backtracking ([#1384](https://github.com/everruns/bashkit/pull/1384)) by @chaliy
* fix(interpreter): restore function pipeline stdin on error ([#1383](https://github.com/everruns/bashkit/pull/1383)) by @chaliy
* fix(interpreter): prevent xargs stdin glob/brace expansion ([#1382](https://github.com/everruns/bashkit/pull/1382)) by @chaliy
* fix(interpreter): preserve arithmetic depth for array indexes ([#1381](https://github.com/everruns/bashkit/pull/1381)) by @chaliy
* fix(interpreter): quote execution-plan args to prevent find -exec expansion ([#1380](https://github.com/everruns/bashkit/pull/1380)) by @chaliy
* fix(fs): avoid double-counting hidden lower children on recursive delete ([#1379](https://github.com/everruns/bashkit/pull/1379)) by @chaliy
* fix(python): disable `re` imports to mitigate regex DoS ([#1378](https://github.com/everruns/bashkit/pull/1378)) by @chaliy
* fix(builtins): reject invalid unexpand tab-stop values ([#1377](https://github.com/everruns/bashkit/pull/1377)) by @chaliy
* fix(interpreter): enforce memory limits for local assignments ([#1376](https://github.com/everruns/bashkit/pull/1376)) by @chaliy
* fix(grep): validate indexed search paths via vfs reads ([#1375](https://github.com/everruns/bashkit/pull/1375)) by @chaliy
* fix(interpreter): restore env after path-script subprocess ([#1374](https://github.com/everruns/bashkit/pull/1374)) by @chaliy
* fix(interpreter): prevent exec argv reparse injection ([#1373](https://github.com/everruns/bashkit/pull/1373)) by @chaliy
* fix(network): enforce HttpHandler timeout and response-size guardrails ([#1372](https://github.com/everruns/bashkit/pull/1372)) by @chaliy
* fix(numfmt): bound padding and printf precision ([#1371](https://github.com/everruns/bashkit/pull/1371)) by @chaliy
* fix(fs): enforce limits for lazy files ([#1370](https://github.com/everruns/bashkit/pull/1370)) by @chaliy
* fix(snapshot): enforce limit checks during snapshot restore ([#1369](https://github.com/everruns/bashkit/pull/1369)) by @chaliy
* fix(network): bind bot-auth signatures to method and target URI ([#1368](https://github.com/everruns/bashkit/pull/1368)) by @chaliy
* fix(examples): pin ticket-cli upstream commit in CI ([#1367](https://github.com/everruns/bashkit/pull/1367)) by @chaliy
* feat(site): render /docs from canonical docs/ markdown ([#1366](https://github.com/everruns/bashkit/pull/1366)) by @chaliy
* fix(sort): avoid UTF-8 panic in -k character slicing ([#1365](https://github.com/everruns/bashkit/pull/1365)) by @chaliy
* fix(sed): bound fancy-regex fallback backtracking ([#1364](https://github.com/everruns/bashkit/pull/1364)) by @chaliy
* fix(realfs): canonicalize mount paths before policy checks ([#1363](https://github.com/everruns/bashkit/pull/1363)) by @chaliy
* fix(scripted-tool): block DiscoverTool command injection via shell separators ([#1362](https://github.com/everruns/bashkit/pull/1362)) by @chaliy
* fix(ci): prevent Homebrew PAT exposure in cli-binaries workflow ([#1361](https://github.com/everruns/bashkit/pull/1361)) by @chaliy
* feat(site): refine homepage and add builtins index ([#1360](https://github.com/everruns/bashkit/pull/1360)) by @chaliy
* fix(ci): skip quota-exhausted harness example ([#1359](https://github.com/everruns/bashkit/pull/1359)) by @chaliy
* fix(snapshot): enforce parser and function limits on restore ([#1358](https://github.com/everruns/bashkit/pull/1358)) by @chaliy
* fix(fs): block RealFs symlink escape via non-existent path suffix ([#1357](https://github.com/everruns/bashkit/pull/1357)) by @chaliy
* fix(interpreter): bound allexport env growth by memory limits ([#1356](https://github.com/everruns/bashkit/pull/1356)) by @chaliy
* fix(interpreter): enforce array entry limits for split array assignments ([#1355](https://github.com/everruns/bashkit/pull/1355)) by @chaliy
* fix(ci): pin harness ref for OpenAI example and scope Doppler secret ([#1354](https://github.com/everruns/bashkit/pull/1354)) by @chaliy
* feat(python): publish Python 3.14 wheels to PyPI ([#1351](https://github.com/everruns/bashkit/pull/1351)) by @oliverlambson
* fix(examples): patch vulnerable npm dependencies ([#1347](https://github.com/everruns/bashkit/pull/1347)) by @chaliy
* fix(site): improve homepage proof and sourcing ([#1346](https://github.com/everruns/bashkit/pull/1346)) by @chaliy
* test(source): regression for sourced fn with fd3 block redirect + procsub while-read ([#1345](https://github.com/everruns/bashkit/pull/1345)) by @chaliy
* feat(awk): implement range patterns (/start/,/end/) ([#1344](https://github.com/everruns/bashkit/pull/1344)) by @chaliy
* fix(site): drop ASSETS binding for static-only worker ([#1342](https://github.com/everruns/bashkit/pull/1342)) by @chaliy
* fix(parser): preserve glob expansion inside process substitution ([#1341](https://github.com/everruns/bashkit/pull/1341)) by @chaliy
* fix(examples): fail harness smoke test correctly ([#1340](https://github.com/everruns/bashkit/pull/1340)) by @chaliy
* feat(site): add bashkit.sh homepage (Astro + Cloudflare) ([#1339](https://github.com/everruns/bashkit/pull/1339)) by @chaliy
* fix(interpreter): keep repl alive after child exit ([#1338](https://github.com/everruns/bashkit/pull/1338)) by @chaliy
* fix(ci): retry npm publish verification by @chaliy
* feat(python): Jupyter compatibility for async custom_builtins + notebook example by @chaliy

**Full Changelog**: https://github.com/everruns/bashkit/commits/v0.1.21

## [0.1.20] - 2026-04-17

### Highlights

- **Snapshot fidelity** - Added shell-only snapshots, preserved shell functions in snapshots, and stored function source alongside AST for restore parity
- **Bindings parity** - Added Python `shell_state`, `custom_builtins`, `clear_cancel()`, and streaming callback support plus JS `clearCancel` and BashTool snapshot support
- **Bindings hardening** - Rejected same-instance callback re-entry and expanded cancel-state and FastAPI regression coverage
- **CI and release polish** - Added coverage uploads and Codecov config, removed duplicate example builds, and hardened crates publish verification
- **External contributions** - Thanks to @oliverlambson and @shubhlohiya for shipping Python and JS binding improvements in this release

### What's Changed

* fix(snapshot): store function source alongside ast ([#1332](https://github.com/everruns/bashkit/pull/1332)) by @chaliy
* fix(bindings): reject same-instance callback re-entry ([#1331](https://github.com/everruns/bashkit/pull/1331)) by @chaliy
* test(bindings): cover async cancel state recovery ([#1330](https://github.com/everruns/bashkit/pull/1330)) by @chaliy
* test(python): cover fastapi custom builtins ([#1329](https://github.com/everruns/bashkit/pull/1329)) by @chaliy
* fix(snapshot): preserve functions in shell snapshots ([#1328](https://github.com/everruns/bashkit/pull/1328)) by @chaliy
* feat(snapshot): add shell-only snapshots ([#1327](https://github.com/everruns/bashkit/pull/1327)) by @chaliy
* feat(python): expose shell_state on Bash and BashTool ([#1326](https://github.com/everruns/bashkit/pull/1326)) by @oliverlambson
* chore(ci): add codecov config ([#1324](https://github.com/everruns/bashkit/pull/1324)) by @chaliy
* docs(specs): remove numeric prefixes from spec names ([#1323](https://github.com/everruns/bashkit/pull/1323)) by @chaliy
* test(python): cover in-flight clear_cancel recovery ([#1322](https://github.com/everruns/bashkit/pull/1322)) by @chaliy
* feat(js): add clearCancel parity ([#1321](https://github.com/everruns/bashkit/pull/1321)) by @chaliy
* feat(ci): add binding coverage uploads ([#1318](https://github.com/everruns/bashkit/pull/1318)) by @chaliy
* fix(ci): split codecov upload job ([#1317](https://github.com/everruns/bashkit/pull/1317)) by @chaliy
* fix(ci): avoid duplicate example builds in test job ([#1316](https://github.com/everruns/bashkit/pull/1316)) by @chaliy
* feat(python): add custom_builtins to Bash and BashTool ([#1315](https://github.com/everruns/bashkit/pull/1315)) by @oliverlambson
* feat(python): add clear_cancel() to Bash and BashTool ([#1314](https://github.com/everruns/bashkit/pull/1314)) by @shubhlohiya
* fix(ci): harden crates publish verification ([#1313](https://github.com/everruns/bashkit/pull/1313)) by @chaliy
* feat(javascript): add BashTool snapshot support ([#1310](https://github.com/everruns/bashkit/pull/1310)) by @chaliy
* feat(node): expose streaming output callbacks ([#1309](https://github.com/everruns/bashkit/pull/1309)) by @oliverlambson
* feat(python): expose streaming output callbacks ([#1308](https://github.com/everruns/bashkit/pull/1308)) by @oliverlambson

**Full Changelog**: https://github.com/everruns/bashkit/commits/v0.1.20

## [0.1.19] - 2026-04-15

### Highlights

- **Scripted tool ergonomics** — New `ToolImpl` composition plus `--help`, `--dry-run`, and structured discover schema for MCP-backed callbacks
- **Python bindings** — Added direct VFS helpers, callable file providers, snapshot restore, stronger parity coverage, and security regression tests
- **Targeted fixes** — Correct `touch` mtimes for existing paths, quoted-adjacent glob expansion, Python publish stripping for multi-feature defaults, and `rustls-webpki` audit advisories
- **Docs and CI polish** — New snapshotting guide, richer Python/Node examples, aligned package docs, and JS type-check coverage in CI
- **External contribution** — Python snapshot restore support landed via @oliverlambson in [#1298](https://github.com/everruns/bashkit/pull/1298)

### What's Changed

* docs(readme): align Python and Node package guides ([#1307](https://github.com/everruns/bashkit/pull/1307)) by @chaliy
* test(python): tag security tests with threat-model ids ([#1306](https://github.com/everruns/bashkit/pull/1306)) by @chaliy
* test(python): add parity suites for builtins, strings, and scripts ([#1305](https://github.com/everruns/bashkit/pull/1305)) by @chaliy
* refactor(python): split binding tests by category ([#1304](https://github.com/everruns/bashkit/pull/1304)) by @chaliy
* fix(python): cover issue 1264 security gaps ([#1303](https://github.com/everruns/bashkit/pull/1303)) by @chaliy
* docs: add public snapshotting guide ([#1302](https://github.com/everruns/bashkit/pull/1302)) by @chaliy
* test(node): add missing security coverage ([#1300](https://github.com/everruns/bashkit/pull/1300)) by @chaliy
* test(node): add integration workflow coverage ([#1299](https://github.com/everruns/bashkit/pull/1299)) by @chaliy
* feat(python): add snapshot restore support ([#1298](https://github.com/everruns/bashkit/pull/1298)) by @oliverlambson
* feat(python): support callable file providers ([#1297](https://github.com/everruns/bashkit/pull/1297)) by @chaliy
* feat(python): add direct VFS convenience methods ([#1295](https://github.com/everruns/bashkit/pull/1295)) by @chaliy
* fix(touch): update mtimes for existing paths ([#1294](https://github.com/everruns/bashkit/pull/1294)) by @chaliy
* feat(scripted-tool): add --dry-run flag with pluggable validation ([#1293](https://github.com/everruns/bashkit/pull/1293)) by @chaliy
* feat(scripting-toolset): structured discover input schema for MCP ([#1292](https://github.com/everruns/bashkit/pull/1292)) by @chaliy
* docs(python): add @example blocks to type stubs and modules ([#1291](https://github.com/everruns/bashkit/pull/1291)) by @chaliy
* docs: add missing examples to Python and Node bindings ([#1290](https://github.com/everruns/bashkit/pull/1290)) by @chaliy
* ci(node): add TypeScript type-check job to JS workflow ([#1289](https://github.com/everruns/bashkit/pull/1289)) by @chaliy
* feat(scripted-tool): add --help flag to tool callbacks ([#1288](https://github.com/everruns/bashkit/pull/1288)) by @chaliy
* fix(glob): expand glob * adjacent to quoted variable expansion ([#1287](https://github.com/everruns/bashkit/pull/1287)) by @chaliy
* fix(security): resolve 6 CodeQL alerts in test code ([#1286](https://github.com/everruns/bashkit/pull/1286)) by @chaliy
* fix(ci): handle multi-feature default array in python stripping ([#1285](https://github.com/everruns/bashkit/pull/1285)) by @chaliy
* feat(scripted_tool): add ToolImpl combining ToolDef + sync/async exec ([#1284](https://github.com/everruns/bashkit/pull/1284)) by @chaliy
* feat(credential): generic credential injection for outbound HTTP requests ([#1282](https://github.com/everruns/bashkit/pull/1282)) by @chaliy

**Full Changelog**: https://github.com/everruns/bashkit/commits/v0.1.19

## [0.1.18] - 2026-04-14

### Highlights

- **Hooks system** — New interceptor hooks with tool-level pipeline integration and HTTP hook support
- **Interactive shell** — Full REPL mode with rustyline line editing, tab completion, and streaming output
- **Security hardening** — SSRF prevention, MCP rate limiting, template injection fixes, secret redaction, and host key verification
- **Expansion fixes** — Correct `${@/#/prefix}` per positional param, mixed literal+quoted `${var#pattern}`, and backreferences in sed
- **Async Python callbacks** — ScriptedTool now supports async Python callbacks with ContextVar propagation

### What's Changed

* chore: pre-release maintenance pass (2026-04-14) ([#1280](https://github.com/everruns/bashkit/pull/1280)) by @chaliy
* chore(bench): add 2026-04-13 benchmark results ([#1276](https://github.com/everruns/bashkit/pull/1276)) by @chaliy
* docs(hooks): add public hooks guide with examples ([#1275](https://github.com/everruns/bashkit/pull/1275)) by @chaliy
* docs: add contributing section to README and emphasize issues in CONTRIBUTING.md ([#1274](https://github.com/everruns/bashkit/pull/1274)) by @chaliy
* feat(cli): add cargo-binstall metadata ([#1273](https://github.com/everruns/bashkit/pull/1273)) by @chaliy
* feat(scripted_tool): async Python callbacks + ContextVar propagation ([#1272](https://github.com/everruns/bashkit/pull/1272)) by @chaliy
* fix(bench): enable jq feature and fix expected outputs for jq bench cases ([#1271](https://github.com/everruns/bashkit/pull/1271)) by @chaliy
* chore(specs): simplify specs — remove duplication, trim stale content ([#1270](https://github.com/everruns/bashkit/pull/1270)) by @chaliy
* feat(bench): add gbash and gbash-server benchmark runners ([#1269](https://github.com/everruns/bashkit/pull/1269)) by @chaliy
* feat(hooks): wire tool hooks into builtin pipeline, add HTTP hooks ([#1255](https://github.com/everruns/bashkit/pull/1255)) by @chaliy
* feat(python): bump monty to 0.0.11, add datetime/json support ([#1254](https://github.com/everruns/bashkit/pull/1254)) by @chaliy
* feat(hooks): implement interceptor hooks system ([#1253](https://github.com/everruns/bashkit/pull/1253)) by @chaliy
* fix(mount): add path validation, allowlist, and writable warnings ([#1252](https://github.com/everruns/bashkit/pull/1252)) by @chaliy
* fix(ln): allow symlinks in ReadWrite RealFs mounts ([#1251](https://github.com/everruns/bashkit/pull/1251)) by @chaliy
* fix(expansion): handle mixed literal+quoted var in ${var#pattern} ([#1250](https://github.com/everruns/bashkit/pull/1250)) by @chaliy
* chore(deps): bump the rust-dependencies group with 2 updates ([#1249](https://github.com/everruns/bashkit/pull/1249)) by @dependabot
* chore(ci): bump softprops/action-gh-release from 2 to 3 in the github-actions group ([#1248](https://github.com/everruns/bashkit/pull/1248)) by @dependabot
* fix(expansion): apply ${@/#/prefix} per positional param ([#1247](https://github.com/everruns/bashkit/pull/1247)) by @chaliy
* fix(sed): support backreferences in search patterns ([#1246](https://github.com/everruns/bashkit/pull/1246)) by @chaliy
* fix(bashkit-js): bump langsmith 0.5.16 → 0.5.18 ([#1244](https://github.com/everruns/bashkit/pull/1244)) by @chaliy
* fix(mcp): add request rate limiting for MCP tool calls ([#1243](https://github.com/everruns/bashkit/pull/1243)) by @chaliy
* fix(snapshot): add keyed HMAC API and document forgery limitation ([#1242](https://github.com/everruns/bashkit/pull/1242)) by @chaliy
* fix(interpreter): suppress DEBUG trap inside trap handlers ([#1241](https://github.com/everruns/bashkit/pull/1241)) by @chaliy
* fix(template): prevent injection via #each data values ([#1240](https://github.com/everruns/bashkit/pull/1240)) by @chaliy
* fix(tool): sanitize ScriptedTool callback errors ([#1239](https://github.com/everruns/bashkit/pull/1239)) by @chaliy
* fix(trace): extend redaction to common CLI secret flags ([#1238](https://github.com/everruns/bashkit/pull/1238)) by @chaliy
* fix(cli): emit warning when --mount-rw is used in MCP mode ([#1237](https://github.com/everruns/bashkit/pull/1237)) by @chaliy
* feat: add hooks system with on_exit interceptor for interactive mode ([#1236](https://github.com/everruns/bashkit/pull/1236)) by @chaliy
* fix(date): resolve relative paths in date -r against CWD ([#1234](https://github.com/everruns/bashkit/pull/1234)) by @chaliy
* fix(network): block private IPs in allowlist check (SSRF) ([#1233](https://github.com/everruns/bashkit/pull/1233)) by @chaliy
* fix(interpreter): re-validate budget after alias expansion ([#1232](https://github.com/everruns/bashkit/pull/1232)) by @chaliy
* fix(ai): add output sanitization and length limiting to AI integrations ([#1231](https://github.com/everruns/bashkit/pull/1231)) by @chaliy
* feat(builtins): add --help and --version support to all tools ([#1230](https://github.com/everruns/bashkit/pull/1230)) by @chaliy
* fix(python): add mutex timeout to prevent execute_sync deadlock ([#1229](https://github.com/everruns/bashkit/pull/1229)) by @chaliy
* fix(ssh): add host key verification to SSH client ([#1227](https://github.com/everruns/bashkit/pull/1227)) by @chaliy
* fix(interactive): flush stdout/stderr after streaming command output ([#1226](https://github.com/everruns/bashkit/pull/1226)) by @chaliy
* fix(interactive): avoid nested tokio runtime panic in tab completion ([#1224](https://github.com/everruns/bashkit/pull/1224)) by @chaliy
* refactor(deps): simplify dependency tree ([#1223](https://github.com/everruns/bashkit/pull/1223)) by @chaliy
* fix(interpreter): seed $RANDOM PRNG per-instance ([#1222](https://github.com/everruns/bashkit/pull/1222)) by @chaliy
* fix(interpreter): clean up process substitution temp files ([#1221](https://github.com/everruns/bashkit/pull/1221)) by @chaliy
* fix(mcp): sanitize JSON-RPC error responses ([#1220](https://github.com/everruns/bashkit/pull/1220)) by @chaliy
* fix(logging): add runtime guard for unsafe logging methods ([#1219](https://github.com/everruns/bashkit/pull/1219)) by @chaliy
* fix(interpreter): filter SHOPT_ variables from set/declare output ([#1218](https://github.com/everruns/bashkit/pull/1218)) by @chaliy
* fix(vfs): emit warnings when tar extraction skips unsupported entry types ([#1217](https://github.com/everruns/bashkit/pull/1217)) by @chaliy
* fix(limits): treat zero limit values as "use default" ([#1216](https://github.com/everruns/bashkit/pull/1216)) by @chaliy
* feat(cli): interactive shell mode with rustyline ([#1215](https://github.com/everruns/bashkit/pull/1215)) by @chaliy
* fix(interpreter): filter additional internal variables from declare -p and set ([#1212](https://github.com/everruns/bashkit/pull/1212)) by @chaliy
* fix(date): preserve spaces in format string from variable expansion ([#1211](https://github.com/everruns/bashkit/pull/1211)) by @chaliy
* fix(git): sanitize control characters in git output ([#1210](https://github.com/everruns/bashkit/pull/1210)) by @chaliy
* fix(integrations): propagate framework timeout to bashkit execution limits ([#1207](https://github.com/everruns/bashkit/pull/1207)) by @chaliy

**Full Changelog**: https://github.com/everruns/bashkit/commits/v0.1.18

## [0.1.17] - 2026-04-08

### Highlights

- **Expanded fuzz testing** — 10 new fuzz targets (tomlq, archive, csv, grep, template, yaml, sed, envsubst, base64, printf) for stronger security coverage
- **Redirect fixes** — Correct fd3 redirection routing and stderr suppression from builtins
- **Bug fixes** — VFS path resolution with `./` prefix, `date -r` flag, `tar -C`, `command -v` PATH search, and shopt preservation across `exec()`

### What's Changed

* feat(fuzz): add tomlq_fuzz target ([#1151](https://github.com/everruns/bashkit/pull/1151)) by @chaliy
* feat(fuzz): add archive_fuzz target ([#1150](https://github.com/everruns/bashkit/pull/1150)) by @chaliy
* feat(fuzz): add csv_fuzz target ([#1149](https://github.com/everruns/bashkit/pull/1149)) by @chaliy
* feat(fuzz): add grep_fuzz target for ReDoS prevention ([#1148](https://github.com/everruns/bashkit/pull/1148)) by @chaliy
* feat(fuzz): add template_fuzz target ([#1147](https://github.com/everruns/bashkit/pull/1147)) by @chaliy
* feat(fuzz): add yaml_fuzz target ([#1146](https://github.com/everruns/bashkit/pull/1146)) by @chaliy
* feat(fuzz): add sed_fuzz target ([#1145](https://github.com/everruns/bashkit/pull/1145)) by @chaliy
* feat(fuzz): add envsubst_fuzz target ([#1144](https://github.com/everruns/bashkit/pull/1144)) by @chaliy
* feat(fuzz): add base64_fuzz target ([#1143](https://github.com/everruns/bashkit/pull/1143)) by @chaliy
* fix(vfs): handle ./ prefix in path resolution ([#1142](https://github.com/everruns/bashkit/pull/1142)) by @chaliy
* fix(date): implement -r flag for file modification time ([#1141](https://github.com/everruns/bashkit/pull/1141)) by @chaliy
* feat(fuzz): add printf_fuzz target ([#1140](https://github.com/everruns/bashkit/pull/1140)) by @chaliy
* fix(redirect): fd3 redirection pattern 3>&1 >file now routes correctly ([#1139](https://github.com/everruns/bashkit/pull/1139)) by @chaliy
* fix(redirect): suppress stderr from builtins with 2>/dev/null ([#1138](https://github.com/everruns/bashkit/pull/1138)) by @chaliy
* feat(iconv): support //translit transliteration mode ([#1136](https://github.com/everruns/bashkit/pull/1136)) by @chaliy
* test(redirect): add append redirect spec tests ([#1137](https://github.com/everruns/bashkit/pull/1137)) by @chaliy
* fix(tar): pass -C directory to create_tar for VFS file resolution ([#1135](https://github.com/everruns/bashkit/pull/1135)) by @chaliy
* fix(builtins): command -v/-V now searches PATH for external scripts ([#1134](https://github.com/everruns/bashkit/pull/1134)) by @chaliy
* feat(js): expose mounts option, mountReal, and unmount on wrapper ([#1133](https://github.com/everruns/bashkit/pull/1133)) by @chaliy
* feat(js): readDir returns entries with metadata (Python parity) ([#1132](https://github.com/everruns/bashkit/pull/1132)) by @chaliy
* fix(interpreter): preserve shopt options across exec() calls ([#1131](https://github.com/everruns/bashkit/pull/1131)) by @chaliy
* fix(ci): strip python feature from all workspace crates before publish ([#1127](https://github.com/everruns/bashkit/pull/1127)) by @chaliy
* fix(ci): fix crates.io publish + add verification ([#1126](https://github.com/everruns/bashkit/pull/1126)) by @chaliy

**Full Changelog**: https://github.com/everruns/bashkit/commits/v0.1.17

## [0.1.16] - 2026-04-06

### Highlights

- **npm publish fix** — Stable releases now correctly tagged as `latest` on npm (was stuck at 0.1.10 since v0.1.11)
- **OpenAI Responses API migration** — Examples updated from deprecated Chat Completions function calling to the new Responses API

### What's Changed

* fix(ci): pass --ref tag to publish workflow dispatches ([#1124](https://github.com/everruns/bashkit/pull/1124)) by @chaliy
* fix(examples): migrate OpenAI examples to Responses API ([#1122](https://github.com/everruns/bashkit/pull/1122)) by @chaliy
* fix(ci): commit Cargo.lock for reproducible builds ([#1123](https://github.com/everruns/bashkit/pull/1123)) by @chaliy
* fix(ci): update python3-dll-a cargo-vet exemption to 0.2.15 ([#1121](https://github.com/everruns/bashkit/pull/1121)) by @chaliy
* chore(deps): bump rand 0.8→0.10 and russh 0.52→0.60 by @dependabot[bot]
* feat(fuzz): add awk_fuzz target for awk builtin ([#1112](https://github.com/everruns/bashkit/pull/1112)) by @chaliy
* feat(fuzz): add jq_fuzz target for jq builtin ([#1111](https://github.com/everruns/bashkit/pull/1111)) by @chaliy
* fix(interpreter): prevent byte range panic in ${#arr[idx]} with malformed input ([#1110](https://github.com/everruns/bashkit/pull/1110)) by @chaliy
* fix(interpreter): Box::pin expand_word to prevent stack overflow in nested $() ([#1109](https://github.com/everruns/bashkit/pull/1109)) by @chaliy
* fix(interpreter): add max_subst_depth limit to prevent OOM from nested $() ([#1107](https://github.com/everruns/bashkit/pull/1107)) by @chaliy

## [0.1.15] - 2026-04-06

### Highlights

- **Transparent request signing (bot-auth)** — Ed25519 request signing per RFC 9421 for all outbound HTTP requests, configured via `BotAuthConfig`
- **Opt-in SSH/SCP/SFTP builtins** — Pluggable `SshHandler` trait with russh transport, host allowlists (default-deny), and session pooling
- **Opt-in TypeScript via ZapCode** — Embedded TS/JS runtime with `ts`, `node`, `deno`, `bun` builtins, VFS bridging, and configurable resource limits
- **AI SDK adapters** — First-class JS adapters for Vercel AI SDK, OpenAI SDK, and Anthropic SDK with zero-boilerplate tool integration
- **Snapshot/resume** — Serialize and restore interpreter state mid-execution for checkpointing and migration
- **wedow/harness compatibility** — Running the wedow/harness agent framework via bashkit as another bash compatibility milestone
- **Security hardening** — 20+ fixes: regex size limits, memory exhaustion caps, sandbox escape fix, credential leak prevention, header injection mitigation

### What's Changed

* chore(specs): make CI health a hard gate in maintenance checklist ([#1092](https://github.com/everruns/bashkit/pull/1092)) by @chaliy
* feat(examples): run wedow/harness via bashkit with OpenAI ([#1086](https://github.com/everruns/bashkit/pull/1086)) by @chaliy
* fix(interpreter): populate BASH_SOURCE[0] for PATH-resolved scripts ([#1087](https://github.com/everruns/bashkit/pull/1087)) by @chaliy
* feat(js): expose stat() and missing fs operations directly on Bash/BashTool ([#1084](https://github.com/everruns/bashkit/pull/1084)) by @chaliy
* feat(js): expose fs() accessor for direct VFS operations ([#1081](https://github.com/everruns/bashkit/pull/1081)) by @chaliy
* fix(parser): prevent word-splitting inside quoted strings during array assignment ([#1082](https://github.com/everruns/bashkit/pull/1082)) by @chaliy
* feat(builtins): add ls -C multi-column output ([#1079](https://github.com/everruns/bashkit/pull/1079)) by @chaliy
* feat(js): expose additional execution limits for Python parity ([#1078](https://github.com/everruns/bashkit/pull/1078)) by @chaliy
* fix(grep): grep -r on single file returns empty ([#1080](https://github.com/everruns/bashkit/pull/1080)) by @chaliy
* feat(js): expose real filesystem mounts with per-mount readOnly support ([#1077](https://github.com/everruns/bashkit/pull/1077)) by @chaliy
* feat: expose maxMemory to prevent OOM from untrusted input ([#1075](https://github.com/everruns/bashkit/pull/1075)) by @chaliy
* feat(cli): relax execution limits for CLI mode ([#1076](https://github.com/everruns/bashkit/pull/1076)) by @chaliy
* fix(parser): handle all token types in process substitution reconstruction ([#1073](https://github.com/everruns/bashkit/pull/1073)) by @chaliy
* feat(ssh): add ssh/scp/sftp builtins with russh transport ([#945](https://github.com/everruns/bashkit/pull/945)) by @chaliy
* fix(deps): resolve all npm security vulnerabilities ([#1064](https://github.com/everruns/bashkit/pull/1064)) by @chaliy
* docs: add GitHub links to PyPI metadata and Everruns ecosystem section ([#1065](https://github.com/everruns/bashkit/pull/1065)) by @chaliy
* chore: pre-release maintenance pass ([#1063](https://github.com/everruns/bashkit/pull/1063)) by @chaliy
* feat(network): add transparent request signing (bot-auth) ([#1062](https://github.com/everruns/bashkit/pull/1062)) by @chaliy
* fix(audit): update semver exemption to 1.0.28 ([#1059](https://github.com/everruns/bashkit/pull/1059)) by @chaliy
* fix(builtins): limit AWK getline file cache to prevent memory exhaustion ([#1061](https://github.com/everruns/bashkit/pull/1061)) by @chaliy
* fix(builtins): cap AWK printf width/precision to prevent memory exhaustion ([#1048](https://github.com/everruns/bashkit/pull/1048)) by @chaliy
* fix(interpreter): support exec {var}>&- fd-variable redirect syntax ([#1060](https://github.com/everruns/bashkit/pull/1060)) by @chaliy
* fix(builtins): cap AWK output buffer size to prevent memory exhaustion ([#1055](https://github.com/everruns/bashkit/pull/1055)) by @chaliy
* fix(builtins): cap parallel cartesian product size to prevent memory blowup ([#1054](https://github.com/everruns/bashkit/pull/1054)) by @chaliy
* fix(builtins): sanitize curl multipart field names to prevent header injection ([#1053](https://github.com/everruns/bashkit/pull/1053)) by @chaliy
* fix(interpreter): splat "${arr[@]}" elements individually in array assignment ([#1052](https://github.com/everruns/bashkit/pull/1052)) by @chaliy
* fix(builtins): reject path traversal in patch diff headers ([#1051](https://github.com/everruns/bashkit/pull/1051)) by @chaliy
* fix(js): use single interpreter instance in AI adapters ([#1050](https://github.com/everruns/bashkit/pull/1050)) by @chaliy
* fix(builtins): enforce regex size limits in sed, grep, and awk ([#1049](https://github.com/everruns/bashkit/pull/1049)) by @chaliy
* fix(js): use shared runtime and concurrency limit for tool callbacks ([#1047](https://github.com/everruns/bashkit/pull/1047)) by @chaliy
* fix(python): enforce recursion depth limits in monty_to_py and py_to_monty ([#1046](https://github.com/everruns/bashkit/pull/1046)) by @chaliy
* fix(builtins): parse combined short flags in paste builtin ([#1045](https://github.com/everruns/bashkit/pull/1045)) by @chaliy
* fix(js): use SeqCst ordering for cancellation flag ([#1044](https://github.com/everruns/bashkit/pull/1044)) by @chaliy
* fix(interpreter): support recursive function calls inside $() command substitution ([#1043](https://github.com/everruns/bashkit/pull/1043)) by @chaliy
* chore: update semver exemption to 1.0.28 in cargo-vet config ([#1058](https://github.com/everruns/bashkit/pull/1058)) by @chaliy
* chore: update cc exemption to 1.2.59 in cargo-vet config ([#1057](https://github.com/everruns/bashkit/pull/1057)) by @chaliy
* fix(mcp): apply CLI execution limits to MCP-created interpreters ([#1041](https://github.com/everruns/bashkit/pull/1041)) by @chaliy
* fix(interpreter): remove exported vars from env on unset ([#1042](https://github.com/everruns/bashkit/pull/1042)) by @chaliy
* fix(fs): prevent sandbox escape via TOCTOU fallback in RealFs::resolve ([#1040](https://github.com/everruns/bashkit/pull/1040)) by @chaliy
* fix(interpreter): expand parameter operators inside arithmetic base# expressions ([#1039](https://github.com/everruns/bashkit/pull/1039)) by @chaliy
* fix(interpreter): set BASH_SOURCE[0] when running bash /path/script.sh ([#1037](https://github.com/everruns/bashkit/pull/1037)) by @chaliy
* fix(interpreter): short-circuit && and || inside [[ ]] for set -u ([#1035](https://github.com/everruns/bashkit/pull/1035)) by @chaliy
* test(interpreter): add regression tests for bash -c exported variable visibility ([#1038](https://github.com/everruns/bashkit/pull/1038)) by @chaliy
* fix(interpreter): forward piped stdin to bash script/command child ([#1036](https://github.com/everruns/bashkit/pull/1036)) by @chaliy
* fix(interpreter): route exec fd redirects through VFS targets ([#1034](https://github.com/everruns/bashkit/pull/1034)) by @chaliy
* fix(interpreter): compose indirect expansion with default operator by @chaliy
* chore: update tagline to "Awesomely fast virtual sandbox with bash and file system" ([#1029](https://github.com/everruns/bashkit/pull/1029)) by @chaliy
* fix(interpreter): contain ${var:?msg} error within subshell boundary ([#1031](https://github.com/everruns/bashkit/pull/1031)) by @chaliy
* fix(interpreter): exec < file redirects stdin for subsequent commands ([#1030](https://github.com/everruns/bashkit/pull/1030)) by @chaliy
* fix(builtins): unescape \/ in sed replacement strings ([#1028](https://github.com/everruns/bashkit/pull/1028)) by @chaliy
* fix(builtins): filter internal markers from Python os.environ ([#1021](https://github.com/everruns/bashkit/pull/1021)) by @chaliy
* fix(builtins): harden curl redirect against credential leaks ([#1020](https://github.com/everruns/bashkit/pull/1020)) by @chaliy
* fix(parser): cap lookahead in looks_like_brace_expansion ([#1019](https://github.com/everruns/bashkit/pull/1019)) by @chaliy
* fix(parser): enforce subst depth limit in unquoted cmdsub ([#1018](https://github.com/everruns/bashkit/pull/1018)) by @chaliy
* fix(interpreter): cap global pattern replacement result size ([#1017](https://github.com/everruns/bashkit/pull/1017)) by @chaliy
* fix(interpreter): cap glob_match calls in remove_pattern_glob ([#1016](https://github.com/everruns/bashkit/pull/1016)) by @chaliy
* fix(interpreter): save/restore memory_budget in subshell/cmdsub ([#1015](https://github.com/everruns/bashkit/pull/1015)) by @chaliy
* fix(fs): handle symlinks in overlay rename and copy ([#1014](https://github.com/everruns/bashkit/pull/1014)) by @chaliy
* fix(builtins): block unset of internal variables and readonly marker bypass ([#1013](https://github.com/everruns/bashkit/pull/1013)) by @chaliy
* fix(builtins): emit stderr warning when sed branch loop limit is reached ([#1012](https://github.com/everruns/bashkit/pull/1012)) by @chaliy
* fix(cli): install custom panic hook to suppress backtrace information disclosure ([#1011](https://github.com/everruns/bashkit/pull/1011)) by @chaliy
* fix(builtins): clamp printf precision to prevent panic on large values ([#1010](https://github.com/everruns/bashkit/pull/1010)) by @chaliy
* fix(trace): handle all header flag formats and missing secret headers in redaction ([#1009](https://github.com/everruns/bashkit/pull/1009)) by @chaliy
* fix(builtins): URL-encode query params and form body in HTTP builtin ([#1008](https://github.com/everruns/bashkit/pull/1008)) by @chaliy
* fix(builtins): prevent JSON injection in HTTP build_json_body ([#1007](https://github.com/everruns/bashkit/pull/1007)) by @chaliy
* fix(builtins): clear variable on read at EOF with no remaining data ([#976](https://github.com/everruns/bashkit/pull/976)) by @chaliy
* fix(builtins): honor jq -j/--join-output flag to suppress trailing newline ([#975](https://github.com/everruns/bashkit/pull/975)) by @chaliy
* fix(builtins): add find -path predicate and fix -not argument consumption ([#974](https://github.com/everruns/bashkit/pull/974)) by @chaliy
* fix(builtins): support long options in tree builtin ([#973](https://github.com/everruns/bashkit/pull/973)) by @chaliy
* fix(parser): treat escaped dollar \\$ in double quotes as literal ([#972](https://github.com/everruns/bashkit/pull/972)) by @chaliy
* fix(builtins): produce empty JSON string for jq -Rs with empty stdin ([#971](https://github.com/everruns/bashkit/pull/971)) by @chaliy
* fix(parser): reconstruct braces in process substitution token loop ([#970](https://github.com/everruns/bashkit/pull/970)) by @chaliy
* feat(js): Vercel AI SDK adapter — first-class integration ([#958](https://github.com/everruns/bashkit/pull/958)) by @chaliy
* feat(js): OpenAI SDK adapter — first-class GPT integration ([#957](https://github.com/everruns/bashkit/pull/957)) by @chaliy
* feat(js): Anthropic SDK adapter — first-class Claude integration ([#956](https://github.com/everruns/bashkit/pull/956)) by @chaliy
* docs: fix rustdoc guides rendering on docs.rs ([#955](https://github.com/everruns/bashkit/pull/955)) by @chaliy
* feat: snapshot/resume — serialize interpreter state mid-execution ([#954](https://github.com/everruns/bashkit/pull/954)) by @chaliy
* feat(builtins): add embedded TypeScript/JS runtime via ZapCode ([#940](https://github.com/everruns/bashkit/pull/940)) by @chaliy
* test(security): adversarial tests — sparse arrays, extreme indices, expansion bombs ([#936](https://github.com/everruns/bashkit/pull/936)) by @chaliy
* docs: update README features to reflect current implementation ([#935](https://github.com/everruns/bashkit/pull/935)) by @chaliy
* feat(builtins): support `-d @-` and `-d @file` in curl builtin ([#929](https://github.com/everruns/bashkit/pull/929)) by @chaliy
* chore(supply-chain): update exemptions for hybrid-array, hyper ([#927](https://github.com/everruns/bashkit/pull/927)) by @chaliy
* test: implement missing glob_fuzz target ([#926](https://github.com/everruns/bashkit/pull/926)) by @chaliy
* test(builtins): add spec tests for jq --arg/--argjson ([#925](https://github.com/everruns/bashkit/pull/925)) by @chaliy
* feat(builtins): implement ls -F (classify) option ([#924](https://github.com/everruns/bashkit/pull/924)) by @chaliy
* feat(vfs): lazy file content loading for InMemoryFs ([#923](https://github.com/everruns/bashkit/pull/923)) by @chaliy
* feat(builtins): add numfmt builtin ([#922](https://github.com/everruns/bashkit/pull/922)) by @chaliy
* feat(network): custom HTTP handler / fetch interception callback ([#921](https://github.com/everruns/bashkit/pull/921)) by @chaliy
* feat(builtins): full sort -k KEYDEF parsing with multi-key support ([#920](https://github.com/everruns/bashkit/pull/920)) by @chaliy
* fix(security): sanitize internal state in error messages ([#919](https://github.com/everruns/bashkit/pull/919)) by @chaliy
* feat(builtins): implement sort -V version sort ([#918](https://github.com/everruns/bashkit/pull/918)) by @chaliy
* fix(interpreter): isolate command substitution subshell state ([#917](https://github.com/everruns/bashkit/pull/917)) by @chaliy
* fix(interpreter): handle ++/-- in complex arithmetic expressions (#916) by @chaliy
* fix(interpreter): preserve stdout from if/elif condition commands ([#905](https://github.com/everruns/bashkit/pull/905)) by @chaliy
* fix(interpreter): exit builtin terminates execution in compound commands ([#904](https://github.com/everruns/bashkit/pull/904)) by @chaliy
* fix(interpreter): get_ifs_separator respects local IFS ([#902](https://github.com/everruns/bashkit/pull/902)) by @chaliy
* fix(builtins): read builtin respects local variable scoping ([#901](https://github.com/everruns/bashkit/pull/901)) by @chaliy
* chore(ci): bump the github-actions group with 2 updates ([#899](https://github.com/everruns/bashkit/pull/899)) by @chaliy
* refactor(builtins): migrate base64 from manual arg parsing to ArgParser ([#890](https://github.com/everruns/bashkit/pull/890)) by @chaliy
* fix(interpreter): expand command substitutions in assoc array keys ([#883](https://github.com/everruns/bashkit/pull/883)) by @chaliy

**Full Changelog**: https://github.com/everruns/bashkit/compare/v0.1.14...v0.1.15

## [0.1.14] - 2026-03-28

### Highlights

- **Massive Bash compatibility push** — 25+ interpreter fixes covering errexit, namerefs, associative arrays, arithmetic expansion, redirects, glob patterns, and ANSI-C quoting
- **AWK engine hardened** — 8 fixes for regex literals, newline handling, printf, keyword tokenization, and multi-file FILENAME support
- **New Bash features** — `set -a` (allexport), `BASH_SOURCE` array, `exec` with command replacement, `declare -f`, `compgen -c` PATH scanning
- **Prebuilt CLI binaries** — macOS (ARM64/x86_64) and Linux x86_64 binaries now published to GitHub Releases with Homebrew formula
- **Dependency upgrades** — jaq 3.0, digest crates 0.11

### What's Changed

* feat(deps): upgrade jaq to 3.0, digest crates to 0.11 ([#893](https://github.com/everruns/bashkit/pull/893)) by @chaliy
* chore(deps): require major version upgrades in maintenance checklist ([#892](https://github.com/everruns/bashkit/pull/892)) by @chaliy
* ci(js): add Bun and Deno to JS CI matrix with runtime-compat tests ([#889](https://github.com/everruns/bashkit/pull/889)) by @chaliy
* fix(interpreter): handle compound array assignment in local builtin ([#888](https://github.com/everruns/bashkit/pull/888)) by @chaliy
* fix(interpreter): expand special variables ($#, $?, etc.) in arithmetic ([#887](https://github.com/everruns/bashkit/pull/887)) by @chaliy
* chore: pre-release maintenance (test counts, fuzz fix, code cleanup) ([#885](https://github.com/everruns/bashkit/pull/885)) by @chaliy
* fix(interpreter): set -e should not trigger on compound commands with && chain failure ([#879](https://github.com/everruns/bashkit/pull/879)) by @chaliy
* fix(interpreter): expand assoc array keys with command substitutions ([#878](https://github.com/everruns/bashkit/pull/878)) by @chaliy
* feat(release): add prebuilt CLI binary builds and Homebrew formula ([#871](https://github.com/everruns/bashkit/pull/871)) by @chaliy
* fix(builtins): preserve raw bytes from /dev/urandom through pipeline ([#870](https://github.com/everruns/bashkit/pull/870)) by @chaliy
* fix(interpreter): resolve namerefs in parameter expansion for assoc array subscripts ([#869](https://github.com/everruns/bashkit/pull/869)) by @chaliy
* fix(interpreter): propagate errexit_suppressed through compound commands ([#868](https://github.com/everruns/bashkit/pull/868)) by @chaliy
* test(parser): unskip parse_unexpected_do and parse_unexpected_rbrace ([#866](https://github.com/everruns/bashkit/pull/866)) by @chaliy
* fix(parser): expand $'\n' ANSI-C quoting in concatenated function args ([#865](https://github.com/everruns/bashkit/pull/865)) by @chaliy
* fix(interpreter): treat assoc array subscripts as literal strings ([#864](https://github.com/everruns/bashkit/pull/864)) by @chaliy
* fix(interpreter): correct left-to-right redirect ordering for fd dup + file combos ([#863](https://github.com/everruns/bashkit/pull/863)) by @chaliy
* fix(parser): handle $'...' ANSI-C quoting in parameter expansion patterns ([#856](https://github.com/everruns/bashkit/pull/856)) by @chaliy
* fix(awk): check word boundary before emitting keyword tokens ([#859](https://github.com/everruns/bashkit/pull/859)) by @chaliy
* fix(builtins): preserve full path in ls output for file arguments ([#858](https://github.com/everruns/bashkit/pull/858)) by @chaliy
* fix(builtins): suppress rg line numbers by default (non-tty behavior) ([#857](https://github.com/everruns/bashkit/pull/857)) by @chaliy
* fix(interpreter): resolve nameref for ${!ref[@]} key enumeration ([#855](https://github.com/everruns/bashkit/pull/855)) by @chaliy
* fix(interpreter): fire EXIT trap inside command substitution subshell ([#854](https://github.com/everruns/bashkit/pull/854)) by @chaliy
* fix(js): update exec security test for sandbox-safe exec behavior ([#851](https://github.com/everruns/bashkit/pull/851)) by @chaliy
* fix(interpreter): reset last_exit_code in VFS subprocess isolation ([#850](https://github.com/everruns/bashkit/pull/850)) by @chaliy
* fix(interpreter): treat invalid glob bracket expressions as literals ([#845](https://github.com/everruns/bashkit/pull/845)) by @chaliy
* fix(awk): support backslash-newline line continuation ([#841](https://github.com/everruns/bashkit/pull/841)) by @chaliy
* fix(awk): treat # inside regex literals as literal, not comment ([#840](https://github.com/everruns/bashkit/pull/840)) by @chaliy
* fix(interpreter): resolve namerefs before nounset check ([#839](https://github.com/everruns/bashkit/pull/839)) by @chaliy
* fix(builtins): sort -n extracts leading numeric prefix from strings ([#838](https://github.com/everruns/bashkit/pull/838)) by @chaliy
* feat(interpreter): implement BASH_SOURCE array variable ([#832](https://github.com/everruns/bashkit/pull/832)) by @chaliy
* fix(awk): treat newlines as statement separators in action blocks ([#831](https://github.com/everruns/bashkit/pull/831)) by @chaliy
* feat(api): add BashBuilder::tty() for configurable terminal detection ([#830](https://github.com/everruns/bashkit/pull/830)) by @chaliy
* fix(awk): accept expressions as printf format string ([#829](https://github.com/everruns/bashkit/pull/829)) by @chaliy
* fix(vfs): preserve raw bytes when reading /dev/urandom ([#828](https://github.com/everruns/bashkit/pull/828)) by @chaliy
* fix(awk): evaluate regex literals against $0 in boolean context ([#827](https://github.com/everruns/bashkit/pull/827)) by @chaliy
* fix(parser): preserve double quotes inside $() in double-quoted strings ([#826](https://github.com/everruns/bashkit/pull/826)) by @chaliy
* fix(interpreter): set -e respects AND-OR lists in functions and loops ([#824](https://github.com/everruns/bashkit/pull/824)) by @chaliy
* test(allexport): add regression tests for set -a behavior ([#823](https://github.com/everruns/bashkit/pull/823)) by @chaliy
* fix(builtins): implement `declare -f` for function display and lookup ([#822](https://github.com/everruns/bashkit/pull/822)) by @chaliy
* feat(interpreter): nameref resolution for associative array operations ([#821](https://github.com/everruns/bashkit/pull/821)) by @chaliy
* test(awk): add spec tests for delete array (already implemented) ([#820](https://github.com/everruns/bashkit/pull/820)) by @chaliy
* feat(compgen): scan PATH directories for executables in compgen -c ([#819](https://github.com/everruns/bashkit/pull/819)) by @chaliy
* feat(test): configurable -t fd terminal detection ([#818](https://github.com/everruns/bashkit/pull/818)) by @chaliy
* feat(awk): route /dev/stderr and /dev/stdout to interpreter streams ([#817](https://github.com/everruns/bashkit/pull/817)) by @chaliy
* feat(awk): implement FILENAME built-in variable for multi-file processing ([#816](https://github.com/everruns/bashkit/pull/816)) by @chaliy
* feat(interpreter): exec with command argument — execute and don't return ([#815](https://github.com/everruns/bashkit/pull/815)) by @chaliy
* feat(interpreter): implement set -a (allexport) ([#814](https://github.com/everruns/bashkit/pull/814)) by @chaliy
* feat(interpreter): subprocess isolation for VFS script-by-path execution ([#813](https://github.com/everruns/bashkit/pull/813)) by @chaliy
* feat(interpreter): pipe stdin to VFS script execution ([#812](https://github.com/everruns/bashkit/pull/812)) by @chaliy
* refactor(scripted_tool): ScriptingToolSet returns tools() instead of implementing Tool ([#789](https://github.com/everruns/bashkit/pull/789)) by @chaliy

**Full Changelog**: https://github.com/everruns/bashkit/compare/v0.1.13...v0.1.14

## [0.1.13] - 2026-03-23

### Highlights

- **Community contribution from @achicu**: fixed `find` with multiple paths silently discarding results when one path is missing ([#781](https://github.com/everruns/bashkit/pull/781))
- **Python/Node binding parity** — both bindings now expose the same API surface ([#785](https://github.com/everruns/bashkit/pull/785))
- **Live mount/unmount** on running `Bash` instances for dynamic filesystem composition ([#784](https://github.com/everruns/bashkit/pull/784))

### What's Changed

* fix(examples): exit langchain example to prevent NAPI event loop hang ([#786](https://github.com/everruns/bashkit/pull/786)) by @chaliy
* feat(bindings): add Python/Node binding parity ([#785](https://github.com/everruns/bashkit/pull/785)) by @chaliy
* feat(fs): expose live mount/unmount on running Bash instance ([#784](https://github.com/everruns/bashkit/pull/784)) by @chaliy
* chore: add cargo-vet exemptions for jni-sys 0.3.1, 0.4.1 and jni-sys-macros 0.4.1 ([#783](https://github.com/everruns/bashkit/pull/783)) by @chaliy
* fix: find with multiple paths no longer discards results on missing path ([#781](https://github.com/everruns/bashkit/pull/781)) by @achicu

**Full Changelog**: https://github.com/everruns/bashkit/compare/v0.1.12...v0.1.13

## [0.1.12] - 2026-03-21

### Highlights

- **Restored SearchCapable/SearchProvider traits** for indexed filesystem search
- **Improved text file handling** across 17 builtins with shared lossy read helpers

### What's Changed

* feat(fs): restore SearchCapable/SearchProvider traits ([#779](https://github.com/everruns/bashkit/pull/779))
* refactor(builtins): adopt read_text_file helper across 17 builtins ([#778](https://github.com/everruns/bashkit/pull/778))
* chore(skills): move repo skills under .agents ([#777](https://github.com/everruns/bashkit/pull/777))
* refactor(builtins): share lossy text file reads ([#775](https://github.com/everruns/bashkit/pull/775))

**Full Changelog**: https://github.com/everruns/bashkit/compare/v0.1.11...v0.1.12

## [0.1.11] - 2026-03-20

### Highlights

- **Second external contribution!** Welcome @shubham-lohiya, who exposed the `Bash` class with Monty Python execution and external function handler in the Python bindings ([#760](https://github.com/everruns/bashkit/pull/760)) — making it easy to extend bashkit with custom Python functions
- **Browser terminal example**: Bashkit now runs entirely in the browser via WebAssembly (`wasm32-wasip1-threads`), with a single-file terminal UI — no framework required
- **New features**: structured execution trace events, per-instance memory budgets, static AST budget validation, `head -c` byte mode, IFS separator + `$_` tracking, final environment state in `ExecResult`
- **Security hardening**: blackbox security audit surfaced 15 vulnerabilities — all fixed; readonly variable bypass blocked; stack overflow, memory exhaustion, and source recursion depth limits enforced; shell injection prevented in JS VFS helpers
- **Major refactoring**: FileSystem split into core + FileSystemExt, shared ArgParser extracted, register_builtins! macro replacing 120+ insert calls, ShellRef Context API, shell options split-brain fix

### What's Changed

* chore: pre-release maintenance — docs, fuzz, threat model, cargo-vet ([#774](https://github.com/everruns/bashkit/pull/774))
* fix(interpreter): stabilize command-not-found suggestions ([#773](https://github.com/everruns/bashkit/pull/773))
* refactor: remove blanket clippy::unwrap_used allows ([#772](https://github.com/everruns/bashkit/pull/772))
* chore: move /ship from command to skill format ([#771](https://github.com/everruns/bashkit/pull/771))
* refactor(fs): split FileSystem into core + FileSystemExt ([#770](https://github.com/everruns/bashkit/pull/770))
* refactor(builtins): extract shared ArgParser (#744) ([#769](https://github.com/everruns/bashkit/pull/769))
* refactor: replace hardcoded if-name dispatch with ShellRef Context API ([#767](https://github.com/everruns/bashkit/pull/767))
* refactor: break up 6 monster functions into smaller helpers ([#766](https://github.com/everruns/bashkit/pull/766))
* refactor(interpreter): fix shell options split brain (#736) ([#764](https://github.com/everruns/bashkit/pull/764))
* refactor(builtins): replace 120+ insert calls with register_builtins! macro ([#762](https://github.com/everruns/bashkit/pull/762))
* refactor(builtins): move find/xargs/timeout execution plans from interpreter to builtins ([#761](https://github.com/everruns/bashkit/pull/761))
* feat(python): expose `Bash` class with Monty Python execution and external function handler ([#760](https://github.com/everruns/bashkit/pull/760)) by @shubham-lohiya
* fix(git): error on non-HEAD revision in git show rev:path ([#758](https://github.com/everruns/bashkit/pull/758))
* refactor(builtins): extract git_err helper to eliminate 24 identical error wrapping lines ([#757](https://github.com/everruns/bashkit/pull/757))
* refactor(error): simplify Error enum by merging Parse/ParseAt and removing dead CommandNotFound ([#756](https://github.com/everruns/bashkit/pull/756))
* refactor(fs): remove dead SearchCapable/SearchProvider traits ([#755](https://github.com/everruns/bashkit/pull/755))
* fix(vfs): use fs.remove() for patch file deletion instead of empty write ([#754](https://github.com/everruns/bashkit/pull/754))
* refactor(interpreter): deduplicate declare/local compound assignment and flag parsing ([#753](https://github.com/everruns/bashkit/pull/753))
* refactor(builtins): extract shared search utilities from grep and rg ([#752](https://github.com/everruns/bashkit/pull/752))
* refactor: deduplicate is_valid_var_name into single pub(crate) function ([#751](https://github.com/everruns/bashkit/pull/751))
* refactor(builtins): replace magic variable hack with BuiltinSideEffect enum ([#750](https://github.com/everruns/bashkit/pull/750))
* chore(skills): add design quality review phase to ship command ([#749](https://github.com/everruns/bashkit/pull/749))
* refactor(interpreter): extract glob/pattern matching to glob.rs ([#748](https://github.com/everruns/bashkit/pull/748))
* fix(skills): delegate process-issues shipping to /ship skill ([#747](https://github.com/everruns/bashkit/pull/747))
* chore: convert process-issues command to .claude/skills/ format ([#746](https://github.com/everruns/bashkit/pull/746))
* feat: IFS separator, $_ tracking, and prefix assignment order ([#724](https://github.com/everruns/bashkit/pull/724))
* fix(deps): bump ai SDK to ^5.0.52 and override jsondiffpatch >=0.7.2 ([#723](https://github.com/everruns/bashkit/pull/723))
* fix(deps): override langsmith >=0.4.6 to fix SSRF vulnerability ([#722](https://github.com/everruns/bashkit/pull/722))
* fix(js): wrap napi structs in Arc<SharedState> to prevent invalid pointer access ([#721](https://github.com/everruns/bashkit/pull/721))
* fix: hex escapes, POSIX classes, DEBUG trap, noclobber, indirect arrays ([#719](https://github.com/everruns/bashkit/pull/719))
* fix(js): prevent shell injection in Bash/BashTool VFS helpers ([#718](https://github.com/everruns/bashkit/pull/718))
* fix(interpreter): prevent stack overflow in nested command substitution ([#717](https://github.com/everruns/bashkit/pull/717))
* fix(builtins): bound seq output to prevent memory exhaustion ([#716](https://github.com/everruns/bashkit/pull/716))
* feat(builtins): add head -c byte count mode ([#715](https://github.com/everruns/bashkit/pull/715))
* fix(interpreter): reset transient state between exec() calls (TM-ISO-005/006/007) ([#714](https://github.com/everruns/bashkit/pull/714))
* fix(interpreter): block readonly variable bypass via unset/declare/export (TM-INJ-019/020/021) ([#713](https://github.com/everruns/bashkit/pull/713))
* fix(interpreter): enforce execution timeout via tokio::time::timeout (TM-DOS-057) ([#712](https://github.com/everruns/bashkit/pull/712))
* fix(interpreter): source recursion depth limit (TM-DOS-056) ([#711](https://github.com/everruns/bashkit/pull/711))
* fix(interpreter): declare -a/-i and local -a with inline init ([#710](https://github.com/everruns/bashkit/pull/710))
* feat(fs): optional SearchCapable trait for indexed search ([#709](https://github.com/everruns/bashkit/pull/709))
* feat(trace): structured execution trace events ([#708](https://github.com/everruns/bashkit/pull/708))
* feat(limits): per-instance memory budget for variables/arrays/functions ([#707](https://github.com/everruns/bashkit/pull/707))
* feat(limits): YAML/template depth limits + session-level cumulative counters ([#706](https://github.com/everruns/bashkit/pull/706))
* fix(fs): OverlayFs validate_path + directory count limits + accounting gaps ([#701](https://github.com/everruns/bashkit/pull/701))
* test(python): add advanced security tests for Python integration ([#705](https://github.com/everruns/bashkit/pull/705))
* test(security): add JavaScript integration security tests ([#700](https://github.com/everruns/bashkit/pull/700))
* test(security): blackbox security testing — 15 vulnerability findings ([#688](https://github.com/everruns/bashkit/pull/688))
* fix(security): guard all builtins against internal variable namespace injection ([#696](https://github.com/everruns/bashkit/pull/696))
* feat(interpreter): return final environment state in ExecResult ([#695](https://github.com/everruns/bashkit/pull/695))
* feat(parser): static budget validation on parsed AST before execution ([#694](https://github.com/everruns/bashkit/pull/694))

**Full Changelog**: https://github.com/everruns/bashkit/compare/v0.1.10...v0.1.11

## [0.1.10] - 2026-03-15

### Highlights

- **Node.js native bindings** (`@everruns/bashkit`): Full npm package with NAPI-RS, async execute API, VFS file helpers, lazy file values — 6 platforms, tested on Node 20/22/24, with 200+ tests and 6 examples including OpenAI, Vercel AI, and LangChain integrations
- **Pi coding agent integration**: Bashkit extension for [pi.dev](https://pi.dev/) terminal coding agent — replaces shell, read, write, and edit tools with bashkit-backed virtual implementations, zero real filesystem access
- **41 new builtins** (109→150): rg, patch, zip/unzip, iconv, compgen, json, csv, tomlq, yaml, template, parallel, http, help, fc, tree, readlink, clear, fold, expand/unexpand, envsubst, join, split, and more
- **Performance**: Criterion benchmark harness with auto-save, 7-runner comparison suite, lazy-init HTTP client, trimmed CLI one-shot startup path
- **Coprocess & background execution**: `coproc` support with named FD pairs, background `&` execution with `wait` builtin, cancellation via AtomicBool token

### New Tools & Builtins

- 14 new builtins batch 2: rg, patch, zip/unzip, iconv, compgen, json, csv, tomlq, yaml, template, parallel, http, help, fc
- 7 non-standard builtins + alias/unalias docs
- join and split commands
- clear, fold, expand/unexpand, envsubst
- tree, readlink
- ScriptingToolSet with exclusive/discovery modes
- MCP: expose ScriptedTool as MCP tool
- help builtin for runtime schema introspection

### What's Changed

* feat(pi-integration): add Pi coding agent extension with bashkit VFS ([#638](https://github.com/everruns/bashkit/pull/638))
* feat(find): add -printf format flag support ([#637](https://github.com/everruns/bashkit/pull/637))
* test: un-ignore exec_azure_query_capacity, now passing ([#636](https://github.com/everruns/bashkit/pull/636))
* feat(awk): add Unicode \u escape sequences ([#635](https://github.com/everruns/bashkit/pull/635))
* feat(jq): upgrade jaq crates to latest stable versions ([#634](https://github.com/everruns/bashkit/pull/634))
* feat(vfs): add /dev/urandom and /dev/random to virtual filesystem ([#632](https://github.com/everruns/bashkit/pull/632))
* feat: fix bindings stderr, agent prompt, jq 1.8, awk --csv ([#631](https://github.com/everruns/bashkit/pull/631))
* fix(errexit): assignment-only commands now return exit code 0 ([#630](https://github.com/everruns/bashkit/pull/630))
* chore: pre-release maintenance pass ([#627](https://github.com/everruns/bashkit/pull/627))
* fix(awk): implement output redirection for print/printf ([#626](https://github.com/everruns/bashkit/pull/626))
* feat(js): expose VFS file helpers for agent integrations ([#624](https://github.com/everruns/bashkit/pull/624))
* fix(builtins): preserve empty fields in read IFS splitting ([#623](https://github.com/everruns/bashkit/pull/623))
* fix(interpreter): correct &&/|| operator precedence in [[ ]] conditional ([#622](https://github.com/everruns/bashkit/pull/622))
* fix(js): prevent invalid pointer access in napi bindings ([#621](https://github.com/everruns/bashkit/pull/621))
* fix(builtins): correct -a/-o operator precedence in test/[ builtin ([#620](https://github.com/everruns/bashkit/pull/620))
* refactor(net): lazy-init http client ([#613](https://github.com/everruns/bashkit/pull/613))
* feat(cancel): add cancellation support via AtomicBool token ([#612](https://github.com/everruns/bashkit/pull/612))
* fix(eval): stop scoring tool-call trajectory ([#611](https://github.com/everruns/bashkit/pull/611))
* refactor(cli): trim one-shot startup path ([#609](https://github.com/everruns/bashkit/pull/609))
* fix(parser): track bracket/brace depth in array subscript reader ([#603](https://github.com/everruns/bashkit/pull/603))
* fix(lexer): track brace depth in unquoted ${...} tokenization ([#602](https://github.com/everruns/bashkit/pull/602))
* fix(interpreter): expand ${...} syntax in arithmetic contexts ([#601](https://github.com/everruns/bashkit/pull/601))
* feat(js): support lazy file values in VFS ([#598](https://github.com/everruns/bashkit/pull/598))
* feat(js): add async execute API ([#597](https://github.com/everruns/bashkit/pull/597))
* feat(history): persistent searchable history across Bash instances ([#596](https://github.com/everruns/bashkit/pull/596))
* feat(git): add show/ls-files/rev-parse/restore/merge-base/grep ([#595](https://github.com/everruns/bashkit/pull/595))
* feat(interpreter): implement coproc (coprocess) support ([#594](https://github.com/everruns/bashkit/pull/594))
* feat(eval): improve discovery prompts and bump to gpt-5.4 ([#593](https://github.com/everruns/bashkit/pull/593))
* fix(tool): align toolkit library contract ([#592](https://github.com/everruns/bashkit/pull/592))
* feat(vfs): add mkfifo and named pipe (FIFO) support ([#591](https://github.com/everruns/bashkit/pull/591))
* feat(interpreter): implement background execution with & and wait ([#590](https://github.com/everruns/bashkit/pull/590))
* feat(bench): add Criterion parallel bench with auto-save ([#589](https://github.com/everruns/bashkit/pull/589))
* feat(builtins): add 14 new builtins batch 2 ([#588](https://github.com/everruns/bashkit/pull/588))
* feat(eval): improve scripted tool evals with ScriptingToolSet ([#587](https://github.com/everruns/bashkit/pull/587))
* fix(fs): flush RealFs append to prevent data loss race ([#586](https://github.com/everruns/bashkit/pull/586))
* feat(builtins): add 7 non-standard builtins + alias/unalias docs ([#585](https://github.com/everruns/bashkit/pull/585))
* feat(builtins): add join and split commands ([#584](https://github.com/everruns/bashkit/pull/584))
* feat(bench): 7-runner benchmark comparison with expanded test suite ([#583](https://github.com/everruns/bashkit/pull/583))
* feat(builtins): add clear, fold, expand/unexpand, envsubst commands ([#582](https://github.com/everruns/bashkit/pull/582))
* feat(builtins): add tree command ([#581](https://github.com/everruns/bashkit/pull/581))
* chore(maintenance): extract /maintain skill, add simplification ([#580](https://github.com/everruns/bashkit/pull/580))
* feat(builtins): add readlink command ([#579](https://github.com/everruns/bashkit/pull/579))
* feat(scripted_tool): add ScriptingToolSet with discovery mode support ([#534](https://github.com/everruns/bashkit/pull/534))
* chore(agents): clarify worktree sync and commit identity ([#533](https://github.com/everruns/bashkit/pull/533))
* feat(mcp): expose ScriptedTool as MCP tool ([#532](https://github.com/everruns/bashkit/pull/532))
* docs(scripted_tool): shared context and state patterns ([#530](https://github.com/everruns/bashkit/pull/530))
* feat(scripted_tool): help builtin for runtime schema introspection ([#529](https://github.com/everruns/bashkit/pull/529))
* feat(js): add JavaScript/TypeScript package with npm publishing ([#528](https://github.com/everruns/bashkit/pull/528))
* feat: upgrade to Rust edition 2024 + add doppler to cloud setup ([#527](https://github.com/everruns/bashkit/pull/527))
* feat(eval): add scripting tool evals with multi-dataset support ([#525](https://github.com/everruns/bashkit/pull/525))
* fix: prevent fuzz-found panics on multi-byte input ([#513](https://github.com/everruns/bashkit/pull/513))

**Full Changelog**: https://github.com/everruns/bashkit/compare/v0.1.9...v0.1.10

## [0.1.9] - 2026-03-04

### Highlights

- **First external contribution!** Welcome @achicu, who contributed external function handler support for the Python bindings ([#394](https://github.com/everruns/bashkit/pull/394)) — a milestone for the project as our first community-contributed feature. Thank you!
- Comprehensive security hardening: deep audit with 40+ fixes across VFS, parser, interpreter, network, and Python bindings
- HTTP, git, and Python features now enabled by default in the CLI
- Multi-byte UTF-8 safety across builtins (awk, tr, printf, expr)
- Python runtime improvements: GIL release, tokio runtime reuse, security config preservation

### What's Changed

* feat(python): add external function handler support ([#394](https://github.com/everruns/bashkit/pull/394)) by Alexandru Chiculita
* feat(cli): enable http, git, python by default ([#507](https://github.com/everruns/bashkit/pull/507))
* chore: run maintenance checklist (maintenance) ([#508](https://github.com/everruns/bashkit/pull/508))
* docs: convert doc examples to tested doctests ([#504](https://github.com/everruns/bashkit/pull/504))
* fix(security): batch 3 — issues #498-#499 ([#503](https://github.com/everruns/bashkit/pull/503))
* fix(security): batch 2 — issues #493-#497 ([#502](https://github.com/everruns/bashkit/pull/502))
* fix(security): batch 1 — issues #488-#492 ([#501](https://github.com/everruns/bashkit/pull/501))
* docs: align rustdoc with README, add doc review to maintenance ([#500](https://github.com/everruns/bashkit/pull/500))
* test(security): deep security audit with regression tests ([#487](https://github.com/everruns/bashkit/pull/487))
* fix(builtins): make exported variables visible to Python's os.getenv ([#486](https://github.com/everruns/bashkit/pull/486))
* refactor(interpreter): extract inline builtins from execute_dispatched_command ([#485](https://github.com/everruns/bashkit/pull/485))
* fix(parser): allow glob expansion on unquoted suffix after quoted prefix ([#484](https://github.com/everruns/bashkit/pull/484))
* fix(parser): handle quotes inside ${...} in double-quoted strings ([#483](https://github.com/everruns/bashkit/pull/483))
* fix(parser): expand variables in [[ =~ $var ]] regex patterns ([#482](https://github.com/everruns/bashkit/pull/482))
* fix(builtins): count newlines for wc -l instead of logical lines ([#481](https://github.com/everruns/bashkit/pull/481))
* fix(interpreter): reset OPTIND between bash script invocations ([#478](https://github.com/everruns/bashkit/pull/478))
* fix(builtins): awk array features — SUBSEP, multi-subscript, pre-increment ([#477](https://github.com/everruns/bashkit/pull/477))
* fix(builtins): prevent awk parser panic on multi-byte UTF-8 ([#476](https://github.com/everruns/bashkit/pull/476))
* fix(network): use byte-safe path boundary check in allowlist ([#475](https://github.com/everruns/bashkit/pull/475))
* fix(interpreter): use byte-safe indexing for arithmetic compound assignment ([#474](https://github.com/everruns/bashkit/pull/474))
* fix(builtins): add recursion depth limit to AWK function calls ([#473](https://github.com/everruns/bashkit/pull/473))
* fix(network): use try_from instead of truncating u64-to-usize cast ([#472](https://github.com/everruns/bashkit/pull/472))
* fix(network): redact credentials from allowlist error messages ([#471](https://github.com/everruns/bashkit/pull/471))
* fix(scripted_tool): use Display not Debug format in errors ([#470](https://github.com/everruns/bashkit/pull/470))
* fix(python): add depth limit to py_to_json/json_to_py ([#469](https://github.com/everruns/bashkit/pull/469))
* fix(builtins): handle multi-byte UTF-8 in tr expand_char_set() ([#468](https://github.com/everruns/bashkit/pull/468))
* fix(builtins): use char-based precision truncation in printf ([#467](https://github.com/everruns/bashkit/pull/467))
* fix(builtins): use char count instead of byte length in expr ([#466](https://github.com/everruns/bashkit/pull/466))
* fix(interpreter): detect cyclic nameref to prevent wrong resolution ([#465](https://github.com/everruns/bashkit/pull/465))
* fix(interpreter): sandbox $$ to return 1 instead of host PID ([#464](https://github.com/everruns/bashkit/pull/464))
* fix(python): preserve security config across Bash.reset() ([#463](https://github.com/everruns/bashkit/pull/463))
* fix(git): validate branch names to prevent path injection ([#462](https://github.com/everruns/bashkit/pull/462))
* fix(tool): preserve custom builtins across create_bash calls ([#461](https://github.com/everruns/bashkit/pull/461))
* fix(fs): add validate_path to all InMemoryFs methods ([#460](https://github.com/everruns/bashkit/pull/460))
* fix(fs): recursive delete whiteouts lower-layer children in OverlayFs ([#459](https://github.com/everruns/bashkit/pull/459))
* fix(fs): use combined usage for OverlayFs write limits ([#458](https://github.com/everruns/bashkit/pull/458))
* fix(fs): prevent usage double-counting in OverlayFs ([#457](https://github.com/everruns/bashkit/pull/457))
* fix(fs): enforce write limits on chmod copy-on-write ([#456](https://github.com/everruns/bashkit/pull/456))
* fix(archive): prevent tar path traversal in VFS ([#455](https://github.com/everruns/bashkit/pull/455))
* fix(fs): prevent TOCTOU race in InMemoryFs::append_file() ([#454](https://github.com/everruns/bashkit/pull/454))
* docs: add quick install section to README ([#453](https://github.com/everruns/bashkit/pull/453))
* fix(jq): prevent process env pollution in jq builtin ([#452](https://github.com/everruns/bashkit/pull/452))
* fix(python): reuse tokio runtime instead of creating per call ([#451](https://github.com/everruns/bashkit/pull/451))
* fix(python): release GIL before blocking on tokio runtime ([#450](https://github.com/everruns/bashkit/pull/450))
* fix(python): prevent heredoc delimiter injection in write() ([#449](https://github.com/everruns/bashkit/pull/449))
* fix(python): prevent shell injection in BashkitBackend ([#448](https://github.com/everruns/bashkit/pull/448))
* fix(interpreter): add depth limit to extglob pattern matching ([#447](https://github.com/everruns/bashkit/pull/447))
* fix(interpreter): block internal variable namespace injection ([#445](https://github.com/everruns/bashkit/pull/445))
* chore(ci): bump the github-actions group with 2 updates ([#479](https://github.com/everruns/bashkit/pull/479))
* chore: add tokio-macros 2.6.1 to cargo-vet exemptions ([#480](https://github.com/everruns/bashkit/pull/480))

**Full Changelog**: https://github.com/everruns/bashkit/compare/v0.1.8...v0.1.9

## [0.1.8] - 2026-03-01

### Highlights

- Stderr and combined redirects (`2>`, `2>&1`, `&>`) for real-world script compatibility
- ANSI-C quoting (`$'...'`) and `$"..."` syntax support
- New builtins: `base64`, `md5sum`/`sha1sum`/`sha256sum`, `find -exec`, `grep -L/--exclude-dir`
- `jq` enhancements: `setpath`, `leaf_paths`, improved `match`/`scan`
- Recursive variable deref and array access in arithmetic expressions
- `awk` user-defined functions, `curl -F` multipart form data
- `tar -C/-O` flags, `xargs` command execution
- Per-tool-call `timeout_ms` in ToolRequest
- 244 new Oils-inspired spec tests
- 20+ interpreter/parser bug fixes: heredoc pipes, IFS splitting, subshell isolation, exit code truncation, unicode `${#x}`, shift builtin, and more

### What's Changed

* fix(ci): trigger Python publish workflow on release ([#403](https://github.com/everruns/bashkit/pull/403))
* chore(eval): 2026-02-28 eval run across 5 models with v0.1.7 analysis ([#402](https://github.com/everruns/bashkit/pull/402))
* feat: process remaining issues (#308, #310, #311, #312, #321, #327, #329, #331, #332, #333, #334) ([#393](https://github.com/everruns/bashkit/pull/393))
* chore: add rebase hint to process-issues step 9 ([#392](https://github.com/everruns/bashkit/pull/392))
* fix: reduce skipped spec tests, implement cut/tr features (#309, #314) ([#391](https://github.com/everruns/bashkit/pull/391))
* fix(ci): switch tarpaulin to LLVM engine to fix coverage failures ([#390](https://github.com/everruns/bashkit/pull/390))
* fix: implement var operators, IFS splitting, parser errors, nameref, alias ([#389](https://github.com/everruns/bashkit/pull/389))
* fix(builtins): add jq -R raw input and awk printf parens ([#388](https://github.com/everruns/bashkit/pull/388))
* chore: update pin-project-lite cargo-vet exemption to 0.2.17 ([#387](https://github.com/everruns/bashkit/pull/387))
* feat(builtins): implement find -exec command execution ([#386](https://github.com/everruns/bashkit/pull/386))
* feat(builtins): add grep -L, --exclude-dir, -s, -Z flags ([#385](https://github.com/everruns/bashkit/pull/385))
* feat(builtins): implement jq setpath, leaf_paths, fix match/scan ([#384](https://github.com/everruns/bashkit/pull/384))
* fix(parser): handle heredoc pipe ordering and edge cases ([#379](https://github.com/everruns/bashkit/pull/379))
* fix(interpreter): count unicode chars in ${#x} and add printf \u/\U escapes ([#378](https://github.com/everruns/bashkit/pull/378))
* feat(interpreter): implement stderr and combined redirects (2>, 2>&1, &>) ([#377](https://github.com/everruns/bashkit/pull/377))
* fix(interpreter): isolate subshell state for functions, cwd, traps, positional params ([#376](https://github.com/everruns/bashkit/pull/376))
* chore(specs): document sort/uniq flags, update spec test counts ([#375](https://github.com/everruns/bashkit/pull/375))
* fix(interpreter): split command substitution output on IFS in list context ([#374](https://github.com/everruns/bashkit/pull/374))
* feat(interpreter): implement recursive variable deref and array access in arithmetic ([#373](https://github.com/everruns/bashkit/pull/373))
* feat(parser): implement $'...' ANSI-C quoting and $"..." syntax ([#371](https://github.com/everruns/bashkit/pull/371))
* fix(interpreter): write heredoc content when redirected to file ([#370](https://github.com/everruns/bashkit/pull/370))
* feat(eval): add OpenAI Responses API provider ([#366](https://github.com/everruns/bashkit/pull/366))
* fix(interpreter): truncate exit codes to 8-bit range ([#365](https://github.com/everruns/bashkit/pull/365))
* fix(builtins): make xargs execute commands instead of echoing ([#364](https://github.com/everruns/bashkit/pull/364))
* chore: add ignored-test review step to process-issues ([#363](https://github.com/everruns/bashkit/pull/363))
* test: add 14 Oils-inspired spec test files (244 tests) ([#351](https://github.com/everruns/bashkit/pull/351))
* feat(tool): add per-tool-call timeout_ms to ToolRequest ([#350](https://github.com/everruns/bashkit/pull/350))
* chore(eval): expand eval suite to 52 tasks, add multi-model results ([#349](https://github.com/everruns/bashkit/pull/349))
* feat(eval): add database, config, and build simulation eval categories ([#344](https://github.com/everruns/bashkit/pull/344))
* feat(tool): list all 80+ builtins in help text ([#343](https://github.com/everruns/bashkit/pull/343))
* fix(wc): match real bash output padding behavior ([#342](https://github.com/everruns/bashkit/pull/342))
* chore(tests): update spec_tests.rs skip count from 66 to 18 ([#341](https://github.com/everruns/bashkit/pull/341))
* refactor(error): add From<regex::Error> impl for Error ([#340](https://github.com/everruns/bashkit/pull/340))
* chore: add /process-issues claude command ([#339](https://github.com/everruns/bashkit/pull/339))
* chore: close verified-not-reproducible issues #279, #282 ([#307](https://github.com/everruns/bashkit/pull/307))
* test: verify issues #275, #279, #282 are not reproducible ([#306](https://github.com/everruns/bashkit/pull/306))
* feat(curl): add -F multipart form data support ([#305](https://github.com/everruns/bashkit/pull/305))
* feat(find): parse -exec flag without erroring ([#304](https://github.com/everruns/bashkit/pull/304))
* feat(awk): add user-defined function support ([#303](https://github.com/everruns/bashkit/pull/303))
* feat(tar): add -C (change directory) and -O (stdout) flags ([#302](https://github.com/everruns/bashkit/pull/302))
* feat(base64): add base64 encode/decode builtin command ([#301](https://github.com/everruns/bashkit/pull/301))
* fix(eval): add /var/log to script_health_check task files ([#300](https://github.com/everruns/bashkit/pull/300))
* fix(eval): accept both quoted and unquoted CSV in json_to_csv_export ([#299](https://github.com/everruns/bashkit/pull/299))
* fix(jq): return ExecResult::err instead of Error::Execution for stderr suppression ([#298](https://github.com/everruns/bashkit/pull/298))
* fix(test): resolve relative paths against cwd in file test operators ([#297](https://github.com/everruns/bashkit/pull/297))
* fix(interpreter): shift builtin now updates positional parameters ([#296](https://github.com/everruns/bashkit/pull/296))
* fix(lexer): handle backslash-newline line continuation between tokens ([#295](https://github.com/everruns/bashkit/pull/295))
* fix(interpreter): forward pipeline stdin to user-defined functions ([#294](https://github.com/everruns/bashkit/pull/294))
* fix(test): trim whitespace in parse_int for integer comparisons ([#293](https://github.com/everruns/bashkit/pull/293))

**Full Changelog**: https://github.com/everruns/bashkit/compare/v0.1.7...v0.1.8

## [0.1.7] - 2026-02-26

### Highlights

- 20+ new builtins: `declare`/`typeset`, `let`, `getopts`, `trap`, `caller`, `shopt`, `pushd`/`popd`/`dirs`, `seq`, `tac`, `rev`, `yes`, `expr`, `mktemp`, `realpath`, and more
- Glob options: `dotglob`, `nocaseglob`, `failglob`, `noglob`, `globstar`
- Shell flags: `bash -e/-x/-u/-f/-o`, `set -x` xtrace debugging
- `select` construct, `case ;;&` fallthrough, `FUNCNAME` variable
- Nameref variables (`declare -n`), case conversion (`declare -l/-u`)
- 10+ bug fixes for quoting, arrays, globs, and redirections

### What's Changed

* feat(interpreter): implement bash/sh -e/-x/-u/-f/-o flags ([#270](https://github.com/everruns/bashkit/pull/270))
* chore(eval): run 2026-02-25 evals across 4 models ([#271](https://github.com/everruns/bashkit/pull/271))
* feat(interpreter): implement glob options (dotglob, nocaseglob, failglob, noglob, globstar) ([#269](https://github.com/everruns/bashkit/pull/269))
* feat(builtins): implement pushd, popd, dirs ([#268](https://github.com/everruns/bashkit/pull/268))
* feat(builtins): implement file comparison test operators ([#267](https://github.com/everruns/bashkit/pull/267))
* feat(builtins): implement expr builtin ([#266](https://github.com/everruns/bashkit/pull/266))
* feat(builtins): implement yes and realpath builtins ([#265](https://github.com/everruns/bashkit/pull/265))
* feat(interpreter): implement caller builtin ([#264](https://github.com/everruns/bashkit/pull/264))
* feat(builtins): implement printf %q shell quoting ([#263](https://github.com/everruns/bashkit/pull/263))
* feat(builtins): implement tac and rev builtins ([#262](https://github.com/everruns/bashkit/pull/262))
* feat(builtins): implement seq builtin ([#261](https://github.com/everruns/bashkit/pull/261))
* chore(deps): bump pyo3 to 0.28.2 and pyo3-async-runtimes to 0.28 ([#260](https://github.com/everruns/bashkit/pull/260))
* feat(builtins): implement mktemp builtin ([#259](https://github.com/everruns/bashkit/pull/259))
* feat(interpreter): implement trap -p flag and sorted trap listing ([#258](https://github.com/everruns/bashkit/pull/258))
* feat(builtins): implement set -o / set +o option display ([#257](https://github.com/everruns/bashkit/pull/257))
* feat(interpreter): implement declare -l/-u case conversion attributes ([#256](https://github.com/everruns/bashkit/pull/256))
* feat(interpreter): implement declare -n nameref variables ([#255](https://github.com/everruns/bashkit/pull/255))
* feat(builtins): implement shopt builtin with nullglob enforcement ([#254](https://github.com/everruns/bashkit/pull/254))
* feat(interpreter): implement set -x xtrace debugging ([#253](https://github.com/everruns/bashkit/pull/253))
* feat(bash): auto-populate shell variables (PWD, HOME, USER, etc.) ([#252](https://github.com/everruns/bashkit/pull/252))
* feat(bash): implement select construct ([#251](https://github.com/everruns/bashkit/pull/251))
* feat(bash): implement let builtin and fix declare -i arithmetic ([#250](https://github.com/everruns/bashkit/pull/250))
* feat(bash): case ;& and ;;& fallthrough/continue-matching ([#249](https://github.com/everruns/bashkit/pull/249))
* feat(bash): implement FUNCNAME special variable ([#248](https://github.com/everruns/bashkit/pull/248))
* fix(bash): backslash-newline line continuation in double quotes ([#247](https://github.com/everruns/bashkit/pull/247))
* fix(bash): nested double quotes inside $() in double-quoted strings ([#246](https://github.com/everruns/bashkit/pull/246))
* fix(bash): input redirections on compound commands ([#245](https://github.com/everruns/bashkit/pull/245))
* fix(bash): glob pattern matching in [[ == ]] and [[ != ]] ([#244](https://github.com/everruns/bashkit/pull/244))
* fix(bash): negative array indexing ${arr[-1]} ([#243](https://github.com/everruns/bashkit/pull/243))
* fix(bash): BASH_REMATCH not populated when regex starts with parens ([#242](https://github.com/everruns/bashkit/pull/242))
* feat(bash): arithmetic exponentiation, base literals, mapfile ([#241](https://github.com/everruns/bashkit/pull/241))
* feat: grep binary detection, awk %.6g and sorted for-in ([#240](https://github.com/everruns/bashkit/pull/240))
* feat: bash compatibility — compound arrays, grep -f, awk getline, jq env/input ([#238](https://github.com/everruns/bashkit/pull/238))
* feat: string ops, read -r, heredoc tests ([#237](https://github.com/everruns/bashkit/pull/237))
* feat: associative arrays, chown/kill builtins, array slicing tests ([#236](https://github.com/everruns/bashkit/pull/236))
* feat: cat -v, sort -m, brace/date/lexer fixes ([#234](https://github.com/everruns/bashkit/pull/234))
* feat: type/which/declare/ln builtins, errexit, nounset fix, sort -z, cut -z ([#233](https://github.com/everruns/bashkit/pull/233))
* feat: paste, command, getopts, nounset, [[ =~ ]], glob **, backtick subst ([#232](https://github.com/everruns/bashkit/pull/232))
* feat(date): add -R, -I flags and %N format ([#231](https://github.com/everruns/bashkit/pull/231))
* fix(lexer): handle backslash-escaped metacharacters ([#230](https://github.com/everruns/bashkit/pull/230))
* feat(grep): add --include/--exclude glob patterns ([#229](https://github.com/everruns/bashkit/pull/229))
* feat(sort,uniq,cut,tr): add sort/uniq/cut/tr missing options ([#228](https://github.com/everruns/bashkit/pull/228))
* feat(sed): grouped commands, branching, Q quit, step/zero addresses ([#227](https://github.com/everruns/bashkit/pull/227))
* chore(deps): upgrade monty to latest main (87f8f31) ([#226](https://github.com/everruns/bashkit/pull/226))
* fix(ci): repair nightly CI and add fuzz compile guard ([#225](https://github.com/everruns/bashkit/pull/225))

**Full Changelog**: https://github.com/everruns/bashkit/compare/v0.1.6...v0.1.7

## [0.1.6] - 2026-02-20

### Highlights

- ScriptedTool for composing multi-tool bash orchestration with Python/LangChain bindings
- Streaming output support for Tool trait
- Script file execution by path
- 10 interpreter bug fixes surfaced by eval harness

### What's Changed

* chore: pre-release maintenance checklist ([#223](https://github.com/everruns/bashkit/pull/223)) by @chaliy
* feat(interpreter): support executing script files by path ([#222](https://github.com/everruns/bashkit/pull/222)) by @chaliy
* fix(jq): fix argument parsing, add test coverage, update docs ([#221](https://github.com/everruns/bashkit/pull/221)) by @chaliy
* feat(tool): add streaming output support ([#220](https://github.com/everruns/bashkit/pull/220)) by @chaliy
* feat(python): ScriptedTool bindings + LangChain integration ([#219](https://github.com/everruns/bashkit/pull/219)) by @chaliy
* refactor(examples): extract fake tools into separate module ([#218](https://github.com/everruns/bashkit/pull/218)) by @chaliy
* chore: add small-PR preference to AGENTS.md ([#217](https://github.com/everruns/bashkit/pull/217)) by @chaliy
* fix(builtins): resolve 10 eval-surfaced interpreter bugs ([#216](https://github.com/everruns/bashkit/pull/216)) by @chaliy
* fix: address 10 code TODOs across codebase ([#215](https://github.com/everruns/bashkit/pull/215)) by @chaliy
* test: add skipped tests for eval-surfaced interpreter bugs ([#214](https://github.com/everruns/bashkit/pull/214)) by @chaliy
* feat(scripted_tool): add ScriptedTool for multi-tool bash composition ([#213](https://github.com/everruns/bashkit/pull/213)) by @chaliy
* ci(python): add Python bindings CI with ruff and pytest ([#212](https://github.com/everruns/bashkit/pull/212)) by @chaliy
* fix(interpreter): apply brace/glob expansion in for-loop word list ([#211](https://github.com/everruns/bashkit/pull/211)) by @chaliy
* feat(python): add PydanticAI integration and example ([#210](https://github.com/everruns/bashkit/pull/210)) by @chaliy
* fix(ci): add --allow-dirty for cargo publish after stripping monty ([#209](https://github.com/everruns/bashkit/pull/209)) by @chaliy
* fix(ci): strip git-only monty dep before crates.io publish ([#208](https://github.com/everruns/bashkit/pull/208)) by @chaliy

**Full Changelog**: https://github.com/everruns/bashkit/compare/v0.1.5...v0.1.6

## [0.1.5] - 2026-02-17

### Highlights

- Direct Monty Python integration (removed subprocess worker) for simpler embedding
- Improved AWK parser: match, gensub, power operators, printf formats
- PyPI publishing with pre-built wheels for all major platforms
- Bug fixes for sed, parser redirections, array expansion, and env assignments

### What's Changed

* chore: pre-release maintenance — deps, docs, specs ([#206](https://github.com/everruns/bashkit/pull/206)) by @chaliy
* test(python): regression tests for monty v0.0.5/v0.0.6 ([#205](https://github.com/everruns/bashkit/pull/205)) by @chaliy
* refactor(python): direct Monty integration, remove worker subprocess ([#203](https://github.com/everruns/bashkit/pull/203)) by @chaliy
* docs: add overview video to README ([#202](https://github.com/everruns/bashkit/pull/202)) by @chaliy
* fix(interpreter): expand array args as separate fields ([#201](https://github.com/everruns/bashkit/pull/201)) by @chaliy
* fix(interpreter): prefix env assignments visible to commands ([#200](https://github.com/everruns/bashkit/pull/200)) by @chaliy
* chore(specs): add domain egress allowlist threat model ([#199](https://github.com/everruns/bashkit/pull/199)) by @chaliy
* chore(deps): update pyo3 requirement from 0.24 to 0.24.2 ([#198](https://github.com/everruns/bashkit/pull/198)) by @chaliy
* chore: reframe language from sandboxed bash to virtual bash ([#197](https://github.com/everruns/bashkit/pull/197)) by @chaliy
* fix(builtins): fix sed ampersand replacement and escape handling ([#196](https://github.com/everruns/bashkit/pull/196)) by @chaliy
* fix(parser): support output redirection on compound commands ([#195](https://github.com/everruns/bashkit/pull/195)) by @chaliy
* fix(builtins): use streaming JSON deserializer in jq for multi-line input ([#194](https://github.com/everruns/bashkit/pull/194)) by @chaliy
* fix(builtins): handle escape sequences in AWK -F field separator ([#193](https://github.com/everruns/bashkit/pull/193)) by @chaliy
* fix(builtins): improve AWK parser with match, gensub, power, printf ([#192](https://github.com/everruns/bashkit/pull/192)) by @chaliy
* docs(examples): use bashkit from PyPI instead of local build ([#190](https://github.com/everruns/bashkit/pull/190)) by @chaliy
* fix(python): enable PyO3 generate-import-lib for Windows wheels ([#189](https://github.com/everruns/bashkit/pull/189)) by @chaliy
* feat(python): add PyPI publishing with pre-built wheels ([#188](https://github.com/everruns/bashkit/pull/188)) by @chaliy
* chore(ci): Bump taiki-e/cache-cargo-install-action from 2 to 3 ([#186](https://github.com/everruns/bashkit/pull/186)) by @chaliy
* feat(eval): expand dataset to 37 tasks with JSON scenarios ([#185](https://github.com/everruns/bashkit/pull/185)) by @chaliy

**Full Changelog**: https://github.com/everruns/bashkit/compare/v0.1.4...v0.1.5

## [0.1.4] - 2026-02-09

### Highlights

- jq builtin now supports file arguments

### What's Changed

* fix(builtins): support file arguments in jq builtin ([#183](https://github.com/everruns/bashkit/pull/183)) by @chaliy
* chore(ci): split monolithic check job and move heavy analysis to nightly ([#182](https://github.com/everruns/bashkit/pull/182)) by @chaliy
* refactor(test): drop 'new_' prefix from curl/wget flag test modules ([#181](https://github.com/everruns/bashkit/pull/181)) by @chaliy
* fix(publish): remove unpublished monty git dep for v0.1.3 ([#180](https://github.com/everruns/bashkit/pull/180)) by @chaliy
* fix(publish): remove cargo dep on unpublished bashkit-monty-worker by @chaliy

**Full Changelog**: https://github.com/everruns/bashkit/compare/v0.1.3...v0.1.4

## [0.1.3] - 2026-02-08

### Highlights

- 9 new CLI tools: nl, paste, column, comm, diff, strings, od, xxd, hexdump
- Security hardening: parser depth limits, path validation, nested loop threat mitigation
- Embedded Python interpreter via Monty with subprocess isolation and crash protection
- LLM evaluation harness for tool usage testing across multiple models
- Improved bash compatibility for LLM-generated scripts

### What's Changed

* chore(eval): multi-model eval results and docs ([#177](https://github.com/everruns/bashkit/pull/177)) by @chaliy
* chore: pre-release maintenance — deps, docs, security, specs ([#176](https://github.com/everruns/bashkit/pull/176)) by @chaliy
* chore(specs): document file size reporting requirements ([#175](https://github.com/everruns/bashkit/pull/175)) by @chaliy
* fix(date): support compound expressions, prevent 10k cmd limit blow-up ([#174](https://github.com/everruns/bashkit/pull/174)) by @chaliy
* fix(limits): reset execution counters per exec() call ([#173](https://github.com/everruns/bashkit/pull/173)) by @chaliy
* fix(interpreter): complete source/. function loading ([#172](https://github.com/everruns/bashkit/pull/172)) by @chaliy
* feat(builtins): add 9 CLI tools — nl, paste, column, comm, diff, strings, od, xxd, hexdump ([#171](https://github.com/everruns/bashkit/pull/171)) by @chaliy
* feat(tool): add language warnings and rename llmtext to help ([#170](https://github.com/everruns/bashkit/pull/170)) by @chaliy
* fix(eval): remove llmtext from system prompt ([#169](https://github.com/everruns/bashkit/pull/169)) by @chaliy
* docs: update READMEs and lib.rs with latest features ([#168](https://github.com/everruns/bashkit/pull/168)) by @chaliy
* fix: close 5 critical bashkit gaps blocking LLM-generated scripts ([#167](https://github.com/everruns/bashkit/pull/167)) by @chaliy
* fix(security): mitigate path validation and nested loop threats ([#166](https://github.com/everruns/bashkit/pull/166)) by @chaliy
* feat(python): upgrade monty to v0.0.4 ([#165](https://github.com/everruns/bashkit/pull/165)) by @chaliy
* feat: improve bash compatibility for LLM-generated scripts ([#164](https://github.com/everruns/bashkit/pull/164)) by @chaliy
* fix(security): add depth limits to awk/jq builtin parsers (TM-DOS-027) ([#163](https://github.com/everruns/bashkit/pull/163)) by @chaliy
* feat(python): subprocess isolation for Monty crash protection ([#162](https://github.com/everruns/bashkit/pull/162)) by @chaliy
* fix(security): mitigate parser depth overflow attacks ([#161](https://github.com/everruns/bashkit/pull/161)) by @chaliy
* feat(eval): multi-model evals with tool call success metric ([#160](https://github.com/everruns/bashkit/pull/160)) by @chaliy
* feat(python): embed Monty Python interpreter with VFS bridging ([#159](https://github.com/everruns/bashkit/pull/159)) by @chaliy
* feat(eval): add bashkit-eval crate for LLM tool usage evaluation ([#158](https://github.com/everruns/bashkit/pull/158)) by @chaliy
* chore: rename BashKit → Bashkit ([#157](https://github.com/everruns/bashkit/pull/157)) by @chaliy
* docs(readme): add security links by @chaliy

**Full Changelog**: https://github.com/everruns/bashkit/compare/v0.1.2...v0.1.3

## [0.1.2] - 2026-02-06

### Highlights

- Python bindings with LangChain and Deep Agents integrations
- Virtual git support (branch, checkout, diff, reset)
- Bash/sh script execution commands
- Virtual filesystem improvements: /dev/null support, duplicate name prevention, FsBackend trait

### What's Changed

* feat(interpreter): add bash and sh commands for script execution ([#154](https://github.com/everruns/bashkit/pull/154)) by @chaliy
* fix(vfs): prevent duplicate file/directory names + add FsBackend trait ([#153](https://github.com/everruns/bashkit/pull/153)) by @chaliy
* feat(python): add Deep Agents integration with shared VFS ([#152](https://github.com/everruns/bashkit/pull/152)) by @chaliy
* test(fs): add file size reporting tests ([#150](https://github.com/everruns/bashkit/pull/150)) by @chaliy
* chore(ci): bump github-actions group dependencies ([#149](https://github.com/everruns/bashkit/pull/149)) by @chaliy
* fix(sandbox): normalize paths and support root directory access ([#148](https://github.com/everruns/bashkit/pull/148)) by @chaliy
* feat(python): add Python bindings and LangChain integration ([#147](https://github.com/everruns/bashkit/pull/147)) by @chaliy
* docs: add security policy reference to README ([#146](https://github.com/everruns/bashkit/pull/146)) by @chaliy
* chore: add .claude/settings.json ([#145](https://github.com/everruns/bashkit/pull/145)) by @chaliy
* feat(examples): add git_workflow example ([#144](https://github.com/everruns/bashkit/pull/144)) by @chaliy
* feat(git): add sandboxed git support with branch/checkout/diff/reset ([#143](https://github.com/everruns/bashkit/pull/143)) by @chaliy
* test(find,ls): add comprehensive subdirectory recursion tests ([#142](https://github.com/everruns/bashkit/pull/142)) by @chaliy
* fix(ls): add -t option for sorting by modification time ([#141](https://github.com/everruns/bashkit/pull/141)) by @chaliy
* feat(jq): add --version flag support ([#140](https://github.com/everruns/bashkit/pull/140)) by @chaliy
* feat(vfs): add /dev/null support at interpreter level ([#139](https://github.com/everruns/bashkit/pull/139)) by @chaliy
* chore: clarify commit type for specs and AGENTS.md updates ([#138](https://github.com/everruns/bashkit/pull/138)) by @chaliy
* feat(grep): add missing flags and unskip tests ([#137](https://github.com/everruns/bashkit/pull/137)) by @chaliy

**Full Changelog**: https://github.com/everruns/bashkit/compare/v0.1.1...v0.1.2

## [0.1.1] - 2026-02-04

### Highlights

- Network commands: curl/wget now support timeout flags (--max-time, --timeout)
- Parser improvements: $LINENO variable and line numbers in error messages
- jq enhanced: new flags (-S, -s, -e, --tab, -j, -c, -n)
- sed: in-place editing with -i flag
- Structured logging with automatic security redaction

### What's Changed

* fix(test): fix printf format repeat and update test coverage ([#135](https://github.com/everruns/bashkit/pull/135)) by @chaliy
* feat(network): implement curl/wget timeout support with safety limits ([#134](https://github.com/everruns/bashkit/pull/134)) by @chaliy
* docs: consolidate intentionally unimplemented features documentation ([#133](https://github.com/everruns/bashkit/pull/133)) by @chaliy
* feat(parser): add line number support for $LINENO and error messages ([#132](https://github.com/everruns/bashkit/pull/132)) by @chaliy
* feat(sed): enable -i in-place editing flag ([#131](https://github.com/everruns/bashkit/pull/131)) by @chaliy
* feat(tool): refactor Tool trait with improved outputs ([#130](https://github.com/everruns/bashkit/pull/130)) by @chaliy
* docs(vfs): clarify symlink handling is intentional security decision ([#129](https://github.com/everruns/bashkit/pull/129)) by @chaliy
* fix(test): fix failing tests and remove dead code ([#128](https://github.com/everruns/bashkit/pull/128)) by @chaliy
* feat(curl): implement --max-time per-request timeout ([#127](https://github.com/everruns/bashkit/pull/127)) by @chaliy
* feat(jq): add -S, -s, -e, --tab, -j flags ([#126](https://github.com/everruns/bashkit/pull/126)) by @chaliy
* feat(for): implement positional params iteration in for loops ([#125](https://github.com/everruns/bashkit/pull/125)) by @chaliy
* test(jq): enable group_by test that already passes ([#124](https://github.com/everruns/bashkit/pull/124)) by @chaliy
* docs(agents): add testing requirements to pre-PR checklist ([#123](https://github.com/everruns/bashkit/pull/123)) by @chaliy
* test(jq): enable jq_del test that already passes ([#122](https://github.com/everruns/bashkit/pull/122)) by @chaliy
* chore(deps): update reqwest, schemars, criterion, colored, tabled ([#121](https://github.com/everruns/bashkit/pull/121)) by @chaliy
* docs: add Everruns ecosystem reference ([#120](https://github.com/everruns/bashkit/pull/120)) by @chaliy
* feat(jq): add compact output (-c) and null input (-n) flags ([#119](https://github.com/everruns/bashkit/pull/119)) by @chaliy
* docs(network): remove outdated 'stub' references for curl/wget ([#118](https://github.com/everruns/bashkit/pull/118)) by @chaliy
* docs: remove benchmark interpretation from README ([#117](https://github.com/everruns/bashkit/pull/117)) by @chaliy
* feat(logging): add structured logging with security redaction ([#116](https://github.com/everruns/bashkit/pull/116)) by @chaliy
* fix(security): prevent panics and add internal error handling ([#115](https://github.com/everruns/bashkit/pull/115)) by @chaliy
* fix(parser): support quoted heredoc delimiters ([#114](https://github.com/everruns/bashkit/pull/114)) by @chaliy
* fix(date): handle timezone format errors gracefully ([#113](https://github.com/everruns/bashkit/pull/113)) by @chaliy
* fix: implement missing parameter expansion and fix output mismatches ([#112](https://github.com/everruns/bashkit/pull/112)) by @chaliy
* docs(security): add threat model with stable IDs and public doc ([#111](https://github.com/everruns/bashkit/pull/111)) by @chaliy
* chore(bench): add performance benchmark results ([#110](https://github.com/everruns/bashkit/pull/110)) by @chaliy
* docs: update KNOWN_LIMITATIONS.md with current test counts ([#109](https://github.com/everruns/bashkit/pull/109)) by @chaliy
* refactor(builtins): extract shared resolve_path helper ([#108](https://github.com/everruns/bashkit/pull/108)) by @chaliy
* refactor(vfs): rename to mount_text/mount_readonly_text with custom fs support ([#107](https://github.com/everruns/bashkit/pull/107)) by @chaliy
* fix(echo): support combined flags and fix test expectations ([#106](https://github.com/everruns/bashkit/pull/106)) by @chaliy

**Full Changelog**: https://github.com/everruns/bashkit/compare/v0.1.0...v0.1.1

## [0.1.0] - 2026-02-02

### Highlights

- Initial release of Bashkit virtual bash interpreter
- Core interpreter with bash-compatible syntax support
- Virtual filesystem (VFS) abstraction for virtual file operations
- Resource limits: memory, execution time, operation count
- Built-in commands: echo, printf, cat, head, tail, wc, grep, sed, awk, jq, sort, uniq, cut, tr, date, base64, md5sum, sha256sum, gzip, gunzip, etc
- CLI tool for running scripts and interactive REPL
- Security testing with fail-point injection

### What's Changed

* feat(test): add grammar-based differential fuzzing ([#83](https://github.com/everruns/bashkit/pull/83)) by @chaliy
* feat: implement missing grep and sed flags ([#82](https://github.com/everruns/bashkit/pull/82)) by @chaliy
* feat(test): add compatibility report generator ([#81](https://github.com/everruns/bashkit/pull/81)) by @chaliy
* feat(grep): implement missing grep flags (-A/-B/-C, -m, -q, -x, -e) ([#80](https://github.com/everruns/bashkit/pull/80)) by @chaliy
* feat(test): add script to check expected outputs against bash ([#79](https://github.com/everruns/bashkit/pull/79)) by @chaliy
* feat: implement release process for 0.1.0 ([#78](https://github.com/everruns/bashkit/pull/78)) by @chaliy
* test(spec): enable bash comparison tests in CI ([#77](https://github.com/everruns/bashkit/pull/77)) by @chaliy
* feat: implement POSIX Shell Command Language compliance ([#76](https://github.com/everruns/bashkit/pull/76)) by @chaliy
* docs: embed custom guides in rustdoc via include_str ([#75](https://github.com/everruns/bashkit/pull/75)) by @chaliy
* docs: update built-in commands documentation to reflect actual implementation ([#74](https://github.com/everruns/bashkit/pull/74)) by @chaliy
* test(builtins): add example and integration tests for custom builtins ([#73](https://github.com/everruns/bashkit/pull/73)) by @chaliy
* docs: update KNOWN_LIMITATIONS and compatibility docs ([#72](https://github.com/everruns/bashkit/pull/72)) by @chaliy
* fix: resolve cargo doc collision and rustdoc warnings ([#71](https://github.com/everruns/bashkit/pull/71)) by @chaliy
* docs(specs): document 18 new CLI builtins ([#70](https://github.com/everruns/bashkit/pull/70)) by @chaliy
* docs: add comprehensive rustdoc documentation for public API ([#69](https://github.com/everruns/bashkit/pull/69)) by @chaliy
* docs(tests): complete skipped tests TODO list ([#68](https://github.com/everruns/bashkit/pull/68)) by @chaliy
* feat: implement bash compatibility features ([#67](https://github.com/everruns/bashkit/pull/67)) by @chaliy
* feat(parser): add fuel-based operation limit to prevent DoS ([#66](https://github.com/everruns/bashkit/pull/66)) by @chaliy
* feat(parser): add AST depth limit to prevent stack overflow ([#65](https://github.com/everruns/bashkit/pull/65)) by @chaliy
* feat(parser): add input size validation to prevent DoS ([#64](https://github.com/everruns/bashkit/pull/64)) by @chaliy
* feat(parser): add parser timeout to prevent DoS ([#63](https://github.com/everruns/bashkit/pull/63)) by @chaliy
* fix(interpreter): handle command not found like bash ([#61](https://github.com/everruns/bashkit/pull/61)) by @chaliy
* feat(builtins): add custom builtins support ([#60](https://github.com/everruns/bashkit/pull/60)) by @chaliy
* docs: document skipped tests and curl coverage gap ([#59](https://github.com/everruns/bashkit/pull/59)) by @chaliy
* fix(timeout): make timeout tests reliable with virtual time ([#58](https://github.com/everruns/bashkit/pull/58)) by @chaliy
* test(bash): enable bash core tests in CI ([#57](https://github.com/everruns/bashkit/pull/57)) by @chaliy
* chore(clippy): enable clippy::unwrap_used lint ([#56](https://github.com/everruns/bashkit/pull/56)) by @chaliy
* feat(security): add cargo-vet for supply chain tracking ([#54](https://github.com/everruns/bashkit/pull/54)) by @chaliy
* ci: add AddressSanitizer job for stack overflow detection ([#52](https://github.com/everruns/bashkit/pull/52)) by @chaliy
* fix(ci): add checks:write permission for cargo-audit ([#51](https://github.com/everruns/bashkit/pull/51)) by @chaliy
* chore(ci): add Dependabot configuration ([#50](https://github.com/everruns/bashkit/pull/50)) by @chaliy
* test: port comprehensive test cases from just-bash ([#49](https://github.com/everruns/bashkit/pull/49)) by @chaliy
* fix(awk): fix multi-statement parsing and add gsub/split support ([#48](https://github.com/everruns/bashkit/pull/48)) by @chaliy
* feat(time,timeout): implement time keyword and timeout command ([#47](https://github.com/everruns/bashkit/pull/47)) by @chaliy
* refactor(test): optimize proptest for CI speed ([#46](https://github.com/everruns/bashkit/pull/46)) by @chaliy
* feat(builtins): implement 18 new CLI commands ([#45](https://github.com/everruns/bashkit/pull/45)) by @chaliy
* feat(system): add configurable username and hostname to BashBuilder ([#44](https://github.com/everruns/bashkit/pull/44)) by @chaliy
* feat(security): add security tooling for vulnerability detection ([#43](https://github.com/everruns/bashkit/pull/43)) by @chaliy
* feat(sed): implement case insensitive flag and multiple commands ([#42](https://github.com/everruns/bashkit/pull/42)) by @chaliy
* docs: update testing docs to reflect current status ([#41](https://github.com/everruns/bashkit/pull/41)) by @chaliy
* feat(grep): implement -w and -l stdin support ([#40](https://github.com/everruns/bashkit/pull/40)) by @chaliy
* fix(jq): use pretty-printed output for arrays and objects ([#39](https://github.com/everruns/bashkit/pull/39)) by @chaliy
* feat(jq): implement -r/--raw-output flag ([#38](https://github.com/everruns/bashkit/pull/38)) by @chaliy
* feat(fs): enable custom filesystem implementations from external crates ([#37](https://github.com/everruns/bashkit/pull/37)) by @chaliy
* fix(parser,interpreter): add support for arithmetic commands and C-style for loops ([#36](https://github.com/everruns/bashkit/pull/36)) by @chaliy
* feat(grep): implement -o flag for only-matching output ([#35](https://github.com/everruns/bashkit/pull/35)) by @chaliy
* docs(agents): add test-first principle for bug fixes ([#34](https://github.com/everruns/bashkit/pull/34)) by @chaliy
* docs: update testing spec and known limitations with accurate counts ([#33](https://github.com/everruns/bashkit/pull/33)) by @chaliy
* docs: add PR convention to never include Claude session links ([#32](https://github.com/everruns/bashkit/pull/32)) by @chaliy
* feat(examples): add LLM agent example with real Claude integration ([#31](https://github.com/everruns/bashkit/pull/31)) by @chaliy
* fix: resolve Bashkit parsing and filesystem bugs ([#30](https://github.com/everruns/bashkit/pull/30)) by @chaliy
* feat(bench): add parallel execution benchmark ([#29](https://github.com/everruns/bashkit/pull/29)) by @chaliy
* feat(fs): add direct filesystem access via Bash.fs() ([#28](https://github.com/everruns/bashkit/pull/28)) by @chaliy
* feat(bench): add benchmark tool to compare bashkit, bash, and just-bash ([#27](https://github.com/everruns/bashkit/pull/27)) by @chaliy
* fix(test): isolate fail-point tests for CI execution ([#26](https://github.com/everruns/bashkit/pull/26)) by @chaliy
* ci: add examples execution to CI workflow ([#25](https://github.com/everruns/bashkit/pull/25)) by @chaliy
* feat: add comprehensive builtins, job control, and test coverage ([#24](https://github.com/everruns/bashkit/pull/24)) by @chaliy
* feat(security): add fail-rs security testing and threat model ([#23](https://github.com/everruns/bashkit/pull/23)) by @chaliy
* docs: update CONTRIBUTING and prepare repo for publishing ([#22](https://github.com/everruns/bashkit/pull/22)) by @chaliy
* docs: remove MCP server mode references from README ([#21](https://github.com/everruns/bashkit/pull/21)) by @chaliy
* docs: update compatibility scorecard with array fixes ([#20](https://github.com/everruns/bashkit/pull/20)) by @chaliy
* docs: add acknowledgment for Vercel's just-bash inspiration ([#19](https://github.com/everruns/bashkit/pull/19)) by @chaliy
* feat(bashkit): fix array edge cases (102 tests passing) ([#18](https://github.com/everruns/bashkit/pull/18)) by @chaliy
* docs: add licensing and attribution files ([#17](https://github.com/everruns/bashkit/pull/17)) by @chaliy
* feat(bashkit): improve spec test coverage from 78% to 100% ([#16](https://github.com/everruns/bashkit/pull/16)) by @chaliy
* docs: add compatibility scorecard ([#15](https://github.com/everruns/bashkit/pull/15)) by @chaliy
* feat(bashkit): Phase 12 - Spec test framework for compatibility testing ([#14](https://github.com/everruns/bashkit/pull/14)) by @chaliy
* docs: update README with project overview ([#13](https://github.com/everruns/bashkit/pull/13)) by @chaliy
* feat(bashkit): Phase 11 - Text processing commands (jq, grep, sed, awk) ([#12](https://github.com/everruns/bashkit/pull/12)) by @chaliy
* feat(bashkit-cli): Phase 10 - MCP server mode ([#11](https://github.com/everruns/bashkit/pull/11)) by @chaliy
* feat(bashkit): Phase 9 - Network allowlist and HTTP client ([#10](https://github.com/everruns/bashkit/pull/10)) by @chaliy
* feat(bashkit): Phase 8 - OverlayFs and MountableFs ([#9](https://github.com/everruns/bashkit/pull/9)) by @chaliy
* feat(bashkit): Phase 7 - Resource limits for sandboxing ([#8](https://github.com/everruns/bashkit/pull/8)) by @chaliy
* feat(bashkit-cli): Add CLI binary for command line usage ([#7](https://github.com/everruns/bashkit/pull/7)) by @chaliy
* feat(bashkit): Phase 5 - Array support ([#6](https://github.com/everruns/bashkit/pull/6)) by @chaliy
* feat(bashkit): Phase 4 - Here documents, builtins, and parameter expansion ([#5](https://github.com/everruns/bashkit/pull/5)) by @chaliy
* feat(bashkit): Phase 3 - Command substitution and arithmetic expansion ([#4](https://github.com/everruns/bashkit/pull/4)) by @chaliy
* feat(bashkit): Phase 2 complete - control flow, functions, builtins ([#3](https://github.com/everruns/bashkit/pull/3)) by @chaliy
* feat(bashkit): Phase 1 - Foundation with variables, pipes, redirects ([#2](https://github.com/everruns/bashkit/pull/2)) by @chaliy
* feat(bashkit): Phase 0 - Bootstrap minimal working shell ([#1](https://github.com/everruns/bashkit/pull/1)) by @chaliy

**Full Changelog**: https://github.com/everruns/bashkit/commits/v0.1.0
