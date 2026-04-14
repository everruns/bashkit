# 019 — Generic Credential Injection

> Transparent per-host credential injection for outbound HTTP requests, without exposing secrets to sandboxed scripts.

## Problem

AI agents generate scripts that call external APIs (`curl https://api.github.com/...`). Today, the only way to authenticate these requests is to pass secrets as environment variables — but the script can read and exfiltrate them. The industry has converged on **outbound proxy credential injection** as the solution: a trusted layer between the sandbox and the network injects credentials per-host, so the agent never sees the raw secret.

Bashkit already controls the HTTP client in-process via `HttpClient` and the `before_http` hook (landed in #1255). We don't need external proxy infrastructure — we can do credential injection at the `HttpClient` layer with the same security guarantees.

## Design Decisions

1. **Two modes** — *injection* (script has no knowledge of credentials) and *placeholder* (script uses opaque placeholder strings that get replaced on the wire). Both are common in the industry; both have valid use cases.

2. **Built on `before_http` hooks** — `CredentialPolicy` internally registers a `before_http` interceptor. No new hook types or interception points needed.

3. **Header-only for v1** — Credentials are injected/replaced only in HTTP headers. No URL query parameter or request body mutation (reduces attack surface).

4. **Overwrite semantics** — Injected headers **replace** any existing headers with the same name set by the script. This follows Vercel's approach and prevents the agent from spoofing `Authorization` headers.

5. **Non-blocking** — Credential injection failures (missing placeholder, callback error) do not block the request. The request is sent without credentials. Follows bot-auth precedent (TM-AVAIL-001).

6. **Scoped to allowlist patterns** — Credentials use the same `scheme+host+port+path-prefix` matching as `NetworkAllowlist`. No wildcards, no subdomain matching. Credentials only go to pre-approved destinations.

7. **Redacted in traces** — Injected credential values are never logged. Placeholder tokens are logged as `[CREDENTIAL_PLACEHOLDER]`.

8. **No feature gate** — Available whenever `http_client` feature is enabled. No additional dependencies.

## Architecture

```
BashBuilder::credential(pattern, Credential::bearer(token))
    │
    ▼
CredentialPolicy { rules: Vec<CredentialRule> }
    │
    ▼  (internally registers a before_http hook)
before_http interceptor
    │
    ▼  (on every request, after allowlist + SSRF check)
Match URL against rules → inject/replace headers
```

Request pipeline (unchanged from #1255, credential injection slots into step 3):

```
1. Allowlist check              ← security gate
2. Private IP / SSRF check      ← SSRF protection
3. before_http hooks            ← credential injection lives here
4. Bot-auth signing             ← Ed25519 headers
5. Custom HttpHandler OR reqwest
6. after_http hooks             ← observational
```

## Modes

### Mode 1: Injection

The script has no knowledge of credentials. It makes plain requests; the `before_http` hook adds authentication headers automatically.

```rust
let bash = Bash::builder()
    .network(NetworkAllowlist::new()
        .allow("https://api.github.com"))
    .credential("https://api.github.com",
        Credential::bearer("ghp_xxxx"))
    .build();
```

Script inside sandbox:
```bash
curl -s https://api.github.com/repos/foo/bar
# → Authorization: Bearer ghp_xxxx added transparently
```

### Mode 2: Placeholder

The script sees an opaque placeholder string in an env var. It uses the placeholder like a real credential. The `before_http` hook finds the placeholder in outbound headers and replaces it with the real value.

```rust
let bash = Bash::builder()
    .network(NetworkAllowlist::new()
        .allow("https://api.openai.com"))
    .credential_placeholder("OPENAI_API_KEY",
        "https://api.openai.com",
        Credential::bearer(real_key))
    .build();
```

Script inside sandbox:
```bash
# $OPENAI_API_KEY contains "bk_placeholder_a8f3c9e1..."
curl -H "Authorization: Bearer $OPENAI_API_KEY" \
     https://api.openai.com/v1/chat/completions -d '{...}'
# → placeholder replaced with real Bearer token in the header
```

The placeholder is a random hex string (`bk_placeholder_<32 random hex chars>`). It is:
- **Not sensitive** — cannot be reversed to the real credential
- **Useless outside bashkit** — only replaced for approved hosts
- **SDK-compatible** — looks like a non-empty string, passes most client-side validation

## API

### Credential enum

```rust
/// A credential to inject into outbound HTTP requests.
pub enum Credential {
    /// Inject `Authorization: Bearer <token>`.
    Bearer(String),
    /// Inject a custom header.
    Header { name: String, value: String },
    /// Inject multiple headers.
    Headers(Vec<(String, String)>),
}
```

### BashBuilder methods

```rust
impl BashBuilder {
    /// Inject credentials for requests matching the given URL pattern.
    ///
    /// The pattern uses the same matching as NetworkAllowlist
    /// (scheme + host + port + path prefix).
    /// Injected headers overwrite existing headers with the same name.
    pub fn credential(self, pattern: &str, credential: Credential) -> Self;

    /// Inject credentials with a placeholder env var visible to scripts.
    ///
    /// Sets env var `name` to an opaque placeholder string.
    /// When a request to `pattern` contains the placeholder in any header
    /// value, it is replaced with the real credential value.
    pub fn credential_placeholder(
        self,
        env_name: &str,
        pattern: &str,
        credential: Credential,
    ) -> Self;
}
```

### CredentialPolicy (internal)

```rust
/// Internal type that manages credential injection rules.
/// Built by BashBuilder, converted to a before_http hook at build time.
pub(crate) struct CredentialPolicy {
    rules: Vec<CredentialRule>,
}

struct CredentialRule {
    pattern: String,
    credential: Credential,
    /// For placeholder mode: the placeholder string to find-and-replace
    placeholder: Option<String>,
}
```

## Header Overwrite Semantics

When injecting headers, existing headers with the same name are **removed** before injection. This prevents the agent from setting `Authorization: Basic evil` and having it forwarded alongside the injected `Authorization: Bearer real`.

```
Script sets:    Authorization: Basic attacker-controlled
Policy injects: Authorization: Bearer ghp_xxxx

Result:         Authorization: Bearer ghp_xxxx  (script's header removed)
```

This matches Vercel Sandbox behavior and is the secure default.

## Placeholder Generation

Placeholders are generated at `BashBuilder::build()` time:

```
bk_placeholder_<32 hex chars from random bytes>
```

Example: `bk_placeholder_a8f3c9e1b2d4567890abcdef12345678`

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
