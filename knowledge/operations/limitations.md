---
type: Limitations
title: Known Limitations
description: Intentional gaps, partial features, and Bash and POSIX compatibility stance.
tags:
  - bashkit
  - limitations
  - compatibility
---

# Limitations

## Status
Living document (updated as limitations are added/lifted)

## Summary

The negative spec: what Bashkit deliberately does NOT do (and why), plus
known partial implementations. Absences can't be recovered from code, so
they're recorded here; everything positive is generated or tested instead:

- **Builtin inventory**: generated [`builtins.json`](../status/builtins.json)
  (`just regen-builtins`, drift-checked by `builtins-drift.yml`)
- **Test counts / pass rates**: CI (`spec_tests::bash_spec_tests` suite);
  spec cases in `crates/bashkit/tests/spec_cases/`
- **Resource limit defaults**: `crates/bashkit/src/limits.rs`
- **Hook/binding API surface**: rustdoc + binding type stubs

Intentional-limitation IDs (`L-<AREA>-<NNN>`) are stable: code comments
and docs reference them (like TM-* threat IDs). Never renumber; mark
lifted limitations as removed in the PR that lifts them.
`limitations_doc_format` in `crates/bashkit/tests/integration/` lints the
table format and ID uniqueness; evidence cells naming `l_*` tests must
resolve to functions in `limitations_evidence_tests.rs` (also linted).
`stance` marks rows that are positions rather than testable behaviors.

## Intentional Limitations

By design, these conflict with the sandboxed, virtual, stateless
execution model. Evidence is a threat-model ID, a test, or `stance`
(untestable position).

