# Changelog

## [Unreleased]

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
