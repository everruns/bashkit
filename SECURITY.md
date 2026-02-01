# Security Policy

## Reporting a Vulnerability

If you discover a security vulnerability in BashKit, please report it responsibly.

**Do not open a public GitHub issue for security vulnerabilities.**

Instead, please email security issues to: **security@everruns.com**

Include:
- Description of the vulnerability
- Steps to reproduce
- Potential impact
- Any suggested fixes (optional)

## Response Timeline

- **Acknowledgment**: Within 48 hours
- **Initial assessment**: Within 7 days
- **Resolution target**: Within 30 days for critical issues

## Scope

This security policy applies to:
- The `bashkit` crate
- The `bashkit-cli` crate
- Official examples and documentation

## Security Model

BashKit is designed as a sandboxed interpreter. Key security boundaries:

| Boundary | Protection |
|----------|------------|
| Filesystem | Virtual filesystem by default, no real FS access |
| Network | Allowlist-based HTTP access control |
| Resources | Configurable limits on commands, loops, recursion |
| Commands | No shell escape, no external process execution |

### Known Limitations

See [KNOWN_LIMITATIONS.md](KNOWN_LIMITATIONS.md) for documented gaps and edge cases.

## Supported Versions

| Version | Supported |
|---------|-----------|
| 0.1.x   | Yes       |

## Acknowledgments

We appreciate responsible disclosure and will acknowledge security researchers who report valid vulnerabilities (with permission).
