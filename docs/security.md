# Security in Bashkit

Bashkit is a virtual Bash interpreter designed for safe, sandboxed script
execution. Security is a first-class concern — every design decision considers
what an untrusted script could do and how to prevent it.

This article gives a high-level overview. For the full threat model with
individual threat IDs and mitigation status, see the
[rustdoc threat model guide](https://docs.rs/bashkit/latest/bashkit/threat_model/index.html).

## Core security boundaries

| Boundary | What it does |
|----------|-------------|
| **Virtual filesystem** | Scripts run against an in-memory VFS. No real filesystem access by default. Path traversal (`../../../etc/passwd`) is normalised away. Symlinks are stored but never followed. |
| **No process execution** | `exec` is excluded entirely. `bash -c` re-invokes the virtual interpreter instead of spawning a real process. Background jobs (`&`) parse but run synchronously. |
| **Network allowlist** | HTTP/HTTPS only, pre-validated against an explicit host allowlist. No DNS resolution, no auto-redirect, no auto-decompression. |
| **Resource limits** | Configurable caps on commands, loop iterations, recursion depth, AST depth, timeouts, and parser operations prevent denial-of-service from malicious scripts. |
| **Filesystem limits** | Total bytes, per-file size, file count, path depth, and filename length are all capped to prevent storage exhaustion (zip bombs, tar bombs, recursive copies). |

## Threat model

Bashkit maintains a living threat model in [`specs/006-threat-model.md`](../specs/006-threat-model.md)
with stable threat IDs across these categories:

| Category | ID prefix | Examples |
|----------|-----------|----------|
| Denial of Service | `TM-DOS` | Resource exhaustion, infinite loops, parser bombs |
| Sandbox Escape | `TM-ESC` | Path traversal, real FS access, privilege escalation |
| Information Disclosure | `TM-INF` | Secret leakage, host info exposure, data exfiltration |
| Injection | `TM-INJ` | Command injection, variable namespace pollution |
| Network | `TM-NET` | DNS rebinding, allowlist bypass, response flooding |
| Multi-Tenant Isolation | `TM-ISO` | Cross-tenant data leaks |
| Internal Errors | `TM-INT` | Panics, error message information leaks |
| Git | `TM-GIT` | Repo access control, remote URL injection |
| Logging | `TM-LOG` | Sensitive data in logs, log injection |
| Python Sandbox | `TM-PY` | Monty resource limits, VFS bridge escapes |
| Unicode | `TM-UNI` | Byte-boundary panics, homoglyph attacks |

The full threat model — including mitigation status for each threat — is
published in the rustdoc:
[**bashkit::threat_model**](https://docs.rs/bashkit/latest/bashkit/threat_model/index.html).

## POSIX deviations for security

Bashkit intentionally deviates from POSIX where compliance would compromise
the sandbox. Key exclusions:

- **`exec`** — would break sandbox containment (`TM-ESC-005`)
- **`trap`** — conflicts with the stateless execution model
- **Real process spawning** — all subprocess commands stay within the virtual interpreter (`TM-ESC-015`)

These decisions are documented in [`specs/009-implementation-status.md`](../specs/009-implementation-status.md).

## Security testing

Bashkit uses multiple layers of security testing:

**Threat model tests** — 185 tests in `threat_model_tests.rs` that directly
validate mitigations against documented threat IDs. Each test maps to a specific
`TM-*` threat.

**Fail-point injection** — A framework defined in [`specs/005-security-testing.md`](../specs/005-security-testing.md)
that injects failures at specific points to verify the interpreter handles them
safely. 14+ tests in `security_failpoint_tests.rs`.

**Network security tests** — 53 tests covering allowlist enforcement, URL
validation, timeout behaviour, and response limits.

**Error handling tests** — 39 tests verifying that builtins wrapped with
`catch_unwind` never leak panic messages, stack traces, or memory addresses.

**Logging security tests** — 26 tests confirming that sensitive data (passwords,
tokens, API keys, JWTs) is redacted in logs and that log injection is prevented.

**Fuzz testing** — Parser and lexer fuzzing to catch panics and unexpected
behaviour on malformed input.

**Differential tests** — Compare Bashkit output against real Bash to ensure
behaviour parity where expected, and confirm intentional divergences where
security requires it.

## Panic safety

All builtin commands are wrapped with `catch_unwind`. If a builtin panics, the
error is caught and converted to a sanitised error message — no stack traces, no
memory addresses, no real filesystem paths leak to the caller (`TM-INT-001`,
`TM-INT-002`).

## Reporting security issues

**Do not open a public GitHub issue for security vulnerabilities.**

Email: **security@everruns.com**

Please include a description of the vulnerability, steps to reproduce, and
potential impact. We acknowledge reports within 48 hours, provide an initial
assessment within 7 days, and target 30-day resolution for critical issues.

See [`SECURITY.md`](../SECURITY.md) for the full policy.

We appreciate responsible disclosure and acknowledge researchers who report
valid vulnerabilities (with permission).
