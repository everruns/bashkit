# 008: POSIX Shell Command Language Compliance

## Status
Implemented (substantial compliance)

## Summary

BashKit aims for substantial compliance with IEEE Std 1003.1-2024 (POSIX.1-2024)
Shell Command Language specification. This document tracks compliance status and
documents intentional deviations for security/sandbox reasons.

## POSIX Compliance Overview

BashKit implements the core POSIX shell functionality required for portable script
execution while maintaining its security-first, sandboxed architecture.

### Compliance Level

| Category | Status | Notes |
|----------|--------|-------|
| Reserved Words | Full | All 16 reserved words supported |
| Special Parameters | Full | All 8 POSIX parameters supported |
| Special Built-in Utilities | Substantial | 13/15 implemented (2 excluded for security) |
| Regular Built-in Utilities | Full | Core set implemented |
| Quoting | Full | All quoting mechanisms supported |
| Word Expansions | Substantial | Most expansions supported |
| Redirections | Full | All POSIX redirection operators |
| Compound Commands | Full | All compound command types |
| Functions | Full | Both syntax forms supported |

## POSIX Reserved Words

All 16 POSIX reserved words are recognized:

| Reserved Word | Status | Notes |
|---------------|--------|-------|
| `!` | Implemented | Pipeline negation |
| `{` | Implemented | Brace group start |
| `}` | Implemented | Brace group end |
| `case` | Implemented | Case statement |
| `do` | Implemented | Loop body start |
| `done` | Implemented | Loop body end |
| `elif` | Implemented | Else-if clause |
| `else` | Implemented | Else clause |
| `esac` | Implemented | Case statement end |
| `fi` | Implemented | If statement end |
| `for` | Implemented | For loop |
| `if` | Implemented | If statement |
| `in` | Implemented | For/case keyword |
| `then` | Implemented | Then clause |
| `until` | Implemented | Until loop |
| `while` | Implemented | While loop |

## POSIX Special Parameters

All 8 POSIX special parameters are implemented:

| Parameter | Status | Implementation |
|-----------|--------|----------------|
| `$@` | Implemented | All positional parameters (separate words in quotes) |
| `$*` | Implemented | All positional parameters (single word when quoted) |
| `$#` | Implemented | Count of positional parameters |
| `$?` | Implemented | Exit status of last pipeline |
| `$-` | Implemented | Current option flags (e, x, etc.) |
| `$$` | Implemented | Process ID (uses actual Rust process ID) |
| `$!` | Implemented | Last background job ID (placeholder in sandbox) |
| `$0` | Implemented | Script/function name |

## POSIX Special Built-in Utilities

POSIX defines 15 special built-in utilities. BashKit implements 13:

| Utility | Status | Notes |
|---------|--------|-------|
| `.` (dot) | Implemented | Execute commands in current environment |
| `:` (colon) | Implemented | Null utility (no-op, returns success) |
| `break` | Implemented | Exit from loop with optional level count |
| `continue` | Implemented | Continue loop with optional level count |
| `eval` | Implemented | Construct and execute command |
| `exec` | **Not Implemented** | Security: cannot replace shell process |
| `exit` | Implemented | Exit shell with status code |
| `export` | Implemented | Export variables to environment |
| `readonly` | Implemented | Mark variables as read-only |
| `return` | Implemented | Return from function with status |
| `set` | Implemented | Set options and positional parameters |
| `shift` | Implemented | Shift positional parameters |
| `times` | Implemented | Display process times (returns zeros in sandbox) |
| `trap` | **Not Implemented** | Security: signal handling excluded |
| `unset` | Implemented | Remove variables and functions |

### Security Exclusions

**`exec`**: Cannot be implemented in a sandboxed environment. The POSIX `exec`
replaces the current shell process, which would break sandbox containment.
Scripts requiring `exec` should be refactored to use standard command execution.

**`trap`**: Signal handlers require persistent state across commands, conflicting
with BashKit's stateless execution model. Additionally, there are no signal
sources in the sandbox (no external processes send SIGINT/SIGTERM). Scripts
should handle errors through exit codes and conditional execution.

## Word Expansions

POSIX defines these expansion types (in order of processing):

| Expansion | Status | Notes |
|-----------|--------|-------|
| Tilde Expansion | Implemented | `~` expands to `$HOME` |
| Parameter Expansion | Implemented | Full `${...}` syntax |
| Command Substitution | Implemented | `$(...)` and `` `...` `` |
| Arithmetic Expansion | Implemented | `$((...))` with full operators |
| Field Splitting | Implemented | IFS-based splitting |
| Pathname Expansion | Implemented | `*`, `?`, `[...]` globs |
| Quote Removal | Implemented | Final stage |

### Parameter Expansion Details

| Syntax | Status | Description |
|--------|--------|-------------|
| `${parameter}` | Implemented | Basic expansion |
| `${parameter:-word}` | Implemented | Use default if unset/null |
| `${parameter:=word}` | Implemented | Assign default if unset/null |
| `${parameter:?word}` | Implemented | Error if unset/null |
| `${parameter:+word}` | Implemented | Use alternative if set |
| `${#parameter}` | Implemented | String length |
| `${parameter%word}` | Implemented | Remove shortest suffix |
| `${parameter%%word}` | Implemented | Remove longest suffix |
| `${parameter#word}` | Implemented | Remove shortest prefix |
| `${parameter##word}` | Implemented | Remove longest prefix |

