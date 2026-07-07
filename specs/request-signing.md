# Transparent Request Signing (bot-auth)

> Ed25519 request signing for all outbound HTTP requests per RFC 9421 / web-bot-auth profile.

## Problem

The [toolkit library contract](https://github.com/everruns/everruns/blob/main/specs/toolkit-library-contract.md) section 9 requires HTTP-capable kits to support Ed25519 request signing. bashkit's curl/wget/http builtins make outbound HTTP requests; target servers need cryptographic bot-identity verification.

## Design Decisions

1. **Transparent** — signing happens inside `HttpClient`, before every outbound request. No CLI flags, no script changes.
2. **Feature-gated** — `bot-auth` cargo feature; implies `http_client`. Disabled = zero crypto deps compiled in.
3. **Non-blocking** — signing failures (clock errors, key issues) never block the request; it is sent unsigned. Preserves tool availability.
4. **Follows fetchkit** — same `BotAuthConfig` shape, signing algorithm, header format. Reference: `everruns/fetchkit/crates/fetchkit/src/bot_auth.rs`.

## Architecture

`BashBuilder::bot_auth(config)` → `HttpClient::set_bot_auth(config)` → on every
request, after allowlist check, `BotAuthConfig::sign_request(method, target_uri)`
adds `Signature` + `Signature-Input` + `Signature-Agent` headers.

Signing happens in `HttpClient` at the same layer as the allowlist check. **All** outbound HTTP paths are covered:

| Path | Signed | How |
|------|--------|-----|
| `HttpClient::request_with_headers` (default reqwest) | Yes | `bot_auth_headers()` injected before `request.send()` |
| `HttpClient::request_with_timeouts` (per-request timeout) | Yes | Same `bot_auth_headers()` injection |
| Custom `HttpTransport` (and deprecated `HttpHandler`) | Yes | Signing headers merged into `HttpTransportRequest.headers` before dispatch (see `specs/http-transport.md`) |
| Redirects (manual follow in curl/wget) | Yes | Each redirect is a new `HttpClient` request, re-signed with the new authority |

Every HTTP builtin — `curl`, `wget`, `http` — goes through `HttpClient`, so no builtin can bypass signing.

## API

`BotAuthConfig` (`from_seed`, `from_base64_seed`, `with_agent_fqdn`,
`with_validity_secs` [default 300], `keyid()` = JWK Thumbprint),
`derive_bot_auth_public_key(seed)` → `BotAuthPublicKey { key_id, jwk }`,
`BashBuilder::bot_auth(config)`: see `crates/bashkit/src/network/bot_auth.rs` /
rustdoc. Consumers use the derived public key to serve the well-known key
directory endpoint; typical wiring reads seed + agent FQDN from env vars.

## Signing Format

Per RFC 9421 with web-bot-auth tag:

- **Covered components**: `@method`, `@target-uri` (+ `signature-agent` when FQDN set)
- **Algorithm**: Ed25519 (`alg="ed25519"`)
- **Key identity**: JWK Thumbprint (RFC 7638) as `keyid`
- **Tag**: `"web-bot-auth"`
- **Nonce**: 32 random bytes, base64url
- **Timestamps**: `created` (now), `expires` (now + validity_secs)

### Headers Added

| Header | Value |
|--------|-------|
| `Signature` | `sig=:<base64url-encoded-signature>:` |
| `Signature-Input` | `sig=("@method" "@target-uri");created=...;expires=...;keyid="...";alg="ed25519";nonce="...";tag="web-bot-auth"` |
| `Signature-Agent` | FQDN (only when `agent_fqdn` is set) |

## Dependencies

Feature `bot-auth` adds: `ed25519-dalek` 2.x, `rand` 0.10 (nonce), `zeroize`
1.x (key zeroization on drop), `sha2` (already required for checksum builtins).

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
