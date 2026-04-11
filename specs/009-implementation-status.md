# 009: Implementation Status

## Status
Living document (updated as features change)

## Summary

Tracks what's implemented, what's not, and why. Single source of truth for
feature status across Bashkit.

## Intentionally Unimplemented Features

These features are **by design** not implemented. They conflict with Bashkit's
stateless, virtual execution model or pose security risks.

| Feature | Rationale | Threat ID |
|---------|-----------|-----------|
| `exec` (process replace) | Cannot replace shell process in sandbox; `exec cmd` runs cmd and stops execution instead | TM-ESC-005 |
| ~~Background execution (`&`)~~ | ~~Stateless model~~ | Implemented: `&` and `wait` now supported |
| Job control (`bg`, `fg`, `jobs`) | Requires process state; interactive feature | - |
| Symlink following | Prevents symlink loop attacks and sandbox escape | TM-DOS-011 |
| Process spawning | External commands run as builtins, not subprocesses | - |
| Raw network sockets | Only allowlisted HTTP via curl builtin | - |

### Design Rationale

**Stateless Execution Model**: Bashkit runs scripts in isolated, stateless
contexts. Each command executes to completion before the next begins. This
design:
- Prevents resource leaks from orphaned background processes
- Simplifies resource accounting and limits enforcement
- Enables deterministic execution for AI agent workflows

**Symlinks**: Stored in the virtual filesystem but not followed during path
resolution. The `ln -s` command works, and `read_link()` returns targets, but
traversal is blocked. This prevents:
- Infinite symlink loops (e.g., `a -> b -> a`)
- Symlink-based sandbox escapes (e.g., `link -> /etc/passwd`)

**Security Exclusions**: `exec` is excluded because it would replace the shell
process, breaking sandbox containment.

**bash/sh Commands**: The `bash` and `sh` commands are implemented as virtual
re-invocations of the Bashkit interpreter, NOT external process spawning. This
enables common patterns like `bash script.sh` while maintaining security:
- `bash --version` returns Bashkit version (not host bash)
- `bash -c "cmd"` executes within the same virtual environment
- `bash -n script.sh` performs syntax checking without execution
- Variables set in `bash -c` affect the parent (shared interpreter state)
- Resource limits are shared/inherited from parent execution

See [006-threat-model.md](006-threat-model.md) threat TM-ESC-015 for security analysis.

## POSIX Compliance

Bashkit implements IEEE 1003.1-2024 Shell Command Language. See
[008-posix-compliance.md](008-posix-compliance.md) for design rationale.

### Compliance Level

| Category | Status | Notes |
|----------|--------|-------|
| Reserved Words | Full | All 16 reserved words supported |
| Special Parameters | Full | All 8 POSIX parameters supported |
| Special Built-in Utilities | Substantial | 14/15 implemented (1 excluded: exec) |
| Regular Built-in Utilities | Full | Core set implemented |
| Quoting | Full | All quoting mechanisms supported |
| Word Expansions | Substantial | Most expansions supported |
| Redirections | Full | All POSIX redirection operators |
| Compound Commands | Full | All compound command types |
| Functions | Full | Both syntax forms supported |

### POSIX Special Built-in Utilities

