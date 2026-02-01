# Security Testing with Fail Points

## Overview

BashKit uses [fail-rs](https://github.com/tikv/fail-rs) for fault injection security testing. This enables systematic testing of error handling paths and resource limit enforcement under failure conditions.

## Enabling Fail Points

Fail points are disabled by default and have zero runtime overhead when disabled.

```bash
# Run security tests
cargo test --features failpoints security_

# Run with single thread to avoid race conditions
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

## Using Fail Points

### In Tests

```rust
#[tokio::test]
async fn test_resource_limit_bypass() {
    // Configure fail point
    fail::cfg("limits::tick_command", "return(skip_increment)").unwrap();

    // Run code that should be affected
    let result = run_script("echo hello").await;

    // Always clean up
    fail::cfg("limits::tick_command", "off").unwrap();

    // Assert expected behavior
    assert!(/* ... */);
}
```

### Fail Point Actions

From fail-rs documentation:

| Action | Syntax | Description |
|--------|--------|-------------|
| `return` | `return(value)` | Return early with value |
| `panic` | `panic(msg)` | Panic with message |
| `print` | `print(msg)` | Print message |
| `sleep` | `sleep(ms)` | Sleep for milliseconds |
| `pause` | `pause` | Pause until unpaused |
| `yield` | `yield` | Yield to scheduler |

### Probability and Count

```bash
# 10% probability
FAILPOINTS="limits::tick_command=10%return(skip_increment)"

# First 5 times only
FAILPOINTS="limits::tick_command=5*return(skip_increment)"

# Combined: 10% probability, max 3 times
FAILPOINTS="limits::tick_command=3*10%return(skip_increment)"
```

## Security Test Categories

### 1. Resource Limit Bypass Tests

Test that resource limits cannot be bypassed through:
- Counter corruption
- Check skipping
- Counter overflow/underflow

### 2. Filesystem Failure Tests

Test graceful handling of:
- I/O errors
- Permission denied
- Disk full
- Partial writes
- Data corruption

### 3. Interpreter Failure Tests

Test behavior under:
- Execution errors
- Panic recovery (where applicable)
- Unexpected exit codes

## Adding New Fail Points

1. Import the macro:
```rust
#[cfg(feature = "failpoints")]
use fail::fail_point;
```

2. Add fail point at critical location:
```rust
#[cfg(feature = "failpoints")]
fail_point!("module::function", |action| {
    match action.as_deref() {
        Some("failure_mode") => {
            return Err(/* error */);
        }
        _ => {}
    }
    Ok(()) // Default: continue normal execution
});
```

3. Document in this spec.

4. Add tests in `tests/security_failpoint_tests.rs`.

## Best Practices

1. **Always clean up**: Call `fail::cfg("name", "off")` after tests.
2. **Single-threaded tests**: Use `--test-threads=1` for fail point tests due to global state.
3. **Document actions**: List all supported actions in code comments and this spec.
4. **Test both paths**: Test that fail points affect behavior AND that normal operation works without them.

## Related Files

- `crates/bashkit/tests/security_failpoint_tests.rs` - Security test suite
- `crates/bashkit/src/limits.rs` - Resource limit fail points
- `crates/bashkit/src/fs/memory.rs` - Filesystem fail points
- `crates/bashkit/src/interpreter/mod.rs` - Interpreter fail points
