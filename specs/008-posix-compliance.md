# 008: POSIX Shell Command Language Compliance

## Status
Implemented (substantial compliance)

## Summary

BashKit aims for substantial compliance with IEEE Std 1003.1-2024 (POSIX.1-2024)
Shell Command Language specification. This document explains our compliance
approach and security-motivated deviations.

For detailed implementation status, see [009-implementation-status.md](009-implementation-status.md).

## Design Philosophy

BashKit prioritizes:
1. **Security over completeness** - exclude features that break sandbox containment
2. **Stateless execution** - no persistent state between command invocations
3. **Deterministic behavior** - predictable results for AI agent workflows

## Security Exclusions

Two POSIX special builtins are intentionally excluded:

**`exec`**: The POSIX `exec` replaces the current shell process, which would
break sandbox containment. Scripts requiring `exec` should be refactored to
use standard command execution.

**`trap`**: Signal handlers require persistent state across commands, conflicting
with BashKit's stateless execution model. Additionally, there are no signal
sources in the sandbox (no external processes send SIGINT/SIGTERM). Scripts
should handle errors through exit codes and conditional execution.

## Intentional Deviations

### Security-Motivated

1. **No process spawning**: External commands run as builtins, not subprocesses
2. **No signal handling**: `trap` excluded for sandbox isolation
3. **No process replacement**: `exec` excluded for containment
4. **Virtual filesystem**: Real FS access requires explicit configuration
5. **Network allowlist**: HTTP requires URL allowlist configuration

### Simplification (Stateless Model)

1. **Background execution**: `&` is parsed but runs synchronously
2. **Job control**: Not implemented (interactive feature)
3. **Process times**: `times` returns zeros (no CPU tracking)

## Testing Approach

POSIX compliance is verified through:

1. **Unit Tests** - 22 POSIX-specific tests for special builtins and parameters
2. **Spec Tests** - 435 bash spec test cases in CI
3. **Bash Comparison** - differential testing against real bash

```bash
# Run POSIX compliance tests
cargo test --lib -- interpreter::tests::test_colon
cargo test --lib -- interpreter::tests::test_readonly
cargo test --lib -- interpreter::tests::test_times
cargo test --lib -- interpreter::tests::test_eval
cargo test --lib -- interpreter::tests::test_special_param

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
