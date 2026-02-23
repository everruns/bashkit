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
| `exec` builtin | Cannot replace shell process in sandbox; breaks containment | TM-ESC-005 |
| `trap` builtin | Stateless model - no persistent handlers; no signal sources in virtual environment | - |
| Background execution (`&`) | Stateless model - no persistent processes between commands | TM-ESC-007 |
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
process, breaking sandbox containment. `trap` is excluded because signal
handlers require persistent state (conflicts with stateless model) and there
are no signal sources in the virtual environment. Scripts should use exit-code-based error
handling instead.

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
| Special Built-in Utilities | Substantial | 13/15 implemented (2 excluded) |
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
| `exec` | **Excluded** | See [Intentionally Unimplemented](#intentionally-unimplemented-features) |
| `exit` | Implemented | Exit shell with status code |
| `export` | Implemented | Export variables to environment |
| `readonly` | Implemented | Mark variables as read-only |
| `return` | Implemented | Return from function with status |
| `set` | Implemented | Set options and positional parameters |
| `shift` | Implemented | Shift positional parameters |
| `times` | Implemented | Display process times (returns zeros in virtual mode) |
| `trap` | **Excluded** | See [Intentionally Unimplemented](#intentionally-unimplemented-features) |
| `unset` | Implemented | Remove variables and functions |

### Pipelines and Lists

| Operator | Status | Description |
|----------|--------|-------------|
| `\|` | Implemented | Pipeline |
| `&&` | Implemented | AND list |
| `\|\|` | Implemented | OR list |
| `;` | Implemented | Sequential execution |
| `&` | Parsed only | Runs synchronously (stateless model) |
| `!` | Implemented | Pipeline negation |

## Spec Test Coverage

**Total spec test cases:** 1085 (997 pass, 88 skip)

| Category | Cases | In CI | Pass | Skip | Notes |
|----------|-------|-------|------|------|-------|
| Bash (core) | 673 | Yes | 624 | 49 | `bash_spec_tests` in CI |
| AWK | 90 | Yes | 73 | 17 | loops, arrays, -v, ternary, field assign |
| Grep | 82 | Yes | 79 | 3 | now with -z, -r, -a, -b, -H, -h, -f, -P, --include, --exclude |
| Sed | 65 | Yes | 53 | 12 | hold space, change, regex ranges, -E |
| JQ | 108 | Yes | 100 | 8 | reduce, walk, regex funcs, --arg/--argjson, combined flags |
| Python | 58 | Yes | 50 | 8 | **Experimental.** VFS bridging, pathlib, env vars |
| **Total** | **1076** | **Yes** | **980** | **96** | |

### Bash Spec Tests Breakdown

| File | Cases | Notes |
|------|-------|-------|
| arithmetic.test.sh | 29 | includes logical operators |
| arrays.test.sh | 20 | includes indices, `${arr[@]}` / `${arr[*]}` expansion |
| background.test.sh | 4 | |
| bash-command.test.sh | 34 | bash/sh re-invocation |
| brace-expansion.test.sh | 21 | {a,b,c}, {1..5}, for-loop brace expansion |
| column.test.sh | 10 | column alignment |
| command.test.sh | 9 | `command -v`, `-V`, function bypass |
| command-not-found.test.sh | 17 | unknown command handling |
| conditional.test.sh | 17 | `[[ ]]` conditionals, `=~` regex, BASH_REMATCH |
| command-subst.test.sh | 14 | 2 skipped |
| control-flow.test.sh | 33 | if/elif/else, for, while, case |
| cuttr.test.sh | 32 | cut and tr commands (25 skipped) |
| date.test.sh | 38 | format specifiers, `-d` relative/compound/epoch, `-R`, `-I`, `%N` (3 skipped) |
| diff.test.sh | 4 | line diffs |
| echo.test.sh | 24 | escape sequences (1 skipped) |
| errexit.test.sh | 8 | set -e tests |
| fileops.test.sh | 21 | |
| find.test.sh | 10 | file search |
| functions.test.sh | 14 | |
| getopts.test.sh | 9 | POSIX option parsing, combined flags, silent mode |
| globs.test.sh | 12 | for-loop glob expansion, recursive `**` |
| headtail.test.sh | 14 | |
| herestring.test.sh | 8 | 1 skipped |
| hextools.test.sh | 5 | od/xxd/hexdump (3 skipped) |
| negative-tests.test.sh | 16 | error conditions (3 skipped) |
| nl.test.sh | 14 | line numbering |
| nounset.test.sh | 7 | `set -u` unbound variable checks (1 skipped) |
| paste.test.sh | 4 | line merging with `-s` serial and `-d` delimiter |
| path.test.sh | 14 | |
| pipes-redirects.test.sh | 19 | includes stderr redirects |
| printf.test.sh | 24 | format specifiers, array expansion |
| procsub.test.sh | 6 | |
| sleep.test.sh | 6 | |
| sortuniq.test.sh | 32 | sort and uniq (2 skipped) |
| source.test.sh | 21 | source/., function loading, PATH search, positional params |
| test-operators.test.sh | 17 | file/string tests |
| time.test.sh | 11 | Wall-clock only (user/sys always 0) |
| timeout.test.sh | 17 | |
| variables.test.sh | 44 | includes special vars, prefix env assignments |
| wc.test.sh | 35 | word count (5 skipped) |
| eval-bugs.test.sh | 3 | regression tests for eval/script bugs |
| script-exec.test.sh | 10 | script execution by path, $PATH search, exit codes |

## Shell Features

### Not Yet Implemented

Features that may be added in the future (not intentionally excluded):

| Feature | Priority | Notes |
|---------|----------|-------|
| Coprocesses `coproc` | Low | Rarely used |
| Extended globs `@()` `!()` | Medium | Requires `shopt -s extglob` |
| Associative arrays `declare -A` | Medium | Bash 4+ feature |
| ~~`[[ =~ ]]` regex matching~~ | ~~Medium~~ | Implemented: `[[ ]]` conditionals with `=~` and BASH_REMATCH |
| ~~`getopts`~~ | ~~Medium~~ | Implemented: POSIX option parsing |
| ~~`command` builtin~~ | ~~Medium~~ | Implemented: `-v`, `-V`, bypass functions |
| `alias` | Low | Interactive feature |
| History expansion | Out of scope | Interactive only |

### Partially Implemented

| Feature | What Works | What's Missing |
|---------|------------|----------------|
| Prefix env assignments | `VAR=val cmd` temporarily sets env for cmd | Array prefix assignments not in env |
| `local` | Declaration | Proper scoping in nested functions |
| `return` | Basic usage | Return value propagation |
| Heredocs | Basic | Variable expansion inside |
| Arrays | Indexing, `[@]`/`[*]` as separate args, `${!arr[@]}`, `+=` | Slice `${arr[@]:1:2}` |
| `echo -n` | Flag parsed | Trailing newline handling |
| `time` | Wall-clock timing | User/sys CPU time (always 0) |
| `timeout` | Basic usage | `-k` kill timeout |
| `bash`/`sh` | `-c`, `-n`, script files, stdin, `--version`, `--help` | `-e` (exit on error), `-x` (trace), `-o`, login shell |

## Builtins

### Implemented

**84 core builtins + 3 feature-gated = 87 total**

`echo`, `printf`, `cat`, `nl`, `cd`, `pwd`, `true`, `false`, `exit`, `test`, `[`,
`export`, `set`, `unset`, `local`, `source`, `.`, `read`, `shift`, `break`,
`continue`, `return`, `grep`, `sed`, `awk`, `jq`, `sleep`, `head`, `tail`,
`basename`, `dirname`, `mkdir`, `rm`, `cp`, `mv`, `touch`, `chmod`, `wc`,
`sort`, `uniq`, `cut`, `tr`, `paste`, `column`, `diff`, `comm`, `date`,
`wait`, `curl`, `wget`, `timeout`, `command`, `getopts`,
`time` (keyword), `whoami`, `hostname`, `uname`, `id`, `ls`, `rmdir`, `find`, `xargs`, `tee`,
`:` (colon), `eval`, `readonly`, `times`, `bash`, `sh`,
`od`, `xxd`, `hexdump`, `strings`,
`tar`, `gzip`, `gunzip`, `file`, `less`, `stat`, `watch`,
`env`, `printenv`, `history`, `df`, `du`,
`git` (requires `git` feature, see [010-git-support.md](010-git-support.md)),
`python`, `python3` (requires `python` feature, see [011-python-builtin.md](011-python-builtin.md))

### Not Yet Implemented

`ln`, `chown`, `type`, `which`, `hash`, `declare`,
`typeset`, `kill`

## Text Processing

### AWK Limitations

- Regex literals in function args: `gsub(/pattern/, replacement)` ✅
- Array assignment in split: `split($0, arr, ":")` ✅
- Complex regex patterns

**Skipped Tests (15):**

| Feature | Count | Notes |
|---------|-------|-------|
| Power operators | 2 | `^`, `**` |
| Printf formats | 4 | `%x`, `%o`, `%c`, width specifier |
| Functions | 3 | `match()`, `gensub()`, `exit` statement |
| Field handling | 2 | `-F'\t'` tab delimiter, missing field returns empty |
| Negation | 1 | `!$1` logical negation operator |
| ORS/getline | 2 | Output record separator, getline |
| $0 modification | 1 | `$0 = "x y z"` re-splits fields |

**Recently Implemented:**
- For/while/do-while loops with break/continue
- Postfix/prefix increment/decrement (`i++`, `++i`, `i--`, `--i`)
- Arrays: `arr[key]=val`, `"key" in arr`, `for (k in arr)`, `delete arr[k]`
- `-v var=value` flag for variable initialization
- Ternary operator `(cond ? a : b)`
- Field assignment `$2 = "X"`
- `next` statement

<!-- Known AWK gaps for LLM compatibility (tracked in docs/compatibility.md) -->
<!-- - Power operators (^ and **) - used in math scripts -->
<!-- - printf %x/%o/%c formats - used in hex/octal output -->
<!-- - match()/gensub() functions - used in text extraction -->
<!-- - exit statement with code - used in error handling -->
<!-- - !$1 negation - used in filtering empty fields -->
<!-- - ORS variable - used in custom output formatting -->
<!-- - getline - used in multi-file processing -->
<!-- - $0 modification with field re-splitting -->

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

**Skipped Tests (3):**

| Feature | Count | Notes |
|---------|-------|-------|
| Recursive test | 1 | Test needs VFS setup with files |
| Pattern file `-f` | 1 | Requires file redirection support |
| Binary detection | 1 | Auto-detect binary files |

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
- Binary handling: `-a` (filter null bytes)
- Perl regex: `-P` (regex crate supports PCRE features)
- No-op flags: `--color`, `--line-buffered`

### JQ Limitations

**Skipped Tests (8):**

| Feature | Count | Notes |
|---------|-------|-------|
| Alternative `//` | 1 | jaq errors on `.foo` applied to null instead of returning null |
| Path functions | 2 | `setpath`, `leaf_paths` not in jaq standard library |
| I/O functions | 3 | `input`, `inputs` (iterator not wired), `env` (shell vars not propagated) |
| Regex functions | 2 | `match` (jaq omits capture `name` field), `scan` (jaq needs explicit `"g"` flag) |

**Recently Fixed:**
- `try`/`catch` expressions now work (jaq handles runtime errors)
- `debug` passes through values correctly (stderr not captured)
- Combined short flags (`-rn`, `-sc`, `-snr`)
- `--arg name value` and `--argjson name value` variable bindings
- `--indent N` flag no longer eats the filter argument

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

## Parser Limitations

- Single-quoted strings are completely literal (correct behavior)
- Some complex nested structures may timeout
- Very long pipelines may cause stack issues
- Configurable limits: timeout, fuel, input size, AST depth

## Filesystem

- Virtual filesystem only (InMemoryFs, OverlayFs, MountableFs)
- No real filesystem access by default
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
| Output size | 10MB |
| Parser timeout | 5 seconds |
| Parser operations (fuel) | 100,000 |
| Input size | 10MB |
| AST depth | 100 |

## Testing

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
