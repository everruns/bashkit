# Known Limitations

BashKit is a sandboxed bash interpreter designed for AI agents. It provides
substantial POSIX shell compliance while prioritizing safety and security.
This document tracks known limitations.

## Intentionally Unimplemented Features

These features are **by design** not implemented. They conflict with BashKit's
stateless, sandboxed execution model or pose security risks.

| Feature | Rationale | Threat ID |
|---------|-----------|-----------|
| `exec` builtin | Cannot replace shell process in sandbox; breaks containment | TM-ESC-005 |
| `trap` builtin | Signal handling could interfere with host/other tenants | - |
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

**Security Exclusions**: `exec` and `trap` are POSIX special builtins excluded
for sandbox isolation. Scripts should use standard command execution and
exit-code-based error handling instead.

See [specs/006-threat-model.md](specs/006-threat-model.md) for threat details.

## POSIX Compliance

BashKit implements most of IEEE 1003.1-2024 Shell Command Language with
intentional security exclusions. See [specs/008-posix-compliance.md](specs/008-posix-compliance.md)
for detailed compliance status.

## Spec Test Coverage

**Total spec test cases:** 754

| Category | Cases | In CI | Pass | Skip | Notes |
|----------|-------|-------|------|------|-------|
| Bash (core) | 435 | Yes | 330 | 105 | `bash_spec_tests` in CI |
| AWK | 89 | Yes | 48 | 41 | loops, arrays, functions |
| Grep | 70 | Yes | 56 | 14 | now with -A/-B/-C, -m, -q, -x, -e |
| Sed | 65 | Yes | 49 | 16 | now with -E, nth occurrence, ! negation |
| JQ | 95 | Yes | 58 | 37 | reduce, walk, regex funcs |
| **Total** | **754** | **Yes** | **541** | **213** | |

### Bash Spec Tests Breakdown

| File | Cases | Notes |
|------|-------|-------|
| arithmetic.test.sh | 22 | includes logical operators |
| arrays.test.sh | 14 | includes indices |
| background.test.sh | 2 | |
| brace-expansion.test.sh | 10 | NEW: {a,b,c}, {1..5} |
| command-subst.test.sh | 14 | |
| control-flow.test.sh | 31 | if/elif/else, for, while, case |
| cuttr.test.sh | 35 | cut and tr commands |
| date.test.sh | 31 | format specifiers |
| echo.test.sh | 26 | escape sequences |
| errexit.test.sh | 10 | NEW: set -e tests |
| fileops.test.sh | 15 | |
| functions.test.sh | 14 | |
| globs.test.sh | 7 | |
| headtail.test.sh | 14 | |
| herestring.test.sh | 8 | |
| negative-tests.test.sh | 8 | NEW: error conditions |
| path.test.sh | 14 | |
| pipes-redirects.test.sh | 13 | includes stderr redirects |
| procsub.test.sh | 6 | |
| sleep.test.sh | 6 | |
| sortuniq.test.sh | 31 | sort and uniq |
| test-operators.test.sh | 12 | NEW: file/string tests |
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

