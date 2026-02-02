# Known Limitations

BashKit is a sandboxed bash interpreter designed for AI agents. It prioritizes safety and simplicity over full POSIX/bash compliance. This document tracks known limitations.

## Spec Test Coverage

**Total spec test cases:** 510+

| Category | Cases | In CI | Pass | Skip | Notes |
|----------|-------|-------|------|------|-------|
| Bash (core) | 209+ | **No** | - | - | `bash_spec_tests` ignored in CI |
| AWK | 89 | Yes | 46 | 43 | loops, arrays, functions |
| Grep | 55 | Yes | 32 | 23 | context, -m, -x flags |
| Sed | 65 | Yes | 36 | 29 | hold space, -E, branching |
| JQ | 92 | Yes | 54 | 38 | reduce, walk, regex funcs |
| **Total** | **510+** | **301** | 168 | 133 | |

### Bash Spec Tests Breakdown (not in CI)

| File | Cases | Notes |
|------|-------|-------|
| arithmetic.test.sh | 22 | |
| arrays.test.sh | 14 | |
| background.test.sh | 2 | |
| command-subst.test.sh | 14 | |
| control-flow.test.sh | - | Skipped (.skip suffix) |
| cuttr.test.sh | 35 | cut and tr commands |
| date.test.sh | 31 | format specifiers |
| echo.test.sh | 26 | escape sequences |
| fileops.test.sh | 15 | |
| functions.test.sh | 14 | |
| globs.test.sh | 7 | |
| headtail.test.sh | 14 | |
| herestring.test.sh | 8 | |
| path.test.sh | 14 | |
| pipes-redirects.test.sh | 13 | |
| procsub.test.sh | 6 | |
| sleep.test.sh | 6 | |
| sortuniq.test.sh | 31 | sort and uniq |
| time.test.sh | 12 | Wall-clock only (user/sys always 0) |
| timeout.test.sh | 17 | 2 skipped (timing-dependent) |
| variables.test.sh | 20 | |
| wc.test.sh | 22 | word count |

## Shell Features

### Not Implemented

| Feature | Priority | Notes |
|---------|----------|-------|
| `set -e` (errexit) | High | Critical for scripts |
| Coprocesses `coproc` | Low | Rarely used |
| Extended globs `@()` `!()` | Medium | Requires `shopt -s extglob` |
| Associative arrays `declare -A` | Medium | Bash 4+ feature |
| `[[ =~ ]]` regex matching | Medium | Bash extension |
| Backtick substitution | Low | Deprecated, use `$()` |
| Brace expansion `{a,b,c}` | Medium | Common pattern |
| `trap` signal handling | High | Error handling |
| `getopts` | Medium | Option parsing |
| `alias` | Low | Interactive feature |
| History expansion | Out of scope | Interactive only |
| Job control (bg/fg/jobs) | Out of scope | Requires process control |

### Implemented (previously missing)
- Process substitution `<(cmd)` - now works

### Partially Implemented

| Feature | What Works | What's Missing |
|---------|------------|----------------|
| `local` | Declaration | Proper scoping in nested functions |
| `return` | Basic usage | Return value propagation |
| Arithmetic | Basic ops | Comparison, ternary, bitwise |
| Heredocs | Basic | Variable expansion inside |
| Arrays | Indexing, `[@]` | `+=` append, `${!arr[@]}` |
| `echo -n` | Flag parsed | Trailing newline handling |
| `time` | Wall-clock timing | **User/sys CPU time not tracked (always 0)** |
| `timeout` | Basic usage | `-k` kill timeout (always terminates immediately) |

## Builtins

### Implemented
`echo`, `printf`, `cat`, `cd`, `pwd`, `true`, `false`, `exit`, `test`, `[`, `export`, `set`, `unset`, `local`, `source`, `read`, `shift`, `break`, `continue`, `return`, `grep`, `sed`, `awk`, `jq`, `sleep`, `head`, `tail`, `basename`, `dirname`, `mkdir`, `rm`, `cp`, `mv`, `touch`, `chmod`, `wc`, `sort`, `uniq`, `cut`, `tr`, `date`, `wait`, `curl`, `wget`, `timeout`, `time` (keyword)

### Not Implemented
`ls`, `rmdir`, `ln`, `chown`, `tee`, `xargs`, `find`, `diff`, `type`, `which`, `command`, `hash`, `declare`, `typeset`, `readonly`, `getopts`, `kill`, `eval`, `exec`, `trap`

## Text Processing

### AWK Limitations
- Regex literals in function args: `gsub(/pattern/, replacement)`
- Array assignment in split: `split($0, arr, ":")`
- Complex regex patterns

**Skipped Tests (43):**
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
| Regex match ~ | 2 | `$0 ~ /pattern/` |

### Sed Limitations
- In-place editing (`-i`) - not implemented for security

**Skipped Tests (29):**
| Feature | Count | Notes |
|---------|-------|-------|
| Extended regex `-E` | 5 | `+`, `?`, `\|`, `()` grouping |
| Hold space | 6 | `h`, `H`, `g`, `G`, `x` commands |
| Pattern ranges | 4 | `/start/,/end/` address ranges |
| Branching | 4 | `b`, `t`, `:label` commands |
| Append/Insert | 3 | `a\`, `i\` commands |
| Character classes | 3 | `[:alpha:]`, `[:digit:]` in `y///` |
| Multiple `-e` | 2 | `-e 's/a/b/' -e 's/c/d/'` |
| Line number ranges | 2 | `1,5s/...` |

### Grep Limitations

**Skipped Tests (23):**
| Feature | Count | Notes |
|---------|-------|-------|
| Context flags | 6 | `-A`, `-B`, `-C` (after/before/context) |
| Max count `-m` | 3 | Stop after N matches |
| Exact match `-x` | 2 | Match whole line only |
| Files with matches `-l` | 2 | List filenames only |
| Quiet mode `-q` | 2 | Exit status only |
| Invert `-v` with count | 2 | Combined flags |
| Word boundary `\b` | 2 | `\bword\b` |
| Multiple `-e` patterns | 2 | `-e pat1 -e pat2` |
| Perl regex `-P` | 2 | Lookahead, lookbehind |

### JQ Limitations

**Skipped Tests (38):**
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
1. Requires network feature flag (`--features network`)
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

## Filesystem

- Virtual filesystem only (InMemoryFs, OverlayFs, MountableFs)
- No real filesystem access by default
- Symlinks stored but not followed
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
