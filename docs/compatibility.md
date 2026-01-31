# BashKit Compatibility Scorecard

> Automated compatibility testing against bash behavior

## Overall Score

| Metric | Score | Target |
|--------|-------|--------|
| **Bash Core** | 78% | 90% |
| **Text Processing** | 85% | 90% |
| **Overall** | 80% | 90% |

## Test Coverage by Category

### Shell Core

| Feature | Tests | Passing | Score | Status |
|---------|-------|---------|-------|--------|
| Echo/Printf | 10 | 8 | 80% | ✅ Good |
| Variables | 20 | 19 | 95% | ✅ Excellent |
| Control Flow | 31 | - | - | ⚠️ Investigating |
| Functions | 14 | 10 | 71% | 🔶 Needs work |
| Arithmetic | 22 | 12 | 55% | 🔴 Needs work |
| Arrays | 14 | 8 | 57% | 🔶 Needs work |
| Globs | 7 | 4 | 57% | 🔶 Partial |
| Pipes/Redirects | 13 | 10 | 77% | ✅ Good |
| Command Substitution | 14 | 12 | 86% | ✅ Good |

### Text Processing Builtins

| Builtin | Tests | Passing | Score | Status |
|---------|-------|---------|-------|--------|
| awk | 19 | 17 | 89% | ✅ Excellent |
| grep | 15 | 12 | 80% | ✅ Good |
| sed | 17 | 13 | 76% | ✅ Good |
| jq | 21 | 20 | 95% | ✅ Excellent |

## Feature Implementation Status

### Fully Implemented ✅

- Basic commands: `echo`, `printf`, `cat`, `true`, `false`, `exit`
- Navigation: `cd`, `pwd`
- Variables: assignment, expansion, `export`, `unset`
- Parameter expansion: `${var:-default}`, `${var:=value}`, `${#var}`, `${var#pattern}`, `${var%pattern}`
- Control flow: `if`/`elif`/`else`, `for`, `while`, `case`
- Functions: definition, arguments, `$@`, `$#`, `$1`-`$9`
- Pipes and redirections: `|`, `>`, `>>`, `<`, `<<<`
- Command substitution: `$(command)`
- Arithmetic: `$((expression))` - basic operations
- Arrays: declaration, indexing, `${arr[@]}`, `${#arr[@]}`
- Globs: `*`, `?`
- Here documents: `<<EOF`
- Test command: `test`, `[`, string and numeric comparisons
- Text processing: `grep`, `sed`, `awk`, `jq`

### Partially Implemented 🔶

| Feature | What Works | What's Missing |
|---------|------------|----------------|
| `local` | Declaration | Proper nested scope |
| `return` | Basic | Value propagation to `$?` |
| Arithmetic | `+ - * / %` | `== != > < && \|\|` ternary, bitwise |
| Heredocs | Basic | Variable expansion |
| `echo -n` | Flag parsed | Newline suppression |
| Arrays | Basic | `+=` append, `${!arr[@]}` |

### Not Implemented 🔴

| Feature | Priority | Notes |
|---------|----------|-------|
| `set -e` (errexit) | High | Critical for scripts |
| `trap` | High | Signal/error handling |
| Brace expansion `{a,b}` | Medium | Common pattern |
| Extended globs `@()` | Medium | Requires shopt |
| `[[ =~ ]]` regex | Medium | Bash extension |
| Process substitution `<()` | Medium | Advanced |
| Associative arrays | Low | Bash 4+ |
| Coprocesses | Low | Rarely used |
| Job control | Out of scope | Interactive only |

## Running Compatibility Tests

```bash
# Quick validation (awk, grep, sed, jq)
cargo test --test spec_tests

# Full bash compatibility report
cargo test --test spec_tests -- bash_spec_tests --ignored --nocapture

# Compare against real bash
cargo test --test spec_tests -- bash_comparison_tests --ignored --nocapture
```

## Test File Format

Tests use `.test.sh` format in `crates/bashkit/tests/spec_cases/`:

```sh
### test_name
# Description of the test
echo hello world
### expect
hello world
### end

### skipped_test
### skip: reason for skipping
command_that_fails
### expect
expected_output
### end
```

## Comparison with Alternatives

| Feature | BashKit | just-bash | Real Bash |
|---------|---------|-----------|-----------|
| Language | Rust | TypeScript | C |
| Sandboxed | ✅ | ✅ | ❌ |
| Async | ✅ | ✅ | ❌ |
| VFS | ✅ | ✅ | ❌ |
| Network Control | ✅ | ✅ | ❌ |
| Resource Limits | ✅ | ✅ | ❌ |
| POSIX Compliance | ~80% | ~80% | 100% |
| Spec Tests | 114 | 150+ | N/A |

## Roadmap

### Q1 Goals
- [ ] Reach 90% bash core compatibility
- [ ] Implement `set -e` (errexit)
- [ ] Fix arithmetic comparisons
- [ ] Add file manipulation builtins

### Future
- [ ] Property-based testing
- [ ] Fuzzing for parser
- [ ] POSIX sh compliance tests

## Contributing

To improve compatibility:

1. Add failing test to appropriate `.test.sh` file
2. Run `cargo test --test spec_tests -- bash_spec_tests --ignored`
3. Implement the feature
4. Remove `### skip` from test
5. Update this scorecard

See [KNOWN_LIMITATIONS.md](../KNOWN_LIMITATIONS.md) for detailed gap analysis.
