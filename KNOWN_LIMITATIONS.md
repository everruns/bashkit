# Known Limitations

BashKit is a sandboxed bash interpreter designed for AI agents. It prioritizes safety and simplicity over full POSIX/bash compliance. This document tracks known limitations.

## Spec Test Coverage

**Total spec test cases:** 281

| Category | Cases | In CI | Pass | Skip | Notes |
|----------|-------|-------|------|------|-------|
| Bash (core) | 209 | **No** | - | - | `bash_spec_tests` ignored in CI |
| AWK | 19 | Yes | 17 | 2 | gsub regex, split (blocked by parser bug) |
| Grep | 15 | Yes | 15 | 0 | All tests pass |
| Sed | 17 | Yes | 17 | 0 | All tests pass |
| JQ | 21 | Yes | 21 | 0 | All tests pass |
| **Total** | **281** | **72** | 70 | 2 | |

### Bash Spec Tests Breakdown (not in CI)

| File | Cases | Notes |
|------|-------|-------|
| arithmetic.test.sh | 22 | |
| arrays.test.sh | 14 | |
| background.test.sh | 2 | |
| command-subst.test.sh | 14 | |
| control-flow.test.sh | - | Skipped (.skip suffix) |
| cuttr.test.sh | 10 | |
| date.test.sh | 4 | |
| echo.test.sh | 10 | |
| fileops.test.sh | 15 | |
| functions.test.sh | 14 | |
| globs.test.sh | 7 | |
| headtail.test.sh | 14 | |
| herestring.test.sh | 8 | |
| path.test.sh | 14 | |
| pipes-redirects.test.sh | 13 | |
| procsub.test.sh | 6 | |
| sleep.test.sh | 6 | |
| sortuniq.test.sh | 12 | |
| time.test.sh | 12 | Wall-clock only (user/sys always 0) |
| timeout.test.sh | 17 | 2 skipped (timing-dependent) |
| variables.test.sh | 20 | |
| wc.test.sh | 4 | |

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

### Sed Limitations
- In-place editing (`-i`) - not implemented for security
- All spec tests pass

### Grep Limitations
- No known limitations (all spec tests pass)

### JQ Limitations
- Pretty printing (outputs compact JSON)

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
