# Known Limitations

BashKit is a sandboxed bash interpreter designed for AI agents. It prioritizes safety and simplicity over full POSIX/bash compliance. This document tracks known limitations.

## Spec Test Coverage

Current compatibility: **100%** (102/102 non-skipped tests passing)

| Category | Passed | Skipped | Total | Notes |
|----------|--------|---------|-------|-------|
| Echo | 8 | 2 | 10 | -n flag edge case, empty echo |
| Variables | 20 | 0 | 20 | All passing |
| Control Flow | - | - | - | Skipped (timeout investigation) |
| Functions | 14 | 0 | 14 | All passing |
| Arithmetic | 18 | 4 | 22 | Skipped: assignment, ternary, bitwise |
| Arrays | 12 | 2 | 14 | Skipped: indices, slicing |
| Globs | 4 | 3 | 7 | Skipped: brackets, recursive, brace |
| Pipes/Redirects | 11 | 2 | 13 | Skipped: stderr redirect |
| Command Subst | 13 | 1 | 14 | Skipped: exit code propagation |
| AWK | 17 | 2 | 19 | gsub regex, split |
| Grep | 12 | 3 | 15 | -w, -o, -l stdin |
| Sed | 13 | 4 | 17 | -i flag, multiple commands |
| JQ | 20 | 1 | 21 | -r flag |

## Shell Features

### Not Implemented

| Feature | Priority | Notes |
|---------|----------|-------|
| `set -e` (errexit) | High | Critical for scripts |
| Process substitution `<(cmd)` | Medium | Used in advanced scripts |
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

### Partially Implemented

| Feature | What Works | What's Missing |
|---------|------------|----------------|
| `local` | Declaration | Proper scoping in nested functions |
| `return` | Basic usage | Return value propagation |
| Arithmetic | Basic ops | Comparison, ternary, bitwise |
| Heredocs | Basic | Variable expansion inside |
| Arrays | Indexing, `[@]` | `+=` append, `${!arr[@]}` |
| `echo -n` | Flag parsed | Trailing newline handling |

## Builtins

### Implemented
`echo`, `printf`, `cat`, `cd`, `pwd`, `true`, `false`, `exit`, `test`, `[`, `export`, `set`, `unset`, `local`, `source`, `read`, `grep`, `sed`, `awk`, `jq`

### Not Implemented
`cp`, `mv`, `rm`, `mkdir`, `rmdir`, `ls`, `touch`, `chmod`, `chown`, `ln`, `head`, `tail`, `sort`, `uniq`, `wc`, `tr`, `cut`, `tee`, `xargs`, `find`, `type`, `which`, `command`, `hash`, `declare`, `typeset`, `readonly`, `shift`, `wait`, `kill`, `eval`, `exec`

## Text Processing

### AWK Limitations
- Regex literals in function args: `gsub(/pattern/, replacement)`
- Array assignment in split: `split($0, arr, ":")`
- Complex regex patterns

### Sed Limitations
- Case insensitive flag `/i`
- Multiple commands in single invocation
- Append/insert commands (`a\`, `i\`)
- In-place editing (`-i`)

### Grep Limitations
- Word boundary `-w`
- Only matching `-o`
- Stdin filename with `-l`

### JQ Limitations
- Raw output `-r` flag
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
