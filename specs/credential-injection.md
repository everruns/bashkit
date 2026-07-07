# Generic Credential Injection

> Transparent per-host credential injection for outbound HTTP requests, without exposing secrets to sandboxed scripts.

## Problem

AI agents generate scripts that call external APIs. Passing secrets as env
vars lets the script read and exfiltrate them. The industry has converged on
**outbound proxy credential injection**: a trusted layer between sandbox and
network injects credentials per-host, so the agent never sees the raw secret.
Bashkit already controls the HTTP client in-process via `HttpClient` and the
`before_http` hook (#1255), so injection happens at the `HttpClient` layer —
no external proxy infrastructure needed.

## Design Decisions

1. **Two modes** — *injection* (script has no knowledge of credentials) and *placeholder* (script uses opaque placeholder strings replaced on the wire). Both are common in the industry; both have valid use cases.
2. **Built on `before_http` hooks** — `CredentialPolicy` internally registers a `before_http` interceptor. No new hook types or interception points.
3. **Header-only for v1** — no URL query parameter or request body mutation (reduces attack surface).
4. **Overwrite semantics** — injected headers **replace** same-name headers set by the script (Vercel's approach; prevents `Authorization` spoofing).
5. **Non-blocking** — injection failures (missing placeholder, callback error) do not block the request; it is sent without credentials. Follows bot-auth precedent (TM-AVAIL-001).
6. **Scoped to allowlist patterns** — same `scheme+host+port+path-prefix` matching as `NetworkAllowlist`. No wildcards, no subdomain matching. Credentials only go to pre-approved destinations.
7. **Redacted in traces** — injected credential values never logged; placeholder tokens logged as `[CREDENTIAL_PLACEHOLDER]`.
8. **No feature gate** — available whenever `http_client` is enabled. No additional dependencies.

## Architecture

`BashBuilder::credential(pattern, cred)` builds a `CredentialPolicy` that
registers a `before_http` hook; on every request (after allowlist + SSRF
check) the URL is matched against rules and headers injected/replaced.

Request pipeline (unchanged from #1255; injection is step 3):

```
1. Allowlist check              ← security gate
2. Private IP / SSRF check      ← SSRF protection
3. before_http hooks            ← credential injection lives here
4. Bot-auth signing             ← Ed25519 headers
5. Custom HttpTransport OR reqwest (see specs/http-transport.md)
6. after_http hooks             ← observational
```

## Modes

### Mode 1: Injection

Script has no knowledge of credentials; the hook adds auth headers
automatically.

```rust
.credential("https://api.github.com", Credential::bearer("ghp_xxxx"))
```

Sandboxed `curl -s https://api.github.com/...` gets
`Authorization: Bearer ghp_xxxx` transparently.

### Mode 2: Placeholder

Script sees an opaque placeholder string in an env var and uses it like a real
credential; the hook finds the placeholder in outbound headers and replaces it
with the real value.

```rust
.credential_placeholder("OPENAI_API_KEY", "https://api.openai.com",
    Credential::bearer(real_key))
```

Sandboxed script uses `$OPENAI_API_KEY` (contains `bk_placeholder_...`) in an
`Authorization` header; the placeholder is replaced on the wire.

The placeholder is:
- **Not sensitive** — cannot be reversed to the real credential
- **Useless outside bashkit** — only replaced for approved hosts
- **SDK-compatible** — looks like a non-empty string, passes most client-side validation

## API

`Credential` enum (`Bearer`, `Header`, `Headers`),
`BashBuilder::credential(pattern, credential)`,
`BashBuilder::credential_placeholder(env_name, pattern, credential)`, and the
internal `CredentialPolicy`/`CredentialRule`: see
`crates/bashkit/src/credential.rs` / rustdoc.

## Header Overwrite Semantics

Existing same-name headers are **removed** before injection, so the agent
cannot have `Authorization: Basic evil` forwarded alongside the injected
`Authorization: Bearer real`. Matches Vercel Sandbox behavior; secure default.

## Placeholder Generation

Generated at `BashBuilder::build()` time:
`bk_placeholder_<32 hex chars from random bytes>`.

Properties:
- 128 bits of randomness — collision-resistant across sessions
- Prefix `bk_placeholder_` — recognizable for debugging but not a real credential format
- Passes most SDK non-empty checks
- Not a valid JWT, API key, or Bearer token format — reduces echo attack risk

## Security

| Threat | Mitigation |
|--------|-----------|
| Script reads env var to get real secret | Injection mode: no env var. Placeholder mode: env var contains random placeholder, not real secret |
| Script exfiltrates placeholder to unapproved host | Allowlist blocks unapproved hosts. Placeholder only replaced for matching patterns |
| Script sets competing Authorization header | Overwrite semantics: injected header replaces script's header |
| Credential appears in error messages | Injected values redacted in all error paths (extend TM-INF-015) |
| Credential appears in traces | Trace output shows `[CREDENTIAL]` instead of real values |
| Echo attack: approved host reflects Authorization header in response body | Accepted risk for v1. Mitigation: limit approved hosts to trusted APIs. Future: `after_http` response scrubbing |
| Placeholder format recognized by attacker | Placeholder reveals credential *exists*, not its value. Acceptable metadata leakage |
| Client-side token validation rejects placeholder | Placeholder is 48+ chars of hex — passes most non-empty/length checks. Known limitation with strict format validators (e.g., GitHub Copilot CLI) |

## Files

| File | Purpose |
|------|---------|
| `crates/bashkit/src/credential.rs` | `Credential`, `CredentialPolicy`, `CredentialRule` |
| `crates/bashkit/src/lib.rs` | `BashBuilder::credential()`, `BashBuilder::credential_placeholder()`, public exports |
| `crates/bashkit/docs/credential-injection.md` | Rustdoc guide |
| `crates/bashkit/tests/credential_injection_tests.rs` | Integration tests |

## Industry References

| Platform | Pattern | Agent sees secret? |
|----------|---------|-------------------|
| Cloudflare Sandboxes | `outboundByHost` proxy injection | No |
| Vercel Sandbox | Firewall-layer header overwrite | No |
| Deno Sandbox | Placeholder env var + proxy replacement | No (placeholder only) |
| E2B (proposed) | "Gondolin" placeholder + TLS MITM | No (placeholder only) |
| NVIDIA OpenShell | `openshell:resolve:env:*` placeholder | No (placeholder only) |
| nono.sh | Phantom token + localhost proxy | No |
| Bashkit (this spec) | `before_http` hook injection + placeholder | No |
