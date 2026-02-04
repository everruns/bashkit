# 009: Implementation Status

## Status
Living document (updated as features change)

## Summary

Tracks what's implemented, what's not, and why. Single source of truth for
feature status across BashKit.

## Intentionally Unimplemented Features

These features are **by design** not implemented. They conflict with BashKit's
stateless, sandboxed execution model or pose security risks.

| Feature | Rationale | Threat ID |
|---------|-----------|-----------|
| `exec` builtin | Cannot replace shell process in sandbox; breaks containment | TM-ESC-005 |
| `trap` builtin | Stateless model - no persistent handlers; no signal sources in sandbox | - |
| Background execution (`&`) | Stateless model - no persistent processes between commands | TM-ESC-007 |
| Job control (`bg`, `fg`, `jobs`) | Requires process state; interactive feature | - |
| Symlink following | Prevents symlink loop attacks and sandbox escape | TM-DOS-011 |
| Process spawning | External commands run as builtins, not subprocesses | - |
| Raw network sockets | Only allowlisted HTTP via curl builtin | - |

### Design Rationale

**Stateless Execution Model**: BashKit runs scripts in isolated, stateless
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
are no signal sources in the sandbox. Scripts should use exit-code-based error
handling instead.

See [006-threat-model.md](006-threat-model.md) for threat details.

## POSIX Compliance

BashKit implements IEEE 1003.1-2024 Shell Command Language. See
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
| `.` (dot) | Implemented | Execute commands in current environment |
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
| `times` | Implemented | Display process times (returns zeros in sandbox) |
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

**Total spec test cases:** 790

| Category | Cases | In CI | Pass | Skip | Notes |
|----------|-------|-------|------|------|-------|
| Bash (core) | 471 | Yes | 406 | 65 | `bash_spec_tests` in CI |
| AWK | 89 | Yes | 55 | 34 | loops, arrays, functions |
| Grep | 70 | Yes | 65 | 5 | now with -z, -r, -a, -b, -H, -h, -f, -P |
| Sed | 65 | Yes | 50 | 15 | now with -E, nth occurrence, ! negation |
| JQ | 95 | Yes | 85 | 10 | reduce, walk, regex funcs |
| **Total** | **790** | **Yes** | **655** | **135** | |

### Bash Spec Tests Breakdown

| File | Cases | Notes |
|------|-------|-------|
| arithmetic.test.sh | 22 | includes logical operators |
| arrays.test.sh | 14 | includes indices |
| background.test.sh | 2 | |
| brace-expansion.test.sh | 10 | {a,b,c}, {1..5} |
| command-subst.test.sh | 14 | |
| control-flow.test.sh | 31 | if/elif/else, for, while, case |
| cuttr.test.sh | 35 | cut and tr commands |
| date.test.sh | 31 | format specifiers |
| echo.test.sh | 26 | escape sequences |
| errexit.test.sh | 10 | set -e tests |
| fileops.test.sh | 15 | |
| functions.test.sh | 14 | |
| globs.test.sh | 7 | |
| headtail.test.sh | 14 | |
| herestring.test.sh | 8 | |
| negative-tests.test.sh | 8 | error conditions |
| path.test.sh | 14 | |
| pipes-redirects.test.sh | 13 | includes stderr redirects |
| procsub.test.sh | 6 | |
| sleep.test.sh | 6 | |
| sortuniq.test.sh | 31 | sort and uniq |
| test-operators.test.sh | 12 | file/string tests |
| time.test.sh | 12 | Wall-clock only (user/sys always 0) |
| timeout.test.sh | 17 | 2 skipped (timing-dependent) |
| variables.test.sh | 20 | includes special vars |
| wc.test.sh | 22 | word count |

## Shell Features

### Not Yet Implemented

Features that may be added in the future (not intentionally excluded):

| Feature | Priority | Notes |
|---------|----------|-------|
| Coprocesses `coproc` | Low | Rarely used |
| Extended globs `@()` `!()` | Medium | Requires `shopt -s extglob` |
| Associative arrays `declare -A` | Medium | Bash 4+ feature |
| `[[ =~ ]]` regex matching | Medium | Bash extension |
| `getopts` | Medium | POSIX option parsing |
| `command` builtin | Medium | POSIX command lookup |
| `alias` | Low | Interactive feature |
| History expansion | Out of scope | Interactive only |

### Partially Implemented

| Feature | What Works | What's Missing |
|---------|------------|----------------|
| `local` | Declaration | Proper scoping in nested functions |
| `return` | Basic usage | Return value propagation |
| Heredocs | Basic | Variable expansion inside |
| Arrays | Indexing, `[@]`, `${!arr[@]}`, `+=` | Slice `${arr[@]:1:2}` |
| `echo -n` | Flag parsed | Trailing newline handling |
| `time` | Wall-clock timing | User/sys CPU time (always 0) |
| `timeout` | Basic usage | `-k` kill timeout |

