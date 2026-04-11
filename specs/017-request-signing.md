# 017 — Transparent Request Signing (bot-auth)

> Ed25519 request signing for all outbound HTTP requests per RFC 9421 / web-bot-auth profile.

## Problem

The [toolkit library contract](https://github.com/everruns/everruns/blob/main/specs/toolkit-library-contract.md) section 9 requires HTTP-capable kits to support Ed25519 request signing. bashkit has curl, wget, and http builtins that make outbound HTTP requests. Target servers need a way to verify bot identity cryptographically.

## Design Decisions

1. **Transparent** — signing happens inside `HttpClient`, before every outbound request. No CLI flags, no script changes. Scripts using `curl -s https://api.example.com` get signed requests automatically.

2. **Feature-gated** — `bot-auth` cargo feature. When disabled, zero crypto dependencies compiled in. Implies `http_client`.

3. **Non-blocking** — signing failures (clock errors, key issues) never block the request. The request is sent unsigned. This preserves tool availability.

4. **Follows fetchkit** — same `BotAuthConfig` shape, same signing algorithm, same header format. Reference: `everruns/fetchkit/crates/fetchkit/src/bot_auth.rs`.

## Architecture

```
BashBuilder::bot_auth(config)
    │
    ▼
HttpClient::set_bot_auth(config)
    │
    ▼  (on every request, after allowlist check)
BotAuthConfig::sign_request(authority)
    │
    ▼
Signature + Signature-Input + Signature-Agent headers
```

Signing happens in `HttpClient` at the same layer as the allowlist check. **All** outbound HTTP paths are covered:

| Path | Signed | How |
|------|--------|-----|
| `HttpClient::request_with_headers` (default reqwest) | Yes | `bot_auth_headers()` injected before `request.send()` |
| `HttpClient::request_with_timeouts` (per-request timeout) | Yes | Same `bot_auth_headers()` injection |
| Custom `HttpHandler` | Yes | Signing headers merged into the handler's `headers` slice |
| Redirects (manual follow in curl/wget) | Yes | Each redirect is a new `HttpClient` request, re-signed with the new authority |

Every HTTP builtin — `curl`, `wget`, `http` — goes through `HttpClient`, so signing is guaranteed for all outbound requests when configured. No builtin can bypass signing.

## API

### Builder

```rust
use bashkit::{Bash, NetworkAllowlist, BotAuthConfig};

let bash = Bash::builder()
    .network(NetworkAllowlist::new().allow("https://api.example.com"))
    .bot_auth(BotAuthConfig::from_seed([42u8; 32])
        .with_agent_fqdn("bot.example.com")
        .with_validity_secs(300))
    .build();
```

### BotAuthConfig

```rust
pub struct BotAuthConfig {
    seed: [u8; 32],               // Ed25519 seed
    agent_fqdn: Option<String>,  // Signature-Agent header
    validity_secs: u64,          // default: 300
}

impl BotAuthConfig {
    fn from_seed(seed: [u8; 32]) -> Self;
    fn from_base64_seed(encoded: &str) -> Result<Self, BotAuthError>;
    fn with_agent_fqdn(self, fqdn: impl Into<String>) -> Self;
    fn with_validity_secs(self, secs: u64) -> Self;
    fn keyid(&self) -> String;  // JWK Thumbprint
}
```

### Public Key Derivation

```rust
pub fn derive_bot_auth_public_key(seed: &str) -> Result<BotAuthPublicKey, BotAuthError>;

pub struct BotAuthPublicKey {
    pub key_id: String,              // JWK Thumbprint (RFC 7638)
    pub jwk: serde_json::Value,      // Full JWK (OKP/Ed25519)
}
```

Consumer uses this to serve the well-known key directory endpoint.

## Signing Format

Per RFC 9421 with web-bot-auth tag:

- **Covered components**: `@authority` (+ `signature-agent` when FQDN set)
- **Algorithm**: Ed25519 (`alg="ed25519"`)
- **Key identity**: JWK Thumbprint (RFC 7638) as `keyid`
- **Tag**: `"web-bot-auth"`
- **Nonce**: 32 random bytes, base64url
- **Timestamps**: `created` (now), `expires` (now + validity_secs)

### Headers Added

| Header | Value |
|--------|-------|
| `Signature` | `sig=:<base64url-encoded-signature>:` |
| `Signature-Input` | `sig=("@authority");created=...;expires=...;keyid="...";alg="ed25519";nonce="...";tag="web-bot-auth"` |
| `Signature-Agent` | FQDN (only when `agent_fqdn` is set) |

## Consumer Wiring

```rust
if let Ok(seed) = std::env::var("BOT_AUTH_SIGNING_KEY_SEED") {
    builder = builder.bot_auth(BotAuthConfig::from_base64_seed(&seed)?
        .with_agent_fqdn(std::env::var("BOT_AUTH_AGENT_FQDN").ok().unwrap_or_default())
    );
}
```

## Dependencies

Feature `bot-auth` adds:
- `ed25519-dalek` 2.x (Ed25519 signing)
- `rand` 0.10 (nonce generation)
- `zeroize` 1.x (key material zeroization on drop)
- `sha2` (already a required dep for checksum builtins)

## Files

| File | Purpose |
|------|---------|
| `crates/bashkit/src/network/bot_auth.rs` | BotAuthConfig, signing, key derivation |
| `crates/bashkit/src/network/client.rs` | HttpClient integration (bot_auth_headers) |
| `crates/bashkit/src/network/mod.rs` | Module and re-exports |
| `crates/bashkit/src/lib.rs` | BashBuilder::bot_auth(), public exports |

## Security

- Signing key never leaves `BotAuthConfig` — only the public key is derivable
- `Drop` explicitly calls `zeroize()` on seed bytes before deallocation (TM-CRY-001)
- JWK Thumbprint uses SHA-256 with canonical JSON member ordering (RFC 7638)
- Nonce prevents replay attacks
- Expiry window limits signature validity
- Signing failures are non-blocking (TM-AVAIL-001)

## References

- [RFC 9421 — HTTP Message Signatures](https://www.rfc-editor.org/rfc/rfc9421)
- [draft-meunier-web-bot-auth-architecture](https://datatracker.ietf.org/doc/html/draft-meunier-web-bot-auth-architecture)
- [RFC 7638 — JSON Web Key Thumbprint](https://www.rfc-editor.org/rfc/rfc7638)
- [Toolkit library contract section 9](https://github.com/everruns/everruns/blob/main/specs/toolkit-library-contract.md)
- [fetchkit bot-auth implementation](https://github.com/everruns/fetchkit/blob/main/crates/fetchkit/src/bot_auth.rs)