| Utility | Status | Notes |
|---------|--------|-------|
| `.` (dot) | Implemented | Execute commands in current environment; PATH search, positional params |
| `:` (colon) | Implemented | Null utility (no-op, returns success) |
| `break` | Implemented | Exit from loop with optional level count |
| `continue` | Implemented | Continue loop with optional level count |
| `eval` | Implemented | Construct and execute command |
| `exec` | Partial | `exec cmd` runs and stops; fd redirects work; no true process replace (see [Intentionally Unimplemented](#intentionally-unimplemented-features)) |
| `exit` | Implemented | Exit shell with status code |
| `export` | Implemented | Export variables to environment |
| `readonly` | Implemented | Mark variables as read-only |
| `return` | Implemented | Return from function with status |
| `set` | Implemented | Set options and positional parameters |
| `shift` | Implemented | Shift positional parameters |
| `times` | Implemented | Display process times (returns zeros in virtual mode) |
| `trap` | Implemented | EXIT, ERR handlers; signal traps stored but no signal delivery in virtual mode |
| `unset` | Implemented | Remove variables and functions |

### Pipelines and Lists

| Operator | Status | Description |
|----------|--------|-------------|
| `\|` | Implemented | Pipeline |
| `&&` | Implemented | AND list |
| `\|\|` | Implemented | OR list |
| `;` | Implemented | Sequential execution |
| `&` | Implemented | Background execution with `wait` |
| `!` | Implemented | Pipeline negation |

## Spec Test Coverage

**Total spec test cases:** 2463 (2435 pass, 28 skip)

| Category | Cases | In CI | Pass | Skip | Notes |
|----------|-------|-------|------|------|-------|
| Bash (core) | 1939 | Yes | 1916 | 23 | `bash_spec_tests` in CI |
| AWK | 126 | Yes | 126 | 0 | loops, arrays, -v, ternary, field assign, getline, %.6g, delete, dev-stderr |
| Grep | 95 | Yes | 95 | 0 | -z, -r, -a, -b, -H, -h, -f, -P, --include, --exclude, binary detect, rg |
| Sed | 78 | Yes | 78 | 0 | hold space, change, regex ranges, -E |
| JQ | 121 | Yes | 120 | 1 | reduce, walk, regex funcs, --arg/--argjson, combined flags, input/inputs, env |
| Python | 60 | Yes | 58 | 2 | embedded Python (Monty) |
| TypeScript | 44 | Yes | 42 | 2 | embedded TypeScript (ZapCode) |
| **Total** | **2463** | **Yes** | **2435** | **28** | |

### Bash Spec Tests Breakdown

| File | Cases | Notes |
|------|-------|-------|
| alias.test.sh | 15 | alias expansion (1 skipped) |
| arith-dynamic.test.sh | 14 | dynamic arithmetic contexts |
| arithmetic-base-expansion.test.sh | 4 | arithmetic base `#` expansion with parameter operators |
| arithmetic.test.sh | 75 | includes logical, bitwise, compound assign, increment/decrement, `let` builtin, `declare -i` arithmetic |
| array-slicing.test.sh | 8 | array slice operations |
| array-splat.test.sh | 2 | `"${arr[@]}"` individual element splatting in assignments |
| arrays.test.sh | 34 | indices, `${arr[@]}` / `${arr[*]}`, negative indexing `${arr[-1]}`, quoted expansion |
| assoc-arrays.test.sh | 22 | associative arrays `declare -A` |
| awk-printf-width.test.sh | 4 | AWK printf width/precision memory limits |
| background.test.sh | 2 | background job handling |
| bash-c-exports.test.sh | 3 | `bash -c` exported variable visibility |
| bash-command.test.sh | 25 | bash/sh re-invocation |
| bash-flags.test.sh | 13 | bash `-e`, `-x`, `-u`, `-f`, `-o option` flags |
| bash-source-var.test.sh | 2 | BASH_SOURCE in `bash /path/script.sh` |
| bash-stdin-pipe.test.sh | 3 | piped stdin forwarding to bash script/command |
| bc.test.sh | 15 | `bc` arbitrary-precision calculator, scale, arithmetic, sqrt |
| brace-expansion.test.sh | 20 | {a,b,c}, {1..5}, for-loop brace expansion |
| brace_expansion_lookahead.test.sh | 4 | lookahead cap in brace expansion parser |
| checksum.test.sh | 10 | md5sum, sha256sum, sha1sum |
| chown-kill.test.sh | 7 | chown, kill builtins |
| cmd-suggestions.test.sh | 4 | command suggestions on typos |
| cmdsub_depth_unquoted.test.sh | 3 | subst depth limit in unquoted cmdsub |
| column.test.sh | 5 | column alignment |
| comm.test.sh | 6 | comm column comparison |
| command-not-found.test.sh | 9 | unknown command handling |
| command-subst.test.sh | 32 | includes backtick substitution, nested quotes in `$()` |
| command.test.sh | 9 | `command -v`, `-V`, function bypass |
| compgen-path.test.sh | 2 | compgen PATH completion |
| conditional-short-circuit.test.sh | 4 | `&&`/`\|\|` short-circuit inside `[[ ]]` with `set -u` |
| conditional.test.sh | 29 | `[[ ]]` conditionals, `=~` regex, BASH_REMATCH, glob `==`/`!=` |
| control-flow.test.sh | 60 | if/elif/else, for, while, case `;;`/`;&`/`;;&`, select, trap ERR, `[[ =~ ]]` BASH_REMATCH, compound input redirects |
| cuttr.test.sh | 39 | cut and tr commands, `-z` zero-terminated |
| date.test.sh | 39 | format specifiers, `-d` relative/compound/epoch/RFC2822, `-R`, `-I`, `%N` (2 skipped) |
| declare.test.sh | 23 | `declare`/`typeset`, `-i`, `-r`, `-x`, `-a`, `-p`, `-n` nameref, `-l`/`-u` case conversion |
| df.test.sh | 3 | disk free reporting |
| diff.test.sh | 6 | line diffs |
| dirstack.test.sh | 12 | `pushd`, `popd`, `dirs` directory stack operations |
| du.test.sh | 4 | disk usage reporting |
| echo.test.sh | 24 | escape sequences |
| empty-bodies.test.sh | 8 | empty loop/function bodies |
| env.test.sh | 3 | environment variable operations |
| errexit.test.sh | 11 | set -e tests |
| eval-bugs.test.sh | 4 | regression tests for eval/script bugs |
| exec-command.test.sh | 6 | exec builtin |
| exec-fd-redirect.test.sh | 4 | exec fd redirects through VFS targets |
| exec-fd-variable.test.sh | 2 | exec `{var}>&-` fd-variable redirect syntax |
| exit-status.test.sh | 28 | exit code propagation |
| expr.test.sh | 13 | `expr` arithmetic, string ops, pattern matching, exit codes |
| extglob.test.sh | 15 | `@()`, `?()`, `*()`, `+()`, `!()` extended globs |
| file.test.sh | 8 | file type detection |
| fileops.test.sh | 28 | `mktemp`, `-d`, `-p`, template |
| find.test.sh | 22 | file search |
| functions.test.sh | 26 | local dynamic scoping, nested writes, FUNCNAME call stack, `caller` builtin |
| getopts.test.sh | 9 | POSIX option parsing, combined flags, silent mode |
| glob-options.test.sh | 13 | dotglob, nocaseglob, failglob, nullglob, noglob, globstar |
| glob_match_cap.test.sh | 4 | glob match call cap in remove_pattern_glob |
| globs.test.sh | 9 | for-loop glob expansion, recursive `**` |
| gzip.test.sh | 2 | gzip/gunzip compression |
| headtail.test.sh | 14 | |
| heredoc-edge.test.sh | 15 | heredoc edge cases |
| heredoc.test.sh | 13 | heredoc variable expansion, quoted delimiters, file redirects, `<<-` tab strip |
| herestring.test.sh | 8 | here-string `<<<` |
| hextools.test.sh | 4 | od/xxd/hexdump (3 skipped) |
| history.test.sh | 2 | history builtin |
| indirect-expansion.test.sh | 4 | indirect expansion with default operator |
| less.test.sh | 3 | less pager |
| ln.test.sh | 5 | `ln -s`, `-f`, symlink creation |
| ls.test.sh | 8 | ls directory listing |
| memory_budget_desync.test.sh | 3 | memory budget save/restore in subshell/cmdsub |
| nameref-assoc.test.sh | 7 | nameref with associative arrays |
| nameref.test.sh | 23 | nameref variables (1 skipped) |
| negative-tests.test.sh | 13 | error conditions |
| nl.test.sh | 14 | line numbering |
| nounset.test.sh | 7 | `set -u` unbound variable checks, `${var:-default}` nounset-aware |
| numfmt.test.sh | 13 | numfmt number formatting |
| parse-errors.test.sh | 18 | syntax error detection (4 skipped) |
| paste-flags.test.sh | 3 | paste combined short flags |
| paste.test.sh | 4 | line merging with `-s` serial and `-d` delimiter |
| path.test.sh | 18 | basename, dirname, `realpath` canonical path resolution |
| pipes-redirects.test.sh | 26 | includes stderr redirects |
| printenv.test.sh | 2 | printenv builtin |
| printf.test.sh | 32 | format specifiers, array expansion, `-v` variable assignment, `%q` shell quoting |
| procsub.test.sh | 14 | process substitution |
| quote.test.sh | 42 | quoting edge cases |
| read-builtin.test.sh | 16 | `read` builtin, IFS splitting, `-r`, `-a` (array), `-n` (nchars), here-string |
| recursive-cmdsub.test.sh | 3 | recursive function calls inside `$()` command substitution |
| regex-limit.test.sh | 1 | regex size limits in sed, grep, awk |
| replace_pattern_limit.test.sh | 3 | global pattern replacement result size cap |
| script-exec.test.sh | 14 | script execution by path, $PATH search, exit codes |
| seq.test.sh | 12 | `seq` numeric sequences, `-w`, `-s`, decrement, negative |
| set-allexport.test.sh | 5 | set -a / allexport |
| shell-grammar.test.sh | 23 | shell grammar edge cases |
| sleep.test.sh | 9 | sleep timing |
| sortuniq.test.sh | 49 | sort `-f`/`-n`/`-r`/`-u`/`-V`/`-t`/`-k`/`-s`/`-c`/`-h`/`-M`/`-m`/`-z`/`-o`, uniq `-c`/`-d`/`-u`/`-i`/`-f` |
| source.test.sh | 19 | source/., function loading, PATH search, positional params |
| stat.test.sh | 7 | stat file information |
| string-ops.test.sh | 14 | string replacement (prefix/suffix anchored), `${var:?}`, case conversion |
| strings.test.sh | 6 | strings extraction |
| subprocess-isolation.test.sh | 8 | subprocess variable isolation |
| subshell.test.sh | 14 | subshell execution |
| tar.test.sh | 8 | tar archive operations |
| tee.test.sh | 6 | tee output splitting |
| temp-binding.test.sh | 10 | temporary variable bindings `VAR=val cmd` |
| test-operators.test.sh | 29 | file/string tests, `-nt`/`-ot`/`-ef` file comparisons |
| test-tty.test.sh | 5 | tty detection tests |
| textrev.test.sh | 14 | `tac` reverse line order, `rev` reverse characters, `yes` repeated output |
| time.test.sh | 11 | Wall-clock only (user/sys always 0) |
| timeout.test.sh | 16 | |
| tree.test.sh | 1 | tree directory listing |
| type.test.sh | 15 | `type`, `which`, `hash` builtins |
| unicode.test.sh | 17 | unicode handling (3 skipped) |
| unset-exported-var.test.sh | 3 | unset removes exported vars from env (1 skipped) |
| var-op-test.test.sh | 26 | variable operations (1 skipped) |
| variables.test.sh | 97 | includes special vars, prefix env, PIPESTATUS, trap EXIT, `${var@Q}`, `\<newline>` line continuation, PWD/HOME/USER/HOSTNAME/BASH_VERSION/SECONDS, `set -x` xtrace, `shopt` builtin, nullglob, `set -o`/`set +o` display, `trap -p` |
| wait.test.sh | 2 | wait builtin |
| watch.test.sh | 2 | watch command |
| wc.test.sh | 20 | word count |
| word-split.test.sh | 39 | IFS word splitting (10 skipped) |
| xargs.test.sh | 7 | xargs command (1 skipped) |
| blackbox-edge-cases.test.sh | 89 | edge cases for quoting, expansion, redirection, error handling |
| blackbox-exploration.test.sh | 199 | broad coverage exploration: builtins, pipelines, subshells, traps |

## Shell Features

### Not Yet Implemented

Features that may be added in the future (not intentionally excluded):

| Feature | Priority | Notes |
|---------|----------|-------|
| History expansion | Out of scope | Interactive only |

**Recently Implemented (moved from this table):**
- Coprocesses `coproc`: `coproc [NAME] cmd` with NAME array FDs + `read -u` support
- Background execution `&`: async execution with `wait` builtin

### Partially Implemented

| Feature | What Works | What's Missing |
|---------|------------|----------------|
| Prefix env assignments | `VAR=val cmd` temporarily sets env for cmd | Array prefix assignments not in env |
| `local` | Declaration | Proper scoping in nested functions |
| `return` | Basic usage | Return value propagation |
| Heredocs | Basic, `<<-` tab strip, variable expansion | — |
| Arrays | Indexing, `[@]`/`[*]` as separate args, `${!arr[@]}`, `+=`, slice `${arr[@]:1:2}`, assoc `declare -A`, compound init `declare -A m=([k]=v)` | — |
| `trap` | EXIT, ERR handlers | No signal delivery in virtual mode (INT, TERM stored but not triggered) |
| `set -o pipefail` | Pipeline returns rightmost non-zero exit code | — |
| `time` | Wall-clock timing | User/sys CPU time (always 0) |
| `timeout` | Basic usage | `-k` kill timeout |
| `bash`/`sh` | `-c`, `-n`, `-e`, `-x`, `-u`, `-f`, `-o option`, script files, stdin, `--version`, `--help` | Login shell |

## Builtins

### Implemented

**148 core builtins + 8 feature-gated = 156 total**

`echo`, `printf`, `cat`, `nl`, `cd`, `pwd`, `true`, `false`, `exit`, `test`, `[`,
`export`, `set`, `unset`, `local`, `source`, `.`, `read`, `shift`, `break`,
`continue`, `return`, `grep`, `sed`, `awk`, `jq`, `sleep`, `head`, `tail`,
`basename`, `dirname`, `realpath`, `readlink`, `mkdir`, `mktemp`, `mkfifo`, `rm`, `cp`, `mv`,
`touch`, `chmod`, `chown`, `ln`, `wc`,
`sort`, `uniq`, `cut`, `tr`, `paste`, `column`, `diff`, `comm`, `date`,
`wait`, `curl`, `wget`, `timeout`, `command`, `getopts`,
`type`, `which`, `hash`, `declare`, `typeset`, `let`, `kill`, `shopt`,
`trap`, `caller`, `mapfile`, `readarray`, `seq`, `tac`, `rev`, `yes`, `expr`,
`time` (keyword), `whoami`, `hostname`, `uname`, `id`, `ls`, `rmdir`, `find`, `xargs`, `tee`,
`:` (colon), `eval`, `readonly`, `times`, `bash`, `sh`,
`od`, `xxd`, `hexdump`, `strings`, `base64`, `md5sum`, `sha1sum`, `sha256sum`,
`tar`, `gzip`, `gunzip`, `file`, `less`, `stat`, `watch`,
`env`, `printenv`, `history`, `df`, `du`,
`pushd`, `popd`, `dirs`, `bc`, `tree`,
`clear`, `fold`, `expand`, `unexpand`, `envsubst`, `join`, `split`,
`assert`, `dotenv`, `glob`, `log`, `retry`, `semver`, `verify`,
`compgen`, `csv`, `fc`, `help`, `http`, `iconv`, `json`,
`numfmt`, `parallel`, `patch`, `rg`, `template`, `tomlq`, `yaml`, `zip`, `unzip`,
`alias`, `unalias`,
`git` (requires `git` feature, see [010-git-support.md](010-git-support.md)),
`python`, `python3` (requires `python` feature, see [011-python-builtin.md](011-python-builtin.md)),
`ts`, `typescript`, `node`, `deno`, `bun` (requires `typescript` feature, see [016-zapcode-runtime.md](016-zapcode-runtime.md))

### Not Yet Implemented

None currently tracked.

## Text Processing

### AWK Limitations

- Regex literals in function args: `gsub(/pattern/, replacement)` ✅
- Array assignment in split: `split($0, arr, ":")` ✅
- Complex regex patterns

**Skipped Tests: 0** (all AWK tests pass)

**Implemented Features:**
- For/while/do-while loops with break/continue
- Postfix/prefix increment/decrement (`i++`, `++i`, `i--`, `--i`)
- Arrays: `arr[key]=val`, `"key" in arr`, `for (k in arr)` (sorted), `delete arr[k]`
- `-v var=value` flag for variable initialization
- Ternary operator `(cond ? a : b)`
- Field assignment `$2 = "X"`, `$0 = "x y z"` re-splits fields
- `getline` — reads next input record into `$0`
- ORS (output record separator)
- `next`, `exit` with code
- Power operators `^`, `**`
- Printf formats: `%x`, `%o`, `%c`, width specifier
- `match()` (RSTART/RLENGTH), `gensub()`, `sub()`, `gsub()`
- `!$1` logical negation, `-F'\t'` tab delimiter
- `%.6g` number formatting (OFMT-compatible)
- Deterministic `for-in` iteration (sorted keys)

**Safety Limits:**
- Printf width/precision capped to prevent buffer exhaustion
- Output buffer size limited to prevent unbounded accumulation
- Getline file cache limited to prevent memory exhaustion
- Regex patterns size-limited (shared with sed/grep)

### Sed Limitations

**Skipped Tests: 0** (all previously-skipped sed tests now pass)

**Recently Implemented:**
- Grouped commands: `{cmd1;cmd2}` blocks with address support
- Branching: `b` (unconditional), `t` (on substitution), `:label`
- `Q` (quiet quit) — exits without printing current line
- Step addresses: `0~2` (every Nth line)
- `0,/pattern/` addressing (first match only)
- Hold space with grouped commands: `h`, `H` in `{...}` blocks
- Hold space commands: `h` (copy), `H` (append), `g` (get), `G` (get-append), `x` (exchange)
- Change command: `c\text` line replacement
- Regex range addressing: `/start/,/end/` with stateful tracking
- Numeric-regex range: `N,/pattern/`
- Extended regex (`-E`), nth occurrence, address negation (`!`)
- Ampersand `&` in replacement, `\n` literal newline in replacement

### Grep Limitations

**Skipped Tests: 0** (all grep tests pass)

**Implemented Features:**
- Basic flags: `-i`, `-v`, `-c`, `-n`, `-o`, `-l`, `-w`, `-E`, `-F`, `-q`, `-m`, `-x`
- Context: `-A`, `-B`, `-C` (after/before/context lines)
- Multiple patterns: `-e`
- Include/exclude: `--include=GLOB`, `--exclude=GLOB` for recursive search
- Pattern file: `-f` (requires file to exist in VFS)
- Filename control: `-H` (always show), `-h` (never show)
- Byte offset: `-b`
- Null-terminated: `-z` (split on `\0` instead of `\n`)
- Recursive: `-r`/`-R` (uses VFS read_dir)
- Binary handling: `-a` (filter null bytes), auto-detect binary (null byte → "Binary file ... matches")
- Perl regex: `-P` (regex crate supports PCRE features)
- No-op flags: `--color`, `--line-buffered`

### JQ Limitations

**Skipped Tests (1):**

| Feature | Count | Notes |
|---------|-------|-------|
| Alternative `//` | 1 | jaq errors on `.foo` applied to null instead of returning null |

**Recently Fixed:**
- `try`/`catch` expressions now work (jaq handles runtime errors)
- `debug` passes through values correctly (stderr not captured)
- Combined short flags (`-rn`, `-sc`, `-snr`)
- `--arg name value` and `--argjson name value` variable bindings
- `--indent N` flag no longer eats the filter argument
- `env` builtin now exposes bashkit shell env vars to jaq runtime
- `input`/`inputs` iterators wired to shared input stream

### Curl Limitations

Tests not ported (requires `--features http_client` and URL allowlist):

- HTTP methods (GET, POST, PUT, DELETE)
- Headers (`-H`)
- Data payloads (`-d`, `--data-raw`)
- Output options (`-o`, `-O`)
- Authentication (`-u`)
- Follow redirects (`-L`)
- Silent mode (`-s`)

**Implemented:**
- curl: Timeout (`-m`/`--max-time`) - per-request timeout support
- curl: Connection timeout (`--connect-timeout`) - connection establishment timeout
- wget: Timeout (`-T`/`--timeout`) - per-request timeout support
- wget: Connection timeout (`--connect-timeout`) - connection establishment timeout

**Safety Limits:**
- Timeout values are clamped to [1, 600] seconds (1 second to 10 minutes)
- Prevents resource exhaustion from very long timeouts or instant timeouts
- Multipart field names sanitized to prevent header injection
- Redirect handling hardened against credential leaks
- Transparent Ed25519 request signing when bot-auth feature enabled

## Parser Limitations

- Single-quoted strings are completely literal (correct behavior)
- Some complex nested structures may timeout
- Very long pipelines may cause stack issues
- Configurable limits: timeout, fuel, input size, AST depth

## Filesystem

- Virtual filesystem only (InMemoryFs, OverlayFs, MountableFs)
- Optional real filesystem access via `RealFs` backend (`realfs` feature flag)
  - Readonly and read-write modes
  - Root overlay or mount-at-path
  - Path traversal prevention via canonicalization
  - CLI: `--mount-ro` / `--mount-rw` flags
- Symlinks stored but not followed (see [Intentionally Unimplemented](#intentionally-unimplemented-features))
- No file permissions enforcement

## Network

- HTTP only (via `curl` builtin when enabled)
- URL allowlist required
- No raw sockets
- No DNS resolution (host must be in allowlist)

## Resource Limits

Default limits (configurable):

| Limit | Default |
|-------|---------|
| Commands | 10,000 |
| Loop iterations | 100,000 |
| Function depth | 100 |
| Interpreter memory | 10MB (`max_memory` / `maxMemory`) |
| Output size | 10MB |
| Parser timeout | 5 seconds |
| Parser operations (fuel) | 100,000 |
| Input size | 10MB |
| AST depth | 100 |

## Language Bindings

### JavaScript/Node.js (`@everruns/bashkit`)

NAPI-RS bindings in `crates/bashkit-js/`. TypeScript wrapper in `wrapper.ts`.

| Class | Methods | Notes |
|-------|---------|-------|
| `Bash` | `executeSync`, `execute`, `cancel`, `reset` | Core interpreter |
| `Bash` (VFS) | `readFile`, `writeFile`, `mkdir`, `exists`, `remove` | Direct VFS access via NAPI |
| `Bash` (helpers) | `ls`, `glob` | Shell-based convenience wrappers |
| `BashTool` | `executeSync`, `execute`, `cancel`, `reset` | Interpreter + tool metadata |
| `BashTool` (metadata) | `name`, `shortDescription`, `description`, `help`, `systemPrompt`, `inputSchema`, `outputSchema`, `version` | LLM tool contract |
| `BashTool` (helpers) | `readFile`, `writeFile`, `exists`, `ls`, `glob` | Shell-based VFS wrappers |

**Platform matrix:** macOS (x86_64, aarch64), Linux (x86_64, aarch64), Windows (x86_64), WASM

**Tests:** `crates/bashkit-js/__test__/` — VFS roundtrip, interop, error handling, security (100+ white/black-box tests covering TM-DOS, TM-ESC, TM-INF, TM-INT, TM-ISO, TM-UNI, TM-INJ, TM-NET)

### Python (`bashkit`)

PyO3 bindings in `crates/bashkit-python/`. See [013-python-package.md](013-python-package.md).

### Examples

| Example | Description |
|---------|-------------|
| `examples/bashkit-pi/` | Pi coding agent extension — replaces bash/read/write/edit tools with bashkit VFS |

## Testing

### Security Tests

**Unicode byte-boundary tests:** 68 tests in `unicode_security_tests.rs`

| Section | Tests | Component | Verified |
|---------|-------|-----------|----------|
| Awk byte-boundary | 15 | `awk.rs` | Panics caught by catch_unwind |
| Sed byte-boundary | 8 | `sed.rs` | Panics caught by catch_unwind |
| Expr byte-boundary | 6 | `expr.rs` | Panics caught by catch_unwind |
| Printf byte-boundary | 5 | `printf.rs` | Panics caught by catch_unwind |
| Cut/tr byte-boundary | 6 | `cuttr.rs` | Silent data loss |
| Interpreter byte-boundary | 2 | `interpreter/mod.rs` | Wrong result, no panic |
| Sed extended | 7 | `sed.rs` | Panics caught |
| Zero-width chars | 5 | VFS path validation | Correct rejection |
| Homoglyph confusion | 4 | VFS | Accepted risk |
| Normalization | 3 | VFS | Matches Linux behavior |
| Combining marks | 4 | Builtins | Length limits bound damage |
| Bidi/tag/annotation | 3 | Various | Detection gaps documented |
| Cross-component E2E | 5 | Pipeline | End-to-end multi-byte flows |

See [006-threat-model.md](006-threat-model.md) TM-UNI-001 through TM-UNI-019.

### Comparison with Real Bash

```bash
cargo test --test spec_tests -- bash_comparison_tests --ignored
```

Runs each spec test against both Bashkit and real bash, reporting differences.

### Contributing

To add a known limitation:
1. Add a spec test that demonstrates the limitation
2. Mark the test with `### skip: reason`
3. Update this document
4. Optionally file an issue for tracking
