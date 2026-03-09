# Changelog

## [Unreleased]

### What's Changed

* fix: prevent fuzz-found panics on multi-byte input ([#513](https://github.com/everruns/bashkit/pull/513))

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
* chore: run maintenance checklist (012-maintenance) ([#508](https://github.com/everruns/bashkit/pull/508))
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
