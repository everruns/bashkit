# Logging Guide

BashKit provides optional structured logging via the `tracing` crate when the
`logging` feature is enabled. This guide covers configuration, log levels,
security considerations, and integration with your application.

**See also:**
- [Threat Model](./threat-model.md) - Security threats including TM-LOG-*
- [API Documentation](https://docs.rs/bashkit) - Full API reference

## Enabling Logging

Add the `logging` feature to your `Cargo.toml`:

```toml
[dependencies]
bashkit = { version = "0.1", features = ["logging"] }
tracing-subscriber = "0.3"  # or your preferred subscriber
```

## Log Levels

BashKit emits logs at five levels:

| Level | Target | When Used |
|-------|--------|-----------|
| **ERROR** | `bashkit::session` | Unrecoverable failures, script errors, security violations |
| **WARN** | `bashkit::parser` | Parse errors, recoverable issues |
| **INFO** | `bashkit::session` | Session start/end, execution completion |
| **DEBUG** | `bashkit::parser`, `bashkit::interpreter` | Parsing, command execution flow |
| **TRACE** | `bashkit::interpreter` | Detailed internal state (not currently emitted) |

### Example Log Output

```text
INFO bashkit::session: Starting script execution script="[script: 3 lines, 45 bytes]"
DEBUG bashkit::parser: Parsing script input_len=45 max_ast_depth=100 max_operations=100000
DEBUG bashkit::parser: Parse completed successfully
DEBUG bashkit::interpreter: Starting interpretation
INFO bashkit::session: Script execution completed exit_code=0 stdout_len=12 stderr_len=0
```

## Configuration

### Basic Setup

```rust,ignore
use bashkit::{Bash, LogConfig};
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

fn main() {
    // Initialize tracing subscriber
    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(EnvFilter::from_default_env())
        .init();

    // BashKit will now emit logs
    let mut bash = Bash::builder()
        .log_config(LogConfig::new())
        .build();
}
```

### Environment Variables

Control log levels via `RUST_LOG`:

```bash
# All BashKit logs at debug level
RUST_LOG=bashkit=debug cargo run

# Only session lifecycle (info level)
RUST_LOG=bashkit::session=info cargo run

# Parser details only
RUST_LOG=bashkit::parser=debug cargo run
```

### Custom Configuration

```rust,ignore
use bashkit::{Bash, LogConfig};

let bash = Bash::builder()
    .log_config(LogConfig::new()
        // Add custom sensitive variable patterns
        .redact_env("MY_INTERNAL_SECRET")
        .redact_env("COMPANY_API_KEY")
        // Limit logged value lengths
        .max_value_length(100))
    .build();
```

## Security (TM-LOG-*)

BashKit's logging is designed with security as a priority. Sensitive data is
redacted by default to prevent accidental exposure in logs.

### What Gets Redacted

1. **Environment Variables** (TM-LOG-001)
   - Variables matching patterns: PASSWORD, SECRET, TOKEN, KEY, AUTH, etc.
   - Custom patterns added via `redact_env()`

2. **Script Content** (TM-LOG-002)
   - By default, only script metadata (lines, bytes) is logged
   - Full content is never logged unless explicitly enabled

3. **URL Credentials** (TM-LOG-003)
   - `https://user:pass@host.com` → `https://[REDACTED]@host.com`

4. **API Keys and Tokens** (TM-LOG-004)
   - JWTs (three-part base64 strings)
   - API keys with common prefixes (`sk-`, `ghp_`, `AKIA`, etc.)
   - High-entropy strings that look like secrets

### Log Injection Prevention (TM-LOG-005, TM-LOG-006)

Script content is sanitized to prevent log injection attacks:

```text
// Attacker tries: "echo hello\n[ERROR] Security breach!"
// Logged as: "echo hello\\n[ERROR] Security breach!"
```

Control characters are filtered, and newlines are escaped.

### Unsafe Options

For debugging in **non-production** environments only:

```rust,ignore
// WARNING: May expose sensitive data
let config = LogConfig::new()
    .unsafe_disable_redaction()  // Disable ALL redaction
    .unsafe_log_scripts();       // Log full script content
```

**Never use these options in production.**

## Integration Examples

### With tokio-console

```rust,ignore
use bashkit::Bash;
use console_subscriber;

fn main() {
    console_subscriber::init();

    let mut bash = Bash::new();
    // Logs will appear in tokio-console
}
```

### With JSON Logging

```rust,ignore
use bashkit::Bash;
use tracing_subscriber::{fmt, prelude::*};

fn main() {
    tracing_subscriber::registry()
        .with(fmt::layer().json())
        .init();

    let mut bash = Bash::new();
    // Logs will be JSON formatted
}
```

### Filtering by Component

```rust,ignore
use bashkit::Bash;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

fn main() {
    let filter = EnvFilter::new("")
        .add_directive("bashkit::session=info".parse().unwrap())
        .add_directive("bashkit::parser=warn".parse().unwrap());

    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(filter)
        .init();

    let mut bash = Bash::new();
}
```

## Log Targets

| Target | Description |
|--------|-------------|
| `bashkit::session` | Script lifecycle (start, complete, error) |
| `bashkit::parser` | Parsing operations and errors |
| `bashkit::interpreter` | Command execution flow |
| `bashkit::config` | Configuration and builder operations |

## Performance Considerations

- Logging is compile-time optional via feature flag
- When disabled (`--no-default-features`), zero overhead
- When enabled but filtered (e.g., `RUST_LOG=error`), minimal overhead
- Redaction operations are optimized to avoid unnecessary allocations

## Troubleshooting

### No Logs Appearing

1. Ensure the `logging` feature is enabled
2. Initialize a tracing subscriber before creating `Bash`
3. Set `RUST_LOG` environment variable appropriately

### Sensitive Data in Logs

1. Check if `unsafe_disable_redaction()` was called
2. Verify custom env var patterns with `redact_env()`
3. Ensure script content logging is disabled (default)

### Too Much Log Output

1. Use `EnvFilter` to limit to specific targets
2. Set higher log level (e.g., `RUST_LOG=bashkit=warn`)
3. Reduce `max_value_length` in `LogConfig`