| ID | Limitation | Why | Evidence |
|----|------------|-----|----------|
| L-PROC-001 | `exec` does not replace the process; `exec cmd` runs cmd then stops execution. fd redirects work; `exec >log 2>&1` routes the shell's output per top-level command, so output written earlier in the same top-level command (not streamed yet) also lands in the log, and output under the redirect reaches a streaming caller only when that command ends. `exec N>&-` closes fd 1 or 2 for the rest of the script: later writes to it are dropped, the command reports `write error: Bad file descriptor` naming itself, and its status is 1. An `exec` redirect inside a subshell applies to the subshell's own output, routed at the subshell boundary; output the subshell wrote before the `exec` still reaches the caller. Inside a `bash -c` child it still does not take effect. `exec &>file` and `exec >&word` are ignored | True process replace would break sandbox containment; there is no real fd 1 to swap, the shell's output is a value returned to the caller | TM-ESC-005, `exec-redirect-shell-output.test.sh` |
| L-PROC-002 | Background jobs run concurrently (`jobs`, `wait -n`, `kill`, `ps`, `pgrep`), but cannot be stopped or resumed: `kill -STOP/-CONT` are ignored, there is no `suspend`/Ctrl-Z, `bg` is a no-op and `fg` just waits; jobs end when `exec()` returns | No terminal process groups in a virtual shell; a job must not outlive the call that owns it (TM-DOS-122) | `l_proc_002_no_job_control` |
| L-PROC-003 | No process spawning; external commands run as builtins. So bash's fork suppression has no equivalent: a `bash -c` that is the last command of its shell raises `$SHLVL` like any other child, where bash execs it in place and keeps the level (`bash -c 'bash -c "echo \$SHLVL"'` in a top-level script prints 3, bash prints 2) | Core sandbox model: no fork/exec escape surface | `l_proc_003_no_process_spawning` |
| L-PROC-004 | `bash`/`sh` child shells nest at most 8 deep (and count against `max_function_depth`); deeper nesting fails `maximum nesting depth exceeded` | Child shells run in-process on one stack; unbounded nesting crashed the process (TM-DOS-125) | `l_proc_004_child_shell_depth` |
| L-PIPE-001 | Pipeline stages stream only from the first stage that runs shell code (loop, group, function, `eval`). Leading single-builtin stages run to completion first and hand over their whole output, so they never get SIGPIPE (`sort big \| head -1` gives `PIPESTATUS` `0 0`, bash `141 0`); `yes`, `seq`, plain `cat`, `grep` and `tr` are the exception and stream. Downstream of a streaming stage, a single builtin reads all its input before it starts, except `head`, `read`, plain `cat`, `grep` (not `-c/-l/-L/-q`, context, `-b`) and `tr` (not `-s`): `while :; do echo; done \| sed p \| head -1` runs until a limit. Pipes hold 4 KiB, not 64 KiB, so a finite loop writing more than that into an early-exiting reader exits 141 where bash would exit 0. Pipelines nested more than 4 subshells deep run their stages in sequence | Builtins return their output as one value, not a stream; streaming them needs a reader/writer `Context`. The smaller pipe bounds how far a producer runs ahead (each byte costs budgeted commands); the nesting cap bounds native stack (TM-DOS-124) | `l_pipe_001_stages_run_sequentially` |
| L-SUDO-001 | `sudo` runs its command as the one sandbox user: `-u`, `-g`, `-i` change nothing, `whoami` still prints `sandbox`, and there is never a password prompt. `busybox APPLET` runs bashkit's builtin of that name, not BusyBox's own applet (flags and messages are bashkit's) | The sandbox has no privilege boundary to cross; the VFS has no permission enforcement (L-FS-002) | `sudo-busybox.test.sh` |
| L-RAND-001 | `uuidgen` makes random (v4) UUIDs only (no `-t`, `-m`, `-s`); `openssl` implements only `rand` | Time/MAC-based UUIDs would leak host identity; a full TLS/crypto toolkit is out of scope | `uuidgen_time_based_unsupported` |
| L-FS-001 | Symlinks are followed, but `..` after a linked directory resolves lexically (`/link/..` is the link's parent, the `cd -L` view), and `ln` without `-s` (and `link`) makes a symlink, not a hard link | The interpreter normalizes paths before the VFS sees them; the VFS has no inodes to share | `symlink.test.sh`, `ln_default_symbolic` |
| L-ROOTFS-001 | Default rootfs is static and read-only: `/proc` has no `self`, pid dirs, `uptime` or live counters; `/etc/passwd` has no root entry; `/dev/zero` yields 1 MiB per read; `/bin`, `/usr/bin` are stubs that dispatch builtins | Host state must not leak (TM-INF-003, TM-ISO-018); fixed values keep runs deterministic | `rootfs_layout`, `threat_etc_passwd_blocked` |
| L-FS-002 | No file permission enforcement in the VFS | Single-tenant virtual FS; permissions would be theater | `l_fs_002_no_permission_enforcement` |
| L-FS-003 | On Windows, `RealFs::symlink()` validates the target but creates an empty host file rather than a symlink/reparse point; pre-existing host symlinks and junctions remain readable subject to containment checks | Windows requires choosing file-vs-directory link semantics and may require link privileges; the portable VFS symlink contract does not carry that host metadata | TM-ESC-033 |
| L-NET-001 | No raw network sockets; HTTP only via `curl`/`wget`/`http` builtins | Allowlist-mediated egress is the only network surface | `l_net_001_no_raw_sockets` |
| L-NET-002 | No DNS resolution; hosts must appear in the allowlist | Resolution would bypass allowlist intent | `l_net_002_default_deny_no_resolution` |
| L-SIG-001 | No signal arrives from outside the sandbox, so a handler only runs for a signal the script sends itself (`kill -SIG $$`, plus EXIT, ERR and DEBUG). Without a handler, a self-sent signal ends the script with 128 + signal | No host signals exist inside the sandbox | `l_sig_001_signal_traps_not_delivered` |
| L-SSH-001 | CA-signed SSH host *certificates* are rejected under `strict_host_key_checking` (default), even when the public key they wrap is a configured trusted key; configure the host's public key directly | No CA trust store exists to validate the signature chain, validity window, principals or critical options; matching the embedded key would grant trust never actually verified | TM-SSH-006, `test_strict_rejects_certificate_even_when_inner_key_is_trusted` |
| L-WASM-001 | **Removed:** JS-host timers now drive `sleep`, builtin `timeout`, execution limits, and tool `timeoutMs` | `gloo-timers` bridges the host event-loop clock without threads or cross-origin isolation | [Browser Package](../runtimes/browser-package.md) |
| L-WASM-002 | Browser build: `executeSync()` cannot run async custom builtins (fails with a clear message); use `execute()` | Single-threaded event loop can't settle a JS `Promise` without yielding | `crates/bashkit-wasm/__test__/bashkit-wasm.test.mjs` |
| L-WASM-003 | Browser build: background jobs (`cmd &`) run synchronously and `awk` file redirects drive the VFS inline; no work runs on a separate thread | `wasm32-unknown-unknown` is single-threaded, `std::thread::spawn`/`tokio::spawn` are unavailable; safe because the in-memory VFS never suspends | `crates/bashkit-wasm/__test__/bashkit-wasm.test.mjs` |
| L-CAPI-001 | C ABI v1 excludes callbacks, custom builtins, streaming, async cancellation, host mounts, transport hooks, snapshots, scripted tools, and external filesystem providers | These require explicit reentrancy, callback lifetime, and dynamic-library unload contracts | [C API](../runtimes/c-api.md), stance |
| L-STREAM-001 | Shell words, variables, command substitution, script source, text-oriented builtins, and JSON tool responses cannot represent arbitrary bytes. Command substitution removes NUL; other text boundaries decode invalid UTF-8 with replacement. Use `StreamData`, binding byte fields, redirects, or byte-oriented builtins for exact data | Bash variables and the parser are text domains; JSON strings are Unicode | `byte_stream_tests`, [Architecture](../foundations/architecture.md) |

### Design Rationale

**Stateless execution model**: scripts run in isolated, stateless
contexts; each command completes before the next begins. Prevents
resource leaks from orphaned work, simplifies limit enforcement, keeps
agent runs deterministic. (`&` background execution + `wait` are
supported within an exec call.)

**bash/sh as virtual re-invocation**: `bash script.sh` / `bash -c` /
`bash -n` re-enter the Bashkit interpreter, same virtual environment,
shared state and limits, never an external process. `bash --version`
reports Bashkit. Security analysis: TM-ESC-015 in
[threat-model.md](../security/threat-model.md).

## POSIX Compliance Stance

Target: IEEE 1003.1-2024 Shell Command Language.

| Category | Status | Notes |
|----------|--------|-------|
| Reserved words, special parameters | Full | All 16 / all 8 |
| Special built-in utilities | Substantial | 14/15; `exec` partial (L-PROC-001); `times` returns zeros; `trap` per L-SIG-001 |
| Quoting, redirections, compound commands, functions | Full | |
| Word expansions | Substantial | Most expansions supported |
| Pipelines and lists | Full | `\|`, `&&`, `\|\|`, `;`, `&`+`wait`, `!`; every stage is a subshell, `shopt -s lastpipe` keeps the last one in the shell (L-PIPE-001: stages are not concurrent) |

## Shell Features

### Not Yet Implemented

| Feature | Priority | Notes |
|---------|----------|-------|
| History expansion | Out of scope | Interactive only |

### Partially Implemented

| Feature | What Works | What's Missing |
|---------|------------|----------------|
| Prefix env assignments | `VAR=val cmd` temporarily sets env for cmd | Array prefix assignments not in env |
| `return` | Basic usage | Return value propagation |
| `time` | Reserved-word pipelines; `-p`, `--`, GNU `-f/-o/-a/-v`; elapsed time, exit status, and Bashkit counters | Host user/system CPU, RSS, and other process metrics are reported as `unavailable`, never fabricated |
| `timeout` | Basic usage | `-k` kill timeout |
| `bash`/`sh` | `-c`, `-n`, `-e`, `-x`, `-u`, `-f`, `-o option`, script files, stdin, `--version`, `--help` | Login shell |
| Expansion errors | `set -u` unbound variables stop the shell | `$((1/0))` evaluates to 0 and `${x!}` expands empty; bash reports them and drops the rest of the line the failing command ends on (`bashbox_line_number_an_expansion_error_drops_the_rest_of_the_line_it_ends_on`, skipped) |
| Process substitution | `<(cmd)` and `>(cmd)` as filenames, nesting, redirections | The path is `/dev/fd/proc_sub_<n>`, not bash's `/dev/fd/63`. Several interpreters can share one filesystem, so a per-shell descending number would let one tenant's substitution overwrite another's. A script that parses the number sees a different one (`scripts/bash-oracle/cases/process-substitution-dev-fd.sh`). A substitution is always its own word: bash joins it to adjacent text (`x<(true)` is `x/dev/fd/63`, `x=<(true)` assigns the path), Bashkit splits it off (`bashbox-process-substitution.test.sh`, skipped). The substituted list runs in a subshell, as in bash |
| Aliases | `alias`/`unalias`, `shopt -s expand_aliases`, trailing-space chaining, recursion guard | Expanded when the simple command runs, not when its line is read: bash parses a whole line (and a function body at definition) before an `alias`/`unalias` on it takes effect; Bashkit applies them to the very next command |
| Named fds / `<>` | `{var}>file`, `{var}<file`, `{var}<>file`, `{var}>>file`, `{var}<<<w`, `{var}<<E`, `{var}>&N`/`<&N` allocate the lowest free fd >= 10 on any command (persisting, as in bash); `{var}>&-` closes; `>&N` with N closed is `Bad file descriptor`; `N<>file`, `N<<<w` for any N | Descriptors are virtual: `N<>file` has no shared read/write offset (writes append); `3<file` also feeds stdin; output to fd 3-9 opened by a command's own redirect (`f 3>&1`, `( ... ) 3>&1`) is dropped; a `Bad file descriptor` message ignores redirect order (`2>&1 >&7`) |
| Locale | UTF-8 text everywhere; `${#v}` counts bytes when `LC_ALL`/`LC_CTYPE`/`LANG` (first non-empty) is `C` or `POSIX` | With no locale variable set bashkit is UTF-8, where bash falls back to C. Under C, `${s:off:len}`, `${s^^}`, `${s#?}` and glob `?` still work on characters, not bytes: shell words are Rust strings and cannot hold a split UTF-8 sequence (L-STREAM-001) |
| Brace expansion | Runs before other expansions on unquoted literal text; lists, nesting, ranges with step and zero padding, quoted/escaped braces and commas | Works on parsed words, not raw text: `$v{1,2}` expands `$v` then appends `1`/`2` (bash reads `$v1`, `$v2`); write `${v}` |

## Builtins

Inventory is generated, see [status/builtins.json](../status/builtins.json)
and the [builtins spec](../foundations/builtins.md). No wholly unimplemented
builtins are currently tracked; partial boundaries follow.

| ID | Tool | Limitation | Evidence |
|----|------|------------|----------|
| L-DATE-001 | date | `TZ` accepts bundled IANA identifiers/aliases only. POSIX rule strings and `:zoneinfo` paths are unsupported and intentionally resolve to UTC because the sandbox has no trusted host zoneinfo filesystem. GNU nanosecond formatting supports the useful `%N`/`%3N`/`%6N`/`%9N` forms, not other widths | `date_timezone_tests` |
| L-MAPFILE-001 | mapfile | `-u FD` reads only fd 0, and the `-C`/`-c` callback is not run; both exit 2 with a message instead of reading the wrong input | `mapfile_bad_option` (param-expansion-gaps spec) |

### CPython runtime (`cpython` feature)

Boundaries of the WebAssembly CPython guest; see
[CPython WebAssembly Runtime](../runtimes/cpython-wasm.md).

| ID | Limitation | Why | Evidence |
|----|------------|-----|----------|
| L-CPY-001 | No subprocesses from Python (`subprocess`, `os.system`, `os.fork`, `os.popen` fail); Python cannot call back into the shell | The guest has no process API; a shell bridge is a deliberate follow-up, not an accident | `processes_unavailable` (cpython_security_tests) |
| L-CPY-002 | No raw network from Python: `socket` cannot connect and `ssl` is not built. HTTP works only through `urllib.request`/`http.client` (and bashkit's `requests`/`httpx`) via the host's egress pipeline (needs `http_client` + an allowlist); responses are buffered up to the response cap; proxy, `ssl` context and client-certificate settings are ignored | No socket imports in the WASI host; HTTP goes through `HttpClient` like `curl`, TLS on the host | TM-PY-CPY-003, `network_unavailable`, `cpython_http_tests` |
| L-CPY-003 | No threads, `multiprocessing`, `ctypes`, native extensions or third-party packages (`requests`/`httpx`/`httpx2` are bashkit's own compact implementations of the common API, see L-CPY-010) | Single-threaded wasm guest with only the bundled pure-Python stdlib | `threads_and_native_code_unavailable` |
| L-CPY-004 | `errno` values are WASI's (`ENOENT` is 44); exception types and the `errno` module agree | wasi-libc numbering; rewriting it would desync the guest's `errno` module | `cpython_integration_tests::missing_script_exits_2`, stance |
| L-CPY-005 | `hash()` of `str`/`bytes` uses one seed baked into the snapshot | The seed is chosen during snapshot initialization; per-call re-seeding would require re-hashing every interned object | stance |
| L-CPY-006 | Deep C-level recursion ends the call with `python3: fatal error: stack overflow` instead of `RecursionError` | The interpreter's wasm stack is bounded (4 MiB); the trap is contained | `deep_c_recursion_is_contained` |
| L-CPY-007 | No interactive REPL; `python3` with no program reads one from stdin | No TTY inside the sandbox | stance |
| L-CPY-008 | Guest memory per call is capped at 1 GiB even if `max_memory` is higher | Pooled instance slots have a fixed maximum size | [CPython WebAssembly Runtime](../runtimes/cpython-wasm.md) |
| L-CPY-010 | `requests` and `httpx` are subsets: no retries, proxies, client certificates, HTTP/2, digest auth, streaming uploads or OPTIONS; bodies are buffered; `AsyncClient` requests run sequentially | Upstream packages cost ~3 s per call to import on Pulley; the subset is preloaded in the snapshot and costs nothing | `requests_lib`, `httpx_lib` in `cpython_http_tests` |
| L-CPY-009 | Stdlib ships as bytecode only: tracebacks show no source line for stdlib frames, `inspect.getsource()` fails on stdlib objects. Modules that cannot work in the guest are not shipped: `ctypes`, `ssl`, `ftplib`, `imaplib`, `poplib`, `smtplib`, `socketserver`, `http.server`, `wsgiref`, `xmlrpc`, `webbrowser`, `bdb`, `pydoc` (so `help()`), `_pyrepl`, `mailbox`, `tty`, `pty`, `bz2`, `lzma`, `compression.zstd`; low-use for agent scripts: `unittest`, `doctest`, `cProfile`, `profile`, `pstats`, `trace`, `tabnanny`, `pyclbr`, `modulefinder`, `pickletools`, `compileall`, `zipapp`, `dbm`, `shelve`, `plistlib`, `wave`, `netrc`, `cmd`, `rlcompleter`, `concurrent.interpreters`; `pdb` is a stand-in (`breakpoint()` prints a notice and continues) | Compiling source on Pulley costs seconds per import; sources would push the crate past the crates.io 10 MiB cap. No FFI, TLS, sockets, TTY or bz2/lzma/zstd C codecs exist. The workload is agents' file-processing scripts; low-use modules cost crate bytes and preload budget | `stdlib_is_bytecode_only` (cpython_integration_tests) |

## Text Processing

What each tool does is covered by its spec tests (all unskipped tests
pass in CI); only divergences and boundaries are recorded here.

| ID | Tool | Limitation | Evidence |
|----|------|------------|----------|
| L-AWK-001 | awk | `for (k in a)` without `PROCINFO["sorted_in"]` follows a model of gawk's array storage replayed from the keys still present; after `delete`, gawk's order also depends on the deleted keys, so it can differ from Debian awk | `order::tests`, `awk_for_in_string_keys` spec |
| L-AWK-002 | awk | `print \| cmd` collects the command's input and runs it at `close()` or exit (as if the pipe were read at once); `\|&` coprocesses, `@include`/`@load`/`@namespace`, MPFR (`-M`), `--profile` and the debugger are not supported | stance |
| L-AWK-003 | awk | Commands (`system()`, pipes) need the shell: through `Bash::exec` they run as `sh -c` in the sandbox; a direct `Builtin::execute` call (embedders) has no shell, so they print a notice and report status 127 | `test_awk_print_redirect_pipe_needs_plan_driver` |
| L-AWK-004 | awk | `mawk` and `nawk` run the gawk dialect: mawk's `-W` options, messages and its `substr`/division-by-zero behavior are not emulated | stance |
| L-JQ-001 | jq | Alternative `//`: jaq errors on `.foo` applied to null instead of returning null (upstream jaq divergence) | 1 skipped spec test |
| L-JQ-002 | jq | Regex natives compile the pattern per filter invocation; mapping `test`/`match`/`split` over many inputs can repeat compilation because jaq's native callback has no per-run cache state | `regex_compat.rs::re_native` |
| L-JQ-003 | jq | User-defined recursion is bounded by a 64 live-evaluator-context ceiling for host stack safety: non-tail recursive defs (`def s: if length==0 then 0 else .[0] + (.[1:]|s) end`) fail past ~62 levels with `jq: error: recursion limit (64) exceeded` (exit 5). Tail-recursive defs are unaffected and run to thousands of levels; unbounded recursion stops at the execution timeout | `jq_non_tail_recursion_is_bounded_on_a_two_mib_stack`, `jq_recursive_filters_stop_without_host_abort_or_hang`, `finite_user_recursion_stays_available` |
| L-JQ-004 | jq | Input number literals are read as 64-bit floats, so jq 1.7's literal preservation is partial: `1.0` and `2.5` print as written, but `1.50` prints `1.5`, `1e2` prints `100.0`, `-0` prints `0` and integers past 2^64 lose digits. Computed numbers print like jq (`1024`, `1e+17`). Integer arithmetic stays exact where jq rounds to doubles | `input_number_literals_keep_their_form_until_computed` |
| L-FMT-001 | fmt | Line breaks come from the uutils Knuth-Plass breaker, not GNU's cost function, so a paragraph can wrap at different words than GNU `fmt` (same width limits, same paragraphs). `-t` indents continuation lines like uutils. GNU `fmt.c` is GPL and cannot be ported into MIT bashkit | `fmt.test.sh` cases marked `bash_diff` |
| L-YQ-001 | yq | Expressions are Bashkit jq expressions; mikefarah/yq-only node, comment, style, anchor, tag, filename, and eval-all operators are not implemented | stance |
| L-YQ-002 | yq | YAML conversion follows YAML 1.1, deterministically sorts mapping keys at the JSON-value boundary, drops comments/style/anchors, and rejects aliases, custom tags, non-string mapping keys, and non-finite numbers rather than expanding graphs or silently corrupting data | `yaml_aliases_are_rejected_before_expansion`, `yaml_tags_and_non_string_keys_fail_closed`, `yaml_duplicate_keys_and_lossy_numbers_fail_closed`, `inplace_update_is_atomic_and_suppresses_stdout` |
| L-PR-001 | pr | `-e`/`--expand-tabs` and `-i`/`--output-tabs` are rejected as unknown options; `-c`/`-v` are accepted but control characters print as is | `pr.rs` header |
| L-YQ-003 | yq | Input/output conversion supports YAML and JSON only; mikefarah/yq's XML, CSV, TOML, properties, HCL, Lua, and INI formats are not exposed through yq | stance |
| L-GREP-001 | grep | `--color`/`--colour`, `--line-buffered` accepted as no-ops | `l_grep_001_noop_flags` |
| L-GREP-002 | grep | No "stray \\ before X" warnings (GNU grep >= 3.8 warns on `\/`, `\d`, ...; the escaped character is matched literally either way). Patterns with back-references run on fancy-regex and match leftmost-first, not POSIX leftmost-longest. Under `-o`, a line that is not valid UTF-8 counts as binary output even when the match itself is valid (GNU checks only the match) | `l_grep_002_no_stray_backslash_warning` |
| L-GREP-003 | grep | `-r`/`-R` visit directory entries in name order; GNU uses readdir order, which depends on the host filesystem (on tmpfs, newest first) | `l_grep_003_recursive_name_order` |
| L-SED-001 | sed | A multi-byte `s`/`y`/address delimiter is accepted (`s≠a≠X≠`); GNU rejects it with "delimiter character is not a single-byte character". Deliberate superset: accepting more never turns a working GNU script into a silently wrong one | `multibyte_delimiters_do_not_panic`, 1 `bash_diff` spec test |
| L-SED-002 | sed | The `e` command and the `s///e` flag (run the pattern space as a shell command) are refused at compile time, not silently ignored | `the_e_command_is_refused_not_silently_ignored` |
| L-SED-003 | sed | `-i` refuses a file that is not valid UTF-8 instead of rewriting it through a lossy decode; without `-i`, non-UTF-8 input is decoded with replacement like every other text builtin (L-STREAM-001) | `in_place_refuses_non_utf8_rather_than_corrupting` |
| L-SED-004 | sed | `\<` and `\>` (GNU word-edge operators) both compile to `\b`; the linear-time `regex` engine has no look-around, so a word start and a word end are not distinguished | `translate_is_positional` |
| L-SED-005 | sed | `D` restarts the script without reading input, so a non-progressing loop such as `sed 'G;D'` never terminates — GNU hangs on it. Restarts are capped at `SED_MAX_CYCLE_STEPS`, after which sed stops with the same `loop limit` warning `b`/`t` loops get, and exits 0. A `D` loop that does make progress is unaffected: restarts and the per-cycle step budget are counted per input line read, so `$!N;P;D` and `:b;$b;N;...;bb` run over any input length | `a_non_progressing_delete_restart_loop_terminates` |
| L-SED-006 | sed | Scripts that carry the stream past a range's end with `n`, `N` or `D` can still diverge from GNU's range bookkeeping in rare shapes (e.g. `sed '$!D;1,2!x'`). A randomized differential sweep of 18,000 generated scripts against GNU sed 4.9 finds 3 such cases; all combine a multi-line pattern space with a range address, a shape absent from ordinary scripts | `ranges_that_the_stream_skipped_past` |
| L-CURL-001 | curl | Spec-test coverage for methods/headers/auth/redirects not ported (needs `http_client` + allowlist in harness); payload behavior has integration and real-curl differential coverage | stance |
| L-CURL-002 | curl/wget | Unknown options are ignored for compatibility, not rejected (real curl/wget error); deliberate leniency | `curl.rs` |
| L-PRINTF-001 | printf | `%(fmt)T` argument `-2` (bash: shell start time) formats the current time; the shell start instant is not tracked. GNU-only `%N` in the time format is expanded to nanoseconds, bash prints it literally | `printf.rs::expand_time_directives` |
| L-OD-002 | od | `-w`/`--width` above 65,536 bytes is rejected, including empty input; zero or type-misaligned widths retain the type-size fallback; output growth is constrained by the shared live-intermediate budget | `od_resource_tests` |
| L-FACTOR-001 | factor | Numbers above 2^64-1 are rejected ("too large"); GNU factor accepts arbitrary precision | `factor.rs::rejects_bad_tokens` |
| L-ENV-001 | umask, ulimit, enable | `umask` is stored and reported (subshell-scoped) but does not yet change VFS file creation modes; `ulimit` reports fixed sandbox values (not host limits) and stores lowered values without enforcing them, real caps are `ExecutionLimits`; `enable -n/-d/-f` (disable or load builtins) are refused | `shellenv.rs`, `process-env-builtins.test.sh` |
| L-DD-001 | dd | Transfer-statistics line reports `0 s, 0 B/s` (no wall-clock rate); each invocation moves at most 64 MiB (`DD_MAX_BYTES`) and an uncounted `/dev/zero`/`/dev/urandom` read stops there with exit 1 | `dd.rs` |
| L-FIND-001 | find | Not implemented: `-ok`/`-okdir` (need an interactive terminal; refused with an error), `-samefile`, `-inum`, `-links`, `-fstype`, `-context`, `-used`. Entries are visited in byte-sorted order, not readdir order. Access/change times read the modification time (the VFS keeps one). `-newerXt` dates use `date -d` parsing. `{} +` batches run at the end of the walk (or every 4096 paths), so their output follows find's own output; `-execdir ... {} +` is not flushed per directory. `-printf` `%i`/`%D` print 0, `%F` prints `vfs`, `%k`/`%b` assume 4 KiB blocks; `-ls` always shows `HH:MM`, never the year, and directories have size 0 | `find/mod.rs`, `find.test.sh` |
| L-FIND-002 | find | `-exec` commands run with no stdin (GNU passes find's stdin through) | `find/mod.rs` |
| L-FIND-003 | find | `-fprint`/`-fprint0`/`-fprintf`/`-fls` truncate their file before the walk (as GNU does) but write the collected text when the walk ends, so the file stays empty while `-exec` commands run | `find_fprint_family` |
| L-DU-001 | du | The VFS has no block allocation: sizes are apparent sizes (default output is ceil(bytes/1024)), hard links are not de-duplicated, and `-x`/`-l`/`-S`/`-L`/`-P`/`-0` are accepted but have no effect | `disk.rs` tests |
| L-OD-001 | od | Floating-point types (`-t f`, `-e`, `-f`, `-F`) are refused with an error; `-S`/`--strings` and `--traditional` are accepted but ignored | `test_od_float_rejected` |
| L-FOLD-001 | fold | Every character counts one column (no East Asian wide-char widths); `-b` counts UTF-8 bytes | `fold.rs` |
| L-STR-001 | strings | Accepts dash-prefixed filenames (e.g. `-data.bin`), so only a lone unknown short option (`-Q`) is rejected as invalid; GNU rejects `-data.bin` too | `strings.rs` |
| L-MAKE-001 | make | `$(eval)` and `$(file)` stop make with `function '...' is not supported (L-MAKE-001)` | Both write makefile state or files mid-expansion; expansion restarts on a `$(shell)`/`$(wildcard)` miss, so side-effecting functions would run more than once | `l_make_001_eval_and_file_unsupported` |
| L-MAKE-002 | make | No built-in implicit rules (as `make -r`), pattern rules do not chain through intermediate files, and `vpath`/`VPATH` are not searched | The sandbox has no compiler for built-in rules to call; chaining and directory search are the next steps if real makefiles need them | `l_make_002_no_builtin_or_chained_rules` |
| L-MAKE-003 | make | Recipes always run in the sandbox shell one at a time: `SHELL` is ignored, `-j` is accepted but sequential, and `$(MAKE)` fails at MAKELEVEL 4 (`recursive make depth exceeds 4`; GNU has no cap) | Recipes must stay inside the interpreter; each nested make runs on the caller's stack (TM-DOS-126) | `l_make_003_sequential_sandbox_shell` |

Safety boundaries (enforced, not bugs): printf width/precision caps,
output buffer caps, getline file-cache cap, shared regex size limit, runtime regex
cache cap (64 entries and 1 MB retained pattern text per evaluator),
curl/wget timeouts clamped to [1, 600] s, multipart field-name
sanitization, redirect handling hardened against credential leaks, and curl's
aggregate data/multipart request body capped at 10 MB. Repeated mixed curl
`-d`/`--data`, `--data-raw`, `--data-binary`, and `--data-urlencode` parts retain
command-line order; `-G`/`--get` moves their joined payload to the query string.
The jq input boundary alone accepts literal U+0000..U+001F controls inside JSON
strings; controls outside strings and malformed quoting remain invalid. The
`json` builtin, serde defaults, tool JSON contracts, `--argjson`, and
`--jsonargs` stay strict. See [jq Input Compatibility](../foundations/jq.md).
Archive compression is intentionally in-process and limited to gzip and bzip2;
GNU tar selectors for xz, lzip, lzma, compress, and zstd remain unsupported.

## CLI

Divergences in the `bashkit` binary's one-shot (`-c` / script) mode.
Positional parameters, stdin forwarding, and streaming output all work;
what remains is how stdin is obtained.

| ID | Limitation | Why | Evidence |
|----|------------|-----|----------|
| L-CLI-002 | Host stdin is read to EOF *before* execution, not lazily when a command asks for it, and only when stdin is not a terminal. `cmd \| bashkit -c 'echo hi'` waits for the writer to finish even though the script never reads; `--no-stdin` opts out | The interpreter takes its stdin as a value up front (`ExecOptions::stdin`); lazy reads would need a reader-backed fd 0 in the sandbox. Capped at 10 MiB and bounded by the selected execution timeout | `crates/bashkit-cli/tests/cli_oneshot.rs` |

## Terminal

Divergences in the in-process terminal (`terminal` feature,
[In-Process Terminal](../integrations/in-process-terminal.md)) from a real
PTY running GNU bash and vim.

| ID | Limitation | Why | Evidence |
|----|------------|-----|----------|
| L-TERM-001 | `vi` is a subset editor: normal/insert/command-line modes, counts, common motions and operators, undo/redo, `/` search, `:s`, `:w :q :wq :x :cq`. No visual mode, named registers, macros, splits, vimrc, or `:!` shell escape; unsupported keys are ignored | Enough to edit config and source files from an agent or a browser terminal without vendoring an editor; `:!` would re-enter the shell from inside a running command | `l_term_001_vi_is_a_subset` |
| L-TERM-002 | Command stdin is wired to the terminal only for `read` and `select`, which wait for a typed line. Other commands with no file (`cat`, `head`, `wc`) get EOF instead of waiting; only programs that read the device directly (`vi`) see keystrokes | Same root as L-CLI-002: the interpreter takes stdin as a value before a command starts. `read`/`select` reach the device themselves; a reader-backed fd 0 for every command is not built | `l_term_002_cat_does_not_wait_for_terminal_input` |
| L-TERM-003 | **Removed:** Ctrl-C now drops the running command mid-way (a running `sleep 5` stops at once) and `$?` is 130 | The exec future races an interrupt `Notify` and reuses the timeout path's `clear_cancelled_execution_state` cleanup | `terminal::tests::ctrl_c_interrupts_a_running_command` |
| L-TERM-004 | Inside a terminal session, `less` and `more` page even when stdout is redirected (`less file > out` shows the pager and writes nothing to `out`). Outside a session they are always cat-like | Builtins cannot see whether their stdout is a pipe or redirect; TTY state is per-session, not per-fd-redirect | `builtins::pager::tests::pagers_are_cat_like_without_terminal` |
| L-TERM-005 | `nano` is a subset editor: typing, arrows/Home/End/PageUp/PageDown, `^O` write out, `^X` exit with save prompt, `^K`/`^U` line cut/paste, `^W` search, `^G` help, `^C` position. No undo, multiple buffers, syntax highlighting, mouse, spell check, or nanorc; unknown keys are ignored | Covers editing a config or commit message from an agent or browser terminal; the rest is a large editor that `vi` already backs up | `builtins::nano::tests::edits_saves_and_exits` |

## Parser

- Single-quoted strings are completely literal (correct behavior)
- Some complex nested structures may hit the parser timeout
- Very long pipelines may cause stack issues
- Bounded by configurable limits: timeout, fuel, input size, AST depth

## Lifting a Limitation / Adding One

1. Add a spec test demonstrating it, marked `### skip: reason`
   (or an expected-fail differential test)
2. Add a row here, with an `L-*` ID if it's an intentional decision
3. When lifting: un-skip the test, delete the row, update referencing
   code comments in the same PR