For intentionally excluded features (`exec`, `trap`, background execution, job
control, symlink following), see [Intentionally Unimplemented Features](#intentionally-unimplemented-features) above.

### Implemented (previously missing)
- Process substitution `<(cmd)` - works
- `set -e` (errexit) - exits on command failure, respects conditionals
- Brace expansion `{a,b,c}`, `{1..5}` - full support
- Tilde expansion `~` - expands to $HOME
- Special variables `$$`, `$RANDOM`, `$LINENO`
- File test operators `-r`, `-w`, `-x`, `-L`
- Stderr redirections `2>`, `2>&1`, `&>`
- Arithmetic logical operators `&&`, `||` with short-circuit
- String comparison `<`, `>` in test
- Array indices `${!arr[@]}`
- POSIX special parameters `$-` (current options) and `$!` (last bg PID)
- POSIX special builtins: `:` (colon/null), `eval`, `readonly`, `times`

### Partially Implemented

| Feature | What Works | What's Missing |
|---------|------------|----------------|
| `local` | Declaration | Proper scoping in nested functions |
| `return` | Basic usage | Return value propagation |
| Heredocs | Basic | Variable expansion inside |
| Arrays | Indexing, `[@]`, `${!arr[@]}`, `+=` | Slice `${arr[@]:1:2}` |
| `echo -n` | Flag parsed | Trailing newline handling |
| `time` | Wall-clock timing | **User/sys CPU time not tracked (always 0)** |
| `timeout` | Basic usage | `-k` kill timeout (always terminates immediately) |

## Builtins

### Implemented
`echo`, `printf`, `cat`, `cd`, `pwd`, `true`, `false`, `exit`, `test`, `[`, `export`, `set`, `unset`, `local`, `source`, `read`, `shift`, `break`, `continue`, `return`, `grep`, `sed`, `awk`, `jq`, `sleep`, `head`, `tail`, `basename`, `dirname`, `mkdir`, `rm`, `cp`, `mv`, `touch`, `chmod`, `wc`, `sort`, `uniq`, `cut`, `tr`, `date`, `wait`, `curl`, `wget`, `timeout`, `time` (keyword), `whoami`, `hostname`, `ls`, `rmdir`, `find`, `xargs`, `tee`, `:` (colon), `eval`, `readonly`, `times`

### Not Implemented
`ln`, `chown`, `diff`, `type`, `which`, `command`, `hash`, `declare`, `typeset`, `getopts`, `kill`

### Security Exclusions (Intentional)
`exec`, `trap` - See [Intentionally Unimplemented Features](#intentionally-unimplemented-features).

## Text Processing

### AWK Limitations
- Regex literals in function args: `gsub(/pattern/, replacement)`
- Array assignment in split: `split($0, arr, ":")`
- Complex regex patterns

**Skipped Tests (41):**
| Feature | Count | Notes |
|---------|-------|-------|
| Arrays | 8 | `arr[key]`, associative arrays, `in` operator |
| For loops | 6 | `for (i=0; i<n; i++)`, `for (k in arr)` |
| While loops | 2 | `while (condition)` |
| Ternary operator | 2 | `condition ? true : false` |
| User functions | 4 | `function name() {}` |
| gsub/sub regex | 3 | Regex literals as first argument |
| split() | 2 | Array assignment from split |
| printf formatting | 4 | `%s`, `%-10s`, `%d` width/precision |
| Multiple -v vars | 2 | `-v a=1 -v b=2` |
| BEGIN/END blocks | 3 | Multiple or complex blocks |
| Field assignment | 2 | `$1 = "new"` |
| NR/NF in conditions | 3 | `NR > 1`, `NF == 3` |

### Sed Limitations
- In-place editing (`-i`) - not yet implemented

**Implemented (previously missing):**
- Extended regex `-E` (`+`, `?`, `|`, `()` grouping)
- Nth occurrence replacement (`s/a/X/2`, `s/a/X/3`)
- Address negation (`!`) for inverted matches
- Append/insert (`a\`, `i\`) commands
- Multiple `-e` expressions (`sed -e 's/a/b/' -e 's/c/d/'`)

**Skipped Tests (16):**
| Feature | Count | Notes |
|---------|-------|-------|
| Hold space | 3 | `h`, `H`, `x` commands |
| Pattern ranges | 3 | `/start/,/end/` address ranges |
| Branching | 1 | `b`, `t`, `:label` commands |
| Grouped commands | 1 | `{cmd1;cmd2}` blocks |
| Special addresses | 2 | `0~2` step, `0,/pattern/` first match |
| Replacement escapes | 2 | `\n` newline, `&` with adjacent chars |
| Change command | 1 | `c\` command |
| Q command | 1 | `Q` quit without printing |
| In-place edit | 1 | `-i` flag |
| Backreferences | 1 | Some edge cases |

### Grep Limitations

**Implemented (previously missing):**
- Context flags `-A`, `-B`, `-C` (after/before/context)
- Max count `-m` (stop after N matches)
- Whole line match `-x`
- Quiet mode `-q` (exit status only)
- Multiple `-e` patterns (`-e pat1 -e pat2`)

**Skipped Tests (8):**
| Feature | Count | Notes |
|---------|-------|-------|
| Recursive `-r` | 2 | Recursive search in directories |
| Pattern file `-f` | 1 | Read patterns from file |
| Byte offset `-b` | 1 | Show byte offset |
| Show filename `-H` | 1 | Force filename display |
| Word boundary `\b` in ERE | 1 | `\bword\b` with `-E` |
| Binary files | 2 | `-a`, binary detection |

### JQ Limitations

**Skipped Tests (37):**
| Feature | Count | Notes |
|---------|-------|-------|
| CLI flags | 8 | `-c`, `-S`, `-s`, `-n`, `-e`, `-j`, `--tab` |
| Regex functions | 4 | `test`, `match`, `scan`, `gsub`, `sub` |
| Path functions | 4 | `getpath`, `setpath`, `paths`, `leaf_paths` |
| Control flow | 4 | `reduce`, `foreach`, `until`, `while`, `limit` |
| Math functions | 3 | `ceil`, `round`, `abs`, `range` |
| Advanced filters | 3 | `walk`, `recurse`, `del` |
| String functions | 2 | `rindex`, `indices` |
| I/O functions | 3 | `input`, `inputs`, `debug`, `env` |
| Alternative `//` | 2 | Null coalescing operator |
| Try-catch | 2 | `try ... catch` |
| Group by | 2 | `group_by(.key)` |

### Curl Limitations

**Tests NOT Ported:** Curl tests from just-bash were not ported because:
1. Requires http_client feature flag (`--features http_client`)
2. Needs URL allowlist configuration
3. just-bash tests mock HTTP responses; bashkit uses real requests
4. Different error handling semantics

**Coverage Gap:** ~25 curl test patterns from just-bash not yet adapted:
- HTTP methods (GET, POST, PUT, DELETE)
- Headers (`-H`)
- Data payloads (`-d`, `--data-raw`)
- Output options (`-o`, `-O`)
- Authentication (`-u`)
- Follow redirects (`-L`)
- Silent mode (`-s`)
- Timeout (`--connect-timeout`)

**TODO:** Create curl.test.sh with mock server or allowlisted test endpoints.

## Parser Limitations

- Single-quoted strings are completely literal (correct behavior)
- Some complex nested structures may timeout
- Very long pipelines may cause stack issues
- Parser has configurable limits: timeout, fuel (operations), input size, AST depth

## Filesystem

- Virtual filesystem only (InMemoryFs, OverlayFs, MountableFs)
- No real filesystem access by default
- Symlinks stored but not followed (see [Intentionally Unimplemented Features](#intentionally-unimplemented-features))
- No file permissions enforcement

## Network

- HTTP only (via `curl` builtin when enabled)
- URL allowlist required
- No raw sockets
- No DNS resolution (host must be in allowlist)

## Resource Limits

Default limits (configurable):
- Commands: 10,000
- Loop iterations: 100,000
- Function depth: 100
- Output size: 10MB
- Parser timeout: 5 seconds
- Parser operations (fuel): 100,000
- Input size: 10MB
- AST depth: 100

## Comparison with Real Bash

Run comparison tests:
```bash
cargo test --test spec_tests -- bash_comparison_tests --ignored
```

This runs each spec test against both BashKit and real bash, reporting differences.

## Contributing

To add a known limitation:
1. Add a spec test that demonstrates the limitation
2. Mark the test with `### skip: reason`
3. Update this document
4. Optionally file an issue for tracking
