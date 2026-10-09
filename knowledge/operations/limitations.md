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
| L-DIAG-001 | Shell diagnostic names show at most 1,024 UTF-8 bytes of `$0` or the current source filename; the actual `$0` is unchanged. Prefix amplification can abort a request at the shared live-memory/work cap, including redirected or streamed diagnostics | Prevent attacker-controlled diagnostic names from multiplying into unchecked host allocations (TM-DOS-103) | `diagnostic_prefix_tests` |
| L-PROC-001 | `exec` does not replace the process; `exec cmd` runs cmd then stops execution. fd redirects work; `exec >log 2>&1` routes the shell's output per top-level command (each element of a top-level `;`/`&&`/`||` list counts as one), so output written earlier in the same top-level command (not streamed yet) also lands in the log, and output under the redirect reaches a streaming caller only when that command ends. `exec N>&-` closes fd 1 or 2 for the rest of the script: later writes to it are dropped, the command reports `write error: Bad file descriptor` naming itself, and its status is 1. An `exec` redirect inside a subshell applies to the subshell's own output, routed at the subshell boundary; output the subshell wrote before the `exec` still reaches the caller. Inside a `bash -c` child it still does not take effect. `exec &>file` and `exec >&word` are ignored | True process replace would break sandbox containment; there is no real fd 1 to swap, the shell's output is a value returned to the caller | TM-ESC-005, `exec-redirect-shell-output.test.sh` |
| L-PROC-002 | Background jobs run concurrently (`jobs`, `wait -n`, `kill`, `ps`, `pgrep`), but cannot be stopped or resumed: `kill -STOP/-CONT` are ignored, there is no `suspend`/Ctrl-Z, `bg` is a no-op and `fg` just waits; jobs end when `exec()` returns | No terminal process groups in a virtual shell; a job must not outlive the call that owns it (TM-DOS-122) | `l_proc_002_no_job_control` |
| L-PROC-003 | No process spawning; external commands run as builtins. bash's fork suppression is modeled only for `$SHLVL` (see `NoforkScope` in the interpreter): a `bash`/`sh` child bash would exec in place keeps the level, but another command run there still sees the unlowered value (bash's `bash -c 'printenv SHLVL'` prints one less than the child's level), nor do `env`/`timeout`/`xargs` starting a shell, and a `SHLVL=n` prefix on the in-place command counts | Core sandbox model: no fork/exec escape surface | `l_proc_003_no_process_spawning` |
| L-PROC-005 | Coprocesses run concurrently with bash's fd numbers and cleanup, with these gaps: a coproc fd as a compound command's stdin (`while read l; do ...; done <&${C[0]}`) is read to end of input up front; `read -n`/`-N` on a coproc fd take a whole line (the rest of it is lost); writing to a coproc that exited is silently dropped (bash takes SIGPIPE); a finished coproc stays (NAME set, output readable) until `wait` reaps it, where bash reaps on SIGCHLD; coprocs end with the `exec()` call; an empty `<&"${C[0]}"` reports `ambiguous redirect` without the quotes where bash says `Bad file descriptor` | Deterministic, sandboxed job model (no SIGCHLD, no process outliving the call) | `coproc_tests.rs` |
| L-PROC-004 | `bash`/`sh` child shells nest at most 8 deep (and count against `max_function_depth`); deeper nesting fails `maximum nesting depth exceeded` | Child shells run in-process on one stack; unbounded nesting crashed the process (TM-DOS-125) | `l_proc_004_child_shell_depth` |
| L-HIST-001 | `fc` with no `-l`/`-s` (and `fc -e EDITOR`) fails `fc: editing history is not supported in bashkit` instead of opening an editor on the selected commands; `fc -s` and `fc -e -` rerun them | No terminal and no external editor process in the sandbox | `history-fc.test.sh` |
| L-PIPE-001 | Pipeline stages stream only from the first stage that runs shell code (loop, group, function, `eval`). Leading single-builtin stages run to completion first and hand over their whole output, so they never get SIGPIPE (`sort big \| head -1` gives `PIPESTATUS` `0 0`, bash `141 0`); `yes`, `seq`, plain `cat`, `grep` and `tr` are the exception and stream. Downstream of a streaming stage, a single builtin reads all its input before it starts, except `head`, `read`, plain `cat`, `grep` (not `-c/-l/-L/-q`, context, `-b`) and `tr` (not `-s`): `while :; do echo; done \| sed p \| head -1` runs until a limit. Pipes hold 4 KiB, not 64 KiB, so a finite loop writing more than that into an early-exiting reader exits 141 where bash would exit 0. Pipelines nested more than 4 subshells deep run their stages in sequence | Builtins return their output as one value, not a stream; streaming them needs a reader/writer `Context`. The smaller pipe bounds how far a producer runs ahead (each byte costs budgeted commands); the nesting cap bounds native stack (TM-DOS-124) | `l_pipe_001_stages_run_sequentially` |
| L-SUDO-001 | `sudo` runs its command as the one sandbox user: `-u`, `-g`, `-i` change nothing, `whoami` still prints `sandbox`, and there is never a password prompt. `busybox APPLET` runs bashkit's builtin of that name, not BusyBox's own applet (flags and messages are bashkit's) | The sandbox has no privilege boundary to cross; the VFS has no permission enforcement (L-FS-002) | `sudo-busybox.test.sh` |
| L-RAND-001 | `uuidgen` makes random (v4) UUIDs only (no `-t`, `-m`, `-s`); `openssl` implements only `rand` | Time/MAC-based UUIDs would leak host identity; a full TLS/crypto toolkit is out of scope | `uuidgen_time_based_unsupported` |
| L-FS-001 | Symlinks are followed, but `..` after a linked directory resolves lexically (`/link/..` is the link's parent, the `cd -L` view), and `ln` without `-s` (and `link`) makes a symlink, not a hard link | The interpreter normalizes paths before the VFS sees them; the VFS has no inodes to share | `symlink.test.sh`, `ln_default_symbolic` |
| L-ROOTFS-001 | Default rootfs is static and read-only: `/proc` has no `self`, pid dirs, `uptime` or live counters; `/etc/passwd` has no root entry; `/dev/zero` yields 1 MiB per read; `/dev/full` is a plain file, so writes to it succeed (bash: `No space left on device`); `/bin`, `/usr/bin` are stubs that dispatch builtins | Host state must not leak (TM-INF-003, TM-ISO-018); fixed values keep runs deterministic | `rootfs_layout`, `threat_etc_passwd_blocked` |
| L-FS-002 | No file permission enforcement in the VFS | Single-tenant virtual FS; permissions would be theater | `l_fs_002_no_permission_enforcement` |
| L-FS-003 | On Windows, `RealFs::symlink()` validates the target but creates an empty host file rather than a symlink/reparse point; pre-existing host symlinks and junctions remain readable subject to containment checks | Windows requires choosing file-vs-directory link semantics and may require link privileges; the portable VFS symlink contract does not carry that host metadata | TM-ESC-033 |
| L-FS-004 | File names cannot contain control characters (newline, tab, ESC, ...) or bidi overrides: creating `$'a\nb'` fails with `unsafe character U+000A in path component`, so tools never print the quoted forms (`stat -c %N` as `'a'$'\n''b'`, `md5sum`'s `\` escape) for such names | Control characters in names let output forge extra lines or terminal escapes (TM-DOS-015) | `bashbox-checksum.test.sh`, `bashbox-stat.test.sh` (skipped) |
| L-NET-001 | No raw network sockets; HTTP only via `curl`/`wget`/`http` builtins | Allowlist-mediated egress is the only network surface | `l_net_001_no_raw_sockets` |
| L-NET-002 | No DNS resolution; hosts must appear in the allowlist | Resolution would bypass allowlist intent | `l_net_002_default_deny_no_resolution` |
| L-ARITH-001 | A variable's value read as an arithmetic expression or subscript never runs command substitution: `x='a[$(cmd)]'; echo $((x))` and `unset "a[$(cmd)]"` built from data are arithmetic syntax errors, and an indirect target holding `$(`, `` ` ``, `<(` or `>(` (`r='a[$(cmd)]'; ${!r}`) is `invalid variable name`, where bash runs `cmd` | Values are data; re-parsing them as code is the classic bash arithmetic injection, and a sandboxed agent shell must not execute text it only read | stance |
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
| Prefix env assignments | `VAR=val cmd` temporarily sets env for cmd; `B=(x) cmd` binds the text `(x)` | An existing array `B` shadows the `B=(x)` text inside `cmd` |
| `return` | Basic usage | Return value propagation |
| `time` | Reserved-word pipelines; `-p`, `--`, GNU `-f/-o/-a/-v`; `TIMEFORMAT` (`%[p][l]R`, `%%`, empty = no report); elapsed time, exit status, and Bashkit counters | Host user/system CPU, RSS, and other process metrics are reported as `unavailable`, never fabricated (also `TIMEFORMAT`'s `%U`/`%S`/`%P`) |
| `timeout` | Basic usage | `-k` kill timeout |
| `bash`/`sh` | `-c` anywhere among the options (the command is the first operand, then `$0` and the arguments), `-n`, every `set` letter, `-o`/`+o option`, `-O`/`+O shopt`, `-`/`--` end options, script files, stdin, `--version`, `--help`; `export SHELLOPTS` passes the `set -o` options on; `env -i bash` still gets `PATH`, `PWD`, `IFS`, `PS2`, `PS4`, `SHELLOPTS` | Login shell; `-v` (and `set -v`) is accepted but input lines are not echoed; `$-` holds `c` for every `-c` run (the Oils harness runs bash from stdin, where it is `s`); `-i` gives an interactive child (see Interactive builtins row) but no terminal, line editing, job control or `PS2` continuation prompt |
| Interactive builtins | `history` (`-c -d -a -n -r -w -p -s`, `HISTSIZE`/`HISTFILESIZE`, `histappend`), `fc -l/-n/-r/-s`, `complete`/`compgen`/`compopt` (actions, `-W -G -F -C -X -P -S`, `-o` options, `complete -p` in bash's format), `bind` (bash's default emacs bindings, `-l -p -P -s -S -v -V -X -q -u -r -x -m -f`), prompt decoding (`${x@P}`, `PS1` printed to stderr and `PROMPT_COMMAND` run per line of a `bash -i` stdin script, `\!` `\#` `\u` `\h` `\w` `\t` `\D{fmt}` ...) | `fc` without `-s`/`-l` cannot open an editor (L-HIST-001); no `!` history expansion; `bind` changes are recorded but nothing reads keys, and the vi keymaps list no defaults; `complete -p` lists specs in definition order (bash: hash order); history numbers after `HISTSIZE` trims count from 1 in the trimmed list; `PS2` is never printed; `-A service` and `-A disabled` list nothing; `compgen -A user`/`-u` read the sandbox `/etc/passwd` or fall back to the sandbox user; `compgen -b` lists bashkit's builtin registry (`awk`, `grep`, ...), not bash's list |
| Expansion errors | `set -u` unbound variables stop the shell | `$((1/0))` evaluates to 0 and `${x!}` expands empty; bash reports them and drops the rest of the line the failing command ends on (`bashbox_line_number_an_expansion_error_drops_the_rest_of_the_line_it_ends_on`, skipped) |
| Process substitution | `<(cmd)` and `>(cmd)` as filenames, nesting, redirections | Paths are bash's `/dev/fd/63`, `/dev/fd/62`, ... per command, resolved in the shell's own fd namespace, never the shared VFS (TM-ISO-028; `process_substitution_fd_tests.rs`). The data is buffered, not a pipe: `<(cmd)` runs to completion before the command, `>(cmd)` runs after it. `readlink` prints `pipe:[N]` with the fd as N, not an inode; `ls /dev/fd` lists none of them; `exec 3> >(cmd)` runs `cmd` right after the `exec` with no input; later writes to fd 3 are dropped. `[[ -e <(true) ]]` and `for x in <(a)` do not parse a substitution there. A substitution is always its own word: bash joins it to adjacent text (`x<(true)` is `x/dev/fd/63`, `x=<(true)` assigns the path), Bashkit splits it off (`bashbox-process-substitution.test.sh`, skipped). The substituted list runs in a subshell, as in bash |
| `cd` / `pwd` | `-L`/`-P`/`--`, `cd -` (prints the directory), `CDPATH`, `pwd -P`, bash's errors (`too many arguments`, `OLDPWD not set`, `cd nope/..` fails) | Assigning or unsetting `PWD` sticks until the next `cd`; there is no host startup directory, so a shell never starts inside a symlinked path and `pwd` at startup cannot show one |
| Special variables | `$_` (last argument, empty after an assignment-only command, `/bin/bash` at startup), `$BASHPID` (a fresh virtual pid per subshell, `$$` unchanged), `SHELLOPTS`/`BASHOPTS` (computed, readonly), `UID`/`EUID`/`PPID` readonly integers; `$LINENO` in `for (( ))` clauses is the `for` line | The environment starts empty (sandbox identity), so `env` does not list `PWD` and `HOME` is always set; `HISTFILE` is set only by `bash -i` (and read/written through the VFS only); a top-level `(( ))` keeps the line of the command before it for `$LINENO` (the AST stores no position for it) |
| `GLOBIGNORE` | Colon-separated patterns (`:` inside `[...]` kept), matched per path component, `.`/`..` dropped, non-empty value turns `dotglob` on, an emptied match list falls back to the literal word / `nullglob` / `failglob` | `shopt -u globskipdots` is accepted but has no effect: `.*` never yields `.` and `..` (bash 5.2 does once `globskipdots` is off), since the VFS lists no such entries |
| Aliases | `alias`/`unalias`, `shopt -s expand_aliases`, trailing-space chaining, recursion guard | Expanded when the simple command runs, not when its line is read: bash parses a whole line (and a function body at definition) before an `alias`/`unalias` on it takes effect; Bashkit applies them to the very next command |
| Named fds / `<>` | `{var}>file`, `{var}<file`, `{var}<>file`, `{var}>>file`, `{var}<<<w`, `{var}<<E`, `{var}>&N`/`<&N` allocate the lowest free fd >= 10 on any command (persisting, as in bash); `{var}>&-` closes; `>&N` with N closed is `Bad file descriptor`; `N<>file`, `N<<<w` for any N | Descriptors are virtual: `N<>file` has no shared read/write offset (writes append, so `read -n 1 <&3; echo . >&3` does not overwrite the second byte); a `mkfifo` file is not a pipe, so `exec 8<> fifo` round-trips nothing; a fd moved for one command (`: 6>&7-`) is back afterwards, where bash 5.2 leaves it closed (a bash bug Oils pins); `3<file` also feeds stdin; output to fd 3-9 opened by a command's own redirect (`f 3>&1`, `( ... ) 3>&1`) is dropped; a `Bad file descriptor` message ignores redirect order (`2>&1 >&7`); output to a stdout closed by the command's own `>&-` reports `write error: Bad file descriptor` without naming the command, since the redirect is applied after it ran |
| Locale | UTF-8 text everywhere; `${#v}` counts bytes when `LC_ALL`/`LC_CTYPE`/`LANG` (first non-empty) is `C` or `POSIX` | With no locale variable set bashkit is UTF-8, where bash falls back to C. Under C, `${s:off:len}`, `${s^^}`, `${s#?}` and glob `?` (in patterns and pathname expansion: `LC_ALL=C; echo _?_` still matches `_μ_`) still work on characters, not bytes, and an invalid `LC_CTYPE` gives no `setlocale` warning: shell words are Rust strings and cannot hold a split UTF-8 sequence (L-STREAM-001) |
| Diagnostic prefix | `$0: line N: ` on interpreter and shell-builtin messages, `$0` = sourced file inside `source`; `eval` text counts lines from the eval's own line; syntax errors read like bash (`near unexpected token `T'` plus the offending line, `unexpected end of file` on the line after the last, `unexpected EOF while looking for matching `"'`), named `bash: -c`, the script path, `$0: eval` or the sourced file | `(( ))`, `[[ ]]`, `{ }` and `( )` carry no span: `(( ))`/`[[ ]]` keep the line of the list or command before them, `{ }`/`( )` report line 1 at the top level when the construct itself (not a command inside it) fails; a function defined in a sourced file and called from the script names the caller's file (bash names the defining file); `${3}` under `set -u` reports `$3` (bash: `3`, only bare `$3` gets the `$`); `command not found` keeps the sandbox "Did you mean" / unavailable-command hint bash does not print; parse errors outside the grammar (`((` unterminated, `$(` unterminated, `[[ ]]` operand errors other than an unclosed `=~` group) keep bashkit's wording, and `x=(` with no `)` is accepted; under the CLI's `-c` a syntax error after commands that ran is named `bash: line N` (bash: `bash: -c: line N`); an `eval` command spanning several lines counts from its first line (bash: its last), and a function defined inside `eval` numbers `$LINENO` from the eval text, not the script |
| Stderr routing | A command's own errors (`command not found`, a missing `./path`, `return`/`exit`/`source`/`getopts`/`builtin` usage errors) follow its redirects; stderr written inside `$(...)` reaches the outer stderr ahead of the expanding command's own, outside that command's redirects and inside a compound command's ; under `2>&1` (or `&>`, `>f 2>&1`) on a group, function, script, `eval`, `source` or `bash` child each inner command's stderr merges in write order between commands | within one command stdout and stderr are separate values, so a builtin that writes both (`cat a missing b 2>&1`) gives all its stdout first; a streaming caller gets each command's stdout and stderr as one chunk (the CLI writes stdout first, so `set -x` lines follow the output they trace on a terminal); `exit 1 2` drops the rest of the line and carries on like `bash SCRIPT` even under `bash -c` (bash -c exits 1) |
| `set -x` | Simple commands with bash's word quoting (`''`, `'a b'`, `$'\001'`), assignments (`+ a=1`, `+ arr=(1 $(x))` as written), function calls, `[[ ]]` primaries; PS4 expanded (`$LINENO`, `$(...)`) with its first character repeated per `$(...)`/`eval`/`source`/trap level | `for`/`case`/`select` headers are not traced; a quoted `[[ == ]]` pattern shows only glob characters escaped (bash escapes each quoted character); `unset PS4` still prefixes `+ ` (PS4 is not seeded, so unset and never-set look alike); `set -o verbose` does not echo input lines; a `<(...)` body is not traced |
| Tilde expansion | A leading unquoted `~`, `~+`, `~-`; in assignments (and `name=` arguments such as `local x=a:~`) after `=` and each `:`; `${x:-~}` operands when unquoted; pattern operands (`${x#~}`, `${x//~/y}`) even quoted; quoted, escaped or expansion-joined prefixes stay literal (`~"x"`, `\~`, `~$v`) | No user database: only `~root` (`/root`) and `~<sandbox user>` (`/home/<user>`) resolve, other `~name` stays literal; no `~N`/`~+N` directory-stack forms; a `${v-~:~}` operand inside an assignment gets only the leading tilde; `x=~` inside `a=( ... )` is expanded (bash leaves it) |
| DEBUG trap | Fires before simple commands, `(( ))`, `[[ ]]`, `case`, each `for` round and arithmetic-`for` clause, and each pipeline stage (run in the shell, before the stages); `$?` survives it; `return` in it returns from the function, `exit` ends the shell; inherited by functions, subshells and `$(...)` only under `set -T` (plus once on function entry), a handler set inside a function stays | `$BASH_COMMAND` is not set; `$LINENO` inside the handler for a top-level `(( ))`/`[[ ]]` is the previous command's line (no span on those nodes); `shopt -s extdebug` (status 2 skipping the command, `declare -F` file/line) is not implemented |
| Arrays | Sparse indexed and associative arrays, slices by index (`${a[@]:1:2}`, negative offsets from the highest index), `a[i]` subscripts evaluated arithmetically on read and write (side effects once), `${!ref}` through `a[i]`/`a[@]` targets with any operator | Quotes inside an indexed subscript are removed before a read (`${a['1']}` is element 1 in bashkit, an arithmetic syntax error in bash); `B=(x) cmd` binds the text `(x)` as bash does, but an existing array `B` still shadows that binding inside `cmd`; writing through a nameref to `a[@]` (`ref=(x)`) is not refused like bash |
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

Safety boundaries (enforced, not bugs): printf width/precision caps (10,000;
`printf "%10239s"` fails, so `bashbox_du_human_rounding` is skipped),
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

## Oils Spec Misses

Every case the Oils spec harness (`just oils-spec`, see
[Oils Spec Pass Rate](oils-spec.md)) still misses, by cause. Counts are for
the latest saved run; a case listed here is a known gap, not a regression.

| Cause | Cases | Why | Spec cases |
|-------|-------|-----|------------|
| Harness artifacts (the harness runs bashkit with `-c`, bash from stdin) | 6 | Under `-c` bash ends the string on a fatal expansion error with 127 and `$-` holds `c`; bashkit's top level keeps script semantics; the harness host has no `$HOME`/passwd shell, and bashkit seeds `PIPESTATUS` | `fatal-errors #${undef} with nounset`; `fatal-errors #Unrecoverable: ${undef?message}`; `pipeline #Initial value of PIPESTATUS is empty string`; `sh-options #$- with pipefail`; `vars-bash #$SHELL is set to what is in /etc/passwd`; `vars-special #$HOME is NOT set` |
| Host files, processes and signals | 37 | Need host `PATH` programs or `$REPO_ROOT` files the VFS cannot see, real processes, job specs, signals from another process, `ulimit` on real fds, hard links, exec bits or `umask` applied by the host kernel (L-SIG-001, L-PROC-002, L-FS-001, L-FS-002, L-ROOTFS-001) | `assign #Escaped = in command name`; `assign-extended #declare -F with shopt -s extdebug and main file`; `assign-extended #declare -F with shopt -s extdebug prints more info`; `background #Signal message for killed background job`; `background #Start background pipeline, wait %job_spec`; `background #Wait for job and PIPESTATUS`; `background #Wait for job and PIPESTATUS - cat`; `background #wait -n with arguments - arguments are respected`; `background #wait for N parallel jobs and check failure`; `background #wait with invalid arg`; `bugs #'echo' and printf fail on writing to full disk`; `bugs #other builtins fail on writing to full disk`; `builtin-bracket #-ef`; `builtin-cd #pwd in symlinked dir on shell initialization`; `builtin-meta #command -p (hide tool in custom path)`; `builtin-meta #command -p (override existing program)`; `builtin-meta #command -v doesn't find non-executable file`; `builtin-process #ulimit -n limits file descriptors`; `builtin-trap #exit codes for traps are isolated`; `builtin-trap #trap EXIT, sleep, SIGINT: non-interactively`; `builtin-trap #trap USR1, sleep, SIGINT: non-interactively`; `builtin-trap #traps are cleared in subshell (started with &)`; `builtin-trap-bash #Combine DEBUG trap and USR1 trap`; `builtin-trap-bash #Combine ERR trap and USR1 trap`; `builtin-type #type of relative path`; `builtin-umask #'umask 0002' sets the umask`; `builtin-umask #set umask with symbolic mode: g-w,o-w`; `divergence #builtin cat crashes a subshell (#2530)`; `glob #glob can expand to command and arg`; `history-expand #!! expansion`; `process-sub #Process sub from shell to stdin`; `redirect #<> for read/write named pipes`; `sh-options #interactive shell starts with emacs mode on`; `sh-options #noclobber on <>`; `sh-usage #LC_CTYPE=invalid`; `sh-usage #Set LC_ALL LC_CTYPE LC_COLLATE LANG - affects glob ?`; `var-op-patsub #When LC_ALL=C, pattern ? doesn't match multibyte character` |
| Bytes that are not UTF-8 (L-STREAM-001) | 8 | Words and variables are text; `\377` and invalid sequences become U+FFFD | `bugs #file with NUL byte`; `builtin-printf #printf octal backslash escapes`; `quote #$'' octal escapes don't have leading 0`; `serialize #printf %q invalid unicode`; `serialize #printf %q unprintable`; `unicode #OSH source code doesn't have to be valid Unicode (like other shells)`; `var-op-len #String length with incomplete utf-8`; `var-op-len #String length with invalid utf-8 continuation bytes` |
| Out of scope by design | 11 | ERR trap combinations are a separate track; `$(...)` read from data into arithmetic stays refused (L-ARITH-001); ble.sh's recursive arithmetic passes the 50-level cap (TM-DOS-026) | `array-assign #LHS array is protected with shopt -s eval_unsafe_arith, e.g. 'a[$(echo 2)]'`; `ble-idioms #recursive arith: recursion`; `bugs #command execution $(echo 42 \| tee PWNED) not allowed`; `bugs #unset doesn't allow command execution`; `builtin-trap-bash #Combine DEBUG trap and ERR trap`; `builtin-trap-err #set -o errtrace: trap ERR with &`; `builtin-trap-err #trap ERR pipelines without simple commands`; `builtin-trap-err #trap ERR with "atoms": assignment (( [[`; `nameref #a[expr] in nameref`; `var-ref #Var Ref Code Injection $(tee PWNED)`; `var-ref #var ref TO array with arbitrary subscripts` |
| Bash bugs or quirks Oils pins | 7 | bash 5.2 behavior that Oils marks as a bug or that depends on bash internals (a moved fd not restored, `read` and `mapfile` on a directory, brace expansion of a var-only item, mixed-case char ranges, `[^]]` in patsub) | `brace-expansion #Mixed case char expansion is invalid`; `brace-expansion #double expansion with literal and simple var`; `brace-expansion #double expansion with simple var -- bash bug`; `builtin-read #mapfile from directory (bash doesn't handle errors)`; `builtin-read #read bash bug`; `redirect #1>&2- (Bash bug: fail to restore closed fd)`; `var-op-patsub #patsub with [^]]` |
| Aliases expanded per command, not per line | 9 | bashkit parses the whole input before expanding aliases, so an alias defined on the same line, an alias for `{`/`(`, a here-doc or loop split across aliases, and `{ls;\n}` / `failglob` on one line do not match bash's line-by-line reading | `alias #Alias can be defined and used on a single line`; `alias #Loop split across alias in another way`; `alias #Loop split across both iterative and recursive aliases`; `alias #alias for left brace`; `alias #alias for left paren`; `alias #define and use alias on a single line`; `alias #here doc inside alias`; `glob-bash #shopt -s failglob behavior on single line with semicolon`; `parse-errors #} on the second line` |
| Parser gaps | 9 | Constructs bashkit's parser reads differently: `a[` with no `]` as a command word, a nested array literal, `for ((;;)) { }`, `for i` with `in` on the next line, a redirect on `(( ))`, `!(` without `extglob`, and extglob words (`$*@(...)`, `[+()]`, `[[ !(x) ]]` before `shopt -s extglob`) | `array-assign #More fragments like a[  a[5  a[5 +  a[5 + 3]`; `divergence #!( as negation and subshell versus extended glob - #2463`; `dparen #(( )) with redirect`; `extglob-files #Extended glob in same word as array`; `extglob-match #Turning extglob on changes the meaning of [[ !(str) ]] in bash`; `for-expr #Accepts { } syntax too`; `parse-errors #array literal inside array is a parse error`; `toysh #char class / extglob`; `toysh-posix #for loop parsing - http://landley.net/notes.html#04-03-2020` |
| Globbing gaps | 6 | `shopt -u globskipdots` and `GLOBIGNORE=.:..` never list `.`/`..` (the VFS read_dir has no such entries), a no-match extglob prints its pattern with escapes, `[C\-D]` in a pathname glob holds no literal `-` (it does in `case` and `[[ ]]`), and `failglob` in an array literal | `extglob-files #no match`; `extglob-files #noglob`; `glob #glob with escaped - in char class`; `glob #shopt -u globskipdots shows . and ..`; `glob-bash #shopt -s failglob in array literal context`; `globignore #Ignore .:..` |
| Arrays and namerefs | 8 | `${a[b=2]}` evaluates read subscripts without assignment, a temporary `a[0 + 1]=x cmd` element persists, `assoc["${a[@]}"]` does not join, `builtin`/`command` before `declare a=(..)` is not a declaration, `ref=(..)` through `declare -n ref='a[@]'` is not refused, and ble.sh's `eval` array shift idiom | `arith #Side Effect in Array Indexing`; `array-assign #Multiple LHS array words`; `array-assoc #Indexed array as key of associative array coerces to string (without shopt -s strict_array)`; `assign-deferred #is 'builtin' prefix and array allowed?  OSH is smarter`; `assign-deferred #is 'command' prefix and array allowed?  OSH is smarter`; `ble-idioms #shift unshift reverse`; `builtin-meta-assign #builtin declare a=(x y) is allowed`; `nameref #a[@] in nameref` |
| Other open gaps | 13 | `$LINENO` of `(( ))`/`[[ ]]` needs a span on those AST nodes (the serialized snapshot fixtures pin the AST shape); `set -v` / `bash -v` echo nothing; `compgen -A builtin` lists bashkit's whole builtin registry; tilde in `${undef-~:~}` inside an assignment; `[[ $HOME =~ ~ ]]` quoting; `set -e` inside a function called under `!`; a function's redirect is expanded after its body runs, not before; `1>& "$@"` with several words; a nested `${a:-${a:-"1 2" "3 4"}5}` field tree; `unset` of a dynamic local from a subshell | `ble-unset #[bash_unset] local-unset / dynamic-unset for localvar on nested-context`; `builtin-completion #compgen -A builtin`; `builtin-trap-bash #trap DEBUG with non-compound commands`; `dbracket #tilde expansion with =~ (confusing)`; `errexit #set -e in function #2`; `redirect-command #Redirect in function body is evaluated multiple times`; `sh-usage #sh - and sh -- stop flag processing`; `tilde #x=${undef-~:~}`; `var-sub #Descriptor redirect to bad "$@"`; `var-sub-quote #part_value tree with multiple words`; `vars-special #$LINENO in ((`; `xtrace #set -o verbose prints unevaluated code`; `ysh-builtin-private #compgen -A builtin doesn't find private builtins` |

## Lifting a Limitation / Adding One

1. Add a spec test demonstrating it, marked `### skip: reason`
   (or an expected-fail differential test)
2. Add a row here, with an `L-*` ID if it's an intentional decision
3. When lifting: un-skip the test, delete the row, update referencing
   code comments in the same PR