## Redirections

All POSIX redirection operators are supported:

| Operator | Status | Description |
|----------|--------|-------------|
| `[n]<word` | Implemented | Input redirection |
| `[n]>word` | Implemented | Output redirection (truncate) |
| `[n]>>word` | Implemented | Output redirection (append) |
| `[n]<&digit` | Implemented | Duplicate input fd |
| `[n]>&digit` | Implemented | Duplicate output fd |
| `[n]<&-` | Implemented | Close input fd |
| `[n]>&-` | Implemented | Close output fd |
| `[n]<>word` | Implemented | Open for read/write |
| `<<word` | Implemented | Here-document |
| `<<-word` | Implemented | Here-document with tab strip |
| `<<<word` | Implemented | Here-string (bash extension) |

## Compound Commands

All POSIX compound commands are supported:

| Command | Status | Syntax |
|---------|--------|--------|
| Brace Group | Implemented | `{ list; }` |
| Subshell | Implemented | `( list )` |
| For Loop | Implemented | `for name in words; do list; done` |
| Case | Implemented | `case word in pattern) list;; esac` |
| If | Implemented | `if list; then list; [elif...] [else...] fi` |
| While | Implemented | `while list; do list; done` |
| Until | Implemented | `until list; do list; done` |

## Pipelines and Lists

| Operator | Status | Description |
|----------|--------|-------------|
| `\|` | Implemented | Pipeline |
| `&&` | Implemented | AND list |
| `\|\|` | Implemented | OR list |
| `;` | Implemented | Sequential execution |
| `&` | Partial | Parsed, runs synchronously |
| `!` | Implemented | Pipeline negation |

## Function Definitions

Both POSIX and bash-style function definitions are supported:

```sh
# POSIX style
name() compound-command

# Bash style (also accepted)
function name { compound-command; }
function name() { compound-command; }
```

## Shell Options

Implemented via `set` builtin:

| Option | Flag | Status | Notes |
|--------|------|--------|-------|
| errexit | `-e` | Implemented | Exit on error |
| xtrace | `-x` | Stored | Trace mode (not enforced) |
| nounset | `-u` | Stored | Error on unset variables |
| noclobber | `-C` | Stored | Prevent file overwrite |

## Intentional Deviations

See [KNOWN_LIMITATIONS.md](../KNOWN_LIMITATIONS.md#intentionally-unimplemented-features)
for the complete list with threat IDs.

### Security-Motivated

1. **No process spawning**: External commands run as builtins, not subprocesses
2. **No signal handling**: `trap` excluded for sandbox isolation
3. **No process replacement**: `exec` excluded for containment
4. **Virtual filesystem**: Real FS access requires explicit configuration
5. **Network allowlist**: HTTP requires URL allowlist configuration

### Simplification

1. **Background execution**: `&` is parsed but runs synchronously (stateless model)
2. **Job control**: Not implemented (interactive feature)
3. **Process times**: `times` returns zeros (no CPU tracking)

## Testing POSIX Compliance

### Testing Approach

POSIX compliance is verified through multiple layers of testing:

1. **Unit Tests** (22 POSIX-specific tests in `interpreter/mod.rs`)
   - Positive tests: Verify correct behavior for valid inputs
   - Negative tests: Verify correct handling of edge cases and errors
   - Coverage: All new POSIX special builtins and parameters

2. **Positive Tests** verify:
   - `:` (colon) returns success with no output
   - `:` works in common patterns (while loops, if/then, variable defaults)
   - `readonly` correctly sets and marks variables
   - `times` outputs correct format (two lines, POSIX time format)
   - `eval` accepts and stores commands
   - `$-` reflects current shell options
   - `$!` is accessible (empty when no background jobs)

3. **Negative/Edge Case Tests** verify:
   - `:` produces no output even with arguments
   - `eval` with no arguments succeeds silently
   - `readonly` handles empty values correctly
   - `times` ignores extraneous arguments
   - `$!` is empty when no background jobs have run
   - `:` resets exit code to 0 after failed command

### Running Tests

```bash
# Run all POSIX compliance tests
cargo test --lib -- interpreter::tests::test_colon
cargo test --lib -- interpreter::tests::test_readonly
cargo test --lib -- interpreter::tests::test_times
cargo test --lib -- interpreter::tests::test_eval
cargo test --lib -- interpreter::tests::test_special_param

# Run POSIX-focused spec tests
cargo test --test spec_tests

# Compare with real bash
cargo test --test spec_tests -- bash_comparison_tests --ignored
```

## Future Work

- [ ] Implement `getopts` builtin for option parsing
- [ ] Add `command` builtin for command lookup control
- [ ] Consider `type` builtin for command type detection
- [ ] Evaluate `hash` builtin for command caching info

## References

- [IEEE Std 1003.1-2024](https://pubs.opengroup.org/onlinepubs/9699919799/)
- [Shell Command Language](https://pubs.opengroup.org/onlinepubs/9699919799/utilities/V3_chap02.html)
- [Special Built-in Utilities](https://pubs.opengroup.org/onlinepubs/007904975/idx/sbi.html)