## Builtins

### Implemented

`echo`, `printf`, `cat`, `cd`, `pwd`, `true`, `false`, `exit`, `test`, `[`,
`export`, `set`, `unset`, `local`, `source`, `read`, `shift`, `break`,
`continue`, `return`, `grep`, `sed`, `awk`, `jq`, `sleep`, `head`, `tail`,
`basename`, `dirname`, `mkdir`, `rm`, `cp`, `mv`, `touch`, `chmod`, `wc`,
`sort`, `uniq`, `cut`, `tr`, `date`, `wait`, `curl`, `wget`, `timeout`,
`time` (keyword), `whoami`, `hostname`, `ls`, `rmdir`, `find`, `xargs`, `tee`,
`:` (colon), `eval`, `readonly`, `times`

### Not Yet Implemented

`ln`, `chown`, `diff`, `type`, `which`, `command`, `hash`, `declare`,
`typeset`, `getopts`, `kill`

## Text Processing

### AWK Limitations

- Regex literals in function args: `gsub(/pattern/, replacement)`
- Array assignment in split: `split($0, arr, ":")`
- Complex regex patterns

**Skipped Tests (34):**

| Feature | Count | Notes |
|---------|-------|-------|
| Increment/decrement | 4 | `i++`, `++i`, `i--`, `--i` |
| Power operators | 2 | `^`, `**` |
| Printf formats | 4 | `%x`, `%o`, `%c`, width specifier |
| Loops | 6 | `for`, `while`, `do-while`, `break`, `continue` |
| Arrays | 4 | `arr[key]`, `in` operator, `for-in`, `delete` |
| Control flow | 3 | `if-else`, ternary, `next` |
| Functions | 3 | `match()`, `gensub()`, `exit` |
| -v flag | 1 | Variable initialization |
| Field handling | 3 | Field separator, missing fields, field assignment |
| Negation | 1 | Logical negation operator |
| ORS/getline | 3 | Output record separator, getline, $0 modification |

### Sed Limitations

- In-place editing (`-i`) - not yet implemented

**Skipped Tests (15):**

| Feature | Count | Notes |
|---------|-------|-------|
| Hold space | 3 | `h`, `H`, `x` commands |
| Pattern ranges | 4 | `/start/,/end/` and `/pattern/,$` address ranges |
| Branching | 1 | `b`, `t`, `:label` commands |
| Grouped commands | 1 | `{cmd1;cmd2}` blocks |
| Special addresses | 2 | `0~2` step, `0,/pattern/` first match |
| Replacement escapes | 2 | `\n` newline, `&` with adjacent chars |
| Change command | 1 | `c\` command |
| Q command | 1 | `Q` quit without printing |

### Grep Limitations

**Skipped Tests (5):**

| Feature | Count | Notes |
|---------|-------|-------|
| Recursive test | 1 | Test needs VFS setup with files |
| Pattern file `-f` | 1 | Requires file redirection support |
| Include/exclude | 2 | `--include`, `--exclude` patterns |
| Binary detection | 1 | Auto-detect binary files |

**Implemented Features:**
- Basic flags: `-i`, `-v`, `-c`, `-n`, `-o`, `-l`, `-w`, `-E`, `-F`, `-q`, `-m`, `-x`
- Context: `-A`, `-B`, `-C` (after/before/context lines)
- Multiple patterns: `-e`
- Pattern file: `-f` (requires file to exist in VFS)
- Filename control: `-H` (always show), `-h` (never show)
- Byte offset: `-b`
- Null-terminated: `-z` (split on `\0` instead of `\n`)
- Recursive: `-r`/`-R` (uses VFS read_dir)
- Binary handling: `-a` (filter null bytes)
- Perl regex: `-P` (regex crate supports PCRE features)
- No-op flags: `--color`, `--line-buffered`

### JQ Limitations

**Skipped Tests (10):**

| Feature | Count | Notes |
|---------|-------|-------|
| Alternative `//` | 1 | Null coalescing operator |
| Try-catch | 1 | `try` expression |
| Path functions | 2 | `setpath`, `leaf_paths` |
| I/O functions | 4 | `input`, `inputs`, `debug`, `env` |
| Regex functions | 2 | `match`, `scan` |

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

Runs each spec test against both BashKit and real bash, reporting differences.

### Contributing

To add a known limitation:
1. Add a spec test that demonstrates the limitation
2. Mark the test with `### skip: reason`
3. Update this document
4. Optionally file an issue for tracking
