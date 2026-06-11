# Security Testing with Fail Points

## Overview

Bashkit uses [fail-rs](https://github.com/tikv/fail-rs) for fault injection
security testing of error handling paths and resource limit enforcement.
Fail points are disabled by default, zero runtime overhead when disabled.

```bash
cargo test --features failpoints security_ -- --test-threads=1
```

## Available Fail Points

### Resource Limits (`limits.rs`)

| Fail Point | Actions | Security Test Purpose |
|------------|---------|----------------------|
| `limits::tick_command` | `skip_increment`, `force_overflow`, `corrupt_high` | Test command limit bypass resistance |
| `limits::tick_loop` | `skip_check`, `reset_counter` | Test loop limit bypass resistance |
| `limits::push_function` | `skip_check`, `corrupt_depth` | Test recursion limit bypass resistance |

### Filesystem (`fs/memory.rs`)

| Fail Point | Actions | Security Test Purpose |
|------------|---------|----------------------|
| `fs::read_file` | `io_error`, `permission_denied`, `corrupt_data` | Test read failure handling |
| `fs::write_file` | `io_error`, `disk_full`, `permission_denied`, `partial_write` | Test write failure handling |

### Interpreter (`interpreter/mod.rs`)

| Fail Point | Actions | Security Test Purpose |
|------------|---------|----------------------|
| `interp::execute_command` | `panic`, `error`, `exit_nonzero` | Test command execution failure handling |

## Usage

In tests: `fail::cfg("limits::tick_command", "return(skip_increment)")` before,
`fail::cfg("name", "off")` after. Action syntax (return/panic/sleep/pause,
probability `10%`, count `5*`): see [fail crate docs](https://docs.rs/fail).

## Security Test Categories

1. **Resource limit bypass**: counter corruption, check skipping, overflow/underflow
2. **Filesystem failure**: I/O errors, permission denied, disk full, partial writes, data corruption
3. **Interpreter failure**: execution errors, panic recovery, unexpected exit codes

## Adding New Fail Points

Add `fail_point!("module::function", |action| ...)` under
`#[cfg(feature = "failpoints")]` at the critical location, document it in this
spec, and add tests in `tests/security_failpoint_tests.rs`.

## Best Practices

1. **Always clean up**: Call `fail::cfg("name", "off")` after tests.
2. **Single-threaded tests**: Use `--test-threads=1` for fail point tests due to global state.
3. **Document actions**: List all supported actions in code comments and this spec.
4. **Test both paths**: Test that fail points affect behavior AND that normal operation works without them.

## JavaScript Security Tests

The JavaScript/TypeScript bindings have a dedicated security test suite at
`crates/bashkit-js/__test__/security.spec.ts` (run: `cd crates/bashkit-js &&
pnpm test`). White-box and black-box scenarios across 18 categories:

1. Resource limit enforcement (TM-DOS)
2. Output truncation (TM-DOS)
3. Sandbox escape prevention (TM-ESC)
4. VFS security — path traversal, file count, nesting, filename limits (TM-DOS, TM-INJ)
5. Instance isolation (TM-ISO)
6. Error message safety (TM-INT)
7. TypeScript wrapper injection prevention (TM-INJ)
8. Adversarial script inputs — null bytes, deep nesting, expansion bombs
9. Unicode & encoding attacks (TM-UNI)
10. Injection via constructor options (TM-INJ)
11. Concurrency & cancellation (TM-DOS)
12. Async API security
13. BashTool metadata safety
14. Bash feature abuse — traps, special variables, /dev/tcp
15. Mounted files security
16. Rapid instance creation/destruction
17. Edge case inputs
18. Async factory security

## Related Files

- `crates/bashkit/tests/security_failpoint_tests.rs` - Fail-point security tests
- `crates/bashkit/tests/threat_model_tests.rs` - Threat model tests (51 tests)
- `crates/bashkit/tests/builtin_error_security_tests.rs` - Builtin error security tests (39 tests, includes TM-INT-003)
- `crates/bashkit-js/__test__/security.spec.ts` - JavaScript security tests (90+ tests, 18 categories)
- `crates/bashkit/src/limits.rs` - Resource limit fail points
- `crates/bashkit/src/fs/memory.rs` - Filesystem fail points
- `crates/bashkit/src/interpreter/mod.rs` - Interpreter fail points, panic catching
- `crates/bashkit/src/builtins/system.rs` - Hardcoded system builtins
- `specs/threat-model.md` - Threat model specification
